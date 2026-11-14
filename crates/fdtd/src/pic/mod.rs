//! Particle-in-cell on the Yee grid (docs/adr/0012-particles.md, stage B): macro-particles
//! pushed by the grid's fields, their current fed back into Ampère's law.
//!
//! **The loop** (C. K. Birdsall, A. B. Langdon, *Plasma Physics via Computer Simulation*, 1991,
//! §15-2 and §15-4). E and the positions at whole steps, the momenta u = γv at the half steps
//! between, B kept at whole steps by two half updates either side of the push. A step from n to
//! n + 1: the walls emit; each particle gathers Eⁿ and Bⁿ at xⁿ, its u is pushed to n + ½
//! (Boris or Vay, [`emcraft_particles::push`]), it moves to xⁿ⁺¹ and deposits the current
//! Jⁿ⁺½ of its move ([`deposit`]); then B to n + ½, E to n + 1 by
//! ε₀ ∂E/∂t = ∇ × B/μ₀ − J, and B to n + 1. Fields are gathered from their own staggered
//! lattices with the particles' first-order shape.
//!
//! **Charge.** Esirkepov's deposition conserves charge exactly, so Gauss's law holds at every
//! node to rounding as long as it held at the start: start neutral with no field, or with a
//! field of no divergence (a uniform one between walls), and emit from walls (a wall's node is
//! not checked: its charge is the wall's own). A uniform background (immobile ions) may
//! neutralise the particles.
//!
//! **Walls.** A particle reaching a conducting wall is absorbed there, its current deposited up
//! to the wall; one crossing a periodic face comes back through the other side. The voltage
//! across the walls of an axis can be held, as by an ideal source in an outer circuit, when the
//! other two axes are periodic: the gap's mean current is returned by a uniform current, whose
//! field — a uniform E along the axis — has no curl or divergence.
//!
//! **Determinism.** Particles are pushed in a fixed order on one thread and emitted at
//! quasi-random places of a fixed sequence, so a run gives the same bits every time.

use emcraft_model::units::{C0, EPS0, MU0};
use emcraft_particles::{Pusher, Species, V3, push, velocity};

use crate::{FdtdError, Result, invalid};

mod absorb;
mod deposit;
mod emit;
mod filter;
mod grid;
mod media;
#[cfg(test)]
mod tests;

pub use absorb::MAX_LAYER;
pub use emit::{Bunch, Emission, Emitter, MAX_PER_NODE, Pulse};
pub use grid::{Axis, Grid, Wall};
pub use media::Medium;

/// The most macro-particles a run holds.
pub const MAX_PARTICLES: usize = 20_000_000;

/// A macro-particle: its position (m), its momentum u = γv (m/s) and how many particles it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub x: V3,
    pub u: V3,
    pub w: f64,
}

/// Particles of one species.
#[derive(Clone, Debug, PartialEq)]
pub struct Population {
    pub species: Species,
    pub particles: Vec<Particle>,
}

impl Population {
    /// None yet (a population only emitted).
    pub fn new(species: Species) -> Population {
        Population { species, particles: Vec::new() }
    }

    /// `density` (m⁻³) spread evenly over the grid, `per_cell` macro-particles a cell along
    /// each axis on a regular lattice (a quiet start), each with the momentum `momentum` gives
    /// at its position.
    ///
    /// ```
    /// use emcraft_fdtd::pic::{Axis, Grid, Population, Wall};
    /// use emcraft_particles::ELECTRON;
    /// let a = Axis { cells: 4, h: 1e-3, wall: Wall::Periodic };
    /// let g = Grid::new([a, a, a])?;
    /// let p = Population::uniform(&g, ELECTRON, 1e16, [2, 1, 1], |_| [0.0; 3])?;
    /// assert_eq!(p.particles.len(), 64 * 2);
    /// // The charge of 10¹⁶ m⁻³ in 64 mm³.
    /// assert!((p.charge() / (ELECTRON.charge * 1e16 * 64e-9) - 1.0).abs() < 1e-12);
    /// # Ok::<(), emcraft_fdtd::FdtdError>(())
    /// ```
    pub fn uniform(grid: &Grid, species: Species, density: f64, per_cell: [usize; 3], momentum: impl Fn(V3) -> V3) -> Result<Population> {
        if !(density.is_finite() && density >= 0.0) {
            return Err(invalid("density", "must be a finite number ≥ 0"));
        }
        let count = per_cell.iter().try_fold(grid.cells(), |n, &k| n.checked_mul(k)).unwrap_or(usize::MAX);
        if per_cell.contains(&0) || count > MAX_PARTICLES {
            return Err(invalid("particles", format!("1 to {MAX_PARTICLES} in all, at least one a cell along each axis")));
        }
        let w = density * grid.cell_volume() / per_cell.iter().product::<usize>() as f64;
        let mut particles = Vec::new();
        particles.try_reserve_exact(count).map_err(|_| invalid("particles", format!("no memory for {count}")))?;
        let [nx, ny, nz] = [0, 1, 2].map(|d| grid.axes[d].cells * per_cell[d]);
        let at = |d: usize, i: usize| (i as f64 + 0.5) / per_cell[d] as f64 * grid.axes[d].h;
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    let x = [at(0, i), at(1, j), at(2, k)];
                    particles.push(Particle { x, u: momentum(x), w });
                }
            }
        }
        Ok(Population { species, particles })
    }

    /// The charge it carries (C).
    pub fn charge(&self) -> f64 {
        self.species.charge * self.particles.iter().map(|p| p.w).sum::<f64>()
    }

    /// Its kinetic energy (J).
    pub fn kinetic(&self) -> f64 {
        self.particles.iter().map(|p| p.w * self.species.kinetic(p.u)).sum()
    }
}

/// What a step did: the charge emitted (C), the charge absorbed on each wall (C; by axis, low
/// and high) and on the bodies inside, the particles dropped because their momentum stopped
/// being a number, and the kinetic energy (J) the particles emitted brought in and those
/// absorbed took out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Moved {
    pub emitted: f64,
    pub collected: [[f64; 2]; 3],
    pub metal: f64,
    pub lost: usize,
    pub kinetic_in: f64,
    pub kinetic_out: f64,
}

/// The energy on the grid (J): the field's, its electric part, and the particles'.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Energy {
    pub field: f64,
    pub electric: f64,
    pub kinetic: f64,
}

/// A particle-in-cell run.
pub struct Pic {
    grid: Grid,
    dt: f64,
    pusher: Pusher,
    e: [Vec<f64>; 3],
    b: [Vec<f64>; 3],
    j: [Vec<f64>; 3],
    populations: Vec<Population>,
    background: f64,
    /// The charge the walls and bodies hold (C/m³ on the nodes): what particles brought onto
    /// them, less what left them.
    surface: Vec<f64>,
    /// Binomial passes over the current each step.
    smoothing: usize,
    hold: Option<usize>,
    emitters: Vec<Emitter>,
    bunches: Vec<Bunch>,
    media: Option<media::Media>,
    emitted: u64,
    steps: usize,
    absorber: Option<absorb::Absorber>,
}

/// The rate (1/s) an instability grows at: the slope of ½ ln W over the stretch where the
/// energy `w` (J, at `t`, s) first rises from e⁻¹² to e⁻⁴ of its peak — past the start's
/// transient, before saturation (a field amplitude growing as e^{γt}, its energy as e^{2γt}).
/// None without five records there.
///
/// ```
/// use emcraft_fdtd::pic::growth_rate;
/// let t: Vec<f64> = (0..400).map(|n| n as f64 * 1e-11).collect();
/// // e^{2γt} up to saturation, γ = 2e9/s.
/// let w: Vec<f64> = t.iter().map(|t| (2.0 * 2e9 * t).exp().min(1e12)).collect();
/// assert!((growth_rate(&t, &w).unwrap() / 2e9 - 1.0).abs() < 1e-9);
/// ```
pub fn growth_rate(t: &[f64], w: &[f64]) -> Option<f64> {
    let peak = w.iter().copied().filter(|x| x.is_finite()).fold(0.0, f64::max);
    if !(peak > 0.0) {
        return None;
    }
    let first = |level: f64| w.iter().position(|x| *x >= level);
    let (a, b) = (first(peak * (-12f64).exp())?, first(peak * (-4f64).exp())?);
    let pts: Vec<(f64, f64)> = t.iter().zip(w).take(b + 1).skip(a).filter(|(_, x)| **x > 0.0).map(|(t, x)| (*t, 0.5 * x.ln())).collect();
    if pts.len() < 5 {
        return None;
    }
    let n = pts.len() as f64;
    let (mt, my) = (pts.iter().map(|p| p.0).sum::<f64>() / n, pts.iter().map(|p| p.1).sum::<f64>() / n);
    let var = pts.iter().map(|p| (p.0 - mt).powi(2)).sum::<f64>();
    (var > 0.0).then(|| pts.iter().map(|p| (p.0 - mt) * (p.1 - my)).sum::<f64>() / var)
}

/// `n` zeros, refused when there is no memory for them.
fn zeros(n: usize) -> Result<Vec<f64>> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| invalid("grid", format!("no memory for {n} values")))?;
    v.resize(n, 0.0);
    Ok(v)
}

impl Pic {
    /// A run on `grid` with no field and no particles, stepped by `dt` (s), which must be
    /// within the grid's stability limit, pushed by `pusher`.
    ///
    /// ```
    /// use emcraft_fdtd::pic::{Axis, Grid, Pic, Wall};
    /// use emcraft_particles::Pusher;
    /// let a = Axis { cells: 4, h: 1e-3, wall: Wall::Periodic };
    /// let g = Grid::new([a, a, a])?;
    /// let dt = g.time_step(0.9);
    /// assert!(Pic::new(g, dt, Pusher::Boris).is_ok());
    /// assert!(Pic::new(g, 2.0 * dt, Pusher::Boris).is_err());
    /// # Ok::<(), emcraft_fdtd::FdtdError>(())
    /// ```
    pub fn new(grid: Grid, dt: f64, pusher: Pusher) -> Result<Pic> {
        if !(dt.is_finite() && dt > 0.0 && dt < grid.time_step(1.0)) {
            return Err(invalid("time step", format!("{dt} s: must be > 0 and below the stability limit {} s", grid.time_step(1.0))));
        }
        let e = [0, 1, 2].map(|a| zeros(grid.e(a).len()));
        let b = [0, 1, 2].map(|a| zeros(grid.b(a).len()));
        let j = [0, 1, 2].map(|a| zeros(grid.e(a).len()));
        let [e0, e1, e2] = e;
        let [b0, b1, b2] = b;
        let [j0, j1, j2] = j;
        Ok(Pic {
            grid,
            dt,
            pusher,
            e: [e0?, e1?, e2?],
            b: [b0?, b1?, b2?],
            j: [j0?, j1?, j2?],
            populations: Vec::new(),
            background: 0.0,
            surface: zeros(grid.nodes().len())?,
            smoothing: 0,
            hold: None,
            emitters: Vec::new(),
            bunches: Vec::new(),
            media: None,
            emitted: 0,
            steps: 0,
            absorber: None,
        })
    }

    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    pub fn dt(&self) -> f64 {
        self.dt
    }

    /// The time (s) of the fields and positions.
    pub fn time(&self) -> f64 {
        self.steps as f64 * self.dt
    }

    pub fn steps(&self) -> usize {
        self.steps
    }

    pub fn populations(&self) -> &[Population] {
        &self.populations
    }

    /// How many macro-particles the run holds.
    pub fn particles(&self) -> usize {
        self.populations.iter().map(|p| p.particles.len()).sum()
    }

    /// Adds `population`, its particles checked: finite, inside the box, of a weight ≥ 0, at
    /// most [`MAX_PARTICLES`] in all. Returns its index.
    pub fn add(&mut self, mut population: Population) -> Result<usize> {
        if self.particles().saturating_add(population.particles.len()) > MAX_PARTICLES {
            return Err(invalid("particles", format!("at most {MAX_PARTICLES} in all")));
        }
        let lengths = self.grid.lengths();
        for p in &mut population.particles {
            let finite = p.x.iter().chain(&p.u).all(|v| v.is_finite()) && p.w.is_finite() && p.w >= 0.0;
            if !finite {
                return Err(invalid("particle", format!("{p:?}: not finite, or a weight < 0")));
            }
            for d in 0..3 {
                match self.grid.axes[d].wall {
                    Wall::Periodic => p.x[d] = p.x[d].rem_euclid(lengths[d]),
                    Wall::Conductor if !(0.0..=lengths[d]).contains(&p.x[d]) => {
                        return Err(invalid("particle", format!("{:?} m is outside the box", p.x)));
                    }
                    Wall::Conductor => {}
                }
            }
        }
        self.populations.push(population);
        Ok(self.populations.len() - 1)
    }

    /// A uniform background charge density (C/m³): immobile ions neutralising a plasma.
    pub fn set_background(&mut self, rho: f64) {
        self.background = if rho.is_finite() { rho } else { 0.0 };
    }

    /// The background that makes the box neutral with the particles it holds.
    pub fn neutralise(&mut self) {
        let volume: f64 = self.grid.lengths().iter().product();
        self.set_background(-self.populations.iter().map(Population::charge).sum::<f64>() / volume);
    }

    /// E_a (V/m) set to `f` at each point of its lattice off the walls it is tangential to.
    /// Gauss's law holds afterwards only if `f` has the divergence of the charge.
    pub fn set_e(&mut self, a: usize, f: impl Fn(V3) -> f64) {
        let Some(values) = self.e.get_mut(a) else { return };
        let lattice = self.grid.e(a);
        let metal = self.media.as_ref();
        for p in lattice.points() {
            let id = lattice.at(p);
            if !self.grid.e_on_wall(a, p)
                && !metal.is_some_and(|m| m.edge(a, id))
                && let Some(v) = values.get_mut(id)
            {
                *v = f(self.grid.position(lattice, p));
            }
        }
    }

    /// Conducting bodies inside the box: each cell whose centre `metal` says is in a conductor
    /// (a position in m) becomes metal, E on the edges touching it held at zero from now on.
    /// Returns whether any cell is.
    pub fn set_metal(&mut self, metal: impl Fn(V3) -> bool) -> bool {
        self.set_media(|x| if metal(x) { Medium::Metal } else { Medium::Dielectric(1.0) })
    }

    /// What fills each cell, as `medium` says at its centre (m): metal (E on the edges touching
    /// it held at zero from now on) or a dielectric. Returns whether any cell is other than
    /// vacuum.
    pub fn set_media(&mut self, medium: impl Fn(V3) -> Medium) -> bool {
        let m = media::Media::new(&self.grid, medium);
        if !m.any() {
            self.media = None;
            return false;
        }
        for a in 0..3 {
            for (id, v) in self.e[a].iter_mut().enumerate() {
                if m.edge(a, id) {
                    *v = 0.0;
                }
            }
        }
        self.media = Some(m);
        true
    }

    /// Smooths the current by `passes` binomial passes along each axis each step (1-2-1, Birdsall
    /// & Langdon §8-6), against the numerical Cherenkov radiation of particles faster than the
    /// grid's shortest waves; Gauss's law then holds for the charge smoothed alike. At most 8.
    pub fn smooth(&mut self, passes: usize) {
        self.smoothing = passes.min(filter::MAX_PASSES);
    }

    /// Opens a face: a convolutional perfectly matched layer of `cells` inside the conducting wall
    /// of `axis` at its `high` end or its low one absorbs what reaches it (the module `absorb`'s
    /// docs), the box open there inside the layer.
    pub fn absorb(&mut self, axis: usize, high: bool, cells: usize) -> Result<()> {
        let mut absorber = match self.absorber.take() {
            Some(a) => a,
            None => absorb::Absorber::new(&self.grid)?,
        };
        let done = absorber.layer(&self.grid, axis, high, cells, self.dt);
        self.absorber = Some(absorber);
        done
    }

    /// Adds `bunch`, checked against the grid and the populations.
    pub fn inject(&mut self, bunch: Bunch) -> Result<()> {
        bunch.check(&self.grid, self.populations.len())?;
        self.bunches.push(bunch);
        Ok(())
    }

    /// B_a (T) set to `f` at each point of its lattice.
    pub fn set_b(&mut self, a: usize, f: impl Fn(V3) -> f64) {
        let Some(values) = self.b.get_mut(a) else { return };
        let lattice = self.grid.b(a);
        for p in lattice.points() {
            if let Some(v) = values.get_mut(lattice.at(p)) {
                *v = f(self.grid.position(lattice, p));
            }
        }
    }

    /// Holds the voltage across the walls of `axis` from now on; the other two axes must be
    /// periodic.
    pub fn hold(&mut self, axis: usize) -> Result<()> {
        let closed = |d: usize| self.grid.axes[d].wall == Wall::Conductor;
        if axis > 2 || !closed(axis) || (0..3).any(|d| d != axis && closed(d)) {
            return Err(FdtdError::Unsupported("holding a voltage across walls other than one pair, the other axes periodic".into()));
        }
        self.hold = Some(axis);
        Ok(())
    }

    /// Adds `emitter`, checked against the grid and the populations.
    pub fn emit(&mut self, emitter: Emitter) -> Result<()> {
        emitter.check(&self.grid, self.populations.len())?;
        self.emitters.push(emitter);
        Ok(())
    }

    /// E (V/m) at `x` (m).
    pub fn e_at(&self, x: V3) -> V3 {
        [0, 1, 2].map(|a| self.grid.interpolate(self.grid.e(a), &self.e[a], x))
    }

    /// B (T) at `x` (m).
    pub fn b_at(&self, x: V3) -> V3 {
        [0, 1, 2].map(|a| self.grid.interpolate(self.grid.b(a), &self.b[a], x))
    }

    /// The mean over the box's cross-section of ∫E_a da across `axis` (V): the voltage of the
    /// low wall over the high one's.
    pub fn line_integral(&self, axis: usize) -> f64 {
        let Some(values) = self.e.get(axis) else { return 0.0 };
        let lattice = self.grid.e(axis);
        let across: usize = (0..3).filter(|&d| d != axis).map(|d| lattice.dims[d]).product();
        values.iter().sum::<f64>() * self.grid.axes[axis].h / across.max(1) as f64
    }

    /// The energy on the grid: the field's ½ε₀εrE² + B²/2μ₀ over the cells, the particles'.
    pub fn energy(&self) -> Energy {
        let v = self.grid.cell_volume();
        let eps = |a: usize, id: usize| self.media.as_ref().map_or(1.0, |m| m.eps(a, id));
        let electric: f64 = (0..3).map(|a| self.e[a].iter().enumerate().map(|(id, x)| eps(a, id) * x * x).sum::<f64>()).sum();
        let magnetic: f64 = self.b.iter().flatten().map(|x| x * x).sum();
        Energy {
            field: v * (0.5 * EPS0 * electric + 0.5 / MU0 * magnetic),
            electric: v * 0.5 * EPS0 * electric,
            kinetic: self.populations.iter().map(Population::kinetic).sum(),
        }
    }

    /// ε₀∇·(εr E) − ρ (C/m³) at each node off the walls and the metal: zero where Gauss's law
    /// holds.
    pub fn gauss(&self) -> Vec<f64> {
        let nodes = self.grid.nodes();
        let mut rho: Vec<f64> = self.surface.iter().map(|s| s + self.background).collect();
        for pop in &self.populations {
            for p in &pop.particles {
                deposit::charge(&self.grid, &mut rho, p.x, pop.species.charge * p.w);
            }
        }
        filter::smooth(&self.grid, nodes, &mut rho, self.smoothing);
        let axes = self.grid.axes;
        nodes
            .points()
            .filter(|p| {
                (0..3).all(|d| !axes[d].on_wall(p[d]))
                    && !self.media.as_ref().is_some_and(|m| m.near_node(&self.grid, *p))
                    && !self.absorber.as_ref().is_some_and(|a| a.inside(&self.grid, *p))
            })
            .map(|p| {
                let div: f64 = (0..3)
                    .map(|a| {
                        let lattice = self.grid.e(a);
                        let mut below = p;
                        below[a] = axes[a].clamp(p[a] as isize - 1, false);
                        let d = |q: [usize; 3]| {
                            let id = lattice.at(q);
                            self.media.as_ref().map_or(1.0, |m| m.eps(a, id)) * self.e[a][id]
                        };
                        (d(p) - d(below)) / axes[a].h
                    })
                    .sum();
                EPS0 * div - rho[nodes.at(p)]
            })
            .collect()
    }

    /// One step: emission, the particles pushed and moved, their current, the fields.
    pub fn step(&mut self) -> Result<Moved> {
        let mut moved = Moved::default();
        self.emit_step(&mut moved)?;
        self.j.iter_mut().for_each(|j| j.fill(0.0));
        let Pic { grid, dt, pusher, e, b, j, populations, media, surface, .. } = self;
        for pop in populations.iter_mut() {
            let species = pop.species;
            pop.particles.retain_mut(|p| advance((grid, media.as_ref()), [e, b], (j, surface), (species, *pusher, *dt), p, &mut moved));
        }
        for (a, values) in self.j.iter_mut().enumerate() {
            filter::smooth(&self.grid, self.grid.e(a), values, self.smoothing);
        }
        if let Some(a) = self.hold {
            let mean = self.j[a].iter().sum::<f64>() / self.j[a].len().max(1) as f64;
            self.j[a].iter_mut().for_each(|v| *v -= mean);
        }
        faraday(&self.grid, &self.e, &mut self.b, 0.5 * self.dt, self.absorber.as_mut());
        ampere(&self.grid, &mut self.e, &self.b, &self.j, self.dt, (self.media.as_ref(), self.absorber.as_mut()));
        faraday(&self.grid, &self.e, &mut self.b, 0.5 * self.dt, self.absorber.as_mut());
        self.steps += 1;
        Ok(moved)
    }

    /// The walls' emission and the bunches at the step's start; none into a metal cell.
    fn emit_step(&mut self, moved: &mut Moved) -> Result<()> {
        let t = self.time();
        let made: Vec<(usize, Vec<Particle>)> = self
            .emitters
            .iter()
            .filter_map(|em| {
                let pop = self.populations.get(em.population)?;
                Some((em.population, em.emit(&self.grid, &self.e[em.axis], pop, self.dt, &mut self.emitted)))
            })
            .collect();
        let sent: Vec<(usize, Vec<Particle>)> = self
            .bunches
            .iter()
            .filter_map(|bu| {
                let pop = self.populations.get(bu.population)?;
                Some((bu.population, bu.emit(&self.grid, pop, t, self.dt, &mut self.emitted)))
            })
            .collect();
        for (k, mut new) in made.into_iter().chain(sent) {
            if let Some(m) = &self.media {
                new.retain(|p| !m.at(&self.grid, p.x));
            }
            if self.particles().saturating_add(new.len()) > MAX_PARTICLES {
                return Err(invalid("particles", format!("the emission would hold more than {MAX_PARTICLES}")));
            }
            let Some(pop) = self.populations.get_mut(k) else { continue };
            let species = pop.species;
            moved.emitted += new.iter().map(|p| species.charge * p.w).sum::<f64>();
            moved.kinetic_in += new.iter().map(|p| p.w * species.kinetic(p.u)).sum::<f64>();
            // What leaves a wall leaves its opposite charge there.
            for p in &new {
                deposit::charge(&self.grid, &mut self.surface, p.x, -species.charge * p.w);
            }
            pop.particles.extend(new);
        }
        Ok(())
    }
}

/// Pushes and moves particle `p`, deposits its current; false when it is gone (absorbed on a
/// wall, or no longer a number).
fn advance(
    (grid, metal): (&Grid, Option<&media::Media>),
    [e, b]: [&[Vec<f64>; 3]; 2],
    (j, surface): (&mut [Vec<f64>; 3], &mut Vec<f64>),
    (species, pusher, dt): (Species, Pusher, f64),
    p: &mut Particle,
    moved: &mut Moved,
) -> bool {
    let field = |f: &[Vec<f64>; 3], lattice: fn(&Grid, usize) -> grid::Lattice| [0, 1, 2].map(|a| grid.interpolate(lattice(grid, a), &f[a], p.x));
    let u = push(pusher, species, p.u, field(e, Grid::e), field(b, Grid::b), dt);
    if !u.iter().all(|v| v.is_finite()) {
        moved.lost += 1;
        return false;
    }
    let v = velocity(u);
    let x1 = [0, 1, 2].map(|d| p.x[d] + v[d] * dt);
    let (to, hit) = clip(grid, p.x, x1);
    deposit::current(grid, j, p.x, to, species.charge * p.w, dt);
    if let Some((axis, high)) = hit {
        moved.collected[axis][usize::from(high)] += species.charge * p.w;
        deposit::charge(grid, surface, to, species.charge * p.w);
        moved.kinetic_out += p.w * species.kinetic(u);
        return false;
    }
    if metal.is_some_and(|m| m.at(grid, to)) {
        moved.metal += species.charge * p.w;
        deposit::charge(grid, surface, to, species.charge * p.w);
        moved.kinetic_out += p.w * species.kinetic(u);
        return false;
    }
    for d in 0..3 {
        if grid.axes[d].wall == Wall::Periodic {
            p.x[d] = to[d].rem_euclid(grid.axes[d].length());
        } else {
            p.x[d] = to[d];
        }
    }
    p.u = u;
    true
}

/// The move from `x0` to `x1` cut where it first meets a conducting wall, and that wall (its
/// axis, high or low).
fn clip(grid: &Grid, x0: V3, x1: V3) -> (V3, Option<(usize, bool)>) {
    let mut f = 1.0;
    let mut hit = None;
    for d in 0..3 {
        let a = grid.axes[d];
        if a.wall != Wall::Conductor {
            continue;
        }
        let l = a.length();
        let (g, high) = if x1[d] < 0.0 {
            (x0[d] / (x0[d] - x1[d]), false)
        } else if x1[d] > l {
            ((l - x0[d]) / (x1[d] - x0[d]), true)
        } else {
            continue;
        };
        if g < f {
            (f, hit) = (g.max(0.0), Some((d, high)));
        }
    }
    let mut to = [0, 1, 2].map(|d| x0[d] + f * (x1[d] - x0[d]));
    for d in 0..3 {
        let a = grid.axes[d];
        if a.wall == Wall::Conductor {
            to[d] = to[d].clamp(0.0, a.length());
        }
    }
    if let Some((d, high)) = hit {
        to[d] = if high { grid.axes[d].length() } else { 0.0 };
    }
    (to, hit)
}

/// B advanced by `dt` (s): ∂B/∂t = −∇ × E, on every face (those normal to a wall keep the zero
/// they start with, the tangential E about them being zero).
fn faraday(grid: &Grid, e: &[Vec<f64>; 3], b: &mut [Vec<f64>; 3], dt: f64, mut absorber: Option<&mut absorb::Absorber>) {
    let axes = grid.axes;
    for a in 0..3 {
        let (q, r) = ((a + 1) % 3, (a + 2) % 3);
        let (lattice, eq, er) = (grid.b(a), grid.e(q), grid.e(r));
        for p in lattice.points() {
            // ∂_q E_r − ∂_r E_q, E's nodes p and p + 1 along q and r.
            let (Some(nq), Some(nr)) = (axes[q].wrap(p[q] as isize + 1, true), axes[r].wrap(p[r] as isize + 1, true)) else { continue };
            let (mut pq, mut pr) = (p, p);
            (pq[q], pr[r]) = (nq, nr);
            let (mut dq, mut dr) = ((e[r][er.at(pq)] - e[r][er.at(p)]) / axes[q].h, (e[q][eq.at(pr)] - e[q][eq.at(p)]) / axes[r].h);
            let id = lattice.at(p);
            if let Some(ab) = absorber.as_deref_mut() {
                (dq, dr) = stretched(ab, true, (a, [q, r]), (lattice, p, id), [dq, dr]);
            }
            b[a][id] -= dt * (dq - dr);
        }
    }
}

/// The differences `d` along `axes` at point `p` (index `id`) of component a's lattice (`field_b`:
/// B's) through the absorber's layers: each ∂ + ψ, ψ advanced.
fn stretched(
    ab: &mut absorb::Absorber,
    field_b: bool,
    (a, axes): (usize, [usize; 2]),
    (lattice, p, id): (grid::Lattice, [usize; 3], usize),
    d: [f64; 2],
) -> (f64, f64) {
    let mut out = d;
    for k in 0..2 {
        let c = ab.at(field_b, axes[k], lattice.node[axes[k]], p[axes[k]]);
        if c == crate::yee::Cpml::NONE {
            continue;
        }
        let psi = if field_b { &mut ab.psi_b[a][k] } else { &mut ab.psi_e[a][k] };
        if let Some(v) = psi.get_mut(id) {
            *v = c.psi(*v, d[k]);
            out[k] += *v;
        }
    }
    (out[0], out[1])
}

/// E advanced by `dt` (s): ∂E/∂t = c²∇ × B − J/ε₀, off the walls it is tangential to and the
/// edges metal holds at zero.
fn ampere(
    grid: &Grid,
    e: &mut [Vec<f64>; 3],
    b: &[Vec<f64>; 3],
    j: &[Vec<f64>; 3],
    dt: f64,
    (metal, mut absorber): (Option<&media::Media>, Option<&mut absorb::Absorber>),
) {
    let axes = grid.axes;
    for a in 0..3 {
        let (q, r) = ((a + 1) % 3, (a + 2) % 3);
        let (lattice, bq, br) = (grid.e(a), grid.b(q), grid.b(r));
        for p in lattice.points() {
            if grid.e_on_wall(a, p) {
                continue;
            }
            // ∂_q B_r − ∂_r B_q, B's centres p − ½ and p + ½ along q and r.
            let (Some(mq), Some(mr)) = (axes[q].wrap(p[q] as isize - 1, false), axes[r].wrap(p[r] as isize - 1, false)) else { continue };
            let (mut pq, mut pr) = (p, p);
            (pq[q], pr[r]) = (mq, mr);
            let (mut dq, mut dr) = ((b[r][br.at(p)] - b[r][br.at(pq)]) / axes[q].h, (b[q][bq.at(p)] - b[q][bq.at(pr)]) / axes[r].h);
            let id = lattice.at(p);
            if let Some(ab) = absorber.as_deref_mut() {
                (dq, dr) = stretched(ab, false, (a, [q, r]), (lattice, p, id), [dq, dr]);
            }
            let curl = dq - dr;
            if metal.is_some_and(|m| m.edge(a, id)) {
                continue;
            }
            let eps = metal.map_or(1.0, |m| m.eps(a, id));
            e[a][id] += dt / eps * (C0 * C0 * curl - j[a][id] / EPS0);
        }
    }
}

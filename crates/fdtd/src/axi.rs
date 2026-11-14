//! Particle-in-cell in a body of revolution (docs/adr/0012-particles.md): the fields that do not
//! vary round the axis (m = 0) on an r–z grid, the particles rings about it.
//!
//! **Fields.** Both sets of an m = 0 field: E_r, E_z, B_θ (TM) and E_θ, B_r, B_z (TE, for an
//! axial magnetic field and the particles' turning). On the r–z Yee grid (as
//! C. K. Birdsall, A. B. Langdon, *Plasma Physics via Computer Simulation*, 1991, §14-11, and
//! the codes that model BWOs this way) E_r lies at (r_{i+½}, z_k), E_z at (r_i, z_{k+½}), E_θ at
//! (r_i, z_k), B_r at (r_i, z_{k+½}), B_z at (r_{i+½}, z_k), B_θ at (r_{i+½}, z_{k+½}); the
//! curls are written as circulations over the rings' faces (finite integration), so E_z on the
//! axis takes 4B_θ(r_½)/Δr and B_z next to it r_1 E_θ(r_1) over r_½Δr. ε₀ ∂E/∂t = ∇×B/μ₀ − J,
//! ∂B/∂t = −∇×E, leapfrogged as the Cartesian grid's are.
//!
//! **Rings.** A macro-particle is a ring of charge at (r, z) with momentum (u_r, u_θ, u_z): it
//! is pushed in the plane θ = 0 as a particle at (r, 0, z) (Boris or Vay), moved straight there,
//! and its position and momentum turned back onto that plane (the angle of its move round the
//! axis). Fields are gathered linearly from their lattices, odd about the axis for E_r, E_θ, B_θ
//! and even for B_z.
//!
//! **Charge.** A ring's charge goes to the four nodes about it with linear weights in r and z;
//! a node holds it in its ring of volume 2πr_iΔrΔz (π(Δr/2)²Δz on the axis). Its current is
//! Esirkepov's (Comput. Phys. Commun. 135:144, 2001) in the indices: the change of its weights
//! split into the flux through each node ring's faces, I_r through 2πr_{i+½}Δz and I_z through
//! the ring's area, so that the flux form of Gauss's law, ε₀∮E·dA = Q, holds at every node to
//! rounding. Its azimuthal current goes to E_θ's nodes at its move's middle.
//!
//! **Walls.** A conductor at r = R; along z conducting walls (which emit and absorb) or
//! periodic ends; conducting bodies as staircases of cells (`set_metal`, as the Cartesian
//! grid's); an absorbing layer (CPML along z, Roden & Gedney 2000) inside a conducting end.
//! One step is the Cartesian grid's: emission, push and deposit, B half, E, B half.

use std::f64::consts::PI;

use emcraft_model::units::{C0, EPS0, MU0};
use emcraft_particles::{Pusher, Species, V3, push, velocity};

use crate::pic::{Moved, Pulse, Wall};
use crate::yee::{Cpml, Grading};
use crate::{FdtdError, MAX_CELLS, Result, invalid};

/// The most rings a run holds.
pub const MAX_RINGS: usize = 20_000_000;

/// The r–z grid: `nr` cells of `dr` from the axis to a conducting wall, `nz` of `dz` along z
/// between conducting walls or periodic ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxiGrid {
    pub nr: usize,
    pub nz: usize,
    pub dr: f64,
    pub dz: f64,
    pub z_wall: Wall,
}

/// One of the six components' lattices: its points along r and z.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Lat {
    nr: usize,
    nz: usize,
    r_node: bool,
    z_node: bool,
}

impl Lat {
    fn len(self) -> usize {
        self.nr * self.nz
    }

    #[inline]
    fn at(self, i: usize, k: usize) -> usize {
        i + self.nr * k
    }
}

impl AxiGrid {
    /// The grid checked: cells of finite sizes > 0, at most [`MAX_CELLS`].
    ///
    /// ```
    /// use emcraft_fdtd::axi::AxiGrid;
    /// use emcraft_fdtd::pic::Wall;
    /// let g = AxiGrid::new(20, 40, 0.5e-3, 0.5e-3, Wall::Conductor)?;
    /// assert!(g.time_step(0.9) < 0.5e-3 / 299_792_458.0);
    /// assert!(AxiGrid::new(0, 40, 0.5e-3, 0.5e-3, Wall::Conductor).is_err());
    /// # Ok::<(), emcraft_fdtd::FdtdError>(())
    /// ```
    pub fn new(nr: usize, nz: usize, dr: f64, dz: f64, z_wall: Wall) -> Result<AxiGrid> {
        let ok = |h: f64| h.is_finite() && h > 0.0;
        if nr == 0 || nz == 0 || !ok(dr) || !ok(dz) {
            return Err(invalid("axisymmetric grid", "at least a cell of a finite size > 0 along r and z"));
        }
        let cells = nr.saturating_mul(nz);
        if cells > MAX_CELLS {
            return Err(FdtdError::TooLarge { cells, limit: MAX_CELLS });
        }
        Ok(AxiGrid { nr, nz, dr, dz, z_wall })
    }

    /// The largest stable step (s) times `courant`.
    pub fn time_step(&self, courant: f64) -> f64 {
        courant / (C0 * (1.0 / (self.dr * self.dr) + 1.0 / (self.dz * self.dz)).sqrt())
    }

    pub fn radius(&self) -> f64 {
        self.nr as f64 * self.dr
    }

    pub fn length(&self) -> f64 {
        self.nz as f64 * self.dz
    }

    /// Points along z: nodes or centres.
    fn zs(&self, node: bool) -> usize {
        if node && self.z_wall == Wall::Conductor { self.nz + 1 } else { self.nz }
    }

    fn lat(&self, r_node: bool, z_node: bool) -> Lat {
        Lat { nr: if r_node { self.nr + 1 } else { self.nr }, nz: self.zs(z_node), r_node, z_node }
    }

    /// Point `k` along z, wrapped round periodic ends, none past a wall.
    #[inline]
    fn wz(&self, k: isize, node: bool) -> Option<usize> {
        let n = self.zs(node) as isize;
        match self.z_wall {
            Wall::Periodic => Some(k.rem_euclid(n) as usize),
            Wall::Conductor => (0..n).contains(&k).then_some(k as usize),
        }
    }

    /// Whether z node `k` is on a conducting end.
    fn z_end(&self, k: usize) -> bool {
        self.z_wall == Wall::Conductor && (k == 0 || k == self.nz)
    }

    /// The area of node ring `i`'s faces normal to z (m²).
    fn area(&self, i: usize) -> f64 {
        if i == 0 { PI * 0.25 * self.dr * self.dr } else { 2.0 * PI * i as f64 * self.dr * self.dr }
    }

    /// The volume of node `i`'s ring (m³).
    fn volume(&self, i: usize) -> f64 {
        self.area(i) * self.dz
    }
}

/// A ring of particles: its radius and z (m), momentum γv (m/s) along r, θ and z, and how many
/// particles it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub r: f64,
    pub z: f64,
    pub u: V3,
    pub w: f64,
}

/// Rings of one species.
#[derive(Clone, Debug, PartialEq)]
pub struct Rings {
    pub species: Species,
    pub rings: Vec<Ring>,
}

/// A beam or bunch sent in through a conducting end, as the Cartesian grid's [`crate::pic::Bunch`]:
/// `pulse` in time, spread evenly over the area between `inner` and `radius` (m), at `energy` (J)
/// along z, `per_step` rings a step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Injection {
    pub high: bool,
    pub population: usize,
    pub pulse: Pulse,
    pub inner: f64,
    pub radius: f64,
    pub energy: f64,
    pub per_step: usize,
}

/// The six components.
struct Fields {
    er: Vec<f64>,
    ez: Vec<f64>,
    et: Vec<f64>,
    br: Vec<f64>,
    bz: Vec<f64>,
    bt: Vec<f64>,
}

/// The absorbing layers along z: the coefficients at z's nodes and centres for E's steps and B's
/// half steps, and ψ of each z difference (Er's ∂zBθ, Eθ's ∂zBr, Bθ's ∂zEr, Br's ∂zEθ).
struct Layers {
    cells: [usize; 2],
    e: [Vec<Cpml>; 2],
    b: [Vec<Cpml>; 2],
    psi: [Vec<f64>; 4],
}

/// An axisymmetric particle-in-cell run.
pub struct AxiPic {
    grid: AxiGrid,
    dt: f64,
    pusher: Pusher,
    f: Fields,
    jr: Vec<f64>,
    jz: Vec<f64>,
    jt: Vec<f64>,
    lats: [Lat; 6],
    populations: Vec<Rings>,
    injections: Vec<Injection>,
    metal: Option<Vec<bool>>,
    layers: Option<Layers>,
    count: u64,
    steps: usize,
}

/// Lattices' order in `lats`.
const ER: usize = 0;
const EZ: usize = 1;
const ET: usize = 2;
const BR: usize = 3;
const BZ: usize = 4;
const BT: usize = 5;

fn zeros(n: usize) -> Result<Vec<f64>> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| invalid("axisymmetric grid", format!("no memory for {n} values")))?;
    v.resize(n, 0.0);
    Ok(v)
}

/// The first-order shape's weight `s` cells away.
#[inline]
fn shape(s: f64) -> f64 {
    (1.0 - s.abs()).max(0.0)
}

/// The R₃ sequence's steps.
const R3: [f64; 3] = [0.819_172_513_396_164_4, 0.671_043_606_703_789_3, 0.549_700_477_901_970_4];

impl AxiPic {
    /// A run with no field and no rings, stepped by `dt` (s) within the grid's limit.
    pub fn new(grid: AxiGrid, dt: f64, pusher: Pusher) -> Result<AxiPic> {
        if !(dt.is_finite() && dt > 0.0 && dt < grid.time_step(1.0)) {
            return Err(invalid("time step", format!("{dt} s: must be > 0 and below the stability limit {} s", grid.time_step(1.0))));
        }
        let lats = [
            grid.lat(false, true),
            grid.lat(true, false),
            grid.lat(true, true),
            grid.lat(true, false),
            grid.lat(false, true),
            grid.lat(false, false),
        ];
        let f = Fields {
            er: zeros(lats[ER].len())?,
            ez: zeros(lats[EZ].len())?,
            et: zeros(lats[ET].len())?,
            br: zeros(lats[BR].len())?,
            bz: zeros(lats[BZ].len())?,
            bt: zeros(lats[BT].len())?,
        };
        Ok(AxiPic {
            grid,
            dt,
            pusher,
            f,
            jr: zeros(lats[ER].len())?,
            jz: zeros(lats[EZ].len())?,
            jt: zeros(lats[ET].len())?,
            lats,
            populations: Vec::new(),
            injections: Vec::new(),
            metal: None,
            layers: None,
            count: 0,
            steps: 0,
        })
    }

    pub fn grid(&self) -> &AxiGrid {
        &self.grid
    }

    pub fn dt(&self) -> f64 {
        self.dt
    }

    pub fn time(&self) -> f64 {
        self.steps as f64 * self.dt
    }

    pub fn populations(&self) -> &[Rings] {
        &self.populations
    }

    pub fn rings(&self) -> usize {
        self.populations.iter().map(|p| p.rings.len()).sum()
    }

    /// Adds `rings`, checked (finite, inside, weights ≥ 0); returns its index.
    pub fn add(&mut self, mut rings: Rings) -> Result<usize> {
        if self.rings().saturating_add(rings.rings.len()) > MAX_RINGS {
            return Err(invalid("rings", format!("at most {MAX_RINGS}")));
        }
        let (rmax, l) = (self.grid.radius(), self.grid.length());
        for p in &mut rings.rings {
            let ok = p.r.is_finite()
                && p.z.is_finite()
                && p.u.iter().all(|v| v.is_finite())
                && p.w.is_finite()
                && p.w >= 0.0
                && (0.0..=rmax).contains(&p.r);
            if !ok {
                return Err(invalid("ring", format!("{p:?}")));
            }
            match self.grid.z_wall {
                Wall::Periodic => p.z = p.z.rem_euclid(l),
                Wall::Conductor if !(0.0..=l).contains(&p.z) => return Err(invalid("ring", format!("{p:?}: outside"))),
                Wall::Conductor => {}
            }
        }
        self.populations.push(rings);
        Ok(self.populations.len() - 1)
    }

    /// Adds `injection`, through a conducting end.
    pub fn inject(&mut self, injection: Injection) -> Result<()> {
        let ok = |x: f64| x.is_finite() && x >= 0.0;
        if self.grid.z_wall != Wall::Conductor
            || injection.population >= self.populations.len()
            || !(ok(injection.inner)
                && ok(injection.radius)
                && injection.inner <= injection.radius
                && injection.radius <= self.grid.radius()
                && ok(injection.energy))
            || injection.per_step == 0
            || injection.per_step > crate::pic::MAX_PER_NODE
        {
            return Err(invalid("injection", format!("{injection:?}: through a conducting end, a ring inside the wall, 1 to 64 a step")));
        }
        self.injections.push(injection);
        Ok(())
    }

    /// The fields set to `f` of each point's (r, z) (m): E_r, E_θ, E_z (V/m) or B_r, B_θ, B_z (T)
    /// by `which` (0–2 E, 3–5 B, in that order), off the walls they are tangential to.
    pub fn set(&mut self, which: usize, f: impl Fn(f64, f64) -> f64) {
        let (lat, values) = match which {
            0 => (self.lats[ER], &mut self.f.er),
            1 => (self.lats[ET], &mut self.f.et),
            2 => (self.lats[EZ], &mut self.f.ez),
            3 => (self.lats[BR], &mut self.f.br),
            4 => (self.lats[BT], &mut self.f.bt),
            5 => (self.lats[BZ], &mut self.f.bz),
            _ => return,
        };
        let (dr, dz) = (self.grid.dr, self.grid.dz);
        for k in 0..lat.nz {
            for i in 0..lat.nr {
                let r = (i as f64 + if lat.r_node { 0.0 } else { 0.5 }) * dr;
                let z = (k as f64 + if lat.z_node { 0.0 } else { 0.5 }) * dz;
                let tangential_wall =
                    which < 3 && ((lat.r_node && i == self.grid.nr && which != 0) || (lat.z_node && self.grid.z_end(k) && which != 2));
                let axis_zero = matches!(which, 0 | 1 | 3) && lat.r_node && i == 0;
                if let Some(v) = values.get_mut(lat.at(i, k)) {
                    *v = if tangential_wall || axis_zero { 0.0 } else { f(r, z) };
                }
            }
        }
    }

    /// Conducting bodies: each cell whose centre (r, z) `metal` says is in a conductor. Returns
    /// whether any is.
    pub fn set_metal(&mut self, metal: impl Fn(f64, f64) -> bool) -> bool {
        let g = self.grid;
        let cells: Vec<bool> =
            (0..g.nz).flat_map(|k| (0..g.nr).map(move |i| (i, k))).map(|(i, k)| metal((i as f64 + 0.5) * g.dr, (k as f64 + 0.5) * g.dz)).collect();
        let any = cells.contains(&true);
        self.metal = any.then_some(cells);
        // E on edges touching metal starts at zero.
        for (which, lat) in [(ER, self.lats[ER]), (EZ, self.lats[EZ]), (ET, self.lats[ET])] {
            for k in 0..lat.nz {
                for i in 0..lat.nr {
                    if self.held(which, i, k) {
                        let v = match which {
                            ER => &mut self.f.er,
                            EZ => &mut self.f.ez,
                            _ => &mut self.f.et,
                        };
                        if let Some(x) = v.get_mut(lat.at(i, k)) {
                            *x = 0.0;
                        }
                    }
                }
            }
        }
        any
    }

    /// Whether cell (i, k) is metal (none outside the grid).
    fn metal_cell(&self, i: isize, k: isize) -> bool {
        let Some(m) = &self.metal else { return false };
        let g = self.grid;
        if i < 0 || i >= g.nr as isize {
            return false;
        }
        let Some(k) = g.wz(k, false) else { return false };
        m.get(i as usize + g.nr * k).copied().unwrap_or(false)
    }

    /// Whether E component `which` at (i, k) touches a metal cell.
    fn held(&self, which: usize, i: usize, k: usize) -> bool {
        let (i, k) = (i as isize, k as isize);
        match which {
            ER => self.metal_cell(i, k - 1) || self.metal_cell(i, k),
            EZ => self.metal_cell(i - 1, k) || self.metal_cell(i, k),
            _ => (i - 1..=i).any(|a| (k - 1..=k).any(|b| self.metal_cell(a, b))),
        }
    }

    /// Opens the conducting end at `high` (or the low one): a CPML of `cells` inside it.
    pub fn absorb(&mut self, high: bool, cells: usize) -> Result<()> {
        let g = self.grid;
        if g.z_wall != Wall::Conductor || cells == 0 || cells > crate::pic::MAX_LAYER {
            return Err(invalid("absorber", "a layer of 1 to 64 cells inside a conducting end"));
        }
        let mut layers = match self.layers.take() {
            Some(l) => l,
            None => Layers {
                cells: [0; 2],
                e: [vec![Cpml::NONE; g.nz + 1], vec![Cpml::NONE; g.nz]],
                b: [vec![Cpml::NONE; g.nz + 1], vec![Cpml::NONE; g.nz]],
                psi: [zeros(self.lats[ER].len())?, zeros(self.lats[ET].len())?, zeros(self.lats[BT].len())?, zeros(self.lats[BR].len())?],
            },
        };
        layers.cells[usize::from(high)] = cells;
        if layers.cells[0] + layers.cells[1] >= g.nz {
            self.layers = Some(layers);
            return Err(invalid("absorber", "the layers longer than the grid"));
        }
        let [lo, hi] = layers.cells.map(|n| n as f64 * g.dz);
        let length = g.length();
        let fc = C0 / length;
        let profile = |grading: &Grading, node: bool| -> Vec<Cpml> {
            (0..g.zs(node))
                .map(|k| {
                    let z = (k as f64 + if node { 0.0 } else { 0.5 }) * g.dz;
                    let low = if lo > 0.0 { (lo - z) / lo } else { 0.0 };
                    let up = if hi > 0.0 { (z - (length - hi)) / hi } else { 0.0 };
                    grading.at(low.max(up).max(0.0))
                })
                .collect()
        };
        let (whole, half) = (Grading::new(g.dz, self.dt, fc), Grading::new(g.dz, 0.5 * self.dt, fc));
        layers.e = [profile(&whole, true), profile(&whole, false)];
        layers.b = [profile(&half, true), profile(&half, false)];
        self.layers = Some(layers);
        Ok(())
    }

    /// The field at (r, z): (E_r, E_θ, E_z) (V/m) and (B_r, B_θ, B_z) (T).
    pub fn field_at(&self, r: f64, z: f64) -> (V3, V3) {
        let g = |which: usize, values: &[f64], odd: bool| self.interpolate(self.lats[which], values, r, z, odd);
        (
            [g(ER, &self.f.er, true), g(ET, &self.f.et, true), g(EZ, &self.f.ez, false)],
            [g(BR, &self.f.br, true), g(BT, &self.f.bt, true), g(BZ, &self.f.bz, false)],
        )
    }

    /// `values` on `lat` at (r, z), linearly; across the axis odd (`odd`) or even, past the outer
    /// wall and a conducting end the nearest point's.
    fn interpolate(&self, lat: Lat, values: &[f64], r: f64, z: f64, odd: bool) -> f64 {
        let g = self.grid;
        let sr = r / g.dr - if lat.r_node { 0.0 } else { 0.5 };
        let sz = z / g.dz - if lat.z_node { 0.0 } else { 0.5 };
        let (i0, k0) = (sr.floor(), sz.floor());
        let (fr, fz) = (sr - i0, sz - k0);
        let at = |i: isize, k: isize| -> f64 {
            let (i, sign) = if i < 0 { (-i - if lat.r_node { 0 } else { 1 }, if odd { -1.0 } else { 1.0 }) } else { (i, 1.0) };
            let i = (i as usize).min(lat.nr - 1);
            let k = match g.z_wall {
                Wall::Periodic => k.rem_euclid(lat.nz as isize) as usize,
                Wall::Conductor => k.clamp(0, lat.nz as isize - 1) as usize,
            };
            sign * values.get(lat.at(i, k)).copied().unwrap_or(0.0)
        };
        let (i0, k0) = (i0 as isize, k0 as isize);
        (1.0 - fr) * (1.0 - fz) * at(i0, k0) + fr * (1.0 - fz) * at(i0 + 1, k0) + (1.0 - fr) * fz * at(i0, k0 + 1) + fr * fz * at(i0 + 1, k0 + 1)
    }

    /// The energy in the field and in the rings (J).
    pub fn energy(&self) -> (f64, f64) {
        let g = self.grid;
        let sum = |lat: Lat, values: &[f64], coefficient: f64| -> f64 {
            let mut s = 0.0;
            for k in 0..lat.nz {
                for i in 0..lat.nr {
                    let r = (i as f64 + if lat.r_node { 0.0 } else { 0.5 }) * g.dr;
                    let ring = if lat.r_node && i == 0 { PI * 0.25 * g.dr * g.dr } else { 2.0 * PI * r * g.dr };
                    let x = values.get(lat.at(i, k)).copied().unwrap_or(0.0);
                    s += coefficient * x * x * ring * g.dz;
                }
            }
            s
        };
        let field = sum(self.lats[ER], &self.f.er, 0.5 * EPS0)
            + sum(self.lats[EZ], &self.f.ez, 0.5 * EPS0)
            + sum(self.lats[ET], &self.f.et, 0.5 * EPS0)
            + sum(self.lats[BR], &self.f.br, 0.5 / MU0)
            + sum(self.lats[BZ], &self.f.bz, 0.5 / MU0)
            + sum(self.lats[BT], &self.f.bt, 0.5 / MU0);
        let kinetic = self.populations.iter().map(|p| p.rings.iter().map(|q| q.w * p.species.kinetic(q.u)).sum::<f64>()).sum();
        (field, kinetic)
    }

    /// ε₀∮E·dA − Q (C) about each node off the walls, the axis' too, the metal and the layers:
    /// zero where Gauss's law holds.
    pub fn gauss(&self) -> Vec<f64> {
        let g = self.grid;
        let nodes = g.lat(true, true);
        let mut q = vec![0.0; nodes.len()];
        for pop in &self.populations {
            for p in &pop.rings {
                self.charge(&mut q, nodes, p.r, p.z, pop.species.charge * p.w);
            }
        }
        let mut out = Vec::new();
        for k in 0..nodes.nz {
            if g.z_end(k) || self.layers.as_ref().is_some_and(|l| (l.cells[0] > 0 && k <= l.cells[0]) || (l.cells[1] > 0 && k + l.cells[1] >= g.nz)) {
                continue;
            }
            let km = g.wz(k as isize - 1, false);
            for i in 0..g.nr {
                if (i as isize - 1..=i as isize).any(|a| (k as isize - 1..=k as isize).any(|b| self.metal_cell(a, b))) {
                    continue;
                }
                let er = |ii: usize| self.f.er.get(self.lats[ER].at(ii, k)).copied().unwrap_or(0.0);
                let radial = 2.0 * PI * g.dz * ((i as f64 + 0.5) * g.dr * er(i) - if i > 0 { (i as f64 - 0.5) * g.dr * er(i - 1) } else { 0.0 });
                let ez = |kk: usize| self.f.ez.get(self.lats[EZ].at(i, kk)).copied().unwrap_or(0.0);
                let axial = g.area(i) * (ez(k.min(g.nz - 1)) * if k < g.nz { 1.0 } else { 0.0 } - km.map_or(0.0, ez));
                out.push(EPS0 * (radial + axial) - q[nodes.at(i, k)]);
            }
        }
        out
    }

    /// Adds `charge` (C) at (r, z) to the nodes' charge `q`.
    fn charge(&self, q: &mut [f64], nodes: Lat, r: f64, z: f64, charge: f64) {
        let g = self.grid;
        let (sr, sz) = (r / g.dr, z / g.dz);
        let (i0, k0) = (sr.floor(), sz.floor());
        for (di, wr) in [(0, 1.0 - (sr - i0)), (1, sr - i0)] {
            for (dk, wz) in [(0, 1.0 - (sz - k0)), (1, sz - k0)] {
                let i = i0 as isize + di;
                if i < 0 || i > g.nr as isize {
                    continue;
                }
                let Some(k) = g.wz(k0 as isize + dk, true) else { continue };
                if let Some(v) = q.get_mut(nodes.at(i as usize, k)) {
                    *v += charge * wr * wz;
                }
            }
        }
    }

    /// One step.
    pub fn step(&mut self) -> Result<Moved> {
        let mut moved = Moved::default();
        self.jr.fill(0.0);
        self.jz.fill(0.0);
        self.jt.fill(0.0);
        self.emit(&mut moved)?;
        for k in 0..self.populations.len() {
            let species = self.populations[k].species;
            let mut rings = std::mem::take(&mut self.populations[k].rings);
            rings.retain_mut(|p| self.advance(species, p, &mut moved));
            self.populations[k].rings = rings;
        }
        self.faraday(0.5 * self.dt);
        self.ampere();
        self.faraday(0.5 * self.dt);
        self.steps += 1;
        Ok(moved)
    }

    /// The injections' rings for this step, on their ends.
    fn emit(&mut self, moved: &mut Moved) -> Result<()> {
        let t = self.time();
        for inj in self.injections.clone() {
            let Some(pop) = self.populations.get(inj.population) else { continue };
            let species = pop.species;
            let q = match inj.pulse {
                Pulse::Gaussian { charge, t0, sigma } => {
                    let mid = t + 0.5 * self.dt - t0;
                    if mid.abs() > 6.0 * sigma { 0.0 } else { charge * self.dt * (-0.5 * (mid / sigma).powi(2)).exp() / (sigma * (2.0 * PI).sqrt()) }
                }
                Pulse::Steady { current, rise } => current * self.dt * if rise > 0.0 { ((t + 0.5 * self.dt) / rise).min(1.0) } else { 1.0 },
            };
            if q <= 0.0 {
                continue;
            }
            let w = q / (species.charge.abs() * inj.per_step as f64);
            let sign = if inj.high { -1.0 } else { 1.0 };
            let u = species.momentum(inj.energy, [0.0, 0.0, sign]);
            let z = if inj.high { self.grid.length() } else { 0.0 };
            let mut new = Vec::with_capacity(inj.per_step);
            for _ in 0..inj.per_step {
                self.count += 1;
                let s = (0.5 + self.count as f64 * R3[0]).fract();
                let r = (inj.inner * inj.inner + s * (inj.radius * inj.radius - inj.inner * inj.inner)).sqrt();
                if self.metal_cell((r / self.grid.dr).floor() as isize, if inj.high { self.grid.nz as isize - 1 } else { 0 }) {
                    continue;
                }
                new.push(Ring { r, z, u, w });
            }
            if self.rings().saturating_add(new.len()) > MAX_RINGS {
                return Err(invalid("rings", format!("the injection would hold more than {MAX_RINGS}")));
            }
            moved.emitted += new.iter().map(|p| species.charge * p.w).sum::<f64>();
            moved.kinetic_in += new.iter().map(|p| p.w * species.kinetic(p.u)).sum::<f64>();
            if let Some(pop) = self.populations.get_mut(inj.population) {
                pop.rings.extend(new);
            }
        }
        Ok(())
    }

    /// Pushes, moves and deposits ring `p`; false when it is gone.
    fn advance(&mut self, species: Species, p: &mut Ring, moved: &mut Moved) -> bool {
        let g = self.grid;
        let (e, b) = self.field_at(p.r, p.z);
        let u = push(self.pusher, species, p.u, e, b, self.dt);
        if !u.iter().all(|v| v.is_finite()) {
            moved.lost += 1;
            return false;
        }
        let v = velocity(u);
        // Straight in the plane θ = 0, then turned back onto it.
        let (x, y) = (p.r + v[0] * self.dt, v[1] * self.dt);
        let mut r1 = x.hypot(y);
        let mut z1 = p.z + v[2] * self.dt;
        let (c, s) = if r1 > 0.0 { (x / r1, y / r1) } else { (1.0, 0.0) };
        let u1 = [c * u[0] + s * u[1], -s * u[0] + c * u[1], u[2]];
        // Walls: the conductor at R and conducting ends.
        let mut hit = None;
        let mut f = 1.0f64;
        if r1 > g.radius() {
            f = ((g.radius() - p.r) / (r1 - p.r)).clamp(0.0, 1.0);
            hit = Some(2);
        }
        if g.z_wall == Wall::Conductor {
            for (end, beyond) in [(0usize, z1 < 0.0), (1, z1 > g.length())] {
                if beyond {
                    let wall = if end == 0 { 0.0 } else { g.length() };
                    let fe = ((wall - p.z) / (z1 - p.z)).clamp(0.0, 1.0);
                    if fe < f {
                        (f, hit) = (fe, Some(end));
                    }
                }
            }
        }
        if hit.is_some() {
            r1 = (p.r + f * (r1 - p.r)).clamp(0.0, g.radius());
            z1 = (p.z + f * (z1 - p.z)).clamp(0.0, g.length());
        }
        let q = species.charge * p.w;
        self.deposit(p.r, p.z, r1, z1, q);
        self.deposit_theta(0.5 * (p.r + r1), 0.5 * (p.z + z1), q * v[1]);
        let absorbed = |moved: &mut Moved| moved.kinetic_out += p.w * species.kinetic(u1);
        match hit {
            Some(2) => {
                moved.collected[0][1] += q;
                absorbed(moved);
                return false;
            }
            Some(end) => {
                moved.collected[2][end] += q;
                absorbed(moved);
                return false;
            }
            None => {}
        }
        if self.metal_cell((r1 / g.dr).floor() as isize, (z1 / g.dz).floor() as isize) {
            moved.metal += q;
            absorbed(moved);
            return false;
        }
        p.r = r1;
        p.z = if g.z_wall == Wall::Periodic { z1.rem_euclid(g.length()) } else { z1 };
        p.u = u1;
        true
    }

    /// The current of charge `q` (C) moving from (r0, z0) to (r1, z1) in a step (z unwrapped):
    /// Esirkepov's flux through each node ring's faces, over their areas.
    fn deposit(&mut self, r0: f64, z0: f64, r1: f64, z1: f64, q: f64) {
        let g = self.grid;
        let window = |x0: f64, x1: f64, h: f64| -> (isize, [f64; 4], [f64; 4]) {
            let first = (x0 / h).floor() as isize - 1;
            let at = |x: f64| std::array::from_fn(|m| shape(x / h - (first + m as isize) as f64));
            (first, at(x0), at(x1))
        };
        let (fr, s0r, s1r) = window(r0, r1, g.dr);
        let (fz, s0z, s1z) = window(z0, z1, g.dz);
        let dsr: [f64; 4] = std::array::from_fn(|m| s1r[m] - s0r[m]);
        let dsz: [f64; 4] = std::array::from_fn(|m| s1z[m] - s0z[m]);
        let (ler, lez) = (self.lats[ER], self.lats[EZ]);
        // I_r through r_{i+½}: the running sum over i.
        for mz in 0..4 {
            let Some(k) = g.wz(fz + mz as isize, true) else { continue };
            let across = s0z[mz] + 0.5 * dsz[mz];
            if across == 0.0 {
                continue;
            }
            let mut flux = 0.0;
            for mr in 0..3 {
                flux -= q * dsr[mr] * across / self.dt;
                let i = fr + mr as isize;
                if i < 0 || i >= g.nr as isize {
                    continue;
                }
                let face = 2.0 * PI * (i as f64 + 0.5) * g.dr * g.dz;
                if let Some(v) = self.jr.get_mut(ler.at(i as usize, k)) {
                    *v += flux / face;
                }
            }
        }
        // I_z through the node rings' areas: the running sum over k.
        for mr in 0..4 {
            let i = fr + mr as isize;
            if i < 0 || i > g.nr as isize {
                continue;
            }
            let across = s0r[mr] + 0.5 * dsr[mr];
            if across == 0.0 {
                continue;
            }
            let area = g.area(i as usize);
            let mut flux = 0.0;
            for mz in 0..3 {
                flux -= q * dsz[mz] * across / self.dt;
                let Some(k) = g.wz(fz + mz as isize, false) else { continue };
                if let Some(v) = self.jz.get_mut(lez.at(i as usize, k)) {
                    *v += flux / area;
                }
            }
        }
    }

    /// The azimuthal current q v_θ (A·m) at (r, z) on E_θ's nodes, over their rings' volumes.
    fn deposit_theta(&mut self, r: f64, z: f64, qv: f64) {
        let g = self.grid;
        let let_ = self.lats[ET];
        let (sr, sz) = (r / g.dr, z / g.dz);
        let (i0, k0) = (sr.floor(), sz.floor());
        for (di, wr) in [(0, 1.0 - (sr - i0)), (1, sr - i0)] {
            let i = i0 as isize + di;
            if i < 1 || i > g.nr as isize {
                continue;
            }
            for (dk, wz) in [(0, 1.0 - (sz - k0)), (1, sz - k0)] {
                let Some(k) = g.wz(k0 as isize + dk, true) else { continue };
                let volume = g.volume(i as usize);
                if let Some(v) = self.jt.get_mut(let_.at(i as usize, k)) {
                    *v += qv * wr * wz / volume;
                }
            }
        }
    }

    /// A z difference through the layers: d + ψ, ψ advanced (`which` ψ, B's coefficients or E's,
    /// at z point `k`, node or centre).
    #[inline]
    fn stretch(layers: Option<&mut Layers>, (which, field_b): (usize, bool), (node, k, id): (bool, usize, usize), d: f64) -> f64 {
        let Some(l) = layers else { return d };
        let set = if field_b { &l.b } else { &l.e };
        let c = set[usize::from(!node)].get(k).copied().unwrap_or(Cpml::NONE);
        if c == Cpml::NONE {
            return d;
        }
        let Some(psi) = l.psi[which].get_mut(id) else { return d };
        *psi = c.psi(*psi, d);
        d + *psi
    }

    /// B advanced by `dt` (s): ∂B/∂t = −∇ × E.
    fn faraday(&mut self, dt: f64) {
        let g = self.grid;
        let [ler, lez, let_, lbr, lbz, lbt] = self.lats;
        let f = &mut self.f;
        let mut layers = self.layers.as_mut();
        // B_θ: −(∂z E_r − ∂r E_z).
        for k in 0..lbt.nz {
            let Some(kp) = g.wz(k as isize + 1, true) else { continue };
            for i in 0..lbt.nr {
                let dz = (f.er[ler.at(i, kp)] - f.er[ler.at(i, k)]) / g.dz;
                let id = lbt.at(i, k);
                let dz = Self::stretch(layers.as_deref_mut(), (2, true), (false, k, id), dz);
                let dr = (f.ez[lez.at(i + 1, k)] - f.ez[lez.at(i, k)]) / g.dr;
                f.bt[id] -= dt * (dz - dr);
            }
        }
        // B_r: +∂z E_θ.
        for k in 0..lbr.nz {
            let Some(kp) = g.wz(k as isize + 1, true) else { continue };
            for i in 0..lbr.nr {
                let id = lbr.at(i, k);
                let dz = (f.et[let_.at(i, kp)] - f.et[let_.at(i, k)]) / g.dz;
                let dz = Self::stretch(layers.as_deref_mut(), (3, true), (false, k, id), dz);
                f.br[id] += dt * dz;
            }
        }
        // B_z: −(1/r) ∂r (r E_θ).
        for k in 0..lbz.nz {
            for i in 0..lbz.nr {
                let (r0, r1, rm) = (i as f64 * g.dr, (i + 1) as f64 * g.dr, (i as f64 + 0.5) * g.dr);
                let curl = (r1 * f.et[let_.at(i + 1, k)] - r0 * f.et[let_.at(i, k)]) / (rm * g.dr);
                f.bz[lbz.at(i, k)] -= dt * curl;
            }
        }
    }

    /// E advanced by a step: ε₀ ∂E/∂t = ∇ × B/μ₀ − J, off the walls and metal it is tangential to.
    fn ampere(&mut self) {
        let g = self.grid;
        let dt = self.dt;
        let c2 = C0 * C0;
        let [ler, lez, let_, lbr, lbz, lbt] = self.lats;
        // Held edges first (they need &self).
        let held: [Vec<bool>; 3] = [ER, EZ, ET].map(|which| {
            let lat = self.lats[which];
            if self.metal.is_none() {
                return Vec::new();
            }
            (0..lat.nz).flat_map(|k| (0..lat.nr).map(move |i| (i, k))).map(|(i, k)| self.held(which, i, k)).collect()
        });
        let is_held = |w: usize, id: usize| held[w].get(id).copied().unwrap_or(false);
        let f = &mut self.f;
        let mut layers = self.layers.as_mut();
        // E_r: −c² ∂z B_θ − J_r/ε₀, off the conducting ends.
        for k in 0..ler.nz {
            if g.z_end(k) {
                continue;
            }
            let Some(km) = g.wz(k as isize - 1, false) else { continue };
            for i in 0..ler.nr {
                let id = ler.at(i, k);
                let dz = (f.bt[lbt.at(i, k.min(lbt.nz - 1))] - f.bt[lbt.at(i, km)]) / g.dz;
                let dz = Self::stretch(layers.as_deref_mut(), (0, false), (true, k, id), dz);
                if is_held(0, id) {
                    continue;
                }
                f.er[id] += dt * (-c2 * dz - self.jr[id] / EPS0);
            }
        }
        // E_θ: c²(∂z B_r − ∂r B_z) − J_θ/ε₀, off the axis, the wall and the ends.
        for k in 0..let_.nz {
            if g.z_end(k) {
                continue;
            }
            let Some(km) = g.wz(k as isize - 1, false) else { continue };
            for i in 1..g.nr {
                let id = let_.at(i, k);
                let dz = (f.br[lbr.at(i, k.min(lbr.nz - 1))] - f.br[lbr.at(i, km)]) / g.dz;
                let dz = Self::stretch(layers.as_deref_mut(), (1, false), (true, k, id), dz);
                let dr = (f.bz[lbz.at(i, k)] - f.bz[lbz.at(i - 1, k)]) / g.dr;
                if is_held(2, id) {
                    continue;
                }
                f.et[id] += dt * (c2 * (dz - dr) - self.jt[id] / EPS0);
            }
        }
        // E_z: c² (1/r) ∂r (r B_θ) − J_z/ε₀, off the wall; on the axis 4B_θ(r_½)/Δr.
        for k in 0..lez.nz {
            for i in 0..g.nr {
                let id = lez.at(i, k);
                let curl = if i == 0 {
                    4.0 * f.bt[lbt.at(0, k)] / g.dr
                } else {
                    let (rp, rm, r) = ((i as f64 + 0.5) * g.dr, (i as f64 - 0.5) * g.dr, i as f64 * g.dr);
                    (rp * f.bt[lbt.at(i, k)] - rm * f.bt[lbt.at(i - 1, k)]) / (r * g.dr)
                };
                if is_held(1, id) {
                    continue;
                }
                f.ez[id] += dt * (c2 * curl - self.jz[id] / EPS0);
            }
        }
    }
}

#[cfg(test)]
#[path = "axi_tests.rs"]
mod tests;

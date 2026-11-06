//! Closed problems on solids in the time domain: the solid's bounding box on a tensor grid
//! whose lines pass through the box's faces, the ports' rectangles and the bounds of its
//! bodies, each port's face extended outward by a feed and a convolutional PML (Roden & Gedney
//! 2000) as the guide's ends are, the TE₁₀ of each port's rectangle launched and projected out
//! as on the guide (the crate's documentation), one run per port excited.
//!
//! **Geometry.** Each cell takes the material at its centre ([`Model::material_at`]: none outside
//! the solid or in a conductor, whose cells are metal); a feed's cells are the port's material
//! inside its rectangle, metal elsewhere. Each E edge is updated when its four cells are
//! material and its midpoint is not in a conductor (a post thinner than a cell stands as a line
//! of conducting edges), with the area-weighted ε, conductivity and poles of its cells (the
//! exponential update with conductivity, auxiliary equations with poles); each H face takes the
//! mean μ and magnetic conductivity of the cells either side that are material. Curved bodies
//! are staircased; the walls are perfect conductors. Symmetry planes, which halve the frequency
//! domain's model, are ignored: the whole solid is solved, TE10 keeping the planes' parity.
//!
//! **Ports.** A port's face lies on a face of the solid's bounding box, a rectangle of one
//! material whose broad side sets TE₁₀: E along the narrow side, sin(π s/a) across the broad.
//! The grid along the port's axis continues outward from the face with the face's own step: a
//! feed of `feed` cells (at least 1.5 broad sides: the fields beyond cut-off must die out before
//! the absorber, or a mode trapped below cut-off, in a bend, makes the CPML grow without bound)
//! to the soft source, as many more and the `pml` cells of the absorber, closed by a perfect
//! conductor.
//!
//! **S-parameters.** Exciting port q, the TE₁₀ projections at every port's face are transformed on
//! the fly; a run of port q's guide alone, straight and uniform on the same transverse lines and
//! the same step along it, gives the incident wave at its face. Then S_qq = (V_q − V_qⁱⁿᶜ)/V_qⁱⁿᶜ
//! and S_pq = V_p/V_qⁱⁿᶜ in power waves (amplitudes V/‖e‖, power ∝ ‖e‖ β/μ), at the ports' faces.

use emcraft_model::units::angular;
use std::f64::consts::PI;
use std::ops::ControlFlow;

use emcraft_model::units::{C0, EPS0};
use emcraft_model::{Aabb, End, Material, Model, Point3};
use num_complex::Complex64 as C;

use crate::dispersion::{Dispersion, Term, check_poles, exponential_fit};
use crate::guide::lines;
use crate::open::{BoxCoefficients, BoxCpml, BoxPsi, OpenKernel, Steps};
use crate::yee::{Band, Cpml, Fields, Grading, Pulse, Settling, index, lattices, leapfrog, time_step};
use crate::{FdtdError, Losses, MAX_CELLS, Options, PortGuide, Result, TdResult, frequencies_ok, invalid, transmission};

/// The most ports a closed solid may excite in one solve.
pub const MAX_EXCITED: usize = 16;

/// How far past the incident wave a transform may grow before the run counts as unstable.
const GROWTH: f64 = 20.0;

/// The shortest feed, in broad sides of the ports it carries.
const FEED_BROAD: f64 = 1.5;

/// Steps between checks of whether the transforms have settled.
const CHECK: usize = 500;

/// The broadband S-parameters of the model's solid between its waveguide ports, each of
/// `inputs` (indices into the model's ports) excited in turn, at `frequencies` (Hz).
pub fn s_parameters_solid(model: &Model, o: &Options, frequencies: &[f64], inputs: &[usize]) -> Result<TdResult> {
    o.validate()?;
    if !frequencies_ok(frequencies) {
        return Err(invalid("frequencies", "must be finite numbers > 0"));
    }
    let bounds = refuse_unsupported(model)?;
    let ports = model.ports.iter().map(|p| FacePort::new(model, p, bounds)).collect::<Result<Vec<_>>>()?;
    if ports.is_empty() {
        return Err(FdtdError::NotReady("no waveguide ports".into()));
    }
    if inputs.is_empty() || inputs.iter().any(|&q| q >= ports.len()) || inputs.len() > MAX_EXCITED {
        return Err(invalid("excitation", format!("1 to {MAX_EXCITED} of the model's {} ports", ports.len())));
    }
    let band = Band::of(frequencies);
    let losses = Losses::new(band.fc);
    let guides: Vec<PortGuide> = ports.iter().map(|p| p.guide(model, losses)).collect::<Result<_>>()?;
    for (p, g) in ports.iter().zip(&guides) {
        if let Some(f) = frequencies.iter().find(|&&f| !g.propagates(f)) {
            return Err(invalid("frequency", format!("{f} Hz is below port `{}`'s TE10 cut-off", p.name)));
        }
    }
    let grid = SolidGrid::new(model, o, bounds, &ports)?;
    let steps = Steps::of(&grid.lines);
    let min = steps.primal.each_ref().map(|d| d.iter().copied().fold(f64::INFINITY, f64::min));
    let dt = time_step(o.courant, min);
    check_poles(model.materials.iter().flat_map(|m| m.dispersion.iter()), dt)?;
    let cutoff = guides.iter().map(cutoff).fold(0.0f64, f64::max);
    // 10⁻⁸ at the cut-off: a mode trapped just below it (in a bend) is barely excited and rings
    // on lossless after the pulse.
    let pulse = Pulse::above(band, cutoff, 1e-8);
    let t_min = pulse.t0 + 4.0 * pulse.tau + 3.0 * grid.diagonal() / slowest(model, &guides, band);
    let run = Run { model, o, frequencies, dt, losses, pulse, t_min };
    let nf = frequencies.len();
    let mut s = vec![C::new(0.0, 0.0); inputs.len() * ports.len() * nf];
    let (mut device_steps, mut reference_steps, mut converged) = (0, 0, true);
    for (c, &q) in inputs.iter().enumerate() {
        let reference = SolidGrid::reference(&grid, &ports[q], o)?;
        let (inc, r_steps, _) = run.go(&reference, &ports[q], std::slice::from_ref(&ports[q]), None)?;
        let inc = inc.into_iter().next().unwrap_or_default();
        let scale = inc.iter().fold(0.0f64, |m, c| m.max(c.norm()));
        let (v, d_steps, settled) = run.go(&grid, &ports[q], &ports, Some(scale))?;
        reference_steps += r_steps;
        device_steps += d_steps;
        converged &= settled;
        for (p, vp) in v.iter().enumerate() {
            for (k, &f) in frequencies.iter().enumerate() {
                let inc_k = inc.get(k).copied().unwrap_or_default();
                let x = if p == q {
                    (vp[k] - inc_k) / inc_k
                } else {
                    transmission(vp[k], &guides[p], inc_k, &guides[q], f)
                        .ok_or_else(|| invalid("frequency", format!("{f} Hz is below a port's TE10 cut-off")))?
                };
                s[(c * ports.len() + p) * nf + k] = x;
            }
        }
    }
    if !s.iter().all(|x| x.re.is_finite() && x.im.is_finite()) {
        return Err(invalid("run", "the incident wave vanished at a frequency (outside the pulse's band)"));
    }
    Ok(TdResult {
        frequencies: frequencies.to_vec(),
        ports: ports.len(),
        inputs: inputs.to_vec(),
        s,
        cells: grid.cells.len(),
        dt,
        steps: device_steps,
        reference_steps,
        converged,
        loss_reference: losses.reference,
    })
}

/// The solid's bounding box, refusing what closed solids in the time domain do not support.
/// Symmetry planes are left out: the solid is whole, and TE10 launched in it keeps the parity the
/// planes' walls keep (they only spare the frequency domain half its unknowns).
fn refuse_unsupported(model: &Model) -> Result<Aabb> {
    let unsupported = |what: &str| Err(FdtdError::Unsupported(format!("{what} in the time domain's closed solids")));
    if model.domain.solid.is_none() {
        return Err(FdtdError::NotReady("no solid".into()));
    }
    if model.domain.wall_conductivity.is_some() {
        return unsupported("walls of finite conductivity (its walls are perfect conductors; leave out model.walls' conductivity)");
    }
    if !model.elements.is_empty() {
        return Err(FdtdError::Unsupported("lumped elements in the time domain".into()));
    }
    if !model.lumped.is_empty() || !model.sources.is_empty() {
        return unsupported("lumped ports and sources (waveguide ports only; an open boundary takes them)");
    }
    model.bounds().ok_or_else(|| FdtdError::NotReady("no solid".into()))
}

/// The cut-off frequency of a port guide's TE₁₀.
fn cutoff(g: &PortGuide) -> f64 {
    C0 / (2.0 * g.width * (g.material.eps_r.get() * g.material.mu_r.get()).sqrt())
}

/// The slowest in-band wave: the ports' TE₁₀ group velocity at the band's bottom, and light in
/// the slowest material.
fn slowest(model: &Model, guides: &[PortGuide], band: Band) -> f64 {
    let slow_material = model.materials.iter().map(|m| (m.eps_r.get() * m.mu_r.get()).sqrt()).fold(1.0f64, f64::max);
    guides.iter().fold(C0 / slow_material, |v, g| {
        let n = (g.material.eps_r.get() * g.material.mu_r.get()).sqrt();
        let r = cutoff(g) / band.f0;
        v.min(C0 / n * (1.0 - r * r).max(1e-4).sqrt())
    })
}

/// A waveguide port on a face of the solid's bounding box.
struct FacePort {
    name: String,
    /// The axis normal to the face, and whether the feed extends to its high side.
    axis: usize,
    high: bool,
    /// The face's position along its axis, and the rectangle's span along each axis (the
    /// normal's unused).
    at: f64,
    span: [(f64, f64); 3],
    /// The broad side's axis and the narrow side's, along which E lies.
    broad: usize,
    narrow: usize,
    material: usize,
}

impl FacePort {
    fn new(model: &Model, port: &emcraft_model::Port, b: Aabb) -> Result<FacePort> {
        let name = port.name.clone();
        let refuse = |why: &str| Err(FdtdError::Unsupported(format!("port `{name}` in the time domain: {why}")));
        if port.deembed != 0.0 {
            return refuse("de-embedding (set it to 0)");
        }
        if port.reference.is_some() {
            return refuse("a reference impedance (its TE10 needs none)");
        }
        let high = port.end == End::End;
        let (axis, at, mut span) = match port.face {
            Some(f) => {
                let a = f.axis.index();
                let (u, v) = others(a);
                let mut span = [(0.0, 0.0); 3];
                span[u] = (f.u0, f.u0 + f.a.get());
                span[v] = (f.v0, f.v0 + f.b.get());
                (a, f.at, span)
            }
            None => (2, if high { b.max[2] } else { b.min[2] }, [(b.min[0], b.max[0]), (b.min[1], b.max[1]), (0.0, 0.0)]),
        };
        let size = (b.max - b.min).norm();
        let face = if high { b.max[axis] } else { b.min[axis] };
        if (at - face).abs() > 1e-9 * size {
            return refuse("its face must lie on the solid's bounding box, its guide outside");
        }
        let (u, v) = others(axis);
        for d in [u, v] {
            span[d] = (span[d].0.max(b.min[d]), span[d].1.min(b.max[d]));
        }
        let (lu, lv) = (span[u].1 - span[u].0, span[v].1 - span[v].0);
        if !(lu > 0.0 && lv > 0.0) {
            return refuse("its rectangle lies off the solid's face");
        }
        if (lu - lv).abs() <= 1e-9 * lu.max(lv) {
            return refuse("a square rectangle (TE10 and TE01 are degenerate)");
        }
        let (broad, narrow) = if lu > lv { (u, v) } else { (v, u) };
        // The rectangle must be one material just inside the face: TE₁₀ of a filled rectangle.
        let inward = if high { -1e-7 } else { 1e-7 } * size;
        let mut material = None;
        for i in 0..7 {
            for j in 0..7 {
                let mut p = [0.0; 3];
                p[axis] = at + inward;
                p[u] = span[u].0 + (i as f64 + 0.5) / 7.0 * lu;
                p[v] = span[v].0 + (j as f64 + 0.5) / 7.0 * lv;
                let m = model.material_at(Point3::new(p[0], p[1], p[2]));
                if m.is_none() || (material.is_some() && m != material) {
                    return refuse("its cross-section is not a rectangle of one material (TE10 ports only)");
                }
                material = m;
            }
        }
        let material = material.ok_or_else(|| invalid("port", format!("`{name}` has no material")))?;
        Ok(FacePort { name, axis, high, at, span, broad, narrow, material })
    }

    /// The port's guide, its losses as the time domain realises them.
    fn guide(&self, model: &Model, losses: Losses) -> Result<PortGuide> {
        let m: &Material = model.materials.get(self.material).ok_or_else(|| invalid("material", "unknown"))?;
        let (b, n) = (self.span[self.broad], self.span[self.narrow]);
        Ok(PortGuide { width: b.1 - b.0, height: n.1 - n.0, material: m.clone(), losses })
    }

    /// Whether (the other two coordinates of) `p` lie inside the rectangle.
    fn holds(&self, p: [f64; 3]) -> bool {
        let (u, v) = others(self.axis);
        [u, v].iter().all(|&d| p[d] > self.span[d].0 && p[d] < self.span[d].1)
    }
}

/// The two axes other than `a`, ascending.
const fn others(a: usize) -> (usize, usize) {
    match a {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

/// A tensor grid with the cells' materials, the absorber's cells at each end of each axis and
/// each port's face and source plane (node indices along its axis).
struct SolidGrid {
    lines: [Vec<f64>; 3],
    /// The material of each cell, row-major in x; `None` for metal.
    cells: Vec<Option<u32>>,
    pml: [[usize; 2]; 3],
    /// The feed's cells at each end of each axis, between the face and the source and again
    /// between the source and the absorber.
    feeds: [[usize; 2]; 3],
    planes: Vec<usize>,
    sources: Vec<usize>,
    /// Where a conductor thinner than a cell is looked for at the edges' midpoints: the solid's
    /// box (none on a reference guide).
    solid: Option<Aabb>,
}

impl SolidGrid {
    fn new(model: &Model, o: &Options, b: Aabb, ports: &[FacePort]) -> Result<SolidGrid> {
        let h = o.cell;
        let size = b.max - b.min;
        // A feed long enough for the fields beyond cut-off to die out before the absorber: at
        // least 1.5 times the broadest port of its face (a mode trapped below cut-off, in a bend,
        // reaching the CPML makes the run grow without bound).
        let feed = |d: usize, high: bool, step: f64| {
            let broad = ports.iter().filter(|p| p.axis == d && p.high == high).map(|p| p.span[p.broad].1 - p.span[p.broad].0).fold(0.0f64, f64::max);
            o.feed.max((FEED_BROAD * broad / step).ceil() as usize)
        };
        let most = |d: usize| o.pml + 2 * feed(d, true, h).max(feed(d, false, h));
        let estimate: f64 = (0..3).map(|d| size[d] / h + 2.0 * most(d) as f64 + 1.0).product();
        if !(estimate <= MAX_CELLS as f64) {
            return Err(FdtdError::TooLarge { cells: estimate.min(usize::MAX as f64) as usize, limit: MAX_CELLS });
        }
        let mut pml = [[0usize; 2]; 3];
        let mut feeds = [[0usize; 2]; 3];
        let lines: [Vec<f64>; 3] = [0, 1, 2].map(|d| {
            let (lo, hi) = (b.min[d], b.max[d]);
            let inside = |x: f64| x > lo && x < hi;
            // The box's faces and the ports' edges, always; the bodies' bounds unless within a
            // quarter cell of another break (a sliver of a cell would shorten the time step).
            let mut breaks = vec![lo, hi];
            for p in ports.iter().filter(|p| p.axis != d) {
                breaks.extend([p.span[d].0, p.span[d].1].into_iter().filter(|x| inside(*x)));
            }
            let bodies =
                model.inclusions.iter().map(emcraft_model::Inclusion::bounds).chain(model.features.iter().map(emcraft_model::Feature::bounds));
            let mut extra: Vec<f64> = bodies.flat_map(|a| [a.min[d], a.max[d]]).filter(|x| inside(*x)).collect();
            extra.sort_by(f64::total_cmp);
            for x in extra {
                if breaks.iter().all(|b| (b - x).abs() >= 0.25 * h) {
                    breaks.push(x);
                }
            }
            let mut x = lines(breaks, size[d], h);
            // The feeds: the face's own step outward.
            let (low, high) = (ports.iter().any(|p| p.axis == d && !p.high), ports.iter().any(|p| p.axis == d && p.high));
            if high && let [.., a, z] = x[..] {
                feeds[d][1] = feed(d, true, z - a);
                x.extend((1..=o.pml + 2 * feeds[d][1]).map(|k| z + k as f64 * (z - a)));
                pml[d][1] = o.pml;
            }
            if low && let [a, z, ..] = x[..] {
                feeds[d][0] = feed(d, false, z - a);
                let mut lower: Vec<f64> = (1..=o.pml + 2 * feeds[d][0]).rev().map(|k| a - k as f64 * (z - a)).collect();
                lower.extend(x);
                x = lower;
                pml[d][0] = o.pml;
            }
            x
        });
        let n = lines.each_ref().map(|x| x.len().saturating_sub(1));
        let count = n[0].saturating_mul(n[1]).saturating_mul(n[2]);
        if count > MAX_CELLS {
            return Err(FdtdError::TooLarge { cells: count, limit: MAX_CELLS });
        }
        let node = |d: usize, x: f64| lines[d].iter().position(|y| (y - x).abs() <= 1e-9 * size[d].max(f64::MIN_POSITIVE)).unwrap_or(0);
        let planes: Vec<usize> = ports.iter().map(|p| node(p.axis, p.at)).collect();
        let sources = ports
            .iter()
            .zip(&planes)
            .map(|(p, &k)| {
                let f = feeds[p.axis][usize::from(p.high)];
                if p.high { k + f } else { k.saturating_sub(f) }
            })
            .collect();
        let centre = |d: usize, i: usize| 0.5 * (lines[d][i] + lines[d][i + 1]);
        let mut cells = Vec::with_capacity(count);
        for k in 0..n[2] {
            for j in 0..n[1] {
                for i in 0..n[0] {
                    let c = [centre(0, i), centre(1, j), centre(2, k)];
                    let out: Vec<usize> = (0..3).filter(|&d| c[d] < b.min[d] || c[d] > b.max[d]).collect();
                    let m = match out.as_slice() {
                        [] => model.material_at(Point3::new(c[0], c[1], c[2])),
                        [d] => ports.iter().find(|p| p.axis == *d && p.high == (c[*d] > b.max[*d]) && p.holds(c)).map(|p| p.material),
                        _ => None,
                    };
                    cells.push(m.map(|m| m as u32));
                }
            }
        }
        Ok(SolidGrid { lines, cells, pml, feeds, planes, sources, solid: Some(b) })
    }

    /// Port `p`'s guide alone, straight and uniform: the transverse lines across its rectangle,
    /// along its axis the feed's step from the feed's end through the face to an absorber.
    fn reference(g: &SolidGrid, p: &FacePort, o: &Options) -> Result<SolidGrid> {
        let a = p.axis;
        let plane = node_of(&g.lines[a], p.at);
        let step = if p.high {
            g.lines[a].get(plane + 1).zip(g.lines[a].get(plane)).map(|(y, x)| y - x)
        } else {
            g.lines[a].get(plane).zip(g.lines[a].get(plane.wrapping_sub(1))).map(|(y, x)| y - x)
        }
        .filter(|s| *s > 0.0)
        .ok_or_else(|| invalid("grid", "a port's feed has no step"))?;
        let feed = g.feeds[a][usize::from(p.high)];
        let ext = o.pml + 2 * feed;
        let beyond = o.pml + feed;
        let lines: [Vec<f64>; 3] = [0, 1, 2].map(|d| {
            if d == a {
                // The face at node `ext` from the feed's end (low) or from the absorber's (high).
                let first = if p.high { p.at - beyond as f64 * step } else { p.at - ext as f64 * step };
                (0..=ext + beyond).map(|i| first + i as f64 * step).collect()
            } else {
                g.lines[d].iter().copied().filter(|&x| x >= p.span[d].0 - 1e-12 && x <= p.span[d].1 + 1e-12).collect()
            }
        });
        let n = lines.each_ref().map(|x| x.len().saturating_sub(1));
        let mut pml = [[0usize; 2]; 3];
        pml[a] = [o.pml, o.pml];
        let plane = if p.high { beyond } else { ext };
        let source = if p.high { plane + feed } else { plane - feed };
        let mut feeds = [[0usize; 2]; 3];
        feeds[a] = [feed, feed];
        Ok(SolidGrid {
            cells: vec![Some(p.material as u32); n[0] * n[1] * n[2]],
            lines,
            pml,
            feeds,
            planes: vec![plane],
            sources: vec![source],
            solid: None,
        })
    }

    fn n(&self) -> [usize; 3] {
        self.lines.each_ref().map(|x| x.len().saturating_sub(1))
    }

    /// The length of the grid's diagonal.
    fn diagonal(&self) -> f64 {
        self.lines.iter().map(|x| (x[x.len() - 1] - x[0]).powi(2)).sum::<f64>().sqrt()
    }

    /// The material of cell `c`: `None` off the grid or in metal.
    fn cell(&self, c: [isize; 3]) -> Option<usize> {
        let n = self.n();
        if (0..3).any(|d| c[d] < 0 || c[d] as usize >= n[d]) {
            return None;
        }
        self.cells.get(index(n, c[0] as usize, c[1] as usize, c[2] as usize)).copied().flatten().map(|m| m as usize)
    }

    /// The update coefficients for a time step `dt`, the loss tangents realised by `losses`.
    fn coefficients(&self, model: &Model, steps: &Steps, dt: f64, losses: Losses) -> BoxCoefficients {
        let n = self.n();
        let [e_dims, h_dims] = lattices(n);
        let vacuum = Material::vacuum();
        let material = |m: usize| model.materials.get(m).unwrap_or(&vacuum);
        let mut dispersion = [Dispersion::none(), Dispersion::none(), Dispersion::none()];
        let ce = [0, 1, 2].map(|a| {
            let (u, v) = others(a);
            let d = e_dims[a];
            let len = d.iter().product();
            let mut ce = vec![0.0; len];
            for k in 0..d[2] {
                for j in 0..d[1] {
                    for i in 0..d[0] {
                        let idx = [i, j, k];
                        // The four cells round the edge, with their areas.
                        let mut round = Vec::with_capacity(4);
                        for du in [-1isize, 0] {
                            for dv in [-1isize, 0] {
                                let mut c = idx.map(|x| x as isize);
                                c[u] += du;
                                c[v] += dv;
                                let (cu, cv) = (c[u] as usize, c[v] as usize);
                                if let Some(m) = self.cell(c) {
                                    round.push((material(m), steps.primal[u][cu] * steps.primal[v][cv]));
                                }
                            }
                        }
                        if round.len() < 4 || self.thin_conductor(model, a, idx) {
                            continue;
                        }
                        let area: f64 = round.iter().map(|(_, s)| s).sum();
                        let eps = round.iter().map(|(m, s)| s * m.eps_r.get()).sum::<f64>() / area;
                        let sigma = round.iter().map(|(m, s)| s * losses.of(m).0).sum::<f64>() / area;
                        let eps = eps * exponential_fit(sigma * dt / (EPS0 * eps));
                        let id = index(d, i, j, k);
                        ce[id] = dt / (EPS0 * eps);
                        if sigma > 0.0 || round.iter().any(|(m, _)| m.is_dispersive()) {
                            let terms = round.iter().flat_map(|(m, s)| m.dispersion.iter().map(move |p| Term::new(*p, s / area, dt)));
                            dispersion[a].add_edge(id, EPS0 * eps, 0.5 * sigma * dt, terms);
                        }
                    }
                }
            }
            ce
        });
        for d in &mut dispersion {
            d.finish();
        }
        let (ch, h_decay) = crate::open::face_coefficients(model, h_dims, dt, losses, 0.0, |c| self.cell(c).map(material));
        BoxCoefficients { dt, ce, ch, h_decay, dispersion }
    }

    /// Whether the E edge along `a` at node `idx` has its midpoint in a conductor inside the
    /// solid's box.
    fn thin_conductor(&self, model: &Model, a: usize, idx: [usize; 3]) -> bool {
        let Some(b) = self.solid else { return false };
        let mid = [0, 1, 2].map(|d| if d == a { 0.5 * (self.lines[d][idx[d]] + self.lines[d][idx[d] + 1]) } else { self.lines[d][idx[d]] });
        (0..3).all(|d| mid[d] > b.min[d] && mid[d] < b.max[d]) && model.material_at(Point3::new(mid[0], mid[1], mid[2])).is_none()
    }

    /// The CPML at the ends of the axes that have one: coefficients for E (at nodes) and H (at
    /// cell centres).
    fn cpml(&self, dt: f64, fc: f64) -> BoxCpml {
        let profile = |d: usize, xs: Vec<f64>| -> Vec<Cpml> {
            let x = &self.lines[d];
            let n = x.len() - 1;
            let [lo, hi] = self.pml[d];
            let side = |cells: usize, inner: f64, outer: f64| {
                (cells > 0).then(|| (inner, (inner - outer).abs(), Grading::new((inner - outer).abs() / cells as f64, dt, fc)))
            };
            let low = side(lo, x[lo], x[0]);
            let high = side(hi, x[n - hi], x[n]);
            xs.iter()
                .map(|&p| match (&low, &high) {
                    (Some((inner, depth, g)), _) if p < *inner => g.at((inner - p) / depth),
                    (_, Some((inner, depth, g))) if p > *inner => g.at((p - inner) / depth),
                    _ => Cpml::NONE,
                })
                .collect()
        };
        let e = [0, 1, 2].map(|d| profile(d, self.lines[d].clone()));
        let h = [0, 1, 2].map(|d| profile(d, self.lines[d].windows(2).map(|w| 0.5 * (w[0] + w[1])).collect()));
        BoxCpml { e, h }
    }
}

/// The index of the node at `x` along `lines` (the nearest).
fn node_of(lines: &[f64], x: f64) -> usize {
    lines.iter().enumerate().min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs())).map_or(0, |(i, _)| i)
}

/// The TE₁₀ profile of a port on its face (or source) plane: E along the narrow side's edges,
/// sin(π s/a) across the broad side; the edges' indices and weights, and the projection's
/// weights (the profile times each edge's dual area).
struct Profile {
    narrow: usize,
    edges: Vec<(usize, f64)>,
    weights: Vec<f64>,
}

impl Profile {
    fn new(g: &SolidGrid, steps: &Steps, p: &FacePort, plane: usize) -> Profile {
        let n = g.n();
        let dims = lattices(n)[0][p.narrow];
        let (s0, s1) = p.span[p.broad];
        let (t0, t1) = p.span[p.narrow];
        let mut edges = Vec::new();
        let mut weights = Vec::new();
        let (bl, nl) = (&g.lines[p.broad], &g.lines[p.narrow]);
        for (i, &x) in bl.iter().enumerate().filter(|(_, x)| **x > s0 && **x < s1) {
            for j in 0..nl.len() - 1 {
                let t = 0.5 * (nl[j] + nl[j + 1]);
                if !(t > t0 && t < t1) {
                    continue;
                }
                let mut idx = [0usize; 3];
                idx[p.axis] = plane;
                idx[p.broad] = i;
                idx[p.narrow] = j;
                let w = (PI * (x - s0) / (s1 - s0)).sin();
                edges.push((index(dims, idx[0], idx[1], idx[2]), w));
                weights.push(w * steps.dual[p.broad][i] * steps.primal[p.narrow][j]);
            }
        }
        Profile { narrow: p.narrow, edges, weights }
    }

    /// The projection of E onto the profile.
    fn project(&self, e: &crate::yee::Field) -> f64 {
        self.edges.iter().zip(&self.weights).map(|((id, _), w)| e.get(*id) * w).sum()
    }
}

/// What every run of a solve shares.
struct Run<'a> {
    model: &'a Model,
    o: &'a Options,
    frequencies: &'a [f64],
    dt: f64,
    losses: Losses,
    pulse: Pulse,
    t_min: f64,
}

impl Run<'_> {
    /// One run on `g`: the pulse launched at `source`'s source plane, the TE₁₀ projections at
    /// the faces of `ports` transformed; every [`CHECK`] steps past `t_min` they are checked
    /// against the incident wave's `scale` (when given) and the run stops once they settle.
    /// Returns the transforms (per port, per frequency), the steps and whether they settled.
    fn go(&self, g: &SolidGrid, source: &FacePort, ports: &[FacePort], scale: Option<f64>) -> Result<(Vec<Vec<C>>, usize, bool)> {
        let steps = Steps::of(&g.lines);
        let coef = g.coefficients(self.model, &steps, self.dt, self.losses);
        let cpml = g.cpml(self.dt, Band::of(self.frequencies).fc);
        let which = ports.iter().position(|p| std::ptr::eq(p, source)).unwrap_or(0);
        let src = Profile::new(g, &steps, source, g.sources.get(which).copied().unwrap_or(0));
        let planes: Vec<Profile> = ports.iter().zip(&g.planes).map(|(p, &k)| Profile::new(g, &steps, p, k)).collect();
        let fields = Fields::new(g.n());
        let psi = BoxPsi::new(&fields);
        let kernel = OpenKernel { fields: &fields, coef: &coef, cpml: &cpml, psi: &psi, steps: &steps, dt: self.dt };
        let nf = self.frequencies.len();
        let advance: Vec<C> = self.frequencies.iter().map(|f| C::from_polar(1.0, -angular(*f) * self.dt)).collect();
        let mut phasor = advance.clone();
        let mut sums = vec![vec![C::new(0.0, 0.0); nf]; planes.len()];
        let mut settling = Settling::new(self.o.decay);
        let (mut converged, mut failed, mut grew) = (false, false, false);
        let ce = &coef.ce[src.narrow];
        let e_src = &fields.e[src.narrow];
        let taken = leapfrog(&kernel, g.n()[2] + 1, self.o.max_steps, |n| {
            let s = self.pulse.at((n + 1) as f64 * self.dt);
            for &(id, w) in &src.edges {
                if ce.get(id).is_some_and(|c| *c != 0.0) {
                    e_src.set(id, e_src.get(id) + s * w);
                }
            }
            for (sum, plane) in sums.iter_mut().zip(&planes) {
                let v = plane.project(&fields.e[plane.narrow]);
                failed |= !v.is_finite();
                for (x, p) in sum.iter_mut().zip(&phasor) {
                    *x += *p * v;
                }
            }
            for (p, a) in phasor.iter_mut().zip(&advance) {
                *p *= a;
            }
            if failed {
                return ControlFlow::Break(());
            }
            let t = (n + 1) as f64 * self.dt;
            if (n + 1).is_multiple_of(CHECK) && t > self.t_min {
                let now: Vec<C> = sums.iter().flatten().copied().collect();
                let peak = now.iter().fold(0.0f64, |m, c| m.max(c.norm()));
                // Waves out of a passive device stay below the incident one: far beyond it the
                // run is growing (a mode trapped below cut-off feeding the absorbers).
                if scale.is_some_and(|s| peak > GROWTH * s) {
                    grew = true;
                    return ControlFlow::Break(());
                }
                if settling.settled(now, scale.unwrap_or(peak)) {
                    converged = true;
                    return ControlFlow::Break(());
                }
            }
            ControlFlow::Continue(())
        });
        if failed {
            return Err(invalid("run", "the fields diverged"));
        }
        if grew {
            return Err(invalid(
                "run",
                "the fields grew without bound before they settled (energy trapped below the ports' cut-off reaching the absorbers): start the band further above cut-off or use a smaller cell",
            ));
        }
        Ok((sums, taken, converged))
    }
}

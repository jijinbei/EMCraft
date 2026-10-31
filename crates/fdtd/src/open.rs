//! Open problems in the time domain: a box solid on a uniform Yee grid, a convolutional PML
//! (Roden & Gedney 2000) lining all six faces, driven by current elements and lumped ports;
//! the ports' voltages and currents and the fields on a Huygens box are Fourier-transformed on
//! the fly at the requested frequencies, giving input impedances and, by the same near-to-far
//! transform as the frequency-domain solver, far fields and radiated power.
//!
//! **Geometry.** Each cell takes the material at its centre ([`Model::material_at`]); each E
//! edge is a conductor's (its update off) when its midpoint lies in a conductor or a cell next
//! to it does, so a wire thinner than a cell still stands as a line of conducting edges (a
//! staircase; a one-cell wire behaves like one of radius about 0.135 h — K. R. Umashankar, A.
//! Taflove, B. Beker, IEEE Trans. AP 35(11), 1987).
//!
//! **Materials.** Each E edge takes the average of its four cells' ε∞, conductivity and poles,
//! each H face the average of its two cells' μ and magnetic conductivity, updated as in the
//! guide (the crate's documentation: loss tangents as conductivities matched at the band's
//! centre, the exponential update, dispersion by auxiliary differential equations). A current
//! element on a lossy or dispersive edge enters its solve, the edge's terms following; lumped
//! ports, whose semi-implicit update assumes a plain edge, are refused there. Conductors are
//! perfect: finite wall conductivity is refused.
//!
//! **Sources.** A current element of moment I·l on the grid edge nearest its centre and along
//! its axis (moment kept: the edge carries I·l/h). A lumped port is a resistive voltage source
//! on the run of grid edges along its gap (M. Piket-May, A. Taflove, J. Baron, *FD-TD modeling
//! of digital signal propagation in 3-D circuits with passive and active loads*, IEEE Trans.
//! MTT 42(8):1514–1523, 1994): with I = (V_s + Σ h E)/R through it, semi-implicit in E. All
//! sources share one Gaussian-modulated pulse; impedances are ratios of transforms, so its
//! spectrum cancels, and fields are reported per volt (or ampere) of source spectrum.

use std::f64::consts::PI;
use std::ops::{ControlFlow, Range};

use emcraft_model::units::{C0, EPS0, ETA0, MU0};
use emcraft_model::{CVec3, Material, Model, Point3, Shape, Source, Vec3};
use emcraft_theory::radiation::{Huygens, HuygensSample};
use num_complex::Complex64 as C;

use crate::device::{Axis, CurrentPlan, OpenPlan, OpenStepper, PortEdgePlan, PortPlan, SamplePlan};
use crate::dispersion::{Dispersion, Poles, Term, check_poles, exponential_fit, magnetic_update};
use crate::yee::{Band, Cpml, Field, Fields, Grading, Kernel, Pulse, Settling, decay, index, lattices, layer_profile, leapfrog, time_step};
use crate::{FdtdError, Losses, MAX_CELLS, MAX_STEPS, Result, frequencies_ok, invalid};

/// Run settings of an open problem.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpenOptions {
    /// The cell size (m).
    pub cell: f64,
    pub courant: f64,
    /// Stop when no transform moves by more than this fraction of its largest in a check.
    pub decay: f64,
    pub max_steps: usize,
}

impl OpenOptions {
    pub fn new(cell: f64) -> OpenOptions {
        OpenOptions { cell, courant: 0.99, decay: 1e-4, max_steps: 100_000 }
    }

    /// The settings checked: a finite cell > 0, a Courant number in (0, 1), 1 to [`MAX_STEPS`]
    /// steps and a decay > 0.
    pub fn validate(&self) -> Result<()> {
        let h = self.cell;
        if !(h.is_finite() && h > 0.0)
            || !(self.courant > 0.0 && self.courant < 1.0)
            || self.max_steps == 0
            || self.max_steps > MAX_STEPS
            || !(self.decay > 0.0)
        {
            return Err(invalid("options", format!("{self:?}")));
        }
        Ok(())
    }
}

/// A lumped port's result at each frequency.
#[derive(Clone, Debug, PartialEq)]
pub struct PortSpectrum {
    pub name: String,
    /// Terminal voltage and current per unit of source spectrum, and their ratio.
    pub voltage: Vec<C>,
    pub current: Vec<C>,
    pub impedance: Vec<C>,
    /// The power it delivers (W) for 1 V of source at each frequency.
    pub power: Vec<f64>,
}

/// The result of an open run.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenRun {
    pub frequencies: Vec<f64>,
    pub ports: Vec<PortSpectrum>,
    /// Power through the Huygens box at each frequency, for 1 V (or 1 A·m) of source spectrum.
    pub radiated: Vec<f64>,
    /// The fields on the Huygens box at each frequency, per unit of source spectrum.
    pub huygens: Vec<Huygens>,
    pub cells: usize,
    pub steps: usize,
    pub dt: f64,
    pub converged: bool,
}

impl OpenRun {
    /// r e^{jkr} (E_θ, E_φ) at frequency index `fi` in direction (θ, φ), per unit of source
    /// spectrum (the same near-to-far transform as the frequency-domain solver).
    pub fn far_field(&self, fi: usize, theta: f64, phi: f64) -> Option<(C, C)> {
        Some(self.huygens.get(fi)?.far_field(theta, phi))
    }

    /// Directivity at frequency index `fi` in direction (θ, φ).
    pub fn directivity(&self, fi: usize, theta: f64, phi: f64) -> Option<f64> {
        let (a, b) = self.far_field(fi, theta, phi)?;
        let p = *self.radiated.get(fi)?;
        Some(4.0 * PI * (a.norm_sqr() + b.norm_sqr()) / (2.0 * ETA0 * p))
    }
}

/// The two axes across `a`, in cyclic order.
#[inline]
const fn across(a: usize) -> (usize, usize) {
    ((a + 1) % 3, (a + 2) % 3)
}

/// The uniform grid of the box: `n` cells of `hs` along each axis, `npml` of them in the layer
/// at each face.
struct BoxGrid {
    size: Vec3,
    n: [usize; 3],
    hs: [f64; 3],
    npml: usize,
}

impl BoxGrid {
    /// The grid of the model's box solid at cell size `h`, refusing what the time domain's open
    /// problems do not support.
    fn new(model: &Model, h: f64) -> Result<BoxGrid> {
        let (Some(open), Some(solid)) = (&model.open, &model.solid) else {
            return Err(FdtdError::NotReady("no open boundary: a box solid lined by model.open".into()));
        };
        let Shape::Box { size } = solid.shape else {
            return Err(FdtdError::NotReady("the time domain's open problems need a box solid".into()));
        };
        if model.wall_conductivity.is_some() {
            return Err(FdtdError::Unsupported(
                "walls of finite conductivity in the time domain's open problems (its conductors are perfect; clear model.walls)".into(),
            ));
        }
        if !model.ports.is_empty() {
            return Err(FdtdError::Unsupported("waveguide ports in the time domain's open problems (use lumped ports)".into()));
        }
        if model.sources.iter().any(|s| matches!(s, Source::PlaneWave { .. })) {
            return Err(FdtdError::Unsupported("plane waves in the time domain".into()));
        }
        let n = [0, 1, 2].map(|d| ((size[d] / h).round() as usize).max(4));
        let cells = n[0].saturating_mul(n[1]).saturating_mul(n[2]);
        if cells > MAX_CELLS {
            return Err(FdtdError::TooLarge { cells, limit: MAX_CELLS });
        }
        let hs = [0, 1, 2].map(|d| size[d] / n[d] as f64);
        let npml = ((open.thickness / h).round() as usize).max(4);
        if (0..3).any(|d| 2 * npml + 4 > n[d]) {
            return Err(invalid("open boundary", "the layer leaves no room inside the box at this cell size"));
        }
        Ok(BoxGrid { size: size.get(), n, hs, npml })
    }

    fn cells(&self) -> usize {
        self.n[0] * self.n[1] * self.n[2]
    }

    /// The position of (fractional) node `i` along axis `d`.
    fn node(&self, d: usize, i: f64) -> f64 {
        i * self.hs[d]
    }

    /// The cross-section of an edge along `a`.
    fn area(&self, a: usize) -> f64 {
        let (u, v) = across(a);
        self.hs[u] * self.hs[v]
    }

    /// The update coefficients of the model's box for a time step `dt`, the loss tangents
    /// realised by `losses`.
    ///
    /// dt/ε per E edge: 0 on conductors and the box's walls, where an edge's midpoint or any of
    /// the four cells round it is a conductor's; ε the average of those cells', and their
    /// conductivities and poles, a quarter each, on the edge's slot (the exponential update with
    /// conductivity, the auxiliary equations with poles, as in the guide). dt/μ per H face: μ the
    /// average of the cells on either side that are not conductors (μ₀ if neither), with
    /// magnetic losses the exponential update and its decay.
    fn coefficients(&self, model: &Model, dt: f64, losses: Losses) -> Result<BoxCoefficients> {
        let [e_dims, h_dims] = lattices(self.n);
        let n = self.n;

        check_poles(model.materials.iter().flat_map(|m| m.dispersion.iter()), dt)?;
        // Materials per cell: an index into the model's, or `None` in a conductor.
        let mut cell_of = vec![None; self.cells()];
        for k in 0..n[2] {
            for j in 0..n[1] {
                for i in 0..n[0] {
                    let p = Point3::new(self.node(0, i as f64 + 0.5), self.node(1, j as f64 + 0.5), self.node(2, k as f64 + 0.5));
                    cell_of[i + n[0] * (j + n[1] * k)] = model.material_at(p);
                }
            }
        }
        let vacuum = Material::vacuum();
        // The material of cell c: `None` outside the grid or in a conductor.
        let cell = |c: [isize; 3]| -> Option<&Material> {
            if (0..3).any(|d| c[d] < 0 || c[d] as usize >= n[d]) {
                return None;
            }
            cell_of[c[0] as usize + n[0] * (c[1] as usize + n[1] * c[2] as usize)].map(|m| model.materials.get(m).unwrap_or(&vacuum))
        };
        let mut dispersion = [Dispersion::none(), Dispersion::none(), Dispersion::none()];
        let ce = [0, 1, 2].map(|a| {
            let (u, v) = across(a);
            let d = e_dims[a];
            let len = d.iter().product();
            let mut ce = vec![0.0; len];
            for k in 0..d[2] {
                for j in 0..d[1] {
                    for i in 0..d[0] {
                        let idx = [i, j, k];
                        let mid = Point3::from([0, 1, 2].map(|q| self.node(q, idx[q] as f64 + if q == a { 0.5 } else { 0.0 })));
                        if model.material_at(mid).is_none() {
                            continue;
                        }
                        // The four cells round the edge: offsets −1, 0 along u and v.
                        let mut round = Vec::with_capacity(4);
                        for du in [-1isize, 0] {
                            for dv in [-1isize, 0] {
                                let mut c = idx.map(|x| x as isize);
                                c[u] += du;
                                c[v] += dv;
                                round.extend(cell(c));
                            }
                        }
                        if round.len() < 4 {
                            continue;
                        }
                        let eps = 0.25 * round.iter().map(|m| m.eps_r.get()).sum::<f64>();
                        let sigma = 0.25 * round.iter().map(|m| losses.of(m).0).sum::<f64>();
                        let eps = eps * exponential_fit(sigma * dt / (EPS0 * eps));
                        let id = index(d, i, j, k);
                        ce[id] = dt / (EPS0 * eps);
                        if sigma > 0.0 || round.iter().any(|m| m.is_dispersive()) {
                            let terms = round.iter().flat_map(|m| m.dispersion.iter().map(|p| Term::new(*p, 0.25, dt)));
                            dispersion[a].add_edge(len, id, EPS0 * eps, 0.5 * sigma * dt, terms);
                        }
                    }
                }
            }
            ce
        });
        for d in &mut dispersion {
            d.finish();
        }
        let magnetic = model.materials.iter().any(|m| m.is_magnetically_lossy());
        let mut h_decay = [Vec::new(), Vec::new(), Vec::new()];
        let ch = [0, 1, 2].map(|a| {
            let d = h_dims[a];
            let mut ch = vec![dt / MU0; d.iter().product()];
            let mut decay = if magnetic { vec![1.0; ch.len()] } else { Vec::new() };
            for k in 0..d[2] {
                for j in 0..d[1] {
                    for i in 0..d[0] {
                        let b = [i, j, k].map(|x| x as isize);
                        let mut c = b;
                        c[a] -= 1;
                        let sides: Vec<&Material> = [c, b].into_iter().filter_map(cell).collect();
                        if sides.is_empty() {
                            continue;
                        }
                        let count = sides.len() as f64;
                        let mu = MU0 * sides.iter().map(|m| m.mu_r.get()).sum::<f64>() / count;
                        let sigma_m = sides.iter().map(|m| losses.of(m).1).sum::<f64>() / count;
                        let id = index(d, i, j, k);
                        let (c, x) = magnetic_update(dt, mu, sigma_m);
                        ch[id] = c;
                        if let Some(y) = decay.get_mut(id) {
                            *y = x;
                        }
                    }
                }
            }
            h_decay[a] = decay;
            ch
        });
        Ok(BoxCoefficients { ce, ch, h_decay, dispersion })
    }
}

/// The open box's update coefficients: dt/ε per E edge with the slots of the dispersive and
/// lossy ones, dt/μ per H face and its decay with magnetic losses (empty without).
struct BoxCoefficients {
    ce: [Vec<f64>; 3],
    ch: [Vec<f64>; 3],
    h_decay: [Vec<f64>; 3],
    dispersion: [Dispersion; 3],
}

/// A lumped port's edge with its semi-implicit coefficients (Piket-May et al. 1994):
/// Eⁿ⁺¹ = ca Eⁿ + cb (∇ × H) − cv V_s.
struct PortEdge {
    id: usize,
    ca: f64,
    cb: f64,
    cv: f64,
    /// E on the edge before the step's plain update.
    old: f64,
}

/// A lumped port on the grid: its run of edges along `axis`.
struct GridPort {
    name: String,
    axis: Axis,
    edges: Vec<PortEdge>,
    /// Ohms per edge, and the sign of the gap's direction along the axis.
    r_edge: f64,
    sign: f64,
    /// The source voltage per edge.
    voltage: f64,
}

impl GridPort {
    /// The model's lumped port `lp` as a run of edges between grid nodes.
    fn new(lp: &emcraft_model::LumpedPort, grid: &BoxGrid, coef: &BoxCoefficients, dt: f64) -> Result<GridPort> {
        let ce = &coef.ce;
        if lp.voltage[1] != 0.0 {
            return Err(FdtdError::Unsupported("lumped ports with a phase in the time domain (their voltages must be real)".into()));
        }
        let (hs, n) = (grid.hs, grid.n);
        let d = lp.to - lp.from;
        let axis = d.dominant_axis();
        if (0..3).any(|q| q != axis && d[q].abs() > 1e-6 * d[axis].abs()) {
            return Err(invalid("lumped port", format!("`{}` must lie along a grid axis in the time domain", lp.name)));
        }
        let snap = |p: Point3| [0, 1, 2].map(|q| ((p[q] / hs[q]).round().max(0.0) as usize).min(n[q]));
        let (a, b) = (snap(lp.from), snap(lp.to));
        let (lo, hi) = (a[axis].min(b[axis]), a[axis].max(b[axis]));
        if hi == lo {
            return Err(invalid("lumped port", format!("`{}`'s gap is shorter than a cell", lp.name)));
        }
        let dims = lattices(n)[0][axis];
        let ids: Vec<usize> = (lo..hi)
            .map(|t| {
                let mut c = a;
                c[axis] = t;
                index(dims, c[0], c[1], c[2])
            })
            .collect();
        if ids.iter().any(|&id| ce[axis][id] == 0.0) {
            return Err(invalid("lumped port", format!("`{}` crosses a conductor on the grid", lp.name)));
        }
        if ids.iter().any(|&id| coef.dispersion[axis].has(id)) {
            return Err(FdtdError::Unsupported(format!(
                "a lumped port in a lossy or dispersive material in the time domain (`{}`: put its gap in a lossless one)",
                lp.name
            )));
        }
        let count = ids.len() as f64;
        let r_edge = lp.impedance / count;
        let area = grid.area(axis);
        let edges = ids
            .into_iter()
            .map(|id| {
                let eps = dt / ce[axis][id]; // ε (F/m)
                let g = hs[axis] / (2.0 * r_edge * area);
                let den = eps / dt + g;
                PortEdge { id, ca: (eps / dt - g) / den, cb: 1.0 / den, cv: 1.0 / (r_edge * area) / den, old: 0.0 }
            })
            .collect();
        let sign = if b[axis] > a[axis] { 1.0 } else { -1.0 };
        Ok(GridPort { name: lp.name.clone(), axis: Axis::try_from(axis)?, edges, r_edge, sign, voltage: lp.voltage[0] / count })
    }

    /// Redoes the port's edges semi-implicitly after the plain update, with the source at `s`;
    /// returns the terminal voltage and the current along the gap.
    fn update(&self, e: &Field, ce: &[f64], h: f64, s: f64) -> (f64, f64) {
        let mut vsum = 0.0;
        let mut isum = 0.0;
        for edge in &self.edges {
            let (c, old) = (ce[edge.id], edge.old);
            // The plain update added c·curl (with the layer's ψ, zero here): recover it.
            let curl = (e.get(edge.id) - old) / c;
            let vs = self.voltage * s * self.sign;
            let updated = edge.ca * old + edge.cb * curl - edge.cv * vs;
            e.set(edge.id, updated);
            let e_avg = 0.5 * (old + updated);
            isum += (vs + h * e_avg) / self.r_edge * self.sign;
            vsum += -h * e_avg * self.sign;
        }
        (vsum, isum / self.edges.len() as f64)
    }

    /// Keeps E on the port's edges before the next step's plain update.
    fn remember(&mut self, e: &Field) {
        for edge in &mut self.edges {
            edge.old = e.get(edge.id);
        }
    }
}

/// A current element on one E edge, carrying its moment (A, per unit of source): E gains
/// −`gain` × moment × s/area after each step (`gain` dt/ε, or on a lossy or dispersive edge the
/// slot's [`Dispersion::gain`] times dt, its terms following).
struct Current {
    axis: Axis,
    id: usize,
    moment: f64,
    gain: f64,
}

/// The model's current elements, each on the grid edge nearest its centre.
fn currents(model: &Model, grid: &BoxGrid, coef: &BoxCoefficients, dt: f64) -> Result<Vec<Current>> {
    let hs = grid.hs;
    let mut out = Vec::new();
    for s in &model.sources {
        if let Source::Dipole { at, direction, length, current } = *s {
            let axis = direction.dominant_axis();
            if (0..3).any(|q| q != axis && direction[q].abs() > 1e-9) {
                return Err(invalid("dipole", "must lie along a grid axis in the time domain"));
            }
            let dims = lattices(grid.n)[0][axis];
            let c = [0, 1, 2].map(|q| ((at[q] / hs[q] - if q == axis { 0.5 } else { 0.0 }).round().max(0.0) as usize).min(dims[q] - 1));
            let id = index(dims, c[0], c[1], c[2]);
            let gain = coef.dispersion[axis].gain(id).map_or(coef.ce[axis][id], |g| g * dt);
            out.push(Current { axis: Axis::try_from(axis)?, id, moment: current * length * direction[axis].signum() / hs[axis], gain });
        }
    }
    Ok(out)
}

/// The CPML lining the six faces: coefficients per axis for E (at nodes) and H (at half
/// cells).
struct BoxCpml {
    e: [Vec<Cpml>; 3],
    h: [Vec<Cpml>; 3],
}

impl BoxCpml {
    fn new(grid: &BoxGrid, dt: f64, fc: f64) -> BoxCpml {
        let npml = grid.npml;
        // Coefficients at nodes (or half cells) along an axis of `n` cells.
        // In cells along the axis: the layer's inner faces at npml and n − npml.
        let axis = |d: usize, half: bool| -> Vec<Cpml> {
            let n = grid.n[d];
            let grading = Grading::new(grid.hs[d], dt, fc);
            let count = if half { n } else { n + 1 };
            let xs = (0..count).map(|i| i as f64 + if half { 0.5 } else { 0.0 });
            layer_profile(&grading, xs, (npml as f64, (n - npml) as f64), npml as f64)
        };
        BoxCpml { e: [0, 1, 2].map(|d| axis(d, false)), h: [0, 1, 2].map(|d| axis(d, true)) }
    }
}

/// Each component's CPML auxiliary variables for its two derivatives.
struct BoxPsi {
    e: [[Field; 2]; 3],
    h: [[Field; 2]; 3],
}

impl BoxPsi {
    fn new(fields: &Fields) -> BoxPsi {
        let psi = |f: &Field| [f.zeros_like(), f.zeros_like()];
        BoxPsi { e: fields.e.each_ref().map(psi), h: fields.h.each_ref().map(psi) }
    }
}

/// The leapfrog of the box: uniform steps, CPML on every derivative.
struct OpenKernel<'a> {
    fields: &'a Fields,
    coef: &'a BoxCoefficients,
    cpml: &'a BoxCpml,
    psi: &'a BoxPsi,
    hs: [f64; 3],
    dt: f64,
}

impl OpenKernel<'_> {
    /// ∂H_a/∂t = −(1/μ) (∇ × E)_a, (∇ × E)_a = ∂E_v/∂u − ∂E_u/∂v, H_a at (a: node, u, v: half).
    fn step_h_comp<const A: usize>(&self, ks: Range<usize>) {
        let (u, v) = across(A);
        let (h, [eu, ev]) = (&self.fields.h[A], [&self.fields.e[u], &self.fields.e[v]]);
        let [p0, p1] = &self.psi.h[A];
        let (pu, pv) = (&self.cpml.h[u], &self.cpml.h[v]);
        let (su, sv) = (ev.stride(u), eu.stride(v));
        let (ch, h_decay) = (&self.coef.ch[A], &self.coef.h_decay[A]);
        let d = h.dims();
        for k in ks.start.min(d[2])..ks.end.min(d[2]) {
            for j in 0..d[1] {
                for i in 0..d[0] {
                    let idx = [i, j, k];
                    let id = h.at(i, j, k);
                    let (iv, iu) = (ev.at(i, j, k), eu.at(i, j, k));
                    let dev_du = (ev.get(iv + su) - ev.get(iv)) / self.hs[u];
                    let deu_dv = (eu.get(iu + sv) - eu.get(iu)) / self.hs[v];
                    let q0 = pu[idx[u]].psi(p0.get(id), dev_du);
                    let q1 = pv[idx[v]].psi(p1.get(id), deu_dv);
                    p0.set(id, q0);
                    p1.set(id, q1);
                    h.set(id, decay(h_decay, id) * h.get(id) - ch[id] * (dev_du + q0 - deu_dv - q1));
                }
            }
        }
    }

    /// ∂E_a/∂t = (1/ε) (∇ × H)_a, (∇ × H)_a = ∂H_v/∂u − ∂H_u/∂v, E_a at (a: half, u, v: nodes).
    fn step_e_comp<const A: usize>(&self, ks: Range<usize>) {
        let (u, v) = across(A);
        let (e, [hu, hv]) = (&self.fields.e[A], [&self.fields.h[u], &self.fields.h[v]]);
        let [p0, p1] = &self.psi.e[A];
        let (pu, pv) = (&self.cpml.e[u], &self.cpml.e[v]);
        let (su, sv) = (hv.stride(u), hu.stride(v));
        let (ce, disp) = (&self.coef.ce[A], &self.coef.dispersion[A]);
        let d = e.dims();
        for k in ks.start.min(d[2])..ks.end.min(d[2]) {
            for j in 0..d[1] {
                for i in 0..d[0] {
                    let id = e.at(i, j, k);
                    let c = ce[id];
                    let idx = [i, j, k];
                    if c == 0.0 || idx[u] == 0 || idx[v] == 0 {
                        continue;
                    }
                    let (iv, iu) = (hv.at(i, j, k), hu.at(i, j, k));
                    let dhv_du = (hv.get(iv) - hv.get(iv - su)) / self.hs[u];
                    let dhu_dv = (hu.get(iu) - hu.get(iu - sv)) / self.hs[v];
                    let q0 = pu[idx[u]].psi(p0.get(id), dhv_du);
                    let q1 = pv[idx[v]].psi(p1.get(id), dhu_dv);
                    p0.set(id, q0);
                    p1.set(id, q1);
                    let (old, curl) = (e.get(id), dhv_du + q0 - dhu_dv - q1);
                    e.set(id, disp.step(id, old, curl, self.dt).unwrap_or(old + c * curl));
                }
            }
        }
    }
}

impl Kernel for OpenKernel<'_> {
    fn step_h(&self, ks: Range<usize>) {
        self.step_h_comp::<0>(ks.clone());
        self.step_h_comp::<1>(ks.clone());
        self.step_h_comp::<2>(ks);
    }

    /// The ports' edges get the plain update too; it is redone semi-implicitly afterwards.
    fn step_e(&self, ks: Range<usize>) {
        self.step_e_comp::<0>(ks.clone());
        self.step_e_comp::<1>(ks.clone());
        self.step_e_comp::<2>(ks);
    }
}

/// A sample of the Huygens box: the centre of a cell face on the box's face normal to `axis`
/// at node `plane`, at cells (iu, iv) across.
struct BoxSample {
    point: Point3,
    weight: f64,
    normal: Vec3,
    axis: Axis,
    plane: usize,
    iu: usize,
    iv: usize,
}

/// The Huygens box: its samples and centre.
struct HuygensBox {
    samples: Vec<BoxSample>,
    centre: Point3,
}

impl HuygensBox {
    /// A third of the layer inside it, or halfway to what radiates.
    fn new(model: &Model, grid: &BoxGrid) -> Result<HuygensBox> {
        let BoxGrid { size, n, hs, npml } = *grid;
        let inner_lo = [0, 1, 2].map(|d| npml as f64 * hs[d]);
        let inner_hi = [0, 1, 2].map(|d| size[d] - npml as f64 * hs[d]);
        // (A run always has a source, so the box is never empty.)
        let emcraft_model::Aabb { min: clo, max: chi } = emcraft_model::Aabb::around(model.radiators()).unwrap_or_default();
        let inset = npml as f64 / 3.0;
        let mut hlo = [0usize; 3];
        let mut hhi = [0usize; 3];
        for d in 0..3 {
            let lo_nodes = npml as f64 + inset.min(0.5 * (clo[d] - inner_lo[d]) / hs[d]);
            let hi_nodes = (n[d] - npml) as f64 - inset.min(0.5 * (inner_hi[d] - chi[d]) / hs[d]);
            hlo[d] = lo_nodes.floor().max(npml as f64 + 1.0) as usize;
            hhi[d] = (hi_nodes.ceil() as usize).min(n[d] - npml - 1);
            if hhi[d] <= hlo[d] + 1 || clo[d] <= hlo[d] as f64 * hs[d] || chi[d] >= hhi[d] as f64 * hs[d] {
                return Err(invalid("open boundary", "the bodies and sources reach into or up to the layer; enlarge the box"));
            }
        }
        // Cell-face centres on the six faces.
        let samples = Axis::ALL
            .into_iter()
            .flat_map(|axis| [(axis, -1.0, hlo[axis.index()]), (axis, 1.0, hhi[axis.index()])])
            .flat_map(|(axis, side, plane)| {
                let (u, v) = across(axis.index());
                (hlo[u]..hhi[u]).flat_map(move |iu| (hlo[v]..hhi[v]).map(move |iv| (axis, side, plane, iu, iv)))
            })
            .map(|(axis, side, plane, iu, iv)| {
                let a = axis.index();
                let (u, v) = across(a);
                let point = Point3::ORIGIN.with(a, plane as f64 * hs[a]).with(u, (iu as f64 + 0.5) * hs[u]).with(v, (iv as f64 + 0.5) * hs[v]);
                BoxSample { point, weight: hs[u] * hs[v], normal: Vec3::unit(a) * side, axis, plane, iu, iv }
            })
            .collect();
        let centre = Point3::from([0, 1, 2].map(|d| 0.5 * (hlo[d] + hhi[d]) as f64 * hs[d]));
        Ok(HuygensBox { samples, centre })
    }
}

impl BoxSample {
    /// The tangential E and H at the sample: each component averaged over the lattice points
    /// straddling the face centre.
    fn fields(&self, f: &Fields) -> (Vec3, Vec3) {
        let (a, plane, iu, iv) = (self.axis.index(), self.plane, self.iu, self.iv);
        let (u, v) = across(a);
        // E_u at (u half, v node) → average over v and v + 1; E_v likewise.
        let mut e_idx = [0isize; 3];
        e_idx[a] = plane as isize;
        e_idx[u] = iu as isize;
        e_idx[v] = iv as isize;
        let eu = interp(&f.e[u], e_idx, &[v]);
        let ev = interp(&f.e[v], e_idx, &[u]);
        // H_u at (u node, v half, a half) → average over u and a; H_v likewise; H at a ± ½
        // straddles the plane.
        let mut h_idx = [0isize; 3];
        h_idx[a] = plane as isize - 1;
        h_idx[u] = iu as isize;
        h_idx[v] = iv as isize;
        let hu = interp(&f.h[u], h_idx, &[u, a]);
        let hv = interp(&f.h[v], h_idx, &[v, a]);
        (Vec3::ZERO.with(u, eu).with(v, ev), Vec3::ZERO.with(u, hu).with(v, hv))
    }
}

/// A field at a point of a component's lattice, averaged over the positions straddling it along
/// `dirs` (the staggering to the cell-face centre), skipping positions off the lattice's low end.
fn interp(h: &Field, idx: [isize; 3], dirs: &[usize]) -> f64 {
    let mut sum = 0.0;
    let mut count = 0.0;
    for mask in 0..1usize << dirs.len() {
        let mut c = idx;
        for (b, &d) in dirs.iter().enumerate() {
            if mask >> b & 1 == 1 {
                c[d] += 1;
            }
        }
        if c.iter().all(|x| *x >= 0) {
            sum += h.get(h.at(c[0] as usize, c[1] as usize, c[2] as usize));
            count += 1.0;
        }
    }
    if count > 0.0 { sum / count } else { 0.0 }
}

/// The running Fourier transforms of a run, per frequency: the source, the ports' voltages and
/// currents, and the tangential fields on the Huygens box. Each step's phasor is e^{−jωt} Δt,
/// E sampled at t = (n + 1) Δt, H, the source and the ports at (n + ½) Δt.
struct Transforms {
    /// The frequencies (Hz) and the time step (s).
    freqs: Vec<f64>,
    dt: f64,
    source: Vec<C>,
    voltage: Vec<Vec<C>>,
    current: Vec<Vec<C>>,
    e: Vec<Vec<CVec3>>,
    h: Vec<Vec<CVec3>>,
    /// The step's tangential fields at the samples (scratch).
    at_samples: Vec<(Vec3, Vec3)>,
}

impl Transforms {
    fn new(freqs: &[f64], dt: f64, ports: usize, samples: usize) -> Transforms {
        let nf = freqs.len();
        Transforms {
            freqs: freqs.to_vec(),
            dt,
            source: vec![C::new(0.0, 0.0); nf],
            voltage: vec![vec![C::new(0.0, 0.0); nf]; ports],
            current: vec![vec![C::new(0.0, 0.0); nf]; ports],
            e: vec![vec![CVec3::ZERO; samples]; nf],
            h: vec![vec![CVec3::ZERO; samples]; nf],
            at_samples: Vec::with_capacity(samples),
        }
    }

    /// Adds step `step`'s samples: the source `s` and the ports' (voltage, current) at t_h, the
    /// fields on the box.
    fn add(&mut self, step: usize, s: f64, ports: &[(f64, f64)], hbox: &HuygensBox, fields: &Fields) {
        self.add_sources(step, s, ports);
        let dt = self.dt;
        let t_e = (step as f64 + 1.0) * dt;
        let t_h = (step as f64 + 0.5) * dt;
        self.at_samples.clear();
        self.at_samples.extend(hbox.samples.iter().map(|x| x.fields(fields)));
        for (fi, &f) in self.freqs.iter().enumerate() {
            let w = 2.0 * PI * f;
            let pe = C::from_polar(dt, -w * t_e);
            let ph = C::from_polar(dt, -w * t_h);
            for ((de, dh), &(e, h)) in self.e[fi].iter_mut().zip(self.h[fi].iter_mut()).zip(&self.at_samples) {
                *de += e * pe;
                *dh += h * ph;
            }
        }
    }

    /// Adds step `step`'s source `s` and ports' (voltage, current), at t_h.
    fn add_sources(&mut self, step: usize, s: f64, ports: &[(f64, f64)]) {
        let dt = self.dt;
        let t_h = (step as f64 + 0.5) * dt;
        for (fi, &f) in self.freqs.iter().enumerate() {
            let ph = C::from_polar(dt, -2.0 * PI * f * t_h);
            self.source[fi] += ph * s;
            for (pi, (v, i)) in ports.iter().enumerate() {
                self.voltage[pi][fi] += ph * *v;
                self.current[pi][fi] += ph * *i;
            }
        }
    }

    /// The box's transforms as a device reports them (per frequency, per sample, re and im of
    /// x, y, z).
    fn set_fields(&mut self, e: &[f64], h: &[f64]) {
        let samples = self.e.first().map_or(0, Vec::len);
        let at = |a: &[f64], k: usize| CVec3 { x: C::new(a[k], a[k + 1]), y: C::new(a[k + 2], a[k + 3]), z: C::new(a[k + 4], a[k + 5]) };
        for (fi, (de, dh)) in self.e.iter_mut().zip(self.h.iter_mut()).enumerate() {
            for (si, (de, dh)) in de.iter_mut().zip(dh.iter_mut()).enumerate() {
                let k = 6 * (fi * samples + si);
                if k + 6 <= e.len() && k + 6 <= h.len() {
                    *de = at(e, k);
                    *dh = at(h, k);
                }
            }
        }
    }

    /// What settles: the ports' transforms and a proxy of the box's.
    fn settling(&self) -> Vec<C> {
        self.voltage
            .iter()
            .chain(&self.current)
            .flatten()
            .copied()
            .chain(self.e.iter().map(|x| x.iter().map(|v| v.x + v.y + v.z).sum::<C>()))
            .collect()
    }
}

/// Steps between checks of whether the transforms have settled.
const CHECK: usize = 200;

/// The checks after each step: divergence, and every [`CHECK`] steps once the pulse is past,
/// whether the transforms have settled.
struct Checks {
    dt: f64,
    t_min: f64,
    settling: Settling,
    converged: bool,
    diverged: bool,
}

impl Checks {
    fn after(&mut self, step: usize, port_vi: &[(f64, f64)], tr: &Transforms) -> ControlFlow<()> {
        if !port_vi.iter().all(|(v, i)| v.is_finite() && i.is_finite()) || !tr.source.iter().all(|c| c.re.is_finite()) {
            self.diverged = true;
            return ControlFlow::Break(());
        }
        if (step + 1).is_multiple_of(CHECK) && (step as f64 + 1.0) * self.dt > self.t_min {
            let now = tr.settling();
            let scale = now.iter().fold(0.0f64, |m, c| m.max(c.norm())).max(f64::MIN_POSITIVE);
            if self.settling.settled(now, scale) {
                self.converged = true;
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    }
}

/// What a device's plan is made of: the box's grid, its coefficients (moved into the plan), its
/// CPML, the sources and the Huygens box.
struct OpenRunParts<'a> {
    grid: &'a BoxGrid,
    coef: BoxCoefficients,
    cpml: &'a BoxCpml,
    ports: &'a [GridPort],
    currents: &'a [Current],
    hbox: &'a HuygensBox,
}

impl OpenRunParts<'_> {
    /// The run prepared for a device at `frequencies`.
    fn plan(self, frequencies: &[f64], pulse: Pulse, dt: f64) -> OpenPlan {
        let OpenRunParts { grid, coef, cpml, ports, currents, hbox } = self;
        OpenPlan {
            n: grid.n,
            hs: grid.hs,
            dt,
            poles: Poles::of(&coef.dispersion, coef.ce.each_ref().map(Vec::len)),
            ce: coef.ce,
            ch: coef.ch,
            h_decay: coef.h_decay,
            cpml_e: cpml.e.clone(),
            cpml_h: cpml.h.clone(),
            ports: ports
                .iter()
                .map(|p| PortPlan {
                    axis: p.axis,
                    edges: p.edges.iter().map(|e| PortEdgePlan { id: e.id, ca: e.ca, cb: e.cb, cv: e.cv }).collect(),
                    r_edge: p.r_edge,
                    sign: p.sign,
                    voltage: p.voltage,
                })
                .collect(),
            currents: currents
                .iter()
                .map(|c| CurrentPlan { axis: c.axis, id: c.id, coefficient: c.gain * c.moment / grid.area(c.axis.index()) })
                .collect(),
            samples: hbox.samples.iter().map(|x| SamplePlan { axis: x.axis, plane: x.plane, iu: x.iu, iv: x.iv }).collect(),
            frequencies: frequencies.to_vec(),
            batch: CHECK,
            pulse,
        }
    }
}

/// Solves the model's open problem in the time domain at `frequencies` (Hz).
pub fn radiate(model: &Model, o: &OpenOptions, frequencies: &[f64]) -> Result<OpenRun> {
    radiate_with(model, o, frequencies, None)
}

/// [`radiate`] with the leapfrog on `device` (the CPU's threads when `None`).
pub fn radiate_with(model: &Model, o: &OpenOptions, frequencies: &[f64], device: Option<&dyn OpenStepper>) -> Result<OpenRun> {
    o.validate()?;
    if !frequencies_ok(frequencies) || frequencies.len() > 64 {
        return Err(invalid("frequencies", "1 to 64 finite numbers > 0"));
    }
    let grid = BoxGrid::new(model, o.cell)?;
    let band = Band::of(frequencies);
    let dt = time_step(o.courant, grid.hs);
    let coef = grid.coefficients(model, dt, Losses::new(band.fc))?;
    let mut ports = model.lumped.iter().map(|lp| GridPort::new(lp, &grid, &coef, dt)).collect::<Result<Vec<_>>>()?;
    let currents = currents(model, &grid, &coef, dt)?;
    let ce = &coef.ce;
    if ports.is_empty() && currents.is_empty() {
        return Err(FdtdError::NotReady("no sources: a dipole or a lumped port".into()));
    }
    let cpml = BoxCpml::new(&grid, dt, band.fc);
    // The pulse: Gaussian-modulated over the band.
    let pulse = Pulse::over(band, 0.4);
    let hbox = HuygensBox::new(model, &grid)?;

    let mut tr = Transforms::new(frequencies, dt, ports.len(), hbox.samples.len());
    let t_min = pulse.t0 + 4.0 * pulse.tau + 3.0 * grid.size.iter().fold(0.0f64, f64::max) / C0;
    let mut checks = Checks { dt, t_min, settling: Settling::new(o.decay), converged: false, diverged: false };
    let steps = match device {
        Some(device) => {
            let plan = OpenRunParts { grid: &grid, coef, cpml: &cpml, ports: &ports, currents: &currents, hbox: &hbox }.plan(frequencies, pulse, dt);
            let mut vi = Vec::with_capacity(plan.ports.len());
            device.radiate(&plan, o.max_steps, &mut |batch| {
                // The box's transforms stand as of the batch's end, where the check falls.
                tr.set_fields(batch.e, batch.h);
                let np = plan.ports.len();
                for b in 0..batch.steps {
                    let step = batch.first + b;
                    vi.clear();
                    vi.extend(batch.ports.get(b * np..(b + 1) * np).unwrap_or(&[]).iter().map(|[v, i]| (*v, *i)));
                    tr.add_sources(step, plan.source_at(step), &vi);
                    checks.after(step, &vi, &tr)?;
                }
                ControlFlow::Continue(())
            })?
        }
        None => {
            let fields = Fields::new(grid.n);
            let psi = BoxPsi::new(&fields);
            let kernel = OpenKernel { fields: &fields, coef: &coef, cpml: &cpml, psi: &psi, hs: grid.hs, dt };
            let mut port_vi = Vec::with_capacity(ports.len());
            leapfrog(&kernel, grid.n[2] + 1, o.max_steps, |step| {
                let s_now = pulse.at((step as f64 + 0.5) * dt);
                // Lumped ports: redo their edges semi-implicitly; then the current elements.
                port_vi.clear();
                port_vi.extend(ports.iter().map(|p| {
                    let a = p.axis.index();
                    p.update(&fields.e[a], &ce[a], grid.hs[a], s_now)
                }));
                for c in &currents {
                    let a = c.axis.index();
                    let e = &fields.e[a];
                    let delta = -c.gain * c.moment * s_now / grid.area(a);
                    if !coef.dispersion[a].inject(e, c.id, delta) {
                        e.set(c.id, e.get(c.id) + delta);
                    }
                }
                tr.add(step, s_now, &port_vi, &hbox, &fields);
                checks.after(step, &port_vi, &tr)?;
                for p in &mut ports {
                    p.remember(&fields.e[p.axis.index()]);
                }
                ControlFlow::Continue(())
            })
        }
    };
    if checks.diverged {
        return Err(invalid("run", "the fields diverged"));
    }
    let (ports, radiated, huygens) = normalise(frequencies, &ports, tr, hbox)?;
    Ok(OpenRun { frequencies: frequencies.to_vec(), ports, radiated, huygens, cells: grid.cells(), steps, dt, converged: checks.converged })
}

/// The ports' spectra, the radiated power and the Huygens box's fields: the transforms
/// normalised by the source spectrum.
fn normalise(frequencies: &[f64], ports: &[GridPort], tr: Transforms, hbox: HuygensBox) -> Result<(Vec<PortSpectrum>, Vec<f64>, Vec<Huygens>)> {
    let ds = &tr.source;
    let huygens = frequencies
        .iter()
        .zip(ds.iter().zip(tr.e.iter().zip(&tr.h)))
        .map(|(&frequency, (&s, (ef, hf)))| {
            if s.norm() == 0.0 {
                return Err(invalid("frequency", "outside the pulse's band"));
            }
            let samples = hbox
                .samples
                .iter()
                .zip(ef.iter().zip(hf))
                .map(|(x, (e, h))| HuygensSample { point: x.point, weight: x.weight, normal: x.normal, e: *e / s, h: *h / s })
                .collect();
            Ok(Huygens { frequency, centre: hbox.centre, samples })
        })
        .collect::<Result<Vec<_>>>()?;
    let radiated = huygens.iter().map(Huygens::power).collect();
    let ports = ports
        .iter()
        .zip(tr.voltage.iter().zip(&tr.current))
        .map(|(p, (dvp, dip))| {
            let voltage: Vec<C> = dvp.iter().zip(ds).map(|(v, s)| v / s).collect();
            let current: Vec<C> = dip.iter().zip(ds).map(|(i, s)| i / s).collect();
            let impedance = voltage.iter().zip(&current).map(|(v, i)| v / i).collect();
            let power = voltage.iter().zip(&current).map(|(v, i)| 0.5 * (v * i.conj()).re).collect();
            PortSpectrum { name: p.name.clone(), voltage, current, impedance, power }
        })
        .collect();
    Ok((ports, radiated, huygens))
}

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
//! **Sources.** A current element of moment I·l on the grid edge nearest its centre and along
//! its axis (moment kept: the edge carries I·l/h). A lumped port is a resistive voltage source
//! on the run of grid edges along its gap (M. Piket-May, A. Taflove, J. Baron, *FD-TD modeling
//! of digital signal propagation in 3-D circuits with passive and active loads*, IEEE Trans.
//! MTT 42(8):1514–1523, 1994): with I = (V_s + Σ h E)/R through it, semi-implicit in E. All
//! sources share one Gaussian-modulated pulse; impedances are ratios of transforms, so its
//! spectrum cancels, and fields are reported per volt (or ampere) of source spectrum.

use std::f64::consts::PI;

use emcraft_model::units::{C0, EPS0, ETA0, MU0};
use emcraft_model::{CVec3, Model, Point3, Shape, Source, Vec3};
use emcraft_theory::radiation::{Huygens, HuygensSample};
use num_complex::Complex64 as C;

use crate::{FdtdError, MAX_CELLS, MAX_STEPS, Result, invalid};

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

/// A field component on the grid: dims (nx, ny, nz) of its own lattice, row-major in x.
struct Comp {
    n: [usize; 3],
    v: Vec<f64>,
}

impl Comp {
    fn new(n: [usize; 3]) -> Comp {
        Comp { n, v: vec![0.0; n[0] * n[1] * n[2]] }
    }
    #[inline]
    fn id(&self, i: usize, j: usize, k: usize) -> usize {
        i + self.n[0] * (j + self.n[1] * k)
    }
    #[inline]
    fn get(&self, i: usize, j: usize, k: usize) -> f64 {
        self.v[self.id(i, j, k)]
    }
}

/// CPML coefficients (b, a) at positions `pos` (in cells from the low face, may be half-integer)
/// on an axis of `n` cells with `npml` PML cells at each end.
fn cpml_axis(n: usize, npml: usize, h: f64, dt: f64, fc: f64, half: bool) -> Vec<(f64, f64)> {
    let m = 3.0;
    let sigma_max = 0.8 * (m + 1.0) / (ETA0 * h);
    let alpha_max = 2.0 * PI * fc * EPS0 * 0.05;
    let count = if half { n } else { n + 1 };
    (0..count)
        .map(|i| {
            let x = i as f64 + if half { 0.5 } else { 0.0 };
            let d = if x < npml as f64 {
                (npml as f64 - x) / npml as f64
            } else if x > (n - npml) as f64 {
                (x - (n - npml) as f64) / npml as f64
            } else {
                0.0
            };
            if d <= 0.0 {
                return (1.0, 0.0);
            }
            let sigma = sigma_max * d.powf(m);
            let alpha = alpha_max * (1.0 - d);
            let b = (-(sigma + alpha) * dt / EPS0).exp();
            let a = if sigma > 0.0 { sigma / (sigma + alpha) * (b - 1.0) } else { 0.0 };
            (b, a)
        })
        .collect()
}

/// Runs `f` on every value of a component (dims `d`) and its two ψ arrays, the z planes split
/// into slabs over `threads` scoped threads.
fn par_planes(
    threads: usize,
    d: [usize; 3],
    out: &mut [f64],
    p0: &mut [f64],
    p1: &mut [f64],
    f: &(dyn Fn(usize, usize, usize, &mut f64, &mut f64, &mut f64) + Sync),
) {
    let plane = d[0] * d[1];
    if plane == 0 || d[2] == 0 {
        return;
    }
    let slab = d[2].div_ceil(threads.max(1)).max(1);
    let run = |k0: usize, o: &mut [f64], q0: &mut [f64], q1: &mut [f64]| {
        for (n, ((x, y), z)) in o.iter_mut().zip(q0.iter_mut()).zip(q1.iter_mut()).enumerate() {
            let (i, j, k) = (n % d[0], (n / d[0]) % d[1], k0 + n / plane);
            f(i, j, k, x, y, z);
        }
    };
    if threads <= 1 || d[2] < 2 * threads {
        run(0, out, p0, p1);
        return;
    }
    std::thread::scope(|scope| {
        for (t, ((o, q0), q1)) in out.chunks_mut(slab * plane).zip(p0.chunks_mut(slab * plane)).zip(p1.chunks_mut(slab * plane)).enumerate() {
            let run = &run;
            scope.spawn(move || run(t * slab, o, q0, q1));
        }
    });
}

/// A lumped port on the grid: its edges (axis, i, j, k), each's resistance and source share.
struct GridPort {
    name: String,
    axis: usize,
    edges: Vec<[usize; 3]>,
    /// Ohms per edge, and the sign of the gap's direction along the axis.
    r_edge: f64,
    sign: f64,
    voltage: C,
}

/// Solves the model's open problem in the time domain at `frequencies` (Hz).
pub fn radiate(model: &Model, o: &OpenOptions, frequencies: &[f64]) -> Result<OpenRun> {
    let h = o.cell;
    if !(h.is_finite() && h > 0.0) || !(o.courant > 0.0 && o.courant < 1.0) || o.max_steps == 0 || o.max_steps > MAX_STEPS || !(o.decay > 0.0) {
        return Err(invalid("options", format!("{o:?}")));
    }
    if frequencies.is_empty() || frequencies.len() > 64 || frequencies.iter().any(|f| !(f.is_finite() && *f > 0.0)) {
        return Err(invalid("frequencies", "1 to 64 finite numbers > 0"));
    }
    let (Some(open), Some(solid)) = (&model.open, &model.solid) else {
        return Err(FdtdError::NotReady("no open boundary: a box solid lined by model.open".into()));
    };
    let Shape::Box { size } = solid.shape else {
        return Err(FdtdError::NotReady("the time domain's open problems need a box solid".into()));
    };
    if model.materials.iter().any(|m| m.loss_tangent != 0.0 || m.mu_r != 1.0) || model.wall_conductivity.is_some() {
        return Err(FdtdError::Unsupported("losses or magnetic materials in the time domain's open problems".into()));
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
    let fmin = frequencies.iter().copied().fold(f64::INFINITY, f64::min);
    let fmax = frequencies.iter().copied().fold(0.0f64, f64::max);
    let fc = 0.5 * (fmin + fmax);
    let dt = o.courant / (C0 * (1.0 / hs[0].powi(2) + 1.0 / hs[1].powi(2) + 1.0 / hs[2].powi(2)).sqrt());

    // Materials per cell; conductors per E edge.
    let node = |d: usize, i: f64| i * hs[d];
    let mut eps_cell = vec![0.0; cells];
    let mut pec_cell = vec![false; cells];
    for k in 0..n[2] {
        for j in 0..n[1] {
            for i in 0..n[0] {
                let p = Point3::new(node(0, i as f64 + 0.5), node(1, j as f64 + 0.5), node(2, k as f64 + 0.5));
                let c = i + n[0] * (j + n[1] * k);
                match model.material_at(p) {
                    Some(m) => eps_cell[c] = model.materials.get(m).map_or(1.0, |x| x.eps_r),
                    None => pec_cell[c] = true,
                }
            }
        }
    }
    let cell_ok = |i: isize, j: isize, k: isize| -> Option<usize> {
        if i < 0 || j < 0 || k < 0 || i as usize >= n[0] || j as usize >= n[1] || k as usize >= n[2] {
            return None;
        }
        let c = i as usize + n[0] * (j as usize + n[1] * k as usize);
        (!pec_cell[c]).then_some(c)
    };
    // E components: Ex (nx, ny+1, nz+1), Ey (nx+1, ny, nz+1), Ez (nx+1, ny+1, nz).
    let dims_e = [[n[0], n[1] + 1, n[2] + 1], [n[0] + 1, n[1], n[2] + 1], [n[0] + 1, n[1] + 1, n[2]]];
    let dims_h = [[n[0] + 1, n[1], n[2]], [n[0], n[1] + 1, n[2]], [n[0], n[1], n[2] + 1]];
    let mut e: Vec<Comp> = dims_e.iter().map(|d| Comp::new(*d)).collect();
    let mut hf: Vec<Comp> = dims_h.iter().map(|d| Comp::new(*d)).collect();
    // dt/ε per E edge (0 on conductors and the box's walls).
    let mut ce: Vec<Vec<f64>> = dims_e.iter().map(|d| vec![0.0; d[0] * d[1] * d[2]]).collect();
    for a in 0..3 {
        let (u, v) = ((a + 1) % 3, (a + 2) % 3);
        let d = dims_e[a];
        for k in 0..d[2] {
            for j in 0..d[1] {
                for i in 0..d[0] {
                    let idx = [i, j, k];
                    let mid = Point3::from([0, 1, 2].map(|q| node(q, idx[q] as f64 + if q == a { 0.5 } else { 0.0 })));
                    if model.material_at(mid).is_none() {
                        continue;
                    }
                    // The four cells round the edge: offsets −1, 0 along u and v.
                    let mut eps = 0.0;
                    let mut ok = true;
                    for du in [-1isize, 0] {
                        for dv in [-1isize, 0] {
                            let mut c = [idx[0] as isize, idx[1] as isize, idx[2] as isize];
                            c[u] += du;
                            c[v] += dv;
                            match cell_ok(c[0], c[1], c[2]) {
                                Some(id) => eps += 0.25 * eps_cell[id],
                                None => ok = false,
                            }
                        }
                    }
                    if ok {
                        ce[a][i + d[0] * (j + d[1] * k)] = dt / (EPS0 * eps);
                    }
                }
            }
        }
    }
    let chh = dt / MU0;

    // Lumped ports: runs of edges along an axis between grid nodes.
    let mut ports = Vec::new();
    for lp in &model.lumped {
        if lp.voltage[1] != 0.0 {
            return Err(FdtdError::Unsupported("lumped ports with a phase in the time domain (their voltages must be real)".into()));
        }
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
        let edges: Vec<[usize; 3]> = (lo..hi)
            .map(|t| {
                let mut c = a;
                c[axis] = t;
                c
            })
            .collect();
        for c in &edges {
            let id = c[0] + dims_e[axis][0] * (c[1] + dims_e[axis][1] * c[2]);
            if ce[axis][id] == 0.0 {
                return Err(invalid("lumped port", format!("`{}` crosses a conductor on the grid", lp.name)));
            }
        }
        let sign = if b[axis] > a[axis] { 1.0 } else { -1.0 };
        let count = edges.len() as f64;
        ports.push(GridPort {
            name: lp.name.clone(),
            axis,
            edges,
            r_edge: lp.impedance / count,
            sign,
            voltage: C::new(lp.voltage[0], lp.voltage[1]) / count,
        });
    }
    // Current elements: one edge each, carrying the moment.
    let mut currents: Vec<(usize, [usize; 3], f64)> = Vec::new();
    for s in &model.sources {
        if let Source::Dipole { at, direction, length, current } = *s {
            let axis = direction.dominant_axis();
            if (0..3).any(|q| q != axis && direction[q].abs() > 1e-9) {
                return Err(invalid("dipole", "must lie along a grid axis in the time domain"));
            }
            let c = [0, 1, 2].map(|q| ((at[q] / hs[q] - if q == axis { 0.5 } else { 0.0 }).round().max(0.0) as usize).min(dims_e[axis][q] - 1));
            currents.push((axis, c, current * length * direction[axis].signum() / hs[axis]));
        }
    }
    if ports.is_empty() && currents.is_empty() {
        return Err(FdtdError::NotReady("no sources: a dipole or a lumped port".into()));
    }
    // Port edges' semi-implicit coefficients: (ε/dt − h/(2RA))/(ε/dt + h/(2RA)) etc. per edge.
    struct PortEdge {
        id: usize,
        ca: f64,
        cb: f64,
        cv: f64,
    }
    let port_edges: Vec<Vec<PortEdge>> = ports
        .iter()
        .map(|p| {
            let a = p.axis;
            let area = hs[(a + 1) % 3] * hs[(a + 2) % 3];
            p.edges
                .iter()
                .map(|c| {
                    let id = c[0] + dims_e[a][0] * (c[1] + dims_e[a][1] * c[2]);
                    let eps = dt / ce[a][id]; // ε (F/m)
                    let g = hs[a] / (2.0 * p.r_edge * area);
                    let den = eps / dt + g;
                    PortEdge { id, ca: (eps / dt - g) / den, cb: 1.0 / den, cv: 1.0 / (p.r_edge * area) / den }
                })
                .collect()
        })
        .collect();

    // CPML profiles: E at nodes, H at half cells, per axis.
    let pe: Vec<Vec<(f64, f64)>> = (0..3).map(|d| cpml_axis(n[d], npml, hs[d], dt, fc, false)).collect();
    let ph: Vec<Vec<(f64, f64)>> = (0..3).map(|d| cpml_axis(n[d], npml, hs[d], dt, fc, true)).collect();
    // ψ for each component and each of its two derivatives.
    let mut psi_e: Vec<[Vec<f64>; 2]> = dims_e.iter().map(|d| [vec![0.0; d[0] * d[1] * d[2]], vec![0.0; d[0] * d[1] * d[2]]]).collect();
    let mut psi_h: Vec<[Vec<f64>; 2]> = dims_h.iter().map(|d| [vec![0.0; d[0] * d[1] * d[2]], vec![0.0; d[0] * d[1] * d[2]]]).collect();

    // The pulse: Gaussian-modulated over [fmin, fmax].
    let bw = (fmax - fmin).max(0.4 * fc);
    let tau = 1.5 / (PI * 0.5 * bw);
    let t0 = 4.0 * tau;
    let pulse = |t: f64| (-((t - t0) / tau).powi(2)).exp() * (2.0 * PI * fc * (t - t0)).sin();

    // The Huygens box: a third of the layer inside it, or halfway to what radiates.
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
    // Huygens samples: cell-face centres on the six faces, each sample a (axis, side, u, v).
    let (samples, sample_at): (Vec<(Point3, f64, Vec3)>, Vec<(usize, usize, usize, usize)>) = (0..3)
        .flat_map(|a| [(a, -1.0, hlo[a]), (a, 1.0, hhi[a])])
        .flat_map(|(a, side, plane)| {
            let (u, v) = ((a + 1) % 3, (a + 2) % 3);
            (hlo[u]..hhi[u]).flat_map(move |iu| (hlo[v]..hhi[v]).map(move |iv| (a, side, plane, u, v, iu, iv)))
        })
        .map(|(a, side, plane, u, v, iu, iv)| {
            let p = Point3::ORIGIN.with(a, plane as f64 * hs[a]).with(u, (iu as f64 + 0.5) * hs[u]).with(v, (iv as f64 + 0.5) * hs[v]);
            ((p, hs[u] * hs[v], Vec3::unit(a) * side), (a, plane, iu, iv))
        })
        .unzip();
    let centre = Point3::from([0, 1, 2].map(|d| 0.5 * (hlo[d] + hhi[d]) as f64 * hs[d]));
    let nf = frequencies.len();
    let mut de = vec![vec![CVec3::ZERO; samples.len()]; nf];
    let mut dh = vec![vec![CVec3::ZERO; samples.len()]; nf];
    let mut dv = vec![vec![C::new(0.0, 0.0); nf]; ports.len()];
    let mut di = vec![vec![C::new(0.0, 0.0); nf]; ports.len()];
    let mut ds = vec![C::new(0.0, 0.0); nf];
    // E at a point of a component's lattice, averaged over the two positions straddling it
    // along `dir` (the staggering to the cell-face centre).
    let interp_e = |e: &[Comp], comp: usize, idx: [usize; 3], dirs: &[usize]| -> f64 {
        let mut sum = 0.0;
        let mut count = 0.0;
        let combos = 1usize << dirs.len();
        for mask in 0..combos {
            let mut c = idx;
            for (b, &d) in dirs.iter().enumerate() {
                if mask >> b & 1 == 1 {
                    c[d] += 1;
                }
            }
            sum += e[comp].get(c[0], c[1], c[2]);
            count += 1.0;
        }
        sum / count
    };
    let interp_h = |hf: &[Comp], comp: usize, idx: [isize; 3], dirs: &[usize]| -> f64 {
        let mut sum = 0.0;
        let mut count = 0.0;
        for mask in 0..(1usize << dirs.len()) {
            let mut c = idx;
            for (b, &d) in dirs.iter().enumerate() {
                if mask >> b & 1 == 1 {
                    c[d] += 1;
                }
            }
            if c.iter().all(|x| *x >= 0) {
                sum += hf[comp].get(c[0] as usize, c[1] as usize, c[2] as usize);
                count += 1.0;
            }
        }
        if count > 0.0 { sum / count } else { 0.0 }
    };

    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(16);
    let mut steps = 0;
    let mut converged = false;
    let mut last: Option<Vec<C>> = None;
    let t_min = t0 + 4.0 * tau + 3.0 * size.iter().fold(0.0f64, f64::max) / C0;
    let check = 200;
    for step in 0..o.max_steps {
        // H half step: ∂H/∂t = −(1/μ) ∇ × E, with CPML on the two derivatives of each component;
        // threads take slabs of z planes (each value its own, so any split gives the same bits).
        for a in 0..3 {
            let (u, v) = ((a + 1) % 3, (a + 2) % 3);
            let d = dims_h[a];
            let (er, phr) = (&e, &ph);
            let [q0, q1] = &mut psi_h[a];
            par_planes(threads, d, &mut hf[a].v, q0, q1, &|i, j, k, h, p0, p1| {
                let idx = [i, j, k];
                // H_a at (a: node, u, v: half). (∇×E)_a = ∂E_v/∂u − ∂E_u/∂v.
                let mut pu = idx;
                pu[u] += 1;
                let mut pv = idx;
                pv[v] += 1;
                let dev_du = (er[v].get(pu[0], pu[1], pu[2]) - er[v].get(i, j, k)) / hs[u];
                let deu_dv = (er[u].get(pv[0], pv[1], pv[2]) - er[u].get(i, j, k)) / hs[v];
                let (bu, au) = phr[u][idx[u]];
                let (bv, av) = phr[v][idx[v]];
                *p0 = bu * *p0 + au * dev_du;
                *p1 = bv * *p1 + av * deu_dv;
                *h -= chh * (dev_du + *p0 - deu_dv - *p1);
            });
        }
        let t_h = (step as f64 + 0.5) * dt;
        // E full step: ∂E/∂t = (1/ε)(∇ × H − J). The ports' edges keep their old values to be
        // redone semi-implicitly.
        let s_now = pulse(t_h);
        let olds: Vec<Vec<f64>> = ports.iter().zip(&port_edges).map(|(p, pes)| pes.iter().map(|x| e[p.axis].v[x.id]).collect()).collect();
        for a in 0..3 {
            let (u, v) = ((a + 1) % 3, (a + 2) % 3);
            let d = dims_e[a];
            let (hr, per, cea) = (&hf, &pe, &ce[a]);
            let [q0, q1] = &mut psi_e[a];
            par_planes(threads, d, &mut e[a].v, q0, q1, &|i, j, k, ev, p0, p1| {
                let c = cea[i + d[0] * (j + d[1] * k)];
                let idx = [i, j, k];
                // E_a at (a: half, u, v: nodes); (∇×H)_a = ∂H_v/∂u − ∂H_u/∂v.
                if c == 0.0 || idx[u] == 0 || idx[v] == 0 {
                    return;
                }
                let mut mu_ = idx;
                mu_[u] -= 1;
                let mut mv = idx;
                mv[v] -= 1;
                let dhv_du = (hr[v].get(i, j, k) - hr[v].get(mu_[0], mu_[1], mu_[2])) / hs[u];
                let dhu_dv = (hr[u].get(i, j, k) - hr[u].get(mv[0], mv[1], mv[2])) / hs[v];
                let (bu, au) = per[u][idx[u]];
                let (bv, av) = per[v][idx[v]];
                *p0 = bu * *p0 + au * dhv_du;
                *p1 = bv * *p1 + av * dhu_dv;
                *ev += c * (dhv_du + *p0 - dhu_dv - *p1);
            });
        }
        // Lumped ports: redo their edges semi-implicitly (undo the plain update first).
        let mut port_vi = Vec::with_capacity(ports.len());
        for ((p, pes), old_values) in ports.iter().zip(&port_edges).zip(&olds) {
            let a = p.axis;
            let mut vsum = 0.0;
            let mut isum = 0.0;
            for (pe_, &old) in pes.iter().zip(old_values) {
                let c = ce[a][pe_.id];
                // The plain update added c·curl (with the layer's ψ, zero here): recover it.
                let curl = (e[a].v[pe_.id] - old) / c;
                let vs = p.voltage.re * s_now * p.sign;
                let updated = pe_.ca * old + pe_.cb * curl - pe_.cv * vs;
                e[a].v[pe_.id] = updated;
                let e_avg = 0.5 * (old + updated);
                // Current along the gap's direction and the terminal voltage.
                isum += (vs + hs[a] * e_avg) / p.r_edge * p.sign;
                vsum += -hs[a] * e_avg * p.sign;
            }
            port_vi.push((vsum, isum / pes.len() as f64));
        }
        // Current elements.
        for &(a, c, moment) in &currents {
            let id = e[a].id(c[0], c[1], c[2]);
            let area = hs[(a + 1) % 3] * hs[(a + 2) % 3];
            e[a].v[id] -= ce[a][id] * moment * s_now / area;
        }
        // Transforms: E at t = (n+1) dt, H at (n+½) dt, ports at (n+½) dt.
        let t_e = (step as f64 + 1.0) * dt;
        for (fi, &f) in frequencies.iter().enumerate() {
            let w = 2.0 * PI * f;
            let pe_ = C::from_polar(dt, -w * t_e);
            let ph_ = C::from_polar(dt, -w * t_h);
            ds[fi] += ph_ * s_now;
            for (pi, (v, i)) in port_vi.iter().enumerate() {
                dv[pi][fi] += ph_ * *v;
                di[pi][fi] += ph_ * *i;
            }
            for (si, &(a, plane, iu, iv)) in sample_at.iter().enumerate() {
                let (u, v) = ((a + 1) % 3, (a + 2) % 3);
                // E tangential: E_u at (u half, v node) → average over v and v+1; E_v likewise.
                let mut eu_idx = [0usize; 3];
                eu_idx[a] = plane;
                eu_idx[u] = iu;
                eu_idx[v] = iv;
                let eu = interp_e(&e, u, eu_idx, &[v]);
                let ev = interp_e(&e, v, eu_idx, &[u]);
                // H tangential: H_u at (u node, v half, a half) → average over u and a; H_v
                // likewise; H at a ± ½ straddles the plane.
                let mut hu_idx = [0isize; 3];
                hu_idx[a] = plane as isize - 1;
                hu_idx[u] = iu as isize;
                hu_idx[v] = iv as isize;
                let hu = interp_h(&hf, u, hu_idx, &[u, a]);
                let hv = interp_h(&hf, v, hu_idx, &[v, a]);
                de[fi][si] += Vec3::ZERO.with(u, eu).with(v, ev) * pe_;
                dh[fi][si] += Vec3::ZERO.with(u, hu).with(v, hv) * ph_;
            }
        }
        steps = step + 1;
        let t = t_e;
        if !port_vi.iter().all(|(v, i)| v.is_finite() && i.is_finite()) || !ds.iter().all(|c| c.re.is_finite()) {
            return Err(invalid("run", "the fields diverged"));
        }
        if steps % check == 0 && t > t_min {
            // Settled: the ports' transforms (or the box's power proxy) stop moving.
            let now: Vec<C> =
                dv.iter().chain(&di).flatten().copied().chain(de.iter().map(|x| x.iter().map(|v| v.x + v.y + v.z).sum::<C>())).collect();
            let scale = now.iter().fold(0.0f64, |m, c| m.max(c.norm())).max(f64::MIN_POSITIVE);
            if let Some(prev) = &last
                && prev.iter().zip(&now).all(|(a, b)| (a - b).norm() <= o.decay * scale)
            {
                converged = true;
                break;
            }
            last = Some(now);
        }
    }
    // Normalise by the source spectrum and assemble.
    let huygens = frequencies
        .iter()
        .zip(ds.iter().zip(de.iter().zip(&dh)))
        .map(|(&frequency, (&s, (ef, hf)))| {
            if s.norm() == 0.0 {
                return Err(invalid("frequency", "outside the pulse's band"));
            }
            let samples = samples
                .iter()
                .zip(ef.iter().zip(hf))
                .map(|(&(point, weight, normal), (e, h))| HuygensSample { point, weight, normal, e: *e / s, h: *h / s })
                .collect();
            Ok(Huygens { frequency, centre, samples })
        })
        .collect::<Result<Vec<_>>>()?;
    let radiated = huygens.iter().map(Huygens::power).collect();
    let port_out = ports
        .iter()
        .zip(dv.iter().zip(&di))
        .map(|(p, (dvp, dip))| {
            let voltage: Vec<C> = dvp.iter().zip(&ds).map(|(v, s)| v / s).collect();
            let current: Vec<C> = dip.iter().zip(&ds).map(|(i, s)| i / s).collect();
            let impedance = voltage.iter().zip(&current).map(|(v, i)| v / i).collect();
            let power = voltage.iter().zip(&current).map(|(v, i)| 0.5 * (v * i.conj()).re).collect();
            PortSpectrum { name: p.name.clone(), voltage, current, impedance, power }
        })
        .collect();
    Ok(OpenRun { frequencies: frequencies.to_vec(), ports: port_out, radiated, huygens, cells, steps, dt, converged })
}

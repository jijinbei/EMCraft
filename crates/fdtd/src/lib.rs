//! EMCraft's time-domain solver: the finite integration technique on a tensor grid (T. Weiland,
//! *A discretization method for the solution of Maxwell's equations for six-component fields*,
//! AEÜ 31:116–120, 1977), which on a rectangular grid is the Yee scheme (K. S. Yee, *Numerical
//! solution of initial boundary value problems involving Maxwell's equations in isotropic
//! media*, IEEE Trans. Antennas Propag. 14(3):302–307, 1966): E on the edges of the primal grid,
//! H on the edges of the dual grid, leapfrogged in time. Grid lines need not be evenly spaced:
//! the curls use the primal edge lengths and the dual ones, and each E edge takes the
//! area-weighted permittivity of the cells around it.
//!
//! **Ports.** Each end of the guide is extended by its end section: a feed of a few cells and a
//! convolutional PML (J. A. Roden, S. D. Gedney, *Convolution PML (CPML): an efficient FDTD
//! implementation of the CFS–PML for arbitrary media*, Microw. Opt. Technol. Lett.
//! 27(5):334–339, 2000) that absorbs what leaves along z. A soft source with the TE₁₀ profile
//! in the port-1 feed launches a Gaussian-modulated pulse covering the band.
//!
//! **S-parameters.** The TE₁₀ amplitude is projected out of E_y on the model's end planes, z = 0
//! and z = L, every step and Fourier-transformed on the fly. A second run with the port-1 guide
//! uniform throughout gives the incident wave at z = 0 alone; then S₁₁ = (V₁ − V₁ʳᵉᶠ)/V₁ʳᵉᶠ
//! and S₂₁ = V₂/V₁ʳᵉᶠ (amplitudes, then power waves), already referred to the model's end faces,
//! with no de-embedding, and the grid's own dispersion in the feed cancels.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::f64::consts::PI;

use emcraft_model::units::{C0, EPS0, ETA0, MU0};
use emcraft_model::{Model, Span};
use std::sync::Barrier;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use num_complex::Complex64 as C;

pub mod open;

/// The most cells a grid may have.
pub const MAX_CELLS: usize = 20_000_000;
/// The most time steps a run may take.
pub const MAX_STEPS: usize = 2_000_000;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum FdtdError {
    #[error("the model is not ready: {0}")]
    NotReady(String),
    #[error("invalid {what}: {why}")]
    Invalid { what: String, why: String },
    #[error("not supported yet: {0}")]
    Unsupported(String),
    #[error("the grid would have {cells} cells, more than the limit of {limit}: use a larger cell")]
    TooLarge { cells: usize, limit: usize },
}

pub type Result<T> = std::result::Result<T, FdtdError>;

pub(crate) fn invalid(what: &str, why: impl Into<String>) -> FdtdError {
    FdtdError::Invalid { what: what.into(), why: why.into() }
}

/// Run settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// The cell size (m), the grid's largest step.
    pub cell: f64,
    /// PML cells at each end.
    pub pml: usize,
    /// Cells between the PML and the source, and between the source and the port plane.
    pub feed: usize,
    /// Time step as a fraction of the stability limit.
    pub courant: f64,
    /// Stop when the transforms at the requested frequencies settle: none moves by more than this
    /// fraction of the incident wave's in a check interval.
    pub decay: f64,
    pub max_steps: usize,
}

impl Options {
    pub fn new(cell: f64) -> Options {
        Options { cell, pml: 16, feed: 12, courant: 0.99, decay: 1e-5, max_steps: 200_000 }
    }
}

/// The result of a run.
#[derive(Clone, Debug, PartialEq)]
pub struct TdResult {
    pub frequencies: Vec<f64>,
    /// Power waves referred to the model's end faces.
    pub s11: Vec<C>,
    pub s21: Vec<C>,
    pub cells: usize,
    pub dt: f64,
    /// Steps of the device run and of the reference run.
    pub steps: usize,
    pub reference_steps: usize,
    /// Whether the transforms settled before `max_steps`.
    pub converged: bool,
}

/// Divisions of `len` into steps of at most `h`.
fn steps(len: f64, h: f64) -> usize {
    ((len / h) * (1.0 - 1e-9)).ceil().max(1.0) as usize
}

/// The material and aperture of a z-layer of cells.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layer {
    eps: f64,
    mu: f64,
    x0: f64,
    width: f64,
    y0: f64,
    height: f64,
}

/// The tensor grid and the cells that exist (inside an aperture).
struct Grid {
    x: Vec<f64>,
    y: Vec<f64>,
    z: Vec<f64>,
    /// Per z-layer of cells.
    layers: Vec<Layer>,
    /// z-node index of the model's planes z = 0 and z = L, and of the source.
    plane1: usize,
    plane2: usize,
    source: usize,
    pml: usize,
}

impl Grid {
    fn nx(&self) -> usize {
        self.x.len() - 1
    }
    fn ny(&self) -> usize {
        self.y.len() - 1
    }
    fn nz(&self) -> usize {
        self.z.len() - 1
    }

    /// Whether cell (i, j, k) is inside the domain (signed indices; outside the grid is metal).
    fn exists(&self, i: isize, j: isize, k: isize) -> bool {
        if i < 0 || j < 0 || k < 0 || i as usize >= self.nx() || j as usize >= self.ny() || k as usize >= self.nz() {
            return false;
        }
        let l = self.layers[k as usize];
        let xc = 0.5 * (self.x[i as usize] + self.x[i as usize + 1]);
        let yc = 0.5 * (self.y[j as usize] + self.y[j as usize + 1]);
        xc > l.x0 && xc < l.x0 + l.width && yc > l.y0 && yc < l.y0 + l.height
    }
}

/// Builds the grid of the model's guide with its two feeds; `reference` makes every layer the
/// port-1 guide (the incident run).
fn grid(model: &Model, o: &Options, reference: bool) -> Result<Grid> {
    let w = model.waveguide.as_ref().ok_or_else(|| FdtdError::NotReady("no waveguide".into()))?;
    if model.wall_conductivity.is_some() || model.materials.iter().any(|m| m.loss_tangent != 0.0) {
        return Err(FdtdError::Unsupported("losses in the time domain".into()));
    }
    let h = o.cell;
    let n_est = (w.width / h).ceil() * (w.height / h).ceil() * ((w.length() + 2.0 * (o.pml + 2 * o.feed) as f64 * h) / h).ceil();
    if !(n_est <= MAX_CELLS as f64) {
        return Err(FdtdError::TooLarge { cells: n_est.min(usize::MAX as f64) as usize, limit: MAX_CELLS });
    }
    let layer = |i: usize| -> Result<Layer> {
        let s = w.sections.get(i).ok_or_else(|| invalid("waveguide", "no sections"))?;
        let (_, m) = model.material(&s.material).map_err(|e| invalid("material", e.to_string()))?;
        let Span { start: x0, width } = w.aperture(i).unwrap_or(w.across_x());
        let Span { start: y0, width: height } = w.aperture_y(i).unwrap_or(w.across_y());
        Ok(Layer { eps: m.eps_r, mu: m.mu_r, x0, width, y0, height })
    };
    let last = w.sections.len() - 1;
    let (first_layer, last_layer) = (layer(0)?, layer(last)?);
    // x: through every aperture edge.
    let mut breaks = vec![0.0, w.width];
    for i in 0..w.sections.len() {
        if let Some(a) = w.aperture(i) {
            breaks.push(a.start);
            breaks.push(a.end());
        }
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup_by(|a, b| (*a - *b).abs() <= 1e-9 * w.width);
    let mut x = vec![0.0];
    for b in breaks.windows(2) {
        let n = steps(b[1] - b[0], h);
        x.extend((1..=n).map(|k| b[0] + (b[1] - b[0]) * k as f64 / n as f64));
    }
    let mut ybreaks = vec![0.0, w.height];
    for i in 0..w.sections.len() {
        if let Some(b) = w.aperture_y(i) {
            ybreaks.push(b.start);
            ybreaks.push(b.end());
        }
    }
    ybreaks.sort_by(f64::total_cmp);
    ybreaks.dedup_by(|a, b| (*a - *b).abs() <= 1e-9 * w.height);
    let mut y = vec![0.0];
    for b in ybreaks.windows(2) {
        let n = steps(b[1] - b[0], h);
        y.extend((1..=n).map(|k| b[0] + (b[1] - b[0]) * k as f64 / n as f64));
    }
    // z: feed 1, the sections, feed 2.
    let first_len = w.sections[0].length;
    let dz1 = first_len / steps(first_len, h) as f64;
    let last_len = w.sections[last].length;
    let dz2 = last_len / steps(last_len, h) as f64;
    let ext = o.pml + 2 * o.feed;
    let mut z: Vec<f64> = (0..=ext).map(|k| -((ext - k) as f64) * dz1).collect();
    let mut layers = vec![first_layer; ext];
    let plane1 = ext;
    let mut at = 0.0;
    for (i, s) in w.sections.iter().enumerate() {
        let n = steps(s.length, h);
        z.extend((1..=n).map(|k| at + s.length * k as f64 / n as f64));
        let l = if reference { first_layer } else { layer(i)? };
        layers.extend(std::iter::repeat_n(l, n));
        at += s.length;
    }
    let plane2 = z.len() - 1;
    let end_layer = if reference { first_layer } else { last_layer };
    z.extend((1..=o.pml + o.feed).map(|k| at + k as f64 * dz2));
    layers.extend(std::iter::repeat_n(end_layer, o.pml + o.feed));
    let g = Grid { x, y, z, layers, plane1, plane2, source: o.pml + o.feed, pml: o.pml };
    let cells = g.nx() * g.ny() * g.nz();
    if cells > MAX_CELLS {
        return Err(FdtdError::TooLarge { cells, limit: MAX_CELLS });
    }
    Ok(g)
}

/// A field component's array on the grid. Values are stored as atomics so the threads of a
/// run can share them without `unsafe`: each thread writes only its own slabs of z, and the
/// barriers between half steps order every write before the reads of the next half step
/// (relaxed loads and stores compile to plain moves).
struct Field {
    nx: usize,
    ny: usize,
    v: Vec<AtomicU64>,
}

impl Field {
    fn new(nx: usize, ny: usize, nz: usize) -> Field {
        Field { nx, ny, v: (0..nx * ny * nz).map(|_| AtomicU64::new(0)).collect() }
    }
    #[inline]
    fn at(&self, i: usize, j: usize, k: usize) -> usize {
        i + self.nx * (j + self.ny * k)
    }
    #[inline]
    fn get(&self, id: usize) -> f64 {
        f64::from_bits(self.v[id].load(Ordering::Relaxed))
    }
    #[inline]
    fn set(&self, id: usize, x: f64) {
        self.v[id].store(x.to_bits(), Ordering::Relaxed);
    }
    fn len(&self) -> usize {
        self.v.len()
    }
}

/// A scratch array of the same layout (CPML auxiliary variables).
fn scratch(f: &Field) -> Field {
    Field { nx: f.nx, ny: f.ny, v: (0..f.len()).map(|_| AtomicU64::new(0)).collect() }
}

/// CPML coefficients (b, a) along z at the given positions.
fn cpml(zs: &[f64], g: &Grid, dt: f64, f_center: f64) -> Vec<(f64, f64)> {
    let (z_lo, z_hi) = (g.z[g.pml], g.z[g.nz() - g.pml]);
    let depth = (g.z[g.pml] - g.z[0]).max(f64::MIN_POSITIVE);
    let dz = depth / g.pml.max(1) as f64;
    let m = 3.0;
    let sigma_max = 0.8 * (m + 1.0) / (ETA0 * dz);
    let alpha_max = 2.0 * PI * f_center * EPS0 * 0.05;
    zs.iter()
        .map(|&z| {
            let d = if z < z_lo {
                (z_lo - z) / depth
            } else if z > z_hi {
                (z - z_hi) / depth
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

/// The Fourier transforms of the TE₁₀ projections at the two planes, accumulated every step.
struct Recording {
    d1: Vec<C>,
    d2: Vec<C>,
    steps: usize,
}

/// Steps between checks of whether the transforms have settled.
const CHECK: usize = 500;

/// One run: launches the pulse and transforms the TE₁₀ projections at the two planes at
/// `freqs` on the fly. Every [`CHECK`] steps after the pulse, `stop` sees the transforms and
/// says when they have settled (energy outside the band, near a port's cut-off, may linger
/// long after; it does not matter).
fn run(g: &Grid, o: &Options, freqs: &[f64], mut stop: impl FnMut(&[C], &[C]) -> bool) -> Result<(Recording, f64)> {
    let (f0, f1) = freqs.iter().fold((f64::INFINITY, 0.0f64), |(a, b), f| (a.min(*f), b.max(*f)));
    let (nx, ny, nz) = (g.nx(), g.ny(), g.nz());
    let dx: Vec<f64> = g.x.windows(2).map(|w| w[1] - w[0]).collect();
    let dy: Vec<f64> = g.y.windows(2).map(|w| w[1] - w[0]).collect();
    let dz: Vec<f64> = g.z.windows(2).map(|w| w[1] - w[0]).collect();
    // Dual steps at nodes (half cells at the ends).
    let dual =
        |d: &[f64], n: usize| -> Vec<f64> { (0..=n).map(|i| 0.5 * (if i > 0 { d[i - 1] } else { 0.0 } + if i < n { d[i] } else { 0.0 })).collect() };
    let (dxd, dyd, dzd) = (dual(&dx, nx), dual(&dy, ny), dual(&dz, nz));
    let min = |d: &[f64]| d.iter().copied().fold(f64::INFINITY, f64::min);
    let dt = o.courant / (C0 * (1.0 / min(&dx).powi(2) + 1.0 / min(&dy).powi(2) + 1.0 / min(&dz).powi(2)).sqrt());

    let ex = Field::new(nx, ny + 1, nz + 1);
    let ey = Field::new(nx + 1, ny, nz + 1);
    let ez = Field::new(nx + 1, ny + 1, nz);
    let hx = Field::new(nx + 1, ny, nz);
    let hy = Field::new(nx, ny + 1, nz);
    let hz = Field::new(nx, ny, nz + 1);
    // Update coefficients: dt/ε per E edge (0 on conductors), dt/μ per H face.
    let ii = |v: usize| v as isize;
    let edge_coef = |cells: [(isize, isize, isize, f64); 4]| -> f64 {
        let mut area = 0.0;
        let mut eps = 0.0;
        for (i, j, k, a) in cells {
            if !g.exists(i, j, k) {
                return 0.0;
            }
            area += a;
            eps += a * g.layers[k as usize].eps;
        }
        dt / (EPS0 * eps / area)
    };
    let mut cex = vec![0.0; ex.len()];
    for k in 0..=nz {
        for j in 0..=ny {
            for i in 0..nx {
                let a = |jj: usize, kk: usize| dy.get(jj).copied().unwrap_or(0.0) * dz.get(kk).copied().unwrap_or(0.0);
                let (jm, km) = (j.wrapping_sub(1), k.wrapping_sub(1));
                cex[ex.at(i, j, k)] = edge_coef([
                    (ii(i), ii(j) - 1, ii(k) - 1, a(jm, km)),
                    (ii(i), ii(j), ii(k) - 1, a(j, km)),
                    (ii(i), ii(j) - 1, ii(k), a(jm, k)),
                    (ii(i), ii(j), ii(k), a(j, k)),
                ]);
            }
        }
    }
    let mut cey = vec![0.0; ey.len()];
    for k in 0..=nz {
        for j in 0..ny {
            for i in 0..=nx {
                let a = |ii_: usize, kk: usize| dx.get(ii_).copied().unwrap_or(0.0) * dz.get(kk).copied().unwrap_or(0.0);
                let (im, km) = (i.wrapping_sub(1), k.wrapping_sub(1));
                cey[ey.at(i, j, k)] = edge_coef([
                    (ii(i) - 1, ii(j), ii(k) - 1, a(im, km)),
                    (ii(i), ii(j), ii(k) - 1, a(i, km)),
                    (ii(i) - 1, ii(j), ii(k), a(im, k)),
                    (ii(i), ii(j), ii(k), a(i, k)),
                ]);
            }
        }
    }
    let mut cez = vec![0.0; ez.len()];
    for k in 0..nz {
        for j in 0..=ny {
            for i in 0..=nx {
                let a = |ii_: usize, jj: usize| dx.get(ii_).copied().unwrap_or(0.0) * dy.get(jj).copied().unwrap_or(0.0);
                let (im, jm) = (i.wrapping_sub(1), j.wrapping_sub(1));
                cez[ez.at(i, j, k)] = edge_coef([
                    (ii(i) - 1, ii(j) - 1, ii(k), a(im, jm)),
                    (ii(i), ii(j) - 1, ii(k), a(i, jm)),
                    (ii(i) - 1, ii(j), ii(k), a(im, j)),
                    (ii(i), ii(j), ii(k), a(i, j)),
                ]);
            }
        }
    }
    // H: μ of the cells on either side of the face (one may be metal).
    let face_coef = |a: (isize, isize, isize), b: (isize, isize, isize)| -> f64 {
        let mu: Vec<f64> = [a, b].iter().filter(|c| g.exists(c.0, c.1, c.2)).map(|c| g.layers[c.2 as usize].mu).collect();
        if mu.is_empty() { 0.0 } else { dt / (MU0 * mu.iter().sum::<f64>() / mu.len() as f64) }
    };
    let mut chx = vec![0.0; hx.len()];
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..=nx {
                chx[hx.at(i, j, k)] = face_coef((ii(i) - 1, ii(j), ii(k)), (ii(i), ii(j), ii(k)));
            }
        }
    }
    let mut chy = vec![0.0; hy.len()];
    for k in 0..nz {
        for j in 0..=ny {
            for i in 0..nx {
                chy[hy.at(i, j, k)] = face_coef((ii(i), ii(j) - 1, ii(k)), (ii(i), ii(j), ii(k)));
            }
        }
    }
    let mut chz = vec![0.0; hz.len()];
    for k in 0..=nz {
        for j in 0..ny {
            for i in 0..nx {
                chz[hz.at(i, j, k)] = face_coef((ii(i), ii(j), ii(k) - 1), (ii(i), ii(j), ii(k)));
            }
        }
    }
    // CPML along z: E at nodes, H at cell centres.
    let fc = 0.5 * (f0 + f1);
    let pe = cpml(&g.z, g, dt, fc);
    let zc: Vec<f64> = g.z.windows(2).map(|w| 0.5 * (w[0] + w[1])).collect();
    let ph = cpml(&zc, g, dt, fc);
    let psi_ex = scratch(&ex); // ∂Hy/∂z in Ex
    let psi_ey = scratch(&ey); // ∂Hx/∂z in Ey
    let psi_hx = scratch(&hx); // ∂Ey/∂z in Hx
    let psi_hy = scratch(&hy); // ∂Ex/∂z in Hy

    // Source and projections: the TE₁₀ profile of the planes' apertures.
    // The TE₁₀ profile sin(π(x − x0)/a) on the aperture of a layer (zero outside it, across y too).
    let profile = |k_layer: usize| -> Vec<f64> {
        let l = g.layers[k_layer.min(g.layers.len() - 1)];
        let mut w = vec![0.0; (nx + 1) * ny];
        for j in 0..ny {
            let yc = 0.5 * (g.y[j] + g.y[j + 1]);
            if !(yc > l.y0 && yc < l.y0 + l.height) {
                continue;
            }
            for (i, &x) in g.x.iter().enumerate() {
                if x > l.x0 && x < l.x0 + l.width {
                    w[i + (nx + 1) * j] = (PI * (x - l.x0) / l.width).sin();
                }
            }
        }
        w
    };
    let w_src = profile(g.source);
    let w1 = profile(g.plane1);
    let w2 = profile(g.plane2.saturating_sub(1));
    let project = |ey: &Field, k: usize, w: &[f64]| -> f64 {
        let mut s = 0.0;
        for j in 0..ny {
            for i in 0..=nx {
                s += ey.get(ey.at(i, j, k)) * w[i + (nx + 1) * j] * dxd[i] * dy[j];
            }
        }
        s
    };
    // Gaussian-modulated pulse over [f0, f1], its spectrum down to 1e-4 at the ports' TE₁₀
    // cut-off: energy there travels ever more slowly and would linger in the feeds.
    let bw = (f1 - f0).max(1e-3 * fc);
    let cutoff = g.layers.iter().map(|l| C0 / (2.0 * l.width * (l.eps * l.mu).sqrt())).fold(0.0f64, f64::max);
    let cutoff = [g.layers[g.plane1.min(g.layers.len() - 1)], g.layers[g.plane2.saturating_sub(1)]]
        .iter()
        .map(|l| C0 / (2.0 * l.width * (l.eps * l.mu).sqrt()))
        .fold(0.0f64, f64::max)
        .min(cutoff);
    let mut tau = 1.5 / (PI * 0.5 * bw);
    if fc > 1.01 * cutoff {
        tau = tau.max((1e4f64).ln().sqrt() / (PI * (fc - cutoff)));
    }
    let t0 = 4.0 * tau;
    // Checks begin once the slowest in-band wave has crossed the grid three times.
    let v_min = g
        .layers
        .iter()
        .filter_map(|l| {
            let fc_l = C0 / (2.0 * l.width * (l.eps * l.mu).sqrt());
            (f0 > fc_l).then(|| C0 / (l.eps * l.mu).sqrt() * (1.0 - (fc_l / f0).powi(2)).sqrt())
        })
        .fold(C0, f64::min);
    let t_min = t0 + 4.0 * tau + 3.0 * (g.z[nz] - g.z[0]) / v_min;
    let mut rec = Recording { d1: vec![C::new(0.0, 0.0); freqs.len()], d2: vec![C::new(0.0, 0.0); freqs.len()], steps: 0 };
    // Running transforms: phasors e^{−jωt} advanced each step.
    let advance: Vec<C> = freqs.iter().map(|f| C::from_polar(1.0, -2.0 * PI * f * dt)).collect();
    let mut phasor = advance.clone();
    // Threads own slabs of z nodes, for the whole run.
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(16).min((nz + 1) / 4).max(1);
    let ranges: Vec<std::ops::Range<usize>> = (0..threads).map(|t| t * (nz + 1) / threads..(t + 1) * (nz + 1) / threads).collect();
    let barrier = Barrier::new(threads);
    let done = AtomicBool::new(false);
    let failed = AtomicBool::new(false);
    let fields = (&ex, &ey, &ez, &hx, &hy, &hz);
    let half_steps = |ks: &std::ops::Range<usize>| {
        let (ex, ey, ez, hx, hy, hz) = fields;
        // H half step on this thread's slabs.
        for k in ks.clone().filter(|&k| k < nz) {
            let (bh, ah) = ph[k];
            for j in 0..ny {
                for i in 0..=nx {
                    let id = hx.at(i, j, k);
                    let c = chx[id];
                    if c == 0.0 {
                        continue;
                    }
                    let dez_dy = (ez.get(ez.at(i, j + 1, k)) - ez.get(ez.at(i, j, k))) / dy[j];
                    let dey_dz = (ey.get(ey.at(i, j, k + 1)) - ey.get(ey.at(i, j, k))) / dz[k];
                    let psi = bh * psi_hx.get(id) + ah * dey_dz;
                    psi_hx.set(id, psi);
                    hx.set(id, hx.get(id) - c * (dez_dy - dey_dz - psi));
                }
            }
            for j in 0..=ny {
                for i in 0..nx {
                    let id = hy.at(i, j, k);
                    let c = chy[id];
                    if c == 0.0 {
                        continue;
                    }
                    let dex_dz = (ex.get(ex.at(i, j, k + 1)) - ex.get(ex.at(i, j, k))) / dz[k];
                    let dez_dx = (ez.get(ez.at(i + 1, j, k)) - ez.get(ez.at(i, j, k))) / dx[i];
                    let psi = bh * psi_hy.get(id) + ah * dex_dz;
                    psi_hy.set(id, psi);
                    hy.set(id, hy.get(id) - c * (dex_dz + psi - dez_dx));
                }
            }
        }
        for k in ks.clone() {
            for j in 0..ny {
                for i in 0..nx {
                    let id = hz.at(i, j, k);
                    let c = chz[id];
                    if c == 0.0 {
                        continue;
                    }
                    let dey_dx = (ey.get(ey.at(i + 1, j, k)) - ey.get(ey.at(i, j, k))) / dx[i];
                    let dex_dy = (ex.get(ex.at(i, j + 1, k)) - ex.get(ex.at(i, j, k))) / dy[j];
                    hz.set(id, hz.get(id) - c * (dey_dx - dex_dy));
                }
            }
        }
        barrier.wait();
        // E full step (interior nodes; edges on conductors have zero coefficients).
        for k in ks.clone().filter(|&k| k >= 1 && k < nz) {
            let (be, ae) = pe[k];
            for j in 1..ny {
                for i in 0..nx {
                    let id = ex.at(i, j, k);
                    let c = cex[id];
                    if c == 0.0 {
                        continue;
                    }
                    let dhz_dy = (hz.get(hz.at(i, j, k)) - hz.get(hz.at(i, j - 1, k))) / dyd[j];
                    let dhy_dz = (hy.get(hy.at(i, j, k)) - hy.get(hy.at(i, j, k - 1))) / dzd[k];
                    let psi = be * psi_ex.get(id) + ae * dhy_dz;
                    psi_ex.set(id, psi);
                    ex.set(id, ex.get(id) + c * (dhz_dy - dhy_dz - psi));
                }
            }
            for j in 0..ny {
                for i in 1..nx {
                    let id = ey.at(i, j, k);
                    let c = cey[id];
                    if c == 0.0 {
                        continue;
                    }
                    let dhx_dz = (hx.get(hx.at(i, j, k)) - hx.get(hx.at(i, j, k - 1))) / dzd[k];
                    let dhz_dx = (hz.get(hz.at(i, j, k)) - hz.get(hz.at(i - 1, j, k))) / dxd[i];
                    let psi = be * psi_ey.get(id) + ae * dhx_dz;
                    psi_ey.set(id, psi);
                    ey.set(id, ey.get(id) + c * (dhx_dz + psi - dhz_dx));
                }
            }
        }
        for k in ks.clone().filter(|&k| k < nz) {
            for j in 1..ny {
                for i in 1..nx {
                    let id = ez.at(i, j, k);
                    let c = cez[id];
                    if c == 0.0 {
                        continue;
                    }
                    let dhy_dx = (hy.get(hy.at(i, j, k)) - hy.get(hy.at(i - 1, j, k))) / dxd[i];
                    let dhx_dy = (hx.get(hx.at(i, j, k)) - hx.get(hx.at(i, j - 1, k))) / dyd[j];
                    ez.set(id, ez.get(id) + c * (dhy_dx - dhx_dy));
                }
            }
        }
        barrier.wait();
    };
    std::thread::scope(|scope| {
        for ks in ranges.iter().skip(1) {
            let (half_steps, barrier, done) = (&half_steps, &barrier, &done);
            scope.spawn(move || {
                loop {
                    half_steps(ks);
                    // Thread 0 adds the source and records.
                    barrier.wait();
                    if done.load(Ordering::Relaxed) {
                        break;
                    }
                }
            });
        }
        let ks0 = ranges.first().cloned().unwrap_or(0..0);
        for n in 0..o.max_steps {
            half_steps(&ks0);
            let t = (n + 1) as f64 * dt;
            let s = (-((t - t0) / tau).powi(2)).exp() * (2.0 * PI * fc * (t - t0)).sin();
            for j in 0..ny {
                for i in 0..=nx {
                    let id = ey.at(i, j, g.source);
                    if cey[id] != 0.0 {
                        ey.set(id, ey.get(id) + s * w_src[i + (nx + 1) * j]);
                    }
                }
            }
            let (v1, v2) = (project(&ey, g.plane1, &w1), project(&ey, g.plane2, &w2));
            for ((p, a), (d1, d2)) in phasor.iter_mut().zip(&advance).zip(rec.d1.iter_mut().zip(rec.d2.iter_mut())) {
                *d1 += *p * v1;
                *d2 += *p * v2;
                *p *= a;
            }
            rec.steps = n + 1;
            let check = (n + 1) % CHECK == 0 && t > t_min;
            let finished = n + 1 == o.max_steps || (check && stop(&rec.d1, &rec.d2));
            if !(v1.is_finite() && v2.is_finite()) {
                failed.store(true, Ordering::Relaxed);
            }
            if finished || failed.load(Ordering::Relaxed) {
                done.store(true, Ordering::Relaxed);
            }
            barrier.wait();
            if done.load(Ordering::Relaxed) {
                break;
            }
        }
    });
    if failed.load(Ordering::Relaxed) {
        return Err(invalid("run", "the fields diverged"));
    }
    Ok((rec, dt))
}

/// The broadband S-parameters (port 1 excited) of the model's guide at `frequencies` (Hz).
pub fn s_parameters(model: &Model, o: &Options, frequencies: &[f64]) -> Result<TdResult> {
    if !(o.cell.is_finite() && o.cell > 0.0) {
        return Err(invalid("cell", "must be a finite number > 0"));
    }
    if !(o.courant > 0.0 && o.courant < 1.0) || o.pml < 4 || o.feed < 2 || o.max_steps == 0 || o.max_steps > MAX_STEPS || !(o.decay > 0.0) {
        return Err(invalid("options", format!("{o:?}")));
    }
    if frequencies.is_empty() || frequencies.iter().any(|f| !(f.is_finite() && *f > 0.0)) {
        return Err(invalid("frequencies", "must be finite numbers > 0"));
    }
    let w = model.waveguide.as_ref().ok_or_else(|| FdtdError::NotReady("no waveguide".into()))?;
    if !model.symmetry.is_empty() {
        return Err(FdtdError::Unsupported("symmetry planes in the time domain (clear them: model.symmetry with clear true)".into()));
    }
    if !w.plain() {
        return Err(FdtdError::Unsupported("layers and strips in the time domain (its ports carry TE10 only)".into()));
    }
    // Every frequency above both ports' TE₁₀ cut-off, checked before any run.
    for (i, &f) in [0, w.sections.len() - 1].iter().flat_map(|i| frequencies.iter().map(move |f| (*i, f))) {
        let (x0w, s) = (w.aperture(i).map_or(w.width, |a| a.width), &w.sections[i]);
        let (_, m) = model.material(&s.material).map_err(|e| invalid("material", e.to_string()))?;
        if f <= C0 / (2.0 * x0w * (m.eps_r * m.mu_r).sqrt()) {
            return Err(invalid("frequency", format!("{f} Hz is below a port's TE10 cut-off")));
        }
    }
    // The transforms have settled when no frequency moved by more than `decay` of the incident
    // wave's largest since the last check.
    let settled = |last: &mut Option<(Vec<C>, Vec<C>)>, d1: &[C], d2: &[C], scale: f64| -> bool {
        let moved = last.as_ref().map(|(p1, p2)| {
            let m1 = p1.iter().zip(d1).fold(0.0f64, |m, (a, b)| m.max((a - b).norm()));
            let m2 = p2.iter().zip(d2).fold(0.0f64, |m, (a, b)| m.max((a - b).norm()));
            m1.max(m2)
        });
        *last = Some((d1.to_vec(), d2.to_vec()));
        moved.is_some_and(|m| m <= o.decay * scale)
    };
    // The incident run: the port-1 guide throughout.
    let gr = grid(model, o, true)?;
    let mut last = None;
    let (reference, dt) = run(&gr, o, frequencies, |d1, _| {
        let scale = d1.iter().fold(0.0f64, |m, c| m.max(c.norm()));
        settled(&mut last, d1, &[], scale)
    })?;
    let scale = reference.d1.iter().fold(0.0f64, |m, c| m.max(c.norm()));
    let g = grid(model, o, false)?;
    let mut last = None;
    let mut converged = false;
    let (dut, dt2) = run(&g, o, frequencies, |d1, d2| {
        converged = settled(&mut last, d1, d2, scale);
        converged
    })?;
    if (dt - dt2).abs() > 1e-12 * dt {
        return Err(invalid("grid", "the incident and device runs differ in time step"));
    }
    // Power waves: amplitude = V/‖e‖ (‖e‖ = width·height/2), power ∝ ‖e‖ β/μ.
    let (a1, a2) = (w.aperture(0).map_or(w.width, |a| a.width), w.aperture(w.sections.len() - 1).map_or(w.width, |a| a.width));
    let (h1, h2) = (w.aperture_y(0).map_or(w.height, |a| a.width), w.aperture_y(w.sections.len() - 1).map_or(w.height, |a| a.width));
    let port = |i: usize| model.material(&w.sections[i].material).map(|(_, m)| (m.eps_r, m.mu_r)).map_err(|e| invalid("material", e.to_string()));
    let ((e1, m1), (e2, m2)) = (port(0)?, port(w.sections.len() - 1)?);
    let mut s11 = Vec::new();
    let mut s21 = Vec::new();
    for (fi, &f) in frequencies.iter().enumerate() {
        let k0 = 2.0 * PI * f / C0;
        let beta = |a: f64, e: f64, m: f64| (k0 * k0 * e * m - (PI / a).powi(2)).max(0.0).sqrt();
        let (b1, b2) = (beta(a1, e1, m1), beta(a2, e2, m2));
        if b1 == 0.0 || b2 == 0.0 {
            return Err(invalid("frequency", format!("{f} Hz is below a port's TE10 cut-off")));
        }
        let inc = reference.d1[fi];
        // The transform is linear: the reflected wave's is the difference of the two runs'.
        let refl = dut.d1[fi] - inc;
        let trans = dut.d2[fi];
        s11.push(refl / inc);
        // Amplitudes: V/‖e‖, ‖e‖ = a h/2; power factor √((‖e₂‖ β2/μ2)/(‖e₁‖ β1/μ1)).
        let (n1, n2) = (0.5 * a1 * h1, 0.5 * a2 * h2);
        let amp = (trans / n2) / (inc / n1);
        s21.push(amp * ((n2 * b2 / m2) / (n1 * b1 / m1)).sqrt());
    }
    let cells = g.nx() * g.ny() * g.nz();
    Ok(TdResult { frequencies: frequencies.to_vec(), s11, s21, cells, dt, steps: dut.steps, reference_steps: reference.steps, converged })
}

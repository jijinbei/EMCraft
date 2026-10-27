//! The waveguide run: the guide's tensor grid with its two feeds, the update coefficients
//! (area-weighted materials, dispersive edges), the CPML along z, the TE₁₀ source and
//! projections, and one leapfrog run recording the projections' transforms.

use std::f64::consts::PI;
use std::ops::{ControlFlow, Range};

use emcraft_model::units::{C0, EPS0, MU0};
use emcraft_model::{Model, Pole, SecondOrder, Span, Waveguide};
use num_complex::Complex64 as C;

use crate::device::{Plan, Stepper};
use crate::dispersion::{Dispersion, Term};
use crate::yee::{Band, Cpml, Field, Fields, Grading, Kernel, Pulse, RunningDft, index, lattices, leapfrog, time_step};
use crate::{FdtdError, MAX_CELLS, Options, Result, invalid};

/// Divisions of `len` into steps of at most `h`.
fn steps(len: f64, h: f64) -> usize {
    ((len / h) * (1.0 - 1e-9)).ceil().max(1.0) as usize
}

/// Grid lines from 0 to `len` through every break in `breaks` (positions along the axis), each
/// interval divided evenly into steps of at most `h`.
fn lines(mut breaks: Vec<f64>, len: f64, h: f64) -> Vec<f64> {
    breaks.sort_by(f64::total_cmp);
    breaks.dedup_by(|a, b| (*a - *b).abs() <= 1e-9 * len);
    let mut x = vec![0.0];
    for b in breaks.windows(2) {
        let n = steps(b[1] - b[0], h);
        x.extend((1..=n).map(|k| b[0] + (b[1] - b[0]) * k as f64 / n as f64));
    }
    x
}

/// The material and aperture of a z-layer of cells.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layer {
    /// εr (ε∞ with dispersion).
    eps: f64,
    /// The largest |εr(ω)| over the run's frequencies (`eps` without dispersion): sets the
    /// slowest wave.
    slow: f64,
    /// The layer's dispersive material: an index into the grid's `poles` (`NO_POLES` if none).
    disp: u32,
    mu: f64,
    x0: f64,
    width: f64,
    y0: f64,
    height: f64,
}

impl Layer {
    /// Whether (x, y) lies inside the layer's aperture.
    fn inside(&self, x: f64, y: f64) -> bool {
        x > self.x0 && x < self.x0 + self.width && self.inside_y(y)
    }

    fn inside_y(&self, y: f64) -> bool {
        y > self.y0 && y < self.y0 + self.height
    }

    /// The TE₁₀ cut-off frequency of a guide of the layer's aperture, filled with εr `eps`.
    fn cutoff(&self, eps: f64) -> f64 {
        C0 / (2.0 * self.width * (eps * self.mu).sqrt())
    }
}

/// A layer without dispersion.
const NO_POLES: u32 = u32::MAX;

/// The tensor grid and the cells that exist (inside an aperture).
pub(crate) struct Grid {
    x: Vec<f64>,
    y: Vec<f64>,
    z: Vec<f64>,
    /// Per z-layer of cells.
    layers: Vec<Layer>,
    /// The poles of each dispersive material in the layers.
    poles: Vec<Vec<Pole>>,
    /// z-node index of the model's planes z = 0 and z = L, and of the source.
    plane1: usize,
    plane2: usize,
    source: usize,
    pml: usize,
}

/// Every aperture edge of the guide's sections across x (`y` false) or y, and the walls.
fn aperture_breaks(w: &Waveguide, y: bool) -> Vec<f64> {
    let mut breaks = vec![0.0, if y { w.height } else { w.width }];
    for i in 0..w.sections.len() {
        if let Some(a) = if y { w.aperture_y(i) } else { w.aperture(i) } {
            breaks.push(a.start);
            breaks.push(a.end());
        }
    }
    breaks
}

impl Grid {
    /// Builds the grid of the model's guide with its two feeds; `reference` makes every layer
    /// the port-1 guide (the incident run). `freqs` are the run's frequencies (for the slowest
    /// wave).
    pub(crate) fn new(model: &Model, o: &Options, reference: bool, freqs: &[f64]) -> Result<Grid> {
        let w = model.waveguide.as_ref().ok_or_else(|| FdtdError::NotReady("no waveguide".into()))?;
        if model.wall_conductivity.is_some() || model.materials.iter().any(|m| m.loss_tangent != 0.0) {
            return Err(FdtdError::Unsupported("losses in the time domain".into()));
        }
        // Dispersive materials in the model's order (deterministic), none at the ports.
        let mut poles: Vec<Vec<Pole>> = Vec::new();
        let mut disp_of = vec![NO_POLES; model.materials.len()];
        for (i, m) in model.materials.iter().enumerate().filter(|(_, m)| m.is_dispersive()) {
            m.checked().map_err(|e| invalid("material", e.to_string()))?;
            disp_of[i] = poles.len() as u32;
            poles.push(m.dispersion.clone());
        }
        for i in [0, w.sections.len().saturating_sub(1)] {
            let s = w.sections.get(i).ok_or_else(|| invalid("waveguide", "no sections"))?;
            if model.material(&s.material).is_ok_and(|(_, m)| m.is_dispersive()) {
                return Err(FdtdError::Unsupported("a port in a dispersive material (end the guide with a non-dispersive section)".into()));
            }
        }
        let h = o.cell;
        let n_est = (w.width / h).ceil() * (w.height / h).ceil() * ((w.length() + 2.0 * (o.pml + 2 * o.feed) as f64 * h) / h).ceil();
        if !(n_est <= MAX_CELLS as f64) {
            return Err(FdtdError::TooLarge { cells: n_est.min(usize::MAX as f64) as usize, limit: MAX_CELLS });
        }
        let layer = |i: usize| -> Result<Layer> {
            let s = w.sections.get(i).ok_or_else(|| invalid("waveguide", "no sections"))?;
            let (index, m) = model.material(&s.material).map_err(|e| invalid("material", e.to_string()))?;
            let Span { start: x0, width } = w.aperture(i).unwrap_or(w.across_x());
            let Span { start: y0, width: height } = w.aperture_y(i).unwrap_or(w.across_y());
            let slow = freqs.iter().map(|f| m.eps_at(*f).norm()).fold(m.eps_r, f64::max);
            let disp = disp_of.get(index).copied().unwrap_or(NO_POLES);
            Ok(Layer { eps: m.eps_r, slow, disp, mu: m.mu_r, x0, width, y0, height })
        };
        let last = w.sections.len() - 1;
        let (first_layer, last_layer) = (layer(0)?, layer(last)?);
        // x and y: through every aperture edge.
        let x = lines(aperture_breaks(w, false), w.width, h);
        let y = lines(aperture_breaks(w, true), w.height, h);
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
        let g = Grid { x, y, z, layers, poles, plane1, plane2, source: o.pml + o.feed, pml: o.pml };
        let cells = g.cells();
        if cells > MAX_CELLS {
            return Err(FdtdError::TooLarge { cells, limit: MAX_CELLS });
        }
        Ok(g)
    }

    fn nx(&self) -> usize {
        self.x.len() - 1
    }
    fn ny(&self) -> usize {
        self.y.len() - 1
    }
    fn nz(&self) -> usize {
        self.z.len() - 1
    }

    /// Cells along x, y and z.
    fn n(&self) -> [usize; 3] {
        [self.nx(), self.ny(), self.nz()]
    }

    pub(crate) fn cells(&self) -> usize {
        self.nx() * self.ny() * self.nz()
    }

    /// Whether cell (i, j, k) is inside the domain (signed indices; outside the grid is metal).
    fn exists(&self, i: isize, j: isize, k: isize) -> bool {
        if i < 0 || j < 0 || k < 0 || i as usize >= self.nx() || j as usize >= self.ny() || k as usize >= self.nz() {
            return false;
        }
        let xc = 0.5 * (self.x[i as usize] + self.x[i as usize + 1]);
        let yc = 0.5 * (self.y[j as usize] + self.y[j as usize + 1]);
        self.layers[k as usize].inside(xc, yc)
    }

    /// The layer of cells `k`, clamped to the grid.
    fn layer(&self, k: usize) -> Layer {
        self.layers[k.min(self.layers.len() - 1)]
    }
}

/// The grid's steps along each axis: primal (cell edges) and dual (between cell centres, half
/// cells at the ends).
struct Spacing {
    primal: [Vec<f64>; 3],
    dual: [Vec<f64>; 3],
}

impl Spacing {
    fn new(g: &Grid) -> Spacing {
        let primal = [&g.x, &g.y, &g.z].map(|x| x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<f64>>());
        let dual = primal.each_ref().map(|d| {
            let n = d.len();
            (0..=n).map(|i| 0.5 * (if i > 0 { d[i - 1] } else { 0.0 } + if i < n { d[i] } else { 0.0 })).collect()
        });
        Spacing { primal, dual }
    }

    /// The smallest step along each axis.
    fn min(&self) -> [f64; 3] {
        self.primal.each_ref().map(|d| d.iter().copied().fold(f64::INFINITY, f64::min))
    }
}

/// A second-order pole is explicit in time: stable while its frequencies stay well below 1/Δt
/// (ω₀Δt < 2 and the plasma term likewise; refused well before).
fn check_poles(g: &Grid, dt: f64) -> Result<()> {
    for pole in g.poles.iter().flatten() {
        if let Some(SecondOrder { strength: a, omega0: w0, .. }) = pole.second_order()
            && !(w0 * dt < 0.5 && a.sqrt() * dt < 0.5)
        {
            return Err(FdtdError::Unsupported(format!("a pole too fast for the time step ({dt:e} s): {pole:?}")));
        }
    }
    Ok(())
}

/// The update coefficients: dt/ε per E edge (0 on conductors) with the dispersive edges'
/// poles, dt/μ per H face.
struct Coefficients {
    e: [Vec<f64>; 3],
    h: [Vec<f64>; 3],
    dispersion: [Dispersion; 3],
}

impl Coefficients {
    /// The coefficients on the lattices `[e, h]` of the components.
    fn new(g: &Grid, sp: &Spacing, [e_dims, h_dims]: [[[usize; 3]; 3]; 2], dt: f64) -> Coefficients {
        let mut dispersion = [Dispersion::none(), Dispersion::none(), Dispersion::none()];
        let e = [0, 1, 2].map(|a| {
            let dims = e_dims[a];
            let len = dims.iter().product();
            let mut c = vec![0.0; len];
            for k in 0..dims[2] {
                for j in 0..dims[1] {
                    for i in 0..dims[0] {
                        let id = index(dims, i, j, k);
                        c[id] = edge_coef(g, sp, a, [i, j, k], dt, len, id, &mut dispersion[a]);
                    }
                }
            }
            c
        });
        for d in &mut dispersion {
            d.finish();
        }
        let h = [0, 1, 2].map(|a| {
            let dims = h_dims[a];
            let mut c = vec![0.0; dims.iter().product()];
            for k in 0..dims[2] {
                for j in 0..dims[1] {
                    for i in 0..dims[0] {
                        c[index(dims, i, j, k)] = face_coef(g, a, [i, j, k], dt);
                    }
                }
            }
            c
        });
        Coefficients { e, h, dispersion }
    }
}

/// dt/ε of the E edge along `axis` at node `idx` (component index `id` of `edges`): ε the
/// area-weighted average of the four cells round it, 0 if any is metal. A dispersive edge's
/// poles go into `dispersion`, each weighted by the share of its cells' area.
#[allow(clippy::too_many_arguments)]
fn edge_coef(g: &Grid, sp: &Spacing, axis: usize, idx: [usize; 3], dt: f64, edges: usize, id: usize, dispersion: &mut Dispersion) -> f64 {
    // The two axes across the edge, in order; the four cells at offsets −1, 0 along each, the
    // first varying fastest.
    let (p, q) = if axis == 0 { (1, 2) } else { (0, 3 - axis) };
    let len = |d: usize, i: usize| sp.primal[d].get(i).copied().unwrap_or(0.0);
    let cells = [(1, 1), (0, 1), (1, 0), (0, 0)].map(|(dp, dq): (usize, usize)| {
        let mut c = idx.map(|x| x as isize);
        c[p] -= dp as isize;
        c[q] -= dq as isize;
        (c, len(p, idx[p].wrapping_sub(dp)) * len(q, idx[q].wrapping_sub(dq)))
    });
    let mut area = 0.0;
    let mut eps = 0.0;
    for ([i, j, k], a) in cells {
        if !g.exists(i, j, k) {
            return 0.0;
        }
        area += a;
        eps += a * g.layers[k as usize].eps;
    }
    let c = dt / (EPS0 * eps / area);
    if !g.poles.is_empty() && cells.iter().any(|(c, a)| *a > 0.0 && g.layers[c[2] as usize].disp != NO_POLES) {
        let terms = cells.iter().flat_map(|&(c, a)| {
            let list = g.poles.get(g.layers[c[2] as usize].disp as usize).filter(|_| a > 0.0);
            list.into_iter().flatten().map(move |pole| Term::new(*pole, a / area, dt))
        });
        dispersion.add_edge(edges, id, dt / c, terms);
    }
    c
}

/// dt/μ of the H face normal to `axis` at `idx`: μ the average of the cells on either side of
/// the face that exist (one may be metal), 0 if neither does.
fn face_coef(g: &Grid, axis: usize, idx: [usize; 3], dt: f64) -> f64 {
    let b = idx.map(|x| x as isize);
    let mut a = b;
    a[axis] -= 1;
    let (sum, count) = [a, b].iter().filter(|c| g.exists(c[0], c[1], c[2])).fold((0.0, 0usize), |(s, n), c| (s + g.layers[c[2] as usize].mu, n + 1));
    if count == 0 { 0.0 } else { dt / (MU0 * sum / count as f64) }
}

/// The CPML at the two ends of the guide, absorbing along z: coefficients for E (at the z
/// nodes) and H (at the cell centres).
struct ZCpml {
    e: Vec<Cpml>,
    h: Vec<Cpml>,
}

impl ZCpml {
    fn new(g: &Grid, dt: f64, fc: f64) -> ZCpml {
        let (z_lo, z_hi) = (g.z[g.pml], g.z[g.nz() - g.pml]);
        let depth = (g.z[g.pml] - g.z[0]).max(f64::MIN_POSITIVE);
        let grading = Grading::new(depth / g.pml.max(1) as f64, dt, fc);
        let profile = |zs: &mut dyn Iterator<Item = f64>| -> Vec<Cpml> {
            zs.map(|z| {
                let d = if z < z_lo {
                    (z_lo - z) / depth
                } else if z > z_hi {
                    (z - z_hi) / depth
                } else {
                    0.0
                };
                grading.at(d)
            })
            .collect()
        };
        ZCpml { e: profile(&mut g.z.iter().copied()), h: profile(&mut g.z.windows(2).map(|w| 0.5 * (w[0] + w[1]))) }
    }
}

/// The CPML's auxiliary variables of the four components with a z derivative: ∂Hy/∂z in Ex,
/// ∂Hx/∂z in Ey, ∂Ey/∂z in Hx, ∂Ex/∂z in Hy.
struct Psi {
    ex: Field,
    ey: Field,
    hx: Field,
    hy: Field,
}

impl Psi {
    fn new(fields: &Fields) -> Psi {
        let [ex, ey, _] = &fields.e;
        let [hx, hy, _] = &fields.h;
        Psi { ex: ex.zeros_like(), ey: ey.zeros_like(), hx: hx.zeros_like(), hy: hy.zeros_like() }
    }
}

/// The leapfrog of the guide: the fields, their coefficients and the CPML.
struct GuideKernel<'a> {
    fields: &'a Fields,
    coef: &'a Coefficients,
    cpml: &'a ZCpml,
    psi: &'a Psi,
    sp: &'a Spacing,
    dt: f64,
}

impl Kernel for GuideKernel<'_> {
    fn step_h(&self, ks: Range<usize>) {
        let [ex, ey, ez] = &self.fields.e;
        let [hx, hy, hz] = &self.fields.h;
        let [chx, chy, chz] = &self.coef.h;
        let [dx, dy, dz] = &self.sp.primal;
        let (ph, Psi { hx: psi_hx, hy: psi_hy, .. }) = (&self.cpml.h, self.psi);
        let [nx, ny, nz] = [dx.len(), dy.len(), dz.len()];
        for k in ks.clone().filter(|&k| k < nz) {
            let ch = ph[k];
            for j in 0..ny {
                for i in 0..=nx {
                    let id = hx.at(i, j, k);
                    let c = chx[id];
                    if c == 0.0 {
                        continue;
                    }
                    let dez_dy = (ez.get(ez.at(i, j + 1, k)) - ez.get(ez.at(i, j, k))) / dy[j];
                    let dey_dz = (ey.get(ey.at(i, j, k + 1)) - ey.get(ey.at(i, j, k))) / dz[k];
                    let psi = ch.psi(psi_hx.get(id), dey_dz);
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
                    let psi = ch.psi(psi_hy.get(id), dex_dz);
                    psi_hy.set(id, psi);
                    hy.set(id, hy.get(id) - c * (dex_dz + psi - dez_dx));
                }
            }
        }
        for k in ks {
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
    }

    /// Interior nodes only; edges on conductors have zero coefficients.
    fn step_e(&self, ks: Range<usize>) {
        let [ex, ey, ez] = &self.fields.e;
        let [hx, hy, hz] = &self.fields.h;
        let [cex, cey, cez] = &self.coef.e;
        let [disp_x, disp_y, disp_z] = &self.coef.dispersion;
        let [dxd, dyd, dzd] = &self.sp.dual;
        let (pe, Psi { ex: psi_ex, ey: psi_ey, .. }) = (&self.cpml.e, self.psi);
        let [nx, ny, nz] = [dxd.len() - 1, dyd.len() - 1, dzd.len() - 1];
        let dt = self.dt;
        for k in ks.clone().filter(|&k| k >= 1 && k < nz) {
            let ce = pe[k];
            for j in 1..ny {
                for i in 0..nx {
                    let id = ex.at(i, j, k);
                    let c = cex[id];
                    if c == 0.0 {
                        continue;
                    }
                    let dhz_dy = (hz.get(hz.at(i, j, k)) - hz.get(hz.at(i, j - 1, k))) / dyd[j];
                    let dhy_dz = (hy.get(hy.at(i, j, k)) - hy.get(hy.at(i, j, k - 1))) / dzd[k];
                    let psi = ce.psi(psi_ex.get(id), dhy_dz);
                    psi_ex.set(id, psi);
                    let (e, curl) = (ex.get(id), dhz_dy - dhy_dz - psi);
                    ex.set(id, disp_x.step(id, e, curl, dt).unwrap_or(e + c * curl));
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
                    let psi = ce.psi(psi_ey.get(id), dhx_dz);
                    psi_ey.set(id, psi);
                    let (e, curl) = (ey.get(id), dhx_dz + psi - dhz_dx);
                    ey.set(id, disp_y.step(id, e, curl, dt).unwrap_or(e + c * curl));
                }
            }
        }
        for k in ks.filter(|&k| k < nz) {
            for j in 1..ny {
                for i in 1..nx {
                    let id = ez.at(i, j, k);
                    let c = cez[id];
                    if c == 0.0 {
                        continue;
                    }
                    let dhy_dx = (hy.get(hy.at(i, j, k)) - hy.get(hy.at(i - 1, j, k))) / dxd[i];
                    let dhx_dy = (hx.get(hx.at(i, j, k)) - hx.get(hx.at(i, j - 1, k))) / dyd[j];
                    let (e, curl) = (ez.get(id), dhy_dx - dhx_dy);
                    ez.set(id, disp_z.step(id, e, curl, dt).unwrap_or(e + c * curl));
                }
            }
        }
    }
}

/// The TE₁₀ profile sin(π(x − x0)/a) on the E_y nodes of a z plane, over the aperture of a
/// layer (zero outside it, across y too).
struct Te10 {
    w: Vec<f64>,
}

impl Te10 {
    fn new(g: &Grid, layer: Layer) -> Te10 {
        let nx = g.nx();
        let mut w = vec![0.0; (nx + 1) * g.ny()];
        for (j, y) in g.y.windows(2).enumerate() {
            if !layer.inside_y(0.5 * (y[0] + y[1])) {
                continue;
            }
            for (i, &x) in g.x.iter().enumerate() {
                if x > layer.x0 && x < layer.x0 + layer.width {
                    w[i + (nx + 1) * j] = (PI * (x - layer.x0) / layer.width).sin();
                }
            }
        }
        Te10 { w }
    }

    /// The profile's weight at E_y node (i, j).
    #[inline]
    fn at(&self, nx: usize, i: usize, j: usize) -> f64 {
        self.w[i + (nx + 1) * j]
    }

    /// The projection's weights: the profile times the area of each E_y node's dual face.
    fn weights(&self, sp: &Spacing) -> Vec<f64> {
        let ([dxd, _, _], [_, dy, _]) = (&sp.dual, &sp.primal);
        let nx = dxd.len() - 1;
        let mut w = Vec::with_capacity(self.w.len());
        for (j, dy) in dy.iter().enumerate() {
            for (i, dx) in dxd.iter().enumerate() {
                w.push(self.at(nx, i, j) * dx * dy);
            }
        }
        w
    }

    /// The projection of E_y on plane `k` onto the profile.
    fn project(&self, ey: &Field, k: usize, sp: &Spacing) -> f64 {
        let ([dxd, _, _], [_, dy, _]) = (&sp.dual, &sp.primal);
        let nx = dxd.len() - 1;
        let mut s = 0.0;
        for (j, dy) in dy.iter().enumerate() {
            for (i, dx) in dxd.iter().enumerate() {
                s += ey.get(ey.at(i, j, k)) * self.at(nx, i, j) * dx * dy;
            }
        }
        s
    }
}

/// A Gaussian-modulated pulse over the band, its spectrum down to 1e-4 at the ports' TE₁₀
/// cut-off: energy there travels ever more slowly and would linger in the feeds.
fn pulse(g: &Grid, band: Band) -> Pulse {
    let Band { f0, f1, fc } = band;
    let bw = (f1 - f0).max(1e-3 * fc);
    let cutoff = g.layers.iter().map(|l| l.cutoff(l.eps)).fold(0.0f64, f64::max);
    let cutoff = [g.layer(g.plane1), g.layers[g.plane2.saturating_sub(1)]].iter().map(|l| l.cutoff(l.eps)).fold(0.0f64, f64::max).min(cutoff);
    let mut tau = 1.5 / (PI * 0.5 * bw);
    if fc > 1.01 * cutoff {
        tau = tau.max((1e4f64).ln().sqrt() / (PI * (fc - cutoff)));
    }
    Pulse { t0: 4.0 * tau, tau, fc }
}

/// When checks may begin: once the pulse has passed and the slowest in-band wave has crossed
/// the grid three times.
fn first_check(g: &Grid, band: Band, pulse: Pulse) -> f64 {
    let v_min = g
        .layers
        .iter()
        .filter_map(|l| {
            let fc_l = l.cutoff(l.slow);
            (band.f0 > fc_l).then(|| C0 / (l.slow * l.mu).sqrt() * (1.0 - (fc_l / band.f0).powi(2)).sqrt())
        })
        .fold(C0, f64::min);
    pulse.t0 + 4.0 * pulse.tau + 3.0 * (g.z[g.nz()] - g.z[0]) / v_min
}

/// The Fourier transforms of the TE₁₀ projections at the two planes, accumulated every step.
pub(crate) struct Recording {
    pub(crate) d1: Vec<C>,
    pub(crate) d2: Vec<C>,
    pub(crate) steps: usize,
}

/// Steps between checks of whether the transforms have settled.
const CHECK: usize = 500;

/// One run: launches the pulse and transforms the TE₁₀ projections at the two planes at
/// `freqs` on the fly, on the CPU's threads or on `device`. Every [`CHECK`] steps after the
/// pulse, `stop` sees the transforms and says when they have settled (energy outside the band,
/// near a port's cut-off, may linger long after; it does not matter). Returns the transforms
/// and the time step.
pub(crate) fn run(
    g: &Grid,
    o: &Options,
    freqs: &[f64],
    device: Option<&dyn Stepper>,
    mut stop: impl FnMut(&[C], &[C]) -> bool,
) -> Result<(Recording, f64)> {
    let band = Band::of(freqs);
    let sp = Spacing::new(g);
    let dt = time_step(o.courant, sp.min());
    check_poles(g, dt)?;
    let coef = Coefficients::new(g, &sp, lattices(g.n()), dt);
    let cpml = ZCpml::new(g, dt, band.fc);
    // Source and projections: the TE₁₀ profile of the planes' apertures.
    let (src, w1, w2) = (Te10::new(g, g.layer(g.source)), Te10::new(g, g.layer(g.plane1)), Te10::new(g, g.layer(g.plane2.saturating_sub(1))));
    let pulse = pulse(g, band);
    let t_min = first_check(g, band, pulse);
    let mut dft = RunningDft::<2>::new(freqs, dt);
    let mut failed = false;
    let mut observe = |n: usize, [v1, v2]: [f64; 2]| {
        let t = (n + 1) as f64 * dt;
        dft.push([v1, v2]);
        let check = (n + 1).is_multiple_of(CHECK) && t > t_min;
        let [d1, d2] = dft.sums();
        let finished = n + 1 == o.max_steps || (check && stop(d1, d2));
        failed |= !(v1.is_finite() && v2.is_finite());
        if finished || failed { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }
    };
    let steps = match device {
        Some(device) => {
            let plan = Plan {
                n: g.n(),
                dt,
                dispersive: coef.dispersion.iter().any(Dispersion::any),
                e: coef.e,
                h: coef.h,
                primal: sp.primal.clone(),
                dual: sp.dual.clone(),
                cpml_e: cpml.e.iter().map(|c| [c.b, c.a]).collect(),
                cpml_h: cpml.h.iter().map(|c| [c.b, c.a]).collect(),
                source_plane: g.source,
                source: src.w,
                planes: [g.plane1, g.plane2],
                weights: [w1.weights(&sp), w2.weights(&sp)],
                pulse,
            };
            device.leapfrog(&plan, o.max_steps, &mut observe)?
        }
        None => {
            let fields = Fields::new(g.n());
            let psi = Psi::new(&fields);
            let kernel = GuideKernel { fields: &fields, coef: &coef, cpml: &cpml, psi: &psi, sp: &sp, dt };
            let (ey, cey) = (&fields.e[1], &coef.e[1]);
            let (nx, ny) = (g.nx(), g.ny());
            leapfrog(&kernel, g.nz() + 1, o.max_steps, |n| {
                let s = pulse.at((n + 1) as f64 * dt);
                for j in 0..ny {
                    for i in 0..=nx {
                        let id = ey.at(i, j, g.source);
                        if cey[id] != 0.0 {
                            ey.set(id, ey.get(id) + s * src.at(nx, i, j));
                        }
                    }
                }
                observe(n, [w1.project(ey, g.plane1, &sp), w2.project(ey, g.plane2, &sp)])
            })
        }
    };
    if failed {
        return Err(invalid("run", "the fields diverged"));
    }
    let [d1, d2] = dft.into_sums();
    Ok((Recording { d1, d2, steps }, dt))
}

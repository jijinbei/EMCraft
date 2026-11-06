//! Open bodies of revolution: the meridian half-plane lined by a perfectly matched layer, a
//! source in it, and the far field of what radiates.
//!
//! **The layer.** Stretched coordinates in cylindrical or spherical form are the anisotropic
//! medium εΛ, μΛ in the original ones (F. L. Teixeira, W. C. Chew, *Systematic derivation of
//! anisotropic PML absorbing media in cylindrical and spherical coordinates*, IEEE Microwave
//! Guided Wave Lett. 7(11):371–373, 1997), which the half-plane's assembly takes per quadrature
//! point (as A. D. Greenwood, J.-M. Jin, IEEE Trans. Antennas Propag. 47(4):620–629, 1999, line
//! a body of revolution with a cylindrical one). A cylinder's layer stretches r in its outer
//! `thickness` and z in its two ends': with s_u = 1 − j(σ₀/k₀)ξ_u² and r̃ = r − j(σ₀/k₀)dξ_r³/3,
//! Λ = (r̃/r)(s_z/s_r) r̂r̂ + (r/r̃) s_r s_z φ̂φ̂ + (r̃/r)(s_r/s_z) ẑẑ. A sphere's stretches its
//! radius ρ in its outer shell: Λ = (ρ̃/ρ)²/s_ρ ρ̂ρ̂ + s_ρ (θ̂θ̂ + φ̂φ̂). σ₀ = 3 ln(1/R)/(2d) for
//! the reflection R at normal incidence, as the 3D layer's. Behind the layer is a conductor.
//!
//! **Sources.** A current element of moment I·l along z on the axis is order m = 0: the weak
//! form's −jωμ₀ ∫ N · J dV, per radian of φ, puts −jωμ₀ I l (t̂_z / ℓ)/(2π) on the axis edge
//! holding it (its Whitney function's tangential part 1/ℓ along t̂). A plane wave
//! E_inc = x̂ e^{∓jk₀z} along ±z is orders m = ±1: x̂ = r̂ cos φ − φ̂ sin φ, so its e^{−jφ} part has
//! E_r = ½ e^{∓jk₀z}, E_φ = −(j/2) e^{∓jk₀z} (v = −j r E_φ = −(r/2) e^{∓jk₀z}). The field the
//! dielectrics scatter solves the equation with the source k₀²(εr − 1) E_inc in them (μr = 1),
//! and its e^{+jφ} part is its mirror in the plane y = 0 (E_r, E_z the same, E_φ the opposite).
//!
//! **Far field.** On a contour of the half-plane a third of the layer's thickness inside it,
//! turned about the axis, the fields E = Σ_m (E_t + φ̂ E_φ) e^{−jmφ} and
//! H = −∇ × E/(jωμ₀) are sampled at evenly spaced φ (the trapezoid rule, exact for the
//! e^{−jmφ} e^{jk r' sin θ cos(φ − φ')} it integrates once there are more than 2(k r' + |m|) + 16
//! samples) as the 3D Huygens surface's samples, whose far field and power
//! [`emcraft_theory::radiation::Huygens`] gives (C. A. Balanis, *Advanced Engineering
//! Electromagnetics*, 2nd ed., Wiley 2012, §6.8).

use emcraft_model::units::MU0;
use emcraft_model::{CVec3, Vec3};
use emcraft_theory::radiation::{Huygens, HuygensSample};

use super::*;
use crate::driven::pml::{Shell, spherical_stretch};

/// The most samples along φ of the Huygens surface.
pub const MAX_PHI_SAMPLES: usize = 2048;

/// The shape of a body of revolution's perfectly matched layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayerShape {
    /// A cylinder about z from z0 to z1, its wall at `radius`: the layer is its outer `thickness`
    /// of r and of each end.
    Cylinder { radius: f64, z0: f64, z1: f64 },
    /// A sphere of `radius` about the origin: the layer is its outer shell.
    Sphere { radius: f64 },
}

/// A perfectly matched layer lining a body of revolution's boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub shape: LayerShape,
    pub thickness: f64,
    /// The reflection at normal incidence its profile is designed for.
    pub reflection: f64,
}

impl Layer {
    /// The layer, checked: a finite thickness leaving room inside the shape, a reflection in
    /// (0, 1).
    ///
    /// ```
    /// use emcraft_solver::bor::open::{Layer, LayerShape};
    ///
    /// let l = Layer::new(LayerShape::Sphere { radius: 0.03 }, 0.01, 1e-6)?;
    /// // The Huygens contour a third of the layer inside it: from (0, −16.7 mm) up the axis' far side.
    /// assert!(l.contour().first().is_some_and(|q| q[0] == 0.0 && (q[1] + 0.05 / 3.0).abs() < 1e-12));
    /// assert!(Layer::new(LayerShape::Sphere { radius: 0.03 }, 0.03, 1e-6).is_err());
    /// # Ok::<(), emcraft_solver::SolveError>(())
    /// ```
    pub fn new(shape: LayerShape, thickness: f64, reflection: f64) -> Result<Layer> {
        let room = match shape {
            LayerShape::Cylinder { radius, z0, z1 } => [radius, 0.5 * (z1 - z0)].into_iter().fold(f64::INFINITY, f64::min),
            LayerShape::Sphere { radius } => radius,
        };
        if !(thickness.is_finite() && thickness > 0.0 && room.is_finite() && 4.0 * thickness / 3.0 < room) {
            return Err(invalid("layer", "its thickness must be finite, > 0 and leave room for the Huygens surface inside it"));
        }
        if !(reflection > 0.0 && reflection < 1.0) {
            return Err(invalid("layer", "its reflection must lie in (0, 1)"));
        }
        Ok(Layer { shape, thickness, reflection })
    }

    fn sigma0(&self) -> f64 {
        3.0 * (1.0 / self.reflection).ln() / (2.0 * self.thickness)
    }

    /// The medium at (r, z) of a material (εr, μr) at wavenumber k₀: the layer's εΛ, μΛ where it
    /// reaches, else the material's own.
    pub(crate) fn medium(&self, eps: f64, mu: f64, [r, z]: [f64; 2], k0: f64) -> Medium {
        let d = self.thickness;
        let g = self.sigma0() / k0;
        let zero = C::new(0.0, 0.0);
        match self.shape {
            LayerShape::Cylinder { radius, z0, z1 } => {
                let xr = ((r - (radius - d)) / d).clamp(0.0, 1.0);
                let xz = ((z0 + d - z) / d).max((z - (z1 - d)) / d).clamp(0.0, 1.0);
                if (xr == 0.0 && xz == 0.0) || r <= 0.0 {
                    return Medium::isotropic(eps, mu);
                }
                let (sr, sz) = (C::new(1.0, -g * xr * xr), C::new(1.0, -g * xz * xz));
                let q = C::new(r, -g * d * xr * xr * xr / 3.0) / r;
                let (lr, lphi, lz) = (q * sz / sr, sr * sz / q, q * sr / sz);
                Medium {
                    eps_t: [[lr * eps, zero], [zero, lz * eps]],
                    eps_phi: lphi * eps,
                    nu_w: [[(lz * mu).inv(), zero], [zero, (lr * mu).inv()]],
                    nu_phi: (lphi * mu).inv(),
                }
            }
            LayerShape::Sphere { radius } => {
                let rho = r.hypot(z);
                let shell = Shell { inner: radius - d, thickness: d, sigma0: self.sigma0() };
                let (a, b) = spherical_stretch(rho, shell, k0);
                if rho <= 0.0 || (a == C::new(1.0, 0.0) && b == C::new(1.0, 0.0)) {
                    return Medium::isotropic(eps, mu);
                }
                // ρ̂ and θ̂ in (r, z).
                let (pr, pz) = (r / rho, z / rho);
                let (tr, tz) = (pz, -pr);
                let outer = |x: f64, y: f64| [[C::new(x * x, 0.0), C::new(x * y, 0.0)], [C::new(x * y, 0.0), C::new(y * y, 0.0)]];
                let (pp, tt) = (outer(pr, pz), outer(tr, tz));
                let mix = |ca: C, cb: C, a: &[[C; 2]; 2], b: &[[C; 2]; 2]| {
                    [[ca * a[0][0] + cb * b[0][0], ca * a[0][1] + cb * b[0][1]], [ca * a[1][0] + cb * b[1][0], ca * a[1][1] + cb * b[1][1]]]
                };
                Medium {
                    eps_t: mix(a * eps, b * eps, &pp, &tt),
                    eps_phi: b * eps,
                    // ν_w = J ν_t Jᵀ: J ρ̂ = −θ̂, J θ̂ = ρ̂.
                    nu_w: mix((a * mu).inv(), (b * mu).inv(), &tt, &pp),
                    nu_phi: (b * mu).inv(),
                }
            }
        }
    }

    /// The Huygens contour on the half-plane, a third of the thickness inside the layer: from the
    /// axis below round to the axis above.
    pub fn contour(&self) -> Vec<[f64; 2]> {
        let inset = self.thickness * 4.0 / 3.0;
        match self.shape {
            LayerShape::Cylinder { radius, z0, z1 } => {
                let (r, lo, hi) = (radius - inset, z0 + inset, z1 - inset);
                vec![[0.0, lo], [r, lo], [r, hi], [0.0, hi]]
            }
            LayerShape::Sphere { radius } => {
                let rho = radius - inset;
                let n = 64;
                (0..=n)
                    .map(|k| {
                        let t = PI * k as f64 / n as f64;
                        [if k == 0 || k == n { 0.0 } else { rho * t.sin() }, -rho * t.cos()]
                    })
                    .collect()
            }
        }
    }

    /// The point the far field's phase is taken from.
    fn centre(&self) -> Point3 {
        match self.shape {
            LayerShape::Cylinder { z0, z1, .. } => Point3::new(0.0, 0.0, 0.5 * (z0 + z1)),
            LayerShape::Sphere { .. } => Point3::ORIGIN,
        }
    }
}

/// What drives an open body of revolution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BorSource {
    /// A current element of `moment` I·l (A·m, peak) along +z on the axis at height `z` (m).
    AxialDipole { z: f64, moment: f64 },
    /// A plane wave of 1 V/m, E along x̂, travelling along +z (`up`) or −z.
    PlaneWave { up: bool },
}

/// An open body of revolution solved: the Huygens surface's samples (the field radiated, or
/// scattered for a plane wave), the order solved and the unknowns.
#[derive(Clone, Debug, PartialEq)]
pub struct Radiated {
    pub huygens: Huygens,
    pub m: u32,
    pub unknowns: usize,
}

/// The triangles' unknowns, per local function, as the assembly numbers them.
fn locals(p: &Profile, d: &Dofs) -> Vec<Vec<(usize, Local)>> {
    p.triangles
        .iter()
        .map(|t| {
            let mut l = Vec::with_capacity(6);
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if let Some(&u) = d.edge.get(&[a.min(b), a.max(b)]) {
                    let (i, j) = if a < b { (k, (k + 1) % 3) } else { ((k + 1) % 3, k) };
                    l.push((u, Local::Edge(i, j)));
                }
            }
            for (k, &n) in t.iter().enumerate() {
                if let Some(Some(u)) = d.node.get(n as usize) {
                    l.push((*u, Local::Node(k)));
                }
            }
            l
        })
        .collect()
}

/// A triangle's corners, its doubled signed area and its barycentric gradients.
fn geometry(p: &Profile, t: &[u32; 3]) -> Option<([[f64; 2]; 3], f64, [[f64; 2]; 3])> {
    let q = [p.points.get(t[0] as usize)?, p.points.get(t[1] as usize)?, p.points.get(t[2] as usize)?].map(|x| *x);
    let det = (q[1][0] - q[0][0]) * (q[2][1] - q[0][1]) - (q[2][0] - q[0][0]) * (q[1][1] - q[0][1]);
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let g = [0, 1, 2].map(|i| {
        let (j, k) = ((i + 1) % 3, (i + 2) % 3);
        [(q[j][1] - q[k][1]) / det, (q[k][0] - q[j][0]) / det]
    });
    Some((q, det, g))
}

/// The field of order m in a triangle at barycentric `lam`: (E_r, E_z), u = r E_φ = j v, its
/// gradient (∂_r u, ∂_z u) and the curl's φ component ∂_z E_r − ∂_r E_z.
struct Local3 {
    et: [C; 2],
    u: C,
    grad_u: [C; 2],
    curl_phi: C,
}

fn evaluate(p: &Profile, d: &Dofs, ti: usize, lam: [f64; 3], x: &[C]) -> Option<Local3> {
    let t = p.triangles.get(ti)?;
    let (_, _, g) = geometry(p, t)?;
    let zero = C::new(0.0, 0.0);
    let (mut et, mut v, mut gv, mut curl) = ([zero; 2], zero, [zero; 2], zero);
    for k in 0..3 {
        let (a, b) = (t[k], t[(k + 1) % 3]);
        if let Some(&u) = d.edge.get(&[a.min(b), a.max(b)]) {
            let (i, j) = if a < b { (k, (k + 1) % 3) } else { ((k + 1) % 3, k) };
            let c = x.get(u).copied().unwrap_or_default();
            for dim in 0..2 {
                et[dim] += c * (lam[i] * g[j][dim] - lam[j] * g[i][dim]);
            }
            curl += c * (-2.0 * (g[i][0] * g[j][1] - g[i][1] * g[j][0]));
        }
        if let Some(Some(u)) = d.node.get(t[k] as usize) {
            let c = x.get(*u).copied().unwrap_or_default();
            v += c * lam[k];
            gv[0] += c * g[k][0];
            gv[1] += c * g[k][1];
        }
    }
    let j = C::new(0.0, 1.0);
    Some(Local3 { et, u: j * v, grad_u: [j * gv[0], j * gv[1]], curl_phi: curl })
}

/// The triangle holding `q` and its barycentric coordinates there.
fn locate(p: &Profile, boxes: &[[f64; 4]], q: [f64; 2]) -> Option<(usize, [f64; 3])> {
    let tol = 1e-9;
    for (ti, b) in boxes.iter().enumerate() {
        if q[0] < b[0] || q[0] > b[1] || q[1] < b[2] || q[1] > b[3] {
            continue;
        }
        let Some((c, det, _)) = p.triangles.get(ti).and_then(|t| geometry(p, t)) else { continue };
        let lam = [0, 1, 2].map(|k| {
            let (a, b) = (c[(k + 1) % 3], c[(k + 2) % 3]);
            ((b[0] - a[0]) * (q[1] - a[1]) - (q[0] - a[0]) * (b[1] - a[1])) / det
        });
        if lam.iter().all(|&l| l >= -tol) {
            return Some((ti, lam));
        }
    }
    None
}

/// The open body of revolution `p` lined by `layer`, driven by `source` at `frequency` (Hz):
/// solved in the source's order (m = 0 for the dipole, 1 for the plane wave, its m = −1 part the
/// mirror), and its Huygens surface sampled.
pub fn radiate(p: &Profile, layer: &Layer, source: &BorSource, frequency: f64) -> Result<Radiated> {
    if p.triangles.is_empty() || p.triangles.len() > MAX_TRIANGLES || p.material.len() != p.triangles.len() {
        return Err(invalid("profile", format!("must have 1 to {MAX_TRIANGLES} triangles, each with a material")));
    }
    if !p.material.iter().all(|(e, u)| e.is_finite() && u.is_finite() && *e > 0.0 && *u > 0.0) {
        return Err(invalid("profile", "every material needs finite εr > 0 and μr > 0"));
    }
    crate::driven::check_frequency(frequency)?;
    let k0 = 2.0 * PI * frequency / C0;
    let omega = 2.0 * PI * frequency;
    let m = match source {
        BorSource::AxialDipole { .. } => 0,
        BorSource::PlaneWave { .. } => 1,
    };
    let d = numbering(p, m);
    if d.count == 0 {
        return Err(invalid("profile", "has no unknowns: no interior to the conductor"));
    }
    let (k, mm, _) = assemble_with(p, m, &d, &[], Some((layer, k0)))?;
    let mut a = k;
    for (v, w) in a.val.iter_mut().zip(&mm.val) {
        *v -= w * (k0 * k0);
    }
    let mut b = vec![C::new(0.0, 0.0); d.count];
    let scale = 1e-12 * p.points.iter().map(|q| q[0].abs().max(q[1].abs())).fold(0.0, f64::max).max(1e-300);
    match *source {
        BorSource::AxialDipole { z, moment } => {
            if !(z.is_finite() && moment.is_finite()) {
                return Err(invalid("dipole", "its height and moment must be finite"));
            }
            // The axis edge holding z: its function's tangential part 1/ℓ along t̂.
            let at = |n: u32| p.points.get(n as usize).copied().unwrap_or([f64::NAN; 2]);
            let held = d.edge.iter().find(|(e, _)| {
                let (q0, q1) = (at(e[0]), at(e[1]));
                q0[0] <= scale && q1[0] <= scale && z >= q0[1].min(q1[1]) && z <= q0[1].max(q1[1])
            });
            let Some((e, &u)) = held else { return Err(invalid("dipole", "must lie on the axis inside the body")) };
            let (q0, q1) = (at(e[0]), at(e[1]));
            let len = (q1[1] - q0[1]).abs();
            let nz = (q1[1] - q0[1]).signum() / len;
            b[u] = C::new(0.0, -omega * MU0 * moment * nz / (2.0 * PI));
        }
        BorSource::PlaneWave { up } => {
            if p.material.iter().any(|&(e, mu)| e != 1.0 && mu != 1.0) {
                return Err(invalid("plane wave", "not supported yet: magnetic scatterers (μr ≠ 1)"));
            }
            let s = if up { 1.0 } else { -1.0 };
            let ls = locals(p, &d);
            for (ti, t) in p.triangles.iter().enumerate() {
                let (eps, _) = p.material[ti];
                if eps == 1.0 {
                    continue;
                }
                let Some((q, det, g)) = geometry(p, t) else { return Err(invalid("profile", format!("triangle {} has no area", ti + 1))) };
                let area = 0.5 * det.abs();
                for &(lam, w) in &RULE {
                    let r = lam[0] * q[0][0] + lam[1] * q[1][0] + lam[2] * q[2][0];
                    let z = lam[0] * q[0][1] + lam[1] * q[1][1] + lam[2] * q[2][1];
                    let phase = C::from_polar(1.0, -s * k0 * z);
                    let (er, vinc) = (phase * 0.5, phase * (-0.5 * r));
                    let f = (eps - 1.0) * k0 * k0 * w * area;
                    for &(u, l) in &ls[ti] {
                        let term = match l {
                            Local::Edge(i, j) => er * (r * (lam[i] * g[j][0] - lam[j] * g[i][0])),
                            Local::Node(i) => vinc * (lam[i] / r),
                        };
                        b[u] += term * f;
                    }
                }
            }
        }
    }
    let at = |n: u32| p.points.get(n as usize).map_or(Point3::ORIGIN, |q| Point3::new(q[0], q[1], 0.0));
    let mut xyz = vec![Point3::ORIGIN; d.count];
    for (e, &u) in &d.edge {
        xyz[u] = at(e[0]).lerp(at(e[1]), 0.5);
    }
    for (n, u) in d.node.iter().enumerate() {
        if let Some(u) = u {
            xyz[*u] = at(n as u32);
        }
    }
    let factor = Ldlt::factor(&a, &xyz)?;
    factor.solve(&mut b)?;
    let huygens = sample(p, &d, layer, &b, m, frequency)?;
    Ok(Radiated { huygens, m, unknowns: d.count })
}

/// An open body of revolution `p` lined by `layer`, fed by its `ports` in order m at `frequency`
/// (Hz): the S-parameters between their first modes and, for a unit wave of port `excitation`'s
/// first mode in, the Huygens surface's samples (for m ≥ 1 with the order −m's mirror: the
/// linearly polarized feed, E along x at φ = 0).
pub fn radiate_ports(
    p: &Profile,
    layer: &Layer,
    ports: &[super::BorPort],
    m: u32,
    excitation: usize,
    frequency: f64,
) -> Result<(super::BorSweepPoint, Radiated)> {
    let (point, d, xs) = super::ports::solved(p, m, ports, frequency, Some(layer))?;
    let x = xs.get(excitation).ok_or_else(|| invalid("excitation", "no such port"))?;
    let huygens = sample(p, &d, layer, x, m, frequency)?;
    Ok((point, Radiated { huygens, m, unknowns: d.count }))
}

/// The Huygens surface's samples of the solution `x` of order m (and, for m = 1, its mirror of
/// order −1).
fn sample(p: &Profile, d: &Dofs, layer: &Layer, x: &[C], m: u32, frequency: f64) -> Result<Huygens> {
    let k0 = 2.0 * PI * frequency / C0;
    let omega = 2.0 * PI * frequency;
    let contour = layer.contour();
    let reach = contour.iter().map(|q| q[0]).fold(0.0, f64::max);
    let n_phi = (2 * ((k0 * reach).ceil() as usize + m as usize) + 16).min(MAX_PHI_SAMPLES);
    let boxes: Vec<[f64; 4]> = p
        .triangles
        .iter()
        .map(|t| {
            let q = t.map(|v| p.points.get(v as usize).copied().unwrap_or([f64::NAN; 2]));
            [
                q[0][0].min(q[1][0]).min(q[2][0]),
                q[0][0].max(q[1][0]).max(q[2][0]),
                q[0][1].min(q[1][1]).min(q[2][1]),
                q[0][1].max(q[1][1]).max(q[2][1]),
            ]
        })
        .collect();
    // Two Gauss points per piece of at most a twentieth of the wavelength.
    let gauss = [(0.5 - 0.5 / 3f64.sqrt(), 0.5), (0.5 + 0.5 / 3f64.sqrt(), 0.5)];
    let piece = C0 / frequency / 20.0;
    let parts: Vec<(i32, f64)> = if m == 0 { vec![(0, 1.0)] } else { vec![(m as i32, 1.0), (-(m as i32), -1.0)] };
    let j = C::new(0.0, 1.0);
    let mut samples = Vec::new();
    for w in contour.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dr, dz) = (b[0] - a[0], b[1] - a[1]);
        let len = dr.hypot(dz);
        if len <= 0.0 {
            continue;
        }
        let (nr, nz) = (dz / len, -dr / len);
        let pieces = (len / piece).ceil().max(1.0) as usize;
        for k in 0..pieces {
            for &(t, gw) in &gauss {
                let s = (k as f64 + t) / pieces as f64;
                let q = [a[0] + s * dr, a[1] + s * dz];
                if q[0] <= 0.0 {
                    continue;
                }
                let (ti, lam) =
                    locate(p, &boxes, q).ok_or_else(|| invalid("Huygens surface", "a point of it lies outside the body (in a conductor)"))?;
                if p.material.get(ti) != Some(&(1.0, 1.0)) {
                    return Err(invalid("Huygens surface", "a point of it lies in a dielectric: keep the bodies inside the layer's inner surface"));
                }
                let f = evaluate(p, d, ti, lam, x).ok_or_else(|| invalid("Huygens surface", "a triangle there has no area"))?;
                let weight = gw * len / pieces as f64 * q[0] * 2.0 * PI / n_phi as f64;
                for i in 0..n_phi {
                    let phi = 2.0 * PI * i as f64 / n_phi as f64;
                    let (c, sn) = (phi.cos(), phi.sin());
                    let (rh, ph) = (Vec3::new(c, sn, 0.0), Vec3::new(-sn, c, 0.0));
                    let (mut e, mut h) = (CVec3::ZERO, CVec3::ZERO);
                    for &(mo, sign) in &parts {
                        let mf = f64::from(mo);
                        let (u, gu) = (f.u * sign, [f.grad_u[0] * sign, f.grad_u[1] * sign]);
                        let ephi = u / q[0];
                        let cr = -(j * mf * f.et[1] + gu[1]) / q[0];
                        let cz = (gu[0] + j * mf * f.et[0]) / q[0];
                        let rot = C::from_polar(1.0, -mf * phi);
                        let hs = -1.0 / (j * omega * MU0);
                        e += (CVec3::from_real(rh) * f.et[0] + CVec3::from_real(ph) * ephi + CVec3::from_real(Vec3::new(0.0, 0.0, 1.0)) * f.et[1])
                            * rot;
                        h += (CVec3::from_real(rh) * cr + CVec3::from_real(ph) * f.curl_phi + CVec3::from_real(Vec3::new(0.0, 0.0, 1.0)) * cz)
                            * (rot * hs);
                    }
                    samples.push(HuygensSample {
                        point: Point3::new(q[0] * c, q[0] * sn, q[1]),
                        weight,
                        normal: Vec3::new(nr * c, nr * sn, nz),
                        e,
                        h,
                    });
                }
            }
        }
    }
    Ok(Huygens { frequency, centre: layer.centre(), samples })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sphere of radius `a` about the origin in triangles of about `size`.
    fn ball(a: f64, size: f64) -> Profile {
        let n = ((PI * a / size).ceil() as usize).max(8);
        let points: Vec<[f64; 2]> =
            (0..=n).map(|k| PI * k as f64 / n as f64).map(|t| [if k_end(t) { 0.0 } else { a * t.sin() }, -a * t.cos()]).collect();
        let mut segments: Vec<[u32; 2]> = (0..n as u32).map(|k| [k, k + 1]).collect();
        segments.push([n as u32, 0]);
        let (points, triangles) = emcraft_mesh::planar::pslg(&points, &segments, size).unwrap();
        let material = vec![(1.0, 1.0); triangles.len()];
        Profile { points, triangles, material }
    }

    fn k_end(t: f64) -> bool {
        t == 0.0 || t == PI
    }

    /// A Hertzian dipole at the centre of a spherical layer radiates η (k I l)²/(12π) with
    /// directivity 3/2 (Balanis), the far field sin θ.
    #[test]
    fn a_dipole_on_the_axis_radiates_its_closed_form() {
        let f = 3e9;
        let lambda = C0 / f;
        let profile = ball(0.6 * lambda, lambda / 30.0);
        let layer = Layer::new(LayerShape::Sphere { radius: 0.6 * lambda }, 0.2 * lambda, 1e-6).unwrap();
        let il = 1e-3;
        let r = radiate(&profile, &layer, &BorSource::AxialDipole { z: 0.0, moment: il }, f).unwrap();
        let want = emcraft_theory::radiation::dipole_power(il, f);
        let got = r.huygens.power();
        assert!((got / want - 1.0).abs() < 0.03, "{got} W against {want} W");
        let dmax = r.huygens.directivity(PI / 2.0, 0.0);
        assert!((dmax - 1.5).abs() < 0.03, "{dmax}");
        let (et, _) = r.huygens.far_field(PI / 2.0, 0.0);
        let exact = emcraft_theory::radiation::dipole_far_field(il, f, PI / 2.0);
        assert!((et.norm() / exact - 1.0).abs() < 0.03, "{} against {exact}", et.norm());
    }
}

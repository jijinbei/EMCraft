//! The perfectly matched layer of open problems: slabs on a box's faces (Z. S. Sacks,
//! D. M. Kingsland, R. Lee, J.-F. Lee, IEEE Trans. AP 43(12), 1995; Jin 2014, §9.6) or the
//! outer shell of a sphere (F. L. Teixeira, W. C. Chew, IEEE Microwave Guided Wave Lett. 7(11),
//! 1997), assembled at each frequency since its stretches depend on k₀.

use std::f64::consts::PI;

use emcraft_fem::Order;
use emcraft_model::units::C0;
use emcraft_model::{Mesh, Model, OpenShape, Shape, Solid, Vec3};
use num_complex::Complex64 as C;

use super::assembly::{Coupling, couplings};
use super::{Driven, Result, invalid};
use crate::sparse::Csr;

/// The reflection a perfectly matched layer is designed for at normal incidence, before
/// discretization: its stretch is s = 1 − j(σ₀/k₀)(ξ/d)², σ₀ = 3 ln(1/R)/(2d).
pub const PML_REFLECTION: f64 = 1e-6;

/// A perfectly matched layer prepared for assembly.
#[derive(Clone, Debug)]
pub(crate) struct Layer {
    pub thickness: f64,
    /// The reflection at normal incidence its profile is designed for.
    pub reflection: f64,
    pub kind: LayerKind,
}

#[derive(Clone, Debug)]
pub(crate) enum LayerKind {
    /// Slabs on a box's faces: per tetrahedron, its centroid's depth into the layer along x, y
    /// and z as fractions of the thickness (0 where it is not in the layer along that axis).
    Box(Vec<(usize, Vec3)>),
    /// The shell outside the sphere of radius `inner` about the origin: its tetrahedra, whose
    /// tensors are evaluated at each quadrature point.
    Sphere { inner: f64, tets: Vec<usize> },
}

impl Layer {
    /// The layer lining the model's open boundary (open problems only): its tetrahedra by their
    /// centroids' depth into a box's slabs, or those reaching out of a sphere's inner radius.
    pub(super) fn of(model: &Model, mesh: &Mesh) -> Result<Option<Layer>> {
        let Some(o) = &model.open else { return Ok(None) };
        let reflection = o.reflection.unwrap_or(PML_REFLECTION);
        let kind = match (&model.solid, o.shape) {
            (Some(Solid { shape: Shape::Box { size }, .. }), OpenShape::Box) => {
                let d = o.thickness;
                let mut list = Vec::new();
                for t in 0..mesh.tets.len() {
                    let centroid = mesh.centroid(t).ok_or_else(|| invalid("mesh", format!("no tetrahedron {t}")))?;
                    let depth = centroid.to_vec().zip_map(*size, |c, l| ((d - c).max(c - (l - d)).max(0.0) / d).min(1.0));
                    if depth.iter().any(|x| x > 0.0) {
                        list.push((t, depth));
                    }
                }
                LayerKind::Box(list)
            }
            (Some(Solid { shape: Shape::Sphere { radius }, .. }), OpenShape::Sphere) => {
                // The shell's tetrahedra: a corner outside the inner sphere (the mesh's interface
                // there keeps the others wholly inside it, where the tensor is the identity).
                let inner = radius - o.thickness;
                let tol = 1e-9 * radius;
                let mut tets = Vec::new();
                for t in 0..mesh.tets.len() {
                    let corners = mesh.corners(t).ok_or_else(|| invalid("mesh", format!("no tetrahedron {t}")))?;
                    if corners.iter().any(|c| c.to_vec().norm() > inner + tol) {
                        tets.push(t);
                    }
                }
                LayerKind::Sphere { inner, tets }
            }
            _ => {
                return Err(invalid(
                    "open boundary",
                    "the layer's shape does not match the solid (a box layer on a box, a spherical one on a sphere)",
                ));
            }
        };
        Ok(Some(Layer { thickness: o.thickness, reflection, kind }))
    }

    /// The layer's tetrahedra.
    pub fn tets(&self) -> std::collections::HashSet<usize> {
        match &self.kind {
            LayerKind::Box(list) => list.iter().map(|(t, _)| *t).collect(),
            LayerKind::Sphere { tets, .. } => tets.iter().copied().collect(),
        }
    }

    /// The absorption profile's strength σ₀ = 3 ln(1/R)/(2d): σ(ξ) = σ₀ξ² over the depth
    /// fraction ξ, so a wave crossing the layer and back is damped by e^{−2σ₀d/3} = R.
    fn sigma0(&self) -> f64 {
        3.0 * (1.0 / self.reflection).ln() / (2.0 * self.thickness)
    }
}

/// The spherical layer's tensor at radius r (Teixeira and Chew 1997): the radial stretch
/// s_r = 1 − j(σ₀/k₀)ξ², ξ = (r − r₀)/d, and the stretched radius r̃ = r + (1/jk₀)∫_{r₀}^r σ dr'
/// = r − j(σ₀/k₀) d ξ³/3, give Λ = a r̂r̂ + b (I − r̂r̂) with a = (r̃/r)²/s_r, b = s_r. Returns
/// (a, b); (1, 1) inside the inner sphere.
pub(crate) fn spherical_stretch(r: f64, inner: f64, thickness: f64, sigma0: f64, k0: f64) -> (C, C) {
    let xi = ((r - inner) / thickness).clamp(0.0, 1.0);
    if xi == 0.0 || r <= 0.0 {
        return (C::new(1.0, 0.0), C::new(1.0, 0.0));
    }
    let g = sigma0 / k0;
    let s = C::new(1.0, -g * xi * xi);
    let rt = C::new(r, -g * thickness * xi * xi * xi / 3.0);
    let q = rt / r;
    (q * q / s, s)
}

impl Driven {
    /// The layer's tetrahedra (none without a layer).
    pub(crate) fn pml_tets(&self) -> std::collections::HashSet<usize> {
        self.pml.as_ref().map(Layer::tets).unwrap_or_default()
    }

    /// Adds the layer's tetrahedra at wavenumber k₀: with stretches s_d = 1 − j(σ₀/k₀)ξ_d², the
    /// layer is the anisotropic medium εr Λ, μr Λ, Λ = diag(s_y s_z/s_x, s_z s_x/s_y, s_x s_y/s_z)
    /// (Z. S. Sacks, D. M. Kingsland, R. Lee, J.-F. Lee, IEEE Trans. AP 43(12), 1995; Jin 2014,
    /// §9.6), so its element matrices are Σ_d (1/(μr Λ_d)) K_d − k₀² Σ_d εr Λ_d M_d by component.
    pub(crate) fn add_pml(&self, a: &mut Csr, k0: f64) -> Result<()> {
        let Some(layer) = &self.pml else { return Ok(()) };
        let sigma0 = layer.sigma0();
        let list = match &layer.kind {
            LayerKind::Box(list) => list,
            LayerKind::Sphere { inner, tets } => return self.add_spherical_pml(a, k0, *inner, layer.thickness, sigma0, tets),
        };
        for &(t, depth) in list {
            let s = [depth.x, depth.y, depth.z].map(|x| C::new(1.0, -sigma0 / k0 * x * x));
            let lambda = [s[1] * s[2] / s[0], s[2] * s[0] / s[1], s[0] * s[1] / s[2]];
            let el = self.element_of(t)?;
            let (kc, mc) = (el.stiffness_components(), el.mass_components());
            let unknowns: Vec<u32> = self.dofs_of(t).collect();
            let n = unknowns.len();
            let (eps, mu) = (self.eps_at(t, k0 * C0 / (2.0 * PI)), self.mu_of(t));
            for Coupling { i, j, row, col } in couplings(&unknowns) {
                let mut v = C::new(0.0, 0.0);
                for k in 0..3 {
                    v += kc[k][i * n + j] / (mu * lambda[k]) - k0 * k0 * eps * lambda[k] * mc[k][i * n + j];
                }
                a.add(row, col, v);
            }
        }
        Ok(())
    }

    /// Adds the spherical layer's tetrahedra at wavenumber k₀. Stretching the radius alone,
    /// r → r̃ (W. C. Chew, W. H. Weedon, Microwave Opt. Technol. Lett. 7(13), 1994), is the
    /// Cartesian space filled with the anisotropic medium εr Λ, μr Λ of
    /// Λ = (r̃/r)² s_r⁻¹ r̂r̂ + s_r (θ̂θ̂ + φ̂φ̂) (F. L. Teixeira, W. C. Chew, IEEE Microwave Guided
    /// Wave Lett. 7(11), 1997) — [`spherical_stretch`]. Its axes turn with r̂ inside an element,
    /// so the full tensors are evaluated at each quadrature point:
    /// (Λ v)·w = b v·w + (a − b)(v·r̂)(w·r̂), (Λ⁻¹ v)·w = v·w/b + (1/a − 1/b)(v·r̂)(w·r̂).
    fn add_spherical_pml(&self, a: &mut Csr, k0: f64, inner: f64, thickness: f64, sigma0: f64, tets: &[usize]) -> Result<()> {
        let n = self.order.dofs();
        let mut local = vec![C::new(0.0, 0.0); n * n];
        for &t in tets {
            let el = self.element_of(t)?;
            // Exact for the constant-tensor integrands of straight elements (degree 2n − 3 ≥ the
            // basis' products'), one more for curved ones and the tensor's variation.
            let points = match (self.order, el.curve.is_some()) {
                (Order::One, false) => 3,
                (Order::One, true) | (Order::Two, false) => 4,
                (Order::Two, true) => 5,
            };
            // (The permittivity without dispersion, unlike the box's layer: a dispersive
            // material's poles are not evaluated in the spherical shell.)
            let (eps, mu) = (self.media.eps(t), self.mu_of(t));
            local.iter_mut().for_each(|x| *x = C::new(0.0, 0.0));
            for (l, w) in el.rule(points) {
                let p = el.point(l).to_vec();
                let r = p.norm();
                let (sa, sb) = spherical_stretch(r, inner, thickness, sigma0, k0);
                let rhat = if r > 0.0 { p / r } else { Vec3::ZERO };
                let (v, c) = (el.values(l), el.curl_values(l));
                let (vr, cr): (Vec<f64>, Vec<f64>) = v.iter().zip(&c).map(|(v, c)| (v.dot(rhat), c.dot(rhat))).unzip();
                // Stiffness (1/μr)Λ⁻¹ and mass k₀² εr Λ, by their isotropic and radial parts.
                let (k_iso, k_rad) = (w / (mu * sb), (1.0 / sa - 1.0 / sb) * (w / mu));
                let (m_iso, m_rad) = (k0 * k0 * eps * sb * w, k0 * k0 * eps * (sa - sb) * w);
                for i in 0..n {
                    for j in i..n {
                        local[i * n + j] += k_iso * c[i].dot(c[j]) + k_rad * (cr[i] * cr[j]) - m_iso * v[i].dot(v[j]) - m_rad * (vr[i] * vr[j]);
                    }
                }
            }
            let unknowns: Vec<u32> = self.dofs_of(t).collect();
            for Coupling { i, j, row, col } in couplings(&unknowns) {
                a.add(row, col, local[i.min(j) * n + i.max(j)]);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::NONE;
    use super::*;
    use emcraft_model::{Open, OpenShape, Shape, Solid};

    #[test]
    fn the_spherical_stretch_is_the_closed_form() {
        // Inside the inner sphere: no stretch.
        assert_eq!(spherical_stretch(0.5, 1.0, 0.5, 10.0, 2.0), (C::new(1.0, 0.0), C::new(1.0, 0.0)));
        // Halfway in: s = 1 − j(σ₀/k₀)ξ², r̃ = r − j(σ₀/k₀)dξ³/3.
        let (a, b) = spherical_stretch(1.25, 1.0, 0.5, 10.0, 2.0);
        let s = C::new(1.0, -5.0 * 0.25);
        let rt = C::new(1.25, -5.0 * 0.5 * 0.125 / 3.0);
        assert!((b - s).norm() < 1e-15);
        assert!((a - (rt / 1.25) * (rt / 1.25) / s).norm() < 1e-15);
        // Beyond the outer radius the profile stays at its last value.
        assert_eq!(spherical_stretch(2.0, 1.0, 0.5, 10.0, 2.0).1, spherical_stretch(1.5, 1.0, 0.5, 10.0, 2.0).1);
    }

    #[test]
    fn the_spherical_layer_is_the_vacuum_where_it_does_not_stretch() {
        // With the layer's inner radius beyond the mesh (Λ = I everywhere) its quadrature gives
        // the ordinary element matrices, curved elements on the sphere's wall included.
        let mut m = Model::new();
        m.set_solid(Solid { shape: Shape::Sphere { radius: 0.012 }, material: "vacuum".into() }).unwrap();
        m.set_open(Some(Open { thickness: 0.004, reflection: None, shape: OpenShape::Sphere })).unwrap();
        m.mesh = Some(emcraft_mesh::solid::solid(&m, 0.006).unwrap());
        let d = Driven::open(&m, Order::Two).unwrap();
        let Some(Layer { kind: LayerKind::Sphere { tets, .. }, .. }) = &d.pml else { panic!("no spherical layer") };
        assert!(!tets.is_empty() && tets.len() < d.mesh.tets.len());
        let all: Vec<usize> = (0..d.mesh.tets.len()).collect();
        let k0 = 2.0 * PI * 10e9 / C0;
        let mut p = d.pattern.zeroed();
        d.add_spherical_pml(&mut p, k0, 1.0, 0.004, 100.0, &all).unwrap();
        let mut q = d.pattern.zeroed();
        for t in all {
            let el = d.element_of(t).unwrap();
            let (s, mm) = (el.stiffness(1.0), el.mass(1.0));
            let u: Vec<u32> = d.dofs_of(t).collect();
            let n = u.len();
            for i in (0..n).filter(|&i| u[i] != NONE) {
                for j in (0..n).filter(|&j| u[j] != NONE) {
                    q.add(u[i] as usize, u[j] as usize, C::new(s[i * n + j] - k0 * k0 * mm[i * n + j], 0.0));
                }
            }
        }
        let scale = q.max_abs();
        let diff = p.val.iter().zip(&q.val).map(|(a, b)| (a - b).norm()).fold(0.0, f64::max);
        assert!(diff < 1e-10 * scale, "{diff} {scale}");
    }
}

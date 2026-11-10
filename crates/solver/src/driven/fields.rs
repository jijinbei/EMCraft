//! Fields from the solution coefficients: E and H at points (beyond a symmetry plane, the
//! images of those inside; beyond a rotational sector, the field inside turned on and phased),
//! their L² error against an exact field, and integrals of them over
//! the volume and the walls.

use emcraft_model::units::angular;

use emcraft_fem::Element;
use emcraft_model::units::MU0;
use emcraft_model::{CVec3, Image, Point3, Symmetry, Vec3};
use num_complex::Complex64 as C;

use super::assembly::rule_points;
use super::{Driven, Field, Result};

impl Driven {
    /// The field (E in V/m or H in A/m) at a point for the coefficients `x` at `frequency`.
    /// Beyond a symmetry plane, the image of the field inside.
    pub fn field(&self, x: &[C], frequency: f64, which: Field, p: Point3) -> Option<CVec3> {
        let (p, turns) = self.back_to_sector(p);
        let Image { point: q, e: se, h: sh } = Symmetry::image_through(&self.symmetry, p);
        let (t, _) = emcraft_mesh::locate(&self.mesh, q)?;
        let el = self.element_of(t).ok()?;
        let v = self.field_in((t, &el), el.barycentric(q), x, frequency, which);
        let v = flipped(
            v,
            match which {
                Field::E => se,
                Field::H => sh,
            },
        );
        Some(self.turned_on(v, turns))
    }

    /// A point beyond a rotational sector: the point inside it `k` sectors back, and k (0 within
    /// it, and without a rotation).
    fn back_to_sector(&self, p: Point3) -> (Point3, u32) {
        let Some(r) = &self.rotation else { return (p, 0) };
        let phi = (p.y - r.centre[1]).atan2(p.x - r.centre[0]);
        let k = ((phi - r.start()).rem_euclid(std::f64::consts::TAU) / r.angle()).floor() as u32 % r.n.max(1);
        (r.turned_by(p, -f64::from(k) * r.angle()), k)
    }

    /// A field inside the sector carried `k` sectors on: turned with them and times
    /// e^{−jkψ}, the harmonic's phase per sector (the cyclic condition E(R r) = e^{−jψ} R E(r)).
    fn turned_on(&self, v: CVec3, k: u32) -> CVec3 {
        let Some(r) = self.rotation.as_ref().filter(|_| k > 0) else { return v };
        let a = f64::from(k) * r.angle();
        let (s, c) = a.sin_cos();
        let phase = C::from_polar(1.0, -f64::from(k) * r.phase());
        CVec3 { x: (v.x * c - v.y * s) * phase, y: (v.x * s + v.y * c) * phase, z: v.z * phase }
    }

    /// E and H at a point, `None` outside the mesh (beyond a symmetry plane, the image of the
    /// field inside).
    pub fn fields(&self, x: &[C], frequency: f64, p: Point3) -> Option<(CVec3, CVec3)> {
        let (p, turns) = self.back_to_sector(p);
        let Image { point: q, e: se, h: sh } = Symmetry::image_through(&self.symmetry, p);
        let (t, _) = emcraft_mesh::locate(&self.mesh, q)?;
        let el = self.element_of(t).ok()?;
        let l = el.barycentric(q);
        let (e, h) =
            (flipped(self.field_in((t, &el), l, x, frequency, Field::E), se), flipped(self.field_in((t, &el), l, x, frequency, Field::H), sh));
        Some((self.turned_on(e, turns), self.turned_on(h, turns)))
    }

    /// The field `which` at barycentric coordinates `l` of tetrahedron `t` (its element `el`).
    pub(crate) fn field_in(&self, (t, el): (usize, &Element), l: [f64; 4], x: &[C], frequency: f64, which: Field) -> CVec3 {
        let values = match which {
            Field::E => el.values(l),
            Field::H => el.curl_values(l),
        };
        let field: CVec3 = self.dofs_of(t).zip(&values).filter_map(|(u, v)| Some(*v * x.get(u.get()?).copied().unwrap_or_default())).sum();
        match which {
            Field::E => field,
            // H = −∇ × E / (jωμ).
            // In a ferrite H = [μ]⁻¹ B: B = −∇ × E/(jω), the inverse Polder tensor in 1/μ₀.
            Field::H if let Some(f) = self.media.ferrite(t) => {
                let b = -field / C::new(0.0, angular(frequency) * MU0);
                let nu = f.nu(frequency);
                let (ax, bx, cx) = f.axes();
                let mut h = b;
                h[bx] = nu.diagonal * b[bx] + nu.off * b[cx];
                h[cx] = -nu.off * b[bx] + nu.diagonal * b[cx];
                h[ax] = b[ax];
                h
            }
            Field::H => {
                let mu = self.mu_of(t);
                let omega_mu0 = angular(frequency) * MU0;
                if mu.im == 0.0 { -field / C::new(0.0, omega_mu0 * mu.re) } else { -field / (C::new(0.0, omega_mu0) * mu) }
            }
        }
    }

    /// The power density (W/m³) a magnetized ferrite absorbs at barycentric `l` of tetrahedron
    /// `t`: −½ω Im(H*·B), B = −∇ × E/(jω) and H = [μ]⁻¹B — for a scalar μ = μ₀(μ′ − jμ″) the
    /// familiar ½ωμ₀μ″|H|², for the Polder tensor its anti-Hermitian part's.
    pub(crate) fn ferrite_loss_at(&self, (t, el): (usize, &Element), l: [f64; 4], x: &[C], frequency: f64) -> f64 {
        let omega = angular(frequency);
        let curl: CVec3 = self.dofs_of(t).zip(&el.curl_values(l)).filter_map(|(u, v)| Some(*v * x.get(u.get()?).copied().unwrap_or_default())).sum();
        let b = -curl / C::new(0.0, omega);
        let h = self.field_in((t, el), l, x, frequency, Field::H);
        let hb = h.x.conj() * b.x + h.y.conj() * b.y + h.z.conj() * b.z;
        -0.5 * omega * hb.im
    }

    /// ‖field − exact‖ and ‖exact‖ in L²(Ω), by quadrature on every tetrahedron.
    pub fn l2_error(&self, x: &[C], frequency: f64, which: Field, exact: impl Fn(Point3) -> CVec3) -> Result<(f64, f64)> {
        let n = rule_points(self.order);
        let (mut err, mut norm) = (0.0, 0.0);
        for t in 0..self.mesh.tets.len() {
            let el = self.element_of(t)?;
            for (l, w) in el.rule(n) {
                let (u, e) = (self.field_in((t, &el), l, x, frequency, which), exact(el.point(l)));
                err += w * (u - e).norm_sqr();
                norm += w * e.norm_sqr();
            }
        }
        Ok((err.sqrt(), norm.sqrt()))
    }

    /// ∫ f(t, E) dV over every tetrahedron `keep` admits, E the field at each point of a rule.
    pub(super) fn integrate_e(&self, x: &[C], frequency: f64, keep: impl Fn(usize) -> bool, f: impl Fn(usize, CVec3) -> f64) -> Result<f64> {
        self.integrate_field(x, frequency, Field::E, keep, f)
    }

    /// ∫ f(t, field) dV over every tetrahedron `keep` admits, the field `which` at each point of
    /// a rule.
    pub(super) fn integrate_field(
        &self,
        x: &[C],
        frequency: f64,
        which: Field,
        keep: impl Fn(usize) -> bool,
        f: impl Fn(usize, CVec3) -> f64,
    ) -> Result<f64> {
        let n = rule_points(self.order);
        (0..self.mesh.tets.len()).filter(|t| keep(*t)).try_fold(0.0, |total, t| {
            let el = self.element_of(t)?;
            Ok(total + el.rule(n).into_iter().map(|(l, w)| w * f(t, self.field_in((t, &el), l, x, frequency, which))).sum::<f64>())
        })
    }

    /// ∫ f(field) dS over the walls, the field `which` and the wall's unit normal at each point;
    /// a sheet inside the mesh on both its faces, each with the field on its side.
    pub(super) fn integrate_walls(&self, x: &[C], frequency: f64, which: Field, f: impl Fn(CVec3, Vec3) -> f64) -> Result<f64> {
        let (space, n) = (self.space(), rule_points(self.order));
        self.walls.iter().filter_map(|&i| self.mesh.boundary.get(i)).flat_map(|b| space.sides(b)).try_fold(0.0, |total, b| {
            let face = space.face_rule(&b, n)?;
            let (el, tet) = (&face.el, face.tet);
            Ok(total + face.points.iter().map(|&(l, wa, normal)| wa * f(self.field_in((tet, el), l, x, frequency, which), normal)).sum::<f64>())
        })
    }
}

/// The largest electric field of a solution: its peak over time (V/m) and where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PeakField {
    pub value: f64,
    pub at: Point3,
}

/// The largest |e(t)| over a cycle of the phasor `e` (e(t) = Re(e e^{jωt})): the major
/// semi-axis of its ellipse, √((|e|² + |e·e|)/2) — |e| for a linearly polarized field.
///
/// ```
/// use emcraft_model::CVec3;
/// use num_complex::Complex64 as C;
/// // Linear: its length; circular (x + jy): one, not √2.
/// let linear = CVec3::new(C::new(3.0, 0.0), C::new(0.0, 0.0), C::new(4.0, 0.0));
/// assert!((emcraft_solver::driven::time_peak(linear) - 5.0).abs() < 1e-12);
/// let circular = CVec3::new(C::new(1.0, 0.0), C::new(0.0, 1.0), C::new(0.0, 0.0));
/// assert!((emcraft_solver::driven::time_peak(circular) - 1.0).abs() < 1e-12);
/// ```
pub fn time_peak(e: CVec3) -> f64 {
    let squared = e.x.norm_sqr() + e.y.norm_sqr() + e.z.norm_sqr();
    let dot = e.x * e.x + e.y * e.y + e.z * e.z;
    (0.5 * (squared + dot.norm())).sqrt()
}

impl Driven {
    /// The largest electric field of the coefficients `x` ([`time_peak`]), over the points of
    /// each tetrahedron's rule and its corners, outside the matched layer; `None` for no
    /// tetrahedra. The corners reach the walls, where the largest fields of irises and posts are;
    /// sampled so, the largest value overshoots the true one by the elements' interpolation error
    /// (3 % for a guide's TE10 with second-order elements of λ/10, less as they shrink) — the safe
    /// side for breakdown. The field at a metal edge grows as the mesh refines (it is singular
    /// there): the value is the mesh's.
    pub fn peak_e(&self, x: &[C], frequency: f64) -> Result<Option<PeakField>> {
        let in_pml = self.pml_tets();
        let n = rule_points(self.order);
        let corners = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let mut best: Option<PeakField> = None;
        for t in (0..self.mesh.tets.len()).filter(|t| !in_pml.contains(t)) {
            let el = self.element_of(t)?;
            let points = el.rule(n).into_iter().map(|(l, _)| l).chain(corners);
            for l in points {
                let value = time_peak(self.field_in((t, &el), l, x, frequency, Field::E));
                if value.is_finite() && best.is_none_or(|b| value > b.value) {
                    best = Some(PeakField { value, at: el.point(l) });
                }
            }
        }
        Ok(best)
    }
}

/// A field with its components' signs changed.
fn flipped(mut v: CVec3, signs: [f64; 3]) -> CVec3 {
    for (k, s) in signs.iter().enumerate() {
        v[k] *= *s;
    }
    v
}

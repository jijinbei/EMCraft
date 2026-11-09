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
            Field::H => {
                let mu = self.mu_of(t);
                let omega_mu0 = angular(frequency) * MU0;
                if mu.im == 0.0 { -field / C::new(0.0, omega_mu0 * mu.re) } else { -field / (C::new(0.0, omega_mu0) * mu) }
            }
        }
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

/// A field with its components' signs changed.
fn flipped(mut v: CVec3, signs: [f64; 3]) -> CVec3 {
    for (k, s) in signs.iter().enumerate() {
        v[k] *= *s;
    }
    v
}

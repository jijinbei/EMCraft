//! Fields from the solution coefficients: E and H at points (beyond a symmetry plane, the
//! images of those inside), their L² error against an exact field, and integrals of them over
//! the volume and the walls.

use std::f64::consts::PI;

use emcraft_fem::Element;
use emcraft_model::units::MU0;
use emcraft_model::{CVec3, Image, Point3, Symmetry, Vec3};
use num_complex::Complex64 as C;

use super::assembly::rule_points;
use super::{Driven, Field, NONE, Result};

impl Driven {
    /// The field (E in V/m or H in A/m) at a point for the coefficients `x` at `frequency`.
    /// Beyond a symmetry plane, the image of the field inside.
    pub fn field(&self, x: &[C], frequency: f64, which: Field, p: Point3) -> Option<CVec3> {
        let Image { point: q, e: se, h: sh } = Symmetry::image_through(&self.symmetry, p);
        let (t, _) = emcraft_mesh::locate(&self.mesh, q)?;
        let el = self.element_of(t).ok()?;
        let v = self.field_in(&el, t, x, frequency, which, el.barycentric(q));
        Some(flipped(
            v,
            match which {
                Field::E => se,
                Field::H => sh,
            },
        ))
    }

    /// E and H at a point, `None` outside the mesh (beyond a symmetry plane, the image of the
    /// field inside).
    pub fn fields(&self, x: &[C], frequency: f64, p: Point3) -> Option<(CVec3, CVec3)> {
        let Image { point: q, e: se, h: sh } = Symmetry::image_through(&self.symmetry, p);
        let (t, _) = emcraft_mesh::locate(&self.mesh, q)?;
        let el = self.element_of(t).ok()?;
        let l = el.barycentric(q);
        Some((flipped(self.field_in(&el, t, x, frequency, Field::E, l), se), flipped(self.field_in(&el, t, x, frequency, Field::H, l), sh)))
    }

    /// The field `which` at barycentric coordinates `l` of tetrahedron `t` (its element `el`).
    pub(crate) fn field_in(&self, el: &Element, t: usize, x: &[C], frequency: f64, which: Field, l: [f64; 4]) -> CVec3 {
        let values = match which {
            Field::E => el.values(l),
            Field::H => el.curl_values(l),
        };
        let field: CVec3 =
            self.dofs_of(t).zip(&values).filter(|(u, _)| *u != NONE).map(|(u, v)| *v * x.get(u as usize).copied().unwrap_or_default()).sum();
        match which {
            Field::E => field,
            // H = −∇ × E / (jωμ).
            Field::H => -field / C::new(0.0, 2.0 * PI * frequency * MU0 * self.mu_of(t)),
        }
    }

    /// ‖field − exact‖ and ‖exact‖ in L²(Ω), by quadrature on every tetrahedron.
    pub fn l2_error(&self, x: &[C], frequency: f64, which: Field, exact: impl Fn(Point3) -> CVec3) -> Result<(f64, f64)> {
        let n = rule_points(self.order);
        let (mut err, mut norm) = (0.0, 0.0);
        for t in 0..self.mesh.tets.len() {
            let el = self.element_of(t)?;
            for (l, w) in el.rule(n) {
                let (u, e) = (self.field_in(&el, t, x, frequency, which, l), exact(el.point(l)));
                err += w * (u - e).norm_sqr();
                norm += w * e.norm_sqr();
            }
        }
        Ok((err.sqrt(), norm.sqrt()))
    }

    /// ∫ f(t, E) dV over every tetrahedron `keep` admits, E the field at each point of a rule.
    pub(super) fn integrate_e(&self, x: &[C], frequency: f64, keep: impl Fn(usize) -> bool, f: impl Fn(usize, CVec3) -> f64) -> Result<f64> {
        let n = rule_points(self.order);
        (0..self.mesh.tets.len()).filter(|t| keep(*t)).try_fold(0.0, |total, t| {
            let el = self.element_of(t)?;
            Ok(total + el.rule(n).into_iter().map(|(l, w)| w * f(t, self.field_in(&el, t, x, frequency, Field::E, l))).sum::<f64>())
        })
    }

    /// ∫ f(field) dS over the walls, the field `which` and the wall's unit normal at each point.
    pub(super) fn integrate_walls(&self, x: &[C], frequency: f64, which: Field, f: impl Fn(CVec3, Vec3) -> f64) -> Result<f64> {
        let (space, n) = (self.space(), rule_points(self.order));
        self.walls.iter().filter_map(|&i| self.mesh.boundary.get(i)).try_fold(0.0, |total, b| {
            let face = space.face_rule(b, n)?;
            let (el, tet) = (&face.el, face.tet);
            Ok(total + face.points.iter().map(|&(l, wa, normal)| wa * f(self.field_in(el, tet, x, frequency, which, l), normal)).sum::<f64>())
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

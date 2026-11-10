//! The steady temperature the losses of a solved field raise: −∇·(k ∇T) = q in the mesh, q the
//! power density the dielectrics and magnetic materials absorb (½ωε₀ε″|E|² + ½ωμ₀μ″|H|², fields in
//! peak values), the walls held at the heat sink's temperature (T = 0 here: the rise), the ports
//! and symmetry planes adiabatic (no heat crosses them). Linear (P1) elements on the mesh's
//! corners (O. C. Zienkiewicz, R. L. Taylor, *The Finite Element Method*, vol. 1, 5th ed., 2000,
//! §3.2 and ch. 7: the element conductivity k V ∇λᵢ·∇λⱼ and load ∫ q λᵢ dV); the walls' own loss
//! leaves through them into the sink, so it heats nothing inside.

use std::collections::BTreeMap;

use emcraft_model::units::{MU0, angular};
use emcraft_model::{Point3, Vec3};
use num_complex::Complex64 as C;

use super::assembly::rule_points;
use super::{Driven, Field, Result, invalid};
use crate::ldlt::Ldlt;

/// ε₀ (F/m).
fn eps0() -> f64 {
    1.0 / (MU0 * emcraft_model::units::C0 * emcraft_model::units::C0)
}

/// A temperature solve's result: the rise (K) at each mesh corner node (0 on the walls, `None`
/// for a node no tetrahedron uses), the largest and where, and the heat (W) the field deposits.
#[derive(Clone, Debug, PartialEq)]
pub struct TemperatureRise {
    pub rise: Vec<Option<f64>>,
    pub peak: f64,
    pub at: Point3,
    pub heat: f64,
}

/// The gradients of a tetrahedron's barycentric coordinates and its volume; `None` when flat.
fn gradients(p: [Point3; 4]) -> Option<([Vec3; 4], f64)> {
    let (e1, e2, e3) = (p[1] - p[0], p[2] - p[0], p[3] - p[0]);
    let det = e1.dot(e2.cross(e3));
    if !(det.abs() > 0.0) || !det.is_finite() {
        return None;
    }
    let g1 = e2.cross(e3) / det;
    let g2 = e3.cross(e1) / det;
    let g3 = e1.cross(e2) / det;
    let g0 = -(g1 + g2 + g3);
    Some(([g0, g1, g2, g3], det.abs() / 6.0))
}

impl Driven {
    /// The temperature rise of the field `x` (scaled by `scale`, the field multiplier: √ of the
    /// power over the excitation's unit wave) at `frequency`, each tetrahedron conducting heat
    /// at `conductivity(t)` (W/(m·K), > 0).
    pub fn temperature(&self, x: &[C], frequency: f64, scale: f64, conductivity: impl Fn(usize) -> f64) -> Result<TemperatureRise> {
        let omega = angular(frequency);
        let mesh = &self.mesh;
        // The walls' nodes are held at the sink's temperature.
        let mut held = vec![false; mesh.nodes.len()];
        for b in self.walls.iter().filter_map(|&i| mesh.boundary.get(i)) {
            for &v in &b.nodes {
                if let Some(h) = held.get_mut(v as usize) {
                    *h = true;
                }
            }
        }
        // The free nodes, numbered in order of first use.
        let mut place: BTreeMap<u32, usize> = BTreeMap::new();
        for tet in &mesh.tets {
            for &v in tet {
                if !held.get(v as usize).copied().unwrap_or(true) {
                    let n = place.len();
                    place.entry(v).or_insert(n);
                }
            }
        }
        if !held.iter().any(|h| *h) {
            return Err(invalid("temperature", "needs walls held at the sink's temperature: this model has none"));
        }
        let n = place.len();
        let mut points = vec![Point3::ORIGIN; n];
        for (&v, &k) in &place {
            if let (Some(slot), Some(q)) = (points.get_mut(k), mesh.nodes.get(v as usize)) {
                *slot = *q;
            }
        }
        let groups: Vec<Vec<usize>> = mesh.tets.iter().map(|t| t.iter().filter_map(|v| place.get(v).copied()).collect()).collect();
        let mut a = crate::sparse::Csr::from_groups(n, groups.iter().map(Vec::as_slice));
        let mut f = vec![C::new(0.0, 0.0); n];
        let rule = rule_points(self.order);
        let in_pml = self.pml_tets();
        let mut heat = 0.0;
        for (t, tet) in mesh.tets.iter().enumerate() {
            let corners = tet.map(|v| mesh.nodes.get(v as usize).copied().unwrap_or(Point3::ORIGIN));
            let (grad, volume) = gradients(corners).ok_or_else(|| invalid("temperature", format!("tetrahedron {t} is flat")))?;
            let k = conductivity(t);
            if !(k > 0.0 && k.is_finite()) {
                return Err(invalid("temperature", format!("tetrahedron {t} has no thermal conductivity")));
            }
            for (i, vi) in tet.iter().enumerate() {
                let Some(&pi) = place.get(vi) else { continue };
                for (j, vj) in tet.iter().enumerate() {
                    if let Some(&pj) = place.get(vj) {
                        a.add(pi, pj, C::new(k * volume * grad[i].dot(grad[j]), 0.0));
                    }
                }
            }
            // The heat source: the field's loss density at the rule's points.
            let (e_loss, m_loss) = (-self.eps_at(t, frequency).im, -self.mu_of(t).im);
            let lossy_ferrite = self.media.ferrite(t).is_some_and(|f| f.linewidth > 0.0);
            if in_pml.contains(&t) || !(e_loss > 0.0 || m_loss > 0.0 || lossy_ferrite) {
                continue;
            }
            let el = self.element_of(t)?;
            for (l, w) in el.rule(rule) {
                let e = self.field_in((t, &el), l, x, frequency, Field::E);
                let mut q = 0.5 * omega * eps0() * e_loss * (e.x.norm_sqr() + e.y.norm_sqr() + e.z.norm_sqr());
                if lossy_ferrite {
                    q += self.ferrite_loss_at((t, &el), l, x, frequency);
                } else if m_loss > 0.0 {
                    let h = self.field_in((t, &el), l, x, frequency, Field::H);
                    q += 0.5 * omega * MU0 * m_loss * (h.x.norm_sqr() + h.y.norm_sqr() + h.z.norm_sqr());
                }
                let q = q * scale * scale;
                heat += w * q;
                for (i, vi) in tet.iter().enumerate() {
                    if let (Some(&pi), Some(li)) = (place.get(vi), l.get(i)) {
                        f[pi] += C::new(w * q * li, 0.0);
                    }
                }
            }
        }
        let factored = Ldlt::factor(&a, &points)?;
        let (solution, _) = factored.solve_refined(&a, &f, 2)?;
        let mut rise: Vec<Option<f64>> = vec![None; mesh.nodes.len()];
        for tet in &mesh.tets {
            for &v in tet {
                if let Some(slot) = rise.get_mut(v as usize) {
                    *slot = Some(place.get(&v).and_then(|&k| solution.get(k)).map_or(0.0, |c| c.re));
                }
            }
        }
        let (peak, at) = rise
            .iter()
            .enumerate()
            .filter_map(|(v, r)| Some((r.as_ref().copied()?, v)))
            .fold((0.0, 0usize), |best, (r, v)| if r > best.0 { (r, v) } else { best });
        Ok(TemperatureRise { rise, peak, at: mesh.nodes.get(at).copied().unwrap_or(Point3::ORIGIN), heat })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unit_tetrahedrons_gradients_sum_to_nothing() {
        let p = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0), Point3::new(0.0, 0.0, 1.0)];
        let (g, v) = gradients(p).unwrap();
        assert!((v - 1.0 / 6.0).abs() < 1e-15);
        assert!((g[1] - Vec3::new(1.0, 0.0, 0.0)).norm() < 1e-15 && (g[0] - Vec3::new(-1.0, -1.0, -1.0)).norm() < 1e-15);
        assert!(gradients([p[0], p[1], p[2], p[0]]).is_none());
    }
}

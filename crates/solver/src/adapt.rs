//! A residual a-posteriori error indicator for the driven curl–curl problem, per tetrahedron,
//! for adaptive mesh refinement.
//!
//! For ∇ × (μr⁻¹ ∇ × E) − k₀² εr E = 0 the residual of a Nédélec solution E_h lives in the
//! elements and on the faces between them. Tested with the H(curl) functions and with gradients
//! (the discrete Helmholtz decomposition), it gives the indicator
//!
//! η_K² = h_K² ‖k₀² εr E_h − ∇ × μr⁻¹ ∇ × E_h‖²_K
//!      + Σ_{F ⊂ ∂K interior} ½ h_F ( ‖[n × μr⁻¹ ∇ × E_h]_F‖²_F + k₀² ‖[n · εr E_h]_F‖²_F ),
//!
//! [·]_F the jump across F, h the diameters (P. Monk, *A posteriori error indicators for
//! Maxwell's equations*, J. Comput. Appl. Math. 100(2):173–190, 1998; P. Monk, *Finite Element
//! Methods for Maxwell's Equations*, Oxford 2003, ch. 12; R. Beck, R. Hiptmair, R. H. W. Hoppe,
//! B. Wohlmuth, *Residual based a posteriori error estimators for eddy current computation*,
//! M2AN 34(1):159–182, 2000, there with σ in place of −k₀²εr). The tangential jump comes from
//! the curl part of the test space, the normal jump of εE from the gradients (∇φ tested against
//! the equation leaves −k₀² ∫ εE · ∇φ, whose element parts integrate by parts to the divergence
//! and the normal jumps; scaled by 1/k₀ for the energy norm of ∇φ, hence k₀² ‖·‖²).
//!
//! Left out, with reasons: the element divergence term h_K² k₀² ‖∇ · εr E_h‖²_K (zero for
//! Whitney elements, whose divergence vanishes in each element; for order 2 of the size of the
//! normal jumps it accompanies, which do carry the gradient residual between elements); the
//! residual of the port conditions (ports lie where the field is a few modes the condition
//! carries exactly). The curl of the curl in an element is exact for straight order-2 elements
//! (their curl is linear: its gradient from the four corners' values); curved elements take the
//! same formula with the straight element's ∇λ, as an indicator.
//!
//! The per-tetrahedron values are summed over the excitations of a frequency result (every
//! port's unit incident wave), so a part of the model that matters for any S-parameter is
//! refined. Everything is deterministic: tetrahedra and faces in index order.

use emcraft_fem::Order;
use emcraft_fem::nedelec::LOCAL_FACES;
use emcraft_model::units::C0;
use emcraft_model::{CVec3, Vec3};
use num_complex::Complex64 as C;
use std::f64::consts::PI;

use crate::Driven;
use crate::driven::{FrequencyResult, NONE, Result, invalid};

/// Gauss points per direction of the element and face rules.
const RULE: usize = 3;

/// The longest edge of points.
fn diameter(p: &[emcraft_model::Point3]) -> f64 {
    let mut h: f64 = 0.0;
    for (i, a) in p.iter().enumerate() {
        for b in &p[i + 1..] {
            h = h.max(a.distance(*b));
        }
    }
    h
}

fn dot(a: CVec3, b: Vec3) -> C {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn cross(n: Vec3, a: CVec3) -> CVec3 {
    CVec3::new(n.y * a.z - n.z * a.y, n.z * a.x - n.x * a.z, n.x * a.y - n.y * a.x)
}

fn norm2(a: CVec3) -> f64 {
    a.x.norm_sqr() + a.y.norm_sqr() + a.z.norm_sqr()
}

/// A field from basis values and coefficients.
fn combine(values: &[Vec3], dofs: &[u32], x: &[C]) -> CVec3 {
    let mut f = CVec3::new(C::new(0.0, 0.0), C::new(0.0, 0.0), C::new(0.0, 0.0));
    for (v, &u) in values.iter().zip(dofs) {
        if u == NONE {
            continue;
        }
        let c = x.get(u as usize).copied().unwrap_or_default();
        f += CVec3::new(c * v.x, c * v.y, c * v.z);
    }
    f
}

/// The squared indicator η_K² of every tetrahedron for the solution `r` of `driven`, summed over
/// its excitations.
pub fn indicator(driven: &Driven, r: &FrequencyResult) -> Result<Vec<f64>> {
    if !(r.frequency.is_finite() && r.frequency > 0.0) {
        return Err(invalid("frequency", format!("must be a finite number > 0, got {}", r.frequency)));
    }
    let tets = driven.mesh.tets.len();
    if let Some(x) = r.fields.iter().find(|x| x.len() != driven.dofs.unknowns) {
        return Err(invalid("solution", format!("{} coefficients for {} unknowns", x.len(), driven.dofs.unknowns)));
    }
    let k0 = 2.0 * PI * r.frequency / C0;
    let k2 = k0 * k0;
    let mut eta = vec![0.0; tets];
    let dofs: Vec<Vec<u32>> = (0..tets).map(|t| driven.dofs_of(t).collect()).collect();
    // Element residuals.
    for t in 0..tets {
        let el = driven.element_of(t)?;
        let (eps, nu) = (driven.eps_at(t, r.frequency), 1.0 / driven.mu_of(t));
        let h = diameter(&el.corners);
        let corner_curls: Vec<Vec<Vec3>> = if el.order == Order::One {
            Vec::new()
        } else {
            (0..4).map(|i| el.curl_values(std::array::from_fn(|k| if k == i { 1.0 } else { 0.0 }))).collect()
        };
        let rule = el.rule(RULE);
        let values: Vec<Vec<Vec3>> = rule.iter().map(|(l, _)| el.values(*l)).collect();
        for x in &r.fields {
            // ∇ × (∇ × E): the curl's gradient from its corner values (linear in the element).
            let cc = if corner_curls.is_empty() {
                CVec3::new(C::new(0.0, 0.0), C::new(0.0, 0.0), C::new(0.0, 0.0))
            } else {
                let c: Vec<CVec3> = corner_curls.iter().map(|v| combine(v, &dofs[t], x)).collect();
                // ∂_j C_k = Σ_i C_i,k ∂_j λ_i.
                let d = |j: usize, k: usize| -> C { c.iter().zip(&el.grads).map(|(ci, g)| [ci.x, ci.y, ci.z][k] * [g.x, g.y, g.z][j]).sum() };
                CVec3::new(d(1, 2) - d(2, 1), d(2, 0) - d(0, 2), d(0, 1) - d(1, 0))
            };
            let mut res = 0.0;
            for ((_, w), v) in rule.iter().zip(&values) {
                let e = combine(v, &dofs[t], x);
                let rr = CVec3::new(e.x * eps * k2 - cc.x * nu, e.y * eps * k2 - cc.y * nu, e.z * eps * k2 - cc.z * nu);
                res += w * norm2(rr);
            }
            eta[t] += h * h * res;
        }
    }
    // Interior faces: the tangential jump of μr⁻¹ ∇ × E and the normal jump of εr E.
    let topo = &driven.topo;
    for (f, pair) in topo.face_tets.iter().enumerate() {
        let [t1, t2] = *pair;
        if t1 == u32::MAX || t2 == u32::MAX {
            continue;
        }
        let (t1, t2) = (t1 as usize, t2 as usize);
        let local = |t: usize| topo.tet_faces.get(t).and_then(|tf| tf.iter().position(|&g| g as usize == f));
        let (Some(k1), Some(k2l)) = (local(t1), local(t2)) else {
            return Err(invalid("mesh", format!("face {f} is not a face of its tetrahedra")));
        };
        let (el1, el2) = (driven.element_of(t1)?, driven.element_of(t2)?);
        // The same physical points from both sides: the faces' corners in ascending global order.
        let (r1, r2) = (el1.face_rule(LOCAL_FACES[k1], RULE), el2.face_rule(LOCAL_FACES[k2l], RULE));
        let corners: Vec<emcraft_model::Point3> = LOCAL_FACES[k1].iter().map(|&c| el1.corners[c]).collect();
        let hf = diameter(&corners);
        let (eps1, eps2) = (driven.eps_at(t1, r.frequency), driven.eps_at(t2, r.frequency));
        let (nu1, nu2) = (1.0 / driven.mu_of(t1), 1.0 / driven.mu_of(t2));
        let sides: Vec<_> = r1
            .iter()
            .zip(&r2)
            .map(|((l1, w, n), (l2, _, _))| (el1.values(*l1), el1.curl_values(*l1), el2.values(*l2), el2.curl_values(*l2), *w, *n))
            .collect();
        let mut jump = 0.0;
        for x in &r.fields {
            for (v1, c1, v2, c2, w, n) in &sides {
                let (e1, e2) = (combine(v1, &dofs[t1], x), combine(v2, &dofs[t2], x));
                let (h1, h2) = (combine(c1, &dofs[t1], x), combine(c2, &dofs[t2], x));
                let dh = CVec3::new(h1.x * nu1 - h2.x * nu2, h1.y * nu1 - h2.y * nu2, h1.z * nu1 - h2.z * nu2);
                let tangential = norm2(cross(*n, dh));
                let normal = (dot(e1, *n) * eps1 - dot(e2, *n) * eps2).norm_sqr();
                jump += w * (tangential + k2 * normal);
            }
        }
        let share = 0.5 * hf * jump;
        eta[t1] += share;
        eta[t2] += share;
    }
    if let Some(t) = eta.iter().position(|e| !e.is_finite()) {
        return Err(invalid("error indicator", format!("not finite in tetrahedron {t}")));
    }
    Ok(eta)
}

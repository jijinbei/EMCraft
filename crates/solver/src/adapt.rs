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
//!
//! **Open problems** ([`open_indicator`]) take the same indicator with what drives them:
//! - The perfectly matched layer is left out, its tetrahedra and every face they touch: its
//!   stretched medium is a device that ends the domain, not the physics the error is measured
//!   in, and its mesh stays as the layer's own sizing made it. Z. Chen and X. Liu, *An adaptive
//!   perfectly matched layer technique for time-harmonic scattering problems*, SIAM J. Numer.
//!   Anal. 43(2):645–671, 2005, likewise estimate the error in the physical domain and leave the
//!   layer's own (its exponentially small reflection) to its design.
//! - A plane wave's scattered field E_s solves ∇ × ∇ × E_s − k₀²εr E_s = k₀²(εr − 1)E_inc, so
//!   the element residual gains that source, k₀²(εr − 1)E_inc, and the normal jump is that of the
//!   total field's flux, [n · εr (E_s + E_inc)] (E_inc is smooth and μr = 1, so the tangential
//!   jump of ∇ × E_s is the total field's). On the conductors the scattered field is not zero
//!   but the lifted trace −n × E_inc ([`crate::radiation::OpenResult::lifted`]): the elements
//!   next to them use those values.
//! - A current element is a point source, whose field (∝ 1/r³) is not square-integrable, so the
//!   L² residual means nothing next to it: the tetrahedron holding it and those sharing one of
//!   its corners are left out, as singular data is (M. Ainsworth, J. T. Oden, *A Posteriori
//!   Error Estimation in Finite Element Analysis*, Wiley 2000, ch. 2); refinement round it
//!   follows from its neighbours' indicators. A lumped port with no band (`radius` 0) is a line
//!   current, the same: the tetrahedra its gap crosses are left out. A band's current is a
//!   surface current of bounded density and is kept.

use emcraft_fem::Order;
use emcraft_fem::nedelec::LOCAL_FACES;
use emcraft_model::{Aabb, CVec3, Mesh, Model, Point3, Source, Vec3};
use num_complex::Complex64 as C;

use crate::Driven;
use crate::driven::{FrequencyResult, Result, Slot, check_frequency, invalid, wavenumber};
use crate::radiation::OpenResult;

/// Gauss points per direction of the element and face rules.
const RULE: usize = 3;

/// The longest edge of points.
fn diameter(p: &[Point3]) -> f64 {
    let mut h: f64 = 0.0;
    for (i, a) in p.iter().enumerate() {
        for b in &p[i + 1..] {
            h = h.max(a.distance(*b));
        }
    }
    h
}

fn zero() -> CVec3 {
    CVec3::new(C::new(0.0, 0.0), C::new(0.0, 0.0), C::new(0.0, 0.0))
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

fn scaled(a: CVec3, s: C) -> CVec3 {
    CVec3::new(a.x * s, a.y * s, a.z * s)
}

/// The coefficients of tetrahedron `t`'s local functions in `x`: the unknowns' values, the
/// lifted values of eliminated degrees of freedom (`lifted`, ascending by slot), zero for the
/// rest.
fn local(driven: &Driven, t: usize, x: &[C], lifted: &[(usize, C)]) -> Vec<C> {
    driven
        .dofs
        .slots_of_tet(&driven.topo, t)
        .map(|slot| match driven.dofs.unknown_of(slot) {
            Some(u) => x.get(u).copied().unwrap_or_default(),
            None => lifted.binary_search_by_key(&slot, |(s, _)| *s).ok().and_then(|i| lifted.get(i)).map_or(C::new(0.0, 0.0), |(_, g)| *g),
        })
        .collect()
}

/// A field from basis values and coefficients.
fn combine(values: &[Vec3], c: &[C]) -> CVec3 {
    let mut f = zero();
    for (v, c) in values.iter().zip(c) {
        f += CVec3::new(*c * v.x, *c * v.y, *c * v.z);
    }
    f
}

/// What the indicator is evaluated on: the solutions at one frequency and the problem's data.
struct Data<'a> {
    frequency: f64,
    fields: &'a [Vec<C>],
    /// Tetrahedra left out (the layer, sources' neighbourhoods): no element term, and no jump on
    /// any of their faces.
    skip: Vec<bool>,
    /// The eliminated degrees of freedom's values (slot, value), ascending.
    lifted: &'a [(usize, C)],
    /// Plane waves (direction k̂, polarization ê) whose scattered field the solutions are.
    incident: Vec<(Vec3, Vec3)>,
}

impl Data<'_> {
    /// The incident field at `p`: the plane waves', ê e^{−jk₀ k̂·r}.
    fn incident_at(&self, k0: f64, p: Point3) -> CVec3 {
        let mut e = zero();
        for (k, pol) in &self.incident {
            let phase = C::from_polar(1.0, -k0 * k.dot(p.to_vec()));
            e += CVec3::new(phase * pol.x, phase * pol.y, phase * pol.z);
        }
        e
    }
}

/// The squared indicator η_K² of every tetrahedron for the solution `r` of `driven`, summed over
/// its excitations.
pub fn indicator(driven: &Driven, r: &FrequencyResult) -> Result<Vec<f64>> {
    let skip = vec![false; driven.mesh.tets.len()];
    evaluate(driven, &Data { frequency: r.frequency, fields: &r.fields, skip, lifted: &[], incident: Vec::new() })
}

/// The squared indicator η_K² of every tetrahedron for the open problem's solution `r` of
/// `driven` (made from `model`): zero in the perfectly matched layer and next to point and line
/// sources, a plane wave's incident field taken as the scattered field's data (module docs).
pub fn open_indicator(driven: &Driven, model: &Model, r: &OpenResult) -> Result<Vec<f64>> {
    let mut skip = vec![false; driven.mesh.tets.len()];
    let mut leave_out = |t: usize| {
        if let Some(s) = skip.get_mut(t) {
            *s = true;
        }
    };
    let mut layer: Vec<usize> = driven.pml_tets().into_iter().collect();
    layer.sort_unstable();
    layer.into_iter().for_each(&mut leave_out);
    let mut incident = Vec::new();
    for src in &model.sources {
        match *src {
            Source::Dipole { at, .. } => {
                let (t, _) = emcraft_mesh::locate(&driven.mesh, at).ok_or_else(|| invalid("dipole", "lies outside the mesh"))?;
                let corners = driven.mesh.tets.get(t).copied().ok_or_else(|| invalid("mesh", format!("no tetrahedron {t}")))?;
                let around = Aabb::around(driven.mesh.corners(t).into_iter().flatten()).unwrap_or(Aabb::point(at));
                for n in emcraft_mesh::tets_near(&driven.mesh, around) {
                    if driven.mesh.tets.get(n).is_some_and(|c| c.iter().any(|v| corners.contains(v))) {
                        leave_out(n);
                    }
                }
                leave_out(t);
            }
            Source::PlaneWave { direction, polarization } => incident.push((direction, polarization)),
        }
    }
    for lp in model.lumped.iter().filter(|lp| lp.radius == 0.0) {
        crossed(&driven.mesh, lp.from, lp.to).into_iter().for_each(&mut leave_out);
    }
    let fields = std::slice::from_ref(&r.field);
    evaluate(driven, &Data { frequency: r.frequency, fields, skip, lifted: &r.lifted, incident })
}

/// The tetrahedra the segment from `a` to `b` passes through: those holding one of 33 points
/// evenly along it (ends included), among the tetrahedra whose boxes meet its box.
fn crossed(mesh: &Mesh, a: Point3, b: Point3) -> Vec<usize> {
    let points: Vec<Point3> = (0..=32).map(|i| a + (b - a) * (f64::from(i) / 32.0)).collect();
    emcraft_mesh::tets_near(mesh, Aabb::new(a.min(b), a.max(b)))
        .into_iter()
        .filter(|&t| mesh.corners(t).is_some_and(|c| points.iter().any(|p| emcraft_mesh::barycentric(&c, *p).iter().all(|l| *l >= -1e-9))))
        .collect()
}

fn evaluate(driven: &Driven, data: &Data) -> Result<Vec<f64>> {
    let frequency = data.frequency;
    check_frequency(frequency)?;
    let tets = driven.mesh.tets.len();
    if let Some(x) = data.fields.iter().find(|x| x.len() != driven.dofs.unknowns) {
        return Err(invalid("solution", format!("{} coefficients for {} unknowns", x.len(), driven.dofs.unknowns)));
    }
    if data.skip.len() != tets {
        return Err(invalid("error indicator", "needs one flag per tetrahedron"));
    }
    let skipped = |t: usize| data.skip.get(t).copied().unwrap_or(true);
    let mut eta = vec![0.0; tets];
    for t in (0..tets).filter(|&t| !skipped(t)) {
        eta[t] = element_residual(driven, data, t)?;
    }
    // Interior faces: the tangential jump of μr⁻¹ ∇ × E and the normal jump of εr E.
    for (f, pair) in driven.topo.face_tets.iter().enumerate() {
        let (Some(t1), Some(t2)) = (Slot::from(pair[0]).get(), Slot::from(pair[1]).get()) else { continue };
        if skipped(t1) || skipped(t2) {
            continue;
        }
        let share = face_jump(driven, data, f, [t1, t2])?;
        eta[t1] += share;
        eta[t2] += share;
    }
    if let Some(t) = eta.iter().position(|e| !e.is_finite()) {
        return Err(invalid("error indicator", format!("not finite in tetrahedron {t}")));
    }
    Ok(eta)
}

/// Tetrahedron `t`'s local coefficients, one list per solution.
fn coefficients(driven: &Driven, data: &Data, t: usize) -> Vec<Vec<C>> {
    data.fields.iter().map(|x| local(driven, t, x, data.lifted)).collect()
}

/// Tetrahedron `t`'s element residual h² ‖k₀² εr E − ∇ × μr⁻¹ ∇ × E (+ the plane waves'
/// source)‖², summed over the solutions in order.
fn element_residual(driven: &Driven, data: &Data, t: usize) -> Result<f64> {
    let frequency = data.frequency;
    let k0 = wavenumber(frequency);
    let k2 = k0 * k0;
    let el = driven.element_of(t)?;
    let (eps, nu) = (driven.eps_at(t, frequency), 1.0 / driven.mu_of(t));
    let h = diameter(&el.corners);
    let corner_curls: Vec<Vec<Vec3>> = if el.order == Order::One {
        Vec::new()
    } else {
        (0..4).map(|i| el.curl_values(std::array::from_fn(|k| if k == i { 1.0 } else { 0.0 }))).collect()
    };
    let rule = el.rule(RULE);
    let values: Vec<Vec<Vec3>> = rule.iter().map(|(l, _)| el.values(*l)).collect();
    // A plane wave's source k₀²(εr − 1)E_inc at the rule's points (none in vacuum).
    let contrast = eps - 1.0;
    let sources: Vec<CVec3> = if data.incident.is_empty() || contrast.norm() == 0.0 {
        Vec::new()
    } else {
        rule.iter().map(|(l, _)| scaled(data.incident_at(k0, el.point(*l)), contrast * k2)).collect()
    };
    let mut eta = 0.0;
    for c in coefficients(driven, data, t) {
        // ∇ × (∇ × E): the curl's gradient from its corner values (linear in the element).
        let cc = if corner_curls.is_empty() {
            zero()
        } else {
            let cv: Vec<CVec3> = corner_curls.iter().map(|v| combine(v, &c)).collect();
            // ∂_j C_k = Σ_i C_i,k ∂_j λ_i.
            let d = |j: usize, k: usize| -> C { cv.iter().zip(&el.grads).map(|(ci, g)| [ci.x, ci.y, ci.z][k] * [g.x, g.y, g.z][j]).sum() };
            CVec3::new(d(1, 2) - d(2, 1), d(2, 0) - d(0, 2), d(0, 1) - d(1, 0))
        };
        let mut res = 0.0;
        for (q, ((_, w), v)) in rule.iter().zip(&values).enumerate() {
            let e = combine(v, &c);
            let mut rr = CVec3::new(e.x * eps * k2 - cc.x * nu, e.y * eps * k2 - cc.y * nu, e.z * eps * k2 - cc.z * nu);
            if let Some(f) = sources.get(q) {
                rr += *f;
            }
            res += w * norm2(rr);
        }
        eta += h * h * res;
    }
    Ok(eta)
}

/// The share of interior face `f` between tetrahedra `t1` and `t2` for each: half of
/// h_f ‖[μr⁻¹ ∇ × E]_t‖² + k₀² ‖[εr E]_n‖² on the face, summed over the solutions in order.
fn face_jump(driven: &Driven, data: &Data, f: usize, [t1, t2]: [usize; 2]) -> Result<f64> {
    let frequency = data.frequency;
    let k0 = wavenumber(frequency);
    let k2 = k0 * k0;
    let topo = &driven.topo;
    let local_face = |t: usize| topo.tet_faces.get(t).and_then(|tf| tf.iter().position(|&g| g as usize == f));
    let (Some(k1), Some(k2l)) = (local_face(t1), local_face(t2)) else {
        return Err(invalid("mesh", format!("face {f} is not a face of its tetrahedra")));
    };
    let (el1, el2) = (driven.element_of(t1)?, driven.element_of(t2)?);
    // The same physical points from both sides: the faces' corners in ascending global order.
    let (r1, r2) = (el1.face_rule(LOCAL_FACES[k1], RULE), el2.face_rule(LOCAL_FACES[k2l], RULE));
    let corners: Vec<Point3> = LOCAL_FACES[k1].iter().map(|&c| el1.corners[c]).collect();
    let hf = diameter(&corners);
    let (eps1, eps2) = (driven.eps_at(t1, frequency), driven.eps_at(t2, frequency));
    let (nu1, nu2) = (1.0 / driven.mu_of(t1), 1.0 / driven.mu_of(t2));
    let sides: Vec<_> = r1
        .iter()
        .zip(&r2)
        .map(|((l1, w, n), (l2, _, _))| {
            // A scattered field's flux jump gains the incident field's, (εr1 − εr2) n · E_inc.
            let inc = if data.incident.is_empty() || eps1 == eps2 {
                C::new(0.0, 0.0)
            } else {
                dot(data.incident_at(k0, el1.point(*l1)), *n) * (eps1 - eps2)
            };
            (el1.values(*l1), el1.curl_values(*l1), el2.values(*l2), el2.curl_values(*l2), *w, *n, inc)
        })
        .collect();
    let (c1s, c2s) = (coefficients(driven, data, t1), coefficients(driven, data, t2));
    let mut jump = 0.0;
    for (c1, c2) in c1s.iter().zip(&c2s) {
        for (v1, cu1, v2, cu2, w, n, inc) in &sides {
            let (e1, e2) = (combine(v1, c1), combine(v2, c2));
            let (h1, h2) = (combine(cu1, c1), combine(cu2, c2));
            let dh = CVec3::new(h1.x * nu1 - h2.x * nu2, h1.y * nu1 - h2.y * nu2, h1.z * nu1 - h2.z * nu2);
            let tangential = norm2(cross(*n, dh));
            let normal = (dot(e1, *n) * eps1 - dot(e2, *n) * eps2 + inc).norm_sqr();
            jump += w * (tangential + k2 * normal);
        }
    }
    Ok(0.5 * hf * jump)
}

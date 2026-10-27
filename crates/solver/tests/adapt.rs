//! Adaptive refinement of the inductive post through WR-90: the residual indicator
//! (`emcraft_solver::adapt`) against the post's field, and passes of Dörfler marking and
//! remeshing (`emcraft_mesh::adapt`) against the independent Nyström reference
//! (`emcraft_theory::post`).

use emcraft_fem::Order;
use emcraft_mesh::solid::{Refinement, Sizes, solid_refined};
use emcraft_model::{Axis, Combine, End, Feature, Model, Point3, Port, Shape, Solid, Vec3};
use emcraft_solver::Driven;
use emcraft_solver::adapt::indicator;
use emcraft_theory::post::InductivePost;
use num_complex::Complex64 as C;

const A: f64 = 0.02286;
const B: f64 = 0.01016;
const L: f64 = 0.03;
const R: f64 = 0.0015;
const F: f64 = 10.0e9;

fn post() -> Model {
    let mut m = Model::new();
    m.set_solid(Solid { shape: Shape::Box { size: Vec3::new(A, B, L) }, material: "vacuum".into() }).unwrap();
    m.add_feature(Feature {
        combine: Combine::Subtract,
        shape: Shape::Cylinder { radius: R, length: B },
        at: Point3::new(A / 2.0, 0.0, L / 2.0),
        axis: Axis::Y,
    })
    .unwrap();
    m.add_port(Port::new("1", End::Start).with_modes(4)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(4)).unwrap();
    m
}

fn reference() -> C {
    InductivePost { width: A, length: L, x: A / 2.0, z: L / 2.0, radius: R }.solve(F, 64).unwrap().s11
}

/// Meshes, solves and returns (S11, S21, indicator, unknowns, tets, model).
fn pass(size: f64, refine: &[Refinement], order: Order) -> (C, C, Vec<f64>, usize, Model) {
    let mut m = post();
    m.mesh = Some(solid_refined(&m, &Sizes { default: size, per_material: Vec::new() }, true, refine).unwrap());
    let d = Driven::new(&m, order).unwrap();
    let r = d.solve(F).unwrap();
    let eta = indicator(&d, &r).unwrap();
    (r.s[0][0], r.s[1][0], eta, d.dofs.unknowns, m)
}

/// The distance of a point from the post's axis less its radius.
fn from_post(p: Point3) -> f64 {
    (p.x - A / 2.0).hypot(p.z - L / 2.0) - R
}

/// The indicator's total on the empty guide (the post's box without it) at element size `h`.
fn empty_total(h: f64, order: Order) -> f64 {
    let mut m = Model::new();
    m.set_solid(Solid { shape: Shape::Box { size: Vec3::new(A, B, L) }, material: "vacuum".into() }).unwrap();
    m.add_port(Port::new("1", End::Start).with_modes(2)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(2)).unwrap();
    m.mesh = Some(solid_refined(&m, &Sizes { default: h, per_material: Vec::new() }, true, &[]).unwrap());
    let d = Driven::new(&m, order).unwrap();
    indicator(&d, &d.solve(F).unwrap()).unwrap().iter().sum()
}

/// The squared indicator's total falling from 6 to 3 mm.
fn rate(order: Order) -> f64 {
    empty_total(0.006, order) / empty_total(0.003, order)
}

// TE10 in an empty guide is smooth: the error in the energy norm goes as h^p, so the squared
// indicator's total as h^{2p}: x4 for order 1, x16 for order 2 when h halves (the unstructured
// meshes are not exact halvings, hence the margins). Measured 4.05 and 13.9.
#[test]
fn the_indicator_falls_at_the_first_order_rate() {
    let r = rate(Order::One);
    assert!(r > 2.5 && r < 7.0, "{r}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_indicator_falls_at_the_second_order_rate() {
    let r = rate(Order::Two);
    assert!(r > 9.0 && r < 30.0, "{r}");
}

/// The indicator on the post's mesh at 6 mm, its marking and the mesh it refines to.
fn marks_and_refines(order: Order) {
    let (_, _, eta, _, m) = pass(0.006, &[], order);
    let mesh = m.mesh.as_ref().unwrap();
    assert!(eta.iter().all(|e| e.is_finite() && *e >= 0.0));
    // Deterministic: the same input, the same values bit for bit.
    let (_, _, again, _, _) = pass(0.006, &[], order);
    assert!(eta.iter().zip(&again).all(|(a, b)| a.to_bits() == b.to_bits()));
    // The error's density is highest at the post, where the scattered field varies fastest
    // (measured 5.9 times the rest's within 1 mm of it, at order 2)...
    let density = |set: &mut dyn Iterator<Item = usize>| {
        let (e, v) = set.fold((0.0, 0.0), |(e, v), t| (e + eta[t], v + mesh.volume(t).unwrap().abs()));
        e / v
    };
    let near = density(&mut (0..mesh.tets.len()).filter(|&t| from_post(mesh.centroid(t).unwrap()) < 0.001));
    let far = density(&mut (0..mesh.tets.len()).filter(|&t| from_post(mesh.centroid(t).unwrap()) >= 0.001));
    // (First-order elements' error is dominated by the travelling wave's dispersion everywhere.)
    if order == Order::Two {
        assert!(near > 2.0 * far, "{near:e} {far:e}");
    }
    // ...but the mesher's curvature sizing has the post resolved from the start (elements of a
    // fraction of its radius), so the most error per tetrahedron is in the largest ones, away
    // from it, and those are marked first.
    let marked = emcraft_mesh::adapt::dorfler(&eta, 0.3).unwrap();
    let mean_volume = |set: &[usize]| set.iter().map(|&t| mesh.volume(t).unwrap().abs()).sum::<f64>() / set.len() as f64;
    let all: Vec<usize> = (0..mesh.tets.len()).collect();
    assert!(!marked.is_empty() && marked.len() < mesh.tets.len() / 10);
    assert!(mean_volume(&marked) > 5.0 * mean_volume(&all), "{:e} {:e}", mean_volume(&marked), mean_volume(&all));
    // Meshed again, the marked places get their size (within the mesher's spread; measured 1.5).
    let refine = emcraft_mesh::adapt::refinements(mesh, &marked, 0.006 / 16.0).unwrap();
    let mut fine = post();
    fine.mesh = Some(solid_refined(&fine, &Sizes { default: 0.006, per_material: Vec::new() }, true, &refine).unwrap());
    let fine = fine.mesh.as_ref().unwrap();
    let edge = |t: usize| {
        let c = fine.corners(t).unwrap();
        (0..4).flat_map(|i| (i + 1..4).map(move |j| (i, j))).map(|(i, j)| c[i].distance(c[j])).sum::<f64>() / 6.0
    };
    let worst = refine.iter().map(|r| edge(emcraft_mesh::locate(fine, r.center).unwrap().0) / r.size).fold(0.0, f64::max);
    assert!(worst < 2.0, "{worst}");
    assert!(fine.tets.len() > mesh.tets.len());
}

#[test]
fn the_indicator_marks_the_largest_errors_and_refinement_follows() {
    marks_and_refines(Order::One);
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_second_order_indicator_marks_the_largest_errors_and_refinement_follows() {
    marks_and_refines(Order::Two);
}

/// Adaptive passes against uniform refinement on the post, order 2, against the Nyström
/// reference. Measured (|S11 − reference|, unknowns): adaptive from 6 mm 6.7e-4 (48 978),
/// 2.8e-3 (53 722), 1.1e-4 (103 528), 7.3e-6 (284 286); uniform 5.9e-5 at 3 mm (79 918),
/// 2.1e-4 at 2 mm (98 616), 1.3e-4 at 1.5 mm (142 168), 1.1e-4 at 1 mm (372 700), 8.1e-5 at
/// 0.8 mm (649 940): the uniform
/// size leaves the post's neighbourhood at its curvature sizing, so its error stalls near 1e-4,
/// which the adaptive passes pass with fewer unknowns.
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn adaptive_passes_beat_uniform_refinement_on_the_post() {
    let s_ref = reference();
    let mut refine = Vec::new();
    let mut history = Vec::new();
    for _ in 0..4 {
        let (s11, _, eta, n, m) = pass(0.006, &refine, Order::Two);
        history.push(((s11 - s_ref).norm(), n, s11));
        let marked = emcraft_mesh::adapt::dorfler(&eta, 0.3).unwrap();
        refine.extend(emcraft_mesh::adapt::refinements(m.mesh.as_ref().unwrap(), &marked, 0.006 / 16.0).unwrap());
    }
    let (err, unknowns, _) = *history.last().unwrap();
    // ΔS between the last two passes says the loop has settled.
    let delta = (history[3].2 - history[2].2).norm();
    assert!(delta < 1e-3, "{history:?}");
    let (s_uniform, _, _, n_uniform, _) = pass(0.001, &[], Order::Two);
    let err_uniform = (s_uniform - s_ref).norm();
    assert!(err < 3e-5 && err < 0.5 * err_uniform && unknowns < n_uniform, "adaptive {history:?}, uniform {err_uniform:e} with {n_uniform}");
}

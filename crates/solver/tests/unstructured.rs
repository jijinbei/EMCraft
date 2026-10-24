//! The solvers on unstructured tetrahedral meshes of curved bodies against closed-form cavity
//! resonances (Bessel zeros), and on an unstructured box against the rectangular cavity.

use emcraft_fem::Order;
use emcraft_model::{Model, Shape, Solid};
use emcraft_solver::eigen::eigenmodes;
use emcraft_theory::bessel::{cylinder_cavity_resonances, sphere_cavity_resonances};

fn body(shape: Shape, size: f64) -> Model {
    let mut m = Model::new();
    m.set_solid(Solid { shape, material: "vacuum".into() }).unwrap();
    m.mesh = Some(emcraft_mesh::solid::solid(&m, size).unwrap());
    m
}

/// The largest relative difference of the FEM's modes from the exact ones, in order.
fn worst(found: &[f64], exact: &[(f64, String)]) -> f64 {
    found.iter().zip(exact).map(|(f, (e, _))| ((f - e) / e).abs()).fold(0.0, f64::max)
}

/// The cylinder's three lowest modes at element size `h`: the largest relative error.
fn cylinder_error(h: f64) -> f64 {
    // r = 10 mm, d = 30 mm: TE111 (a degenerate pair) at 10.1 GHz, then TM010 at 11.5 GHz.
    let m = body(Shape::Cylinder { radius: 0.01, length: 0.03 }, h);
    let e = eigenmodes(&m, Order::Two, 3, 11.0e9).unwrap();
    let f: Vec<f64> = e.modes.iter().map(|x| x.frequency).collect();
    worst(&f, &cylinder_cavity_resonances(0.01, 0.03, 1.0, 3))
}

#[test]
fn cylindrical_cavity_resonances() {
    // The flat facets inscribed in the curved wall shrink the cavity: an O(h²) error of the
    // geometry, 5.6e-3 at 3 mm (the elements' own error is far smaller, see the box).
    let w = cylinder_error(0.003);
    assert!(w < 1e-2, "{w:e}");
}

fn sphere_error(h: f64) -> f64 {
    // r = 10 mm: TM11, three-fold, at 13.1 GHz.
    let m = body(Shape::Sphere { radius: 0.01 }, h);
    let e = eigenmodes(&m, Order::Two, 3, 14.0e9).unwrap();
    let f: Vec<f64> = e.modes.iter().map(|x| x.frequency).collect();
    worst(&f, &sphere_cavity_resonances(0.01, 1.0, 3))
}

#[test]
fn spherical_cavity_resonances() {
    // 1.3e-2 at 3 mm, from the geometry as for the cylinder.
    let w = sphere_error(0.003);
    assert!(w < 2e-2, "{w:e}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn curved_cavities_converge_with_the_geometry() {
    // Measured 5.6e-3 → 2.5e-3 (cylinder) and 1.3e-2 → 3.3e-3 (sphere) from 3 to 2 mm.
    let (c3, c2) = (cylinder_error(0.003), cylinder_error(0.002));
    let (s3, s2) = (sphere_error(0.003), sphere_error(0.002));
    assert!(c3 / c2 > 1.8 && s3 / s2 > 1.8, "{c3:e} {c2:e} {s3:e} {s2:e}");
}

#[test]
fn unstructured_box_cavity() {
    let m = body(Shape::Box { size: [0.02286, 0.01016, 0.03] }, 0.003);
    let e = eigenmodes(&m, Order::Two, 3, 12.0e9).unwrap();
    let f: Vec<f64> = e.modes.iter().map(|x| x.frequency).collect();
    let exact = emcraft_theory::cavity_resonances(0.02286, 0.01016, 0.03, 1.0, 3);
    let w = worst(&f, &exact);
    // Flat walls, no geometric error: 2.6e-5, like the structured mesh.
    assert!(w < 2e-4, "{w:e}");
}

//! The solvers on unstructured tetrahedral meshes of curved bodies against closed-form cavity
//! resonances (Bessel zeros), and on an unstructured box against the rectangular cavity. The
//! tetrahedra on curved walls and interfaces are curved (their edges' midpoints on the exact
//! surface) unless a test asks for straight ones.

use emcraft_fem::Order;
use emcraft_model::{Model, Point3, Shape, Solid, Vec3};
use emcraft_solver::eigen::eigenmodes;
use emcraft_theory::bessel::{cylinder_cavity_resonances, sphere_cavity_resonances};

fn body(shape: Shape, size: f64) -> Model {
    body_with(shape, size, true)
}

fn body_with(shape: Shape, size: f64, curved: bool) -> Model {
    let mut m = Model::new();
    m.set_solid(Solid { shape, material: "vacuum".into() }).unwrap();
    m.mesh = Some(
        emcraft_mesh::solid::solid_with(&m, &emcraft_mesh::solid::SolidOptions { curved, ..emcraft_mesh::solid::SolidOptions::new(size) })
            .unwrap()
            .into(),
    );
    m
}

/// The largest relative difference of the FEM's modes from the exact ones, in order.
fn worst(found: &[f64], exact: &[(f64, String)]) -> f64 {
    found.iter().zip(exact).map(|(f, (e, _))| ((f - e) / e).abs()).fold(0.0, f64::max)
}

/// The cylinder's three lowest modes at element size `h`: the largest relative error.
fn cylinder_error(h: f64, curved: bool) -> f64 {
    // r = 10 mm, d = 30 mm: TE111 (a degenerate pair) at 10.1 GHz, then TM010 at 11.5 GHz.
    let m = body_with(Shape::Cylinder { radius: 0.01, length: 0.03 }, h, curved);
    let e = eigenmodes(&m, Order::Two, 3, 11.0e9).unwrap();
    let f: Vec<f64> = e.modes.iter().map(|x| x.frequency).collect();
    worst(&f, &cylinder_cavity_resonances(0.01, 0.03, 1.0, 3))
}

#[test]
fn cylindrical_cavity_resonances() {
    // Curved elements: 1.1e-4 at 4 mm (4.0e-5 at 3 mm, curved_cavities_converge and the oracle's
    // cylinder-cavity). Straight ones, flat facets inscribed in the wall, shrink the cavity: an
    // O(h²) error of the geometry, 8.2e-3 (5.6e-3 at 3 mm).
    let w = cylinder_error(0.004, true);
    assert!(w < 2e-4, "{w:e}");
    let straight = cylinder_error(0.004, false);
    assert!(straight > 3e-3 && straight < 1e-2, "{straight:e}");
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
    // 1.6e-4 at 3 mm (straight elements: 1.3e-2).
    let w = sphere_error(0.003);
    assert!(w < 5e-4, "{w:e}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn curved_cavities_converge() {
    // Measured 4.0e-5 → 1.05e-5 (cylinder) and 1.6e-4 → 2.3e-5 (sphere) from 3 to 2 mm: faster
    // than the h² of straight elements (5.6e-3 → 2.5e-3 for the cylinder).
    let (c3, c2) = (cylinder_error(0.003, true), cylinder_error(0.002, true));
    let (s3, s2) = (sphere_error(0.003), sphere_error(0.002));
    assert!(c3 / c2 > 3.0 && s3 / s2 > 3.0, "{c3:e} {c2:e} {s3:e} {s2:e}");
    let (t3, t2) = (cylinder_error(0.003, false), cylinder_error(0.002, false));
    assert!(t3 / t2 > 1.8, "{t3:e} {t2:e}");
}

#[test]
fn unstructured_box_cavity() {
    let m = body(Shape::Box { size: Vec3::new(0.02286, 0.01016, 0.03) }, 0.003);
    let e = eigenmodes(&m, Order::Two, 3, 12.0e9).unwrap();
    let f: Vec<f64> = e.modes.iter().map(|x| x.frequency).collect();
    let exact = emcraft_theory::cavity_resonances(0.02286, 0.01016, 0.03, 1.0, 3);
    let w = worst(&f, &exact);
    // Flat walls, no geometric error: 2.6e-5, like the structured mesh.
    assert!(w < 2e-4, "{w:e}");
}

#[test]
fn unstructured_waveguide_transmits_with_the_te10_phase() {
    use emcraft_model::{End, Port};
    // A 30 mm length of WR-90 as a box with ports on its ends, meshed without structure.
    let mut m = body(Shape::Box { size: Vec3::new(0.02286, 0.01016, 0.03) }, 0.003);
    m.add_port(Port::new("1", End::Start).with_modes(4)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(4)).unwrap();
    let d = emcraft_solver::Driven::new(&m, Order::Two).unwrap();
    for f in [8.0e9, 10.0e9, 12.0e9] {
        let r = d.solve(f).unwrap();
        let exact = emcraft_theory::Te10Line { a: 0.02286, layers: vec![emcraft_theory::Layer::lossless(0.03, 1.0, 1.0)] }.solve(f).unwrap();
        let e = (r.s[1][0] - exact.s21).norm().max(r.s[0][0].norm());
        // Measured 5e-5 to 2.4e-4: the unstructured mesh as accurate as the structured one.
        assert!(e < 1e-3, "{f}: {e:e}");
        assert!((r.s[0][0].norm_sqr() + r.s[1][0].norm_sqr() - 1.0).abs() < 1e-10);
    }
}

/// The TE11 wave through `len` of empty circular guide, radius r: S21 = e^{−jβL}.
fn circular_s21(r: f64, len: f64, f: f64) -> num_complex::Complex64 {
    let k = 2.0 * std::f64::consts::PI * f / emcraft_model::units::C0;
    let kc = 1.841183781340659 / r;
    num_complex::Complex64::from_polar(1.0, -(k * k - kc * kc).sqrt() * len)
}

fn circular_guide_error(h: f64) -> f64 {
    use emcraft_model::{End, Port};
    // r = 10 mm: TE11 above 8.79 GHz, TM01 above 11.47 GHz, TE21 above 14.58 GHz.
    let mut m = body(Shape::Cylinder { radius: 0.01, length: 0.03 }, h);
    m.add_port(Port::new("1", End::Start).with_modes(3)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(3)).unwrap();
    let d = emcraft_solver::Driven::new(&m, Order::Two).unwrap();
    let mut worst = 0.0f64;
    for f in [10.5e9, 13.5e9] {
        let r = d.solve(f).unwrap();
        let e = (r.s[1][0] - circular_s21(0.01, 0.03, f)).norm().max(r.s[0][0].norm());
        assert!((r.s[0][0].norm_sqr() + r.s[1][0].norm_sqr() - 1.0).abs() < 1e-3, "{f}");
        worst = worst.max(e);
    }
    worst
}

#[test]
fn circular_waveguide_transmits_with_the_te11_phase() {
    // The ports' TE11, TM01 and the cross-polarized TE11 against Bessel modes: 1.1e-3 at 3 mm,
    // |S11| under 1e-4 (straight elements, whose flat facets raise the cut-off: 4.4e-2).
    let w = circular_guide_error(0.003);
    assert!(w < 3e-3, "{w:e}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn circular_waveguide_converges() {
    // Measured 1.1e-3 → 2.3e-4 from 3 to 2 mm.
    let (e3, e2) = (circular_guide_error(0.003), circular_guide_error(0.002));
    assert!(e3 / e2 > 3.0, "{e3:e} {e2:e}");
}

/// A spherical cavity of radius 10 mm holding a concentric dielectric sphere (εr = 4, radius
/// 4 mm): the largest relative error of its three lowest modes at element size `h`.
fn loaded_sphere_error(h: f64) -> f64 {
    use emcraft_model::{Inclusion, Material};
    let mut m = Model::new();
    m.add_material(Material::new("ceramic", 4.0, 1.0).unwrap()).unwrap();
    m.set_solid(Solid { shape: Shape::Sphere { radius: 0.01 }, material: "vacuum".into() }).unwrap();
    m.add_inclusion(Inclusion { shape: Shape::Sphere { radius: 0.004 }, at: Point3::new(0.0, 0.0, 0.0), material: Some("ceramic".into()) }).unwrap();
    m.mesh = Some(emcraft_mesh::solid::solid(&m, h).unwrap().into());
    let exact = emcraft_theory::bessel::layered_sphere_resonances(0.004, 4.0, 0.01, 6);
    let near = exact[2].0 * 1.05;
    let e = eigenmodes(&m, Order::Two, 3, near).unwrap();
    let f: Vec<f64> = e.modes.iter().map(|x| x.frequency).collect();
    worst(&f, &exact)
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn dielectric_loaded_spherical_cavity() {
    // Two regions meshed apart and joined node for node, the wall and the interface curved:
    // the TM11 triplet within 1.3e-4 at 3 mm (straight elements: 1.0e-2).
    let w = loaded_sphere_error(0.003);
    assert!(w < 4e-4, "{w:e}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn dielectric_loaded_spherical_cavity_converges() {
    // 1.3e-4 → 7.7e-5 from 3 to 2 mm (the interface is tessellated at π/16 of its radius
    // whatever the size, so its part of the error stays).
    let (e3, e2) = (loaded_sphere_error(0.003), loaded_sphere_error(0.002));
    assert!(e3 / e2 > 1.4 && e2 < 1.5e-4, "{e3:e} {e2:e}");
}

/// S of a 3 mm post through WR-90, wall to wall, by a Boolean difference (an inductive
/// obstacle), at 10 GHz.
fn post_s(order: Order, size: f64) -> emcraft_solver::SMatrix {
    use emcraft_model::{Axis, Combine, End, Feature, Port};
    let (a, b, l) = (0.02286, 0.01016, 0.03);
    let mut m = Model::new();
    m.set_solid(Solid { shape: Shape::Box { size: Vec3::new(a, b, l) }, material: "vacuum".into() }).unwrap();
    m.add_feature(Feature {
        combine: Combine::Subtract,
        shape: Shape::Cylinder { radius: 0.0015, length: b },
        at: Point3::new(a / 2.0, 0.0, l / 2.0),
        axis: Axis::Y,
    })
    .unwrap();
    m.mesh = Some(emcraft_mesh::solid::solid(&m, size).unwrap().into());
    m.add_port(Port::new("1", End::Start).with_modes(4)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(4)).unwrap();
    let d = emcraft_solver::Driven::new(&m, order).unwrap();
    d.solve(10.0e9).unwrap().s
}

#[test]
fn a_post_across_the_guide_conserves_power() {
    // Lossless and reciprocal whatever the order; first-order elements keep the gate fast.
    let s = post_s(Order::One, 0.004);
    assert!((s[0][0].norm_sqr() + s[1][0].norm_sqr() - 1.0).abs() < 1e-9);
    assert!((s[0][1] - s[1][0]).norm() < 1e-9, "reciprocity");
    assert!(s[0][0].norm() > 0.8, "{}", s[0][0]);
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn a_post_across_the_guide_reflects_and_conserves_power() {
    let s = post_s(Order::Two, 0.003);
    assert!((s[0][0].norm_sqr() + s[1][0].norm_sqr() - 1.0).abs() < 1e-9);
    assert!((s[0][1] - s[1][0]).norm() < 1e-9, "reciprocity");
    // Centred: S11 = S22 but for the mesh's asymmetry. A post is a small shunt reactance
    // (Marcuvitz, Waveguide Handbook, 1951, §5.11: X/Z0 about 0.12 here), so it reflects most of
    // the wave: |S11| = 1/|1 + 2jX/Z0| ≈ 0.97, measured 0.95.
    assert!((s[0][0] - s[1][1]).norm() < 2e-2, "{} {}", s[0][0], s[1][1]);
    assert!(s[0][0].norm() > 0.9 && s[0][0].norm() < 0.99, "{}", s[0][0]);
}

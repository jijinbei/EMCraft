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

#[test]
fn unstructured_waveguide_transmits_with_the_te10_phase() {
    use emcraft_model::{End, Port};
    // A 30 mm length of WR-90 as a box with ports on its ends, meshed without structure.
    let mut m = body(Shape::Box { size: [0.02286, 0.01016, 0.03] }, 0.003);
    m.add_port(Port { name: "1".into(), end: End::Start, modes: 4 }).unwrap();
    m.add_port(Port { name: "2".into(), end: End::End, modes: 4 }).unwrap();
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
    m.add_port(Port { name: "1".into(), end: End::Start, modes: 3 }).unwrap();
    m.add_port(Port { name: "2".into(), end: End::End, modes: 3 }).unwrap();
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
    // The ports' TE11, TM01 and the cross-polarized TE11 against Bessel modes. The flat facets
    // inscribed in the wall raise the cut-off a little, so the phase lags: an O(h²) error of the
    // geometry (|S11| itself stays near 2e-3).
    let w = circular_guide_error(0.003);
    assert!(w < 6e-2, "{w:e}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn circular_waveguide_converges_with_the_geometry() {
    let (e3, e2) = (circular_guide_error(0.003), circular_guide_error(0.002));
    eprintln!("{e3:e} {e2:e}");
    assert!(e3 / e2 > 1.8, "{e3:e} {e2:e}");
}

/// A spherical cavity of radius 10 mm holding a concentric dielectric sphere (εr = 4, radius
/// 4 mm): the largest relative error of its three lowest modes at element size `h`.
fn loaded_sphere_error(h: f64) -> f64 {
    use emcraft_model::{Inclusion, Material};
    let mut m = Model::new();
    m.add_material(Material::new("ceramic", 4.0, 1.0).unwrap()).unwrap();
    m.set_solid(Solid { shape: Shape::Sphere { radius: 0.01 }, material: "vacuum".into() }).unwrap();
    m.add_inclusion(Inclusion { shape: Shape::Sphere { radius: 0.004 }, at: [0.0; 3], material: Some("ceramic".into()) }).unwrap();
    m.mesh = Some(emcraft_mesh::solid::solid(&m, h).unwrap());
    let exact = emcraft_theory::bessel::layered_sphere_resonances(0.004, 4.0, 0.01, 6);
    let near = exact[2].0 * 1.05;
    let e = eigenmodes(&m, Order::Two, 3, near).unwrap();
    let f: Vec<f64> = e.modes.iter().map(|x| x.frequency).collect();
    worst(&f, &exact)
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn dielectric_loaded_spherical_cavity() {
    // Two regions meshed apart and joined node for node: the TM11 triplet within the outer
    // wall's geometric error (1.0 % at 3 mm, as for the empty sphere).
    let w = loaded_sphere_error(0.003);
    assert!(w < 2e-2, "{w:e}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn dielectric_loaded_spherical_cavity_converges() {
    let (e3, e2) = (loaded_sphere_error(0.003), loaded_sphere_error(0.002));
    assert!(e3 / e2 > 1.8 && e2 < 5e-3, "{e3:e} {e2:e}");
}

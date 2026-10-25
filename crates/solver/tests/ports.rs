//! Ports whose modes come from the cross-section solver (`emcraft_solver::portmode`): against
//! the closed forms where those apply, and on cross-sections they do not cover.

mod common;

use common::*;
use emcraft_fem::Order;
use emcraft_model::{Axis, Combine, End, Feature, Material, Model, Point3, Port, Shape, Solid};
use emcraft_solver::{Complex64 as C, Driven};

/// The high-pass filter with its ports' modes solved numerically.
fn numeric_filter() -> Model {
    let mut m = guide(&FILTER, 0.004, 4);
    m.ports.iter_mut().for_each(|p| p.numeric = true);
    m
}

#[test]
fn numerical_port_modes_give_the_closed_forms_s_parameters() {
    // The filter's S against the exact cascade, as with the closed-form TE10, and against the
    // closed-form ports' own S: the modes differ only by the cross-section's discretisation.
    let numeric = Driven::new(&numeric_filter(), Order::Two).unwrap();
    let analytic = Driven::new(&guide(&FILTER, 0.004, 4), Order::Two).unwrap();
    for f in [5.0e9, 6.5e9, 8.0e9] {
        let (rn, ra) = (numeric.solve(f).unwrap(), analytic.solve(f).unwrap());
        let (s11, s21) = line(&FILTER).solve(f).unwrap().s_power();
        assert!((rn.s[1][0] - s21).norm() < 4e-3 && (rn.s[0][0] - s11).norm() < 4e-3, "{f}: {} {}", rn.s[1][0], s21);
        for (p, q) in [(0, 0), (1, 0), (1, 1)] {
            assert!((rn.s[p][q] - ra.s[p][q]).norm() < 1e-3, "{f}: S{}{} {} {}", p + 1, q + 1, rn.s[p][q], ra.s[p][q]);
        }
        // Lossless and reciprocal to rounding, as the closed forms are.
        assert!((rn.s[0][0].norm_sqr() + rn.s[1][0].norm_sqr() - 1.0).abs() < 1e-10);
        assert!((rn.s[0][1] - rn.s[1][0]).norm() < 1e-12);
    }
    // Below the fill's TE10 cut-off (4.64 GHz) the first mode is evanescent: refused.
    assert!(numeric.solve(4.0e9).is_err());
}

/// A coaxial line: PTFE (εr = 2.1) between conductors of radii 1 and 3.5 mm, `length` long,
/// the inner conductor a Boolean difference through the whole length.
fn coax(length: f64, size: f64) -> Model {
    let mut m = Model::new();
    m.add_material(Material::new("ptfe", 2.1, 1.0).unwrap()).unwrap();
    m.set_solid(Solid { shape: Shape::Cylinder { radius: 0.0035, length }, material: "ptfe".into() }).unwrap();
    m.add_feature(Feature { combine: Combine::Subtract, shape: Shape::Cylinder { radius: 0.001, length }, at: Point3::ORIGIN, axis: Axis::Z })
        .unwrap();
    m.mesh = Some(emcraft_mesh::solid::solid(&m, size).unwrap());
    for (name, end) in [("1", End::Start), ("2", End::End)] {
        m.add_port(Port { name: name.into(), end, modes: 2, face: None, numeric: false }).unwrap();
    }
    m
}

#[test]
fn a_coaxial_line_transmits_its_tem_wave() {
    // No closed form covers the annulus, so its ports take the numerical modes: TEM first,
    // S21 = e^{−jk√εr L}, nothing reflected. The cross-section's triangles bend with the
    // volume's onto the conductors (straight ones, a polygon in a curved guide, reflect 5e-3):
    // |S11| 6e-5, the phase within 7e-5 rad at 2 mm.
    let length = 0.01;
    let d = Driven::new(&coax(length, 0.002), Order::Two).unwrap();
    let f = 3.0e9;
    let r = d.solve(f).unwrap();
    let beta = 2.0 * std::f64::consts::PI * f / emcraft_model::units::C0 * 2.1f64.sqrt();
    let exact = C::from_polar(1.0, -beta * length);
    assert!(r.s[0][0].norm() < 2e-4, "{}", r.s[0][0]);
    assert!((r.s[1][0] - exact).norm() < 3e-4, "{} {exact}", r.s[1][0]);
    assert!((r.s[0][0].norm_sqr() + r.s[1][0].norm_sqr() - 1.0).abs() < 1e-10);
}

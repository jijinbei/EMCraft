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

/// WR-90 `length` long with a layer of εr `er` filling it from y = 0 to `d`, ports at both ends.
fn slab_guide(length: f64, d: f64, er: f64, size: f64) -> Model {
    let mut m = Model::new();
    m.add_material(Material::new("slab", er, 1.0).unwrap()).unwrap();
    let w = emcraft_model::Waveguide {
        width: A,
        height: B,
        sections: vec![emcraft_model::Section::new(length, "vacuum")],
        layers: vec![emcraft_model::Layer { thickness: d, material: "slab".into() }],
        strips: Vec::new(),
    };
    m.set_waveguide(w).unwrap();
    for (name, end) in [("1", End::Start), ("2", End::End)] {
        m.add_port(Port { name: name.into(), end, modes: 4, face: None, numeric: false }).unwrap();
    }
    m.mesh = Some(emcraft_mesh::waveguide(&m, size).unwrap());
    m
}

#[test]
fn a_slab_loaded_guide_transmits_its_hybrid_mode() {
    // The layered guide has no closed-form port modes; the numerical LSM₁₀ (TE10-like, with an
    // axial E) must pass with Collin's β and not reflect.
    let (length, d, er) = (0.02, B / 2.0, 2.25);
    let dr = Driven::new(&slab_guide(length, d, er, 0.002), Order::Two).unwrap();
    let f = 10e9;
    let k0 = 2.0 * std::f64::consts::PI * f / emcraft_model::units::C0;
    let beta = emcraft_theory::slab::slab_loaded_modes(A, B, d, er, k0, 0.0, 1)[0].sqrt();
    let r = dr.solve(f).unwrap();
    let exact = C::from_polar(1.0, -beta * length);
    // 2 mm cells: |S11| 1e-5, S21 within 1e-4.
    assert!(r.s[0][0].norm() < 1e-4, "{}", r.s[0][0]);
    assert!((r.s[1][0] - exact).norm() < 5e-4, "{} {exact}", r.s[1][0]);
}

/// A stripline: a box a × b filled with εr, a strip `w` wide across its middle (y = b/2), the
/// whole `length` long.
fn stripline(a: f64, b: f64, w: f64, length: f64, er: f64, size: f64) -> Model {
    let mut m = Model::new();
    m.add_material(Material::new("fill", er, 1.0).unwrap()).unwrap();
    let wg = emcraft_model::Waveguide {
        width: a,
        height: b,
        sections: vec![emcraft_model::Section::new(length, "fill")],
        layers: Vec::new(),
        strips: vec![emcraft_model::Strip { y: b / 2.0, x0: (a - w) / 2.0, width: w, z0: 0.0, z1: length }],
    };
    m.set_waveguide(wg).unwrap();
    for (name, end) in [("1", End::Start), ("2", End::End)] {
        m.add_port(Port { name: name.into(), end, modes: 2, face: None, numeric: false }).unwrap();
    }
    m.mesh = Some(emcraft_mesh::waveguide(&m, size).unwrap());
    m
}

#[test]
fn a_stripline_carries_tem_at_the_speed_of_light_in_its_fill() {
    // A strip across the middle of a filled box: TEM, β = k√εr whatever the cross-section. The
    // strip's edges are singular (E ∝ r^{−1/2}): graded to a twentieth of the size there, |S11|
    // 1.5e-4 and the phase within 5e-5 rad at 1.5 mm (graded to a quarter, as irises are: 9e-4
    // and 7e-4, falling only as h).
    let (a, b, w, length, er) = (0.006, 0.003, 0.0015, 0.006, 2.2);
    let dr = Driven::new(&stripline(a, b, w, length, er, 0.0015), Order::Two).unwrap();
    let f = 5e9;
    let beta = 2.0 * std::f64::consts::PI * f / emcraft_model::units::C0 * er.sqrt();
    let r = dr.solve(f).unwrap();
    let exact = C::from_polar(1.0, -beta * length);
    assert!(r.s[0][0].norm() < 5e-4, "{}", r.s[0][0]);
    assert!((r.s[1][0] - exact).norm() < 5e-4, "{} {exact}", r.s[1][0]);
}

/// A microstrip in a shielding box `a` × `b`: a substrate `h` thick of εr, a strip `w` wide
/// centred on it, `length` long.
fn microstrip(a: f64, b: f64, h: f64, w: f64, er: f64, length: f64, size: f64) -> Model {
    let mut m = Model::new();
    m.add_material(Material::new("substrate", er, 1.0).unwrap()).unwrap();
    let wg = emcraft_model::Waveguide {
        width: a,
        height: b,
        sections: vec![emcraft_model::Section::new(length, "vacuum")],
        layers: vec![emcraft_model::Layer { thickness: h, material: "substrate".into() }],
        strips: vec![emcraft_model::Strip { y: h, x0: (a - w) / 2.0, width: w, z0: 0.0, z1: length }],
    };
    m.set_waveguide(wg).unwrap();
    for (name, end) in [("1", End::Start), ("2", End::End)] {
        m.add_port(Port { name: name.into(), end, modes: 2, face: None, numeric: false }).unwrap();
    }
    m.mesh = Some(emcraft_mesh::waveguide(&m, size).unwrap());
    m
}

#[test]
fn a_microstrip_has_hammerstad_and_jensens_effective_permittivity() {
    // Alumina (εr = 9.8, h = 0.635 mm) at 1 GHz (quasi-static) in a box far from the strip (the
    // cover 38 substrates up, the side walls 12 strip widths away each side): the quasi-TEM
    // mode's (β/k₀)² against the fit (itself within 0.2 %), for a narrow, a 50 Ω and a wide
    // strip. Measured −0.24 %, −0.20 %, +0.30 % (with the cover at 9 h, −0.5 to −0.9 %: a cover
    // adds capacitance in the air and lowers ε_eff).
    let (h, er): (f64, f64) = (0.000635, 9.8);
    let k0 = 2.0 * std::f64::consts::PI * 1e9 / emcraft_model::units::C0;
    for u in [0.4f64, 0.945, 3.0] {
        let w = u * h;
        let dr = Driven::new(&microstrip((24.0 * w).max(0.012), 0.024, h, w, er, 0.0005, 0.0006), Order::Two).unwrap();
        let emcraft_solver::driven::PortModes::Numeric(n) = &dr.ports[0].modes else { panic!("closed-form port on a microstrip") };
        let eps = n.section.modes(k0, 1).unwrap()[0].beta2 / (k0 * k0);
        let fit = emcraft_theory::microstrip::eps_eff(u, er);
        assert!((eps / fit - 1.0).abs() < 5e-3, "{u}: {eps} {fit}");
    }
}

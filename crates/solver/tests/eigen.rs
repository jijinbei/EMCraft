//! Cavity eigenmodes against the closed form: resonant frequencies of a rectangular cavity and
//! the TE101 mode's Q from wall losses.

use emcraft_fem::Order;
use emcraft_model::{Material, Model, Section, Waveguide};
use emcraft_solver::eigen::eigenmodes;
use emcraft_theory::{cavity_resonances, te101_q_walls};

const A: f64 = 0.02286;
const B: f64 = 0.01016;
const D: f64 = 0.03;

fn cavity(size: f64) -> Model {
    let mut m = Model::new();
    m.set_waveguide(Waveguide { width: A, height: B, sections: vec![Section::new(D, "vacuum")] }).unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, size).unwrap());
    m
}

#[test]
fn rectangular_cavity_resonances() {
    // The four modes nearest 12 GHz are the four lowest (TE101, TE102, TE201, TE011); measured
    // within 1e-4 of the closed form.
    let e = eigenmodes(&cavity(0.003), Order::Two, 4, 12.0e9).unwrap();
    let exact = cavity_resonances(A, B, D, 1.0, 4);
    for (m, (f, name)) in e.modes.iter().zip(&exact) {
        assert!(((m.frequency - f) / f).abs() < 1e-3, "{name}: {} {f}", m.frequency);
    }
    assert_eq!(e.modes.len(), 4);
    assert!(e.modes.iter().all(|m| m.q().is_none()));
}

#[test]
fn copper_te101_q() {
    let mut m = cavity(0.003);
    m.set_wall_conductivity(Some(5.8e7)).unwrap();
    let e = eigenmodes(&m, Order::Two, 1, 8.0e9).unwrap();
    let q = e.modes[0].q_walls.unwrap();
    let exact = te101_q_walls(A, B, D, 5.8e7);
    // −1.2 % at 3 mm: H_t on the walls comes from the curl of E, one order lower.
    assert!(((q - exact) / exact).abs() < 0.02, "{q} {exact}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn copper_te101_q_converges() {
    // Measured −1.2 %, −0.55 %, −0.31 % at 3, 2 and 1.5 mm.
    let errors: Vec<f64> = [0.003, 0.002, 0.0015]
        .iter()
        .map(|&h| {
            let mut m = cavity(h);
            m.set_wall_conductivity(Some(5.8e7)).unwrap();
            let q = eigenmodes(&m, Order::Two, 1, 8.0e9).unwrap().modes[0].q_walls.unwrap();
            ((q - te101_q_walls(A, B, D, 5.8e7)) / te101_q_walls(A, B, D, 5.8e7)).abs()
        })
        .collect();
    assert!(errors[0] > errors[1] && errors[1] > errors[2] && errors[2] < 5e-3, "{errors:?}");
}

#[test]
fn a_filled_cavity_scales_and_its_dielectric_sets_q() {
    // Filled with εr = 2.25, tan δ = 1e-3: frequencies fall by 1.5, Q_d = 1/tan δ.
    let mut m = Model::new();
    m.add_material(Material::new("fill", 2.25, 1.0).unwrap().with_loss_tangent(1e-3).unwrap()).unwrap();
    m.set_waveguide(Waveguide { width: A, height: B, sections: vec![Section::new(D, "fill")] }).unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, 0.003).unwrap());
    let e = eigenmodes(&m, Order::Two, 1, 5.5e9).unwrap();
    let (f, _) = cavity_resonances(A, B, D, 2.25, 1)[0].clone();
    assert!(((e.modes[0].frequency - f) / f).abs() < 1e-3);
    let qd = e.modes[0].q_dielectric.unwrap();
    assert!((qd - 1000.0).abs() < 1e-6 * 1000.0, "{qd}");
}

#[test]
fn hostile_requests_are_refused() {
    let m = cavity(0.005);
    assert!(eigenmodes(&m, Order::One, 0, 1e10).is_err());
    assert!(eigenmodes(&m, Order::One, 1000, 1e10).is_err());
    assert!(eigenmodes(&m, Order::One, 2, f64::NAN).is_err());
    assert!(eigenmodes(&Model::new(), Order::One, 2, 1e10).is_err());
}

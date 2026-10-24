//! Losses against exact solutions: a lossy dielectric slab (the TE10 cascade with complex
//! propagation constants) and the balance of power between the ports and the absorbed power.

use emcraft_fem::Order;
use emcraft_model::{End, Material, Model, Port, Section, Waveguide};
use emcraft_solver::{Driven, SolveError};
use emcraft_theory::{Layer, Te10Line};

const A: f64 = 0.02286;
const B: f64 = 0.01016;

/// Vacuum feeds around a 20 mm slab of εr = 2.2, tan δ = 0.02.
fn slab(size: f64) -> Model {
    let mut m = Model::new();
    m.add_material(Material::new("lossy", 2.2, 1.0).unwrap().with_loss_tangent(0.02).unwrap()).unwrap();
    let sections = vec![Section::new(0.01, "vacuum"), Section::new(0.02, "lossy"), Section::new(0.01, "vacuum")];
    m.set_waveguide(Waveguide { width: A, height: B, sections }).unwrap();
    m.add_port(Port { name: "1".into(), end: End::Start, modes: 4 }).unwrap();
    m.add_port(Port { name: "2".into(), end: End::End, modes: 4 }).unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, size).unwrap());
    m
}

fn exact(f: f64) -> (emcraft_solver::Complex64, emcraft_solver::Complex64) {
    let line = Te10Line {
        a: A,
        layers: vec![
            Layer::lossless(0.01, 1.0, 1.0),
            Layer { length: 0.02, eps_r: 2.2, mu_r: 1.0, loss_tangent: 0.02 },
            Layer::lossless(0.01, 1.0, 1.0),
        ],
    };
    line.solve(f).unwrap().s_power()
}

#[test]
fn lossy_slab_matches_the_exact_cascade() {
    let d = Driven::new(&slab(0.003), Order::Two).unwrap();
    for f in [8.0e9, 10.0e9, 12.0e9] {
        let r = d.solve(f).unwrap();
        let (s11, s21) = exact(f);
        let e = (r.s[0][0] - s11).norm().max((r.s[1][0] - s21).norm());
        assert!(e < 2e-3, "{f}: {e:e}");
        // It absorbs: a few percent of the power.
        let lost = 1.0 - r.s[0][0].norm_sqr() - r.s[1][0].norm_sqr();
        assert!(lost > 0.02 && lost < 0.2, "{f}: {lost}");
    }
}

#[test]
fn absorbed_power_balances_the_ports() {
    // 1 − |S11|² − |S21|² = P_absorbed / P_incident: the discrete Poynting theorem.
    let d = Driven::new(&slab(0.004), Order::Two).unwrap();
    for f in [9.0e9, 11.0e9] {
        let r = d.solve(f).unwrap();
        let lost = 1.0 - r.s[0][0].norm_sqr() - r.s[1][0].norm_sqr();
        let absorbed = d.dielectric_loss(&r.fields[0], f).unwrap() / d.incident_power(0, f).unwrap();
        assert!((lost - absorbed).abs() < 1e-9 * lost.max(1e-3) + 1e-12, "{f}: {lost} {absorbed}");
    }
}

#[test]
fn a_port_in_a_lossy_material_is_refused() {
    let mut m = Model::new();
    m.add_material(Material::new("lossy", 2.2, 1.0).unwrap().with_loss_tangent(0.02).unwrap()).unwrap();
    m.set_waveguide(Waveguide { width: A, height: B, sections: vec![Section::new(0.02, "lossy")] }).unwrap();
    m.add_port(Port { name: "1".into(), end: End::Start, modes: 1 }).unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, 0.005).unwrap());
    assert!(matches!(Driven::new(&m, Order::One), Err(SolveError::Invalid { .. })));
}

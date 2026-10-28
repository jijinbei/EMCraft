//! Ports in lossy media and magnetic losses against exact solutions: uniform guides filled with
//! a lossy dielectric or a magnetically lossy material with their ports in it (|S21| = e^{−αL},
//! the phase −βL, S11 ≈ 0, with γ = √((π/a)² − k²) evaluated here), a step from air into a lossy
//! filling (the TE10 cascade), the discrete Poynting balance with magnetic losses, the fast
//! sweep, numerically solved ports in a lossy filling, and the Q of a magnetically lossy cavity.

use std::f64::consts::PI;

use emcraft_fem::Order;
use emcraft_model::units::C0;
use emcraft_model::{End, Material, ModeSolver, Model, Port, Section, Waveguide};
use emcraft_solver::{Complex64 as C, Driven};
use emcraft_theory::{Layer, Te10Line};

const A: f64 = 0.02286;
const B: f64 = 0.01016;

fn guide(materials: Vec<Material>, sections: Vec<Section>, size: f64, solver: ModeSolver) -> Model {
    let mut m = Model::new();
    for mat in materials {
        m.add_material(mat).unwrap();
    }
    m.set_waveguide(Waveguide { width: A, height: B, sections, layers: Vec::new(), strips: Vec::new(), vias: Vec::new() }).unwrap();
    m.add_port(Port::new("1", End::Start).with_modes(3).with_mode_solver(solver)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(3).with_mode_solver(solver)).unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, size).unwrap());
    m
}

/// TE10's γ = √((π/a)² − k0² εr(1 − j tan δ) μr(1 − j tan δm)), Re γ ≥ 0, written out here
/// apart from the solver's.
fn gamma(f: f64, eps_r: f64, tan_d: f64, mu_r: f64, tan_m: f64) -> C {
    let k0 = 2.0 * PI * f / C0;
    let k2 = C::new(eps_r, -eps_r * tan_d) * C::new(mu_r, -mu_r * tan_m) * (k0 * k0);
    let g = (C::new((PI / A).powi(2), 0.0) - k2).sqrt();
    if g.re < 0.0 { -g } else { g }
}

/// A uniform guide `l` long filled with `fill`, its ports in it: S11 ≈ 0, S21 = e^{−γL} (within
/// `tol`: the phase error of second-order elements of 3 mm, about 2e-3 at 11 GHz).
fn check_uniform(fill: Material, l: f64, freqs: &[f64], tol: f64) {
    let (er, td, mr, tm) = (fill.eps_r, fill.loss_tangent, fill.mu_r, fill.magnetic_loss_tangent);
    for solver in [ModeSolver::Auto, ModeSolver::Numeric] {
        let m = guide(vec![fill.clone()], vec![Section::new(l, &fill.name)], 0.003, solver);
        let d = Driven::new(&m, Order::Two).unwrap();
        for &f in freqs {
            let r = d.solve(f).unwrap();
            let g = gamma(f, er, td, mr, tm);
            let exact = (-g * l).exp();
            let (s11, s21) = (r.s[0][0], r.s[1][0]);
            assert!(s11.norm() < tol && (s21 - exact).norm() < tol, "{solver:?} {f}: S11 {s11}, S21 {s21} against {exact}");
            // Reciprocal, and α read back from |S21|.
            assert!((r.s[0][1] - s21).norm() < 1e-9, "{solver:?} {f}: S12 {} S21 {s21}", r.s[0][1]);
            let alpha = -s21.norm().ln() / l;
            assert!((alpha - g.re).abs() < 0.02 * g.re, "{f}: α {alpha} against {}", g.re);
            // The discrete Poynting theorem: 1 − |S11|² − |S21|² is what the filling absorbs,
            // over the power entering through the port plane.
            let lost = 1.0 - s11.norm_sqr() - s21.norm_sqr();
            let absorbed =
                (d.dielectric_loss(&r.fields[0], f).unwrap() + d.magnetic_loss(&r.fields[0], f).unwrap()) / d.incident_power(0, f).unwrap();
            assert!(lost > 0.0, "{f}: {lost}");
            // (The port plane's power is the incident wave's less the reflected one's, plus
            // their cross term in a lossy line: equal to O(|S11|).)
            assert!((lost - absorbed).abs() < 10.0 * s11.norm() + 1e-6, "{f}: {lost} {absorbed}");
        }
    }
}

#[test]
fn a_guide_filled_with_a_lossy_dielectric_decays_from_port_to_port() {
    let fill = Material::new("lossy", 2.2, 1.0).unwrap().with_loss_tangent(0.02).unwrap();
    check_uniform(fill, 0.03, &[8.0e9, 11.0e9], 4e-3);
}

#[test]
fn a_guide_filled_with_a_magnetically_lossy_material_decays_from_port_to_port() {
    let fill = Material::new("ferrite", 1.5, 1.6).unwrap().with_magnetic_loss_tangent(0.03).unwrap();
    check_uniform(fill, 0.03, &[8.0e9, 11.0e9], 4e-3);
}

#[test]
fn a_step_from_air_into_a_lossy_filling_matches_the_cascade() {
    // Port 1 in air, port 2 in the lossy filling (both kinds of loss): the TE10 cascade with
    // its waves normalised as the solver's (the unconjugated root of the wave admittances).
    let fill = Material::new("lossy", 2.0, 1.3).unwrap().with_loss_tangent(0.01).unwrap().with_magnetic_loss_tangent(0.02).unwrap();
    let m = guide(vec![fill], vec![Section::new(0.012, "vacuum"), Section::new(0.02, "lossy")], 0.003, ModeSolver::Auto);
    let d = Driven::new(&m, Order::Two).unwrap();
    for f in [8.0e9, 10.0e9, 12.0e9] {
        let line = Te10Line {
            a: A,
            layers: vec![Layer::lossless(0.012, 1.0, 1.0), Layer { magnetic_loss_tangent: 0.02, ..Layer::lossy(0.02, 2.0, 1.3, 0.01) }],
        };
        let (s11, s21) = line.solve(f).unwrap().s_power();
        let r = d.solve(f).unwrap();
        let e = (r.s[0][0] - s11).norm().max((r.s[1][0] - s21).norm());
        assert!(e < 3e-3, "{f}: {e:e} (S11 {} vs {s11}, S21 {} vs {s21})", r.s[0][0], r.s[1][0]);
        assert!((r.s[0][1] - r.s[1][0]).norm() < 1e-9, "not reciprocal");
    }
}

#[test]
fn magnetic_losses_balance_the_ports() {
    // A slab with magnetic loss between air ports: 1 − |S11|² − |S21|² = ½ωμ₀∫μ″|H|² / P_inc.
    let slab = Material::new("ferrite", 1.0, 2.0).unwrap().with_magnetic_loss_tangent(0.05).unwrap();
    let m =
        guide(vec![slab], vec![Section::new(0.01, "vacuum"), Section::new(0.015, "ferrite"), Section::new(0.01, "vacuum")], 0.004, ModeSolver::Auto);
    let d = Driven::new(&m, Order::Two).unwrap();
    for f in [9.0e9, 11.0e9] {
        let r = d.solve(f).unwrap();
        let lost = 1.0 - r.s[0][0].norm_sqr() - r.s[1][0].norm_sqr();
        let absorbed = d.magnetic_loss(&r.fields[0], f).unwrap() / d.incident_power(0, f).unwrap();
        assert!(lost > 0.01 && (lost - absorbed).abs() < 1e-9 * lost, "{f}: {lost} {absorbed}");
        assert_eq!(d.dielectric_loss(&r.fields[0], f).unwrap(), 0.0);
        // Against the exact cascade.
        let line = Te10Line {
            a: A,
            layers: vec![
                Layer::lossless(0.01, 1.0, 1.0),
                Layer { magnetic_loss_tangent: 0.05, ..Layer::lossless(0.015, 1.0, 2.0) },
                Layer::lossless(0.01, 1.0, 1.0),
            ],
        };
        let (s11, s21) = line.solve(f).unwrap().s_power();
        let e = (r.s[0][0] - s11).norm().max((r.s[1][0] - s21).norm());
        assert!(e < 3e-3, "{f}: {e:e}");
    }
}

#[test]
fn a_fast_sweep_handles_lossy_ports_and_magnetic_losses() {
    let fill = Material::new("lossy", 2.0, 1.2).unwrap().with_loss_tangent(0.01).unwrap().with_magnetic_loss_tangent(0.02).unwrap();
    let m = guide(vec![fill], vec![Section::new(0.01, "vacuum"), Section::new(0.02, "lossy")], 0.005, ModeSolver::Auto);
    let d = Driven::new(&m, Order::One).unwrap();
    let freqs: Vec<f64> = (0..21).map(|i| 8e9 + 2e8 * i as f64).collect();
    let fast = emcraft_solver::sweep::fast_sweep(&d, &freqs, &Default::default()).unwrap();
    assert!(fast.full.iter().filter(|x| **x).count() < freqs.len());
    for (f, r) in freqs.iter().zip(&fast.results) {
        let full = d.solve(*f).unwrap();
        let e = (r.s[0][0] - full.s[0][0]).norm().max((r.s[1][0] - full.s[1][0]).norm());
        assert!(e < 1e-6, "{f}: {e:e}");
    }
}

#[test]
fn de_embedding_a_lossy_line_uses_its_attenuation() {
    // Moving both reference planes in by d along the lossy line: S21′ = S21 e^{2γd} = e^{−γ(L − 2d)}.
    let fill = Material::new("lossy", 2.2, 1.0).unwrap().with_loss_tangent(0.05).unwrap();
    let (l, dz) = (0.03, 0.005);
    let mut m = guide(vec![fill], vec![Section::new(l, "lossy")], 0.003, ModeSolver::Auto);
    for p in &mut m.ports {
        p.deembed = dz;
    }
    let d = Driven::new(&m, Order::Two).unwrap();
    let f = 10.0e9;
    let r = d.solve(f).unwrap();
    let g = gamma(f, 2.2, 0.05, 1.0, 0.0);
    let exact = (-g * (l - 2.0 * dz)).exp();
    assert!((r.s[1][0] - exact).norm() < 2e-3, "{} {exact}", r.s[1][0]);
}

#[test]
fn port_modes_report_the_complex_propagation_constant() {
    let fill = Material::new("lossy", 2.2, 1.4).unwrap().with_magnetic_loss_tangent(0.01).unwrap().with_loss_tangent(0.02).unwrap();
    for solver in [ModeSolver::Auto, ModeSolver::Numeric] {
        let m = guide(vec![fill.clone()], vec![Section::new(0.02, "lossy")], 0.003, solver);
        let f = 10.0e9;
        let modes = emcraft_solver::driven::port_modes(&m, Order::Two, f).unwrap();
        let g = modes[0].1[0].gamma.unwrap();
        let exact = gamma(f, 2.2, 0.02, 1.4, 0.01);
        assert!((g - exact).norm() < 2e-3 * exact.norm(), "{solver:?}: {g} {exact}");
        assert!((modes[0].1[0].beta2 + (exact * exact).re).abs() < 5e-3 * (exact * exact).norm());
    }
}

/// A WR-90 cavity 30 mm long filled with `fill`.
fn cavity(fill: Material) -> Model {
    let mut m = Model::new();
    let name = fill.name.clone();
    m.add_material(fill).unwrap();
    m.set_waveguide(Waveguide {
        width: A,
        height: B,
        sections: vec![Section::new(0.03, &name)],
        layers: Vec::new(),
        strips: Vec::new(),
        vias: Vec::new(),
    })
    .unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, 0.003).unwrap());
    m
}

#[test]
fn a_magnetically_lossy_cavity_has_q_of_its_loss_tangent() {
    // μ = μr(1 − j tan δm) throughout: K/(1 − j tan δm) x = k² M x, so k² is the lossless one
    // divided by 1 − j tan δm, as for a dielectric loss: Q_m = 1/(2 tan(½ atan tan δm)) ≈ 1/tan δm.
    let tan_m: f64 = 1e-3;
    let e = emcraft_solver::eigen::eigenmodes(
        &cavity(Material::new("fill", 1.0, 2.25).unwrap().with_magnetic_loss_tangent(tan_m).unwrap()),
        Order::Two,
        1,
        5.5e9,
    )
    .unwrap();
    let q_exact = 1.0 / (2.0 * (0.5 * tan_m.atan()).tan());
    let mode = &e.modes[0];
    assert!(mode.q_dielectric.is_none());
    let qm = mode.q_magnetic.unwrap();
    assert!((qm / q_exact - 1.0).abs() < 1e-7, "{qm} {q_exact}");
    assert!((mode.q().unwrap() - qm).abs() < 1e-9 * qm);
    // Both losses: 1/Q = 1/Q_d + 1/Q_m, shared by the powers each absorbs (equal tangents,
    // equal electric and magnetic energies at resonance: equal shares).
    let both = Material::new("fill", 1.0, 2.25).unwrap().with_magnetic_loss_tangent(tan_m).unwrap().with_loss_tangent(tan_m).unwrap();
    let e = emcraft_solver::eigen::eigenmodes(&cavity(both), Order::Two, 1, 5.5e9).unwrap();
    let mode = &e.modes[0];
    let (qd, qm) = (mode.q_dielectric.unwrap(), mode.q_magnetic.unwrap());
    assert!((qd / qm - 1.0).abs() < 1e-3, "{qd} {qm}");
    assert!((mode.q().unwrap() - 500.0).abs() < 0.5, "{}", mode.q().unwrap());
}

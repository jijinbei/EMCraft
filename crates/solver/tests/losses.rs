//! Losses against exact solutions: a lossy dielectric slab (the TE10 cascade with complex
//! propagation constants) and the balance of power between the ports and the absorbed power.

use emcraft_fem::Order;
use emcraft_model::{End, Material, Model, Pole, Port, Section, Waveguide};
use emcraft_solver::Driven;
use emcraft_theory::{Layer, Te10Line};

/// A length (m), checked.
fn metres(v: f64) -> emcraft_model::Metres {
    emcraft_model::Metres::new(v).unwrap()
}

const A: f64 = 0.02286;
const B: f64 = 0.01016;

/// Vacuum feeds around a 20 mm slab of εr = 2.2, tan δ = 0.02.
fn slab(size: f64) -> Model {
    let mut m = Model::new();
    m.add_material(Material::new("lossy", 2.2, 1.0).unwrap().with_loss_tangent(0.02).unwrap()).unwrap();
    let sections = vec![Section::new(metres(0.01), "vacuum"), Section::new(metres(0.02), "lossy"), Section::new(metres(0.01), "vacuum")];
    m.set_waveguide(Waveguide { width: metres(A), height: metres(B), sections, layers: Vec::new(), strips: Vec::new(), vias: Vec::new() }).unwrap();
    m.add_port(Port::new("1", End::Start).with_modes(4)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(4)).unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, metres(size)).unwrap().into());
    m
}

fn exact(f: f64) -> (emcraft_solver::Complex64, emcraft_solver::Complex64) {
    let line = Te10Line { a: A, layers: vec![Layer::lossless(0.01, 1.0, 1.0), Layer::lossy(0.02, 2.2, 1.0, 0.02), Layer::lossless(0.01, 1.0, 1.0)] };
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

/// An empty WR-90 guide `length` long with walls of conductivity `sigma`.
fn lossy_walls(length: f64, sigma: f64, size: f64) -> Model {
    let mut m = Model::new();
    m.set_waveguide(Waveguide {
        width: metres(A),
        height: metres(B),
        sections: vec![Section::new(metres(length), "vacuum")],
        layers: Vec::new(),
        strips: Vec::new(),
        vias: Vec::new(),
    })
    .unwrap();
    m.set_wall_conductivity(Some(emcraft_model::Conductivity::new(sigma).unwrap()));
    m.add_port(Port::new("1", End::Start).with_modes(4)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(4)).unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, metres(size)).unwrap().into());
    m
}

#[test]
fn walls_attenuate_as_the_perturbation_formula() {
    // σ = 1e6 S/m (skin depth 5 µm at 10 GHz, far below the guide): |S21| = e^{−α L}.
    let (len, sigma) = (0.1, 1.0e6);
    let d = Driven::new(&lossy_walls(len, sigma, 0.004), Order::Two).unwrap();
    for f in [8.0e9, 10.0e9, 12.0e9] {
        let r = d.solve(f).unwrap();
        let alpha = -r.s[1][0].norm().ln() / len;
        let exact = emcraft_theory::te10_wall_attenuation(A, B, f, sigma, 1.0);
        // Measured within 0.2 %: the formula is first order in R_s/η ≈ 5e-4.
        assert!((alpha - exact).abs() < 0.005 * exact, "{f}: {alpha} {exact}");
        // The ports barely reflect: the lossy guide's TE10 differs from the lossless one by R_s/η.
        assert!(r.s[0][0].norm() < 1e-3);
    }
}

#[test]
fn wall_loss_balances_the_ports() {
    let d = Driven::new(&lossy_walls(0.03, 1.0e6, 0.004), Order::Two).unwrap();
    for f in [9.0e9, 11.0e9] {
        let r = d.solve(f).unwrap();
        let lost = 1.0 - r.s[0][0].norm_sqr() - r.s[1][0].norm_sqr();
        let absorbed = d.wall_loss(&r.fields[0], f).unwrap() / d.incident_power(0, f).unwrap();
        assert!((lost - absorbed).abs() < 1e-9 * lost, "{f}: {lost} {absorbed}");
    }
}

/// The dispersive slab's poles: ε∞ = 2, a Debye pole Δε = 0.6 relaxing at 20 GHz, a Lorentz pole
/// Δε = 0.4 at 16 GHz damped by 1.5 GHz.
const TAU: f64 = 1.0 / (2.0 * std::f64::consts::PI * 20e9);
fn poles() -> Vec<Pole> {
    vec![Pole::debye(0.6, TAU).unwrap(), Pole::lorentz(0.4, 16e9, 1.5e9).unwrap()]
}

/// The poles' permittivity written out here, apart from the model's (e^{jωt}: ε = ε′ − jε″).
fn dispersive_eps(f: f64) -> emcraft_solver::Complex64 {
    use emcraft_solver::Complex64 as C;
    let w = 2.0 * std::f64::consts::PI * f;
    let (w0, g) = (2.0 * std::f64::consts::PI * 16e9, 2.0 * std::f64::consts::PI * 1.5e9);
    C::new(2.0, 0.0) + 0.6 / C::new(1.0, w * TAU) + 0.4 * w0 * w0 / C::new(w0 * w0 - w * w, w * g)
}

fn dispersive_slab(size: f64) -> Model {
    let mut m = Model::new();
    m.add_material(Material::new("disp", 2.0, 1.0).unwrap().with_dispersion(poles()).unwrap()).unwrap();
    let sections = vec![Section::new(metres(0.01), "vacuum"), Section::new(metres(0.02), "disp"), Section::new(metres(0.01), "vacuum")];
    m.set_waveguide(Waveguide { width: metres(A), height: metres(B), sections, layers: Vec::new(), strips: Vec::new(), vias: Vec::new() }).unwrap();
    m.add_port(Port::new("1", End::Start).with_modes(4)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(4)).unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, metres(size)).unwrap().into());
    m
}

#[test]
fn dispersive_slab_matches_the_exact_cascade() {
    let d = Driven::new(&dispersive_slab(0.003), Order::Two).unwrap();
    assert!(d.is_dispersive());
    for f in [8.0e9, 10.0e9, 12.0e9] {
        // The cascade with the complex ε at this frequency, as εr (1 − j tan δ).
        let e = dispersive_eps(f);
        let line = Te10Line {
            a: A,
            layers: vec![Layer::lossless(0.01, 1.0, 1.0), Layer::lossy(0.02, e.re, 1.0, -e.im / e.re), Layer::lossless(0.01, 1.0, 1.0)],
        };
        let (s11, s21) = line.solve(f).unwrap().s_power();
        let r = d.solve(f).unwrap();
        let err = (r.s[0][0] - s11).norm().max((r.s[1][0] - s21).norm());
        assert!(err < 3e-3, "{f}: {err:e}");
        // The discrete Poynting theorem with ε″(ω).
        let lost = 1.0 - r.s[0][0].norm_sqr() - r.s[1][0].norm_sqr();
        let absorbed = d.dielectric_loss(&r.fields[0], f).unwrap() / d.incident_power(0, f).unwrap();
        assert!(lost > 0.01 && (lost - absorbed).abs() < 1e-9 * lost, "{f}: {lost} {absorbed}");
    }
}

#[test]
fn a_fast_sweep_handles_dispersion() {
    // The reduced basis projects each dispersive mass matrix on its own: its results match the
    // full solves to the sweep's tolerance.
    let d = Driven::new(&dispersive_slab(0.005), Order::One).unwrap();
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
fn dispersive_ports_are_taken_and_eigenmodes_refused() {
    let mut m = Model::new();
    let drude = Pole::drude(5e9, 1e9).unwrap();
    m.add_material(Material::new("plasma", 1.0, 1.0).unwrap().with_dispersion(vec![drude]).unwrap()).unwrap();
    m.set_waveguide(Waveguide {
        width: metres(A),
        height: metres(B),
        sections: vec![Section::new(metres(0.02), "plasma")],
        layers: Vec::new(),
        strips: Vec::new(),
        vias: Vec::new(),
    })
    .unwrap();
    m.add_port(Port::new("1", End::Start).with_modes(1)).unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, metres(0.005)).unwrap().into());
    // The port's modes in the plasma: ε(10 GHz) = 1 − 25/(100 − 10j), above the cut-off.
    Driven::new(&m, Order::One).unwrap();
    let modes = emcraft_solver::driven::port_modes(&m, Order::One, 10e9).unwrap();
    let g = modes[0].1[0].gamma.unwrap();
    assert!(g.re > 0.0 && g.im > g.re, "{g}");
    let e = emcraft_solver::eigen::eigenmodes(&m, Order::One, 2, 10e9).unwrap_err();
    assert!(e.to_string().contains("dispersive"), "{e}");
}

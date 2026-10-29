//! Losses in the time domain against closed forms: lossy dielectrics and magnetic losses
//! (realised as conductivities matched at the band's centre) against the TE10 cascade of the
//! same material model, ε = εr − jσ/(ωε₀), written here apart from the solver's; walls of finite
//! conductivity against the TE10 conductor attenuation; ports in lossy and dispersive media.

use std::f64::consts::PI;

use emcraft_fdtd::{Options, TdResult, s_parameters};
use emcraft_model::units::C0;
use emcraft_model::{Complex64 as C, End, Material, Model, Pole, Port, Section, Waveguide};
use emcraft_theory::{Layer, Te10Line, te10_wall_attenuation};

const A: f64 = 0.02286;
const B: f64 = 0.01016;
const FREQS: [f64; 3] = [8e9, 10e9, 12e9];
/// The band's centre, where the loss tangents are matched.
const FC: f64 = 10e9;

fn guide(materials: Vec<Material>, sections: Vec<Section>) -> Model {
    let mut m = Model::new();
    for x in materials {
        m.add_material(x).unwrap();
    }
    m.set_waveguide(Waveguide { width: A, height: B, sections, layers: Vec::new(), strips: Vec::new(), vias: Vec::new() }).unwrap();
    m.add_port(Port::new("1", End::Start).with_modes(1)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(1)).unwrap();
    m
}

fn run(m: &Model, cell: f64, freqs: &[f64]) -> TdResult {
    let r = s_parameters(m, &Options::new(cell), freqs).unwrap();
    assert!(r.converged);
    assert_eq!(r.loss_reference, 0.5 * (freqs[0] + freqs[freqs.len() - 1]));
    r
}

/// A layer whose loss tangent is a conductivity's at `f`: tan δ FC/f.
fn conducting(length: f64, eps_r: f64, mu_r: f64, tan_d: f64, tan_dm: f64, f: f64) -> Layer {
    Layer { length, eps_r, mu_r, loss_tangent: tan_d * FC / f, magnetic_loss_tangent: tan_dm * FC / f }
}

fn worst(r: &TdResult, exact: impl Fn(f64) -> (C, C)) -> (f64, f64) {
    let (mut s, mut mag) = (0.0f64, 0.0f64);
    for (i, f) in r.frequencies.iter().enumerate() {
        let (e11, e21) = exact(*f);
        s = s.max((r.s11[i] - e11).norm().max((r.s21[i] - e21).norm()));
        mag = mag.max((r.s21[i].norm() - e21.norm()).abs());
    }
    (s, mag)
}

/// A 20 mm slab of εr = 2.2, tan δ = 0.05 between 10 mm of empty guide.
fn lossy_slab() -> Model {
    let lossy = Material::new("lossy", 2.2, 1.0).unwrap().with_loss_tangent(0.05).unwrap();
    guide(vec![lossy], vec![Section::new(0.01, "vacuum"), Section::new(0.02, "lossy"), Section::new(0.01, "vacuum")])
}

fn lossy_slab_exact(f: f64) -> (C, C) {
    let air = Layer::lossless(0.01, 1.0, 1.0);
    Te10Line { a: A, layers: vec![air, conducting(0.02, 2.2, 1.0, 0.05, 0.0, f), air] }.solve(f).unwrap().s_power()
}

#[test]
fn a_lossy_slab_matches_the_cascade_of_its_conductivity() {
    let r = run(&lossy_slab(), 0.0015, &FREQS);
    let (s, mag) = worst(&r, lossy_slab_exact);
    // Measured 5.2e-2 (the grid's phase error, as for a lossless slab of the same index) and
    // 3.8e-3 in |S21|, both second order (`losses_converge_at_second_order`).
    assert!(s < 8e-2 && mag < 6e-3, "{s:e} {mag:e}");
    for i in 0..3 {
        assert!(r.s11[i].norm_sqr() + r.s21[i].norm_sqr() < 0.98);
    }
}

#[test]
fn magnetic_losses_match_the_cascade_of_their_magnetic_conductivity() {
    let ferrite = Material::new("ferrite", 1.5, 2.0).unwrap().with_magnetic_loss_tangent(0.05).unwrap();
    let m = guide(vec![ferrite], vec![Section::new(0.01, "vacuum"), Section::new(0.02, "ferrite"), Section::new(0.01, "vacuum")]);
    let r = run(&m, 0.0015, &FREQS);
    let air = Layer::lossless(0.01, 1.0, 1.0);
    let (s, mag) = worst(&r, |f| Te10Line { a: A, layers: vec![air, conducting(0.02, 1.5, 2.0, 0.0, 0.05, f), air] }.solve(f).unwrap().s_power());
    // Measured 0.105 and 8.1e-3: n = √(εr μr) = 1.7, a shorter wavelength on the same cells.
    assert!(s < 0.15 && mag < 1.2e-2, "{s:e} {mag:e}");
}

#[test]
fn finite_conductivity_walls_attenuate_by_the_te10_conductor_loss() {
    // 100 mm of empty guide, walls of 1e6 S/m: α_c = 0.08–0.07 Np/m over the band.
    let sigma = 1e6;
    let mut m = guide(Vec::new(), vec![Section::new(0.1, "vacuum")]);
    m.set_wall_conductivity(Some(sigma)).unwrap();
    let r = run(&m, 0.0015, &FREQS);
    for (i, &f) in FREQS.iter().enumerate() {
        let alpha = te10_wall_attenuation(A, B, f, sigma, 1.0);
        let measured = -r.s21[i].norm().ln() / 0.1;
        // The phase: β of the lossless guide, to the grid's dispersion.
        let beta = ((2.0 * PI * f / C0).powi(2) - (PI / A).powi(2)).sqrt();
        let phase = (r.s21[i] * C::from_polar(1.0, beta * 0.1)).arg();
        eprintln!(
            "{f:e}: α {measured:.5} against {alpha:.5} ({:+.2} %), phase {phase:.4}, |S11| {:.1e}",
            100.0 * (measured / alpha - 1.0),
            r.s11[i].norm()
        );
        // Measured +0.9, +0.9 and +0.8 %.
        assert!((measured / alpha - 1.0).abs() < 0.02, "{f}: {measured} {alpha}");
        assert!(r.s11[i].norm() < 2e-3);
    }
}

#[test]
fn a_lossy_port_carries_the_complex_propagation_constant() {
    // The whole guide lossy: S21 = e^{−γL} with γ the realised material's, S11 ≈ 0.
    let lossy = Material::new("lossy", 2.2, 1.0).unwrap().with_loss_tangent(0.02).unwrap();
    let m = guide(vec![lossy], vec![Section::new(0.05, "lossy")]);
    let r = run(&m, 0.0015, &FREQS);
    let (s, mag) = worst(&r, |f| Te10Line { a: A, layers: vec![conducting(0.05, 2.2, 1.0, 0.02, 0.0, f)] }.solve(f).unwrap().s_power());
    // Measured 0.135 (the phase over 50 mm, 12 rad) and 4.0e-3 (α to 3 %), second order.
    assert!(s < 0.2 && mag < 6e-3, "{s:e} {mag:e}");
    // One lossy port and one empty: the pseudo-waves of the cascade.
    let lossy = Material::new("lossy", 2.2, 1.0).unwrap().with_loss_tangent(0.02).unwrap();
    let m = guide(vec![lossy], vec![Section::new(0.02, "vacuum"), Section::new(0.02, "lossy")]);
    let r = run(&m, 0.0015, &FREQS);
    let exact = |f: f64| {
        Te10Line { a: A, layers: vec![Layer::lossless(0.02, 1.0, 1.0), conducting(0.02, 2.2, 1.0, 0.02, 0.0, f)] }.solve(f).unwrap().s_power()
    };
    let (s, mag) = worst(&r, exact);
    // Measured 6.3e-2 and 9.7e-3 (|S21| at 12 GHz: 9.7e-3, 4.7e-3, 2.5e-3 at 1.5, 1 and 0.75 mm).
    assert!(s < 0.1 && mag < 1.5e-2, "{s:e} {mag:e}");
}

#[test]
fn a_dispersive_port_carries_its_complex_propagation_constant() {
    let tau = 1.0 / (2.0 * PI * 20e9);
    let debye = Material::new("debye", 2.0, 1.0).unwrap().with_dispersion(vec![Pole::Debye { delta_eps: 0.6, tau }]).unwrap();
    let m = guide(vec![debye], vec![Section::new(0.03, "debye")]);
    let r = run(&m, 0.0015, &FREQS);
    let exact = |f: f64| {
        let e = C::new(2.0, 0.0) + 0.6 / C::new(1.0, 2.0 * PI * f * tau);
        Te10Line { a: A, layers: vec![Layer::lossy(0.03, e.re, 1.0, -e.im / e.re)] }.solve(f).unwrap().s_power()
    };
    let (s, mag) = worst(&r, exact);
    // Measured 6.2e-2 and 1.4e-2 (α over 30 mm), second order.
    assert!(s < 0.1 && mag < 2e-2, "{s:e} {mag:e}");
}

#[test]
fn hostile_losses_are_refused_or_stay_stable() {
    let air = || guide(Vec::new(), vec![Section::new(0.02, "vacuum")]);
    for bad in [f64::NAN, f64::INFINITY, -1.0, 0.0, f64::MAX, 1e21] {
        let mut m = air();
        m.wall_conductivity = Some(bad);
        assert!(s_parameters(&m, &Options::new(0.003), &[10e9]).is_err(), "{bad}");
    }
    // Not a good conductor at the band's top.
    let mut m = air();
    m.wall_conductivity = Some(1.0);
    assert!(s_parameters(&m, &Options::new(0.003), &[10e9]).is_err());
    // A huge conductivity: the walls all but perfect.
    let mut m = air();
    m.wall_conductivity = Some(emcraft_fdtd::MAX_WALL_CONDUCTIVITY);
    let r = s_parameters(&m, &Options::new(0.003), &[9e9, 11e9]).unwrap();
    assert!((r.s21[0].norm() - 1.0).abs() < 1e-3, "{:?}", r.s21);
    // Loss tangents read from a file unchecked: refused.
    let mut m = lossy_slab();
    m.materials[1].loss_tangent = f64::NAN;
    assert!(s_parameters(&m, &Options::new(0.003), &[10e9]).is_err());
    m.materials[1].loss_tangent = -0.1;
    assert!(s_parameters(&m, &Options::new(0.003), &[10e9]).is_err());
    // The largest loss tangent the model takes: a slab all but metal, stable at the usual step
    // (the field it lets in diffuses for a long time on the grid: a run cut short).
    let metal = Material::new("metal", 1.0, 1.0).unwrap().with_loss_tangent(emcraft_model::material::MAX_LOSS_TANGENT).unwrap();
    let metal_m = Material::new("metal_m", 1.0, 1.0).unwrap().with_magnetic_loss_tangent(emcraft_model::material::MAX_LOSS_TANGENT).unwrap();
    for (name, x) in [("metal", metal), ("metal_m", metal_m)] {
        let m = guide(vec![x], vec![Section::new(0.01, "vacuum"), Section::new(0.006, name), Section::new(0.01, "vacuum")]);
        let r = s_parameters(&m, &Options { max_steps: 20_000, ..Options::new(0.003) }, &[9e9, 11e9]).unwrap();
        // |S11| within the coarse grid's error of 1 (0.99 at 1.5 mm).
        assert!(r.s21.iter().all(|s| s.norm() < 1e-3) && r.s11.iter().all(|s| s.norm() < 1.05), "{name}: {r:?}");
    }
}

/// The largest |S21| error of the lossy slab and the dispersive port at `cell`.
fn magnitude_errors(cell: f64) -> [f64; 2] {
    let slab = worst(&run(&lossy_slab(), cell, &FREQS), lossy_slab_exact).1;
    let tau = 1.0 / (2.0 * PI * 20e9);
    let debye = Material::new("debye", 2.0, 1.0).unwrap().with_dispersion(vec![Pole::Debye { delta_eps: 0.6, tau }]).unwrap();
    let r = run(&guide(vec![debye], vec![Section::new(0.03, "debye")]), cell, &FREQS);
    let port = worst(&r, |f| {
        let e = C::new(2.0, 0.0) + 0.6 / C::new(1.0, 2.0 * PI * f * tau);
        Te10Line { a: A, layers: vec![Layer::lossy(0.03, e.re, 1.0, -e.im / e.re)] }.solve(f).unwrap().s_power()
    })
    .1;
    [slab, port]
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn losses_converge_at_second_order() {
    // Measured (1.5 mm → 1 mm) 3.8e-3 → 1.7e-3 and 1.38e-2 → 5.97e-3: rate 2.0 and 2.1.
    let (coarse, fine) = (magnitude_errors(0.0015), magnitude_errors(0.001));
    for (c, f) in coarse.iter().zip(&fine) {
        let rate = (c / f).ln() / 1.5f64.ln();
        eprintln!("{c:e} → {f:e}: rate {rate:.2}");
        assert!(rate > 1.7, "{c:e} {f:e} rate {rate}");
    }
}

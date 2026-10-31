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
    m.set_waveguide(Waveguide {
        width: A,
        height: B,
        sections: vec![Section::new(D, "vacuum")],
        layers: Vec::new(),
        strips: Vec::new(),
        vias: Vec::new(),
    })
    .unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, size).unwrap().into());
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
fn modes_far_above_the_shift_come_with_the_gradients_projected_out() {
    // Twelve modes asked near 7 GHz: the higher ones (to about 20 GHz) lie beyond 2 × the
    // shift's k², farther from it than the gradients' zero eigenvalue. With the gradients
    // deflated they come all the same; against the closed form's twelve lowest.
    let e = eigenmodes(&cavity(0.003), Order::Two, 12, 7.0e9).unwrap();
    let exact: Vec<f64> = cavity_resonances(A, B, D, 1.0, 12).into_iter().map(|(f, _)| f).collect();
    assert_eq!(e.modes.len(), 12);
    assert!(exact[11] > 7.0e9 * 2f64.sqrt() * 1.3, "the test reaches beyond 2σ: {}", exact[11]);
    for (m, f) in e.modes.iter().zip(&exact) {
        assert!(((m.frequency - f) / f).abs() < 3e-3, "{} {f}", m.frequency);
    }
}

#[test]
fn a_very_lossy_fill_gives_its_complex_frequency() {
    // tan δ = 0.05 throughout: K x = k² ε′(1 − j tan δ) M₀ x, so every k² is the lossless one
    // divided by 1 − j tan δ, ω = ω₀ (1 − j tan δ)^{−1/2} — exactly, for the discrete problem
    // too. Q = Re ω / (2 Im ω) = 1/(2 tan(½ atan tan δ)) = 20.0125, not perturbation's 20.
    let tan_d: f64 = 0.05;
    let mut m = Model::new();
    m.add_material(Material::new("fill", 2.25, 1.0).unwrap().with_loss_tangent(tan_d).unwrap()).unwrap();
    m.set_waveguide(Waveguide {
        width: A,
        height: B,
        sections: vec![Section::new(D, "fill")],
        layers: Vec::new(),
        strips: Vec::new(),
        vias: Vec::new(),
    })
    .unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, 0.003).unwrap().into());
    let lossy = eigenmodes(&m, Order::Two, 2, 5.5e9).unwrap();
    let mut lossless = m.clone();
    lossless.materials[1] = Material::new("fill", 2.25, 1.0).unwrap();
    let plain = eigenmodes(&lossless, Order::Two, 2, 5.5e9).unwrap();
    let q_exact = 1.0 / (2.0 * (0.5 * tan_d.atan()).tan());
    let shrink = (1.0 + tan_d * tan_d).powf(-0.25) * (0.5 * tan_d.atan()).cos();
    for (l, p) in lossy.modes.iter().zip(&plain.modes) {
        let q = l.q_dielectric.unwrap();
        assert!((q / q_exact - 1.0).abs() < 1e-7, "{q} {q_exact}");
        assert!((l.frequency / (p.frequency * shrink) - 1.0).abs() < 1e-9, "{} {}", l.frequency, p.frequency * shrink);
    }
}

#[test]
fn copper_te101_q() {
    let mut m = cavity(0.003);
    m.set_wall_conductivity(Some(emcraft_model::Conductivity::new(5.8e7).unwrap()));
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
            m.set_wall_conductivity(Some(emcraft_model::Conductivity::new(5.8e7).unwrap()));
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
    m.set_waveguide(Waveguide {
        width: A,
        height: B,
        sections: vec![Section::new(D, "fill")],
        layers: Vec::new(),
        strips: Vec::new(),
        vias: Vec::new(),
    })
    .unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, 0.003).unwrap().into());
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

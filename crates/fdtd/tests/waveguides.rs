//! The time-domain solver against the closed-form TE10 cascade and against mode matching: an
//! independent discretisation (finite integration on a hexahedral grid, a time-stepped pulse,
//! Fourier transforms) of the same problems the frequency-domain FEM verifies.

use std::f64::consts::PI;

use emcraft_fdtd::{Options, s_parameters};
use emcraft_model::{End, Material, Model, Pole, Port, Section, Waveguide};
use emcraft_theory::{HPlaneLine, HSection, Layer, Te10Line};

const A: f64 = 0.02286;
const B: f64 = 0.01016;

fn model(sections: Vec<Section>, materials: &[(&str, f64)]) -> Model {
    with_materials(sections, materials.iter().map(|(name, eps)| Material::new(name, *eps, 1.0).unwrap()).collect())
}

fn with_materials(sections: Vec<Section>, materials: Vec<Material>) -> Model {
    let mut m = Model::new();
    for x in materials {
        m.add_material(x).unwrap();
    }
    m.set_waveguide(Waveguide { width: A, height: B, sections, layers: Vec::new(), strips: Vec::new() }).unwrap();
    m.add_port(Port::new("1", End::Start).with_modes(1)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(1)).unwrap();
    m
}

fn filter() -> Model {
    model(vec![Section::new(0.015, "ptfe"), Section::new(0.02, "vacuum"), Section::new(0.015, "ptfe")], &[("ptfe", 2.0)])
}

fn filter_error(cell: f64, freqs: &[f64]) -> f64 {
    let r = s_parameters(&filter(), &Options::new(cell), freqs).unwrap();
    assert!(r.converged);
    let mut worst: f64 = 0.0;
    let line = Te10Line { a: A, layers: vec![Layer::lossless(0.015, 2.0, 1.0), Layer::lossless(0.02, 1.0, 1.0), Layer::lossless(0.015, 2.0, 1.0)] };
    for (i, f) in freqs.iter().enumerate() {
        let (e11, e21) = line.solve(*f).unwrap().s_power();
        worst = worst.max((r.s11[i] - e11).norm().max((r.s21[i] - e21).norm()));
        // Passive, to the discretisation (2e-3 at 1.5 mm, 1e-4 at 0.5 mm).
        assert!(r.s11[i].norm_sqr() + r.s21[i].norm_sqr() < 1.0 + 5e-3);
    }
    worst
}

#[test]
fn high_pass_filter_matches_the_exact_cascade() {
    // Second order in the cell: 2.5e-2 at 1.5 mm (λg/20 to λg/40), mostly phase.
    let worst = filter_error(0.0015, &[5.5e9, 6.5e9, 7.5e9, 8.5e9]);
    assert!(worst < 4e-2, "{worst:e}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn high_pass_filter_converges_at_second_order() {
    let freqs = [5.5e9, 7.5e9, 8.5e9];
    let (coarse, fine) = (filter_error(0.001, &freqs), filter_error(0.0005, &freqs));
    // Measured 1.16e-2 and 2.8e-3: rate 2.05.
    let rate = (coarse / fine).log2();
    assert!(rate > 1.7 && fine < 5e-3, "{coarse:e} {fine:e} rate {rate}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn inductive_iris_matches_mode_matching() {
    let iris = Section { length: 0.002, material: "vacuum".into(), width: Some(0.012), offset: None, ..Default::default() };
    let m = model(vec![Section::new(0.015, "vacuum"), iris, Section::new(0.015, "vacuum")], &[]);
    let freqs = [9.0e9, 11.0e9];
    let r = s_parameters(&m, &Options::new(0.001), &freqs).unwrap();
    let full = |l: f64| HSection { length: l, eps_r: 1.0, mu_r: 1.0, x0: 0.0, width: A, loss_tangent: 0.0 };
    let line = HPlaneLine { sections: vec![full(0.015), HSection { length: 0.002, x0: (A - 0.012) / 2.0, width: 0.012, ..full(0.0) }, full(0.015)] };
    for (i, f) in freqs.iter().enumerate() {
        let (e11, e21) = line.solve(*f, 160).unwrap();
        let err = (r.s11[i] - e11).norm().max((r.s21[i] - e21).norm());
        // The iris' edges are singular and the staircase of cells does not grade towards them:
        // about 4.5e-2 at 1 mm, first order (the FEM grades its mesh instead).
        assert!(err < 6e-2, "{f}: {err:e}");
    }
}

#[test]
fn hostile_requests_are_refused() {
    let m = filter();
    assert!(s_parameters(&m, &Options::new(0.0), &[7e9]).is_err());
    assert!(s_parameters(&m, &Options::new(f64::NAN), &[7e9]).is_err());
    assert!(s_parameters(&m, &Options::new(1e-6), &[7e9]).is_err());
    assert!(s_parameters(&m, &Options::new(0.002), &[]).is_err());
    assert!(s_parameters(&m, &Options::new(0.002), &[-1.0]).is_err());
    assert!(s_parameters(&m, &Options { courant: 1.5, ..Options::new(0.002) }, &[7e9]).is_err());
    // Below the ports' cut-off.
    assert!(s_parameters(&m, &Options::new(0.002), &[4e9]).is_err());
    let mut lossy = filter();
    lossy.set_wall_conductivity(Some(5.8e7)).unwrap();
    assert!(s_parameters(&lossy, &Options::new(0.002), &[7e9]).is_err());
}

#[test]
fn e_plane_step_matches_mode_matching() {
    // The guide steps down to half its height against the y = 0 wall.
    let low = Section { length: 0.015, material: "vacuum".into(), height: Some(B / 2.0), offset_y: Some(0.0), ..Default::default() };
    let m = model(vec![Section::new(0.012, "vacuum"), low], &[]);
    let freqs = [9.0e9, 11.0e9];
    let r = s_parameters(&m, &Options::new(0.001), &freqs).unwrap();
    let full = emcraft_theory::ESection { length: 0.012, eps_r: 1.0, mu_r: 1.0, loss_tangent: 0.0, y0: 0.0, height: B };
    let line = emcraft_theory::EPlaneLine { a: A, sections: vec![full, emcraft_theory::ESection { length: 0.015, height: B / 2.0, ..full }] };
    for (i, f) in freqs.iter().enumerate() {
        let (e11, e21) = line.solve(*f, 160).unwrap();
        let err = (r.s11[i] - e11).norm().max((r.s21[i] - e21).norm());
        // Measured 1.0e-2 and 1.7e-2 at 1 mm cells.
        assert!(err < 3e-2, "{f}: {err:e}");
    }
}

/// A 20 mm slab of ε∞ = 2 with a Debye pole (Δε = 0.6 relaxing at 20 GHz) and a Lorentz pole
/// (Δε = 0.4 at 16 GHz, damping 1.5 GHz) between 10 mm of empty guide.
fn dispersive_slab() -> Model {
    let tau = 1.0 / (2.0 * PI * 20e9);
    let poles = vec![Pole::Debye { delta_eps: 0.6, tau }, Pole::Lorentz { delta_eps: 0.4, frequency: 16e9, damping: 1.5e9 }];
    let disp = Material::new("disp", 2.0, 1.0).unwrap().with_dispersion(poles).unwrap();
    with_materials(vec![Section::new(0.01, "vacuum"), Section::new(0.02, "disp"), Section::new(0.01, "vacuum")], vec![disp])
}

/// The cascade with the poles' permittivity written out here, apart from the model's.
fn dispersive_exact(f: f64) -> (emcraft_model::Complex64, emcraft_model::Complex64) {
    use emcraft_model::Complex64 as C;
    let (w, w0, g, tau) = (2.0 * PI * f, 2.0 * PI * 16e9, 2.0 * PI * 1.5e9, 1.0 / (2.0 * PI * 20e9));
    let e = C::new(2.0, 0.0) + 0.6 / C::new(1.0, w * tau) + 0.4 * w0 * w0 / C::new(w0 * w0 - w * w, w * g);
    let slab = Layer { length: 0.02, eps_r: e.re, mu_r: 1.0, loss_tangent: -e.im / e.re };
    Te10Line { a: A, layers: vec![Layer::lossless(0.01, 1.0, 1.0), slab, Layer::lossless(0.01, 1.0, 1.0)] }.solve(f).unwrap().s_power()
}

/// The largest |S − S_exact| of the dispersive slab at `freqs` with cells of `cell`.
fn dispersive_error(cell: f64, freqs: &[f64]) -> f64 {
    let r = s_parameters(&dispersive_slab(), &Options::new(cell), freqs).unwrap();
    assert!(r.converged);
    let mut worst: f64 = 0.0;
    for (i, f) in freqs.iter().enumerate() {
        let (e11, e21) = dispersive_exact(*f);
        worst = worst.max((r.s11[i] - e11).norm().max((r.s21[i] - e21).norm()));
        // It absorbs.
        assert!(r.s11[i].norm_sqr() + r.s21[i].norm_sqr() < 0.99);
    }
    worst
}

#[test]
fn dispersive_slab_matches_the_exact_cascade() {
    // The auxiliary differential equations against ε(ω) in closed form. Measured 1.9e-2, 3.7e-2
    // and 7.2e-2 at 8, 10 and 12 GHz with 1.5 mm cells: the grid's phase error, as large as for
    // a lossless slab of the same index (1.4e-2, 5.8e-2, 6.7e-2).
    let worst = dispersive_error(0.0015, &[8e9, 10e9, 12e9]);
    assert!(worst < 0.1, "{worst:e}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn dispersive_slab_converges_at_second_order() {
    // Measured 3.5e-2 at 1 mm and 1.6e-2 at 0.7 mm (12 GHz): rate 2.1.
    let freqs = [8e9, 12e9];
    let (coarse, fine) = (dispersive_error(0.001, &freqs), dispersive_error(0.0007, &freqs));
    let rate = (coarse / fine).ln() / (0.001f64 / 0.0007).ln();
    assert!(rate > 1.7 && fine < 2.5e-2, "{coarse:e} {fine:e} rate {rate}");
}

#[test]
fn dispersion_is_refused_where_unsupported() {
    // At a port.
    let mut m = Model::new();
    let drude = Pole::Drude { plasma_frequency: 5e9, damping: 1e9 };
    m.add_material(Material::new("plasma", 1.0, 1.0).unwrap().with_dispersion(vec![drude]).unwrap()).unwrap();
    m.set_waveguide(Waveguide { width: A, height: B, sections: vec![Section::new(0.02, "plasma")], layers: Vec::new(), strips: Vec::new() }).unwrap();
    assert!(matches!(s_parameters(&m, &Options::new(0.002), &[10e9]), Err(emcraft_fdtd::FdtdError::Unsupported(_))));
    // A resonance far too fast for the time step.
    let fast = Pole::Lorentz { delta_eps: 1.0, frequency: 1e14, damping: 1e9 };
    let fast = Material::new("fast", 1.0, 1.0).unwrap().with_dispersion(vec![fast]).unwrap();
    let m = with_materials(vec![Section::new(0.01, "vacuum"), Section::new(0.01, "fast"), Section::new(0.01, "vacuum")], vec![fast]);
    assert!(matches!(s_parameters(&m, &Options::new(0.002), &[10e9]), Err(emcraft_fdtd::FdtdError::Unsupported(_))));
}

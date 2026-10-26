//! The time-domain solver against the closed-form TE10 cascade and against mode matching: an
//! independent discretisation (finite integration on a hexahedral grid, a time-stepped pulse,
//! Fourier transforms) of the same problems the frequency-domain FEM verifies.

use emcraft_fdtd::{Options, s_parameters};
use emcraft_model::{End, Material, Model, Port, Section, Waveguide};
use emcraft_theory::{HPlaneLine, HSection, Layer, Te10Line};

const A: f64 = 0.02286;
const B: f64 = 0.01016;

fn model(sections: Vec<Section>, materials: &[(&str, f64)]) -> Model {
    let mut m = Model::new();
    for (name, eps) in materials {
        m.add_material(Material::new(name, *eps, 1.0).unwrap()).unwrap();
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

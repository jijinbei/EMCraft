//! H-plane discontinuities (cross-sections that change along z) against converged mode
//! matching (`emcraft_theory::modematch`), an independent method: the FEM meshes the iris or step
//! in 3D, mode matching expands each section in TE_m0 modes and matches them on the junction.

use emcraft_fem::Order;
use emcraft_model::{End, Model, Port, Section, Waveguide};
use emcraft_solver::Driven;
use emcraft_theory::{HPlaneLine, HSection};

const A: f64 = 0.02286;
const B: f64 = 0.01016;

/// (length, aperture start, aperture width), all vacuum.
type Layout = [(f64, f64, f64)];

fn model(layout: &Layout, opts: &emcraft_mesh::Options) -> Model {
    let mut m = Model::new();
    let sections = layout.iter().map(|&(l, x0, w)| Section { length: l, material: "vacuum".into(), width: Some(w), offset: Some(x0) }).collect();
    m.set_waveguide(Waveguide { width: A, height: B, sections }).unwrap();
    m.add_port(Port { name: "1".into(), end: End::Start, modes: 4 }).unwrap();
    m.add_port(Port { name: "2".into(), end: End::End, modes: 4 }).unwrap();
    m.mesh = Some(emcraft_mesh::waveguide_with(&m, opts).unwrap());
    m
}

fn reference(layout: &Layout) -> HPlaneLine {
    HPlaneLine { sections: layout.iter().map(|&(length, x0, width)| HSection { length, eps_r: 1.0, mu_r: 1.0, x0, width }).collect() }
}

/// The largest |S − S_mm| over the frequencies (S11 and S21).
fn worst(d: &Driven, line: &HPlaneLine, freqs: &[f64]) -> f64 {
    let mut w: f64 = 0.0;
    for &f in freqs {
        let r = d.solve(f).unwrap();
        let (s11, s21) = line.solve(f, 160).unwrap();
        w = w.max((r.s[0][0] - s11).norm()).max((r.s[1][0] - s21).norm());
    }
    w
}

/// A 2 mm thick centred inductive iris, 12 mm wide, 15 mm from each port.
const IRIS: [(f64, f64, f64); 3] = [(0.015, 0.0, A), (0.002, (A - 0.012) / 2.0, 0.012), (0.015, 0.0, A)];

/// An asymmetric H-plane step: the guide narrows to 16 mm against its x = 0 wall.
const STEP: [(f64, f64, f64); 2] = [(0.015, 0.0, A), (0.018, 0.0, 0.016)];

#[test]
fn inductive_iris_matches_mode_matching() {
    let d = Driven::new(&model(&IRIS, &emcraft_mesh::Options::new(0.004)), Order::Two).unwrap();
    let w = worst(&d, &reference(&IRIS), &[9.0e9, 11.0e9]);
    // Measured 6.6e-3: the iris' edges are singular (E ∝ r^{-1/3}); grading recovers most of
    // what a uniform mesh loses there.
    assert!(w < 1.5e-2, "{w:e}");
}

#[test]
fn offset_step_matches_mode_matching_and_conserves_power() {
    let d = Driven::new(&model(&STEP, &emcraft_mesh::Options::new(0.004)), Order::Two).unwrap();
    // The 16 mm guide's TE10 cut-off is 9.37 GHz: above it both ports propagate.
    let freqs = [10.0e9, 12.0e9];
    let w = worst(&d, &reference(&STEP), &freqs);
    // Measured 3.1e-3.
    assert!(w < 6e-3, "{w:e}");
    for f in freqs {
        let r = d.solve(f).unwrap();
        for q in 0..2 {
            let p = r.s[0][q].norm_sqr() + r.s[1][q].norm_sqr();
            assert!((p - 1.0).abs() < 1e-10, "{f}: {p}");
        }
        assert!((r.s[0][1] - r.s[1][0]).norm() < 1e-10);
    }
}

#[test]
fn grading_towards_the_edges_beats_a_uniform_mesh() {
    // The same element size: graded towards the iris' edges, then uniform.
    let line = reference(&IRIS);
    let graded = worst(&Driven::new(&model(&IRIS, &emcraft_mesh::Options::new(0.004)), Order::Two).unwrap(), &line, &[10.0e9]);
    let uniform = worst(&Driven::new(&model(&IRIS, &emcraft_mesh::Options::uniform(0.004)), Order::Two).unwrap(), &line, &[10.0e9]);
    assert!(graded < 0.5 * uniform, "{graded:e} {uniform:e}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn iris_converges_with_graded_meshes() {
    // Graded meshes of 4, 3 and 2 mm: the error falls with the size.
    let line = reference(&IRIS);
    let errors: Vec<f64> = [0.004, 0.003, 0.002]
        .iter()
        .map(|&h| worst(&Driven::new(&model(&IRIS, &emcraft_mesh::Options::new(h)), Order::Two).unwrap(), &line, &[10.0e9]))
        .collect();
    assert!(errors[0] > errors[1] && errors[1] > errors[2] && errors[2] < 5e-3, "{errors:?}");
}

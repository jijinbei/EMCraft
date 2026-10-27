//! Periodic cells: an infinite dielectric slab under a plane wave (Floquet ports) against the
//! transfer-matrix closed form (`emcraft_theory::floquet::Stack`), independent of the FEM.

use emcraft_fem::Order;
use emcraft_model::{End, Material, Model, Periodic, Polarization, Port, Section, Waveguide};
use emcraft_solver::Driven;
use emcraft_theory::floquet::{Polarization as Pol, Stack, StackLayer};
use num_complex::Complex64 as C;

const CELL: f64 = 0.01;
const AIR: f64 = 0.006;
const SLAB: f64 = 0.005;
const EPS: f64 = 4.0;

fn slab_cell(theta: f64, phi: f64, pol: Polarization, size: f64) -> Model {
    let mut m = Model::new();
    m.add_material(Material::new("slab", EPS, 1.0).unwrap()).unwrap();
    let sections = vec![Section::new(AIR, "vacuum"), Section::new(SLAB, "slab"), Section::new(AIR, "vacuum")];
    m.set_waveguide(Waveguide { width: CELL, height: CELL, sections, layers: Vec::new(), strips: Vec::new(), vias: Vec::new() }).unwrap();
    m.add_port(Port::new("1", End::Start).with_modes(2)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(2)).unwrap();
    m.set_periodic(Some(Periodic { x: true, y: true, z: false, theta, phi, polarization: pol })).unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, size).unwrap());
    m
}

fn reference(theta: f64, pol: Polarization) -> Stack {
    let layer = |t: f64, e: f64| StackLayer { thickness: t, eps_r: C::new(e, 0.0), mu_r: 1.0 };
    Stack {
        layers: vec![layer(AIR, 1.0), layer(SLAB, EPS), layer(AIR, 1.0)],
        theta,
        polarization: match pol {
            Polarization::Te => Pol::Te,
            Polarization::Tm => Pol::Tm,
        },
    }
}

fn worst(theta: f64, phi: f64, pol: Polarization, size: f64, freqs: &[f64]) -> f64 {
    let d = Driven::new(&slab_cell(theta, phi, pol, size), Order::Two).unwrap();
    let st = reference(theta, pol);
    let mut w: f64 = 0.0;
    for &f in freqs {
        let r = d.solve(f).unwrap();
        let (s11, s21) = st.s(f).unwrap();
        eprintln!("{f:e} fem {} {} ref {s11} {s21} berr {:e}", r.s[0][0], r.s[1][0], r.backward_error);
        w = w.max((r.s[0][0] - s11).norm()).max((r.s[1][0] - s21).norm());
    }
    w
}

#[test]
fn slab_at_normal_incidence() {
    let w = worst(0.0, 0.0, Polarization::Te, 0.0025, &[8e9, 12e9]);
    assert!(w < 1e-2, "{w:e}");
}

#[test]
fn slab_at_oblique_incidence_te_and_tm() {
    for pol in [Polarization::Te, Polarization::Tm] {
        let w = worst(30f64.to_radians(), 20f64.to_radians(), pol, 0.0025, &[10e9]);
        assert!(w < 1e-2, "{pol:?}: {w:e}");
    }
}

#[test]
fn oblique_cells_are_deterministic_passive_and_reciprocal() {
    let d = Driven::new(&slab_cell(25f64.to_radians(), 0.4, Polarization::Te, 0.0025), Order::Two).unwrap();
    let (a, b) = (d.solve(9e9).unwrap(), d.solve(9e9).unwrap());
    assert_eq!(a.s, b.s, "the same input gives the same S, bit for bit");
    // Lossless: |S11|² + |S21|² = 1 but for what the mesh scatters into the cross-polarised
    // (0, 0) mode, which propagates too (measured 2.7e-8 of the power); the slab is symmetric:
    // S22 = S11.
    let (s11, s21, s22) = (a.s[0][0], a.s[1][0], a.s[1][1]);
    assert!((s11.norm_sqr() + s21.norm_sqr() - 1.0).abs() < 1e-6, "{}", s11.norm_sqr() + s21.norm_sqr());
    assert!((s22 - s11).norm() < 1e-3, "{s11} {s22}");
}

#[test]
fn the_reduced_curl_curl_matrix_is_hermitian() {
    // Tᴴ K T with K real symmetric and a complex phase: Hermitian, not symmetric.
    let mut m = slab_cell(0.0, 0.0, Polarization::Te, 0.004);
    m.ports.clear();
    m.set_periodic(Some(Periodic { x: true, y: true, z: true, theta: 0.0, phi: 0.0, polarization: Polarization::Te })).unwrap();
    let d = Driven::cavity(&m, Order::One).unwrap();
    let tie = d.tie.as_ref().unwrap();
    let a = d.matrix_without_ports(5e9).unwrap();
    let r = tie.reduce(&a.val, [0.7, -1.1, 2.3]);
    let (mut asym, mut aherm, mut scale) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..r.n {
        for k in r.ptr[i]..r.ptr[i + 1] {
            let j = r.col[k];
            let (x, y) = (r.val[k], r.get(j, i));
            scale = scale.max(x.norm());
            asym = asym.max((x - y).norm());
            aherm = aherm.max((x - y.conj()).norm());
        }
    }
    assert!(aherm < 1e-12 * scale, "{aherm:e}");
    assert!(asym > 1e-3 * scale, "a complex phase makes it unsymmetric: {asym:e}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn slab_converges() {
    let th = 40f64.to_radians();
    let coarse = worst(th, 0.3, Polarization::Tm, 0.0025, &[11e9]);
    let fine = worst(th, 0.3, Polarization::Tm, 0.00125, &[11e9]);
    eprintln!("coarse {coarse:e} fine {fine:e} ratio {}", coarse / fine);
    assert!(coarse / fine > 3.0, "{coarse:e} → {fine:e}");
}

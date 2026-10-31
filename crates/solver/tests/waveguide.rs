//! The driven solver against closed-form TE10 solutions (docs/verification.md, theory cases).
//!
//! Every guide here has one cross-section, so the TE10 wave stays TE10 through every interface
//! and `emcraft-theory` gives the exact S-parameters and fields: the only error left is the
//! discretisation's. The tolerances are what each element order and mesh resolves, measured
//! when the cases were set up; the heavy tests check the errors fall at the elements' rates.

mod common;

use common::*;
use emcraft_fem::Order;
use emcraft_solver::{Complex64 as C, Driven, Field, SolveError};

/// Frequencies across the filter's band: below the empty section's cut-off (6.557 GHz) and above.
const BAND: [f64; 6] = [5.0e9, 6.0e9, 6.5e9, 7.0e9, 8.0e9, 9.0e9];

/// The largest |S − S_exact| over the frequencies, for S11 and S21.
fn worst_s_error(d: &Driven, sections: &[(f64, f64)], freqs: &[f64]) -> f64 {
    let mut worst: f64 = 0.0;
    for &f in freqs {
        let r = d.solve(f).unwrap();
        let (s11, s21) = line(sections).solve(f).unwrap().s_power();
        worst = worst.max((r.s[1][0] - s21).norm()).max((r.s[0][0] - s11).norm());
    }
    worst
}

/// Relative L² errors of E and H at one frequency.
fn field_errors(d: &Driven, sections: &[(f64, f64)], f: f64) -> (f64, f64) {
    let r = d.solve(f).unwrap();
    let ex = line(sections).solve(f).unwrap();
    let (e, en) = d.l2_error(&r.fields[0], f, Field::E, |p| ex.e(p)).unwrap();
    let (h, hn) = d.l2_error(&r.fields[0], f, Field::H, |p| ex.h(p)).unwrap();
    (e / en, h / hn)
}

#[test]
fn straight_guide_transmits_with_the_te10_phase() {
    let sections = [(0.03, 1.0)];
    let d = Driven::new(&guide(&sections, 0.003, 1), Order::One).unwrap();
    for f in [8.0e9, 10.0e9, 12.0e9] {
        let r = d.solve(f).unwrap();
        let exact = line(&sections).solve(f).unwrap();
        // No reflection; S21 = e^{−jβL}.
        assert!(r.s[0][0].norm() < 5e-3, "{f}: S11 {}", r.s[0][0]);
        assert!((r.s[1][0] - exact.s21).norm() < 5e-3, "{f}: S21 {} exact {}", r.s[1][0], exact.s21);
        assert!(exact.s11.norm() < 1e-15);
    }
}

#[test]
fn dielectric_step_reflects_by_the_impedance_ratio() {
    // PTFE-like fill (εr = 2.1) meeting an empty guide: S11 = (β₁ − β₂)/(β₁ + β₂) at the step.
    let sections = [(0.012, 2.1), (0.018, 1.0)];
    let d = Driven::new(&guide(&sections, 0.003, 4), Order::Two).unwrap();
    for f in [7.5e9, 9.0e9] {
        let r = d.solve(f).unwrap();
        let (s11, s21) = line(&sections).solve(f).unwrap().s_power();
        assert!((r.s[0][0] - s11).norm() < 1e-3, "{f}: S11 {} exact {s11}", r.s[0][0]);
        assert!((r.s[1][0] - s21).norm() < 1e-3, "{f}: S21 {} exact {s21}", r.s[1][0]);
        // Power-normalised S is unitary although the port media differ.
        let unitary = r.s[0][0].norm_sqr() + r.s[1][0].norm_sqr();
        assert!((unitary - 1.0).abs() < 1e-10, "{unitary}");
    }
}

#[test]
fn high_pass_filter_order_one() {
    let d = Driven::new(&guide(&FILTER, 0.004, 4), Order::One).unwrap();
    let worst = worst_s_error(&d, &FILTER, &BAND);
    assert!(worst < 3e-2, "{worst:e}");
}

#[test]
fn high_pass_filter_order_two() {
    let d = Driven::new(&guide(&FILTER, 0.004, 4), Order::Two).unwrap();
    let worst = worst_s_error(&d, &FILTER, &BAND);
    assert!(worst < 4e-3, "{worst:e}");
    // The filter: below the empty section's cut-off the wave tunnels through it attenuated,
    // above it passes.
    let low = d.solve(5.0e9).unwrap();
    let high = d.solve(9.0e9).unwrap();
    assert!(low.s[1][0].norm() < 0.31 && high.s[1][0].norm() > 0.95);
}

#[test]
fn symmetric_and_solved_to_rounding() {
    // The filter being symmetric, S11 = S22: a property of the discrete system itself, to
    // rounding. (Lossless and reciprocal, as every template's S: the engine's invariants.rs.)
    let d = Driven::new(&guide(&FILTER, 0.004, 4), Order::Two).unwrap();
    for f in [5.5e9, 8.5e9] {
        let r = d.solve(f).unwrap();
        assert!((r.s[0][0] - r.s[1][1]).norm() < 1e-10);
        assert!(r.backward_error < 1e-14);
        assert_eq!(r.perturbed_pivots, 0);
    }
}

#[test]
fn fields_match_the_exact_te10_fields() {
    let d = Driven::new(&guide(&FILTER, 0.004, 4), Order::Two).unwrap();
    for f in [5.5e9, 8.0e9] {
        let (e, h) = field_errors(&d, &FILTER, f);
        assert!(e < 2.5e-2 && h < 3.5e-2, "{f}: E {e:e} H {h:e}");
    }
    // Pointwise: the field at the guide's centre, inside the empty section.
    let f = 8.0e9;
    let r = d.solve(f).unwrap();
    let ex = line(&FILTER).solve(f).unwrap();
    let p = emcraft_model::Point3::new(A / 2.0, B / 2.0, 0.025);
    let e = d.field(&r.fields[0], f, Field::E, p).unwrap();
    let e0 = ex.e(p);
    // E_y to 0.2 %; the spurious transverse components to 2.5 % of it.
    let scale = e0[1].norm();
    assert!((e[1] - e0[1]).norm() < 2e-3 * scale && e[0].norm() < 2.5e-2 * scale && e[2].norm() < 2.5e-2 * scale, "{e:?} {e0:?}");
}

#[test]
fn port_modes_beyond_te10_change_nothing_here() {
    // Nothing but TE10 reaches the ports of a guide with one cross-section, so carrying more
    // modes in the port condition leaves the result as it was (to the mesh's asymmetry).
    let one = Driven::new(&guide(&FILTER, 0.004, 1), Order::Two).unwrap().solve(7.0e9).unwrap();
    let many = Driven::new(&guide(&FILTER, 0.004, 8), Order::Two).unwrap().solve(7.0e9).unwrap();
    assert!((one.s[1][0] - many.s[1][0]).norm() < 1e-4, "{} {}", one.s[1][0], many.s[1][0]);
}

#[test]
fn hostile_requests_are_refused() {
    let d = Driven::new(&guide(&FILTER, 0.004, 2), Order::One).unwrap();
    for f in [f64::NAN, -1.0, 0.0, f64::INFINITY] {
        assert!(matches!(d.solve(f), Err(SolveError::Invalid { .. })), "{f}");
    }
    // Below the ports' TE10 cut-off (4.64 GHz in the fill) the wave decays: S is read in
    // pseudo-waves (oracle `evanescent-guide`), not refused.
    let below = d.solve(4.0e9).unwrap();
    assert!(below.s[1][0].norm() < 1.0 && below.s[1][0].norm().is_finite());
    let mut m = guide(&FILTER, 0.004, 2);
    m.mesh = None;
    assert!(matches!(Driven::new(&m, Order::One), Err(SolveError::NotReady(_))));
    m.ports.clear();
    assert!(matches!(Driven::new(&m, Order::One), Err(SolveError::NotReady(_))));
    let _ = C::new(0.0, 0.0);
}

/// The observed order of convergence between two mesh sizes.
fn rate(e_coarse: f64, e_fine: f64, h_coarse: f64, h_fine: f64) -> f64 {
    (e_coarse / e_fine).ln() / (h_coarse / h_fine).ln()
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn order_one_converges_at_its_rates() {
    // Whitney elements: E in L² at O(h), S-parameters (a functional) at O(h²). Sizes that
    // divide every section evenly.
    let (hc, hf) = (0.005, 0.0025);
    let dc = Driven::new(&guide(&FILTER, hc, 4), Order::One).unwrap();
    let df = Driven::new(&guide(&FILTER, hf, 4), Order::One).unwrap();
    let s = rate(worst_s_error(&dc, &FILTER, &BAND), worst_s_error(&df, &FILTER, &BAND), hc, hf);
    let (ec, _) = field_errors(&dc, &FILTER, 8.0e9);
    let (ef, _) = field_errors(&df, &FILTER, 8.0e9);
    let e = rate(ec, ef, hc, hf);
    eprintln!("order 1: S rate {s:.2}, E rate {e:.2}");
    assert!(s > 1.8 && e > 0.9, "S {s} E {e}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn order_two_converges_at_its_rates() {
    // Second-order Nédélec elements: E in L² at O(h²), S-parameters at O(h⁴).
    let (hc, hf) = (0.005, 0.0025);
    let dc = Driven::new(&guide(&FILTER, hc, 4), Order::Two).unwrap();
    let df = Driven::new(&guide(&FILTER, hf, 4), Order::Two).unwrap();
    let s = rate(worst_s_error(&dc, &FILTER, &BAND), worst_s_error(&df, &FILTER, &BAND), hc, hf);
    let (ec, _) = field_errors(&dc, &FILTER, 8.0e9);
    let (ef, _) = field_errors(&df, &FILTER, 8.0e9);
    let e = rate(ec, ef, hc, hf);
    eprintln!("order 2: S rate {s:.2}, E rate {e:.2}");
    assert!(s > 3.5 && e > 1.8, "S {s} E {e}");
}

//! The radius bends, E-plane and magic tees and the cross-guide coupler of the templates.
//!
//! No published reference gives these components' S-parameters to a tolerance an oracle case
//! could hold them to (Marcuvitz's equivalent circuits for E-plane tees and the small-hole
//! theory of cross-guide couplers are approximations of a few per cent at best, and radius bends
//! have no closed form), so they are checked here by what every lossless reciprocal junction
//! obeys — S unitary and symmetric, to rounding — and by what their symmetry makes exact (the
//! tees' equal splits, the magic tee's isolated arms), within tolerances for the unstructured
//! mesh, which is not itself symmetric.

use emcraft_solver::Complex64 as C;
use serde_json::{Value, json};

use crate::Session;

/// S_pq at point `i` of the last sweep, complex.
fn s(r: &Value, i: usize, pq: &str) -> C {
    let e = &r["points"][i][pq];
    C::from_polar(e["mag"].as_f64().unwrap(), e["deg"].as_f64().unwrap().to_radians())
}

/// The template `name` with `values`, meshed at `size` (mm) and solved at `frequencies` (GHz).
fn solved(name: &str, values: Value, size: f64, frequencies: &[f64]) -> Value {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": name, "values": values})).unwrap();
    s.execute("mesh.generate", &json!({"size": size})).unwrap();
    s.execute("solve.frequency", &json!({"frequencies": frequencies, "order": 2})).unwrap()
}

/// S unitary (each column of unit power, columns orthogonal) and symmetric, at point `i` of an
/// `n`-port sweep, to `tol`.
fn lossless_and_reciprocal(r: &Value, i: usize, n: usize, tol: f64) {
    for p in 1..=n {
        for q in 1..=n {
            let d = s(r, i, &format!("s{p}{q}")) - s(r, i, &format!("s{q}{p}"));
            assert!(d.norm() < tol, "reciprocal: s{p}{q} {d}");
            let dot: C = (1..=n).map(|k| s(r, i, &format!("s{k}{p}")).conj() * s(r, i, &format!("s{k}{q}"))).sum();
            let want = if p == q { 1.0 } else { 0.0 };
            assert!((dot - want).norm() < tol, "unitary: columns {p}, {q}: {dot}");
        }
    }
}

/// The phase of `z` relative to `w`, in degrees within (−180, 180].
fn phase(z: C, w: C) -> f64 {
    (z / w).arg().to_degrees()
}

#[test]
fn radius_bends_are_lossless_reciprocal_and_matched() {
    for name in ["h-bend-radius", "e-bend-radius"] {
        let r = solved(name, json!({}), 5.0, &[10.0]);
        lossless_and_reciprocal(&r, 0, 2, 1e-6);
        // A mean radius of one guide wavelength: a smooth bend reflects little (|S11| about 0.002
        // at 10 GHz on the template's mesh).
        assert!(s(&r, 0, "s11").norm() < 0.02, "{name}: {}", s(&r, 0, "s11"));
    }
}

#[test]
fn the_e_plane_tee_splits_equally_in_antiphase() {
    let r = solved("e-tee", json!({}), 5.0, &[10.0]);
    lossless_and_reciprocal(&r, 0, 3, 1e-6);
    // The side arm's TE10 (E across its narrow side, along x) is odd under the mirror across
    // its middle plane x = l + b/2, so it feeds the two ends equally and in antiphase.
    let (s13, s23) = (s(&r, 0, "s13"), s(&r, 0, "s23"));
    assert!((s13.norm() - s23.norm()).abs() < 5e-3, "{s13} {s23}");
    assert!((phase(s13, s23).abs() - 180.0).abs() < 1.0, "{}", phase(s13, s23));
    assert!((s(&r, 0, "s11").norm() - s(&r, 0, "s22").norm()).abs() < 5e-3);
}

/// The magic tee's checks at point `i`: lossless and reciprocal, the E and H arms (4 and 3)
/// isolated, each splitting equally between the collinear arms 1 and 2 — in phase from the H
/// arm, in antiphase from the E arm — as its mirror plane x = l + a/2 makes exact.
fn magic_tee_ideal(r: &Value, i: usize, tol: f64) {
    lossless_and_reciprocal(r, i, 4, 1e-6);
    assert!(s(r, i, "s34").norm() < tol, "isolation: {}", s(r, i, "s34"));
    let (s13, s23, s14, s24) = (s(r, i, "s13"), s(r, i, "s23"), s(r, i, "s14"), s(r, i, "s24"));
    assert!((s13.norm() - s23.norm()).abs() < tol && phase(s13, s23).abs() < 1.0, "H arm: {s13} {s23}");
    assert!((s14.norm() - s24.norm()).abs() < tol && (phase(s14, s24).abs() - 180.0).abs() < 1.0, "E arm: {s14} {s24}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_magic_tee_is_matched_at_its_design_frequency() {
    // The template's own mesh at 9.5 GHz, its design frequency: the arms isolated and split
    // equally by symmetry (to the unstructured mesh's own asymmetry, about 1e-3), and both
    // matched by their posts (|S33|, |S44| about 0.03), so each feeds half its power to each
    // collinear arm. (Its posts' fine mesh makes this a heavy test; the E-plane tee and the
    // bends stand for the junctions in the gate.)
    let mut session = Session::new();
    session.execute("file.sample", &json!({"name": "magic-tee"})).unwrap();
    let r = session.execute("solve.frequency", &json!({"frequencies": [9.5], "order": 2})).unwrap();
    magic_tee_ideal(&r, 0, 5e-3);
    for k in ["s33", "s44"] {
        assert!(s(&r, 0, k).norm() < 0.1, "{k}: {}", s(&r, 0, k));
    }
    for k in ["s13", "s14"] {
        assert!((s(&r, 0, k).norm().powi(2) - 0.5).abs() < 0.02, "{k}: {}", s(&r, 0, k));
    }
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_cross_guide_coupler_couples_one_way() {
    let mut session = Session::new();
    session.execute("file.sample", &json!({"name": "cross-guide-coupler"})).unwrap();
    let r = session.execute("solve.frequency", &json!({"frequencies": [10], "order": 2})).unwrap();
    lossless_and_reciprocal(&r, 0, 4, 1e-6);
    // About 30 dB coupled to port 3, the other end of the crossing guide isolated by about
    // 12 dB more (its directivity); the through arm and its mirror alike.
    let db = |k: &str| 20.0 * s(&r, 0, k).norm().log10();
    assert!((-34.0..-27.0).contains(&db("s31")), "{}", db("s31"));
    assert!(db("s31") - db("s41") > 8.0, "directivity: {} {}", db("s31"), db("s41"));
    assert!(s(&r, 0, "s11").norm() < 0.02);
    assert!((s(&r, 0, "s21") - s(&r, 0, "s43")).norm() < 2e-3, "{} {}", s(&r, 0, "s21"), s(&r, 0, "s43"));
}

//! Rotational symmetry: a pillbox meshed on a quarter gives its modes per harmonic
//! (model.rotation, solve.cyclic) against the closed forms; what a sector cannot carry yet is
//! refused.

use serde_json::{Value, json};

use crate::Session;

/// A vacuum pillbox, radius 10 mm and 15 mm long, about the z axis.
fn pillbox() -> Session {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 15})).unwrap();
    s
}

/// The pillbox's TM/TE (x = the Bessel root, p) frequency in GHz.
fn exact(x: f64, p: f64) -> f64 {
    let k = ((x / 0.01f64).powi(2) + (p * std::f64::consts::PI / 0.015).powi(2)).sqrt();
    k * emcraft_model::units::C0 / (2.0 * std::f64::consts::PI) / 1e9
}

fn modes(reply: &Value, m: u64) -> Vec<f64> {
    let row = reply["harmonics"].as_array().unwrap().iter().find(|h| h["m"] == m).unwrap();
    row["modes"].as_array().unwrap().iter().map(|f| f.as_f64().unwrap()).collect()
}

/// The pillbox on a quarter at `size` mm, element `order`, solved per harmonic `m` (all when
/// none): the reply.
fn quarter(size: f64, order: u8, m: Option<u32>) -> Value {
    let mut s = pillbox();
    s.execute("model.rotation", &json!({"n": 4})).unwrap();
    s.execute("mesh.generate", &json!({"size": size})).unwrap();
    let mut p = json!({"frequency": 19, "count": 2, "order": order});
    if let Some(m) = m {
        p["m"] = json!(m);
    }
    let r = s.execute("solve.cyclic", &p).unwrap();
    // The model keeps its own harmonic.
    assert_eq!(s.model.rotation.map(|r| r.m), Some(0));
    r
}

/// The reply's modes of harmonic 0 (TM010, TM011) and 1 (TE111, TM110) within `tol` of the
/// closed forms.
fn close(r: &Value, harmonics: &[u64], tol: f64) {
    let x01 = 2.404_825_557_695_773;
    let want = [(0, [exact(x01, 0.0), exact(x01, 1.0)]), (1, [exact(1.841_183_781_340_659, 1.0), exact(3.831_705_970_207_512, 0.0)])];
    for (m, f) in want.into_iter().filter(|(m, _)| harmonics.contains(m)) {
        let got = modes(r, m);
        assert_eq!(got.len(), 2);
        for (g, e) in got.iter().zip(f) {
            assert!((g / e - 1.0).abs() < tol, "m {m}: {g} GHz, exact {e} GHz");
        }
    }
}

/// **Setup**: a vacuum pillbox, radius a = 10 mm, length d = 15 mm, perfectly conducting, on a
/// quarter (C4), harmonic m = 1, first-order elements of 4 mm.
/// **Closed form**: f = c/(2π) √((x/a)² + (pπ/d)²); TE111 with x = x′₁₁ = 1.841184 (p = 1),
/// TM110 with x = x₁₁ = 3.831706 (p = 0) (D. M. Pozar, *Microwave Engineering*, 4th ed., §6.3).
/// **Reference**: the Bessel zeros from Abramowitz–Stegun Table 9.5; `exact` evaluates the
/// formula apart from the solver.
#[test]
fn a_pillbox_on_a_quarter_gives_its_modes() {
    let mut s = pillbox();
    let set = s.execute("model.rotation", &json!({"n": 4})).unwrap();
    assert_eq!((set["n"].as_u64(), set["m"].as_u64()), (Some(4), Some(0)));
    assert_eq!(set["centre"], json!([0.0, 0.0]));
    assert_eq!(s.execute("project.values", &json!({"command": "model.rotation"})).unwrap()["n"], 4);
    s.execute("model.rotation", &json!({"clear": true})).unwrap();
    assert!(s.model.rotation.is_none());
    close(&quarter(4.0, 1, Some(1)), &[1], 0.03);
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn a_pillbox_on_a_quarter_gives_its_modes_per_harmonic() {
    let r = quarter(3.0, 2, None);
    // Without `m`: the harmonics 0 to n/2.
    let ms: Vec<u64> = r["harmonics"].as_array().unwrap().iter().map(|h| h["m"].as_u64().unwrap()).collect();
    assert_eq!(ms, [0, 1, 2]);
    close(&r, &[0, 1], 1e-3);
}

#[test]
fn what_a_sector_cannot_carry_is_refused() {
    let mut s = pillbox();
    for bad in [
        json!({}),
        json!({"n": 2}),
        json!({"n": 65}),
        json!({"n": 4, "m": 4}),
        json!({"n": 4, "centre": [1, 0]}),
        json!({"n": 4, "centre": [0]}),
        json!({"clear": true, "n": 4}),
    ] {
        assert!(s.execute("model.rotation", &bad).is_err(), "{bad}");
        assert!(s.model.rotation.is_none(), "{bad}");
    }
    assert!(s.execute("solve.cyclic", &json!({"frequency": 10})).is_err());
    // A box square across z turns onto itself by a quarter only.
    let mut b = Session::new();
    b.execute("geometry.solid", &json!({"shape": "box", "size": [10, 12, 5]})).unwrap();
    assert!(b.execute("model.rotation", &json!({"n": 4})).is_err());
    b.execute("geometry.solid", &json!({"shape": "box", "size": [10, 10, 5]})).unwrap();
    assert!(b.execute("model.rotation", &json!({"n": 6})).is_err());
    b.execute("model.rotation", &json!({"n": 4})).unwrap();
    // A lumped port is not carried by a sector yet.
    let mut l = pillbox();
    l.execute("model.port.lumped", &json!({"name": "L1", "from": [5, 0, 0], "to": [5, 0, 1], "impedance": 50})).unwrap();
    assert!(l.execute("model.rotation", &json!({"n": 4})).is_err());
    // A port on a sector must be a circle on the axis: a square box's end is not.
    b.execute("model.port.create", &json!({"name": "1", "end": "start", "modes": 1})).unwrap();
    b.execute("mesh.generate", &json!({"size": 5})).unwrap();
    let e = b.execute("solve.frequency", &json!({"frequencies": [20], "order": 1})).unwrap_err();
    assert!(e.to_string().contains("circle centred on the axis"), "{e}");
    // On a sector the modes are per harmonic.
    s.execute("model.rotation", &json!({"n": 4})).unwrap();
    s.execute("mesh.generate", &json!({"size": 5})).unwrap();
    let e = s.execute("solve.eigenmodes", &json!({"near": 12})).unwrap_err();
    assert!(e.to_string().contains("solve.cyclic"), "{e}");
    // Another harmonic keeps the sector's mesh; another sector drops it.
    s.execute("model.rotation", &json!({"n": 4, "m": 1})).unwrap();
    assert!(s.model.mesh.is_some());
    s.execute("model.rotation", &json!({"n": 5})).unwrap();
    assert!(s.model.mesh.is_none());
}

#[test]
fn the_rotation_a_model_has_is_found() {
    // Round: any number of sectors, a quarter by default.
    let mut s = pillbox();
    let r = s.execute("model.rotation.find", &json!({})).unwrap();
    assert_eq!((r["group"].as_str(), r["n"].as_u64()), (Some("C∞"), Some(4)));
    s.execute("model.rotation.find", &json!({"n": 7, "m": 2})).unwrap();
    assert_eq!(s.model.rotation.map(|r| (r.n, r.m)), Some((7, 2)));
    // A square box: quarters about its middle.
    let mut b = Session::new();
    b.execute("geometry.solid", &json!({"shape": "box", "size": [10, 10, 5]})).unwrap();
    let r = b.execute("model.rotation.find", &json!({})).unwrap();
    assert_eq!((r["group"].as_str(), r["n"].as_u64(), r["centre"].clone()), (Some("C4"), Some(4), json!([5.0, 5.0])));
    b.execute("geometry.solid", &json!({"shape": "box", "size": [10, 12, 5]})).unwrap();
    // The rotation set before no longer holds: the model says so, and none is found.
    assert!(b.model.rotation_issue().is_some());
    assert!(b.execute("model.rotation.find", &json!({})).is_err());
    // A hexagonal prism off the origin: six sectors about its centre, or three.
    let hexagon: Vec<[f64; 2]> =
        (0..6).map(|k| f64::from(k) * std::f64::consts::FRAC_PI_3).map(|a| [3.0 + 8.0 * a.cos(), -2.0 + 8.0 * a.sin()]).collect();
    let mut h = Session::new();
    h.execute("geometry.solid", &json!({"shape": "prism", "points": hexagon, "length": 6})).unwrap();
    let r = h.execute("model.rotation.find", &json!({})).unwrap();
    assert_eq!((r["group"].as_str(), r["n"].as_u64()), (Some("C6"), Some(6)));
    let c: Vec<f64> = r["centre"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!((c[0] - 3.0).abs() < 1e-9 && (c[1] + 2.0).abs() < 1e-9, "{c:?}");
    h.execute("model.rotation.find", &json!({"n": 3})).unwrap();
    assert!(h.execute("model.rotation.find", &json!({"n": 4})).is_err());
    assert_eq!(h.model.rotation.map(|r| r.n), Some(3));
    // What a sector cannot carry is named.
    let mut f = pillbox();
    f.execute("geometry.inclusion", &json!({"shape": "sphere", "radius": 2, "at": [0, 0, 7]})).unwrap();
    let e = f.execute("model.rotation.find", &json!({})).unwrap_err();
    assert!(e.to_string().contains("across the sector"), "{e}");
}

/// A straight circular guide, radius 10 mm and 30 mm long, ports on both ends, on a quarter.
fn guide_quarter(m: u32, size: f64) -> Session {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 30})).unwrap();
    s.execute("model.port.create", &json!({"name": "1", "end": "start", "modes": 2})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end", "modes": 2})).unwrap();
    s.execute("model.rotation", &json!({"n": 4, "m": m})).unwrap();
    s.execute("mesh.generate", &json!({"size": size})).unwrap();
    s
}

/// The quarter in harmonic `m` at `f` GHz passes its first rotating mode (Bessel root `x` of
/// its cut-off) as e^{−jβL} within `tol`.
fn passes(m: u32, size: f64, f: f64, x: f64, tol: f64) {
    let mut s = guide_quarter(m, size);
    let r = s.execute("solve.frequency", &json!({"frequencies": [f], "order": 2})).unwrap();
    let (s11, s21) = (super::s(&r, 0, "s11"), super::s(&r, 0, "s21"));
    let k = 2.0 * std::f64::consts::PI * f * 1e9 / emcraft_model::units::C0;
    let beta = (k * k - (x / 0.01).powi(2)).sqrt();
    let want = emcraft_solver::Complex64::from_polar(1.0, -beta * 0.03);
    eprintln!("m {m}: S11 {:.2e}, |S21 − exact| {:.2e}", s11.norm(), (s21 - want).norm());
    assert!(s11.norm() < tol && (s21 - want).norm() < tol, "m {m}: S11 {s11}, S21 {s21}, exact {want}");
}

/// **Setup**: an empty circular guide, radius 10 mm, 30 mm long, on a quarter; ports on its ends
/// carrying the rotating modes TE11+ (12 GHz) and TM01 (14 GHz), 4 mm elements.
/// **Closed form**: a matched straight guide: S11 = 0, S21 = e^{−jβL}, β = √(k₀² − (x/a)²),
/// x = x′₁₁ or x₀₁ (Pozar §3.4).
/// **Reference**: the zeros from Abramowitz–Stegun Table 9.5, β evaluated in the test.
#[test]
fn a_circular_guide_on_a_quarter_passes_its_rotating_modes() {
    // TE11+ (ν = 1) and TM01 (ν = 0).
    passes(1, 4.0, 12.0, X11P, 0.01);
    passes(0, 4.0, 14.0, X01, 0.01);
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn a_quarter_s_rotating_modes_converge() {
    // TE21+ (ν = 2, with TE21− beside it): 3.0e-2, 4.5e-3, 8.9e-4 at 4, 3, 2 mm.
    passes(2, 3.0, 17.0, X21P, 0.01);
    passes(2, 2.0, 17.0, X21P, 2e-3);
}

/// The Bessel roots of TE11, TM01 and TE21's cut-offs (Abramowitz–Stegun, Table 9.5).
const X11P: f64 = 1.841_183_781_340_659;
const X01: f64 = 2.404_825_557_695_773;
const X21P: f64 = 3.054_236_928_227_14;

#[test]
fn fields_beyond_the_sector_are_its_turns_phased() {
    let mut s = guide_quarter(1, 4.0);
    s.execute("solve.frequency", &json!({"frequencies": [12], "order": 1})).unwrap();
    // A point in the sector and its turns by a quarter, a half and three quarters.
    let r = s.execute("results.probe", &json!({"frequency": 12, "at": [[4, 1, 15], [-1, 4, 15], [-4, -1, 15], [1, -4, 15]]})).unwrap();
    let e: Vec<[emcraft_solver::Complex64; 3]> = r["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| std::array::from_fn(|k| emcraft_solver::Complex64::new(p["e"][k][0].as_f64().unwrap(), p["e"][k][1].as_f64().unwrap())))
        .collect();
    let j = emcraft_solver::Complex64::new(0.0, 1.0);
    for k in 1..4 {
        // E(R r) = e^{−jπ/2} R E(r), once per quarter.
        let mut want = e[0];
        for _ in 0..k {
            want = [-want[1] * -j, want[0] * -j, want[2] * -j];
        }
        for c in 0..3 {
            assert!((e[k][c] - want[c]).norm() <= 1e-9 * e[0][0].norm().max(1.0), "turn {k}, component {c}: {} against {}", e[k][c], want[c]);
        }
    }
    assert!(e[0][0].norm() > 0.0);
    // The rotating mode's power: all of it through.
    let p = s.execute("results.power", &json!({"frequency": 12})).unwrap();
    assert!(p["reflected"].as_f64().unwrap() < 1e-3, "{p}");
}

#[test]
fn features_must_turn_onto_features() {
    let post = |s: &mut Session, x: f64, y: f64| {
        s.execute("geometry.feature", &json!({"combine": "subtract", "shape": "cylinder", "radius": 1, "length": 6, "at": [x, y, 12], "axis": "z"}))
            .unwrap();
    };
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 30})).unwrap();
    // One post off the axis: no turn maps it onto itself.
    post(&mut s, 5.0, 0.0);
    let e = s.execute("model.rotation", &json!({"n": 4})).unwrap_err();
    assert!(e.to_string().contains("feature 1 does not turn"), "{e}");
    // Four round it: a quarter turn, not an eighth.
    for (x, y) in [(0.0, 5.0), (-5.0, 0.0), (0.0, -5.0)] {
        post(&mut s, x, y);
    }
    assert!(s.execute("model.rotation", &json!({"n": 8})).is_err());
    let r = s.execute("model.rotation.find", &json!({})).unwrap();
    assert_eq!((r["n"].as_u64(), r["group"].as_str()), (Some(4), Some("C4")), "{r}");
    // A ring of the guide widened about the axis turns onto itself by any angle.
    let mut w = Session::new();
    w.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 30})).unwrap();
    w.execute("geometry.feature", &json!({"combine": "add", "shape": "cylinder", "radius": 14, "length": 6, "at": [0, 0, 12], "axis": "z"})).unwrap();
    w.execute("model.rotation", &json!({"n": 7})).unwrap();
    assert_eq!(w.execute("model.rotation.find", &json!({})).unwrap()["group"], "C∞");
}

/// A circular guide widened to 14 mm over 6 mm of its middle (a feature about the axis), its
/// ports on the ends: solved whole, or on a quarter in harmonic 1 (`quarter`).
fn stepped(quarter: bool, size: f64) -> Value {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 30})).unwrap();
    s.execute("geometry.feature", &json!({"combine": "add", "shape": "cylinder", "radius": 14, "length": 6, "at": [0, 0, 12], "axis": "z"})).unwrap();
    s.execute("model.port.create", &json!({"name": "1", "end": "start", "modes": 2})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end", "modes": 2})).unwrap();
    if quarter {
        s.execute("model.rotation", &json!({"n": 4, "m": 1})).unwrap();
    }
    s.execute("mesh.generate", &json!({"size": size})).unwrap();
    s.execute("solve.frequency", &json!({"frequencies": [11, 13], "order": 2})).unwrap()
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn a_widened_guide_on_a_quarter_gives_the_whole_s_s() {
    // The rotating TE11+ and the whole's standing TE11 see the same step (it is round): S alike,
    // within what the two meshes differ by (both converge to within 0.5 % of each other).
    let (w, q) = (stepped(false, 3.0), stepped(true, 3.0));
    for i in 0..2 {
        for pq in ["s11", "s21"] {
            let (a, b) = (super::s(&w, i, pq), super::s(&q, i, pq));
            assert!((a - b).norm() < 0.02, "{pq} at point {i}: whole {a}, quarter {b}");
        }
    }
    let unknowns = |r: &Value| r["unknowns"].as_f64().unwrap();
    assert!(unknowns(&q) < 0.8 * unknowns(&w), "quarter {} of whole {} unknowns", unknowns(&q), unknowns(&w));
}

#[test]
fn the_harmonics_assemble_the_whole_guide_s_standing_modes() {
    let mut s = guide_quarter(0, 4.0);
    let r = s.execute("solve.harmonics", &json!({"frequencies": [12], "order": 2})).unwrap();
    // At 12 GHz harmonic 0's TM01 (cut-off 11.47 GHz) and 2's TE21 (14.6 GHz): TM01 passes, TE21 not.
    let rows = r["harmonics"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[1]["modes"], json!(["TE11+", "TE11+"]));
    assert_eq!(rows[3]["modes"], json!(["TE11−", "TE11−"]));
    assert_eq!(rows[2]["points"][0]["propagating"], false);
    // TE11's standing polarizations: each passes as e^{−jβL}, neither turns into the other.
    let st = &r["standing"];
    assert_eq!(st["mode"], "TE11");
    let c = |k: &str| {
        let e = &st["points"][0][k];
        emcraft_solver::Complex64::new(e["re"].as_f64().unwrap(), e["im"].as_f64().unwrap())
    };
    let k = 2.0 * std::f64::consts::PI * 12e9 / emcraft_model::units::C0;
    let beta = (k * k - (X11P / 0.01).powi(2)).sqrt();
    let want = emcraft_solver::Complex64::from_polar(1.0, -beta * 0.03);
    for key in ["s2c1c", "s2s1s"] {
        assert!((c(key) - want).norm() < 0.01, "{key} {} against {want}", c(key));
    }
    for key in ["s2s1c", "s2c1s", "s1s1c", "s1c1c"] {
        assert!(c(key).norm() < 0.01, "{key} {}", c(key));
    }
}

/// `solve.revolution`'s modes (GHz) of the model's own profile.
fn revolution_modes(s: &mut Session, m: u32, near: f64, count: usize, size: f64) -> Vec<f64> {
    let r = s.execute("solve.revolution", &json!({"frequency": near, "m": m, "count": count, "size": size})).unwrap();
    r["modes"].as_array().unwrap().iter().map(|x| x["frequency"].as_f64().unwrap()).collect()
}

/// **Setup**: the pillbox (a = 10 mm, d = 15 mm) as a body of revolution from its cylinder, m = 0,
/// 0.5 mm triangles; a vacuum sphere of radius 10 mm, m = 1.
/// **Closed form**: TM010 and TM011 as in [`a_pillbox_on_a_quarter_gives_its_modes`]; the sphere's
/// TM101 at k a = 2.743707, the first zero of [x j₁(x)]′ (R. F. Harrington, *Time-Harmonic
/// Electromagnetic Fields*, 1961, §6.10).
/// **Reference**: `exact` and `emcraft_theory::bessel::sphere_cavity_resonances`, both written
/// apart from the solver (the latter is the 3D sphere cavity's reference too).
#[test]
fn a_body_of_revolution_takes_the_model_s_profile() {
    // The pillbox from its cylinder: TM010 and TM011.
    let mut s = pillbox();
    let f = revolution_modes(&mut s, 0, 11.0, 2, 0.5);
    let x01 = X01;
    for (got, want) in f.iter().zip([exact(x01, 0.0), exact(x01, 1.0)]) {
        assert!((got / want - 1.0).abs() < 2e-3, "{got} GHz against {want}");
    }
    // A sphere of radius 10 mm: its lowest modes (TM101, m = 0 among its orders).
    let mut b = Session::new();
    b.execute("geometry.solid", &json!({"shape": "sphere", "radius": 10})).unwrap();
    let f = revolution_modes(&mut b, 1, 12.0, 1, 0.5);
    let want = emcraft_theory::bessel::sphere_cavity_resonances(0.01, 1.0, 1)[0].0 / 1e9;
    assert!((f[0] / want - 1.0).abs() < 5e-3, "{} GHz against {want}", f[0]);
    // Features off the axis have no profile.
    let mut o = pillbox();
    o.execute("geometry.feature", &json!({"combine": "subtract", "shape": "cylinder", "radius": 1, "length": 6, "at": [5, 0, 4], "axis": "z"}))
        .unwrap();
    assert!(o.execute("solve.revolution", &json!({"frequency": 11})).is_err());
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn a_widened_cavity_s_profile_agrees_with_its_quarter() {
    // A 30 mm guide widened to 14 mm over 6 mm of its middle, closed: harmonic 1's lowest two
    // modes as a body of revolution (2D) and on a quarter in 3D.
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 30})).unwrap();
    s.execute("geometry.feature", &json!({"combine": "add", "shape": "cylinder", "radius": 14, "length": 6, "at": [0, 0, 12], "axis": "z"})).unwrap();
    let bor = revolution_modes(&mut s, 1, 12.0, 2, 0.5);
    s.execute("model.rotation", &json!({"n": 4, "m": 1})).unwrap();
    s.execute("mesh.generate", &json!({"size": 3})).unwrap();
    let r = s.execute("solve.cyclic", &json!({"frequency": 12, "m": 1, "count": 2, "order": 2})).unwrap();
    let quarter = modes(&r, 1);
    for (a, b) in bor.iter().zip(&quarter) {
        assert!((a / b - 1.0).abs() < 2e-3, "body of revolution {a} GHz, quarter {b} GHz");
    }
}

/// A circular guide with three layers of four posts round its axis, each layer turned 30° on (a
/// twist: C4, no mirror plane), its ports on the ends; solved whole or on a quarter.
fn twisted(quarter: bool) -> Session {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 30})).unwrap();
    for (z, a0) in [(8.0, 0.0), (13.0, 30.0), (18.0, 60.0)] {
        for k in 0..4 {
            let a = (a0 + 90.0 * f64::from(k)).to_radians();
            let at = [(5.5 * a.cos() * 1e9).round() / 1e9, (5.5 * a.sin() * 1e9).round() / 1e9, z];
            s.execute("geometry.feature", &json!({"combine": "subtract", "shape": "cylinder", "radius": 1.2, "length": 3, "at": at, "axis": "z"}))
                .unwrap();
        }
    }
    s.execute("model.port.create", &json!({"name": "1", "end": "start", "modes": 2})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end", "modes": 2})).unwrap();
    if quarter {
        s.execute("model.rotation", &json!({"n": 4, "m": 1})).unwrap();
    }
    s.execute("mesh.generate", &json!({"size": 3})).unwrap();
    s
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn posts_round_the_axis_on_a_quarter_give_the_whole_s_s() {
    // The whole's TE11 (its cos polarization) against the quarter's standing c from harmonics 1
    // and 3 (second order at 3 mm: S11 0.060 / 0.056, S21 0.9982∠54.41° / 0.9984∠54.63°; first
    // order here, within the test's memory), and the standing S unitary (what the twist turns
    // into the s polarization included).
    let w = twisted(false).execute("solve.frequency", &json!({"frequencies": [12], "order": 1})).unwrap();
    let q = twisted(true).execute("solve.harmonics", &json!({"frequencies": [12], "m": [1, 3], "order": 1})).unwrap();
    let st = |k: &str| {
        let e = &q["standing"]["points"][0][k];
        emcraft_solver::Complex64::new(e["re"].as_f64().unwrap(), e["im"].as_f64().unwrap())
    };
    for (pq, key) in [("s11", "s1c1c"), ("s21", "s2c1c")] {
        let (a, b) = (super::s(&w, 0, pq), st(key));
        assert!((a - b).norm() < 0.03, "{pq}: whole {a}, quarter {b}");
    }
    let power: f64 = ["s1c1c", "s1s1c", "s2c1c", "s2s1c"].iter().map(|k| st(k).norm_sqr()).sum();
    assert!((power - 1.0).abs() < 1e-6, "{power}");
}

#[test]
fn a_widened_guide_as_a_body_of_revolution_gives_its_s() {
    // The guide widened to 14 mm over 6 mm of its middle, its TE11 on the half-plane (m = 1)
    // against the whole in 3D (second order, 2.5 mm: |S11| 0.1316 at 11 GHz, 0.5713 at 13 GHz,
    // converging down by about 0.004 from 3.5 mm) and lossless.
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 30})).unwrap();
    s.execute("geometry.feature", &json!({"combine": "add", "shape": "cylinder", "radius": 14, "length": 6, "at": [0, 0, 12], "axis": "z"})).unwrap();
    s.execute("model.port.create", &json!({"name": "1", "end": "start", "modes": 2})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end", "modes": 2})).unwrap();
    let r = s.execute("solve.revolution.frequency", &json!({"frequencies": [11, 13], "m": 1, "size": 0.25})).unwrap();
    assert_eq!(r["modes"], json!(["TE11", "TE11"]));
    for (i, whole) in [0.1316, 0.5713].iter().enumerate() {
        let (s11, s21) = (super::s(&r, i, "s11"), super::s(&r, i, "s21"));
        eprintln!("point {i}: |S11| {:.4}, S21 {:.4}∠{:.2}°", s11.norm(), s21.norm(), s21.arg().to_degrees());
        assert!((s11.norm() - whole).abs() < 0.01, "point {i}: |S11| {} against the whole's {whole}", s11.norm());
        assert!((s11.norm_sqr() + s21.norm_sqr() - 1.0).abs() < 1e-6);
    }
}

/// **Setup**: a circular guide, radius 10 mm, 30 mm long, filled with εr = 2.2 over 5 mm from
/// z = 12.5 mm; TE11 (m = 1) at 11 and 13 GHz, 0.25 mm triangles.
/// **Closed form**: the slab between two lines of the TE11 wave impedance Z ∝ 1/β:
/// Γ = (β₁ − β₂)/(β₁ + β₂), P = e^{−jβ₂ℓ}, S11 = Γ(1 − P²)/(1 − Γ²P²), S21 = P(1 − Γ²)/(1 − Γ²P²),
/// moved to the ports by e^{−jβ₁·} (Pozar §2.6, §3.4; the guide's modes are the empty guide's in
/// both media, the slab filling the cross-section).
/// **Reference**: evaluated in the test from β₁, β₂.
#[test]
fn a_dielectric_plug_in_a_circular_guide_as_a_body_of_revolution() {
    // The guide filled with εr = 2.2 over 5 mm from z = 12.5 mm (to 10 µm of the wall, as an
    // inclusion must lie inside the solid): TE11 against the slab between
    // two lines of its wave impedance (Z = ωμ/β), reference planes at the ports.
    let mut s = Session::new();
    s.execute("model.material.create", &json!({"name": "ptfe", "eps_r": 2.2})).unwrap();
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 30})).unwrap();
    s.execute("geometry.inclusion", &json!({"shape": "cylinder", "radius": 9.99, "length": 5, "at": [0, 0, 12.5], "material": "ptfe"})).unwrap();
    s.execute("model.port.create", &json!({"name": "1", "end": "start", "modes": 2})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end", "modes": 2})).unwrap();
    let r = s.execute("solve.revolution.frequency", &json!({"frequencies": [11, 13], "m": 1, "size": 0.25})).unwrap();
    for (i, f) in [11e9, 13e9].iter().enumerate() {
        let k = 2.0 * std::f64::consts::PI * f / emcraft_model::units::C0;
        let kc = X11P / 0.01;
        let (b1, b2) = ((k * k - kc * kc).sqrt(), (2.2 * k * k - kc * kc).sqrt());
        let gamma = (b1 - b2) / (b1 + b2); // (Z2 − Z1)/(Z2 + Z1), Z ∝ 1/β
        let c = |x: f64| emcraft_solver::Complex64::from_polar(1.0, x);
        let p = c(-b2 * 0.005);
        let den = 1.0 - gamma * gamma * p * p;
        let s11 = gamma * (1.0 - p * p) / den * c(-2.0 * b1 * 0.0125);
        let s21 = p * (1.0 - gamma * gamma) / den * c(-b1 * 0.025);
        let (g11, g21) = (super::s(&r, i, "s11"), super::s(&r, i, "s21"));
        assert!((g11 - s11).norm() < 5e-3 && (g21 - s21).norm() < 5e-3, "{f} Hz: S11 {g11} against {s11}, S21 {g21} against {s21}");
    }
}

/// A pillbox with four dielectric posts (εr 4, radius 1 mm, `length` mm from z = 1 mm) round its
/// axis at 45° + k·90°.
fn posted_with(length: f64) -> Session {
    let mut s = pillbox();
    s.execute("model.material.create", &json!({"name": "ceramic", "eps_r": 4})).unwrap();
    for k in 0..4 {
        let a = (45.0 + 90.0 * f64::from(k)).to_radians();
        let at = [(5.0 * a.cos() * 1e9).round() / 1e9, (5.0 * a.sin() * 1e9).round() / 1e9, 1.0];
        s.execute("geometry.inclusion", &json!({"shape": "cylinder", "radius": 1, "length": length, "at": at, "material": "ceramic"})).unwrap();
    }
    s
}

fn posted() -> Session {
    posted_with(13.0)
}

#[test]
fn a_quarter_keeps_the_posts_inside_it() {
    // Four posts round the axis: the quarter meshes the one inside it (ceramic tetrahedra there).
    let mut q = posted_with(3.0);
    q.execute("model.rotation", &json!({"n": 4})).unwrap();
    q.execute("mesh.generate", &json!({"size": 5})).unwrap();
    let mesh = q.model.mesh.as_ref().unwrap();
    let ceramic = q.model.materials.iter().position(|m| m.name == "ceramic").unwrap();
    assert!(mesh.tet_material.iter().any(|&m| m as usize == ceramic));
    // A post across the cut faces is refused.
    let mut x = pillbox();
    x.execute("geometry.inclusion", &json!({"shape": "cylinder", "radius": 1, "length": 13, "at": [0, 0, 1]})).unwrap();
    let e = x.execute("model.rotation", &json!({"n": 4})).unwrap_err();
    assert!(e.to_string().contains("across the sector"), "{e}");
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn dielectric_posts_round_the_axis_on_a_quarter_give_the_whole_s_modes() {
    let mut whole = posted();
    whole.execute("mesh.generate", &json!({"size": 3})).unwrap();
    let w = whole.execute("solve.eigenmodes", &json!({"near": 13, "count": 6, "order": 1})).unwrap();
    let all: Vec<f64> = w["modes"].as_array().unwrap().iter().map(|m| m["frequency"].as_f64().unwrap()).collect();
    let mut q = posted();
    q.execute("model.rotation", &json!({"n": 4})).unwrap();
    q.execute("mesh.generate", &json!({"size": 3})).unwrap();
    let r = q.execute("solve.cyclic", &json!({"frequency": 13, "count": 2, "order": 1})).unwrap();
    for m in [0, 1, 2] {
        for f in modes(&r, m) {
            if f > all.last().copied().unwrap_or(0.0) {
                continue;
            }
            let near = all.iter().map(|g| (g / f - 1.0).abs()).fold(f64::INFINITY, f64::min);
            assert!(near < 5e-3, "harmonic {m}: {f} GHz, the whole's {all:?}");
        }
    }
}

#[test]
fn a_body_of_revolution_s_field_is_drawn() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 30})).unwrap();
    s.execute("model.port.create", &json!({"name": "1", "end": "start", "modes": 2})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end", "modes": 2})).unwrap();
    let dir = std::env::temp_dir().join(format!("emcraft-bor-field-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("field.png");
    let r = s.execute("view.revolution_field", &json!({"path": path.to_str().unwrap(), "frequency": 12, "m": 1, "size": 1})).unwrap();
    assert_eq!(r["mode"], "TE11");
    assert!(std::fs::metadata(&path).unwrap().len() > 1000);
    assert!(s.execute("view.revolution_field", &json!({"path": path.to_str().unwrap(), "frequency": 12, "port": "9"})).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The TE101 resonance (Hz) of a spherical cavity of radius `a` with a concentric dielectric
/// sphere of radius `b` and `eps_r` inside: E_φ ∝ R(r), R = j₁(k₁r) inside, R = j₁(k₀r) y₁(k₀a) −
/// y₁(k₀r) j₁(k₀a) outside (R(a) = 0), R and (rR)′ continuous at b (Harrington §6.10), the
/// root found by bisection between the filled and the empty cavity's.
fn layered_sphere_te101(a: f64, b: f64, eps_r: f64) -> f64 {
    let j1 = |x: f64| x.sin() / (x * x) - x.cos() / x;
    let y1 = |x: f64| -x.cos() / (x * x) - x.sin() / x;
    let d = |f: &dyn Fn(f64) -> f64, r: f64| {
        let h = 1e-7 * r;
        ((r + h) * f(r + h) - (r - h) * f(r - h)) / (2.0 * h)
    };
    let mismatch = |k0: f64| {
        let k1 = k0 * eps_r.sqrt();
        let inside = |r: f64| j1(k1 * r);
        let outside = |r: f64| j1(k0 * r) * y1(k0 * a) - y1(k0 * r) * j1(k0 * a);
        inside(b) * d(&outside, b) - outside(b) * d(&inside, b)
    };
    let x = 4.493_409_457_909_064;
    let (mut lo, mut hi) = (x / (a * eps_r.sqrt()) * 1.0001, x / a * 0.9999);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if (mismatch(lo) < 0.0) == (mismatch(mid) < 0.0) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi) * emcraft_model::units::C0 / (2.0 * std::f64::consts::PI)
}

/// **Setup**: a spherical cavity, radius a = 10 mm, with a concentric sphere of εr = 2.2 and
/// radius b = 5 mm; m = 0, 0.2 mm triangles.
/// **Closed form**: TE101, E_φ ∝ R(r): R = j₁(k₁r) inside, R = j₁(k₀r)y₁(k₀a) − y₁(k₀r)j₁(k₀a)
/// outside (R(a) = 0), R and (rR)′ continuous at b (Harrington §6.10).
/// **Reference**: [`layered_sphere_te101`]'s bisection, which an apart script with the
/// derivatives in closed form matches to 6e-10 and whose limits b → 0, b → a and εr = 1 give the
/// empty or filled cavity's root 4.493409 (tests/oracle/bor-dielectric-sphere).
#[test]
fn a_dielectric_sphere_in_a_spherical_cavity_as_a_body_of_revolution() {
    // εr = 2.2 to 5 mm in a 10 mm cavity: TE101 (m = 0, E_φ only) against the two-layer root.
    let want = layered_sphere_te101(0.01, 0.005, 2.2) / 1e9;
    let empty = 4.493_409_457_909_064 / 0.01 * emcraft_model::units::C0 / (2.0 * std::f64::consts::PI) / 1e9;
    assert!(want < 0.97 * empty, "the dielectric pulls TE101 down: {want} against {empty} GHz");
    let mut s = Session::new();
    s.execute("model.material.create", &json!({"name": "ptfe", "eps_r": 2.2})).unwrap();
    s.execute("geometry.solid", &json!({"shape": "sphere", "radius": 10})).unwrap();
    s.execute("geometry.inclusion", &json!({"shape": "sphere", "radius": 5, "at": [0, 0, 0], "material": "ptfe"})).unwrap();
    let f = revolution_modes(&mut s, 0, want, 4, 0.1);
    let got = f.iter().copied().min_by(|x, y| (x - want).abs().total_cmp(&(y - want).abs())).unwrap();
    assert!((got / want - 1.0).abs() < 3e-3, "{got} GHz against {want} ({f:?})");
}

/// **Setup**: a sphere of radius 10 mm as an imported surface (its meridian in 32 chords, turned in
/// 64 sides), m = 1, 0.5 mm triangles; a cube imported the same way, refused.
/// **Closed form**: TM101 at k a = 2.743707 (Harrington §6.10).
/// **Reference**: `emcraft_theory::bessel::sphere_cavity_resonances`.
#[test]
fn an_imported_body_of_revolution_takes_its_section_as_its_profile() {
    // A sphere of radius 10 mm as an imported surface (its meridian in 32 chords, turned in 64
    // sides): TM101 as the primitive sphere's.
    let meridian: Vec<[f64; 2]> = (0..=32)
        .map(|k| std::f64::consts::PI * k as f64 / 32.0)
        .map(|t| [if k_on_axis(t) { 0.0 } else { 0.01 * t.sin() }, -0.01 * t.cos()])
        .collect();
    let mut s = Session::new();
    let shape = emcraft_model::Shape::revolved("ball", &meridian, 64).unwrap();
    s.model.set_solid(emcraft_model::Solid { shape, material: "vacuum".into() }).unwrap();
    let f = revolution_modes(&mut s, 1, 12.0, 1, 0.5);
    let want = emcraft_theory::bessel::sphere_cavity_resonances(0.01, 1.0, 1)[0].0 / 1e9;
    assert!((f[0] / want - 1.0).abs() < 5e-3, "{} GHz against {want}", f[0]);
    // A box imported is no body of revolution.
    let cube = emcraft_model::Shape::revolved("cube", &[[0.0, 0.0], [0.01, 0.0], [0.01, 0.01], [0.0, 0.01]], 4).unwrap();
    s.model.set_solid(emcraft_model::Solid { shape: cube, material: "vacuum".into() }).unwrap();
    let e = s.execute("solve.revolution", &json!({"frequency": 12})).unwrap_err().to_string();
    assert!(e.contains("not turned about the z axis"), "{e}");
}

/// Whether the meridian's angle `t` is one of its ends (on the axis).
fn k_on_axis(t: f64) -> bool {
    t == 0.0 || t == std::f64::consts::PI
}

/// `solve.revolution.radiation`'s reply.
fn revolution_radiation(s: &mut Session, theta: &[f64], size: f64) -> Value {
    s.execute("solve.revolution.radiation", &json!({"frequency": 3, "theta": theta, "size": size})).unwrap()
}

/// **Setup**: a current element I·l = 1 A × 1 mm along z at 3 GHz (λ = 100 mm) at the centre of
/// a vacuum sphere of radius 60 mm lined by a 20 mm spherical layer, and in a cylinder (radius
/// 60 mm, 120 mm long) lined on its wall and ends; m = 0, 3 mm triangles.
/// **Closed form**: P = η₀(k I l)²/(12π), directivity (3/2) sin²θ (C. A. Balanis, *Antenna
/// Theory*, 4th ed., §4.2), η₀ = μ₀c.
/// **Reference**: `emcraft_theory::radiation::dipole_power`, the formula as printed.
#[test]
fn a_dipole_on_the_axis_of_an_open_body_of_revolution_radiates_its_closed_form() {
    // A 1 mm current element at 3 GHz (λ = 100 mm): η (k I l)²/(12π) and directivity 3/2 at
    // θ = 90°, in a sphere's shell and in a cylinder's lined wall and ends.
    let want = emcraft_theory::radiation::dipole_power(1e-3, 3e9);
    for (solid, layer, at) in [
        (json!({"shape": "sphere", "radius": 60}), json!({"thickness": 20, "shape": "sphere"}), json!([0, 0, 0])),
        (json!({"shape": "cylinder", "radius": 60, "length": 120}), json!({"thickness": 20, "shape": "box"}), json!([0, 0, 60])),
    ] {
        let mut s = Session::new();
        s.execute("geometry.solid", &solid).unwrap();
        s.execute("model.open", &layer).unwrap();
        s.execute("source.dipole", &json!({"at": at, "direction": [0, 0, 1], "length": 1})).unwrap();
        let r = revolution_radiation(&mut s, &[0.0, 90.0], 3.0);
        let power = r["power"].as_f64().unwrap();
        assert!((power / want - 1.0).abs() < 0.03, "{solid}: {power} W against {want} W");
        let d = r["points"][1]["directivity"].as_f64().unwrap();
        assert!((d - 1.5).abs() < 0.04, "{solid}: directivity {d}");
        assert!(r["points"][0]["directivity"].as_f64().unwrap() < 0.02, "{solid}: no radiation along the axis");
    }
    // A dipole off the axis is no body of revolution's.
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "sphere", "radius": 60})).unwrap();
    s.execute("model.open", &json!({"thickness": 20, "shape": "sphere"})).unwrap();
    s.execute("source.dipole", &json!({"at": [5, 0, 0], "direction": [0, 0, 1], "length": 1})).unwrap();
    assert!(s.execute("solve.revolution.radiation", &json!({"frequency": 3})).is_err());
}

/// **Setup**: a sphere of εr = 4, radius 15 mm (k a = 0.94) at the centre of a vacuum sphere of
/// radius 60 mm lined by a 20 mm spherical layer, lit at 3 GHz by a plane wave along +z with E
/// along x (orders m = ±1), 2 mm triangles.
/// **Closed form**: the Mie series: σ = 4π|S₂(θ)|²/k² (E-plane), 4π|S₁(θ)|²/k² (H-plane), S₁, S₂
/// from aₙ, bₙ (C. F. Bohren, D. R. Huffman, *Absorption and Scattering of Light by Small
/// Particles*, 1983, §4.4).
/// **Reference**: `emcraft_theory::radiation::sphere_rcs`, which an apart script matches to
/// twelve digits; the script gives Bohren and Huffman's BHMIE example (m = 1.55, x = 5.2128:
/// Q_sca = Q_ext = 3.10543, Q_back = 2.92534) as published, the Rayleigh limit and the optical
/// theorem (tests/oracle/bor-mie-sphere).
#[test]
fn a_dielectric_sphere_scatters_a_plane_wave_as_the_mie_series_has_it() {
    // εr = 4, radius 15 mm at 3 GHz (k a = 0.94), the wave along +z with E along x: the bistatic
    // radar cross-section in the E- and H-planes against Bohren and Huffman's series.
    let mut s = Session::new();
    s.execute("model.material.create", &json!({"name": "ceramic", "eps_r": 4})).unwrap();
    s.execute("geometry.solid", &json!({"shape": "sphere", "radius": 60})).unwrap();
    s.execute("model.open", &json!({"thickness": 20, "shape": "sphere"})).unwrap();
    s.execute("geometry.inclusion", &json!({"shape": "sphere", "radius": 15, "at": [0, 0, 0], "material": "ceramic"})).unwrap();
    s.execute("source.plane_wave", &json!({"direction": [0, 0, 1], "polarization": [1, 0, 0]})).unwrap();
    let thetas = [0.0, 60.0, 120.0, 180.0];
    let r = revolution_radiation(&mut s, &thetas, 2.0);
    for (k, &deg) in thetas.iter().enumerate() {
        for (key, e_plane) in [("rcs_e_plane", true), ("rcs_h_plane", false)] {
            let got = r["points"][k][key].as_f64().unwrap() * 1e-6;
            let want = emcraft_theory::radiation::sphere_rcs(0.015, 4.0, 3e9, deg.to_radians(), e_plane);
            assert!((got / want - 1.0).abs() < 0.05, "θ = {deg}°, {key}: {got} m² against {want} m²");
        }
    }
}

/// An open-ended circular guide (radius 11 mm, wall 2 mm, 42 mm long, closed below its port) in a
/// cylinder of radius 35 mm, 110 mm long, lined by a 10 mm layer, fed in TE11 at 10 GHz.
fn open_guide(s: &mut Session) {
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 35, "length": 110})).unwrap();
    s.execute("geometry.feature", &json!({"combine": "subtract", "shape": "cylinder", "radius": 13, "length": 42, "at": [0, 0, 28]})).unwrap();
    s.execute("geometry.feature", &json!({"combine": "add", "shape": "cylinder", "radius": 11, "length": 42, "at": [0, 0, 30]})).unwrap();
    s.execute("model.port.create", &json!({"name": "feed", "end": "start", "modes": 2, "face": [30, -11, -11, 22, 22]})).unwrap();
    s.execute("model.open", &json!({"thickness": 10, "shape": "box"})).unwrap();
}

/// **Setup**: [`open_guide`], TE11 (m = 1 and its mirror: E along x) into the guide.
/// **Cross-check** (no closed form): the same guide in 3D in a 70 × 70 × 110 mm box lined by a
/// 10 mm layer (`solve.radiation`, order 2, 3.6 mm, 1.6 M unknowns, 165 s; this commit's 3D
/// solver): directivity 7.258 on the axis, 4.130 (E) and 4.161 (H) at 30°, 1.271 (E) and 1.049
/// (H) at 60°; the half-plane's within 1 % of each. Its |S11| (0.125 there) converges here: 0.1158,
/// 0.1075, 0.1062, 0.1059 at 2, 1, 0.5, 0.25 mm — the 3D mesh's elements are coarser than the wall.
#[test]
fn an_open_ended_guide_as_a_body_of_revolution_radiates_as_in_3d() {
    let mut s = Session::new();
    open_guide(&mut s);
    let r = s.execute("solve.revolution.radiation", &json!({"frequency": 10, "theta": [0, 30, 60], "size": 0.5})).unwrap();
    assert_eq!(r["modes"][0], "TE11");
    for (k, e, h) in [(0, 7.258, 7.258), (1, 4.130, 4.161), (2, 1.271, 1.049)] {
        let (de, dh) = (r["points"][k]["directivity"].as_f64().unwrap(), r["points"][k]["directivity_h"].as_f64().unwrap());
        assert!(
            (de / e - 1.0).abs() < 0.015 && (dh / h - 1.0).abs() < 0.015,
            "θ {}: E {de} against {e}, H {dh} against {h}",
            r["points"][k]["theta"]
        );
    }
    let s11 = r["s"]["s11"]["mag"].as_f64().unwrap();
    assert!((s11 - 0.1059).abs() < 0.002, "|S11| {s11}");
    // Lumped ports and sources with ports are refused.
    s.execute("source.dipole", &json!({"at": [0, 0, 80], "direction": [0, 0, 1], "length": 1})).unwrap();
    assert!(s.execute("solve.revolution.radiation", &json!({"frequency": 10})).is_err());
}

/// **Setup**: a conical horn — [`open_guide`]'s feed flared from radius 11 mm at z = 60 mm to
/// 25 mm at z = 100 mm (walls 2 mm), revolved profiles in a cylinder of radius 80 mm, 150 mm long,
/// lined by 15 mm — fed in TE11 at 10 GHz.
/// **Estimate** (no closed form): a circular aperture of diameter d = 50 mm with the TE11
/// distribution has the aperture efficiency 0.837 (S. Silver, *Microwave Antenna Theory and
/// Design*, 1949, §6.8; Balanis, *Antenna Theory*, Table 12.2): D ≈ 0.837 (πd/λ)² = 22.9 for a
/// flare short enough that its phase error (s = d²/(8λℓ) = 0.14) costs little. The solve gives
/// 23.0 at 1 mm and 22.9 at 0.5 mm, |S11| 0.060 and 0.059.
#[test]
fn a_conical_horn_as_a_body_of_revolution_has_its_aperture_s_directivity() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 80, "length": 150})).unwrap();
    s.execute("geometry.feature", &json!({"combine": "subtract", "shape": "revolved", "points": [[0, 38], [13, 38], [13, 60], [27, 100], [0, 100]]}))
        .unwrap();
    s.execute("geometry.feature", &json!({"combine": "add", "shape": "revolved", "points": [[0, 40], [11, 40], [11, 60], [25.7, 102], [0, 102]]}))
        .unwrap();
    s.execute("model.port.create", &json!({"name": "feed", "end": "start", "modes": 2, "face": [40, -11, -11, 22, 22]})).unwrap();
    s.execute("model.open", &json!({"thickness": 15, "shape": "box"})).unwrap();
    let r = s.execute("solve.revolution.radiation", &json!({"frequency": 10, "theta": [0], "size": 1})).unwrap();
    let lambda = emcraft_model::units::C0 / 1e10;
    let estimate = 0.837 * (std::f64::consts::PI * 0.05 / lambda).powi(2);
    let d = r["points"][0]["directivity"].as_f64().unwrap();
    assert!((d / estimate - 1.0).abs() < 0.05, "directivity {d} against {estimate}");
    assert!(r["s"]["s11"]["mag"].as_f64().unwrap() < 0.07);
}

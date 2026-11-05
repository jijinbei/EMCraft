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
    assert!(e.to_string().contains("not supported yet"), "{e}");
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
    let mut s = guide_quarter(1, 5.0);
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

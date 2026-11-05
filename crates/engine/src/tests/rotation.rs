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
    // On a sector the modes are per harmonic.
    s.execute("model.rotation", &json!({"n": 4})).unwrap();
    s.execute("mesh.generate", &json!({"size": 5})).unwrap();
    let e = s.execute("solve.eigenmodes", &json!({"near": 12})).unwrap_err();
    assert!(e.to_string().contains("solve.cyclic"), "{e}");
}

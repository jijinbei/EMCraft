//! Ports across x and y, prisms, and the bends and tees built of them.

use serde_json::{Value, json};

use crate::Session;

/// S_pq at point `i` of the last sweep: (magnitude, degrees).
fn entry(r: &Value, i: usize, pq: &str) -> (f64, f64) {
    let e = &r["points"][i][pq];
    (e["mag"].as_f64().unwrap(), e["deg"].as_f64().unwrap())
}

#[test]
fn ports_across_x_and_y_carry_a_straight_guide() {
    // WR-90, 30 mm along x and along y: S21 = e^{−jβL} as along z (88.01° at 10 GHz).
    let exact = 88.008;
    for (axis, size, far) in [("x", [30.0, 22.86, 10.16], [30.0, 0.0, 0.0]), ("y", [22.86, 30.0, 10.16], [30.0, 0.0, 0.0])] {
        let mut s = Session::new();
        s.execute("geometry.solid", &json!({"shape": "box", "size": size})).unwrap();
        s.execute("mesh.generate", &json!({"size": 4})).unwrap();
        s.execute("model.port.create", &json!({"name": "1", "end": "start", "axis": axis, "face": [0, 0, 0, 22.86, 10.16]})).unwrap();
        let face = json!([far[0], far[1], far[2], 22.86, 10.16]);
        s.execute("model.port.create", &json!({"name": "2", "end": "end", "axis": axis, "face": face})).unwrap();
        let r = s.execute("solve.frequency", &json!({"frequencies": [10], "order": 2})).unwrap();
        let (m21, p21) = entry(&r, 0, "s21");
        assert!((m21 - 1.0).abs() < 1e-4 && (p21 - exact).abs() < 0.1, "{axis}: {m21} {p21}");
        assert!(entry(&r, 0, "s11").0 < 2e-3, "{axis}");
        // The port's values give its axis back, and an edit keeps it.
        let v = s.execute("project.values", &json!({"command": "model.port.edit", "name": "2"})).unwrap();
        assert_eq!(v["axis"], axis);
        s.execute("model.port.edit", &json!({"name": "2", "modes": 2})).unwrap();
        assert_eq!(s.model.ports[1].face.map(|f| f.axis.id()), Some(axis));
    }
}

#[test]
fn the_cross_section_solver_works_on_a_face_across_y() {
    // Across y the port's axes are (x, z, −y): the numerical modes, turned back, carry the
    // guide as the closed forms do.
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [22.86, 30, 10.16]})).unwrap();
    s.execute("mesh.generate", &json!({"size": 4})).unwrap();
    for (name, end, at) in [("1", "start", 0.0), ("2", "end", 30.0)] {
        let p = json!({"name": name, "end": end, "axis": "y", "face": [at, 0, 0, 22.86, 10.16], "mode_solver": "numeric", "modes": 2});
        s.execute("model.port.create", &p).unwrap();
    }
    let modes = s.execute("solve.port_modes", &json!({"frequency": 10, "order": 2})).unwrap();
    assert!(modes.to_string().contains("mode 1"), "{modes}");
    let r = s.execute("solve.frequency", &json!({"frequencies": [10], "order": 2})).unwrap();
    let (m21, p21) = entry(&r, 0, "s21");
    assert!((m21 - 1.0).abs() < 1e-3 && (p21 - 88.008).abs() < 0.3, "{m21} {p21}");
}

#[test]
fn side_ports_and_prisms_refuse_what_they_cannot_be() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [30, 22.86, 10.16]})).unwrap();
    let before = s.summary();
    for bad in [
        json!({"name": "1", "end": "start", "axis": "x"}),
        json!({"name": "1", "end": "start", "axis": "w", "face": [0, 0, 0, 1, 1]}),
        json!({"name": "1", "end": "start", "axis": "x", "face": [0, 0, 0, 1]}),
        json!({"name": "1", "end": "start", "axis": "x", "face": [0, 0, 0, -1, 1]}),
    ] {
        assert!(s.execute("model.port.create", &bad).is_err(), "{bad}");
        assert_eq!(s.summary(), before);
    }
    for bad in [
        json!({"shape": "prism", "length": 5}),
        json!({"shape": "prism", "points": [[0, 0], [1, 0], [1, 1]]}),
        json!({"shape": "prism", "points": [[0, 0], [1, 1], [1, 0], [0, 1]], "length": 5}),
        json!({"shape": "prism", "points": [[0, 0], [1, 0], [2, 0]], "length": 5}),
        json!({"shape": "prism", "points": [[0, 0], [1, 0, 3], [1, 1]], "length": 5}),
        json!({"shape": "prism", "points": [[0, 0], ["=nothing", 0], [1, 1]], "length": 5}),
        json!({"shape": "prism", "points": vec![[0.0, 0.0]; 300], "length": 5}),
        json!({"shape": "prism", "points": [[0, 0], [1e308, 0], [0, 1e308]], "length": 5}),
    ] {
        assert!(s.execute("geometry.solid", &bad).is_err(), "{bad}");
        assert_eq!(s.summary(), before);
    }
}

#[test]
fn a_mitred_bend_is_the_same_turned_round() {
    // (Lossless and reciprocal, as every template: invariants.rs.)
    let (a, b, l, m) = (22.86, 10.16, 20.0, 14.6);
    let w = l + a;
    let mut s = super::template_meshed("h-bend", 4.0);
    let r = s.execute("solve.frequency", &json!({"frequencies": [9, 11], "order": 2})).unwrap();
    // The same bend turned a quarter round about z, (x, y) → (w − y, x): its ports swap axes.
    let turned: Vec<[f64; 2]> = [[0.0, 0.0], [a, 0.0], [a, l], [w, l], [w, w], [m, w], [0.0, w - m]].iter().map(|p| [w - p[1], p[0]]).collect();
    let mut t = Session::new();
    t.execute("geometry.solid", &json!({"shape": "prism", "points": turned, "length": b})).unwrap();
    t.execute("mesh.generate", &json!({"size": 4})).unwrap();
    t.execute("model.port.create", &json!({"name": "1", "end": "end", "axis": "x", "face": [w, 0, 0, a, b]})).unwrap();
    t.execute("model.port.create", &json!({"name": "2", "end": "end", "axis": "y", "face": [w, 0, 0, a, b]})).unwrap();
    let q = t.execute("solve.frequency", &json!({"frequencies": [9, 11], "order": 2})).unwrap();
    for i in 0..2 {
        let ((m11, p11), (m21, p21)) = (entry(&r, i, "s11"), entry(&r, i, "s21"));
        let ((n11, q11), (n21, q21)) = (entry(&q, i, "s11"), entry(&q, i, "s21"));
        assert!((m11 - n11).abs() < 0.01 && (m21 - n21).abs() < 0.01, "{m11} {n11} {m21} {n21}");
        // TE10 is even across its face and E stays along z, so the phases agree too.
        for (x, y) in [(p21, q21), (p11, q11)] {
            let d = (x - y).rem_euclid(360.0);
            assert!(d.min(360.0 - d) < 1.0, "{x} {y}");
        }
    }
}

#[test]
fn the_tee_splits_evenly() {
    // (Its power conserved, as every lossless template's: invariants.rs.)
    let mut s = super::template_meshed("h-tee", 4.0);
    let r = s.execute("solve.frequency", &json!({"frequencies": [10], "order": 2})).unwrap();
    let (m31, p31) = entry(&r, 0, "s31");
    let (m32, p32) = entry(&r, 0, "s32");
    assert!((m31 - m32).abs() < 5e-3 && (p31 - p32).abs() < 1.0, "the side arm sees both ends alike: {m31} {m32}");
}

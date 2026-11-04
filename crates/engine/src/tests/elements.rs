//! Lumped elements: the admittance they add to a gap is what the field sees.

use serde_json::{Value, json};

use crate::Session;

/// A 10 mm box (far below its resonances at 3 GHz) probed from floor to ceiling by a lumped
/// port, with `element` (its parameters) across the same gap when given: the port's impedance.
fn probe(element: Option<Value>) -> emcraft_model::Complex64 {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [10, 10, 10]})).unwrap();
    s.execute("model.port.lumped", &json!({"name": "p", "from": [5, 5, 0], "to": [5, 5, 10], "impedance": 50})).unwrap();
    if let Some(e) = element {
        s.execute("model.element.create", &e).unwrap();
    }
    s.execute("mesh.generate", &json!({"size": 2})).unwrap();
    let r = s.execute("solve.frequency", &json!({"frequencies": [3], "order": 1})).unwrap();
    let s11 = &r["points"][0]["s11"];
    let g = emcraft_model::Complex64::new(s11["re"].as_f64().unwrap(), s11["im"].as_f64().unwrap());
    50.0 * (1.0 + g) / (1.0 - g)
}

#[test]
fn an_element_across_a_ports_gap_loads_it_by_its_admittance() {
    let bare = probe(None);
    // The probe alone: a short (the wire's inductance), well away from 50 Ω.
    for (params, y) in [
        (json!({"name": "r", "from": [5, 5, 0], "to": [5, 5, 10], "resistance": 100}), emcraft_model::Complex64::new(0.01, 0.0)),
        (
            json!({"name": "rc", "from": [5, 5, 0], "to": [5, 5, 10], "resistance": 200, "capacitance": 1, "arrangement": "parallel"}),
            emcraft_model::Complex64::new(1.0 / 200.0, 2.0 * std::f64::consts::PI * 3e9 * 1e-12),
        ),
    ] {
        let loaded = probe(Some(params.clone()));
        // In parallel on one gap: the admittances add.
        let expected = 1.0 / (1.0 / bare + y);
        assert!((loaded - expected).norm() < 1e-6 * expected.norm(), "{params}: {loaded} expected {expected} (bare {bare})");
    }
}

#[test]
fn the_fast_sweep_keeps_an_elements_admittance() {
    // A sweep's points between its full solves come from a reduced model: the element must be in
    // it as in the full solves.
    let point = |frequencies: Value| {
        let mut s = Session::new();
        s.execute("geometry.solid", &json!({"shape": "box", "size": [10, 10, 10]})).unwrap();
        s.execute("model.port.lumped", &json!({"name": "p", "from": [5, 5, 0], "to": [5, 5, 10], "impedance": 50})).unwrap();
        s.execute("model.element.create", &json!({"name": "r", "from": [5, 5, 0], "to": [5, 5, 10], "resistance": 30, "inductance": 2})).unwrap();
        s.execute("mesh.generate", &json!({"size": 2})).unwrap();
        let r = s.execute("solve.frequency", &json!({"frequencies": frequencies, "order": 1})).unwrap();
        let p = r["points"].as_array().unwrap().iter().find(|p| p["frequency"] == 3.0).cloned().unwrap();
        (p["full"].as_bool(), emcraft_model::Complex64::new(p["s11"]["re"].as_f64().unwrap(), p["s11"]["im"].as_f64().unwrap()))
    };
    let (_, full) = point(json!([3]));
    let (was_full, fast) = point(json!([1, 1.5, 2, 2.5, 3, 3.5, 4, 4.5, 5]));
    assert_eq!(was_full, Some(false), "the point comes from the reduced model");
    assert!((fast - full).norm() < 1e-5, "{fast} {full}");
}

#[test]
fn elements_are_named_checked_and_removed() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [10, 10, 10]})).unwrap();
    s.execute("model.port.lumped", &json!({"name": "p", "from": [5, 5, 0], "to": [5, 5, 10]})).unwrap();
    for bad in [
        json!({"name": "p", "from": [5, 5, 0], "to": [5, 5, 10], "resistance": 50}),
        json!({"name": "x", "from": [5, 5, 0], "to": [5, 5, 10]}),
        json!({"name": "x", "from": [5, 5, 0], "to": [5, 5, 10], "resistance": -1}),
        json!({"name": "x", "from": [5, 5, 0], "to": [5, 5, 0], "resistance": 1}),
        json!({"name": "x", "from": [5, 5, 0], "to": [5, 5, 50], "resistance": 1}),
        json!({"name": "x", "from": [5, 5, 0], "to": [5, 5, 10], "resistance": 1, "arrangement": "star"}),
    ] {
        assert!(s.execute("model.element.create", &bad).is_err(), "{bad}");
    }
    s.execute("model.element.create", &json!({"name": "c", "from": [5, 5, 0], "to": [5, 5, 10], "capacitance": 2, "width": 2})).unwrap();
    assert_eq!(s.summary()["elements"][0]["capacitance_pf"], 2.0);
    assert!(s.execute("model.element.remove", &json!({"name": "nope"})).is_err());
    s.execute("model.element.remove", &json!({"name": "c"})).unwrap();
    assert!(s.model().elements.is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.model().elements.len(), 1);
}

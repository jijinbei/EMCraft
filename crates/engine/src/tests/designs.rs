//! Templates built from SolidCraft designs: the design compiled into the templates, opened by
//! `template:<file>`, the template's parameters passed to the design's.

use serde_json::json;

use crate::{EngineError, Session};

/// Template `name`'s model with `values`, built without meshing or solving.
fn built(name: &str, values: serde_json::Value) -> Session {
    let steps: Vec<crate::Step> = crate::template_steps(name, &values)
        .unwrap()
        .into_iter()
        .filter(|s| !s.command.starts_with("mesh.") && !s.command.starts_with("solve."))
        .collect();
    let mut s = Session::new();
    s.run_steps(&steps).unwrap();
    s
}

fn bounds(s: &mut Session) -> Vec<f64> {
    let [lo, hi] = s.model.bounds().unwrap().corners();
    (0..3).map(|d| (hi[d] - lo[d]) * 1e3).collect()
}

#[test]
fn a_template_opens_its_design_and_passes_its_parameters() {
    let mut s = built("taper", json!({"l": 30}));
    let size = bounds(&mut s);
    assert!((size[0] - 22.86).abs() < 1e-6 && (size[1] - 10.16).abs() < 1e-6 && (size[2] - 30.0).abs() < 1e-6, "{size:?}");
    // A parameter changed later rebuilds the model from the design at its new value.
    s.execute("project.variable", &json!({"name": "l", "value": 45})).unwrap();
    assert!((bounds(&mut s)[2] - 45.0).abs() < 1e-6);
    assert_eq!(s.model.ports.len(), 2);
}

#[test]
fn a_design_no_template_has_is_refused() {
    let mut s = Session::new();
    let e = s.execute("geometry.import", &json!({"path": "template:nothing.solidcraft"})).unwrap_err();
    assert!(matches!(&e, EngineError::Params(m) if m.contains("taper.solidcraft")), "{e}");
    // An expression of the project's variables needs them: an unknown one is refused.
    let e = s.execute("geometry.import", &json!({"path": "template:taper.solidcraft", "parameters": {"l": "=nothing"}})).unwrap_err();
    assert!(matches!(&e, EngineError::Params(_)), "{e}");
}

#[test]
fn the_horn_template_builds_its_walls_and_air() {
    let s = built("horn", json!(null));
    let bodies = crate::bodies::bodies(&s.model);
    assert!(
        bodies.iter().any(|b| b.name == "walls") && bodies.iter().any(|b| b.name == "inside"),
        "{:?}",
        bodies.iter().map(|b| &b.name).collect::<Vec<_>>()
    );
    assert!(s.model.open.is_some());
}

#[test]
fn the_termination_template_holds_its_absorber() {
    let s = built("termination", json!(null));
    assert_eq!(s.model.inclusions.len(), 1);
    assert_eq!(s.model.ports.len(), 1);
}

/// The matched load's |S11| over WR-90's band. A design target, not a reference: no closed form
/// gives a tapered lossy wedge's reflection; the bound says the template does what it is for
/// (0.057 at 8.2 GHz falling to 0.012 at 12.4 GHz when this was written).
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_termination_is_matched_over_the_band() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "termination"})).unwrap();
    let r = s.execute("solve.frequency", &json!({"start": 8.2, "stop": 12.4, "points": 5, "order": 2})).unwrap();
    let points = r["points"].as_array().unwrap();
    assert_eq!(points.len(), 5);
    let worst = points.iter().map(|p| p["s11"]["mag"].as_f64().unwrap()).fold(0.0, f64::max);
    assert!(worst < 0.07, "|S11| up to {worst}");
}

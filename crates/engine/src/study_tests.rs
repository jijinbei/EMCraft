//! Variables, sweeps and optimization through the session, against closed forms.

use std::f64::consts::PI;

use emcraft_model::units::C0;
use emcraft_theory::{Layer, Te10Line};
use serde_json::{Value, json};

use crate::{EngineError, Session};

const A: f64 = 22.86e-3;

/// The high-pass filter with its middle section's length a variable `l`.
fn filter(size: f64) -> Session {
    let mut s = Session::new();
    s.run_script(&json!([
        {"command": "project.variable", "params": {"name": "l", "value": 20}},
        {"command": "project.variable", "params": {"name": "feed", "value": "=l*3/4"}},
        {"command": "model.material.create", "params": {"name": "ptfe-like", "eps_r": 2.0}},
        {"command": "geometry.waveguide", "params": {"width": 22.86, "height": 10.16, "sections": [
            {"length": "=feed", "material": "ptfe-like"}, {"length": "=l", "material": "vacuum"}, {"length": 15, "material": "ptfe-like"}]}},
        {"command": "mesh.generate", "params": {"size": size}},
        {"command": "model.port.create", "params": {"name": "1", "end": "start", "modes": 2}},
        {"command": "model.port.create", "params": {"name": "2", "end": "end", "modes": 2}}
    ]))
    .unwrap();
    s
}

fn lengths(s: &Session) -> Vec<f64> {
    s.model.waveguide.as_ref().unwrap().sections.iter().map(|x| x.length).collect()
}

#[test]
fn a_variable_rebuilds_the_model_and_undo_goes_back() {
    let mut s = filter(6.0);
    assert_eq!(lengths(&s), vec![0.015, 0.020, 0.015]);
    let tets = s.model.mesh.as_ref().unwrap().tets.len();
    s.execute("project.variable", &json!({"name": "l", "value": 30})).unwrap();
    assert_eq!(lengths(&s), vec![0.0225, 0.030, 0.015]);
    assert!(s.model.mesh.as_ref().unwrap().tets.len() > tets, "the mesh is rebuilt too");
    assert_eq!(s.variables["feed"], 22.5);
    // The definition changed in place; the expressions are kept.
    assert_eq!(s.history[0]["params"]["value"], 30);
    assert_eq!(s.history[3]["params"]["sections"][1]["length"], "=l");
    let list = s.execute("project.variables", &json!({})).unwrap();
    assert_eq!(list["variables"][0], json!({"name": "feed", "value": 22.5, "uses": 1, "expression": "=l*3/4"}));
    assert_eq!(list["variables"][1]["uses"], 2);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(lengths(&s), vec![0.015, 0.020, 0.015]);
    assert_eq!(s.variables["l"], 20.0);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.variables["l"], 30.0);

    // A value the model cannot take leaves the session as it was.
    let before = s.history.clone();
    assert!(s.execute("project.variable", &json!({"name": "l", "value": -5})).is_err());
    assert_eq!((s.history.clone(), s.variables["l"]), (before, 30.0));
    // Used variables stay; unused ones go.
    assert!(s.execute("project.variable.delete", &json!({"name": "feed"})).is_err());
    s.execute("project.variable", &json!({"name": "spare", "value": 1})).unwrap();
    s.execute("project.variable.delete", &json!({"name": "spare"})).unwrap();
    assert!(!s.variables.contains_key("spare"));
    assert!(s.execute("project.variable", &json!({"name": "pi", "value": 3})).is_err());
    assert!(s.execute("project.variable", &json!({"name": "x", "value": "=nothing + 1"})).is_err());
    assert!(matches!(s.execute("mesh.generate", &json!({"size": "=1/0"})), Err(EngineError::Params(m)) if m.contains("division by zero")));

    // Saved and opened, the project is the same, expressions and all.
    let path = std::env::temp_dir().join(format!("emcraft-study-{}.emcraft", std::process::id()));
    let path = path.to_string_lossy().into_owned();
    s.execute("file.save", &json!({"path": path})).unwrap();
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": path})).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(t.history, s.history);
    assert_eq!(t.variables, s.variables);
    assert_eq!(lengths(&t), lengths(&s));
}

/// The exact (S11, S21) of the filter with middle length `l` (mm) at `f` (GHz).
fn exact(l: f64, f: f64) -> (f64, f64) {
    let line = Te10Line {
        a: A,
        layers: vec![Layer::lossless(0.75 * l * 1e-3, 2.0, 1.0), Layer::lossless(l * 1e-3, 1.0, 1.0), Layer::lossless(0.015, 2.0, 1.0)],
    };
    let (s11, s21) = line.solve(f * 1e9).unwrap().s_power();
    (s11.norm(), s21.norm())
}

#[test]
fn a_sweep_follows_the_exact_cascade_and_leaves_the_model() {
    let mut s = filter(5.0);
    let history = s.history.clone();
    let solve = json!({"command": "solve.frequency", "params": {"frequencies": [6, 8], "order": 2}});
    let r = s.execute("study.sweep", &json!({"variable": "l", "values": [12, 24], "solve": solve})).unwrap();
    let rows = r["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let l = row["value"].as_f64().unwrap();
        for pt in row["points"].as_array().unwrap() {
            let f = pt["frequency"].as_f64().unwrap();
            let (e11, e21) = exact(l, f);
            let (m11, m21) = (pt["s11"]["mag"].as_f64().unwrap(), pt["s21"]["mag"].as_f64().unwrap());
            assert!((m11 - e11).abs() < 5e-3 && (m21 - e21).abs() < 5e-3, "l = {l}, f = {f}: {m11} {m21} against {e11} {e21}");
        }
    }
    // The session is as it was; the study is kept.
    assert_eq!(s.history, history);
    assert_eq!(lengths(&s), vec![0.015, 0.020, 0.015]);
    let study = s.results.study.as_ref().unwrap();
    assert_eq!(study.parameters(), vec!["s11", "s12", "s21", "s22"]);
    // The same input, the same numbers.
    let again = s.execute("study.sweep", &json!({"variable": "l", "values": [12, 24], "solve": solve})).unwrap();
    assert_eq!(again, r);
    // A value the model cannot take is reported in its row.
    let bad = s.execute("study.sweep", &json!({"variable": "l", "values": [-1], "solve": solve, "outputs": ["s21"]})).unwrap();
    assert!(bad["rows"][0]["error"].is_string(), "{bad}");
}

#[test]
fn hostile_studies_are_refused() {
    let mut s = filter(8.0);
    let solve = json!({"command": "solve.frequency", "params": {"frequencies": [8], "order": 1}});
    for bad in [
        json!({"variable": "nope", "values": [1], "solve": solve}),
        json!({"variable": "l", "values": [1], "solve": {"command": "study.sweep"}}),
        json!({"variable": "l", "values": [1], "solve": {"command": "solve.frequency", "params": 3}}),
        json!({"variable": "l", "values": (0..200).collect::<Vec<_>>(), "solve": solve}),
        json!({"variable": "l", "start": 1, "stop": 2, "points": 1_000_000, "solve": solve}),
        json!({"variable": "l", "values": [10], "solve": {"command": "solve.frequency", "params": {"bogus": 1}}}),
    ] {
        assert!(matches!(s.execute("study.sweep", &bad), Err(EngineError::Params(_))), "{bad}");
    }
    for bad in [
        json!({"variables": [], "goal": {"type": "min_max_db"}, "solve": solve}),
        json!({"variables": [{"name": "l", "min": 5, "max": 1}], "goal": {"type": "min_max_db"}, "solve": solve}),
        json!({"variables": [{"name": "l", "min": 5, "max": 10}], "goal": {"type": "what"}, "solve": solve}),
        json!({"variables": [{"name": "l", "min": 5, "max": 10}], "goal": {"type": "min_max_db", "parameter": "s99"}, "solve": solve}),
        json!({"variables": [{"name": "l", "min": 5, "max": 10}], "goal": {"type": "min_max_db"}, "solve": solve, "max_evaluations": 100_000}),
    ] {
        let r = s.execute("study.optimize", &bad);
        assert!(matches!(r, Err(EngineError::Params(_))), "{bad}: {r:?}");
    }
    assert_eq!(lengths(&s), vec![0.015, 0.020, 0.015]);
}

/// A matching section between an empty guide and one filled with εr = 2.2: its εr makes its wave
/// admittance the geometric mean of theirs at 8 GHz, so the reflection vanishes where it is a
/// quarter of its guide wavelength long. The optimizer must find that length.
pub(crate) fn quarter_wave() -> (f64, f64) {
    let f = 8e9;
    let k0 = 2.0 * PI * f / C0;
    let kc = PI / A;
    let beta = |er: f64| (er * k0 * k0 - kc * kc).sqrt();
    let bm = (beta(1.0) * beta(2.2)).sqrt();
    let er = (bm * bm + kc * kc) / (k0 * k0);
    (er, PI / (2.0 * bm) * 1e3)
}

/// The optimizer's length within `within` of λg/4 (relative), solving at `order` on `size` mm.
fn quarter_wave_found(size: f64, order: u8, within: f64, deepest: f64) {
    let (er, quarter) = quarter_wave();
    let mut s = Session::new();
    s.run_script(&json!([
        {"command": "project.variable", "params": {"name": "l", "value": 8}},
        {"command": "model.material.create", "params": {"name": "match", "eps_r": er}},
        {"command": "model.material.create", "params": {"name": "load", "eps_r": 2.2}},
        {"command": "geometry.waveguide", "params": {"width": 22.86, "height": 10.16, "sections": [
            {"length": 8, "material": "vacuum"}, {"length": "=l", "material": "match"}, {"length": 8, "material": "load"}]}},
        {"command": "mesh.generate", "params": {"size": size}},
        {"command": "model.port.create", "params": {"name": "1", "end": "start", "modes": 1}},
        {"command": "model.port.create", "params": {"name": "2", "end": "end", "modes": 1}}
    ]))
    .unwrap();
    let r = s
        .execute(
            "study.optimize",
            &json!({"variables": [{"name": "l", "min": 4, "max": 18}], "goal": {"type": "min_max_db", "parameter": "s11"},
                    "solve": {"command": "solve.frequency", "params": {"frequencies": [8], "order": order}}, "tolerance": 1e-3}),
        )
        .unwrap();
    let best = r["best"]["values"][0].as_f64().unwrap();
    assert!((best - quarter).abs() < within * quarter, "{best} mm against λg/4 = {quarter} mm: {r}");
    assert!(r["best"]["objective"].as_f64().unwrap() < deepest, "{r}");
    assert_eq!(r["converged"], true);
    // The model is left at the optimum, as one step undo takes back.
    assert_eq!(s.variables["l"], best);
    assert!((lengths(&s)[1] - best * 1e-3).abs() < 1e-15);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.variables["l"], 8.0);
    let evaluations: Vec<&Value> = r["rows"].as_array().unwrap().iter().collect();
    assert!(evaluations.len() <= 50 && evaluations.len() == r["evaluations"].as_u64().unwrap() as usize);
}

#[test]
fn the_optimizer_finds_the_quarter_wave_match() {
    quarter_wave_found(5.0, 1, 0.03, -25.0);
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_optimizer_finds_the_quarter_wave_match_closely() {
    quarter_wave_found(3.0, 2, 0.01, -35.0);
}

#[test]
fn the_study_is_charted() {
    let mut s = filter(8.0);
    assert!(s.execute("view.study", &json!({"path": "x.png"})).is_err());
    let solve = json!({"command": "solve.frequency", "params": {"frequencies": [6, 7, 8], "order": 1}});
    s.execute("study.sweep", &json!({"variable": "l", "start": 15, "stop": 25, "points": 3, "solve": solve})).unwrap();
    let study = s.results.study.clone().unwrap();
    let c = super::chart(&study, Some("S21")).unwrap();
    assert_eq!(c.series.len(), 3);
    assert_eq!(c.series[2].label, "l = 25");
    assert!(super::chart(&study, Some("s99")).is_err());
    let path = std::env::temp_dir().join(format!("emcraft-study-{}.png", std::process::id())).to_string_lossy().into_owned();
    let r = s.execute("view.study", &json!({"path": path, "parameter": "s11", "width": 400, "height": 300})).unwrap();
    assert_eq!(r["width"], 400);
    let _ = std::fs::remove_file(&path);
}

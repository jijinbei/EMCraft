use super::*;
use crate::Session;
use serde_json::json;
#[test]
fn board_import_is_embedded_saved_undoable_and_diagnostics_are_explicit() {
    let mut s = Session::new();
    let path = scratch("routing.kicad_pcb");
    std::fs::write(&path, include_str!("../../../io/tests/fixtures/route.kicad_pcb")).unwrap();
    s.execute("pcb.import", &json!({"path": path})).unwrap();
    assert_eq!(s.history().last().unwrap().command, "pcb.load");
    std::fs::remove_file(path).unwrap();
    let report = s.execute("pcb.inspect", &json!({"net": "DATA"})).unwrap();
    assert_eq!(report["model_available"], true);
    assert_eq!(report["si"]["method"], "analytical_transmission_line");
    assert_eq!(report["sections"].as_array().unwrap().len(), 2);
    let project = scratch("pcb.emcraft");
    s.execute("file.save", &json!({"path": project})).unwrap();
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": project})).unwrap();
    assert_eq!(s.model().pcb, t.model().pcb);
    assert_eq!(t.views().pcb_net.as_deref(), Some("DATA"));
    t.execute("pcb.sample", &json!({"split": true})).unwrap();
    let bad = t.execute("pcb.inspect", &json!({"net": "DATA"})).unwrap();
    assert!(bad["si"].is_null());
    assert_eq!(bad["model_available"], false);
    t.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.model().pcb, t.model().pcb);
    let _ = std::fs::remove_file(project);
}

#[test]
fn board_commands_refuse_hostile_inputs_and_keep_the_model() {
    let mut s = Session::new();
    s.execute("pcb.sample", &json!({"split": false})).unwrap();
    let before = s.model().clone();
    for p in [
        json!({"net": "missing"}),
        json!({"net": "DATA", "ground": "DATA"}),
        json!({"net": "DATA", "reference": 0}),
        json!({"net": "DATA", "points": 100000}),
        json!({"net": "DATA", "stop": 1e100}),
    ] {
        assert!(s.execute("pcb.inspect", &p).is_err());
        assert_eq!(s.model(), &before);
    }
    let mut board = s.model().pcb.clone().unwrap();
    board.segments[0].width = -1.0;
    assert!(s.execute("pcb.load", &json!({"data": board})).is_err());
    assert_eq!(s.model(), &before);
    s.execute("units.set", &json!({"length": "m", "frequency": "MHz"})).unwrap();
    let r = s.execute("pcb.inspect", &json!({"net": "DATA", "stop": 1000})).unwrap();
    approx::assert_abs_diff_eq!(r["length"].as_f64().unwrap(), 0.04, epsilon = 1e-12);
    assert_eq!(r["si"]["points"][63]["frequency"], 1000.0);
}

#[test]
fn pcb_templates_declare_their_workflow_and_rebuild_from_parameters() {
    for (name, model, nearby) in [("pcb-microstrip", true, false), ("pcb-split-ground", false, false), ("pcb-parallel", true, true)] {
        let mut s = Session::new();
        s.execute("file.sample", &json!({"name": name, "values": {"l": 80, "w": 0.4, "fmax": 5}})).unwrap();
        assert_eq!(s.model.analysis(), emcraft_model::Analysis::Pcb);
        assert!(s.model.mesh.is_none());
        let status = s.execute("project.status", &json!({})).unwrap();
        assert_eq!(status["analysis"]["solve"], "pcb.inspect");
        assert!(status["not_needed"].as_array().unwrap().contains(&json!("mesh")));
        let r = s.execute("pcb.inspect", &json!({})).unwrap();
        assert_eq!(r["model_available"], model);
        approx::assert_abs_diff_eq!(r["length"].as_f64().unwrap(), 80., epsilon = 1e-10);
        assert_eq!(r["stop"], 5.);
        assert_eq!(r["findings"].as_array().unwrap().iter().any(|f| f["code"] == "nearby_signal"), nearby);
        s.execute("project.variable", &json!({"name": "l", "value": 60})).unwrap();
        let r = s.execute("pcb.inspect", &json!({"net": "DATA"})).unwrap();
        approx::assert_abs_diff_eq!(r["length"].as_f64().unwrap(), 60., epsilon = 1e-10);
        let before = s.summary();
        assert!(s.execute("project.variable", &json!({"name": "h", "value": -1})).is_err());
        assert_eq!(s.summary(), before);
    }
}

#[test]
fn a_board_loaded_with_expressions_is_built_again_from_its_variables() {
    // The microstrip circuit templates draw their boards with `pcb.load`, their numbers
    // expressions of the template's parameters: a parameter changed moves the copper.
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "pcb-stub-filter", "values": {"l_stub": 6, "er": 9.8}})).unwrap();
    assert_eq!(s.model.analysis(), emcraft_model::Analysis::Frequency);
    let board = s.model.pcb.as_ref().unwrap();
    let stub = board.segments.iter().find(|g| g.id == "stub").unwrap();
    approx::assert_abs_diff_eq!(stub.end[1], 0.006, epsilon = 1e-12);
    assert_eq!(board.layers[0].eps_below, 9.8);
    s.execute("project.variable", &json!({"name": "l_stub", "value": 8})).unwrap();
    let stub = s.model.pcb.as_ref().unwrap().segments.iter().find(|g| g.id == "stub").unwrap().clone();
    approx::assert_abs_diff_eq!(stub.end[1], 0.008, epsilon = 1e-12);
    // An expression of a name the session does not hold: refused, nothing changed.
    let before = s.summary();
    let bad = json!({"data": {"name": "x", "layers": [{"name": "F", "z": 0, "eps_below": "=nothing"}, {"name": "B", "z": 1, "eps_below": 1}], "segments": []}});
    assert!(s.execute("pcb.load", &bad).is_err());
    assert_eq!(s.summary(), before);
}

#[test]
fn pcb_workflow_navigation_is_checked_saved_and_does_not_run_diagnosis() {
    let mut s = Session::new();
    s.execute("view.pcb", &json!({"stage": "board"})).unwrap();
    assert!(s.execute("view.pcb", &json!({"stage": "net"})).is_err());
    s.execute("pcb.sample", &json!({"split": false})).unwrap();
    assert_eq!(crate::pcb::stage(&s), crate::pcb::Stage::Board);
    assert!(s.execute("view.pcb", &json!({"stage": "settings"})).is_err());
    assert!(s.execute("view.pcb", &json!({"net": "missing"})).is_err());
    s.execute("view.pcb", &json!({"stage": "net", "net": "DATA"})).unwrap();
    s.execute("view.pcb", &json!({"stage": "settings"})).unwrap();
    let file = scratch("pcb-stage.emcraft");
    s.execute("file.save", &json!({"path": file})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", &json!({"path": file})).unwrap();
    assert_eq!(crate::pcb::stage(&reopened), crate::pcb::Stage::Settings);
    assert_eq!(reopened.views().pcb_net.as_deref(), Some("DATA"));
    reopened.execute("pcb.inspect", &json!({})).unwrap();
    assert_eq!(crate::pcb::stage(&reopened), crate::pcb::Stage::Results);
    reopened.execute("pcb.inspect", &json!({"reference": 75, "stop": 10})).unwrap();
    reopened.execute("file.save", &json!({"path": file})).unwrap();
    let mut again = Session::new();
    again.execute("file.open", &json!({"path": file})).unwrap();
    assert_eq!(again.views().pcb_conditions.as_ref().unwrap().reference, 75.);
    assert_eq!(again.views().pcb_conditions.as_ref().unwrap().stop_hz, 1e10);
    let _ = std::fs::remove_file(file);
}

#[test]
fn coupling_pairs_roles_and_geometry_spacing_are_explicit() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "pcb-parallel"})).unwrap();
    s.execute("view.pcb", &json!({"net": "DATA", "victim": "CLK", "mode": "magnetic"})).unwrap();
    let r = s.execute("pcb.inspect", &json!({"net": "DATA"})).unwrap();
    let pairs = r["coupling_pairs"].as_array().unwrap();
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0]["target"], "CLK");
    approx::assert_abs_diff_eq!(pairs[0]["gap"].as_f64().unwrap(), 0.25, epsilon = 1e-10);
    assert_eq!(pairs[0]["overlap"], 20.);
    assert_eq!(pairs[0]["method"], "geometry_only");
    assert!(s.execute("view.pcb", &json!({"victim": "DATA"})).is_err());
    assert!(s.execute("view.pcb", &json!({"victim": "missing"})).is_err());
    assert!(s.execute("view.pcb", &json!({"mode": "radiation"})).is_err());
    let file = scratch("pcb-coupling.emcraft");
    s.execute("file.save", &json!({"path": file})).unwrap();
    let mut again = Session::new();
    again.execute("file.open", &json!({"path": file})).unwrap();
    assert_eq!(s.views().pcb_illustration, again.views().pcb_illustration);
    s.execute("project.variable", &json!({"name": "gap", "value": 1.5})).unwrap();
    let r = s.execute("pcb.inspect", &json!({"net": "DATA"})).unwrap();
    approx::assert_abs_diff_eq!(r["coupling_pairs"][0]["gap"].as_f64().unwrap(), 1.5, epsilon = 1e-10);
    let _ = std::fs::remove_file(file);
}

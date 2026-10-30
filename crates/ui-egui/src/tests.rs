//! The app's own tests: the headless scenarios in `tests/scenarios/` and a few direct checks.

use serde_json::{Value, json};

use crate::scenario::Harness;

fn scenario(name: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios").join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let steps: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut h = Harness::new();
    if let Err(e) = h.run(&steps) {
        panic!("scenario {name}: {e}\nlast messages: {:?}", h.app.messages.iter().rev().take(5).collect::<Vec<_>>());
    }
}

#[test]
fn scenario_periodic_cell() {
    scenario("periodic_cell");
}

#[test]
fn scenario_dispersion_diagram() {
    scenario("dispersion_diagram");
}

#[test]
fn scenario_open_sample() {
    scenario("open_sample");
}

#[test]
fn scenario_sweep_and_sparameters() {
    scenario("sweep_and_sparameters");
}

#[test]
fn scenario_time_domain_device() {
    scenario("time_domain_device");
}

#[test]
fn scenario_import_bodies() {
    scenario("import_bodies");
}

#[test]
fn scenario_import_cavity() {
    scenario("import_cavity");
}

#[test]
fn scenario_tdr_view() {
    scenario("tdr_view");
}

#[test]
fn scenario_field_view() {
    scenario("field_view");
}

#[test]
fn scenario_port_modes_table() {
    scenario("port_modes_table");
}

#[test]
fn scenario_hostile_dialog_input() {
    scenario("hostile_dialog_input");
}

#[test]
fn scenario_ribbon_and_search() {
    scenario("ribbon_and_search");
}

#[test]
fn scenario_cancel_a_sweep() {
    scenario("cancel_sweep");
}

#[test]
fn scenario_study_sweep() {
    scenario("study_sweep");
}

#[test]
fn scenario_study_optimize() {
    scenario("study_optimize");
}

#[test]
fn scenario_study_grid() {
    scenario("study_grid");
}

#[test]
fn scenario_view3d_orbit() {
    scenario("view3d_orbit");
}

#[test]
fn scenario_view3d_geometry() {
    scenario("view3d_geometry");
}

#[test]
fn scenario_view3d_open_boundary() {
    scenario("view3d_open_boundary");
}

#[test]
fn scenario_view3d_picking() {
    scenario("view3d_picking");
}

#[test]
fn scenario_view3d_ports_before_mesh() {
    scenario("view3d_ports_before_mesh");
}

#[test]
fn scenario_view3d_gpu() {
    scenario("view3d_gpu");
}

#[test]
fn scenario_symmetry_plane() {
    scenario("symmetry_plane");
}

#[test]
fn scenario_stub_and_via() {
    scenario("stub_and_via");
}

#[test]
fn scenario_terminal_ports() {
    scenario("terminal_ports");
}

#[test]
fn scenario_start_page() {
    scenario("start_page");
}

#[test]
fn scenario_view3d_field_arrows() {
    scenario("view3d_field_arrows");
}

#[test]
fn scenario_workflow_walk() {
    scenario("workflow_walk");
}

#[test]
fn scenario_workflow_missing_port() {
    scenario("workflow_missing_port");
}

#[test]
fn scenario_port_mode_rows() {
    scenario("port_mode_rows");
}

#[test]
fn scenario_setup_band() {
    scenario("setup_band");
}

#[test]
fn scenario_setup_device() {
    scenario("setup_device");
}

#[test]
fn scenario_errors_in_place() {
    scenario("errors_in_place");
}

#[test]
fn scenario_hostile_step_input() {
    scenario("hostile_step_input");
}

#[test]
fn scenario_themes() {
    scenario("themes");
}

#[test]
fn scenario_settings() {
    scenario("settings");
}

/// Every scenario file has a test above.
#[test]
fn every_scenario_file_runs() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios");
    let src = include_str!("tests.rs");
    for e in std::fs::read_dir(dir).unwrap() {
        let name = e.unwrap().path().file_stem().unwrap().to_string_lossy().into_owned();
        assert!(src.contains(&format!("scenario(\"{name}\")")), "tests/scenarios/{name}.json has no test");
    }
}

/// Hostile control requests are refused and nothing panics.
#[test]
fn hostile_control_requests_are_refused() {
    let mut h = Harness::new();
    for (m, p) in [
        ("ui.click", json!({"x": f64::MAX, "y": 1})),
        ("ui.click", json!({"x": "a", "y": 1})),
        ("ui.click", json!({"handle": "x".repeat(10_000)})),
        ("ui.key", json!({"key": "NoSuchKey"})),
        ("ui.set", json!({"tab": "NOPE"})),
        ("ui.set", json!({"view": 7})),
        ("engine.execute", json!({"command": "mesh.generate", "params": {"size": -1}})),
        ("engine.execute", json!({"command": "x".repeat(100_000)})),
        ("engine.execute", json!({"params": {}})),
        ("engine.script", json!({"commands": "nope"})),
        ("ui.start", json!({"command": "geometry.waveguide", "params": {"nope": 1}})),
        ("ui.dialog", json!({"ok": true})),
        ("no.such.method", json!({})),
    ] {
        let r = h.call(m, p);
        assert_eq!(r["ok"], json!(false), "{m}: {r}");
    }
    // Text far beyond a field's limit is cut, not stored.
    let r = h.call("ui.text", json!({"text": "9".repeat(1_000_000)}));
    assert_eq!(r["ok"], json!(true));
    let r = h.call("ui.render", json!({"width": 1e12, "height": -5, "scale": f64::NAN}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert!(r["result"]["width"].as_u64().unwrap() <= 4096 * 2);
}

/// `ui.render` draws the window without one; light and dark differ.
#[test]
fn ui_render_draws_both_themes() {
    let mut h = Harness::new();
    let light = crate::raster::render_app(&mut h.app, egui::vec2(800.0, 600.0), 1.0);
    h.app.theme = "dark".into();
    let dark = crate::raster::render_app(&mut h.app, egui::vec2(800.0, 600.0), 1.0);
    assert_eq!((light.w, light.h), (800, 600));
    let mean = |c: &emcraft_render::Canvas| {
        c.rgba.as_chunks::<4>().0.iter().map(|p| u64::from(p[0]) + u64::from(p[1]) + u64::from(p[2])).sum::<u64>() / (c.w * c.h) as u64
    };
    assert!(mean(&light) > mean(&dark) + 100, "light {} dark {}", mean(&light), mean(&dark));
    // The ribbon's tabs are drawn as text: the tab row is not one flat colour.
    let row: Vec<&[u8; 4]> = light.rgba.as_chunks::<4>().0.iter().skip(800 * 55).take(800).collect();
    assert!(row.windows(2).filter(|w| w[0] != w[1]).count() > 20);
}

/// File › Export history as script, Save and Open, Edit › Undo and Redo, through the menus.
#[test]
fn file_and_edit_menus() {
    let dir = std::env::temp_dir().join(format!("emcraft-ui-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (script, project) = (dir.join("history.json"), dir.join("project.emcraft"));
    let mut h = Harness::new();
    let steps = json!([
        {"call": "engine.execute", "params": {"command": "file.sample", "params": {"name": "highpass"}}},
        {"call": "engine.execute", "params": {"command": "project.frequency", "params": {"start": 5, "stop": 9}}},
        {"click": "menu:file"},
        {"click": "file:export"},
        {"expect": {"inspect": {"export": "emcraft-history.json"}, "handles": ["export:path", "export:ok"]}},
        {"click": "export:path"},
        {"key": "A", "cmd": true},
        {"text": script.to_str().unwrap()},
        {"click": "export:ok"},
        {"expect": {"status": "Exported", "inspect": {"export": null}}},
        {"note": "Save: the project has no file yet, so its path is asked (the path field, without a file dialog)"},
        {"expect": {"inspect": {"project.dirty": true, "project.title": "highpass •"}}},
        {"click": "menu:file"},
        {"click": "file:save"},
        {"expect": {"inspect": {"files.prompt": "highpass.emcraft"}}},
        {"click": "file:path"},
        {"key": "A", "cmd": true},
        {"text": project.with_extension("").to_str().unwrap()},
        {"click": "file:ok"},
        {"expect": {"status": "Saved", "inspect": {"project.dirty": false, "project.name": "project", "recent.0": project.to_str().unwrap()}}},
        {"note": "Undo the band from the Edit menu, then redo it"},
        {"click": "menu:edit"},
        {"click": "edit:undo"},
        {"expect": {"status": "Undone", "inspect": {"workflow.steps.intent": "warning"}}},
        {"click": "menu:edit"},
        {"click": "edit:redo"},
        {"expect": {"status": "Redone", "inspect": {"workflow.steps.intent": "done"}}},
        {"note": "A file that is not a project is refused; the project opens"},
        {"call": "engine.execute", "params": {"command": "file.open", "params": {"path": script.to_str().unwrap()}}, "fail": true},
        {"call": "engine.execute", "params": {"command": "file.open", "params": {"path": project.to_str().unwrap()}}},
        {"expect": {"status": "Opened", "inspect": {"workflow.steps.intent": "done", "model.mesh.tetrahedra": {"min": 100}}}}
    ]);
    if let Err(e) = h.run(&steps) {
        panic!("{e}\nlast messages: {:?}", h.app.messages.iter().rev().take(5).collect::<Vec<_>>());
    }
    // The exported history rebuilds the model.
    let text = std::fs::read_to_string(&script).unwrap();
    let mut s = emcraft_engine::Session::new();
    s.run_script(&serde_json::from_str::<Value>(&text).unwrap()).unwrap();
    assert_eq!(s.model().waveguide, h.app.session.model().waveguide);
    assert_eq!(s.model().band, h.app.session.model().band);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "solves the dipole (seconds): cargo test --release -p emcraft-ui-egui scenario_dipole_fields -- --ignored"]
fn scenario_dipole_fields() {
    scenario("dipole_fields");
}

#[test]
#[ignore = "a tour of every sample's steps for looking at: EMCRAFT_SCENARIO_SHOTS=DIR cargo test -p emcraft-ui-egui scenario_visibility_tour -- --ignored"]
fn scenario_visibility_tour() {
    scenario("visibility_tour");
}

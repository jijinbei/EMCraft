//! The app's own tests: the headless scenarios in `tests/scenarios/` and a few direct checks.

use serde_json::{Value, json};

use crate::scenario::Harness;

/// Runs `tests/scenarios/<name>.json`; `$TMP` in it is a folder of its own, emptied first and
/// removed after.
fn scenario(name: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios").join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let tmp = std::env::temp_dir().join(format!("emcraft-scenario-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap_or_else(|e| panic!("{}: {e}", tmp.display()));
    let text = text.replace("$TMP", tmp.to_str().unwrap_or_else(|| panic!("{}: not UTF-8", tmp.display())));
    let steps: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut h = Harness::new();
    let r = h.run(&steps);
    let _ = std::fs::remove_dir_all(&tmp);
    if let Err(e) = r {
        panic!("scenario {name}: {e}\nlast messages: {:?}", h.app.messages.iter().rev().take(5).collect::<Vec<_>>());
    }
}

#[test]
fn scenario_chart_style() {
    scenario("chart_style");
}

#[test]
fn scenario_refine_and_planes() {
    scenario("refine_and_planes");
}

#[test]
fn scenario_field_probes() {
    scenario("field_probes");
}

#[test]
fn scenario_far_field_cuts() {
    scenario("far_field_cuts");
}

#[test]
fn scenario_run_estimate() {
    scenario("run_estimate");
}

#[test]
fn scenario_section_view() {
    scenario("section_view");
}

#[test]
fn scenario_setup_restore() {
    scenario("setup_restore");
}

#[test]
fn scenario_edit_items() {
    scenario("edit_items");
}

#[test]
fn scenario_close_and_recover() {
    scenario("close_and_recover");
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
fn scenario_step_status() {
    scenario("step_status");
}

#[test]
fn scenario_stale_results() {
    scenario("stale_results");
}

#[test]
fn scenario_dialog_keys() {
    scenario("dialog_keys");
}

#[test]
fn scenario_job_progress() {
    scenario("job_progress");
}

#[test]
fn scenario_sparameter_display() {
    scenario("sparameter_display");
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
fn scenario_study_surrogate() {
    scenario("study_surrogate");
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
fn scenario_wall_thickness() {
    scenario("wall_thickness");
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
fn scenario_view3d_measure() {
    scenario("view3d_measure");
}

#[test]
fn scenario_view3d_port_tool() {
    scenario("view3d_port_tool");
}

#[test]
fn scenario_port_roles() {
    scenario("port_roles");
}

#[test]
fn scenario_view3d_side_ports() {
    scenario("view3d_side_ports");
}

#[test]
fn scenario_junction_templates() {
    scenario("junction_templates");
}

#[test]
fn scenario_view3d_lumped_tool() {
    scenario("view3d_lumped_tool");
}

#[test]
fn scenario_view3d_tools_ja() {
    scenario("view3d_tools_ja");
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
fn scenario_languages() {
    scenario("languages");
}

#[test]
fn scenario_settings() {
    scenario("settings");
}

#[test]
fn scenario_navigation() {
    scenario("navigation");
}

#[test]
fn scenario_box_select() {
    scenario("box_select");
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
        {"expect": {"status": "Undone", "inspect": {"workflow.steps.intent": "todo"}}},
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

#[test]
fn scenario_chart_packing() {
    scenario("chart_packing");
}

#[test]
fn scenario_small_window() {
    scenario("small_window");
}

#[test]
fn scenario_template_dialog() {
    scenario("template_dialog");
}

#[test]
fn scenario_template_design() {
    scenario("template_design");
}

#[test]
fn scenario_analysis_kind() {
    scenario("analysis_kind");
}

#[test]
fn scenario_purpose_first() {
    scenario("purpose_first");
}

#[test]
fn scenario_project_home() {
    scenario("project_home");
}

#[test]
fn scenario_run_history() {
    scenario("run_history");
}

#[test]
fn scenario_saved_history() {
    scenario("saved_history");
}

/// The high-pass filter's 3D view, front on, and the view's rectangle.
fn front_view() -> (Harness, egui::Rect) {
    let mut h = Harness::new();
    h.run(&json!([
        {"call": "engine.execute", "params": {"command": "file.sample", "params": {"name": "highpass"}}},
        {"call": "ui.set", "params": {"view": "model", "step": "none", "standard_view": "front"}},
        {"frames": 2},
    ]))
    .unwrap();
    let rect = crate::scenario::rect_of("view3d").unwrap();
    (h, rect)
}

/// Where model point `p` is drawn in the view `rect` (window points).
fn on_screen(h: &Harness, rect: egui::Rect, p: [f64; 3]) -> egui::Pos2 {
    let q = h.app.view3d.camera.project(p, f64::from(rect.width()), f64::from(rect.height())).unwrap();
    rect.min + egui::vec2(q[0] as f32, q[1] as f32)
}

/// The model point under window point `at` (the surface there).
fn under(h: &Harness, rect: egui::Rect, at: egui::Pos2) -> Option<[f64; 3]> {
    let q = at - rect.min;
    let scene = &h.app.view3d.shown()?.scene;
    emcraft_render::view3d::surface_at(scene, &h.app.view3d.camera, f64::from(q.x), f64::from(q.y), f64::from(rect.width()), f64::from(rect.height()))
}

/// The wheel, a pinch and Ctrl+wheel zoom toward the point under the cursor: it stays put.
#[test]
fn zooming_keeps_the_point_under_the_cursor() {
    let (mut h, rect) = front_view();
    // On the model, then over empty space (the plane through the target).
    for at in [rect.center() + egui::vec2(40.0, -15.0), rect.left_top() + egui::vec2(60.0, 60.0)] {
        let anchor = under(&h, rect, at).unwrap_or_else(|| {
            let q = at - rect.min;
            h.app.view3d.camera.on_target_plane(f64::from(q.x), f64::from(q.y), f64::from(rect.width()), f64::from(rect.height())).unwrap()
        });
        let before = h.app.view3d.camera.distance;
        for params in [json!({"delta": 2}), json!({"zoom": 1.5}), json!({"delta": -1, "ctrl": true}), json!({"delta": -3})] {
            let mut p = params;
            p["x"] = json!(at.x);
            p["y"] = json!(at.y);
            assert_eq!(h.call("ui.scroll", p.clone())["ok"], json!(true), "{p}");
            let now = on_screen(&h, rect, anchor);
            assert!(now.distance(at) < 0.05, "{p}: {at:?} moved to {now:?}");
        }
        assert!(h.app.view3d.camera.distance != before);
    }
}

/// An orbit about the point under the cursor keeps that point where it was on screen; the
/// default (the view's centre) does not.
#[test]
fn orbiting_about_the_cursor_keeps_the_picked_point() {
    let (mut h, rect) = front_view();
    let at = rect.center() + egui::vec2(35.0, -10.0);
    let picked = under(&h, rect, at).expect("the model under the pointer");
    let drag = |h: &mut Harness| {
        h.step(&json!({"drag": [[at.x, at.y], [at.x + 90.0, at.y + 40.0]], "button": "right"})).unwrap();
    };
    h.call("ui.set", json!({"orbit_around": "cursor"}));
    drag(&mut h);
    assert!(h.app.view3d.camera.yaw.to_degrees() > 300.0, "{:?}", h.app.view3d.camera);
    assert!(on_screen(&h, rect, picked).distance(at) < 0.05, "{:?}", on_screen(&h, rect, picked));
    let (mut h, rect) = front_view();
    let picked = under(&h, rect, at).unwrap();
    drag(&mut h);
    assert!(on_screen(&h, rect, picked).distance(at) > 5.0);
}

/// View changes animate: frames between show the camera on its way, eased, the short way round.
#[test]
fn view_changes_animate_by_the_frames_clock() {
    let (mut h, _) = front_view();
    h.app.view3d.standard(emcraft_render::view3d::StandardView::Right);
    let mut yaws = Vec::new();
    for _ in 0..40 {
        h.frame();
        yaws.push(h.app.view3d.camera.yaw.to_degrees());
    }
    assert!(!h.app.view3d.animating());
    // 0.5 s at 60 frames a second: under way for 30 frames, rising, slowest at its ends.
    assert!(yaws[..29].iter().all(|y| *y < 90.0 - 1e-9), "{yaws:?}");
    assert!((yaws[35] - 90.0).abs() < 1e-9);
    assert!(yaws.windows(2).all(|w| w[1] >= w[0] - 1e-12), "{yaws:?}");
    assert!(yaws[2] - yaws[1] < yaws[15] - yaws[14]);
    // A double-click on empty space fits again, animated, to the framing the model opened with.
    let fitted = h.app.view3d.camera.distance;
    h.call("ui.set", json!({"camera": {"distance": fitted * 3.0}}));
    let rect = crate::scenario::rect_of("view3d").unwrap();
    let at = egui::pos2(rect.right() - 30.0, rect.center().y);
    h.step(&json!({"click": [at.x, at.y], "double": true})).unwrap();
    assert!((h.app.view3d.camera.distance - fitted).abs() < 1e-9 * fitted, "{} {fitted}", h.app.view3d.camera.distance);
}

/// A held arrow keeps turning the view after a tap's step, at 90° a second; let go, it stops.
#[test]
fn a_held_arrow_turns_the_view_steadily() {
    let (mut h, _) = front_view();
    h.step(&json!({"key": "ArrowRight", "press": "down"})).unwrap();
    h.frames(60);
    h.step(&json!({"key": "ArrowRight", "press": "up"})).unwrap();
    let yaw = h.app.view3d.camera.yaw.to_degrees();
    h.frames(30);
    assert_eq!(h.app.view3d.camera.yaw.to_degrees(), yaw);
    // About a second held: the 15° step's start, then a steady turn.
    assert!(yaw > 60.0 && yaw < 120.0, "{yaw}");
}

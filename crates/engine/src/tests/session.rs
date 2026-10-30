//! The session: commands, units, rollback, hostile input, projects, undo, samples and the workflow.

use serde_json::json;

use super::{scratch, small};
use crate::*;

#[test]
fn display_units_convert_at_the_boundary() {
    let mut s = Session::new();
    s.execute("units.set", &json!({"length": "m", "frequency": "MHz"})).unwrap();
    s.execute("geometry.waveguide", &json!({"width": 0.02286, "height": 0.01016, "sections": [{"length": 0.03, "material": "vacuum"}]})).unwrap();
    let w = s.model.waveguide.as_ref().unwrap();
    assert!((w.width - 0.02286).abs() < 1e-15);
    s.execute("mesh.generate", &json!({"size": 0.005})).unwrap();
    s.execute("model.port.create", &json!({"name": "a", "end": "start"})).unwrap();
    s.execute("model.port.create", &json!({"name": "b", "end": "end"})).unwrap();
    let r = s.execute("solve.frequency", &json!({"frequencies": [9000], "order": 1})).unwrap();
    assert_eq!(r["frequency_unit"], "MHz");
    assert_eq!(r["points"][0]["frequency"], 9000.0);
    assert!((s.results.sweep.as_ref().unwrap().results[0].frequency - 9e9).abs() < 1e-3);
}

/// Every command that changes the model says which results the change leaves wrong, unless it
/// changes only what no result is solved from (the band, the display units, a new material) or
/// decides by what it changed (a material's edit: a new name alone keeps them); they are dropped
/// when it succeeds, not when it fails.
#[test]
fn every_model_change_drops_the_results_it_makes_wrong() {
    let keeps = ["project.frequency", "units.set", "model.material.create", "model.material.edit"];
    for c in COMMANDS.iter().filter(|c| c.effect == Effect::Record) {
        assert_eq!(c.invalidates.is_none(), keeps.contains(&c.id), "{}", c.id);
    }
    assert!(COMMANDS.iter().filter(|c| c.effect != Effect::Record).all(|c| c.invalidates.is_none()));
    let mut s = small();
    // Failed: the sweep stays.
    assert!(s.execute("geometry.body", &json!({"shape": "sphere", "radius": -1, "material": "metal"})).is_err());
    assert!(s.results.sweep.is_some());
    s.execute("project.frequency", &json!({"start": 5, "stop": 9})).unwrap();
    assert!(s.results.sweep.is_some());
    s.execute("model.walls", &json!({"conductivity": 5.8e7})).unwrap();
    assert!(s.results.sweep.is_none());
}

/// The states kept to undo to share the mesh rather than copy it, and the oldest go first.
#[test]
fn undo_states_share_the_mesh_and_are_bounded() {
    let mut s = small();
    let mesh = s.model.mesh.clone().unwrap();
    for k in 0..project::UNDO_DEPTH + 5 {
        s.execute("project.frequency", &json!({"start": 5 + k, "stop": 100})).unwrap();
    }
    assert_eq!(s.undo.len(), project::UNDO_DEPTH);
    assert!(s.undo.iter().all(|u| u.model.mesh.as_ref().is_some_and(|m| std::sync::Arc::ptr_eq(m, &mesh))));
    // The oldest left: the state before the sixth change, the fifth band.
    assert_eq!(s.undo.front().and_then(|u| u.model.band).map(|b| b.start), Some(9e9));
}

#[test]
fn a_failed_command_leaves_the_session_as_it_was() {
    let mut s = small();
    let before = s.model.clone();
    // An unknown material fails after the command started changing things.
    let e = s.execute("geometry.waveguide", &json!({"width": 10, "height": 5, "sections": [{"length": 3, "material": "nothing"}]}));
    assert!(e.is_err());
    assert_eq!(s.model, before);
    assert!(s.results.sweep.is_some());
    assert!(matches!(s.execute("mesh.generate", &json!({"size": 1e-5})), Err(EngineError::Failed(_))));
    assert!(s.model.mesh.is_some() && s.results.sweep.is_some());
}

#[test]
fn hostile_parameters_are_refused_and_nothing_panics() {
    let garbage = [
        json!(null),
        json!([]),
        json!("x"),
        json!({"name": 1, "eps_r": "a", "width": -1, "height": f64::MAX, "sections": [{}], "size": 0, "end": "middle", "modes": 1e9,
               "frequencies": [-1, "a"], "start": 1e308, "stop": -5, "points": 1e12, "order": 7, "frequency": 1e300, "at": [[1, 2]],
               "path": "", "plane": "q", "component": "zz", "phase": 1e308, "width_px": 3, "excitation": "nope", "solve": "yes",
               "shape": "torus", "thickness": -3, "reflection": f64::MAX}),
        json!({"frequency": 8, "at": [[1e300, 0, 0]], "path": "/nonexistent-dir/x.png", "width": 1_000_000, "height": 0}),
    ];
    let solved = small();
    for c in COMMANDS {
        for g in &garbage {
            let mut s = solved.clone();
            let r = s.execute(c.id, g);
            assert!(!matches!(r, Err(EngineError::Panicked(_))), "{} panicked on {g}", c.id);
        }
    }
    let mut s = Session::new();
    assert!(matches!(s.execute("no.such", &json!({})), Err(EngineError::UnknownCommand(_))));
    assert!(matches!(s.execute("mesh.generate", &json!({"size": 1, "bogus": 2})), Err(EngineError::Params(_))));
    assert!(matches!(s.execute("file.sample", &json!({"name": "nothing"})), Err(EngineError::Params(_))));
}

#[test]
fn features_page_lists_every_command() {
    let md = features_markdown();
    for c in COMMANDS {
        assert!(md.contains(&format!("`{}`", c.id)));
    }
}

#[test]
fn every_sample_is_a_script_of_known_commands() {
    for smp in emcraft_templates::TEMPLATES {
        let script: serde_json::Value = serde_json::from_str(smp.script).unwrap_or_else(|e| panic!("{}: {e}", smp.name));
        for step in script.as_array().unwrap() {
            let id = step["command"].as_str().unwrap();
            assert!(crate::find_command(id).is_some(), "{}: {id}", smp.name);
        }
    }
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn every_sample_opens() {
    // Without their solve steps: the geometry, mesh, ports and sources of each.
    for smp in emcraft_templates::TEMPLATES {
        let mut s = Session::new();
        s.execute("file.sample", &json!({"name": smp.name})).unwrap_or_else(|e| panic!("{}: {e}", smp.name));
        assert!(s.model.mesh.is_some(), "{}", smp.name);
    }
}

#[test]
fn the_workflow_status_guides_a_job_and_undo_saves_and_exports_work() {
    let mut s = Session::new();
    let status = |s: &mut Session| -> Vec<(String, String)> {
        let r = s.execute("project.status", &json!({})).unwrap();
        r["steps"].as_array().unwrap().iter().map(|x| (x["step"].as_str().unwrap().to_owned(), x["status"].as_str().unwrap().to_owned())).collect()
    };
    let of = |v: &[(String, String)], step: &str| v.iter().find(|(n, _)| n == step).map(|x| x.1.clone()).unwrap();
    // An empty session: the geometry and the band to do, the steps after the geometry and
    // Run waiting for it, nothing wrong.
    let st = status(&mut s);
    assert_eq!(of(&st, "geometry"), "todo");
    assert_eq!(of(&st, "intent"), "todo");
    assert_eq!((of(&st, "materials"), of(&st, "boundaries"), of(&st, "mesh")), ("waiting".into(), "waiting".into(), "waiting".into()));
    assert_eq!((of(&st, "run"), of(&st, "results")), ("waiting".into(), "waiting".into()));
    // The band below WR-90's cut-off is flagged at the port.
    s.execute("project.frequency", &json!({"start": 5, "stop": 12})).unwrap();
    s.execute("geometry.waveguide", &json!({"width": 22.86, "height": 10.16, "sections": [{"length": 20, "material": "vacuum"}]})).unwrap();
    s.execute("model.port.create", &json!({"name": "1", "end": "start"})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end"})).unwrap();
    let st = status(&mut s);
    assert_eq!(of(&st, "excitation"), "warning", "{st:?}");
    assert_eq!((of(&st, "mesh"), of(&st, "run")), ("todo".into(), "waiting".into()));
    s.execute("project.frequency", &json!({"start": 8, "stop": 12})).unwrap();
    s.execute("mesh.generate", &json!({"per_wavelength": 6})).unwrap();
    let st = status(&mut s);
    assert_eq!(of(&st, "excitation"), "done", "{st:?}");
    assert_eq!(of(&st, "mesh"), "done", "{st:?}");
    assert_eq!(of(&st, "run"), "todo", "{st:?}");
    let est = s.execute("project.estimate", &json!({"order": 1})).unwrap();
    assert!(est["unknowns"].as_f64().unwrap() > 100.0 && est["memory_mb"].as_f64().is_some());
    // A sweep over the band by default; then results are done, and stale after a change.
    s.execute("solve.frequency", &json!({"points": 3, "order": 1})).unwrap();
    assert_eq!(of(&status(&mut s), "results"), "done");
    let check = s.execute("results.check", &json!({})).unwrap();
    assert!(check["passive"].as_bool().unwrap() && check["reciprocity_error"].as_f64().unwrap() < 1e-9, "{check}");
    let ts = scratch("guide.s2p");
    s.execute("results.touchstone", &json!({"path": ts})).unwrap();
    let text = std::fs::read_to_string(&ts).unwrap();
    assert!(text.contains("# GHz S RI R 50") && text.lines().filter(|l| !l.starts_with(['!', '#'])).count() == 3, "{text}");
    s.execute("units.set", &json!({"length": "m"})).unwrap();
    assert_eq!(of(&status(&mut s), "results"), "stale");
    // Undo, redo; the history rebuilds the model.
    let n = s.history.len();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.history.len(), n - 1);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.history.len(), n);
    let path = scratch("job.emcraft.json");
    s.execute("file.save", &json!({"path": path})).unwrap();
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": path})).unwrap();
    assert_eq!(t.model.band, s.model.band);
    assert_eq!(t.model.mesh.as_ref().map(|m| m.tets.len()), s.model.mesh.as_ref().map(|m| m.tets.len()));
    // Back to the empty session before the open.
    t.execute("edit.undo", &json!({})).unwrap();
    assert!(t.model.waveguide.is_none());
    // Hostile input is refused.
    assert!(s.execute("project.frequency", &json!({"start": 9, "stop": 2})).is_err());
    assert!(s.execute("project.frequency", &json!({"start": -1, "stop": 2})).is_err());
    assert!(s.execute("file.open", &json!({"path": ts})).is_err());
    assert!(s.execute("results.touchstone", &json!({"path": ts, "format": "db"})).is_err());
    assert!(Session::new().execute("edit.undo", &json!({})).is_err());
}

#[test]
fn every_choice_a_command_lists_is_one_its_parameter_reads() {
    // The table lists each choice's ids (`one_of(&T::IDS)`), which serde reads too
    // (`named_serde!`): one list for both.
    // The component is read by name in either case.
    for c in crate::view::Component::ALL {
        assert_eq!(c.id().parse::<crate::view::Component>(), Ok(c));
        assert_eq!(c.id().to_uppercase().parse::<crate::view::Component>(), Ok(c));
    }
    // Every listed choice is one of these lists, and every command reads its choices.
    let shapes = find_command("geometry.solid").and_then(|c| c.params.iter().find(|p| p.name == "shape")).unwrap();
    assert_eq!(shapes.choices, &crate::geometry::Primitive::IDS[..]);
    let mut s = Session::new();
    assert!(
        matches!(s.execute("solve.device", &json!({"device": "tpu"})), Err(EngineError::Params(m)) if m == "`device` must be cpu, cuda or cubecl-cpu")
    );
    assert!(
        matches!(s.execute("solve.device", &json!({"device": "cpu", "precision": "half"})), Err(EngineError::Params(m)) if m == "`precision` must be single or double")
    );
}

#[test]
fn projects_save_as_emcraft_files_and_know_when_they_changed() {
    let dir = std::env::temp_dir().join(format!("emcraft-project-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut s = Session::new();
    assert!(!s.is_dirty() && s.name() == "Untitled");
    // No file yet: a path is needed.
    assert!(matches!(s.execute("file.save", &json!({})), Err(EngineError::Params(_))));
    s.execute("model.material.create", &json!({"name": "ptfe", "eps_r": 2.1})).unwrap();
    assert!(s.is_dirty());
    let base = dir.join("filter").to_string_lossy().into_owned();
    let r = s.execute("file.save", &json!({"path": base})).unwrap();
    let path = format!("{base}.emcraft");
    assert_eq!(r["path"], json!(path));
    assert!(!s.is_dirty() && s.name() == "filter" && std::path::Path::new(&path).exists());
    // Changed, then saved again to its own file; undo back to the saved state is clean.
    s.execute("project.frequency", &json!({"start": 5, "stop": 9})).unwrap();
    assert!(s.is_dirty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(!s.is_dirty());
    s.execute("project.frequency", &json!({"start": 5, "stop": 9})).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    assert!(!s.is_dirty());
    // Open: the same model, clean, with its path; new: empty and untitled, units kept.
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": path})).unwrap();
    assert!(!t.is_dirty() && t.path.as_deref() == Some(path.as_str()) && t.model.band.is_some());
    t.execute("units.set", &json!({"frequency": "MHz"})).unwrap();
    t.execute("file.new", &json!({})).unwrap();
    assert!(t.path.is_none() && !t.is_dirty() && t.model.band.is_none());
    assert_eq!(t.display.frequency, emcraft_model::units::Frequency::Megahertz);
    // The file's text: the JSON object of the format, the version, the history and the model,
    // its keys sorted, pretty printed.
    let text = std::fs::read_to_string(&path).unwrap();
    let want = json!({"format": "emcraft-project", "version": 2, "history": s.history, "runs": [], "model": s.model});
    assert_eq!(text, serde_json::to_string_pretty(&want).unwrap());
    // A newer version is refused; an older one without a version opens.
    let newer = dir.join("newer.emcraft");
    std::fs::write(&newer, json!({"format": "emcraft-project", "version": 4, "history": []}).to_string()).unwrap();
    let r = t.execute("file.open", &json!({"path": newer.to_string_lossy()}));
    assert!(matches!(&r, Err(EngineError::Failed(m)) if m.contains("version 4")), "{r:?}");
    std::fs::write(&newer, json!({"format": "emcraft-project", "history": [{"command": "units.set"}]}).to_string()).unwrap();
    t.execute("file.open", &json!({"path": newer.to_string_lossy()})).unwrap();
    assert_eq!(t.history, vec![crate::Step::new("units.set", json!({}))]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_change_drops_the_results_it_makes_wrong_and_keeps_the_rest() {
    let mut r = Results::default();
    let filled = |r: &mut Results| {
        r.radiation = None;
        r.time_domain = Some(std::sync::Arc::new(emcraft_fdtd::TdResult {
            frequencies: Vec::new(),
            s11: Vec::new(),
            s21: Vec::new(),
            cells: 0,
            dt: 0.0,
            steps: 0,
            reference_steps: 0,
            converged: true,
            loss_reference: 0.0,
        }));
    };
    filled(&mut r);
    r.invalidate(Change::Open);
    assert!(r.time_domain.is_some(), "an open-boundary change keeps the guide's time domain");
    r.invalidate(Change::Walls);
    assert!(r.time_domain.is_some(), "walls keep the time domain's S-parameters");
    r.invalidate(Change::Ports);
    assert!(r.time_domain.is_none());
    filled(&mut r);
    r.invalidate(Change::Geometry);
    assert!(!r.any());
}

/// The summary names a solid's shape; it never dumps an imported surface's triangles.
#[test]
fn the_summary_names_the_solid_shape() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "sphere", "radius": 5})).unwrap();
    assert_eq!(s.summary()["solid"]["shape"], json!("Sphere"));
}

#[test]
fn a_sample_opened_with_its_solve_has_current_results() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass", "solve": true})).unwrap();
    assert_eq!(s.solved_revision, Some(s.revision));
    // A change after it makes them stale.
    s.execute("project.frequency", &json!({"start": 5, "stop": 9})).unwrap();
    assert_ne!(s.solved_revision, Some(s.revision));
}

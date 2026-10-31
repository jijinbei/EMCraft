//! The project as a whole: undo across a replaced project, its runs, autosave copies and
//! recovery.

use serde_json::json;

use super::scratch;
use crate::*;

/// A project of a band and a material, saved at `path`.
fn saved(path: &str, start: f64) -> Session {
    let mut s = Session::new();
    s.execute("project.frequency", &json!({"start": start, "stop": 12})).unwrap();
    s.execute("file.save", &json!({"path": path})).unwrap();
    s
}

/// Open B over A, undo: A is back whole — its file (so a save writes A, never B), its unsaved
/// state, and redo goes back to B with B's.
#[test]
fn undoing_an_open_brings_the_project_before_it_back() {
    let (a, b) = (scratch("undo-a.emcraft"), scratch("undo-b.emcraft"));
    saved(&b, 7.0);
    let mut s = saved(&a, 5.0);
    // A has an unsaved change when B is opened over it.
    s.execute("model.material.create", &json!({"name": "ptfe", "eps_r": 2.1})).unwrap();
    assert!(s.is_dirty());
    s.execute("file.open", &json!({"path": b})).unwrap();
    assert_eq!((s.path(), s.is_dirty()), (Some(b.as_str()), false));
    let r = s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!((s.path(), s.is_dirty()), (Some(a.as_str()), true));
    assert_eq!((r["path"].as_str(), r["dirty"].as_bool()), (Some(a.as_str()), Some(true)));
    assert_eq!(s.model.band.map(|x| x.start.get()), Some(5e9));
    // The save after the undo writes A's file; B's is untouched.
    let b_text = std::fs::read_to_string(&b).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    assert!(!s.is_dirty());
    assert!(std::fs::read_to_string(&a).unwrap().contains("ptfe"));
    assert_eq!(std::fs::read_to_string(&b).unwrap(), b_text);
    // Redo: B again, clean, its own file.
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!((s.path(), s.is_dirty(), s.model.band.map(|x| x.start.get())), (Some(b.as_str()), false, Some(7e9)));
    // Undo again: A, now clean (it was saved after the first undo).
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!((s.path(), s.is_dirty()), (Some(a.as_str()), false));
}

/// Undo of file.new or file.sample brings back the project's file; an untitled project stays
/// without one, so a save after the undo asks for a path rather than writing someone else's.
#[test]
fn undoing_new_or_a_sample_brings_the_file_back() {
    let a = scratch("undo-new.emcraft");
    let mut s = saved(&a, 5.0);
    s.execute("file.new", &json!({})).unwrap();
    assert_eq!((s.path(), s.name().as_str()), (None, "Untitled"));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!((s.path(), s.is_dirty(), s.model.band.is_some()), (Some(a.as_str()), false, true));
    // An untitled project, a sample over it, undo: untitled again, no file to save to.
    let mut t = Session::new();
    t.execute("project.frequency", &json!({"start": 5, "stop": 9})).unwrap();
    t.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    assert_eq!((t.sample(), t.is_dirty()), (Some("highpass"), false));
    t.execute("edit.undo", &json!({})).unwrap();
    assert_eq!((t.sample(), t.path(), t.is_dirty()), (None, None, true));
    assert!(matches!(t.execute("file.save", &json!({})), Err(EngineError::Params(_))));
    // Undo went straight back to before the sample, not through the steps that built it.
    assert_eq!(t.undo().len(), 1);
}

/// The results that belonged to a project come back with it, current if they were.
#[test]
fn undoing_an_open_brings_back_the_projects_results() {
    let mut s = super::small();
    assert_eq!(s.solved_revision, Some(s.revision));
    let b = scratch("results-b.emcraft");
    saved(&b, 7.0);
    s.execute("file.open", &json!({"path": b})).unwrap();
    assert!(!s.results.any());
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.results.sweep.is_some());
    assert_eq!(s.solved_revision, Some(s.revision), "current again");
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(!s.results.any());
}

/// A solve records its run: the project is unsaved until it is saved; the file keeps the run
/// (not its results), opening restores it without running it, and solve.rerun runs it again.
#[test]
fn runs_belong_to_the_project() {
    let mut s = super::small();
    let first = s.runs().to_vec();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].command, "solve.frequency");
    let path = scratch("runs.emcraft");
    s.execute("file.save", &json!({"path": path})).unwrap();
    s.execute("solve.frequency", &json!({"frequencies": [8.0], "order": 1})).unwrap();
    assert!(s.is_dirty(), "a run with other parameters is unsaved");
    s.execute("file.save", &json!({})).unwrap();
    assert!(!s.is_dirty());
    let text = std::fs::read_to_string(&path).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    // Version 4: the solves are kept in the analysis history.
    assert_eq!(doc["version"], 4);
    assert_eq!(
        doc["runs"],
        json!([{"command": "solve.frequency", "params": {"frequencies": [8.0], "order": 1}, "units": {"length": "mm", "frequency": "GHz"}}])
    );
    // Opened: the run is back, nothing solved, clean.
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": path})).unwrap();
    assert!(!t.results.any() && !t.is_dirty());
    let runs = t.execute("project.runs", &json!({})).unwrap();
    assert_eq!(runs["latest"], "solve.frequency");
    assert_eq!(runs["runs"][0]["params"], json!({"frequencies": [8.0], "order": 1}));
    // In other units the parameters read in them; rerun solves at the same frequency.
    t.execute("units.set", &json!({"frequency": "MHz"})).unwrap();
    let runs = t.execute("project.runs", &json!({})).unwrap();
    assert_eq!(runs["runs"][0]["params"]["frequencies"], json!([8000.0]));
    let r = t.execute("solve.rerun", &json!({})).unwrap();
    assert_eq!(r["rerun"]["command"], "solve.frequency");
    assert!((t.results.sweep.as_ref().unwrap().results[0].frequency - 8e9).abs() < 1e-3);
    // The rerun is recorded in the units it ran in.
    assert_eq!(t.runs()[0].units.frequency, emcraft_model::units::Frequency::Megahertz);
    assert!(
        matches!(t.execute("solve.rerun", &json!({"command": "solve.eigenmodes"})), Err(EngineError::Params(m)) if m.contains("runs: solve.frequency"))
    );
    assert!(Session::new().execute("solve.rerun", &json!({})).is_err());
    // A failed solve records nothing.
    let before = t.runs().to_vec();
    assert!(t.execute("solve.eigenmodes", &json!({"near": -1})).is_err());
    assert_eq!(t.runs(), &before[..]);
}

/// A sample opened without its solve records the solve as its run all the same, so the Setup
/// step shows its settings.
#[test]
fn a_samples_solves_are_its_runs() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    assert!(!s.results.any());
    assert_eq!(s.runs().iter().map(|r| r.command.as_str()).collect::<Vec<_>>(), ["solve.frequency"]);
    assert!(!s.is_dirty());
}

/// A project file's runs are hostile: anything but a solve command, a run twice or unknown
/// parameters refuse the file; a version 1 file (no runs) opens.
#[test]
fn hostile_runs_in_a_file_are_refused() {
    let path = scratch("hostile-runs.emcraft");
    let mut s = Session::new();
    for (runs, why) in [
        (json!([{"command": "file.save", "params": {"path": "/tmp/x"}}]), "not a solve command"),
        (json!([{"command": "solve.frequency"}, {"command": "solve.frequency"}]), "run twice"),
        (json!([{"command": "solve.frequency", "params": {"bogus": 1}}]), "unknown parameter"),
        (json!([{"command": "solve.frequency", "params": [1]}]), "JSON object"),
        (json!([{"command": "solve.frequency", "units": {"length": "km", "frequency": "GHz"}}]), "not a project file"),
        (json!("x"), "not a project file"),
    ] {
        std::fs::write(&path, json!({"format": "emcraft-project", "version": 2, "history": [], "runs": runs}).to_string()).unwrap();
        let r = s.execute("file.open", &json!({"path": path}));
        assert!(matches!(&r, Err(EngineError::Failed(m)) if m.contains(why)), "{why}: {r:?}");
    }
    std::fs::write(&path, json!({"format": "emcraft-project", "version": 1, "history": [{"command": "units.set"}]}).to_string()).unwrap();
    s.execute("file.open", &json!({"path": path})).unwrap();
    assert!(s.runs().is_empty() && !s.is_dirty());
}

/// An autosave is a copy: the project keeps its file, name and unsaved state; recovering it
/// takes the original's file back and is unsaved where it differs from it.
#[test]
fn autosave_copies_and_recovery() {
    let (own, copy) = (scratch("autosave-own.emcraft"), scratch("autosave-copy.emcraft"));
    let mut s = saved(&own, 5.0);
    s.execute("model.material.create", &json!({"name": "ptfe", "eps_r": 2.1})).unwrap();
    let r = s.execute("file.save", &json!({"path": copy, "copy": true})).unwrap();
    assert_eq!(r["copy"], true);
    assert_eq!((s.path(), s.is_dirty(), s.name().as_str()), (Some(own.as_str()), true, "autosave-own"));
    assert!(!std::fs::read_to_string(&own).unwrap().contains("ptfe"));
    assert!(matches!(s.execute("file.save", &json!({"copy": true})), Err(EngineError::Params(_))));
    // Recovered: the copy's model under the original's file, unsaved.
    let mut t = Session::new();
    let r = t.execute("file.open", &json!({"path": copy, "original": own})).unwrap();
    assert_eq!((t.path(), t.is_dirty(), r["dirty"].as_bool()), (Some(own.as_str()), true, Some(true)));
    assert!(t.model.material("ptfe").is_ok());
    // An original that is gone: unsaved, whatever the copy holds.
    let mut u = Session::new();
    u.execute("file.open", &json!({"path": copy, "original": scratch("gone.emcraft")})).unwrap();
    assert!(u.is_dirty());
    assert!(u.execute("file.open", &json!({"path": copy, "original": " "})).is_err());
}

/// The kind of analysis chosen at the start: recorded (undo, the project file), reported, and
/// followed by the workflow's checks; until one is chosen it is inferred and the checks are as
/// before.
#[test]
fn the_analysis_kind_is_chosen_recorded_and_followed() {
    let mut s = super::small();
    let step = |s: &mut Session, id: &str| {
        let st = s.execute("project.status", &json!({})).unwrap();
        st["steps"].as_array().unwrap().iter().find(|x| x["step"] == id).cloned().unwrap()
    };
    // Inferred: a guide with two ports is swept; its results are current.
    let st = s.execute("project.status", &json!({})).unwrap();
    assert_eq!(st["analysis"], json!({"kind": "frequency", "chosen": false, "solve": "solve.frequency"}));
    assert_eq!(step(&mut s, "excitation")["status"], "done");
    // Eigenmodes: its ports are left out, a warning whose fix is the choice; the results stay.
    let r = s.execute("project.analysis", &json!({"kind": "eigenmodes"})).unwrap();
    assert_eq!(r, json!({"kind": "eigenmodes", "chosen": true, "solve": "solve.eigenmodes"}));
    assert_eq!(s.model.analysis, Some(emcraft_model::Analysis::Eigenmodes));
    let ex = step(&mut s, "excitation");
    assert_eq!((ex["status"].as_str(), ex["messages"][0]["command"].as_str()), (Some("warning"), Some("project.analysis")));
    assert_eq!(step(&mut s, "results")["status"], "done");
    assert_eq!(s.summary()["analysis"]["kind"], "eigenmodes");
    assert_eq!(s.execute("project.values", &json!({"command": "project.analysis"})).unwrap(), json!({"kind": "eigenmodes"}));
    // Radiation in a closed guide: the open boundary is to do, and Run waits for it.
    s.execute("project.analysis", &json!({"kind": "radiation"})).unwrap();
    let b = step(&mut s, "boundaries");
    assert_eq!((b["status"].as_str(), b["messages"][0]["command"].as_str()), (Some("todo"), Some("model.open")));
    let run = step(&mut s, "run");
    assert!(run["messages"][0]["text"].as_str().unwrap().contains("boundaries"), "{run}");
    // Hostile kinds are refused and change nothing.
    for bad in [json!("transient"), json!(5), json!(null), json!("x".repeat(10_000))] {
        assert!(s.execute("project.analysis", &json!({"kind": bad})).is_err(), "{bad}");
    }
    assert!(s.execute("project.analysis", &json!({})).is_err());
    assert_eq!(s.model.analysis, Some(emcraft_model::Analysis::Radiation));
    // Undo goes back a choice at a time.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.model.analysis, Some(emcraft_model::Analysis::Eigenmodes));
    // Saved and opened: the choice comes back with the history.
    let path = scratch("analysis.emcraft");
    s.execute("file.save", &json!({"path": path})).unwrap();
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": path})).unwrap();
    assert_eq!(t.model.analysis, Some(emcraft_model::Analysis::Eigenmodes));
    // A resonator chosen with nothing driving it: nothing to do in Excitation; a sweep chosen
    // instead: Run waits for a port.
    let mut c = Session::new();
    c.execute("geometry.waveguide", &json!({"width": 20, "height": 10, "sections": [{"length": 20, "material": "vacuum"}]})).unwrap();
    c.execute("mesh.generate", &json!({"size": 5})).unwrap();
    assert_eq!(step(&mut c, "excitation")["status"], "todo");
    c.execute("project.analysis", &json!({"kind": "eigenmodes"})).unwrap();
    assert_eq!(step(&mut c, "excitation")["status"], "done");
    assert_eq!(step(&mut c, "run")["messages"][0]["command"], "solve.eigenmodes");
    assert_eq!(c.execute("project.status", &json!({})).unwrap()["not_needed"], json!(["excitation"]));
    c.execute("project.analysis", &json!({"kind": "time_domain"})).unwrap();
    assert_eq!(step(&mut c, "run")["status"], "waiting");
    // The time domain grids the model itself: no mesh wanted.
    let mut td = Session::new();
    td.execute("geometry.waveguide", &json!({"width": 20, "height": 10, "sections": [{"length": 20, "material": "vacuum"}]})).unwrap();
    td.execute("model.port.create", &json!({"name": "1", "end": "start"})).unwrap();
    td.execute("model.port.create", &json!({"name": "2", "end": "end"})).unwrap();
    assert_eq!(step(&mut td, "mesh")["status"], "todo");
    td.execute("project.analysis", &json!({"kind": "time_domain"})).unwrap();
    assert_eq!(step(&mut td, "mesh")["status"], "done");
    assert_eq!(step(&mut td, "run")["status"], "todo");
    assert_eq!(step(&mut td, "run")["messages"][0]["command"], "solve.time_domain");
}

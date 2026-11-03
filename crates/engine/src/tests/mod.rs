//! The session's tests through its commands, by topic.

mod adapt;
mod designs;
mod geometry;
mod history;
mod invariants;
mod items;
mod junctions;
mod open;
mod pcb_fem;
mod project;
mod properties;
mod saved;
mod session;
mod sides;
mod solve;
mod symmetry;
mod view;
mod views;

use serde_json::json;

use crate::Session;

/// A path in the tests' scratch directory (under `EMCRAFT_SCRATCH` when set, else the system's
/// temporary directory).
pub(crate) fn scratch(name: &str) -> String {
    let base = std::env::var_os("EMCRAFT_SCRATCH").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = base.join(format!("emcraft-engine-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir.join(name).to_string_lossy().into_owned()
}

/// A project file's `project.json`.
pub(crate) fn project_json(path: &str) -> serde_json::Value {
    let mut r = emcraft_io::package::Reader::open(std::path::Path::new(path)).unwrap();
    serde_json::from_str(&r.text("project.json", 64 << 20).unwrap()).unwrap()
}

/// Writes a project file at `path` holding `doc` as its `project.json` and the other parts of
/// project file `from` (when given), as they are.
pub(crate) fn write_project(path: &str, doc: &serde_json::Value, from: Option<&str>) {
    let mut w = emcraft_io::package::Writer::create(std::path::Path::new(path)).unwrap();
    if let Some(from) = from {
        let mut r = emcraft_io::package::Reader::open(std::path::Path::new(from)).unwrap();
        for name in r.names().into_iter().filter(|n| n != "project.json") {
            let bytes = r.bytes(&name, 1 << 30).unwrap();
            w.raw(&name, &bytes).unwrap();
        }
    }
    w.text("project.json", &doc.to_string()).unwrap();
    w.finish().unwrap();
}

/// As [`write_project`] from `from`, part `name` replaced by `bytes`.
pub(crate) fn write_project_with(path: &str, doc: &serde_json::Value, from: &str, name: &str, bytes: &[u8]) {
    let mut w = emcraft_io::package::Writer::create(std::path::Path::new(path)).unwrap();
    let mut r = emcraft_io::package::Reader::open(std::path::Path::new(from)).unwrap();
    for part in r.names().into_iter().filter(|n| n != "project.json" && n != name) {
        let b = r.bytes(&part, 1 << 30).unwrap();
        w.raw(&part, &b).unwrap();
    }
    w.raw(name, bytes).unwrap();
    w.text("project.json", &doc.to_string()).unwrap();
    w.finish().unwrap();
}

/// The `project.json` of an empty project, its `extra` keys set.
pub(crate) fn bare_project(extra: serde_json::Value) -> serde_json::Value {
    let mut doc = json!({"format": "emcraft-project", "version": "0.1", "emcraft": "0.0.0", "history": [], "runs": [], "views": {},
                         "analysis_history": {"next": 0, "histories": [], "solves": []}, "results": {"current": false, "fields": true}});
    if let (Some(d), Some(e)) = (doc.as_object_mut(), extra.as_object()) {
        d.extend(e.clone());
    }
    doc
}

/// The template `name`'s script run with its mesh at `size` (mm) and without its solve: the
/// model `file.sample` builds, without meshing it at the template's own size first.
fn template_meshed(name: &str, size: f64) -> Session {
    let script: Vec<serde_json::Value> = serde_json::from_str(emcraft_templates::find(name).unwrap().script).unwrap();
    let mut s = Session::new();
    for step in script {
        let command = step["command"].as_str().unwrap();
        if command.starts_with("solve.") {
            continue;
        }
        let params = if command == "mesh.generate" { json!({"size": size}) } else { step["params"].clone() };
        s.execute(command, &params).unwrap();
    }
    s
}

/// The sample with a coarse mesh and two frequencies, for speed.
fn small() -> Session {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    s.execute("mesh.generate", &json!({"size": 5})).unwrap();
    s.execute("solve.frequency", &json!({"frequencies": [5.5, 8.0], "order": 2})).unwrap();
    s
}

mod pcb;

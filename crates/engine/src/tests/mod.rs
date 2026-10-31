//! The session's tests through its commands, by topic.

mod adapt;
mod designs;
mod geometry;
mod invariants;
mod items;
mod junctions;
mod open;
mod project;
mod session;
mod sides;
mod solve;
mod view;
mod views;

use serde_json::json;

use crate::Session;

/// A path in the tests' scratch directory.
pub(crate) fn scratch(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("emcraft-engine-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir.join(name).to_string_lossy().into_owned()
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

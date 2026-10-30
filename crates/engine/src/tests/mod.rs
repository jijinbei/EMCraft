//! The session's tests through its commands, by topic.

mod adapt;
mod designs;
mod geometry;
mod items;
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

/// The sample with a coarse mesh and two frequencies, for speed.
fn small() -> Session {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    s.execute("mesh.generate", &json!({"size": 5})).unwrap();
    s.execute("solve.frequency", &json!({"frequencies": [5.5, 8.0], "order": 2})).unwrap();
    s
}

//! Scripts: lists of commands with their parameters, as a session's history records them, a
//! project file stores them and `emcraft-cli run` reads them.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The command that defines a variable (`project.variable`).
pub(crate) const DEFINE: &str = "project.variable";

/// One step of a script: a command and its parameters (`{}` when a script leaves them out).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub command: String,
    #[serde(default = "no_params")]
    pub params: Value,
}

fn no_params() -> Value {
    json!({})
}

impl Step {
    pub fn new(command: impl Into<String>, params: Value) -> Step {
        Step { command: command.into(), params }
    }

    /// The parameter `name` as the step gives it.
    pub fn param(&self, name: &str) -> Option<&Value> {
        self.params.get(name)
    }

    /// Whether the step defines variable `name` (`project.variable` with that `name`).
    pub fn defines(&self, name: &str) -> bool {
        self.command == DEFINE && self.param("name").and_then(Value::as_str) == Some(name)
    }
}

//! The browser: its rows ([`tree`]) and the item of the model a row names, with the commands
//! that edit and delete it.

mod tree;

pub use tree::*;

use serde::Deserialize;
use serde_json::Value;

/// An item of the model a row names, with the commands that edit and delete it.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Item {
    /// Its edit command (its dialog opens on `project.values`).
    pub edit: &'static str,
    pub delete: &'static str,
    /// The parameter naming it in both (`name`, `body`, `index`) and its value.
    pub key: &'static str,
    pub value: serde_json::Value,
}

impl Item {
    pub fn new(edit: &'static str, delete: &'static str, key: &'static str, value: serde_json::Value) -> Item {
        Item { edit, delete, key, value }
    }

    /// The parameters naming it: `{"name": "ptfe"}`.
    pub fn params(&self) -> serde_json::Value {
        let mut o = serde_json::Map::new();
        o.insert(self.key.to_owned(), self.value.clone());
        serde_json::Value::Object(o)
    }
}

/// An item read back (a recorded action): its commands and parameter the command table's own.
impl<'de> Deserialize<'de> for Item {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Written {
            edit: String,
            delete: String,
            key: String,
            value: Value,
        }
        let w = Written::deserialize(d)?;
        let bad = |what: &str, s: &str| serde::de::Error::custom(format!("no {what} `{}`", emcraft_engine::text::clip(s, 64)));
        let command = |s: &str| match s {
            "" => Some(""),
            s => emcraft_engine::find_command(s).map(|c| c.id),
        };
        let param = emcraft_engine::COMMANDS.iter().flat_map(|c| c.params.iter()).map(|p| p.name).find(|n| *n == w.key);
        Ok(Item {
            edit: command(&w.edit).ok_or_else(|| bad("command", &w.edit))?,
            delete: command(&w.delete).ok_or_else(|| bad("command", &w.delete))?,
            key: param.ok_or_else(|| bad("parameter", &w.key))?,
            value: w.value,
        })
    }
}

/// A face port's plane: its axis and where it lies across it, in display units `l`, to a
/// thousandth (none for an end port).
pub fn face_place(p: &emcraft_model::Port, l: emcraft_model::units::Length) -> (&'static str, String) {
    match p.face {
        Some(f) => (f.axis.id(), format!("{}", (l.from_si(f.at) * 1e3).round() / 1e3 + 0.0)),
        None => ("", String::new()),
    }
}

//! A system's stages as the window shows them ([`emcraft_ui_core::system`]), and the item a
//! stage's row in the browser edits and deletes.

pub use emcraft_ui_core::system::*;
use serde_json::json;

/// The stage `name` as an item of the model, which its row edits and deletes.
pub fn item(name: &str) -> crate::browser::Item {
    crate::browser::Item::new("system.stage.edit", "system.stage.delete", "stage", json!(name))
}

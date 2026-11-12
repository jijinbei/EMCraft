//! Autosave and recovery ([`emcraft_ui_core::autosave`]): the egui window asks each frame
//! whether a copy is due, writes it, and reports a failure once.

pub use emcraft_ui_core::autosave::*;
use serde_json::Value;

use crate::EmcraftApp;

impl EmcraftApp {
    /// Writes a copy of the project when its changes have been unsaved for `every` seconds
    /// (at input time `now`, s), unless a job runs. Called once a frame.
    pub(crate) fn autosave_tick(&mut self, now: f64) {
        let (dirty, busy) = (self.session.is_dirty(), self.busy());
        if self.autosave.due(now, dirty, busy) {
            let _ = self.autosave_now();
        }
    }

    /// Writes a copy of the project now (when there is a folder): the reply of `file.save`, or
    /// why not.
    pub fn autosave_now(&mut self) -> Result<Value, String> {
        let (r, new_failure) = self.autosave.write(&mut self.session);
        match &r {
            Ok(_) => {
                // The crash journal begins again from the project the copy holds.
                let history = serde_json::to_value(self.session.history()).unwrap_or_else(|_| serde_json::json!([]));
                if let Some(j) = self.crashes.journal.as_mut() {
                    j.restart(&history);
                }
            }
            Err(e) if new_failure => self.error_with(t!("Autosave failed: {error}"), crate::lang::args!(error = e)),
            Err(_) => {}
        }
        r
    }

    /// The project's changes were saved or let go: its copy goes.
    pub(crate) fn autosave_drop(&mut self) {
        self.autosave.drop_copy();
    }

    /// Lets a copy found on start go: it is deleted.
    pub(crate) fn dismiss_recovery(&mut self, i: usize) {
        self.autosave.dismiss(i);
    }

    /// The autosave's state for `ui.inspect`.
    pub fn autosave_inspect(&self) -> Value {
        self.autosave.inspect()
    }
}

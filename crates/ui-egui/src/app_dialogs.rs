//! The window opening command dialogs: on the model's values or an item's, and OK.

use super::*;

impl EmcraftApp {
    /// Opens the dialog of `id` (commands without parameters run at once).
    pub fn open_dialog(&mut self, id: &str) -> Result<(), UiError> {
        let spec = find_command(id)?;
        if spec.params.is_empty() {
            return self.run(id, json!({})).map(|_| ());
        }
        self.ui_state.open_dialog(dialogs::open(spec, &mut self.session, &json!({}), self.dock_dialogs));
        Ok(())
    }

    /// Opens an item's edit dialog on its current values: OK unchanged runs nothing.
    pub fn open_item_dialog(&mut self, item: &browser::Item) -> Result<(), UiError> {
        let d = dialogs::edit(item, &mut self.session, self.dock_dialogs)?;
        self.ui_state.open_dialog(d);
        Ok(())
    }

    /// Opens the dialog of `id` with `values` over its usual initial ones (the view's settings
    /// for an export, say).
    pub fn open_dialog_with(&mut self, id: &str, values: Value) -> Result<(), UiError> {
        let spec = find_command(id)?;
        self.ui_state.open_dialog(dialogs::open(spec, &mut self.session, &values, self.dock_dialogs));
        Ok(())
    }

    /// OK on the open dialog: runs its command with its parameters; the dialog stays open with
    /// the error when they are refused.
    pub fn dialog_ok(&mut self) -> Result<Value, UiError> {
        let Some(d) = self.ui_state.dialog_mut() else { return Err(UiError::NoDialog) };
        let id = d.command.id;
        let params = match d.ok().map_err(UiError::Input)? {
            dialogs::Outcome::Run(p) => p,
            // The model's own values, unchanged: nothing to run.
            dialogs::Outcome::Unchanged => {
                self.ui_state.take_dialog();
                return Ok(json!({"unchanged": true}));
            }
        };
        match self.run(id, params) {
            Ok(v) => {
                // A long command's dialog comes back with the error if its job fails.
                let d = self.ui_state.take_dialog().filter(|_| commands::is_long(id));
                if let Some(job) = self.job.as_mut() {
                    job.dialog = d;
                }
                Ok(v)
            }
            Err(e) => {
                if let Some(d) = self.ui_state.dialog_mut() {
                    d.refused(&e.to_string());
                }
                Err(e)
            }
        }
    }
}

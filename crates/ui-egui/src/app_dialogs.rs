//! The window opening command dialogs: on the model's values or an item's, and OK.

use super::*;

impl EmcraftApp {
    /// Opens the dialog of `id` (commands without parameters run at once).
    pub fn open_dialog(&mut self, id: &str) -> Result<(), UiError> {
        let spec = find_command(id)?;
        if spec.params.is_empty() {
            return self.run(id, json!({})).map(|_| ());
        }
        let (init, current) = self.initial(id, &json!({}));
        let d = dialogs::Dialog::new(spec, &self.session, &init, self.dock_dialogs);
        self.ui_state.open_dialog(if current && dialogs::skips_unchanged(id) { d.on_model() } else { d });
        Ok(())
    }

    /// What `id`'s dialog opens on: the model's current state (`project.values`, asked with
    /// `select`'s parameters, a symmetry plane's `axis` say) for the commands that have one
    /// ([`dialogs::ON_VALUES`]; perfect walls are the empty one), else the examples of
    /// [`dialogs::initial`]; and whether it is the model's.
    pub(crate) fn initial(&mut self, id: &str, select: &Value) -> (Value, bool) {
        let mut ask = select.as_object().cloned().unwrap_or_default();
        ask.insert("command".into(), json!(id));
        if dialogs::ON_VALUES.contains(&id)
            && let Ok(v) = self.session.execute("project.values", &Value::Object(ask))
            && (v.as_object().is_some_and(|o| !o.is_empty()) || id == "model.walls")
        {
            return (v, true);
        }
        (dialogs::initial(id, &self.session), false)
    }

    /// Opens an item's edit dialog on its current values: OK unchanged runs nothing.
    pub fn open_item_dialog(&mut self, item: &browser::Item) -> Result<(), UiError> {
        let spec = find_command(item.edit)?;
        let mut ask = item.params();
        ask["command"] = json!(item.edit);
        let values = self.session.execute("project.values", &ask)?;
        self.ui_state.open_dialog(dialogs::Dialog::new(spec, &self.session, &values, self.dock_dialogs).on_model());
        Ok(())
    }

    /// Opens the dialog of `id` with `values` over its usual initial ones (the view's settings
    /// for an export, say).
    pub fn open_dialog_with(&mut self, id: &str, values: Value) -> Result<(), UiError> {
        let spec = find_command(id)?;
        let (mut init, current) = self.initial(id, &values);
        if let (Some(o), Some(v)) = (init.as_object_mut(), values.as_object()) {
            o.extend(v.clone());
        } else {
            init = values;
        }
        let d = dialogs::Dialog::new(spec, &self.session, &init, self.dock_dialogs);
        self.ui_state.open_dialog(if current && dialogs::skips_unchanged(id) { d.on_model() } else { d });
        Ok(())
    }

    /// OK on the open dialog: runs its command with its parameters; the dialog stays open with
    /// the error when they are refused.
    pub fn dialog_ok(&mut self) -> Result<Value, UiError> {
        let Some(d) = self.ui_state.dialog_mut() else { return Err(UiError::NoDialog) };
        let id = d.command.id;
        let params = match d.params() {
            Ok(p) => p,
            Err(e) => {
                d.error = Some(e.clone());
                return Err(UiError::Input(e));
            }
        };
        // The model's own values, unchanged: nothing to run.
        if d.unchanged.as_ref() == Some(&params) {
            self.ui_state.take_dialog();
            return Ok(json!({"unchanged": true}));
        }
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

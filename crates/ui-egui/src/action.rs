//! How this window performs what a frame asks of it ([`emcraft_ui_core::action`]): drawing pushes
//! [`Action`]s into the frame's [`Actions`]; once the frame is drawn, [`EmcraftApp::apply`]
//! performs each in turn.

pub use emcraft_ui_core::action::{Action, Actions, Pref};
use serde_json::{Value, json};

use crate::lang::args;
use crate::{EmcraftApp, UiError, View};

impl UiError {
    /// Whether the error was already put in the message area where it arose: a command's
    /// failure (reported by `run`; an unknown command is refused before it runs) and Run's
    /// refusal.
    fn reported(&self) -> bool {
        match self {
            UiError::Engine(e) => !matches!(e, emcraft_engine::EngineError::UnknownCommand(_)),
            UiError::Refused(_) => true,
            _ => false,
        }
    }
}

impl EmcraftApp {
    /// Performs the actions a frame asked for; what goes wrong goes to the message area.
    pub fn apply_all(&mut self, actions: Actions) {
        // A row's highlight in 3D lasts while the row is hovered: one frame at a time.
        self.view3d.hover = None;
        for a in actions {
            let report = a.reports_errors();
            // A refusal while a job runs is said, whatever asked for it.
            if let Err(e) = self.apply(a)
                && (report || matches!(e, UiError::Busy(_)))
                && !e.reported()
            {
                self.error(e);
            }
        }
    }

    /// Performs one action: the reply of a command it runs (`{"started": id}` for a long one),
    /// else what it changed.
    pub fn apply(&mut self, action: Action) -> Result<Value, UiError> {
        // What is asked for is recorded (crate::record), not what follows from it.
        self.record(&action);
        self.recorder.depth += 1;
        let r = self.perform(action);
        self.recorder.depth -= 1;
        r
    }

    fn perform(&mut self, action: Action) -> Result<Value, UiError> {
        // One rule for a running job: what waits for it is refused here, and greyed out where
        // it is drawn (`UiExt::act`).
        if action.waits_for_job() {
            self.idle()?;
        }
        // The window's own transitions (crate::ui_state), and what follows from them.
        if let Some(then) = self.ui_state.update(&action) {
            for a in then {
                self.apply(a)?;
            }
            return Ok(json!({}));
        }
        match action {
            Action::Run { id, params } => return self.run(&id, params),
            Action::RunAll(list) => return self.run_all(list),
            Action::OpenDialog { id, values } => match values {
                Some(v) => self.open_dialog_with(&id, v)?,
                None => self.open_dialog(&id)?,
            },
            Action::DialogOk => return self.dialog_ok(),
            Action::SetDock(on) => self.set_dock(on),
            Action::SetView(v) => self.place = self.place.show(v),
            // Home's tab is step 0.
            Action::ShowView(v) => self.show_view(v)?,
            Action::SetTab(t) => self.tab = t,
            Action::OpenStep(step) => crate::steps::open_step(self, step),
            Action::Fix(command) => crate::steps::fix(self, &command)?,
            Action::Fit => self.view3d.goal = Some(crate::view3d::Goal::Fit),
            Action::ShowReports => self.show_reports()?,
            Action::DeleteReport(id) => {
                self.delete_report(&id)?;
                self.show_reports()?;
            }
            Action::ReportBug => {
                self.report_bug()?;
                self.show_reports()?;
            }
            Action::EditSelected => return self.edit_selected(),
            Action::Fold(section) => self.fold(section),
            Action::Hover(pick) => self.view3d.hover = Some(pick),
            Action::FocusMode(focus) => self.workflow.mode_focus = Some(focus),
            Action::Undo => return self.run("edit.undo", json!({})),
            Action::Redo => return self.run("edit.redo", json!({})),
            Action::CancelJob => return Ok(json!({"cancelled": self.cancel_job()})),
            // The window's own transitions, made above (crate::ui_state).
            Action::DialogCancel
            | Action::OpenSearch
            | Action::Show(_)
            | Action::OpenSettings(_)
            | Action::CloseSettings
            | Action::Click(_)
            | Action::Select { .. }
            | Action::SelectBox { .. } => {}
            Action::Close(layer) => return self.close_layer(layer),
            Action::Purpose(kind) => self.home.purpose = kind,
            Action::Template(name) => self.show_template(name),
            Action::OpenTemplate(name, values) => crate::template_dialog::open(self, name, values),
            Action::Pref(p) => self.set_pref(p),
            Action::File(f) => crate::files::apply(self, f),
            Action::EditItem(item) => self.open_item_dialog(&item)?,
            Action::DeleteItems(items) => return self.delete_items(items),
            Action::DismissRecovery(i) => self.dismiss_recovery(i),
            Action::AskExport => self.ui_state.set_ask(crate::files::Ask::Export("emcraft-history.json".into())),
            Action::ExportHistory(path) => self.export_history(path),
            Action::ConfirmSetup => return crate::steps::confirm_setup(self),
            Action::ProposeSetup(v) => self.workflow.fields = std::mem::take(&mut self.workflow.fields).load(&v),
            Action::StartRun => return crate::steps::start_run(self),
            Action::PreviewPortModes => return self.preview_port_modes(),
            Action::CheckResults => return self.check_results(),
            // Read quietly: what keeps it from reading (no band yet) shows on the canvas.
            Action::AutoSolve => {
                self.auto_solved = Some(self.revision);
                let params = json!({});
                match self.session.execute("solve.system", &params) {
                    Ok(reply) => {
                        self.auto_error = None;
                        self.after("solve.system", &params, &reply);
                    }
                    Err(e) => self.auto_error = Some(e.to_string()),
                }
            }
            Action::ExportTouchstone => crate::steps::export_touchstone(self),
            Action::SetDevice { device, precision } => self.set_device(device, precision),
            Action::StandardView(v) => self.view3d.standard(v),
            Action::SetPlaying(on) => self.field.player.playing = on,
            Action::Tool(tool) => self.set_tool(tool),
            Action::Pen(pen) => self.set_pen(pen),
            Action::Measure(picks) => return self.measure(picks),
            Action::Message { text, error: true } => self.error(text),
            Action::Message { text, error: false } => self.info(text),
        }
        Ok(Value::Null)
    }

    fn set_dock(&mut self, on: bool) {
        self.dock_dialogs = on;
        if let Some(d) = self.ui_state.dialog_mut() {
            d.docked = on;
        }
    }

    /// Shows view `v`; the port modes' view, with no modes yet and a mesh, opens their dialog.
    fn show_view(&mut self, v: View) -> Result<(), UiError> {
        self.place = self.place.show(v);
        if v == View::PortModes && self.replies.port_modes.is_none() && self.session.model().mesh.is_some() && !self.busy() {
            self.open_dialog("solve.port_modes")?;
        }
        Ok(())
    }

    /// Edits the one row selected, as its double-click does.
    fn edit_selected(&mut self) -> Result<Value, UiError> {
        let row = self.ui_state.selection().single().and_then(|p| crate::browser::rows_of(self, &[p]).into_iter().next());
        match row.and_then(|(_, r)| crate::browser::RowExt::edit_action(&r)) {
            Some(a) => self.apply(a),
            None => Ok(Value::Null),
        }
    }

    /// Folds the browser's `section`, or unfolds it when folded.
    fn fold(&mut self, section: String) {
        if self.browser.folded.contains(&section) {
            self.browser.folded.retain(|f| *f != section);
        } else {
            self.browser.folded.push(section);
        }
    }

    fn close_layer(&mut self, layer: crate::ui_state::Layer) -> Result<Value, UiError> {
        match crate::layers::close(self, layer) {
            Some(then) => self.apply(then),
            None => Ok(Value::Null),
        }
    }

    /// Shows the template's card (none for no name or an unknown one).
    fn show_template(&mut self, name: Option<String>) {
        if let Some(card) = name.and_then(|n| crate::template_dialog::TemplateCard::new(&self.session, &n)) {
            self.ui_state.open_template(card);
        }
    }

    /// Deletes the items (the selection goes with them): one reply, or a list of them. An
    /// imported part's walls are not deleted: their material changes.
    fn delete_items(&mut self, items: Vec<crate::browser::Item>) -> Result<Value, UiError> {
        self.ui_state.select(crate::selection::Selection::default());
        let mut replies = Vec::with_capacity(items.len());
        for item in items.into_iter().filter(|i| !i.delete.is_empty()) {
            replies.push(self.run(item.delete, item.params())?);
        }
        Ok(if replies.len() == 1 { replies.swap_remove(0) } else { Value::Array(replies) })
    }

    /// Solves the port modes when the band is set, else opens their dialog.
    fn preview_port_modes(&mut self) -> Result<Value, UiError> {
        if self.session.model().job.range().is_some() {
            return self.run("solve.port_modes", json!({}));
        }
        self.open_dialog("solve.port_modes")?;
        Ok(Value::Null)
    }

    /// Checks the results, the reply kept with the revision checked.
    fn check_results(&mut self) -> Result<Value, UiError> {
        let v = self.run("results.check", json!({}))?;
        self.workflow.check = Some((self.revision, v.clone()));
        Ok(v)
    }

    /// Chooses the solver's device and precision; a refusal shows as a failure.
    fn set_device(&mut self, device: emcraft_engine::Device, precision: emcraft_engine::Precision) {
        if let Err(e) = self.run("solve.device", json!({"device": device.id(), "precision": precision.id()})) {
            self.failure = Some(crate::Failure { command: "solve.device".into(), error: e.to_string() });
        }
    }

    /// Takes up a 3D tool (or puts it down): the model shown, and the excitation step for the
    /// port tools.
    fn set_tool(&mut self, tool: Option<crate::tools3d::Tool>) {
        self.ui_state.set_tool(tool);
        self.view3d.tools.reset(tool);
        if tool.is_some() {
            self.place = self.place.show(View::Model);
            self.ui_state.page.show(crate::page::Tab::Project);
        }
        if tool.is_some_and(crate::tools3d::Tool::ports) && self.place.step() != Some(crate::steps::Step::Excitation) {
            crate::steps::open_step(self, Some(crate::steps::Step::Excitation));
        }
    }

    /// Takes up the board's pen (or puts it down: a line begun goes with it).
    fn set_pen(&mut self, pen: Option<crate::state::Pen>) {
        self.ui_state.set_drawing(pen.is_some());
        self.views.pcb.pen = pen.unwrap_or_else(|| crate::state::Pen { from: None, ..std::mem::take(&mut self.views.pcb.pen) });
    }

    /// Measures between the picks; the reply kept to draw.
    fn measure(&mut self, picks: [crate::tools3d::Snap; 2]) -> Result<Value, UiError> {
        let params = crate::tools3d::measure_params(self.session.display().length, picks);
        let reply = self.run("geometry.measure", params)?;
        self.view3d.tools.measured = Some(crate::tools3d::Measured { picks, reply: reply.clone() });
        Ok(reply)
    }

    fn set_pref(&mut self, p: Pref) {
        match p {
            Pref::Theme(name) => match self.themes.find(&name) {
                Some(_) => self.theme = name,
                None => self.error(emcraft_engine::text::clip(&format!("no theme `{name}` (there are {})", self.themes.names().join(", ")), 400)),
            },
            Pref::Language(id) => match crate::lang::find(&id) {
                Some(l) => {
                    l.id.clone_into(&mut self.prefs.language);
                    self.language = if self.fonts.shows(l.id) { l.id } else { crate::lang::ENGLISH };
                    self.use_theme();
                }
                None => self.error(format!("no language `{}` (there are {})", crate::lang::clip_id(&id), crate::lang::ids().join(", "))),
            },
            Pref::FontFile(file) => {
                self.prefs.font_file = file.map(|f| f.chars().take(4096).collect());
                self.fonts.set_file(self.prefs.font_file.as_ref().map(std::path::PathBuf::from));
                if self.fonts.file_loaded() == Some(false) {
                    self.error_with(t!("No font in {path}"), args!(path = self.prefs.font_file.clone().unwrap_or_default()));
                }
            }
            Pref::SearchFonts => self.fonts.search_again(),
            Pref::ReloadThemes => self.load_themes(self.themes.dir.clone()),
            p @ (Pref::Navigation(_)
            | Pref::OrbitAround(_)
            | Pref::Navigator(_)
            | Pref::GpuLook(_)
            | Pref::Length(_)
            | Pref::Frequency(_)
            | Pref::ForgetRecent(_)
            | Pref::ClearRecent) => match self.prefs.clone().changed(p, &self.themes) {
                Ok(prefs) => self.prefs = prefs,
                Err(e) => self.error(e),
            },
        }
    }

    /// Writes the history as a script (the session's model-changing commands, which rebuild the
    /// model: `cargo run -p emcraft-cli -- run FILE`); asks again on failure.
    fn export_history(&mut self, path: String) {
        // The prompt answered closes (open again on a failure, with the path).
        self.ui_state.take_ask();
        let text = serde_json::to_string_pretty(self.session.history()).unwrap_or_else(|_| "[]".into());
        match std::fs::write(path.trim(), text) {
            Ok(()) => {
                let n = self.session.history().len();
                self.info_with(t!("Exported {n} commands to {path}."), args!(n = n, path = path.trim()));
            }
            Err(e) => {
                self.error_with(t!("Cannot write {path}: {error}"), args!(path = path.trim(), error = e));

                self.ui_state.set_ask(crate::files::Ask::Export(path));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ribbon::RibbonTab;
    use crate::scene3d::Pick;

    fn frame(app: &mut EmcraftApp, actions: impl IntoIterator<Item = Action>) {
        let mut out = Actions::default();
        out.extend(actions);
        app.apply_all(out);
    }

    #[test]
    fn a_frame_s_actions_are_performed_in_order_and_report_where_they_should() {
        let mut app = EmcraftApp::new();
        let before = app.messages.len();
        frame(
            &mut app,
            [Action::dialog("geometry.waveguide"), Action::SetDock(true), Action::Fold("Bodies".into()), Action::SetTab(RibbonTab::Mesh)],
        );
        assert_eq!(app.ui_state.dialog().map(|d| (d.command.id, d.docked)), Some(("geometry.waveguide", true)));
        assert_eq!((app.dock_dialogs, app.tab, app.browser.folded.clone()), (true, RibbonTab::Mesh, vec!["Bodies".to_owned()]));
        // A refused OK shows under the dialog's fields, not in the messages.
        if let Some(d) = app.ui_state.dialog_mut() {
            let _ = d.set("width", &json!("wide"));
        }
        frame(&mut app, [Action::DialogOk]);
        assert!(app.ui_state.dialog().is_some_and(|d| d.fields.iter().any(|f| f.error.is_some())));
        assert_eq!(app.messages.len(), before);
        // A dialog that cannot open says why; the next action still runs.
        frame(&mut app, [Action::dialog("no.such"), Action::Fold("Bodies".into())]);
        assert_eq!(app.messages.len(), before + 1);
        assert!(app.status().is_some_and(|m| m.error && m.text.contains("no.such")));
        assert!(app.browser.folded.is_empty());
    }

    #[test]
    fn a_hover_lasts_one_frame() {
        let mut app = EmcraftApp::new();
        frame(&mut app, [Action::Hover(Pick::Port(0))]);
        assert_eq!(app.view3d.hover, Some(Pick::Port(0)));
        frame(&mut app, []);
        assert_eq!(app.view3d.hover, None);
    }
}

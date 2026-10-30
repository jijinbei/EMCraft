//! What a frame asks of the app. Drawing code reads the app and pushes [`Action`]s into the
//! frame's [`Actions`]; once the frame is drawn, [`EmcraftApp::apply`] performs each in turn.
//! The control channel and the keyboard shortcuts ask through the same actions, so a click, a
//! request and a key do a thing one way.

use emcraft_model::units::{Frequency, Length};
use emcraft_render::view3d::StandardView;
use serde_json::{Value, json};

use crate::files::FileAction;
use crate::lang::args;
use crate::ribbon::RibbonTab;
use crate::scene3d::Pick;
use crate::steps::{ModeFocus, Step};
use crate::{EmcraftApp, UiError, View};

/// Something a person (a click, a key), an agent or a test asks of the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Runs an engine command: a quick one at once, a long one on the worker thread.
    Run {
        id: String,
        params: Value,
    },
    /// Opens a command's dialog, `values` over its usual initial ones (a command without
    /// parameters runs at once).
    OpenDialog {
        id: String,
        values: Option<Value>,
    },
    /// OK on the open dialog: runs its command; refused, the dialog stays with the error.
    DialogOk,
    DialogCancel,
    /// Dialogs open docked at the right (`true`) or floating, the open one too.
    SetDock(bool),
    /// The viewport shows a view.
    SetView(View),
    /// A view's tab clicked: the view, its port modes solved first when it has none.
    ShowView(View),
    SetTab(RibbonTab),
    /// Opens a workflow step's panel (`None` closes it).
    OpenStep(Option<Step>),
    /// A check's fix: the step that owns `command`, with its dialog.
    Fix(String),
    /// A browser row (its key `section/text`) or a part in 3D selected; `None` clears.
    Select {
        row: Option<String>,
        pick: Option<Pick>,
    },
    /// Folds or unfolds a browser section.
    Fold(String),
    /// A row elsewhere points at a part of the 3D view: highlighted until the next frame.
    Hover(Pick),
    /// The port mode the Port modes view marks.
    FocusMode(ModeFocus),
    Undo,
    Redo,
    /// Cancels the running job.
    CancelJob,
    /// Shows (`true`) or leaves the start page; either leaves the settings page.
    SetHome(bool),
    /// Opens the settings page at a section (`None` closes it).
    Settings(Option<crate::settings::Section>),
    Pref(Pref),
    File(FileAction),
    /// Asks where to export the history as a script.
    AskExport,
    /// Writes the history as a script to a path.
    ExportHistory(String),
    /// The Intent step's band, from its fields.
    ApplyBand,
    /// Run: the solver from the Setup step's fields, after the preflight.
    StartRun,
    /// The Excitation step's port-mode preview.
    PreviewPortModes,
    /// The Results step's passivity and reciprocity check.
    CheckResults,
    /// The Results step's Touchstone export, from its fields.
    ExportTouchstone,
    /// The device (and its precision) the solvers run on.
    SetDevice {
        device: String,
        precision: String,
    },
    /// The 3D view from a standard side, fitted.
    StandardView(StandardView),
    /// The field's phase animation plays or pauses.
    SetPlaying(bool),
    /// A message in the message area.
    Message {
        text: String,
        error: bool,
    },
}

/// A preference set from the settings.
#[derive(Clone, Debug, PartialEq)]
pub enum Pref {
    /// The theme, by name.
    Theme(String),
    /// The window's language, by id (`en`, `ja`).
    Language(String),
    /// Reads the themes again (the user's from their folder).
    ReloadThemes,
    /// The 3D view drawn by the window's GPU.
    Gpu3d(bool),
    Length(Length),
    Frequency(Frequency),
    /// Drops a project from the recent list.
    ForgetRecent(usize),
    ClearRecent,
}

impl Action {
    /// Runs command `id` with `params`.
    pub fn run(id: &str, params: Value) -> Action {
        Action::Run { id: id.to_owned(), params }
    }

    /// Opens the dialog of command `id`.
    pub fn dialog(id: &str) -> Action {
        Action::OpenDialog { id: id.to_owned(), values: None }
    }

    /// Opens the dialog of command `id` with `values` over its usual ones.
    pub fn dialog_with(id: &str, values: Value) -> Action {
        Action::OpenDialog { id: id.to_owned(), values: Some(values) }
    }

    /// An error in the message area.
    pub fn error(text: impl std::fmt::Display) -> Action {
        Action::Message { text: text.to_string(), error: true }
    }

    /// Whether [`EmcraftApp::apply_all`] puts what goes wrong in the message area. The others
    /// report it where it arises (a command's failure, by `run`) or show it in place (a
    /// dialog's refusal, under its field; a step's field errors).
    fn reports_errors(&self) -> bool {
        matches!(self, Action::OpenDialog { .. } | Action::ShowView(_) | Action::Fix(_) | Action::PreviewPortModes | Action::CheckResults)
    }
}

/// The actions a frame asked for, in order.
#[derive(Debug, Default)]
pub struct Actions(Vec<Action>);

impl Actions {
    pub fn push(&mut self, a: Action) {
        self.0.push(a);
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Action> {
        self.0.iter()
    }
}

impl Extend<Action> for Actions {
    fn extend<I: IntoIterator<Item = Action>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

impl IntoIterator for Actions {
    type Item = Action;
    type IntoIter = std::vec::IntoIter<Action>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

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
            if let Err(e) = self.apply(a)
                && report
                && !e.reported()
            {
                self.error(e);
            }
        }
    }

    /// Performs one action: the reply of a command it runs (`{"started": id}` for a long one),
    /// else what it changed.
    pub fn apply(&mut self, action: Action) -> Result<Value, UiError> {
        match action {
            Action::Run { id, params } => return self.run(&id, params),
            Action::OpenDialog { id, values } => match values {
                Some(v) => self.open_dialog_with(&id, v)?,
                None => self.open_dialog(&id)?,
            },
            Action::DialogOk => return self.dialog_ok(),
            Action::DialogCancel => {
                return Ok(json!({"closed": self.dialog.take().is_some()}));
            }
            Action::SetDock(on) => {
                self.dock_dialogs = on;
                if let Some(d) = self.dialog.as_mut() {
                    d.docked = on;
                }
            }
            Action::SetView(v) => self.view = v,
            Action::ShowView(v) => {
                self.view = v;
                if v == View::PortModes && self.replies.port_modes.is_none() && self.session.model().mesh.is_some() && !self.busy() {
                    self.open_dialog("solve.port_modes")?;
                }
            }
            Action::SetTab(t) => self.tab = t,
            Action::OpenStep(step) => crate::steps::open_step(self, step),
            Action::Fix(command) => crate::steps::fix(self, &command)?,
            Action::Select { row, pick } => {
                self.browser.selected = row;
                self.view3d.selected = pick;
            }
            Action::Fold(section) => {
                if self.browser.folded.contains(&section) {
                    self.browser.folded.retain(|f| *f != section);
                } else {
                    self.browser.folded.push(section);
                }
            }
            Action::Hover(pick) => self.view3d.hover = Some(pick),
            Action::FocusMode(focus) => self.workflow.mode_focus = Some(focus),
            Action::Undo => return self.run("edit.undo", json!({})),
            Action::Redo => return self.run("edit.redo", json!({})),
            Action::CancelJob => return Ok(json!({"cancelled": self.cancel_job()})),
            Action::SetHome(open) => {
                self.home.open = open;
                self.settings = None;
            }
            Action::Settings(section) => self.settings = section,
            Action::Pref(p) => self.set_pref(p),
            Action::File(f) => crate::files::apply(self, f),
            Action::AskExport => self.export = Some("emcraft-history.json".into()),
            Action::ExportHistory(path) => self.export_history(path),
            Action::ApplyBand => crate::steps::apply_band(self),
            Action::StartRun => return crate::steps::start_run(self),
            Action::PreviewPortModes => {
                if self.session.model().band.is_some() {
                    return self.run("solve.port_modes", json!({}));
                }
                self.open_dialog("solve.port_modes")?;
            }
            Action::CheckResults => {
                let v = self.run("results.check", json!({}))?;
                self.workflow.check = Some((self.revision, v.clone()));
                return Ok(v);
            }
            Action::ExportTouchstone => crate::steps::export_touchstone(self),
            Action::SetDevice { device, precision } => {
                if let Err(e) = self.run("solve.device", json!({"device": device, "precision": precision})) {
                    self.failure = Some(crate::Failure { command: "solve.device".into(), error: e.to_string() });
                }
            }
            Action::StandardView(v) => self.view3d.standard(v),
            Action::SetPlaying(on) => self.field.player.playing = on,
            Action::Message { text, error } => {
                if error {
                    self.error(text);
                } else {
                    self.info(text);
                }
            }
        }
        Ok(Value::Null)
    }

    fn set_pref(&mut self, p: Pref) {
        match p {
            Pref::Theme(name) => match self.themes.find(&name) {
                Some(_) => self.theme = name,
                None => self.error(emcraft_engine::text::clip(&format!("no theme `{name}` (there are {})", self.themes.names().join(", ")), 400)),
            },
            Pref::Language(id) => match crate::lang::find(&id) {
                Some(l) => {
                    self.prefs.language = l.id.to_owned();
                    self.use_theme();
                }
                None => self.error(format!("no language `{}` (there are {})", crate::lang::clip_id(&id), crate::lang::ids().join(", "))),
            },
            Pref::ReloadThemes => self.load_themes(self.themes.dir.clone()),
            Pref::Gpu3d(on) => self.prefs.gpu3d = on,
            Pref::Length(u) => self.prefs.length = u,
            Pref::Frequency(u) => self.prefs.frequency = u,
            Pref::ForgetRecent(i) => {
                if i < self.prefs.recent.len() {
                    self.prefs.recent.remove(i);
                }
            }
            Pref::ClearRecent => self.prefs.recent.clear(),
        }
    }

    /// Writes the history as a script (the session's model-changing commands, which rebuild the
    /// model: `cargo run -p emcraft-cli -- run FILE`); asks again on failure.
    fn export_history(&mut self, path: String) {
        let text = serde_json::to_string_pretty(self.session.history()).unwrap_or_else(|_| "[]".into());
        match std::fs::write(path.trim(), text) {
            Ok(()) => {
                let n = self.session.history().len();
                self.info_with("Exported {n} commands to {path}.", args!(n = n, path = path.trim()));
            }
            Err(e) => {
                self.error_with("Cannot write {path}: {error}", args!(path = path.trim(), error = e));

                self.export = Some(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            [Action::dialog("geometry.waveguide"), Action::SetDock(true), Action::Fold("Geometry".into()), Action::SetTab(RibbonTab::Mesh)],
        );
        assert_eq!(app.dialog.as_ref().map(|d| (d.command.id, d.docked)), Some(("geometry.waveguide", true)));
        assert_eq!((app.dock_dialogs, app.tab, app.browser.folded.clone()), (true, RibbonTab::Mesh, vec!["Geometry".to_owned()]));
        // A refused OK shows under the dialog's fields, not in the messages.
        if let Some(d) = app.dialog.as_mut() {
            let _ = d.set("width", &json!("wide"));
        }
        frame(&mut app, [Action::DialogOk]);
        assert!(app.dialog.as_ref().is_some_and(|d| d.fields.iter().any(|f| f.error.is_some())));
        assert_eq!(app.messages.len(), before);
        // A dialog that cannot open says why; the next action still runs.
        frame(&mut app, [Action::dialog("no.such"), Action::Fold("Geometry".into())]);
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

//! EMCraft's desktop UI, in the style of SolidCraft's: an application bar; a ribbon of tabs
//! (MODEL, MATERIALS, PORTS & SOURCES, MESH, SIMULATE, RESULTS) whose panels hold the engine's
//! commands, with command search; a browser of the model and its results on the left; the
//! viewport in the middle (the model's outline, S-parameter charts, fields on planes with a phase
//! animation, port-mode and eigenmode tables, far-field patterns, the comparison with theory, all
//! drawn by `emcraft-render` or egui, no GPU rendering); command dialogs as floating or docked
//! cards; messages, progress and Cancel at the bottom.
//!
//! Everything the UI does goes through engine commands ([`EmcraftApp::run`]); long ones run on a
//! worker thread ([`worker`]) so the window keeps drawing, and what follows a command's success
//! (the view it calls for, the state it resets) is in `after`. Agents and tests drive the same
//! app through the control channel ([`control`]: `emcraft --control PORT`) and headless
//! scenarios ([`scenario`]); [`raster`] draws a frame without a window.
//!
//! # A frame: read, ask, apply
//!
//! A frame has three phases ([`EmcraftApp::ui`]):
//! 1. `prepare` brings the caches the drawing reads up to date (the steps' statuses, the solve's
//!    estimate, the devices, the eigenmode table): they run engine queries, which need `&mut`.
//! 2. Drawing reads the app through `&EmcraftApp` and asks for changes by pushing
//!    [`Action`]s (run a command, open a dialog, show a view, open a step, select a row, undo, …)
//!    into the frame's [`Actions`].
//! 3. [`EmcraftApp::apply_all`] performs them, in order, through [`EmcraftApp::apply`] — the one
//!    place the app changes on a person's request. The control channel and the keyboard
//!    shortcuts call the same `apply` with the same actions.
//!
//! What egui edits in place is the exception, and it is local: a drawing function gets `&mut`
//! to a small piece of widget state beside `&EmcraftApp` — the search field's text
//! ([`ribbon::Search`]), the open dialog's fields ([`dialogs::Dialog`]), the step panels' fields
//! ([`steps::Fields`]), the path fields of the file and export prompts, the start page's
//! pictures, and the viewport's own state ([`viewport::Parts`]: the field view's settings and
//! samples, the result views' charts and settings, the 3D view's camera and pictures). Such a
//! piece is lent out of the app for the call ([`EmcraftApp::lend`]) and put back after it, so
//! while a piece is lent the drawing reads it from its argument, never from the app.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod action;
mod after;
pub mod browser;
pub mod commands;
pub mod control;
pub mod dialog_ui;
pub mod dialogs;
pub mod files;
mod fonts;
pub mod glyphs;
pub mod handle;
pub mod home;
pub mod lang;
mod messages;
pub mod picture;
pub mod prefs;
pub mod raster;
pub mod ribbon;
pub mod scenario;
pub mod scene3d;
pub mod settings;
mod shots;
pub mod state;
mod status;
pub mod steps;
pub mod theme;
pub mod ui_ext;
pub mod ui_kit;
pub mod view;
pub mod view3d;
pub mod viewport;
pub mod worker;

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use emcraft_engine::{EngineError, Session};
use serde_json::{Value, json};

pub use action::{Action, Actions};
pub use control::ControlRequest;
pub use handle::{Fixed, Handle};
pub use messages::Message;
pub use picture::Picture;
pub use state::FieldState;
pub use ui_ext::{Hint, Style, UiExt};
pub use view::View;

pub struct EmcraftApp {
    pub session: Session,
    /// Bumped whenever the session changes, so cached pictures and tables are redrawn.
    pub revision: u64,
    pub job: Option<worker::Job>,
    pub tab: ribbon::RibbonTab,
    pub dialog: Option<dialogs::Dialog>,
    /// Dialogs open docked at the right instead of floating.
    pub dock_dialogs: bool,
    pub view: View,
    pub field: FieldState,
    /// What the result views keep: their charts and settings.
    pub views: state::ViewState,
    /// The replies the result views show: the port modes, the comparison with theory.
    pub replies: state::Replies,
    pub messages: Vec<Message>,
    pub search: ribbon::Search,
    pub browser: browser::Browser,
    /// The current theme's name, and the themes there are (built-in and the user's).
    pub theme: String,
    pub themes: theme::Themes,
    /// The control channel: its requests, the input it injects, the screenshots it waits for.
    pub(crate) control: ControlState,
    pub quit_requested: bool,
    /// The input time of the last frame (s), which the animation advances from.
    last_frame: Option<f64>,
    pub frames: u64,
    /// The viewport's rectangle last frame.
    pub viewport_rect: Option<egui::Rect>,
    /// The window's context, to wake it when a job ends.
    ctx: Option<egui::Context>,
    /// The 3D view's camera and pictures.
    pub view3d: view3d::View3d,
    /// The workflow: the active step, the steps' statuses, the step panels' fields.
    pub workflow: steps::Workflow,
    /// The path asked for when exporting the history as a script.
    pub export: Option<String>,
    /// The preferences, and where and as what they were last written.
    pub prefs: prefs::Prefs,
    prefs_store: PrefsStore,
    /// Project files: the file dialog, the path field, the question before changes are lost.
    pub files: files::Files,
    /// The start page.
    pub home: home::Home,
    /// The settings page, at its section, while it is open.
    pub settings: Option<settings::Section>,
    /// The command that failed and why, until a command succeeds: the panel that asked for it
    /// shows it there (the message log keeps every one).
    pub failure: Option<Failure>,
    /// The dialog whose OK started the running job, to open again if the job fails.
    dialog_job: Option<dialogs::Dialog>,
}

/// The control channel's state beside the app.
#[derive(Default)]
pub(crate) struct ControlState {
    /// The request queue (`emcraft --control PORT`).
    pub(crate) rx: Option<Receiver<ControlRequest>>,
    /// Input injected by the control channel, fed to egui one pointer event per frame.
    pub(crate) synthetic: VecDeque<egui::Event>,
    /// The modifiers held by the injected input.
    pub(crate) synthetic_mods: egui::Modifiers,
    /// Screenshots asked for and not yet answered.
    pub(crate) shots: Vec<shots::Shot>,
    pub(crate) shot_token: u64,
}

/// Where the preferences are kept (none in the headless harness) and as what they were last
/// written.
#[derive(Default)]
struct PrefsStore {
    path: Option<std::path::PathBuf>,
    written: prefs::Prefs,
}

/// A command's failure: the command and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub command: String,
    pub error: String,
}

/// What the desktop app gives the window: a file dialog and where preferences are kept.
#[derive(Default)]
pub struct Host {
    pub picker: Option<Arc<dyn files::Picker>>,
    pub prefs: Option<std::path::PathBuf>,
    /// The folder of the user's themes (`theme::default_dir`).
    pub themes: Option<std::path::PathBuf>,
    /// A project file to open at once.
    pub open: Option<String>,
}

impl Default for EmcraftApp {
    fn default() -> Self {
        EmcraftApp::new()
    }
}

impl EmcraftApp {
    pub fn new() -> EmcraftApp {
        let mut app = EmcraftApp {
            session: Session::new(),
            revision: 0,
            job: None,
            tab: ribbon::RibbonTab::Model,
            dialog: None,
            dock_dialogs: false,
            view: View::Model,
            field: FieldState::default(),
            views: state::ViewState::default(),
            replies: state::Replies::default(),
            messages: Vec::new(),
            search: ribbon::Search::default(),
            browser: browser::Browser::default(),
            theme: "light".into(),
            themes: theme::Themes::load(None),
            control: ControlState::default(),
            quit_requested: false,
            last_frame: None,
            frames: 0,
            viewport_rect: None,
            ctx: None,
            view3d: view3d::View3d::default(),
            workflow: steps::Workflow::default(),
            export: None,
            prefs: prefs::Prefs::default(),
            prefs_store: PrefsStore::default(),
            files: files::Files::default(),
            home: home::Home::default(),
            settings: None,
            failure: None,
            dialog_job: None,
        };
        app.info_with("Open a sample or a project (File) or build a model from the ribbon.", &[]);
        app
    }

    /// The app of the desktop window: the host's file dialog and preferences, and the start
    /// page unless a sample is opened.
    pub fn for_host(sample: Option<&str>, host: Host) -> EmcraftApp {
        let prefs = host.prefs.map(|path| (prefs::Prefs::load(&path), path));
        // The first messages are in the preferred language.
        lang::set(prefs.as_ref().map_or(lang::ENGLISH, |(p, _)| p.language()));
        let mut app = EmcraftApp::with_sample(sample);
        if let Some((loaded, path)) = prefs {
            app.prefs = loaded;
            app.prefs_store = PrefsStore { path: Some(path), written: app.prefs.clone() };
            app.theme = app.prefs.theme().to_owned();
            app.dock_dialogs = app.prefs.dock_dialogs;
        }
        app.load_themes(host.themes);
        for p in lang::problems() {
            app.error(p);
        }
        app.files.picker = host.picker;
        app.home.open = sample.is_none() && host.open.is_none();
        if let Some(path) = host.open
            && let Err(e) = app.run("file.open", json!({"path": path}))
        {
            app.error(e);
        }
        app
    }

    /// Reads the themes again: the built-in ones and the user's in `dir` (none without one).
    /// Each file left out is reported once in the messages; a current theme that is gone gives
    /// way to the light one.
    pub fn load_themes(&mut self, dir: Option<std::path::PathBuf>) {
        self.themes = theme::Themes::load(dir.as_deref());
        for p in self.themes.problems.clone() {
            self.error(p);
        }
        if self.themes.find(&self.theme).is_none() {
            self.error(format!("no theme `{}`: using the light theme", emcraft_engine::text::clip(&self.theme, 40)));
            self.theme = "light".into();
        }
        self.use_theme();
    }

    /// Makes the app's theme and language the thread's current ones.
    pub fn use_theme(&self) {
        theme::set(self.themes.get(&self.theme));
        lang::set(self.prefs.language());
    }

    /// Keeps the preferences in step with the window and writes them when they change.
    fn keep_prefs(&mut self) {
        if self.prefs.theme() != self.theme {
            self.prefs.theme.clone_from(&self.theme);
        }
        self.prefs.dock_dialogs = self.dock_dialogs;
        if self.prefs == self.prefs_store.written {
            return;
        }
        self.prefs_store.written = self.prefs.clone();
        if let Some(path) = &self.prefs_store.path
            && let Err(e) = self.prefs.save(path)
        {
            self.error(format!("cannot keep the preferences in {}: {e}", path.display()));
        }
    }

    /// The app with a sample opened and solved (on the worker thread).
    pub fn with_sample(name: Option<&str>) -> EmcraftApp {
        let mut app = EmcraftApp::new();
        if let Some(name) = name
            && let Err(e) = app.run("file.sample", json!({"name": name, "solve": true}))
        {
            app.error(e);
        }
        app
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Refused while a job runs: one at a time.
    fn idle(&self) -> Result<(), UiError> {
        match &self.job {
            Some(job) => Err(UiError::Busy(label(&job.command).to_owned())),
            None => Ok(()),
        }
    }

    /// Runs a command as a person or an agent asks for it: a quick one at once, a long one on
    /// the worker thread (the reply is then `{"started": …}` and the result comes with the job).
    /// Refused while a job runs.
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, UiError> {
        self.idle()?;
        find_command(id)?;
        if commands::is_long(id) {
            self.job = Some(worker::Job::start(&self.session, id, params, self.ctx.clone()));
            self.info(format!("{}…", label(id)));
            return Ok(json!({"started": id}));
        }
        let r = self.session.execute(id, &params).map_err(UiError::from);
        match &r {
            Ok(reply) => self.after(id, &params, reply),
            Err(e) => self.failed(id, e),
        }
        r
    }

    /// A command's failure: kept for the panel that asked for it, and in the messages.
    fn failed(&mut self, id: &str, e: &impl std::fmt::Display) {
        self.failure = Some(Failure { command: id.to_owned(), error: e.to_string() });
        self.error(format!("{}: {e}", label(id)));
    }

    /// Runs a script (a JSON array of `{command, params}`) on the worker thread, as one job.
    pub fn run_script(&mut self, script: Value) -> Result<Value, UiError> {
        self.idle()?;
        let n = script.as_array().map(Vec::len).ok_or_else(|| UiError::Input("a script is a JSON array of {command, params}".into()))?;
        self.job =
            Some(worker::Job::spawn(&self.session, "engine.script", json!({}), self.ctx.clone(), move |s| s.run_script(&script).map(Value::Array)));
        self.info_with("Running a script of {n} commands…", &[("n", &n)]);
        Ok(json!({"started": "engine.script"}))
    }

    /// The sweep's frequencies (Hz).
    pub fn frequencies(&self) -> Vec<f64> {
        self.session.results().sweep.as_ref().map(|s| s.results.iter().map(|r| r.frequency).collect()).unwrap_or_default()
    }

    /// Takes a finished job's session and reply; answers the control requests waiting on it.
    pub fn poll_job(&mut self) {
        let Some(job) = &self.job else { return };
        let done = match job.poll() {
            worker::Poll::Running => return,
            worker::Poll::Finished(d) => Some(d),
            worker::Poll::Lost => None,
        };
        let Some(job) = self.job.take() else { return };
        let secs = job.elapsed();
        let reply = match done {
            Some(d) => {
                if d.reply.is_ok() {
                    self.session = d.session;
                }
                d.reply
            }
            None => Err(EngineError::failed("the worker stopped without an answer")),
        };
        match &reply {
            Ok(v) => {
                self.after(&job.command, &job.params, v);
                if let Some(m) = self.messages.last_mut() {
                    m.text.push_str(&format!(" ({secs:.1} s)"));
                }
            }
            Err(e) => {
                self.failed(&job.command, e);
                // The dialog that started it opens again, the error under its field.
                if let Some(mut d) = self.dialog_job.take().filter(|d| d.command.id == job.command && self.dialog.is_none()) {
                    d.refused(&e.to_string());
                    self.dialog = Some(d);
                }
            }
        }
        self.dialog_job = None;
        let answer = match reply {
            Ok(v) => json!({"ok": true, "result": v}),
            Err(e) => json!({"ok": false, "error": e.to_string()}),
        };
        for w in job.waiting {
            let _ = w.send(answer.clone());
        }
    }

    /// Stops the running job: the session stays as it was before it.
    pub fn cancel_job(&mut self) -> bool {
        let Some(job) = self.job.take() else { return false };
        job.cancel();
        self.dialog_job = None;
        self.info_with("{command} cancelled.", &[("command", &label(&job.command))]);

        for w in job.waiting {
            let _ = w.send(json!({"ok": false, "error": emcraft_engine::progress::CANCELLED}));
        }
        true
    }

    /// Opens the dialog of `id` (commands without parameters run at once).
    pub fn open_dialog(&mut self, id: &str) -> Result<(), UiError> {
        let spec = find_command(id)?;
        if spec.params.is_empty() {
            return self.run(id, json!({})).map(|_| ());
        }
        let init = dialogs::initial(id, &self.session);
        self.dialog = Some(dialogs::Dialog::new(spec, &self.session, &init, self.dock_dialogs));
        Ok(())
    }

    /// Opens the dialog of `id` with `values` over its usual initial ones (the view's settings
    /// for an export, say).
    pub fn open_dialog_with(&mut self, id: &str, values: Value) -> Result<(), UiError> {
        let spec = find_command(id)?;
        let mut init = dialogs::initial(id, &self.session);
        if let (Some(o), Some(v)) = (init.as_object_mut(), values.as_object()) {
            o.extend(v.clone());
        } else {
            init = values;
        }
        self.dialog = Some(dialogs::Dialog::new(spec, &self.session, &init, self.dock_dialogs));
        Ok(())
    }

    /// OK on the open dialog: runs its command with its parameters; the dialog stays open with
    /// the error when they are refused.
    pub fn dialog_ok(&mut self) -> Result<Value, UiError> {
        let Some(d) = self.dialog.as_mut() else { return Err(UiError::NoDialog) };
        let id = d.command.id;
        let params = match d.params() {
            Ok(p) => p,
            Err(e) => {
                d.error = Some(e.clone());
                return Err(UiError::Input(e));
            }
        };
        match self.run(id, params) {
            Ok(v) => {
                // A long command's dialog comes back with the error if its job fails.
                self.dialog_job = self.dialog.take().filter(|_| commands::is_long(id));
                Ok(v)
            }
            Err(e) => {
                if let Some(d) = self.dialog.as_mut() {
                    d.refused(&e.to_string());
                }
                Err(e)
            }
        }
    }

    /// One frame's work before drawing: the job, control requests, the animation clock.
    pub fn logic(&mut self, ctx: &egui::Context) {
        self.frames += 1;
        if self.ctx.as_ref() != Some(ctx) {
            self.ctx = Some(ctx.clone());
        }
        self.use_theme();
        theme::apply(ctx);
        fonts::install(ctx);
        self.poll_job();
        files::poll(self);
        control::serve(self, ctx);
        // The phase plays while a field is on screen: the field view, or the 3D view in Results.
        let time = ctx.input(|i| i.time);
        let showing = self.view == View::Field || (self.view == View::Model && self.workflow.step == Some(steps::Step::Results));
        if showing && self.field.player.playing {
            let dt = self.last_frame.map_or(0.0, |t| time - t).clamp(0.0, 0.1);
            self.field.view.phase = self.field.player.advance(self.field.view.phase, dt);
            ctx.request_repaint();
        }
        self.last_frame = Some(time);
        if self.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if self.quit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Feeds injected input to egui: everything up to and including the next pointer event, so
    /// a click is a press in one frame after a move in the one before.
    pub fn raw_input_hook(&mut self, raw: &mut egui::RawInput) {
        let n = self
            .control
            .synthetic
            .iter()
            .position(|e| {
                matches!(
                    e,
                    egui::Event::PointerMoved(_)
                        | egui::Event::PointerButton { .. }
                        | egui::Event::MouseWheel { .. }
                        | egui::Event::Key { pressed: false, .. }
                )
            })
            .map_or(self.control.synthetic.len(), |i| i + 1);
        let control = &mut self.control;
        for event in control.synthetic.drain(..n) {
            match &event {
                egui::Event::ModifiersChanged(m) => control.synthetic_mods = *m,
                egui::Event::PointerButton { pos, modifiers, pressed, .. } => {
                    raw.events.push(egui::Event::PointerMoved(*pos));
                    raw.events.push(egui::Event::ModifiersChanged(*modifiers));
                    // A drag keeps its press's modifiers until the release.
                    control.synthetic_mods = if *pressed { *modifiers } else { egui::Modifiers::NONE };
                }
                egui::Event::Key { modifiers, .. } => raw.events.push(egui::Event::ModifiersChanged(*modifiers)),
                egui::Event::PointerMoved(_) => raw.events.push(egui::Event::ModifiersChanged(control.synthetic_mods)),
                _ => {}
            }
            raw.events.push(event);
        }
    }

    /// Lays out the whole window: the caches brought up to date, the frame drawn, then what it
    /// asked for performed.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        scenario::clear_handles();
        self.prepare();
        let ctx = ui.ctx().clone();
        let mut out = Actions::default();
        ribbon::app_bar(self, ui, &mut out);
        if let Some(section) = self.settings {
            settings::show(self, section, ui, &mut out);
        } else if self.home.open {
            self.lend(|a| &mut a.home.thumbs, |app, thumbs| home::show(app, thumbs, ui, &mut out));
        } else {
            self.workspace(ui, &ctx, &mut out);
        }
        self.lend(|a| &mut a.files.prompt, |app, prompt| files::show(app, prompt, &ctx, &mut out));
        shortcuts(self, &ctx, &mut out);
        self.apply_all(out);
        self.collect_screenshots(&ctx);
        self.keep_prefs();
    }

    /// Brings the caches the drawing reads up to date: they come from engine queries, which
    /// need the session mutable.
    fn prepare(&mut self) {
        steps::prepare(self);
        if self.view == View::Eigenmodes {
            viewport::prepare_eigenmodes(self);
        }
    }

    /// Lends `self` to `draw` with a piece of its widget state (`piece`) taken out of it, and
    /// puts the piece back after: the drawing reads the app and edits the piece.
    pub fn lend<T: Default, R>(&mut self, piece: impl Fn(&mut Self) -> &mut T, draw: impl FnOnce(&Self, &mut T) -> R) -> R {
        let mut lent = std::mem::take(piece(self));
        let r = draw(self, &mut lent);
        *piece(self) = lent;
        r
    }

    /// The window past the start page: the ribbon, the steps, the messages, the browser, the
    /// step's panel or the docked dialog, the viewport, and what floats over them.
    fn workspace(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, out: &mut Actions) {
        self.lend(|a| &mut a.search, |app, search| ribbon::ribbon(app, search, ui, out));
        steps::bar(self, ui, out);
        messages::area(self, ui, out);
        browser::show(self, ui, out);
        // With a step open, its panel holds the dialog.
        let in_step = self.workflow.step.is_some();
        if in_step {
            let mut fields = std::mem::take(&mut self.workflow.fields);
            self.lend(|a| &mut a.dialog, |app, dialog| steps::panel(app, &mut fields, dialog.as_mut(), ui, out));
            self.workflow.fields = fields;
        } else if self.dialog.as_ref().is_some_and(|d| d.docked) {
            self.lend(|a| &mut a.dialog, |app, dialog| dialog.as_mut().map(|d| dialog_ui::docked(app, d, ui, out)));
        }
        let mut parts = viewport::Parts::take(self);
        let rect = viewport::show(self, &mut parts, ui, out);
        parts.put_back(self);
        self.viewport_rect = rect;
        if !in_step && self.dialog.as_ref().is_some_and(|d| !d.docked) {
            self.lend(|a| &mut a.dialog, |app, dialog| dialog.as_mut().map(|d| dialog_ui::floating(app, d, ctx, out)));
        }
        ribbon::search_popup(&mut self.search, ctx, out);
        self.lend(|a| &mut a.export, |app, export| ribbon::export_prompt(app, export, ctx, out));
    }
}

/// Undo, redo and the File menu's keys (when no field has the keyboard).
fn shortcuts(app: &EmcraftApp, ctx: &egui::Context, out: &mut Actions) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    // Escape leaves the settings page.
    if app.settings.is_some() && app.files.prompt.is_none() && app.files.confirm.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        out.push(Action::Settings(None));
    }
    if app.busy() {
        return;
    }
    let (undo, redo) = ctx.input(|i| {
        let c = i.modifiers.command;
        (
            c && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
            c && (i.key_pressed(egui::Key::Y) || i.modifiers.shift && i.key_pressed(egui::Key::Z)),
        )
    });
    if undo {
        out.push(Action::Undo);
    } else if redo {
        out.push(Action::Redo);
    } else if app.dialog.is_none() && app.files.prompt.is_none() && app.files.confirm.is_none() {
        files::shortcuts(ctx, out);
    }
}

/// The engine's command `id`, or the error naming it (cut short).
fn find_command(id: &str) -> Result<&'static emcraft_engine::CommandSpec, UiError> {
    emcraft_engine::find_command(id).ok_or_else(|| EngineError::UnknownCommand(emcraft_engine::text::clip(id, 100)).into())
}

/// A command's label in the current language (ribbon, search, dialogs, messages).
pub fn label(id: &str) -> &str {
    lang::command_label(id)
}

/// The solved frequency nearest `f`.
pub fn nearest(freqs: &[f64], f: f64) -> Option<f64> {
    freqs.iter().copied().min_by(|a, b| (a - f).abs().total_cmp(&(b - f).abs()))
}

/// Why the app refused or could not do what a person or an agent asked.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum UiError {
    /// A job runs (its label); one at a time.
    #[error("busy: {0} is running (cancel it first)")]
    Busy(String),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error("no dialog is open")]
    NoDialog,
    /// Input that cannot be used: a field's text, a request's shape.
    #[error("{0}")]
    Input(String),
    /// Run's preflight found problems that block it.
    #[error("Run refused: {0}")]
    Refused(String),
}

/// Opens the desktop app; `sample` is opened and solved at once. `control` is the control
/// channel's request queue (`emcraft --control PORT`).
pub fn run(sample: Option<String>, control: Option<Receiver<ControlRequest>>) -> eframe::Result {
    run_with(sample, Host::default(), move |_| control)
}

/// Like [`run`], with the control queue made from the window's context (so the server can wake
/// the window when a request arrives).
pub fn run_with(sample: Option<String>, host: Host, control: impl FnOnce(&egui::Context) -> Option<Receiver<ControlRequest>>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("EMCraft").with_inner_size([1500.0, 960.0]).with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "EMCraft",
        options,
        Box::new(move |cc| {
            let mut app = EmcraftApp::for_host(sample.as_deref(), host);
            app.control.rx = control(&cc.egui_ctx);
            Ok(Box::new(Window(app)))
        }),
    )
}

/// The app inside eframe.
struct Window(EmcraftApp);

impl eframe::App for Window {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.0.logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.0.raw_input_hook(raw);
    }
}

#[cfg(test)]
mod tests;

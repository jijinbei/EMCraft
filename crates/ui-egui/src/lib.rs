//! EMCraft's desktop UI, in the style of SolidCraft's: an application bar; a ribbon of tabs
//! (MODEL, MATERIALS, PORTS & SOURCES, MESH, SIMULATE, RESULTS) whose panels hold the engine's
//! commands, with command search; a browser of the model and its results on the left; the
//! viewport in the middle (the model's outline, S-parameter charts, fields on planes with a phase
//! animation, port-mode and eigenmode tables, far-field patterns, the comparison with theory, all
//! drawn by `emcraft-render` or egui, no GPU rendering); command dialogs as floating or docked
//! cards; messages, progress and Cancel at the bottom.
//!
//! Everything the UI does goes through engine commands ([`EmcraftApp::run`]); long ones run on a
//! worker thread ([`worker`]) so the window keeps drawing. Agents and tests drive the same app
//! through the control channel ([`control`]: `emcraft --control PORT`) and headless scenarios
//! ([`scenario`]); [`raster`] draws a frame without a window.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod browser;
pub mod control;
pub mod dialog_ui;
pub mod dialogs;
pub mod files;
pub mod glyphs;
pub mod home;
pub mod prefs;
pub mod raster;
pub mod ribbon;
pub mod scenario;
pub mod scene3d;
mod status;
pub mod steps;
pub mod theme;
pub mod ui_kit;
pub mod view3d;
pub mod viewport;
pub mod worker;

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;

use emcraft_engine::view::{FieldView, Plane, PlaneSamples, Probe};
use emcraft_engine::{EngineError, Session};
use serde_json::{Value, json};

pub use control::ControlRequest;

/// What the viewport shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// The model in 3D.
    Model,
    /// The model's H-plane and E-plane outlines.
    Outline,
    SParameters,
    Field,
    PortModes,
    FarField,
    Compare,
    Eigenmodes,
}

impl View {
    pub const ALL: [View; 8] =
        [View::Model, View::Outline, View::SParameters, View::Field, View::PortModes, View::FarField, View::Compare, View::Eigenmodes];

    pub fn id(self) -> &'static str {
        match self {
            View::Model => "model",
            View::Outline => "outline",
            View::SParameters => "sparameters",
            View::Field => "field",
            View::PortModes => "port_modes",
            View::FarField => "far_field",
            View::Compare => "compare",
            View::Eigenmodes => "eigenmodes",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            View::Model => "Model 3D",
            View::Outline => "Outline",
            View::SParameters => "S-parameters",
            View::Field => "Field",
            View::PortModes => "Port modes",
            View::FarField => "Far field",
            View::Compare => "Compare with theory",
            View::Eigenmodes => "Eigenmodes",
        }
    }

    pub fn parse(s: &str) -> Option<View> {
        View::ALL.into_iter().find(|v| v.id() == s)
    }
}

/// A line of the message area.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub text: String,
    pub error: bool,
    /// The same message repeated in a row.
    pub count: u32,
}

/// Messages kept.
const MAX_MESSAGES: usize = 200;

/// A CPU picture shown as a texture, uploaded to each egui context that draws it (the window's,
/// and the offscreen one of `ui.render`).
pub struct Picture {
    pub key: String,
    pub image: Arc<egui::ColorImage>,
    textures: Vec<(egui::Context, egui::TextureHandle)>,
}

impl Picture {
    pub fn new(key: String, canvas: &emcraft_render::Canvas) -> Picture {
        let image = egui::ColorImage::from_rgba_unmultiplied([canvas.w, canvas.h], &canvas.rgba);
        Picture { key, image: Arc::new(image), textures: Vec::new() }
    }

    pub fn size(&self) -> egui::Vec2 {
        egui::vec2(self.image.size[0] as f32, self.image.size[1] as f32)
    }

    /// Its texture in `ctx`.
    pub fn texture(&mut self, ctx: &egui::Context) -> egui::TextureId {
        if let Some((_, h)) = self.textures.iter().find(|(c, _)| c == ctx) {
            return h.id();
        }
        let h = ctx.load_texture(format!("picture:{}", self.key.len()), self.image.clone(), egui::TextureOptions::LINEAR);
        let id = h.id();
        // The window's context and at most one offscreen one.
        if self.textures.len() >= 2 {
            drop(self.textures.remove(1));
        }
        self.textures.push((ctx.clone(), h));
        id
    }
}

/// The field view's state.
pub struct FieldState {
    pub view: FieldView,
    /// The plane's position in display length units; the middle when `None`.
    pub at: Option<f64>,
    pub samples: Option<PlaneSamples>,
    pub picture: Option<Picture>,
    /// Where the picture puts the plane, for reading the pointer's position.
    pub layout: Option<emcraft_render::plot::Layout>,
    /// The phase animation, played like a video.
    pub player: ui_kit::Player,
    /// The fields under the pointer (last frame), and the point clicked on the plane (display
    /// units along the picture's axes), read again whenever the view changes.
    pub hover: Option<Probe>,
    pub pinned: Option<[f64; 2]>,
    /// Planes kept for the 3D view beside the field view's own (plane, position in display
    /// units), and their samples.
    pub kept: Vec<(Plane, f64)>,
    pub kept_samples: Vec<Option<PlaneSamples>>,
}

/// A window screenshot waiting for its frame.
pub(crate) struct Shot {
    pub token: u64,
    pub path: Option<String>,
    pub reply: Sender<Value>,
    pub asked: Instant,
    pub sent: bool,
}

pub struct EmcraftApp {
    pub session: Session,
    /// Bumped whenever the session changes, so cached pictures and tables are redrawn.
    pub revision: u64,
    pub job: Option<worker::Job>,
    pub tab: String,
    pub dialog: Option<dialogs::Dialog>,
    /// Dialogs open docked at the right instead of floating.
    pub dock_dialogs: bool,
    pub view: View,
    pub field: FieldState,
    pub chart: Option<Picture>,
    /// The far-field pattern's azimuth (degrees).
    pub pattern_phi: f64,
    pub port_modes: Option<Value>,
    pub compare: Option<Value>,
    pub eigen_table: Option<(u64, Value)>,
    pub messages: Vec<Message>,
    pub search: ribbon::Search,
    pub browser: browser::Browser,
    pub dark: bool,
    /// Input injected by the control channel, fed to egui one pointer event per frame.
    pub synthetic: VecDeque<egui::Event>,
    synthetic_mods: egui::Modifiers,
    pub control_rx: Option<Receiver<ControlRequest>>,
    pub(crate) shots: Vec<Shot>,
    shot_token: u64,
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
    /// The preferences, where they are kept (none in the headless harness) and as last written.
    pub prefs: prefs::Prefs,
    pub prefs_path: Option<std::path::PathBuf>,
    prefs_written: prefs::Prefs,
    /// Project files: the file dialog, the path field, the question before changes are lost.
    pub files: files::Files,
    /// The start page.
    pub home: home::Home,
    /// The last command that failed and why, until it succeeds: the panel that asked for it
    /// shows it there (the message log keeps every one).
    pub failure: Option<(String, String)>,
}

/// What the desktop app gives the window: a file dialog and where preferences are kept.
#[derive(Default)]
pub struct Host {
    pub picker: Option<Arc<dyn files::Picker>>,
    pub prefs: Option<std::path::PathBuf>,
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
        EmcraftApp {
            session: Session::new(),
            revision: 0,
            job: None,
            tab: "MODEL".into(),
            dialog: None,
            dock_dialogs: false,
            view: View::Model,
            field: FieldState {
                view: FieldView { show_h: false, ..FieldView::new(10.0e9, Plane::H) },
                at: None,
                samples: None,
                picture: None,
                layout: None,
                player: ui_kit::Player::default(),
                hover: None,
                pinned: None,
                kept: Vec::new(),
                kept_samples: Vec::new(),
            },
            chart: None,
            pattern_phi: 0.0,
            port_modes: None,
            compare: None,
            eigen_table: None,
            messages: vec![Message { text: "Open a sample or a project (File) or build a model from the ribbon.".into(), error: false, count: 1 }],
            search: ribbon::Search::default(),
            browser: browser::Browser::default(),
            dark: false,
            synthetic: VecDeque::new(),
            synthetic_mods: egui::Modifiers::NONE,
            control_rx: None,
            shots: Vec::new(),
            shot_token: 0,
            quit_requested: false,
            last_frame: None,
            frames: 0,
            viewport_rect: None,
            ctx: None,
            view3d: view3d::View3d::default(),
            workflow: steps::Workflow::default(),
            export: None,
            prefs: prefs::Prefs::default(),
            prefs_path: None,
            prefs_written: prefs::Prefs::default(),
            files: files::Files::default(),
            home: home::Home::default(),
            failure: None,
        }
    }

    /// The app of the desktop window: the host's file dialog and preferences, and the start
    /// page unless a sample is opened.
    pub fn for_host(sample: Option<&str>, host: Host) -> EmcraftApp {
        let mut app = EmcraftApp::with_sample(sample);
        if let Some(path) = host.prefs {
            app.prefs = prefs::Prefs::load(&path);
            app.prefs_written = app.prefs.clone();
            app.prefs_path = Some(path);
            app.dark = app.prefs.dark;
            app.dock_dialogs = app.prefs.dock_dialogs;
        }
        app.files.picker = host.picker;
        app.home.open = sample.is_none() && host.open.is_none();
        if let Some(path) = host.open
            && let Err(e) = app.run("file.open", json!({"path": path}))
        {
            app.message(e, true);
        }
        app
    }

    /// Keeps the preferences in step with the window and writes them when they change.
    fn keep_prefs(&mut self) {
        self.prefs.dark = self.dark;
        self.prefs.dock_dialogs = self.dock_dialogs;
        if self.prefs == self.prefs_written {
            return;
        }
        self.prefs_written = self.prefs.clone();
        if let Some(path) = &self.prefs_path
            && let Err(e) = self.prefs.save(path)
        {
            self.message(format!("cannot keep the preferences in {}: {e}", path.display()), true);
        }
    }

    /// The app with a sample opened and solved (on the worker thread).
    pub fn with_sample(name: Option<&str>) -> EmcraftApp {
        let mut app = EmcraftApp::new();
        if let Some(name) = name
            && let Err(e) = app.run("file.sample", json!({"name": name, "solve": true}))
        {
            app.message(e, true);
        }
        app
    }

    pub fn message(&mut self, text: impl std::fmt::Display, error: bool) {
        let text: String = text.to_string().chars().take(2000).collect();
        if let Some(last) = self.messages.last_mut()
            && last.text == text
            && last.error == error
        {
            last.count = last.count.saturating_add(1);
            return;
        }
        self.messages.push(Message { text, error, count: 1 });
        if self.messages.len() > MAX_MESSAGES {
            let extra = self.messages.len() - MAX_MESSAGES;
            self.messages.drain(..extra);
        }
    }

    pub fn status(&self) -> Option<&Message> {
        self.messages.last()
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Runs a command as a person or an agent asks for it: a quick one at once, a long one on
    /// the worker thread (the reply is then `{"started": …}` and the result comes with the job).
    /// Refused while a job runs.
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, UiError> {
        if let Some(job) = &self.job {
            return Err(UiError::Busy(label(&job.command).to_owned()));
        }
        if emcraft_engine::find_command(id).is_none() {
            return Err(EngineError::UnknownCommand(id.chars().take(100).collect()).into());
        }
        if worker::is_long(id) {
            self.job = Some(worker::Job::start(&self.session, id, params, self.ctx.clone()));
            self.message(format!("{}…", label(id)), false);
            return Ok(json!({"started": id}));
        }
        let r = self.session.execute(id, &params).map_err(UiError::from);
        match &r {
            Ok(reply) => self.after(id, &params, reply),
            Err(e) => {
                self.failure = Some((id.to_owned(), e.to_string()));
                self.message(format!("{}: {e}", label(id)), true);
            }
        }
        r
    }

    /// Runs a script (a JSON array of `{command, params}`) on the worker thread, as one job.
    pub fn run_script(&mut self, script: Value) -> Result<Value, UiError> {
        if let Some(job) = &self.job {
            return Err(UiError::Busy(label(&job.command).to_owned()));
        }
        let n = script.as_array().map(Vec::len).ok_or_else(|| UiError::Input("a script is a JSON array of {command, params}".into()))?;
        self.job =
            Some(worker::Job::spawn(&self.session, "engine.script", json!({}), self.ctx.clone(), move |s| s.run_script(&script).map(Value::Array)));
        self.message(format!("Running a script of {n} commands…"), false);
        Ok(json!({"started": "engine.script"}))
    }

    /// What follows a command's success: the view it calls for, its message.
    fn after(&mut self, id: &str, params: &Value, reply: &Value) {
        self.revision = self.revision.wrapping_add(1);
        if self.failure.as_ref().is_some_and(|f| f.0 == id) {
            self.failure = None;
        }
        self.field.samples = None;
        self.field.picture = None;
        self.field.pinned = None;
        self.field.kept_samples.clear();
        self.chart = None;
        if matches!(id, "file.open" | "file.save")
            && let Some(path) = reply.get("path").and_then(Value::as_str)
        {
            files::opened(self, path);
        }
        let results = &self.session.results;
        match id {
            "file.sample" | "engine.script" | "file.open" | "file.new" => {
                if matches!(id, "file.new" | "file.sample") {
                    self.home.open = false;
                }
                self.port_modes = None;
                self.compare = None;
                self.view = if results.sweep.is_some() {
                    View::SParameters
                } else if results.eigen.is_some() {
                    View::Eigenmodes
                } else if results.radiation.is_some() {
                    View::FarField
                } else {
                    View::Model
                };
                self.pick_frequency();
                self.view3d.fit_next = true;
                self.workflow.check = None;
            }
            "solve.frequency" => {
                self.view = View::SParameters;
                self.pick_frequency();
            }
            "solve.eigenmodes" => {
                self.view = View::Eigenmodes;
                self.field.view.mode = Some(0);
            }
            "solve.radiation" => {
                self.view = View::FarField;
                self.field.view.mode = None;
                if let Some(r) = &results.radiation {
                    self.field.view.frequency = r.result.frequency;
                }
            }
            "solve.time_domain" => {
                self.view = if results.td_radiation.is_some() { View::FarField } else { View::SParameters };
            }
            "solve.port_modes" => {
                self.port_modes = Some(reply.clone());
                // The Excitation step tabulates them in its panel, beside the 3D view.
                if self.workflow.step != Some(steps::Step::Excitation) {
                    self.view = View::PortModes;
                }
            }
            "results.compare" => {
                self.compare = Some(reply.clone());
                self.view = View::Compare;
            }
            "edit.undo" | "edit.redo" => self.port_modes = None,
            other if other.starts_with("geometry.") => {
                self.view = View::Model;
                self.port_modes = None;
                self.compare = None;
            }
            _ => {}
        }
        self.message(status::text(id, params, reply, &self.session), false);
        // A run started from the Run step goes on to Results.
        if id.starts_with("solve.") && id != "solve.port_modes" && self.workflow.step == Some(steps::Step::Run) {
            steps::open_step(self, Some(steps::Step::Results));
        }
    }

    /// Puts the field view on a solved frequency.
    fn pick_frequency(&mut self) {
        let freqs = self.frequencies();
        if let Some(f) = nearest(&freqs, self.field.view.frequency) {
            self.field.view.frequency = f;
        }
        self.field.view.mode = if self.session.results.sweep.is_none() && self.session.results.eigen.is_some() { Some(0) } else { None };
        self.field.at = None;
    }

    /// The sweep's frequencies (Hz).
    pub fn frequencies(&self) -> Vec<f64> {
        self.session.results.sweep.as_ref().map(|s| s.results.iter().map(|r| r.frequency).collect()).unwrap_or_default()
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
                self.failure = Some((job.command.clone(), e.to_string()));
                self.message(format!("{}: {e}", label(&job.command)), true);
            }
        }
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
        self.message(format!("{} cancelled.", label(&job.command)), false);
        for w in job.waiting {
            let _ = w.send(json!({"ok": false, "error": emcraft_engine::progress::CANCELLED}));
        }
        true
    }

    /// Opens the dialog of `id` (commands without parameters run at once).
    pub fn open_dialog(&mut self, id: &str) -> Result<(), UiError> {
        let spec = emcraft_engine::find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.chars().take(100).collect()))?;
        if spec.params.is_empty() {
            return self.run(id, json!({})).map(|_| ());
        }
        let init = dialogs::initial(id, &self.session);
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
                self.dialog = None;
                Ok(v)
            }
            Err(e) => {
                if let Some(d) = self.dialog.as_mut() {
                    d.error = Some(e.to_string());
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
        theme::set_dark(self.dark);
        theme::apply(ctx);
        self.poll_job();
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
            .map_or(self.synthetic.len(), |i| i + 1);
        for event in self.synthetic.drain(..n) {
            match &event {
                egui::Event::ModifiersChanged(m) => self.synthetic_mods = *m,
                egui::Event::PointerButton { pos, modifiers, pressed, .. } => {
                    raw.events.push(egui::Event::PointerMoved(*pos));
                    raw.events.push(egui::Event::ModifiersChanged(*modifiers));
                    // A drag keeps its press's modifiers until the release.
                    self.synthetic_mods = if *pressed { *modifiers } else { egui::Modifiers::NONE };
                }
                egui::Event::Key { modifiers, .. } => raw.events.push(egui::Event::ModifiersChanged(*modifiers)),
                egui::Event::PointerMoved(_) => raw.events.push(egui::Event::ModifiersChanged(self.synthetic_mods)),
                _ => {}
            }
            raw.events.push(event);
        }
    }

    /// Lays out the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        scenario::clear_handles();
        let ctx = ui.ctx().clone();
        ribbon::app_bar(self, ui);
        if self.home.open {
            home::show(self, ui);
            files::show(self, &ctx);
            shortcuts(self, &ctx);
            self.collect_screenshots(&ctx);
            self.keep_prefs();
            return;
        }
        ribbon::ribbon(self, ui);
        steps::bar(self, ui);
        status_area(self, ui);
        browser::show(self, ui);
        // With a step open, its panel holds the dialog.
        let in_step = self.workflow.step.is_some();
        if in_step {
            steps::panel(self, ui);
        } else if self.dialog.as_ref().is_some_and(|d| d.docked) {
            dialog_ui::docked(self, ui);
        }
        viewport::show(self, ui);
        if !in_step && self.dialog.as_ref().is_some_and(|d| !d.docked) {
            dialog_ui::floating(self, &ctx);
        }
        ribbon::search_popup(self, &ctx);
        ribbon::export_prompt(self, &ctx);
        files::show(self, &ctx);
        shortcuts(self, &ctx);
        self.collect_screenshots(&ctx);
        self.keep_prefs();
    }

    /// Asks the window for a screenshot (answered in a later frame).
    pub(crate) fn request_screenshot(&mut self, path: Option<String>, reply: Sender<Value>) {
        self.shot_token += 1;
        self.shots.push(Shot { token: self.shot_token, path, reply, asked: Instant::now(), sent: false });
    }

    fn collect_screenshots(&mut self, ctx: &egui::Context) {
        if self.shots.is_empty() {
            return;
        }
        for s in self.shots.iter_mut().filter(|s| !s.sent) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(s.token)));
            s.sent = true;
        }
        let events: Vec<(u64, Arc<egui::ColorImage>)> = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot { user_data, image, .. } => {
                        let token = user_data.data.as_ref().and_then(|d| d.downcast_ref::<u64>()).copied()?;
                        Some((token, image.clone()))
                    }
                    _ => None,
                })
                .collect()
        });
        for (token, image) in events {
            if let Some(i) = self.shots.iter().position(|s| s.token == token) {
                let shot = self.shots.remove(i);
                let _ = shot.reply.send(control::save_screenshot(&image, shot.path.as_deref()));
            }
        }
        self.shots.retain(|s| {
            if s.asked.elapsed().as_secs_f64() < 8.0 {
                return true;
            }
            let _ = s.reply.send(json!({"ok": false, "error": "no frame was presented (window hidden?); use ui.render"}));
            false
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

/// Undo, redo and save from the keyboard (when no field has it).
fn shortcuts(app: &mut EmcraftApp, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() || app.busy() {
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
        let _ = app.run("edit.undo", json!({}));
    } else if redo {
        let _ = app.run("edit.redo", json!({}));
    } else if app.dialog.is_none() && app.files.prompt.is_none() && app.files.confirm.is_none() {
        files::shortcuts(app, ctx);
    }
}

/// A command's label for messages.
pub fn label(id: &str) -> &str {
    emcraft_engine::find_command(id).map_or(id, |c| c.label)
}

/// The solved frequency nearest `f`.
pub fn nearest(freqs: &[f64], f: f64) -> Option<f64> {
    freqs.iter().copied().min_by(|a, b| (a - f).abs().total_cmp(&(b - f).abs()))
}

/// The message card at the bottom: the last messages, and the running job's progress and Cancel.
fn status_area(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = theme::Tokens::get();
    // Taller while a job runs: its progress row sits above the messages.
    egui::Panel::bottom("emcraft_status")
        .exact_size(if app.job.is_some() { 130.0 } else { 96.0 })
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_bottom).inner_margin(egui::Margin { left: 6, right: 6, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            let height = ui.available_height() - 12.0;
            t.card(t.panel).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(height);
                scenario::publish_rect("status", ui.max_rect());
                let mut cancel = false;
                if let Some(job) = &app.job {
                    let snap = job.snapshot();
                    ui.horizontal(|ui| {
                        // Cancel first, where it stays put while the text beside it changes.
                        if ui_kit::button(ui, "Cancel", false, true, "status:cancel").clicked() {
                            cancel = true;
                        }
                        ui.spinner();
                        let what = if snap.total > 0 {
                            format!("{} — {} {} of {}", label(&job.command), snap.stage, (snap.done + 1).min(snap.total), snap.total)
                        } else {
                            format!("{} — running", label(&job.command))
                        };
                        ui.label(egui::RichText::new(format!("{what} ({:.0} s)", job.elapsed())).size(t.font_body));
                        ui_kit::progress(ui, snap.fraction(), 180.0);
                    });
                }
                if cancel {
                    app.cancel_job();
                }
                let rest = (height - if app.job.is_some() { 36.0 } else { 0.0 }).max(16.0);
                egui::ScrollArea::vertical().stick_to_bottom(true).max_height(rest).auto_shrink([false, false]).show(ui, |ui| {
                    let start = app.messages.len().saturating_sub(40);
                    for m in &app.messages[start..] {
                        let text = if m.count > 1 { format!("{} (×{})", m.text, m.count) } else { m.text.clone() };
                        ui.label(egui::RichText::new(text).size(t.font_body).color(if m.error { t.error } else { t.text }));
                    }
                });
            });
        });
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
            app.control_rx = control(&cc.egui_ctx);
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

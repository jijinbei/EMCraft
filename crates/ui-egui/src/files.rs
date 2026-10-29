//! Projects as files: New, Open, Save and Save As on `.emcraft` files, the recent list, and the
//! question before changes would be lost. Paths come from the host's file dialog (a
//! [`Picker`] the desktop app gives, `rfd` there) or, without one — headless, in scenarios — from
//! a path field in the window. The work itself is the engine's `file.new`, `file.open` and
//! `file.save`.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::vec2;
use serde_json::json;

use crate::theme::Tokens;
use crate::{EmcraftApp, scenario, ui_kit};

/// A file dialog, asked without blocking: `done` is called with the path chosen, `None` when
/// cancelled, from any thread.
pub trait Picker: Send + Sync {
    /// A project to open.
    fn open(&self, done: Box<dyn FnOnce(Option<String>) + Send>);
    /// Where to save a project, `suggested` the file name offered.
    fn save(&self, suggested: &str, done: Box<dyn FnOnce(Option<String>) + Send>);
}

/// What a path asked for is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Open,
    SaveAs,
}

/// What to do once the unsaved changes are dealt with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Then {
    New,
    Open,
    OpenPath(String),
    Sample(String),
}

/// The path field shown when there is no file dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub purpose: Purpose,
    pub text: String,
}

pub struct Files {
    pub picker: Option<Arc<dyn Picker>>,
    tx: Sender<(Purpose, Option<String>)>,
    rx: Receiver<(Purpose, Option<String>)>,
    /// A file dialog is open.
    pub picking: bool,
    pub prompt: Option<Prompt>,
    /// Waiting on "save the changes?".
    pub confirm: Option<Then>,
    /// After the save under way, carry on with this.
    after_save: Option<Then>,
}

impl Default for Files {
    fn default() -> Files {
        let (tx, rx) = channel();
        Files { picker: None, tx, rx, picking: false, prompt: None, confirm: None, after_save: None }
    }
}

/// New, open, a recent project or a sample, asking first when there are unsaved changes.
pub fn request(app: &mut EmcraftApp, then: Then) {
    if app.busy() {
        app.error("busy: wait for the running job or cancel it first");
        return;
    }
    if app.session.is_dirty() {
        app.files.confirm = Some(then);
    } else {
        go(app, then);
    }
}

fn go(app: &mut EmcraftApp, then: Then) {
    match then {
        Then::New => new_project(app),
        Then::Open => pick(app, Purpose::Open),
        Then::OpenPath(path) => {
            let _ = app.run("file.open", json!({"path": path}));
        }
        // Opened at once, not solved: Run (step 7) solves it when wanted.
        Then::Sample(name) => {
            let _ = app.run("file.sample", json!({"name": name}));
        }
    }
}

/// An empty project in the preferred units.
fn new_project(app: &mut EmcraftApp) {
    let units = json!({"length": app.prefs.length, "frequency": app.prefs.frequency});
    if app.session.display().length != app.prefs.length || app.session.display().frequency != app.prefs.frequency {
        let _ = app.run("units.set", units);
    }
    let _ = app.run("file.new", json!({}));
}

/// Saves to the project's own file, or asks where.
pub fn save(app: &mut EmcraftApp) {
    if app.session.path().is_some() {
        let _ = app.run("file.save", json!({}));
    } else {
        pick(app, Purpose::SaveAs);
    }
}

pub fn save_as(app: &mut EmcraftApp) {
    pick(app, Purpose::SaveAs);
}

fn pick(app: &mut EmcraftApp, purpose: Purpose) {
    let suggested = format!("{}.{}", app.session.name(), emcraft_engine::project::EXTENSION);
    let Some(picker) = app.files.picker.clone() else {
        let text = match purpose {
            Purpose::Open => app.prefs.recent.first().cloned().unwrap_or_default(),
            Purpose::SaveAs => app.session.path().map_or(suggested, str::to_owned),
        };
        app.files.prompt = Some(Prompt { purpose, text });
        return;
    };
    let (tx, ctx) = (app.files.tx.clone(), app.ctx.clone());
    let done: Box<dyn FnOnce(Option<String>) + Send> = Box::new(move |path| {
        let _ = tx.send((purpose, path));
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    });
    app.files.picking = true;
    match purpose {
        Purpose::Open => picker.open(done),
        Purpose::SaveAs => picker.save(&suggested, done),
    }
}

/// A path chosen.
fn chosen(app: &mut EmcraftApp, purpose: Purpose, path: Option<String>) {
    app.files.picking = false;
    let Some(path) = path.filter(|p| !p.trim().is_empty()) else {
        app.files.after_save = None;
        return;
    };
    match purpose {
        Purpose::Open => {
            let _ = app.run("file.open", json!({"path": path}));
        }
        Purpose::SaveAs => {
            let saved = app.run("file.save", json!({"path": path}));
            continue_after_save(app, saved.is_ok());
        }
    }
}

fn continue_after_save(app: &mut EmcraftApp, saved: bool) {
    match app.files.after_save.take() {
        Some(then) if saved => go(app, then),
        _ => {}
    }
}

/// Once a frame: the file dialog's answer and the windows of this module.
pub fn show(app: &mut EmcraftApp, ctx: &egui::Context) {
    while let Ok((purpose, path)) = app.files.rx.try_recv() {
        chosen(app, purpose, path);
    }
    prompt(app, ctx);
    confirm(app, ctx);
}

/// The path field, when there is no file dialog.
fn prompt(app: &mut EmcraftApp, ctx: &egui::Context) {
    let Some(mut p) = app.files.prompt.take() else { return };
    let t = Tokens::get();
    let (mut ok, mut cancel) = (false, false);
    let title = match p.purpose {
        Purpose::Open => "Open project",
        Purpose::SaveAs => "Save project as",
    };
    let at = ctx.content_rect().center_top() + vec2(-220.0, 120.0);
    egui::Area::new(egui::Id::new("emcraft_file_prompt")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        t.card(t.dialog_body).inner_margin(14).show(ui, |ui| {
            ui.set_width(440.0);
            ui.label(egui::RichText::new(title).size(t.font_label).strong().color(t.form_ink));
            ui.label(egui::RichText::new("The project file (.emcraft)").size(t.font_small).color(t.text_dim));
            let r = ui.add(egui::TextEdit::singleline(&mut p.text).desired_width(420.0).char_limit(4096));
            scenario::publish_target("file:path", r.rect);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ok = true;
            }
            ui.horizontal(|ui| {
                ok |= ui_kit::button(ui, if p.purpose == Purpose::Open { "Open" } else { "Save" }, true, true, "file:ok").clicked();
                cancel = ui_kit::button(ui, "Cancel", false, true, "file:cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
        });
    });
    if cancel {
        app.files.after_save = None;
    } else if ok {
        chosen(app, p.purpose, Some(p.text.trim().to_owned()));
    } else {
        app.files.prompt = Some(p);
    }
}

/// "Save the changes to … first?": Save, Don't save, Cancel.
fn confirm(app: &mut EmcraftApp, ctx: &egui::Context) {
    let Some(then) = app.files.confirm.take() else { return };
    let t = Tokens::get();
    let (mut save, mut discard, mut cancel) = (false, false, false);
    let at = ctx.content_rect().center_top() + vec2(-200.0, 120.0);
    egui::Area::new(egui::Id::new("emcraft_file_confirm")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        t.card(t.dialog_body).inner_margin(14).show(ui, |ui| {
            ui.set_width(400.0);
            ui.label(egui::RichText::new(format!("Save the changes to {}?", app.session.name())).size(t.font_label).strong().color(t.form_ink));
            ui.label(egui::RichText::new("They are lost otherwise.").size(t.font_small).color(t.text_dim));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                save = ui_kit::button(ui, "Save", true, true, "confirm:save").clicked();
                discard = ui_kit::button(ui, "Don't save", false, true, "confirm:discard").clicked();
                cancel = ui_kit::button(ui, "Cancel", false, true, "confirm:cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
        });
    });
    if cancel {
    } else if discard {
        go(app, then);
    } else if save {
        app.files.after_save = Some(then);
        if app.session.path().is_some() {
            let saved = app.run("file.save", json!({}));
            continue_after_save(app, saved.is_ok());
        } else {
            pick(app, Purpose::SaveAs);
        }
    } else {
        app.files.confirm = Some(then);
    }
}

/// After a successful open or save: the file joins the recent list and the start page closes.
pub fn opened(app: &mut EmcraftApp, path: &str) {
    app.prefs.add_recent(path);
    app.home.open = false;
}

/// The window's title: the project's name, with a dot while it has unsaved changes.
pub fn title(app: &EmcraftApp) -> String {
    format!("{}{}", app.session.name(), if app.session.is_dirty() { " •" } else { "" })
}

/// The File menu's items.
pub fn menu(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    ui.set_min_width(240.0);
    let busy = app.busy();
    let item = |ui: &mut egui::Ui, text: &str, keys: &str, handle: &str, enabled: bool| {
        let r = ui.add_enabled(enabled, egui::Button::new(text).shortcut_text(keys));
        scenario::publish_target(handle, r.rect);
        r.clicked()
    };
    if item(ui, "New project", "Ctrl+N", "file:new", !busy) {
        request(app, Then::New);
    }
    if item(ui, "Open…", "Ctrl+O", "file:open", !busy) {
        request(app, Then::Open);
    }
    let recent = app.prefs.recent.clone();
    ui.add_enabled_ui(!busy && !recent.is_empty(), |ui| {
        ui.menu_button("Open recent", |ui| {
            for path in &recent {
                if ui.button(path).clicked() {
                    request(app, Then::OpenPath(path.clone()));
                }
            }
            ui.separator();
            if ui.button("Clear the list").clicked() {
                app.prefs.recent.clear();
            }
        });
    });
    ui.menu_button("Samples", |ui| {
        for s in emcraft_engine::sample::SAMPLES {
            if ui.add_enabled(!busy, egui::Button::new(s.name)).on_hover_text(s.label).clicked() {
                request(app, Then::Sample(s.name.to_owned()));
            }
        }
    });
    ui.separator();
    if item(ui, "Save", "Ctrl+S", "file:save", !busy) {
        save(app);
    }
    if item(ui, "Save as…", "Ctrl+Shift+S", "file:save_as", !busy) {
        save_as(app);
    }
    ui.separator();
    if item(ui, "Export history as script…", "", "file:export", !app.session.history().is_empty()) {
        app.export = Some("emcraft-history.json".into());
    }
    if item(ui, "Start page", "", "file:start", true) {
        app.home.open = true;
    }
}

/// Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+Shift+S.
pub fn shortcuts(app: &mut EmcraftApp, ctx: &egui::Context) {
    let (new, open, save_now, save_new) = ctx.input(|i| {
        let c = i.modifiers.command;
        (
            c && i.key_pressed(egui::Key::N),
            c && i.key_pressed(egui::Key::O),
            c && !i.modifiers.shift && i.key_pressed(egui::Key::S),
            c && i.modifiers.shift && i.key_pressed(egui::Key::S),
        )
    });
    if new {
        request(app, Then::New);
    } else if open {
        request(app, Then::Open);
    } else if save_new {
        save_as(app);
    } else if save_now {
        save(app);
    }
}

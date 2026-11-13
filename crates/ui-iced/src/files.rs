//! Projects as files in the iced window: New, Open, Save and Save As on `.emcraft` files, and
//! the question before changes would be lost (Save, Don't save, Cancel). Paths come from the
//! host's file dialog ([`Picker`], the system's in the app), asked without blocking: its answer
//! comes back as a message. The work itself is the engine's `file.new`, `file.open` and
//! `file.save`.

use std::sync::Arc;

use emcraft_ui_core::autosave;
use emcraft_ui_core::files::{Answer, FileAction, Picker, Purpose, Then};
use emcraft_ui_core::glyph::Glyph;
use emcraft_ui_core::lang::{args, tr, trf};
use iced::widget::{Space, button, center, column, container, opaque, row, text};
use iced::{Element, Fill, Task, Widget};
use serde_json::{Value, json};

use crate::app::{App, Message};
use crate::look;

impl App {
    /// The window after file action `a`, and the file dialog it asks for (its answer a message).
    pub(crate) fn file(&mut self, a: FileAction) -> Task<Message> {
        match a {
            FileAction::Request(then) if self.session.is_dirty() => {
                self.asking = Some(then);
                Task::none()
            }
            FileAction::Request(then) => self.go(then),
            FileAction::Save => self.save(),
            FileAction::SaveAs => self.pick(Purpose::SaveAs),
            FileAction::Chosen(purpose, path) => self.chosen(purpose, path),
            FileAction::Confirm(answer) => self.answered(answer),
            FileAction::PickPart => self.pick(Purpose::Import),
            FileAction::NewDesign => self.pick(Purpose::NewDesign),
            // A part dropped is imported; anything else is opened as a project.
            FileAction::Dropped(path) if emcraft_ui_core::files::is_part(&path) => {
                self.import_part(&path);
                Task::none()
            }
            FileAction::Dropped(path) => self.file(FileAction::Request(Then::OpenPath(path))),
        }
    }

    /// Carries on with `then`, the changes dealt with.
    fn go(&mut self, then: Then) -> Task<Message> {
        match then {
            Then::New => self.opened("file.new", &json!({})),
            Then::Blank(kind) => self.opened("file.new", &json!({ "analysis": kind.id() })),
            Then::Open => return self.pick(Purpose::Open),
            Then::OpenPath(path) => self.opened("file.open", &json!({ "path": path })),
            Then::Sample { name, values, purpose } => {
                // Its parameters as changed, and the analysis chosen for it.
                let mut params = json!({ "name": name });
                if values.as_object().is_some_and(|v| !v.is_empty()) {
                    params["values"] = values;
                }
                if let Some(kind) = purpose {
                    params["analysis"] = json!(kind.id());
                }
                self.opened("file.sample", &params);
            }
            Then::Quit => {
                self.autosave.drop_copy();
                return iced::exit();
            }
            Then::Recover { file, original } => {
                self.autosave.found.retain(|f| f.file.as_os_str() != file.as_str());
                let params = match original {
                    Some(o) => json!({ "path": file, "original": o }),
                    None => json!({ "path": file }),
                };
                self.opened("file.open", &params);
                // The copy is this project's from now on: a save deletes it.
                if self.error.is_none() {
                    self.autosave.file = Some(file.into());
                }
            }
            // A new project with a part to import: a board chosen at once, else the import's
            // dialog open on the geometry's step.
            Then::Import => {
                self.opened("file.new", &json!({}));
                self.home = false;
                if self.purpose == Some(emcraft_ui_core::steps::Solver::Pcb) {
                    return self.pick(Purpose::Import);
                }
                self.step_act(crate::step_panel::Act::Fix(crate::import::ID.into()));
            }
        }
        Task::none()
    }

    /// Runs `id` (a project opened or begun; it shows as a new window would, [`App::after`]).
    /// Project file `path` opened, as Open would.
    pub(crate) fn open_path(&mut self, path: &str) {
        self.opened("file.open", &json!({ "path": path }));
    }

    fn opened(&mut self, id: &str, params: &Value) {
        // The changes before were saved or let go: their copy goes.
        self.autosave.drop_copy();
        self.run_now(id, params);
        if let Some(path) = params.get("path").and_then(Value::as_str).filter(|_| self.error.is_none() && id == "file.open") {
            self.remember(path);
        }
    }

    /// Whether the project has a file of its own to save to (not an autosaved copy it was
    /// recovered from).
    fn has_file(&self) -> bool {
        self.session.path().is_some_and(|p| !autosave::inside(self.autosave.dir.as_deref(), p))
    }

    /// `path` first in the recent projects (an autosaved copy is none), the preferences kept.
    fn remember(&mut self, path: &str) {
        if autosave::inside(self.autosave.dir.as_deref(), path) {
            return;
        }
        self.prefs.add_recent(path);
        if let Some(p) = &self.prefs_path {
            let _ = self.prefs.save(p);
        }
    }

    /// After a save: its copy goes and the file joins the recent projects.
    fn saved(&mut self) {
        if self.error.is_none() && !self.session.is_dirty() {
            self.autosave.drop_copy();
            if let Some(p) = self.session.path().map(str::to_owned) {
                self.remember(&p);
            }
        }
    }

    /// Saves to the project's own file, or asks where.
    fn save(&mut self) -> Task<Message> {
        if !self.has_file() {
            return self.pick(Purpose::SaveAs);
        }
        self.run_now("file.save", &json!({}));
        self.saved();
        self.after_saved()
    }

    /// After a save that `asking` waited for: what it waited for, when the save went well.
    fn after_saved(&mut self) -> Task<Message> {
        match self.after_save.take() {
            Some(then) if self.error.is_none() && !self.session.is_dirty() => self.go(then),
            _ => Task::none(),
        }
    }

    /// The host's file dialog for `purpose`, its answer a message; none in a window without one.
    fn pick(&mut self, purpose: Purpose) -> Task<Message> {
        let Some(picker) = self.picker.clone() else {
            self.fail(tr(t!("No file dialog here: open a project from the command line.")).to_owned());
            return Task::none();
        };
        let suggested = match purpose {
            Purpose::NewDesign => "design.solidcraft".to_owned(),
            _ => format!("{}.{}", self.session.name(), emcraft_engine::project::EXTENSION),
        };
        Task::perform(asked(picker, purpose, suggested), move |path| Message::File(FileAction::Chosen(purpose, path)))
    }

    /// A path chosen (`None`: cancelled) for `purpose`.
    fn chosen(&mut self, purpose: Purpose, path: Option<String>) -> Task<Message> {
        match (purpose, path) {
            (Purpose::Open, Some(p)) => {
                self.opened("file.open", &json!({ "path": p }));
                Task::none()
            }
            (Purpose::Import, Some(p)) => {
                self.import_part(&p);
                Task::none()
            }
            (Purpose::NewDesign, Some(p)) => {
                self.new_design(&p);
                Task::none()
            }
            (Purpose::SaveAs, Some(p)) => {
                self.run_now("file.save", &json!({ "path": p }));
                self.saved();
                self.after_saved()
            }
            // Cancelled: what waited for the save waits no more.
            _ => {
                self.after_save = None;
                Task::none()
            }
        }
    }

    /// The answer to "save the changes?".
    fn answered(&mut self, answer: Answer) -> Task<Message> {
        let Some(then) = self.asking.take() else { return Task::none() };
        match answer {
            Answer::Cancel | Answer::StopJob => Task::none(),
            Answer::Discard => {
                self.autosave.drop_copy();
                self.go(then)
            }
            Answer::Save => {
                self.after_save = Some(then);
                self.save()
            }
        }
    }
}

/// `picker`'s answer for `purpose`, awaited without blocking the window: the dialog runs on its
/// own thread and sends the path back.
async fn asked(picker: Arc<dyn Picker>, purpose: Purpose, suggested: String) -> Option<String> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    let done: Box<dyn FnOnce(Option<String>) + Send> = Box::new(move |path| {
        let _ = tx.send(path);
    });
    match purpose {
        Purpose::Open => picker.open(done),
        Purpose::SaveAs | Purpose::NewDesign => picker.save(&suggested, done),
        Purpose::Import => picker.import(done),
    }
    rx.await.ok().flatten()
}

/// The window's keys (when no field takes them; [`emcraft_ui_core::shortcuts::KEYS`] says
/// them): Ctrl+N new, Ctrl+O open, Ctrl+S save (Shift: save as), Ctrl+Z undo, Ctrl+Shift+Z or
/// Ctrl+Y redo (Cmd on macOS).
pub fn shortcut(event: iced::Event, status: iced::event::Status, _: iced::window::Id) -> Option<Message> {
    use iced::keyboard::{Event, Key};
    let iced::Event::Keyboard(Event::KeyPressed { key: Key::Character(c), modifiers, .. }) = event else { return None };
    if status == iced::event::Status::Captured || !modifiers.command() {
        return None;
    }
    let shift = modifiers.shift();
    Some(match (c.as_str(), shift) {
        ("n", false) => Message::File(FileAction::Request(Then::New)),
        ("o", false) => Message::File(FileAction::Request(Then::Open)),
        ("s", false) => Message::File(FileAction::Save),
        ("s", true) => Message::File(FileAction::SaveAs),
        ("z", false) => Message::Start("edit.undo".into()),
        ("z", true) | ("y", false) => Message::Start("edit.redo".into()),
        _ => return None,
    })
}

/// A file dropped on the window: a part imported, a project opened.
pub fn dropped(event: iced::Event, _: iced::event::Status, _: iced::window::Id) -> Option<Message> {
    let iced::Event::Window(iced::window::Event::FileDropped(path)) = event else { return None };
    Some(Message::File(FileAction::Dropped(path.to_string_lossy().into_owned())))
}

/// Undo and Redo in the window's bar, each while there is something to take back or do again.
pub fn edits(app: &App) -> impl Iterator<Item = Element<'_, Message>> {
    let t = app.look.tokens;
    let idle = app.job.is_none();
    [("edit.undo", Glyph::Undo, !app.session.undo().is_empty()), ("edit.redo", Glyph::Redo, !app.session.redo().is_empty())].into_iter().map(
        move |(id, glyph, can)| {
            let label = emcraft_ui_core::lang::command_label(id).to_owned();
            crate::icons::tool_button(
                t,
                crate::icons::ToolFace { glyph, name: label, key: Some(id), on: false, place: crate::icons::On::Bar },
                (idle && can).then(|| Message::Start(id.to_owned())),
            )
        },
    )
}

/// The File buttons in the window's bar: New, Open, Save, Save As.
pub fn buttons(app: &App) -> impl Iterator<Item = Element<'_, Message>> {
    let t = app.look.tokens;
    let idle = app.job.is_none();
    [
        ("file.new", Glyph::New, FileAction::Request(Then::New)),
        ("file.open", Glyph::OpenFile, FileAction::Request(Then::Open)),
        ("file.save", Glyph::Save, FileAction::Save),
        ("file.save_as", Glyph::SaveAs, FileAction::SaveAs),
    ]
    .into_iter()
    .map(move |(id, glyph, action)| {
        let label = match id {
            "file.save_as" => tr(t!("Save as…")).to_owned(),
            _ => emcraft_ui_core::lang::command_label(id).to_owned(),
        };
        crate::icons::tool_button(
            t,
            crate::icons::ToolFace { glyph, name: label, key: Some(id), on: false, place: crate::icons::On::Bar },
            idle.then_some(Message::File(action)),
        )
    })
}

/// The recent projects, one chosen opened (asking first about unsaved changes).
pub fn recent(app: &App) -> Option<Element<'_, Message>> {
    let t = app.look.tokens;
    let list = app.prefs.recent.clone();
    (!list.is_empty() && app.job.is_none()).then(|| {
        iced::widget::pick_list(None::<String>, list, |p: &String| {
            std::path::Path::new(p).file_name().map_or_else(|| p.clone(), |n| n.to_string_lossy().into_owned())
        })
        .placeholder(tr(t!("Open recent")))
        .on_select(|p| Message::File(FileAction::Request(Then::OpenPath(p))))
        .text_size(t.font_button)
        .style(look::choice(t))
        .boxed()
    })
}

/// The autosaved copies found on start, newer than their projects: each with Recover and a mark
/// that lets it go.
pub fn recoveries(app: &App) -> Option<Element<'_, Message>> {
    let t = app.look.tokens;
    let found = &app.autosave.found;
    (!found.is_empty()).then(|| {
        let rows = found.iter().enumerate().take(4).map(|(i, f)| {
            let what = match &f.original {
                Some(o) => trf(t!("Autosaved copy of {project}\n{file}"), args!(project = o, file = f.file.display())),
                None => trf(t!("Autosaved copy of a project never saved\n{file}"), args!(file = f.file.display())),
            };
            let then = Then::Recover { file: f.file.display().to_string(), original: f.original.clone() };
            row![
                column![text(f.name.clone()).size(t.font_body), text(what).size(t.font_small).color(t.text_dim)].spacing(t.space_2).width(Fill),
                text(autosave::ago(f.modified)).size(t.font_small).color(t.text_dim),
                button(text(tr(t!("Recover"))).size(t.font_button)).style(look::button(t, true)).on_press(Message::File(FileAction::Request(then))),
                crate::parts::close(t, t!("Dismiss"), Message::Dismiss(i)),
            ]
            .spacing(t.space_8)
            .align_y(iced::Alignment::Center)
            .boxed()
        });
        container(iced::widget::Column::with_children(rows).spacing(t.space_6)).padding(t.space_8).width(Fill).style(look::ground(t)).boxed()
    })
}

/// "Save the changes to … ?" over the window, while one is asked.
pub fn question(app: &App) -> Option<Element<'_, Message>> {
    app.asking.as_ref()?;
    let t = app.look.tokens;
    let answer = |label, primary, a| {
        button(text(tr(label)).size(t.font_button)).style(look::button(t, primary)).on_press(Message::File(FileAction::Confirm(a)))
    };
    let card = column![
        text(trf(t!("Save the changes to {name}?"), args!(name = app.session.name()))).size(t.font_heading),
        row![
            Space::new().width(Fill),
            answer(t!("Cancel"), false, Answer::Cancel),
            answer(t!("Don't save"), false, Answer::Discard),
            answer(t!("Save"), true, Answer::Save)
        ]
        .spacing(t.space_8),
    ]
    .spacing(t.space_16);
    let card = container(card).padding(t.space_16).width(460).style(look::ground(t));
    let veil = container(center(card)).width(Fill).height(Fill).style(look::veil);
    Some(opaque(veil).boxed())
}

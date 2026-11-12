//! Projects as files in the iced window: New, Open, Save and Save As on `.emcraft` files, and
//! the question before changes would be lost (Save, Don't save, Cancel). Paths come from the
//! host's file dialog ([`Picker`], the system's in the app), asked without blocking: its answer
//! comes back as a message. The work itself is the engine's `file.new`, `file.open` and
//! `file.save`.

use std::sync::Arc;

use emcraft_ui_core::files::{Answer, FileAction, Picker, Purpose, Then};
use emcraft_ui_core::lang::{args, tr, trf};
use iced::widget::{Space, button, center, column, container, opaque, row, text};
use iced::{Color, Element, Fill, Task, Widget};
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
            FileAction::PickPart | FileAction::Dropped(_) => Task::none(),
        }
    }

    /// Carries on with `then`, the changes dealt with.
    fn go(&mut self, then: Then) -> Task<Message> {
        match then {
            Then::New => self.opened("file.new", &json!({})),
            Then::Open => return self.pick(Purpose::Open),
            Then::OpenPath(path) => self.opened("file.open", &json!({ "path": path })),
            Then::Sample { name, .. } => self.opened("file.sample", &json!({ "name": name })),
            Then::Quit => return iced::exit(),
            Then::Recover { .. } | Then::Import => {}
        }
        Task::none()
    }

    /// Runs `id` (a project opened or begun) and shows it as a new window would: nothing
    /// selected, its chain's page for a system, else the model.
    fn opened(&mut self, id: &str, params: &Value) {
        self.run_now(id, params);
        if self.error.is_none() {
            self.selection = Default::default();
            self.dialog = None;
            self.page =
                if self.session.model().system.stages.is_empty() { emcraft_ui_core::view::View::Model } else { emcraft_ui_core::view::View::System };
            self.tab = crate::ribbon::first_tab(self);
        }
    }

    /// Whether the project has a file of its own to save to.
    fn has_file(&self) -> bool {
        self.session.path().is_some()
    }

    /// Saves to the project's own file, or asks where.
    fn save(&mut self) -> Task<Message> {
        if !self.has_file() {
            return self.pick(Purpose::SaveAs);
        }
        self.run_now("file.save", &json!({}));
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
            self.error = Some(tr(t!("No file dialog here: open a project from the command line.")).to_owned());
            return Task::none();
        };
        let suggested = format!("{}.{}", self.session.name(), emcraft_engine::project::EXTENSION);
        Task::perform(asked(picker, purpose, suggested), move |path| Message::File(FileAction::Chosen(purpose, path)))
    }

    /// A path chosen (`None`: cancelled) for `purpose`.
    fn chosen(&mut self, purpose: Purpose, path: Option<String>) -> Task<Message> {
        match (purpose, path) {
            (Purpose::Open, Some(p)) => {
                self.opened("file.open", &json!({ "path": p }));
                Task::none()
            }
            (Purpose::SaveAs, Some(p)) => {
                self.run_now("file.save", &json!({ "path": p }));
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
            Answer::Discard => self.go(then),
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
        Purpose::SaveAs => picker.save(&suggested, done),
        Purpose::Import => picker.import(done),
    }
    rx.await.ok().flatten()
}

/// The File buttons in the window's bar: New, Open, Save, Save As.
pub fn buttons(app: &App) -> impl Iterator<Item = Element<'_, Message>> {
    let t = app.look.tokens;
    let idle = app.job.is_none();
    [
        ("file.new", FileAction::Request(Then::New)),
        ("file.open", FileAction::Request(Then::Open)),
        ("file.save", FileAction::Save),
        ("", FileAction::SaveAs),
    ]
    .into_iter()
    .map(move |(id, action)| {
        let label = match id {
            "" => tr(t!("Save as…")).to_owned(),
            _ => emcraft_ui_core::lang::command_label(id).to_owned(),
        };
        button(text(label).size(t.font_button)).style(look::bar_button(t)).on_press_maybe(idle.then_some(Message::File(action))).boxed()
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
        .spacing(8),
    ]
    .spacing(16);
    let card = container(card).padding(18).width(460).style(look::ground(t));
    let veil = container(center(card))
        .width(Fill)
        .height(Fill)
        .style(|_: &iced::Theme| container::Style { background: Some(Color { a: 0.35, ..Color::BLACK }.into()), ..container::Style::default() });
    Some(opaque(veil).boxed())
}

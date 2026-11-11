//! The window's state, the messages that change it and what each ends in: a field's draft, a
//! gesture's next state ([`Canvas::step`], from the shared crate), a command run on the session
//! and the chain read again after it.

use std::collections::BTreeMap;

use emcraft_engine::Session;
use emcraft_engine::view::SystemChart;
use emcraft_ui_core::geom::Point;
use emcraft_ui_core::system::band;
use emcraft_ui_core::system::card::{self, Edit};
use emcraft_ui_core::system::gesture::{Canvas, Command, Input};
use emcraft_ui_core::system::layout::Layout;
use std::rc::Rc;

use emcraft_ui_core::theme::{Theme, Themes};
use iced::widget::{column, container, row, text};
use iced::{Element, Fill, Widget};

use crate::look::{self, Look};
use serde_json::json;

/// The band's fields' node, in a [`Field`].
pub const BAND: &str = "@band";

/// A field typed in place: its node (a card's name, or [`BAND`]) and the parameter it sets.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Field {
    pub node: String,
    pub key: &'static str,
}

/// What the pointer did on the canvas, where (canvas points).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pointer {
    Press(Point),
    Move(Point),
    Release(Point),
}

/// What changes the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /// A field's text typed.
    Typed(Field, String),
    /// A field let go (Enter): its text committed if it changed.
    Commit(Field),
    /// A name chosen in a card's row.
    Chose(Field, String),
    /// The pointer on the empty canvas.
    Pointer(Pointer),
    /// A stage's header pressed, to move it.
    Grab(String),
    /// A node's output pressed, to link it.
    StartLink(String),
    /// A command a button runs (a stage added or deleted, a link taken away).
    Run(Command),
    /// The chart shown under the canvas.
    Chart(SystemChart),
}

/// The window: the project, the canvas between frames, the drafts typed and what went wrong.
pub struct App {
    pub(crate) session: Session,
    pub(crate) canvas: Canvas,
    /// Where the pointer was last on the canvas.
    pub(crate) pointer: Option<Point>,
    pub(crate) drafts: BTreeMap<Field, String>,
    pub(crate) chart: SystemChart,
    /// The last command's refusal.
    pub(crate) error: Option<String>,
    /// Why the chain could not be read after the last change.
    pub(crate) unread: Option<String>,
    /// The theme the window is dressed in.
    pub(crate) look: Look,
}

impl App {
    /// The window on sample `sample` (its chain read), or on an empty project.
    pub fn new(sample: Option<&str>) -> App {
        let mut session = Session::new();
        let error = sample.and_then(|name| session.execute("file.sample", &json!({ "name": name })).err()).map(|e| e.to_string());
        let look = Look::of(Themes::load(None).get("light"));
        let canvas = Canvas::default();
        let mut app = App { session, canvas, pointer: None, drafts: BTreeMap::new(), chart: SystemChart::Along, error, unread: None, look };
        app.read();
        app
    }

    /// The window dressed in `theme`.
    pub fn dressed(self, theme: Rc<Theme>) -> App {
        App { look: Look::of(theme), ..self }
    }

    /// iced's theme from the window's.
    pub fn theme(&self) -> iced::Theme {
        self.look.iced()
    }

    pub fn title(&self) -> String {
        format!("EMCraft — {}", self.session.name())
    }

    /// The canvas laid out now, a node being moved where its drag has it.
    pub fn layout(&self) -> Layout {
        Layout::of(&self.session.model().system, Point::ORIGIN + self.canvas.pan, self.canvas.moving())
    }

    /// The text field `field` shows: what is being typed, else the model's value.
    pub fn draft(&self, field: &Field, value: Option<f64>) -> String {
        self.drafts.get(field).cloned().unwrap_or_else(|| card::shown(value))
    }

    pub fn update(&mut self, message: Message) {
        if let Some(command) = self.next(message) {
            self.run(command);
        }
    }

    /// The window: the application bar over the System view.
    pub fn view(&self) -> Element<'_, Message> {
        let t = self.look.tokens;
        let bar = row![text("EMCraft").size(t.font_body), text(self.session.name()).size(t.font_body).color(t.app_bar_text)].spacing(16);
        let bar = container(bar).padding([6, 14]).width(Fill).style(look::app_bar(t));
        container(column![bar, crate::system::view(self)]).width(Fill).height(Fill).style(look::window(t)).boxed()
    }

    /// The window after `message`, and the command it ends in.
    fn next(&mut self, message: Message) -> Option<Command> {
        match message {
            Message::Typed(field, text) => {
                self.drafts.insert(field, text);
                None
            }
            Message::Commit(field) => {
                let text = self.drafts.remove(&field)?;
                self.committed(&field, &text)
            }
            Message::Chose(field, value) => {
                let layout = self.layout();
                Some(card::command(layout.node(&field.node)?, field.key, json!(value)))
            }
            Message::Pointer(p) => self.pointer(p),
            Message::Grab(name) => self.step(Input::Grab(name)),
            Message::StartLink(name) => self.step(Input::StartLink(name)),
            Message::Run(command) => Some(command),
            Message::Chart(c) => {
                self.chart = c;
                None
            }
        }
    }

    /// The command `field` typed as `text` runs, when it changed: the band confirmed, a card's
    /// setting set.
    fn committed(&self, field: &Field, text: &str) -> Option<Command> {
        if field.node == BAND {
            let now = band::shown(&self.session);
            let value = card::committed(text, band::field(now, field.key), field.key == "frequency")?;
            return band::setup(now, field.key, value).map(|params| Command { id: "project.setup", params });
        }
        let layout = self.layout();
        let node = layout.node(&field.node)?;
        let rows = card::rows(&self.session, node);
        let Edit::Number { value, optional, .. } = rows.into_iter().find(|r| r.key == field.key)?.value else { return None };
        card::committed(text, value, optional).map(|v| card::command(node, field.key, json!(v)))
    }

    /// What the pointer at `p` reports to the gestures: a press on the empty canvas, a drag by
    /// how far it moved, a release.
    fn pointer(&mut self, p: Pointer) -> Option<Command> {
        let (input, at) = match p {
            Pointer::Press(at) => (Some(Input::PressEmpty), at),
            Pointer::Move(at) => (self.pointer.map(|last| Input::Drag(at - last)), at),
            Pointer::Release(at) => (Some(Input::Release(at)), at),
        };
        self.pointer = Some(at);
        self.step(input?)
    }

    /// The canvas after `input` (`Canvas::step`), and the command its gesture ends in.
    fn step(&mut self, input: Input) -> Option<Command> {
        let layout = self.layout();
        let (canvas, command) = std::mem::take(&mut self.canvas).step(input, &layout);
        self.canvas = canvas;
        command
    }

    /// Runs `command` on the project; the chain read again after a change.
    fn run(&mut self, command: Command) {
        match self.session.execute(command.id, &command.params) {
            Ok(_) => {
                self.error = None;
                self.read();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Reads the chain again (`solve.system`, a moment), when it has stages: what keeps it from
    /// reading is kept to show.
    fn read(&mut self) {
        self.unread = if self.session.model().system.stages.is_empty() {
            None
        } else {
            self.session.execute("solve.system", &json!({})).err().map(|e| e.to_string())
        };
    }
}

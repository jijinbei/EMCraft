//! The window's state, the messages that change it and what each ends in: a field's draft, a
//! gesture's next state ([`Canvas::step`], from the shared crate), the 3D view's next camera, a
//! command run on the session (a long one on the worker, [`crate::run`]) and the chain and the
//! scene read again after it.

use std::collections::BTreeMap;

use emcraft_engine::Session;
use emcraft_engine::view::SystemChart;
use emcraft_ui_core::geom::Point;
use emcraft_ui_core::system::band;
use emcraft_ui_core::system::card::{self, Edit};
use emcraft_ui_core::system::gesture::{Canvas, Command, Input};
use emcraft_ui_core::system::layout::Layout;
use std::rc::Rc;

use emcraft_ui_core::browser::{self, Row};
use emcraft_ui_core::job::Job;
use emcraft_ui_core::navigation::NavPreset;
use emcraft_ui_core::prefs::{GpuLook, Prefs};
use emcraft_ui_core::replies::Replies;
use emcraft_ui_core::selection::{self, Selection};
use emcraft_ui_core::theme::{Theme, Themes};
use emcraft_ui_core::view::View;
use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Element, Fill, Subscription, Task, Widget};

use crate::look::{self, Look};
use crate::run::{PAGES, cancel_label, page_label};
use crate::view3d::shader::Shot;
use crate::view3d::{Click, Nav, Shown, View3d};
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
#[derive(Clone, Debug)]
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
    /// The page shown.
    Page(View),
    /// The 3D view's camera or what it shows changed.
    View3d(Nav),
    /// A click on the 3D view: what is under it picked.
    Click(Click),
    /// A command started from a button: a long one on the worker.
    Start(String),
    /// The running job stopped.
    Cancel,
    /// A frame drawn while a job runs (it is looked at) or a picture of the window is waited for.
    Tick,
    /// The picture of the window taken ([`App::shoot`]).
    Shot(iced::window::Screenshot),
}

/// Frames drawn before the window's picture is taken: its first frames settled.
const SHOOT_AFTER: u32 = 20;

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
    /// The page shown.
    pub(crate) page: View,
    pub(crate) view3d: View3d,
    /// What is selected in the 3D view, and its rows (the browser's) for the generation and
    /// selection they were read for.
    pub(crate) selection: Selection,
    pub(crate) picked: Option<((u64, Selection), Vec<Row>)>,
    /// The charts as drawn.
    pub(crate) charts: crate::charts::Charts,
    /// Raised by every command that ends well (the model's revision does not follow a solve's
    /// results): what is drawn from the project is drawn again when it moves.
    pub(crate) generation: u64,
    /// The command running on the worker.
    pub(crate) job: Option<Job>,
    /// What the last job did (its name and time).
    pub(crate) note: Option<String>,
    /// The 3D view's mouse mapping and drawing (the preferences').
    pub(crate) nav: &'static NavPreset,
    pub(crate) gpu_look: GpuLook,
    /// Where to write a picture of the window once it has drawn, and the frames drawn so far.
    shoot: Option<(std::path::PathBuf, u32)>,
}

impl App {
    /// The window on sample `sample` (its chain read), or on an empty project.
    pub fn new(sample: Option<&str>) -> App {
        let mut session = Session::new();
        let error = sample.and_then(|name| session.execute("file.sample", &json!({ "name": name })).err()).map(|e| e.to_string());
        let look = Look::of(Themes::load(None).get("light"));
        let page = if session.model().system.stages.is_empty() { View::Model } else { View::System };
        let prefs = Prefs::default();
        let mut app = App {
            session,
            canvas: Canvas::default(),
            pointer: None,
            drafts: BTreeMap::new(),
            chart: SystemChart::Along,
            error,
            unread: None,
            look,
            page,
            view3d: View3d::default(),
            selection: Selection::default(),
            picked: None,
            charts: crate::charts::Charts::default(),
            generation: 0,
            job: None,
            note: None,
            nav: prefs.nav(),
            gpu_look: prefs.gpu_look,
            shoot: None,
        };
        app.read();
        app.settle();
        app
    }

    /// The window dressed in `theme`.
    pub fn dressed(self, theme: Rc<Theme>) -> App {
        let mut app = App { look: Look::of(theme), ..self };
        app.settle();
        app
    }

    /// The window in the preferences' mouse mapping and 3D drawing.
    pub fn with_prefs(self, prefs: &Prefs) -> App {
        App { nav: prefs.nav(), gpu_look: prefs.gpu_look, ..self }
    }

    /// The window that writes a picture of itself to `path` once it has drawn, then closes.
    pub fn shooting(self, path: Option<std::path::PathBuf>) -> App {
        App { shoot: path.map(|p| (p, 0)), ..self }
    }

    /// What is drawn from the project made again where it changed: the 3D view's scene and the
    /// charts, in the window's theme.
    fn settle(&mut self) {
        let src = crate::view3d::Source {
            session: &self.session,
            generation: self.generation,
            selected: &self.selection,
            theme: &self.look.theme,
            gpu_field: self.gpu_look.field,
        };
        self.view3d = std::mem::take(&mut self.view3d).refreshed(&src);
        let key = (self.generation, self.selection.clone());
        if self.picked.as_ref().is_none_or(|(k, _)| *k != key) {
            let rows = browser::rows_of(&self.session, &Replies::default(), &self.selection).into_iter().map(|(_, r)| r).collect();
            self.picked = Some((key, rows));
        }
        self.charts = std::mem::take(&mut self.charts).refreshed(&self.session, self.generation, &self.look, self.chart);
    }

    /// What the 3D view draws of `shown`: through its camera, on the theme's ground, with the
    /// preferences' drawing.
    pub fn shot(&self, shown: &Shown) -> Shot {
        let t = &self.look.theme.tokens;
        let g = self.gpu_look;
        Shot {
            packed: shown.packed.clone(),
            camera: self.view3d.camera,
            ground: (t.viewport_top.rgb(), t.viewport_bottom.rgb()),
            look: emcraft_gpu3d::Look { edges: g.edges, ao: g.ao, outline: false, pick: false },
            field: shown.field.as_ref().map(|(grid, scale)| emcraft_gpu3d::FieldFrame {
                grid: grid.clone(),
                phase: self.view3d.field.phase,
                scale: *scale,
                contours: self.view3d.field.contours,
            }),
            moving: false,
        }
    }

    /// What the window listens to: each frame while a job runs or its picture is waited for.
    pub fn subscription(&self) -> Subscription<Message> {
        if self.job.is_some() || self.shoot.is_some() { iced::window::frames().map(|_| Message::Tick) } else { Subscription::none() }
    }

    /// The window after `message`, and what iced does next: the picture of the window taken
    /// and written, then the window closed.
    pub fn react(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick if self.shoot.is_some() => {
                self.tick();
                let (path, frames) = self.shoot.take().unwrap_or_default();
                let ready = frames >= SHOOT_AFTER && self.job.is_none();
                self.shoot = Some((path, frames + 1));
                if ready {
                    return iced::window::latest().and_then(iced::window::screenshot).map(Message::Shot);
                }
                Task::none()
            }
            Message::Shot(shot) => {
                let canvas = emcraft_render::Canvas { w: shot.size.width as usize, h: shot.size.height as usize, rgba: shot.rgba.to_vec() };
                if let (Some((path, _)), Some(png)) = (self.shoot.take(), canvas.png())
                    && let Err(e) = std::fs::write(&path, png)
                {
                    eprintln!("emcraft-iced: {}: {e}", path.display());
                }
                iced::exit()
            }
            m => {
                self.update(m);
                Task::none()
            }
        }
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
        self.settle();
    }

    /// The window: the application bar — the project, its pages, the running job — over the page.
    pub fn view(&self) -> Element<'_, Message> {
        let t = self.look.tokens;
        let tabs = PAGES
            .into_iter()
            .map(|v| button(text(page_label(v)).size(t.font_button)).style(look::button(t, v == self.page)).on_press(Message::Page(v)).boxed());
        let status: Vec<Element<Message>> = match (self.progress(), &self.note) {
            (Some(p), _) => vec![
                text(p).size(t.font_body).color(t.app_bar_text).boxed(),
                button(text(cancel_label()).size(t.font_button)).style(look::button(t, false)).on_press(Message::Cancel).boxed(),
            ],
            (None, Some(n)) => vec![text(n.clone()).size(t.font_body).color(t.app_bar_text).boxed()],
            (None, None) => Vec::new(),
        };
        let head = [text("EMCraft").size(t.font_body).boxed(), text(self.session.name()).size(t.font_body).color(t.app_bar_text).boxed()];
        let bar = row(head.into_iter().chain(tabs).chain(std::iter::once(Space::new().width(Fill).boxed())).chain(status))
            .spacing(12)
            .align_y(Alignment::Center);
        let bar = container(bar).padding([6, 14]).width(Fill).style(look::app_bar(t));
        let page = match self.page {
            View::Model => crate::model::view(self),
            View::SParameters => crate::sparameters::view(self),
            _ => crate::system::view(self),
        };
        container(column![bar, page]).width(Fill).height(Fill).style(look::window(t)).boxed()
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
            Message::Page(v) => {
                self.page = v;
                None
            }
            Message::View3d(nav) => {
                self.view3d = std::mem::take(&mut self.view3d).navigated(nav);
                None
            }
            Message::Click(c) => {
                let pick = self.view3d.pick(c.at);
                let click = if c.double { selection::Click::Double { pick } } else { selection::Click::Single { pick, add: c.add } };
                if selection::step(&mut self.selection, click) == selection::Then::Fit {
                    self.view3d = std::mem::take(&mut self.view3d).navigated(Nav::Fit);
                }
                None
            }
            Message::Start(id) => {
                self.start(&id, json!({}));
                None
            }
            Message::Cancel => {
                self.cancel();
                None
            }
            Message::Tick => {
                self.tick();
                None
            }
            Message::Shot(_) => None,
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
                self.after(command.id);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Reads the chain again (`solve.system`, a moment), when it has stages: what keeps it from
    /// reading is kept to show.
    pub(crate) fn read(&mut self) {
        self.unread = if self.session.model().system.stages.is_empty() {
            None
        } else {
            self.session.execute("solve.system", &json!({})).err().map(|e| e.to_string())
        };
    }
}

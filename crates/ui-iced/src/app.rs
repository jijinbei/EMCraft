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

use emcraft_ui_core::browser::{self, Act, Row};
use emcraft_ui_core::dialogs::{self, Dialog};
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

/// The browser's sections (their English names) and rows.
pub type Tree = Vec<(&'static str, Vec<Row>)>;

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
    /// A browser row clicked (by its id): what it names selected, what it shows or runs.
    Row(String),
    /// The selection's edit dialog opened.
    Edit,
    /// The ribbon's tab shown.
    Tab(emcraft_ui_core::ribbon::RibbonTab),
    /// A ribbon command: its dialog opened, or (without parameters) run.
    Command(String),
    /// The items selected deleted.
    Delete,
    /// The open dialog changed, confirmed or cancelled.
    Dialog(crate::dialog::Edit),
    /// New, Open, Save, Save As, a path chosen, the answer to "save the changes?".
    File(emcraft_ui_core::files::FileAction),
    /// A command started from a button: a long one on the worker.
    Start(String),
    /// The running job stopped.
    Cancel,
    /// A frame drawn (when): while a job runs (it is looked at), the field plays or a picture of
    /// the window is waited for.
    Tick(std::time::Instant),
    /// The field's phase played or stopped.
    Play(bool),
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
    /// What is selected (in the 3D view or the browser), and the browser's tree for the
    /// generation it was read for.
    pub(crate) selection: Selection,
    pub(crate) tree: Option<(u64, Tree)>,
    /// The command's dialog open over the window.
    pub(crate) dialog: Option<Dialog>,
    /// The ribbon's tab shown.
    pub(crate) tab: emcraft_ui_core::ribbon::RibbonTab,
    /// The host's file dialog, when it has one.
    pub(crate) picker: Option<std::sync::Arc<dyn emcraft_ui_core::files::Picker>>,
    /// What "save the changes?" is asked before, and what waits for a save.
    pub(crate) asking: Option<emcraft_ui_core::files::Then>,
    pub(crate) after_save: Option<emcraft_ui_core::files::Then>,
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
    /// The field's phase playing: the last frame's time.
    pub(crate) playing: Option<Option<std::time::Instant>>,
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
            tree: None,
            dialog: None,
            tab: Default::default(),
            picker: None,
            asking: None,
            after_save: None,
            charts: crate::charts::Charts::default(),
            generation: 0,
            job: None,
            note: None,
            nav: prefs.nav(),
            gpu_look: prefs.gpu_look,
            playing: None,
            shoot: None,
        };
        app.read();
        app.settle();
        app.tab = crate::ribbon::first_tab(&app);
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

    /// The window with the host's file dialog.
    pub fn with_picker(self, picker: Option<std::sync::Arc<dyn emcraft_ui_core::files::Picker>>) -> App {
        App { picker, ..self }
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
            // The GPU rings what is selected; without its outline the scene lights it.
            lit: !self.gpu_look.outline,
        };
        self.view3d = std::mem::take(&mut self.view3d).refreshed(&src);
        if self.tree.as_ref().is_none_or(|(g, _)| *g != self.generation) {
            self.tree = Some((self.generation, browser::tree(&self.session, &Replies::default())));
        }
        self.charts = std::mem::take(&mut self.charts).refreshed(&self.session, self.generation, &self.look, self.chart);
    }

    /// The browser's rows, in their sections.
    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.tree.iter().flat_map(|(_, t)| t.iter().flat_map(|(_, rows)| rows))
    }

    /// The rows of what is selected, in its order.
    pub fn picked(&self) -> Vec<&Row> {
        self.selection.iter().filter_map(|p| self.rows().find(|r| r.pick == Some(*p))).collect()
    }

    /// Whether row `r` shows selected: its part is, or (a row without one) it was clicked.
    pub fn row_selected(&self, r: &Row) -> bool {
        match r.pick {
            Some(p) => self.selection.contains(&p),
            None => self.selection.row() == Some(r.id.as_str()),
        }
    }

    /// Row `id` clicked: what it names selected alone (its part, else the row), then what it
    /// shows (a page) or runs; the command it runs.
    fn row(&mut self, id: &str) -> Option<Command> {
        let r = self.rows().find(|r| r.id == id)?.clone();
        self.selection = Selection::of(r.pick.into_iter().collect(), Some(r.id.clone()));
        match r.act {
            Act::Show(v) if crate::run::PAGES.contains(&v) => self.page = v,
            Act::Run(id, params) => self.start(id, params),
            _ => {}
        }
        None
    }

    /// The one row selected: its item's edit dialog, else the dialog its command edits with.
    fn edit(&mut self) {
        let Some(r) =
            self.picked().first().map(|r| (*r).clone()).or_else(|| self.rows().find(|r| self.selection.row() == Some(r.id.as_str())).cloned())
        else {
            return;
        };
        let opened = match (&r.item, &r.act) {
            (Some(item), _) => dialogs::edit(item, &mut self.session, false).map_err(|e| e.to_string()),
            (None, Act::Edit(id)) => self.open(id, &serde_json::json!({})),
            (None, Act::EditWith(id, values)) => self.open(id, values),
            _ => return,
        };
        match opened {
            Ok(d) => self.dialog = Some(d),
            Err(e) => self.error = Some(e),
        }
    }

    /// Command `id` from the ribbon: its dialog on the model's values, or (without parameters) run.
    fn command(&mut self, id: &str) {
        match emcraft_engine::find_command(id) {
            Some(spec) if spec.params.is_empty() => self.start(id, serde_json::json!({})),
            Some(_) => match self.open(id, &serde_json::json!({})) {
                Ok(d) => self.dialog = Some(d),
                Err(e) => self.error = Some(e),
            },
            None => self.error = Some(format!("no command `{id}`")),
        }
    }

    /// Command `id`'s dialog on the model's values, `values` over them.
    fn open(&mut self, id: &str, values: &serde_json::Value) -> Result<Dialog, String> {
        let spec = emcraft_engine::find_command(id).ok_or_else(|| format!("no command `{id}`"))?;
        Ok(dialogs::open(spec, &mut self.session, values, false))
    }

    /// The items selected deleted (their rows' delete commands), the selection cleared.
    fn delete(&mut self) {
        let items: Vec<browser::Item> = self.picked().into_iter().filter_map(|r| r.item.clone()).filter(|i| !i.delete.is_empty()).collect();
        self.selection = Selection::default();
        for item in items {
            self.run_now(item.delete, &item.params());
        }
    }

    /// OK on the open dialog: its command run (a long one on the worker, the dialog coming back
    /// with the error if it fails); a refusal shown under the field it names, the dialog kept.
    fn dialog_ok(&mut self) {
        let Some(mut d) = self.dialog.take() else { return };
        let params = match d.ok() {
            Ok(dialogs::Outcome::Run(p)) => p,
            Ok(dialogs::Outcome::Unchanged) => return,
            Err(_) => return self.dialog = Some(d),
        };
        let id = d.command.id;
        if emcraft_ui_core::commands::is_long(id) {
            self.start(id, params);
            if let Some(job) = self.job.as_mut() {
                job.dialog = Some(d);
            }
            return;
        }
        match self.session.execute(id, &params) {
            Ok(_) => {
                self.error = None;
                self.after(id);
            }
            Err(e) => {
                d.refused(&e.to_string());
                self.dialog = Some(d);
            }
        }
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
            look: emcraft_gpu3d::Look { edges: g.edges, ao: g.ao, outline: g.outline, pick: g.outline },
            selected: if g.outline { self.selection.iter().map(|p| p.tag()).collect() } else { Vec::new() },
            outline: {
                let a = self.look.tokens.accent;
                [a.r, a.g, a.b, 1.0]
            },
            field: shown.field.as_ref().map(|(grid, scale)| emcraft_gpu3d::FieldFrame {
                grid: grid.clone(),
                phase: self.view3d.field.phase,
                scale: *scale,
                contours: self.view3d.field.contours,
            }),
            cut: self.view3d.section,
            moving: false,
        }
    }

    /// What the window listens to: each frame while a job runs, the field plays or its picture is
    /// waited for.
    pub fn subscription(&self) -> Subscription<Message> {
        if self.job.is_some() || self.shoot.is_some() || self.playing.is_some() {
            iced::window::frames().map(Message::Tick)
        } else {
            Subscription::none()
        }
    }

    /// The window after `message`, and what iced does next: the picture of the window taken
    /// and written, then the window closed.
    pub fn react(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick(now) if self.shoot.is_some() => {
                self.tick(now);
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
            Message::File(a) => {
                let task = self.file(a);
                self.settle();
                task
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
        // The project's name, marked while it has unsaved changes.
        let name = format!("{}{}", self.session.name(), if self.session.is_dirty() { " •" } else { "" });
        let head = [text("EMCraft").size(t.font_body).boxed(), text(name).size(t.font_body).color(t.app_bar_text).boxed()];
        let bar = row(head
            .into_iter()
            .chain(crate::files::buttons(self))
            .chain(tabs)
            .chain(std::iter::once(Space::new().width(Fill).boxed()))
            .chain(status))
        .spacing(12)
        .align_y(Alignment::Center);
        let bar = container(bar).padding([6, 14]).width(Fill).style(look::app_bar(t));
        let page = match self.page {
            View::Model => crate::model::view(self),
            View::SParameters => crate::sparameters::view(self),
            _ => crate::system::view(self),
        };
        let window = container(column![bar, crate::ribbon::view(self), page]).width(Fill).height(Fill).style(look::window(t));
        let over = self.dialog.as_ref().map(|d| crate::dialog::view(self, d)).or_else(|| crate::files::question(self));
        match over {
            Some(o) => iced::widget::stack![window, o].boxed(),
            None => window.boxed(),
        }
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
            Message::Row(id) => self.row(&id),
            Message::Edit => {
                self.edit();
                None
            }
            Message::Tab(tab) => {
                self.tab = tab;
                None
            }
            Message::Command(id) => {
                self.command(&id);
                None
            }
            Message::Delete => {
                self.delete();
                None
            }
            Message::Dialog(crate::dialog::Edit::Ok) => {
                self.dialog_ok();
                None
            }
            Message::Dialog(crate::dialog::Edit::Cancel) => {
                self.dialog = None;
                None
            }
            Message::Dialog(edit) => {
                self.dialog = self.dialog.take().map(|d| crate::dialog::edited(d, &edit));
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
            Message::Tick(now) => {
                self.tick(now);
                None
            }
            Message::Play(on) => {
                self.playing = on.then_some(None);
                None
            }
            Message::Shot(_) => None,
            Message::File(a) => {
                // Without iced's runtime (the tests): a dialog's answer is not awaited.
                let _ = self.file(a);
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

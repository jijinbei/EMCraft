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
use iced::widget::{Space, button, container, row, text};
use iced::{Alignment, Element, Fill, Subscription, Task, Widget};

use crate::look::{self, Look};
use crate::run::{PAGES, cancel_label, page_label};
use crate::view3d::shader::Shot;
use crate::view3d::{Click, Nav, Shown, View3d};
use serde_json::json;

/// The page a project opens on: a board's diagnosis, a system's chain, else the model.
pub fn first_page(s: &Session) -> View {
    match s.model().analysis() {
        emcraft_engine::workflow::Analysis::Pcb => View::Pcb,
        _ if !s.model().system.stages.is_empty() => View::System,
        _ => View::Model,
    }
}

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
    /// The selection box let go: what it took selected (added to the selection with Shift).
    Boxed(bool),
    /// The 3D view's tool turned on, or off.
    Tool(Option<emcraft_ui_core::tools::Tool>),
    /// Escape: what the tool picked dropped; with nothing picked, the tool off.
    Escape,
    /// The selected port's reference plane's grip dragged.
    Grip(crate::view3d::GripMove),
    /// A key of the 3D view's (an arrow, Ctrl+1…7), taken while the view is shown and nothing
    /// else has the keys.
    Key3d(Nav),
    /// Eigenmode `i`'s field shown in the 3D view.
    Mode(usize),
    /// Where the pointer is over the 3D view (points from its corner), or that it left.
    Hover(Option<emcraft_ui_core::geom::Point>),
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
    /// An autosaved copy found on start let go (deleted).
    Dismiss(usize),
    /// The settings page shown, or left.
    Settings(bool),
    /// The start page shown, or left.
    Home(bool),
    /// A template's picture drawn (its pixels), or why not.
    Thumb(&'static str, Result<(u32, u32, Vec<u8>), String>),
    /// A preference changed.
    Pref(emcraft_ui_core::action::Pref),
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
    /// A control channel's request ([`crate::control`]), and its reply once it is made.
    Control(std::sync::Arc<emcraft_control::ControlRequest>),
    Replied(std::sync::Arc<emcraft_control::ControlRequest>, serde_json::Value),
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
    /// Where the pointer is over the 3D view, while the field's values are read there or a
    /// tool is on.
    pub(crate) hover: Option<Point>,
    /// The 3D view's tool on, and the tools' state (what the pointer is over, the points
    /// picked, the measurement, the reference plane's drag).
    pub(crate) tool: Option<emcraft_ui_core::tools::Tool>,
    pub(crate) tools: emcraft_ui_core::tools::Tools,
    /// What is selected (in the 3D view or the browser), and the browser's tree for the
    /// generation it was read for.
    pub(crate) selection: Selection,
    pub(crate) tree: Option<(u64, Tree)>,
    /// The replies of commands the views show (port modes, a comparison, …), kept and let go
    /// by the shared table.
    pub(crate) replies: Replies,
    /// The command's dialog open over the window.
    pub(crate) dialog: Option<Dialog>,
    /// The ribbon's tab shown.
    pub(crate) tab: emcraft_ui_core::ribbon::RibbonTab,
    /// The host's file dialog, when it has one.
    pub(crate) picker: Option<std::sync::Arc<dyn emcraft_ui_core::files::Picker>>,
    /// What "save the changes?" is asked before, and what waits for a save.
    pub(crate) asking: Option<emcraft_ui_core::files::Then>,
    pub(crate) after_save: Option<emcraft_ui_core::files::Then>,
    /// The control channel's queue of requests, when the window serves one.
    inbox: Option<Inbox>,
    /// The preferences (the recent projects among them) and where they are kept.
    pub(crate) prefs: Prefs,
    pub(crate) prefs_path: Option<std::path::PathBuf>,
    /// Copies of unsaved work, and the clock they are due by.
    pub(crate) autosave: emcraft_ui_core::autosave::Autosave,
    /// The themes there are (the user's too), and whether the settings page is shown.
    pub(crate) themes: Themes,
    pub(crate) settings: bool,
    /// The start page is shown, and the templates' pictures.
    pub(crate) home: bool,
    pub(crate) thumbs: crate::start::Thumbs,
    began: std::time::Instant,
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
        let page = first_page(&session);
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
            hover: None,
            tool: None,
            tools: Default::default(),
            selection: Selection::default(),
            tree: None,
            replies: Replies::default(),
            dialog: None,
            tab: Default::default(),
            picker: None,
            asking: None,
            after_save: None,
            inbox: None,
            prefs: Prefs::default(),
            prefs_path: None,
            autosave: Default::default(),
            themes: Themes::load(None),
            settings: false,
            home: sample.is_none(),
            thumbs: Default::default(),
            began: std::time::Instant::now(),
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

    /// The window in the preferences' mouse mapping and 3D drawing, keeping them (the recent
    /// projects) at `path`.
    pub fn with_prefs(self, prefs: &Prefs, path: Option<std::path::PathBuf>) -> App {
        App { nav: prefs.nav(), gpu_look: prefs.gpu_look, prefs: prefs.clone(), prefs_path: path, ..self }
    }

    /// The window copying unsaved work into `dir` (none: no copies), with the copies found
    /// there newer than their projects offered for recovery.
    pub fn with_autosave(self, dir: Option<std::path::PathBuf>) -> App {
        App { autosave: emcraft_ui_core::autosave::Autosave::at(dir), ..self }
    }

    /// On the PCB page, a net picked in 3D: the command that makes it the route.
    /// Whether the 3D view's keys are its: the view shown, no dialog, question, start or
    /// settings page over it.
    fn keys_free(&self) -> bool {
        matches!(self.page, View::Model | View::Pcb) && self.dialog.is_none() && self.asking.is_none() && !self.home && !self.settings
    }

    fn picked_route(&mut self) -> Option<Command> {
        let Some(emcraft_ui_core::scene3d::Pick::Net(i)) = self.selection.single().filter(|_| self.page == View::Pcb) else { return None };
        let board = self.session.model().pcb()?;
        let net = emcraft_ui_core::scene3d::board::nets(board).get(i).filter(|n| board.segments.iter().any(|s| &s.net == *n))?.clone();
        self.selection = Selection::default();
        Some(Command { id: "view.pcb", params: json!({"net": net, "stage": emcraft_engine::pcb::Stage::Net.id()}) })
    }

    /// The field's values under the pointer, on the field overlay.
    pub fn probe(&self) -> Option<emcraft_engine::view::Probe> {
        let at = self.hover.filter(|_| self.view3d.overlay == emcraft_ui_core::scene3d::Overlay::Field)?;
        let samples = self.view3d.sampled.as_ref()?.samples().clone();
        let (w, h) = self.view3d.size;
        let pixel = emcraft_render::view3d::Pixel::new(f64::from(at.x), f64::from(at.y), f64::from(w), f64::from(h));
        let lo = self.view3d.bounds()?.0;
        emcraft_ui_core::scene3d::plane_probe(&samples, &self.view3d.camera, pixel, lo, self.session.display().length)
    }

    /// The window choosing among `themes` (the user's too).
    pub fn with_themes(self, themes: Themes) -> App {
        App { themes, ..self }
    }

    /// The window after preference `p`: kept (and written where the preferences live), the
    /// theme, language, mouse mapping and drawing it changes in effect at once.
    fn prefer(&mut self, p: emcraft_ui_core::action::Pref) {
        let reload = p == emcraft_ui_core::action::Pref::ReloadThemes;
        match self.prefs.clone().changed(p, &self.themes) {
            Ok(prefs) => {
                if reload {
                    self.themes = Themes::load(self.themes.dir.as_deref());
                }
                emcraft_ui_core::lang::set(prefs.language());
                (self.nav, self.gpu_look) = (prefs.nav(), prefs.gpu_look);
                self.look = Look::of(self.themes.get(&prefs.theme));
                self.prefs = prefs;
                if let Some(path) = &self.prefs_path {
                    let _ = self.prefs.save(path);
                }
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// The window serving the control channel's requests from `rx`.
    pub fn serving(self, rx: Option<std::sync::mpsc::Receiver<emcraft_control::ControlRequest>>) -> App {
        App { inbox: rx.map(|rx| Inbox(std::sync::Arc::new(std::sync::Mutex::new(Some(rx))))), ..self }
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
        // The PCB page shows the board; the model page leaves it.
        use emcraft_ui_core::scene3d::Overlay;
        let board = self.page == View::Pcb;
        if board != (self.view3d.overlay == Overlay::Board) {
            let overlay = if board { Overlay::Board } else { Overlay::Plain };
            self.view3d = std::mem::take(&mut self.view3d).navigated(Nav::Overlay(overlay));
        }
        let (dirty, busy) = (self.session.is_dirty(), self.job.is_some());
        if self.autosave.due(self.began.elapsed().as_secs_f64(), dirty, busy) {
            let (r, new_failure) = self.autosave.write(&mut self.session);
            if let (Err(e), true) = (r, new_failure) {
                self.error = Some(emcraft_ui_core::lang::trf(t!("Autosave failed: {error}"), emcraft_ui_core::args!(error = e)));
            }
        }
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
            self.tree = Some((self.generation, browser::tree(&self.session, &self.replies)));
        }
        let charts = crate::charts::Source {
            session: &self.session,
            replies: &self.replies,
            generation: self.generation,
            look: &self.look,
            which: self.chart,
            page: self.page,
        };
        self.charts = std::mem::take(&mut self.charts).refreshed(&charts);
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
    pub(crate) fn open(&mut self, id: &str, values: &serde_json::Value) -> Result<Dialog, String> {
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
            Ok(reply) => {
                self.error = None;
                self.after(id, &params, &reply);
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
    /// waited for; and the window's close, asked first about unsaved changes.
    pub fn subscription(&self) -> Subscription<Message> {
        let closing =
            iced::window::close_requests().map(|_| Message::File(emcraft_ui_core::files::FileAction::Request(emcraft_ui_core::files::Then::Quit)));
        let frames = if self.job.is_some() || self.shoot.is_some() || self.playing.is_some() || self.view3d.animating() {
            iced::window::frames().map(Message::Tick)
        } else {
            Subscription::none()
        };
        let control = self.inbox.clone().map_or_else(Subscription::none, |inbox| Subscription::run_with(inbox, requests));
        Subscription::batch([
            frames,
            closing,
            iced::event::listen_with(crate::files::shortcut),
            iced::event::listen_with(crate::view3d::shader::keys),
            control,
        ])
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
                if let Some((path, _)) = self.shoot.take()
                    && let Some(e) = save_screenshot(&shot, path.to_str()).get("error")
                {
                    eprintln!("emcraft-iced: {e}");
                }
                iced::exit()
            }
            Message::Control(req) => {
                let task = self.control(req);
                self.settle();
                task
            }
            Message::Replied(req, v) => {
                let _ = req.reply.send(v);
                Task::none()
            }
            Message::File(a) => {
                let task = self.file(a);
                self.settle();
                task
            }
            // The start page (again in a new theme) draws the templates' pictures.
            m @ (Message::Home(true) | Message::Pref(_)) => {
                self.update(m);
                if self.home { self.draw_thumbs() } else { Task::none() }
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
        let tabs = PAGES.into_iter().filter(|v| *v == self.page || v.available(&self.session, &self.replies)).map(|v| {
            button(text(page_label(v)).size(t.font_button))
                .style(look::button(t, v == self.page && !self.settings && !self.home))
                .on_press(Message::Page(v))
                .boxed()
        });
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
            .chain(crate::files::recent(self))
            .chain(crate::files::edits(self))
            .chain(std::iter::once(
                button(text(emcraft_ui_core::lang::tr(t!("Start"))).size(t.font_button))
                    .style(look::button(t, self.home))
                    .on_press(Message::Home(!self.home))
                    .boxed(),
            ))
            .chain(std::iter::once(
                button(text(emcraft_ui_core::lang::tr(t!("Settings"))).size(t.font_button))
                    .style(look::button(t, self.settings))
                    .on_press(Message::Settings(!self.settings))
                    .boxed(),
            ))
            .chain(tabs)
            .chain(std::iter::once(Space::new().width(Fill).boxed()))
            .chain(status))
        .spacing(12)
        .align_y(Alignment::Center);
        let bar = container(bar).padding([6, 14]).width(Fill).style(look::app_bar(t));
        let page = match self.page {
            _ if self.settings => crate::settings::view(self),
            _ if self.home => crate::start::view(self),
            View::Model => crate::model::view(self),
            View::Eigenmodes => crate::eigenmodes::view(self),
            View::PortModes => crate::port_modes::view(self),
            View::Pcb => crate::pcb::view(self),
            v if v.chart().is_some() => crate::results::view(self, v),
            _ => crate::system::view(self),
        };
        // The ribbon's commands are the project's: not over the settings or the start page.
        let ribbon = (!self.settings && !self.home).then(|| crate::ribbon::view(self));
        let parts = std::iter::once(bar.boxed()).chain(ribbon).chain(crate::files::recoveries(self)).chain(std::iter::once(page));
        let window = container(iced::widget::Column::with_children(parts)).width(Fill).height(Fill).style(look::window(t));
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
                self.settings = false;
                self.home = false;
                None
            }
            Message::View3d(nav) => {
                self.view3d = std::mem::take(&mut self.view3d).navigated(nav);
                None
            }
            Message::Row(id) => self.row(&id),
            Message::Mode(i) => {
                self.page = View::Model;
                let view = std::mem::take(&mut self.view3d).navigated(Nav::Overlay(emcraft_ui_core::scene3d::Overlay::Field));
                self.view3d = view.navigated(Nav::Mode(Some(i)));
                None
            }
            Message::Hover(at) => {
                self.hover = at;
                self.tool_hover();
                None
            }
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
            Message::Click(_) if self.tool.is_some() => {
                self.tool_click();
                None
            }
            Message::Tool(tool) => {
                self.tool = tool;
                self.tools.reset(tool);
                None
            }
            Message::Escape => {
                if self.tools.escape() {
                    self.tool = None;
                    self.tools.reset(None);
                }
                None
            }
            Message::Grip(m) => {
                self.grip(m);
                None
            }
            Message::Click(c) => {
                let pick = self.view3d.pick(c.at);
                let click = if c.double { selection::Click::Double { pick } } else { selection::Click::Single { pick, add: c.add } };
                if selection::step(&mut self.selection, click) == selection::Then::Fit {
                    self.view3d = std::mem::take(&mut self.view3d).navigated(Nav::Goal(emcraft_ui_core::navigation::Goal::Fit));
                }
                self.picked_route()
            }
            Message::Boxed(add) => {
                let view = std::mem::take(&mut self.view3d);
                self.selection.boxed(&view.boxsel.map(|b| view.boxed(b)).unwrap_or_default(), add);
                self.view3d = view.navigated(Nav::Box(None));
                None
            }
            Message::Key3d(nav) if self.keys_free() || nav == Nav::Released => {
                self.view3d = std::mem::take(&mut self.view3d).navigated(nav);
                None
            }
            Message::Key3d(_) => None,
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
            Message::Shot(_) | Message::Replied(..) => None,
            Message::Control(req) => {
                // Without iced's runtime (the scenarios): what a request leaves to do later is not awaited.
                let _ = self.control(req);
                None
            }
            Message::Settings(on) => {
                self.settings = on;
                self.home &= !on;
                None
            }
            Message::Home(on) => {
                self.home = on;
                self.settings &= !on;
                None
            }
            Message::Thumb(name, drawn) => {
                let picture = drawn.map(|(w, h, rgba)| iced::widget::image::Handle::from_rgba(w, h, rgba));
                self.thumbs.1.insert(name, picture);
                None
            }
            Message::Pref(p) => {
                self.prefer(p);
                None
            }
            Message::Dismiss(i) => {
                self.autosave.dismiss(i);
                None
            }
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
            Ok(reply) => {
                self.error = None;
                self.after(command.id, &command.params, &reply);
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

/// The control channel's queue, taken once by the subscription that reads it.
#[derive(Clone)]
pub struct Inbox(std::sync::Arc<std::sync::Mutex<Option<std::sync::mpsc::Receiver<emcraft_control::ControlRequest>>>>);

impl std::hash::Hash for Inbox {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        "emcraft.control".hash(h);
    }
}

/// The requests of `inbox` as messages: a thread waits on the queue and passes each one on.
fn requests(inbox: &Inbox) -> iced::futures::channel::mpsc::UnboundedReceiver<Message> {
    let (tx, out) = iced::futures::channel::mpsc::unbounded();
    if let Some(rx) = inbox.0.lock().ok().and_then(|mut q| q.take()) {
        std::thread::spawn(move || {
            for req in rx {
                if tx.unbounded_send(Message::Control(std::sync::Arc::new(req))).is_err() {
                    break;
                }
            }
        });
    }
    out
}

/// A picture of the window written as a PNG at `path` (none: nowhere): what was written, or why
/// not.
pub(crate) fn save_screenshot(shot: &iced::window::Screenshot, path: Option<&str>) -> serde_json::Value {
    let canvas = emcraft_render::Canvas { w: shot.size.width as usize, h: shot.size.height as usize, rgba: shot.rgba.to_vec() };
    let Some(png) = canvas.png() else { return serde_json::json!({"ok": false, "error": "the picture could not be encoded"}) };
    match path {
        Some(p) => match std::fs::write(p, png) {
            Ok(()) => serde_json::json!({"ok": true, "result": {"path": p, "width": canvas.w, "height": canvas.h}}),
            Err(e) => serde_json::json!({"ok": false, "error": format!("{p}: {e}")}),
        },
        None => serde_json::json!({"ok": true, "result": {"width": canvas.w, "height": canvas.h}}),
    }
}

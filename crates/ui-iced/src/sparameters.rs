//! The S-parameters page: how S is shown (in dB, a magnitude, a phase, a
//! group delay, a Smith chart; mixed mode where the ports pair), the chart drawn by the GPU — a
//! click picks the nearest solved frequency for the field view and opens it, a drag picks a band
//! to solve more finely (`solve.refine`), the field view's frequency marked — the ports'
//! reference planes (`model.port.deembed`), the kept solves (each drawn over the chart on a
//! click, restored, pinned or let go), the table of S and the last coupling matrix. What it
//! shows is [`SView`], the shared rules' ([`emcraft_ui_core::results::sparameters`]).

use emcraft_engine::view::SDisplay;
use emcraft_model::units::Frequency;
use emcraft_render::Rgb;
use emcraft_render::chart::Chart;
use emcraft_render::palette::Palette;
use emcraft_ui_core::lang::{shown, tr};
use emcraft_ui_core::results::sparameters::{self as rules, SView};
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Frame, Geometry, Path, Stroke};
use iced::widget::{Column, Space, button, canvas as canvas_widget, column, container, pick_list, row, text, text_input};
use iced::{Alignment, Color, Element, Fill, Point, Rectangle, Renderer, Size, Theme, Widget};
use serde_json::{Value, json};

use crate::app::{App, Message};
use crate::look::{self, Tokens};
use crate::parts::{action, note as small};

/// The points a band may add, to choose from.
const POINTS: [usize; 6] = [5, 11, 21, 51, 101, 201];

/// The chart's height (points), and how far the pointer may move for a click.
const CHART_H: f32 = 460.0;
const CLICK_SLOP: f32 = 4.0;

/// What the page's controls ask for.
#[derive(Clone, Debug)]
pub enum SAct {
    Display(SDisplay),
    Mixed(bool),
    /// A kept solve drawn over the chart, or taken off it.
    Compare(u32),
    Table(bool),
    /// The band picked on the chart (Hz), or none; the points to add in it, typed.
    Band(Option<(f64, f64)>),
    Points(usize),
    Refine,
    /// The frequency clicked on the chart (Hz): the nearest solved one's field.
    Pick(f64),
    /// A port's reference plane typed, and set.
    Plane(String, String),
    SetPlane(String),
    /// A kept solve's command (restore, pin, delete) with its parameters.
    History(&'static str, Value),
}

fn send(a: SAct) -> Message {
    Message::S(a)
}

impl App {
    /// What a control of the S-parameters page asks for.
    pub(crate) fn s_act(&mut self, a: SAct) {
        let v = std::mem::take(&mut self.sview);
        self.sview = match a {
            SAct::Display(display) => SView { display, ..v },
            SAct::Mixed(mixed) => SView { mixed, ..v },
            SAct::Compare(id) => v.toggled(id),
            SAct::Table(table) => SView { table, ..v },
            SAct::Band(band) => SView { band, ..v },
            SAct::Points(n) => SView { points: n.clamp(2, rules::MAX_REFINE), ..v },
            SAct::Refine => {
                if let Some(params) = v.refine(&self.session) {
                    self.start("solve.refine", params);
                }
                SView { band: None, ..v }
            }
            SAct::Pick(f) => {
                self.show_field(f);
                v
            }
            SAct::Plane(name, s) => {
                let seen = self.planes.get(&name).map_or(f64::NAN, |(_, seen)| *seen);
                self.planes.insert(name, (emcraft_ui_core::dialogs::bounded(&s), seen));
                v
            }
            SAct::SetPlane(name) => {
                let typed = self.planes.get(&name).and_then(|(s, _)| s.trim().parse::<f64>().ok()).filter(|x| x.is_finite());
                if let Some(x) = typed {
                    self.run_now("model.port.deembed", &json!({"name": name, "deembed": x}));
                }
                v
            }
            SAct::History(command, params) => {
                self.run_now(command, &params);
                v
            }
        };
    }

    /// The field view at the solved frequency nearest `f` (Hz), on the model page.
    fn show_field(&mut self, f: f64) {
        use crate::view3d::Nav;
        let freqs: Vec<f64> = self.session.results().sweep.iter().flat_map(|s| s.results.iter().map(|r| r.frequency)).collect();
        let Some(f) = rules::nearest(&freqs, f) else { return };
        let view = std::mem::take(&mut self.view3d).navigated(Nav::Overlay(emcraft_ui_core::scene3d::Overlay::Field));
        self.view3d = view.navigated(Nav::Mode(None)).navigated(Nav::Frequency(f));
        self.page = emcraft_ui_core::view::View::Model;
    }

    /// The ports' reference planes' texts following the model's (in display units) until typed.
    pub(crate) fn sync_planes(&mut self) {
        let l = self.session.display().length;
        let now: Vec<(String, f64)> = self.session.model().ports.iter().map(|p| (p.name.clone(), l.from_si(p.deembed))).collect();
        self.planes = now
            .into_iter()
            .map(|(name, x)| {
                let kept = self.planes.remove(&name).filter(|(_, seen)| *seen == x);
                let entry = kept.unwrap_or_else(|| (emcraft_ui_core::tools::trimmed(x, 4), x));
                (name, entry)
            })
            .collect();
    }
}

/// The chart, the field view's frequency marked and the band picked shaded; a click or a drag
/// on it (not on a Smith chart, whose axes are Γ's).
struct SChart<'a> {
    chart: &'a Chart,
    palette: &'a Palette,
    unit: Frequency,
    marked: Option<f64>,
    band: Option<(f64, f64)>,
    pickable: bool,
    tokens: Tokens,
}

/// A press on the chart: where (points), and whether it became a drag.
#[derive(Default)]
struct Press(Option<(f32, bool)>);

impl SChart<'_> {
    fn layout(&self, bounds: Rectangle) -> emcraft_render::axes::Layout {
        emcraft_render::chart::layout_of(self.chart, bounds.width.max(1.0) as usize, bounds.height.max(1.0) as usize)
    }

    /// The frequency (Hz) under screen x, on the plot area.
    fn frequency(&self, bounds: Rectangle, x: f32) -> Option<f64> {
        self.layout(bounds).x_at(f64::from(x)).map(|f| self.unit.to_si(f))
    }

    /// Frequency `f` (Hz)'s x on screen.
    fn x(&self, layout: &emcraft_render::axes::Layout, f: f64) -> f32 {
        layout.to_pixels([self.unit.from_si(f), layout.y.0])[0] as f32
    }
}

impl canvas::Program<Message> for SChart<'_> {
    type State = Press;

    fn update(&self, press: &mut Press, event: &iced::Event, bounds: Rectangle, cursor: Cursor) -> Option<Action<Message>> {
        if !self.pickable {
            return None;
        }
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = cursor.position_in(bounds)?;
                self.frequency(bounds, at.x)?;
                press.0 = Some((at.x, false));
                Some(Action::capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let (x0, dragging) = press.0?;
                let x = position.x - bounds.x;
                if !dragging && (x - x0).abs() <= CLICK_SLOP {
                    return None;
                }
                press.0 = Some((x0, true));
                let layout = self.layout(bounds);
                let [left, _, width, _] = layout.plane;
                let clamp = |x: f32| f64::from(x).clamp(left, left + width);
                let (a, b) = (layout.x_at(clamp(x0))?, layout.x_at(clamp(x))?);
                let (a, b) = (self.unit.to_si(a.min(b)), self.unit.to_si(a.max(b)));
                Some(Action::publish(send(SAct::Band(Some((a, b))))).and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let (x0, dragging) = press.0.take()?;
                if dragging {
                    return Some(Action::capture());
                }
                let f = self.frequency(bounds, x0)?;
                Some(Action::publish(send(SAct::Pick(f))).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, _: &Press, renderer: &Renderer, _: &Theme, bounds: Rectangle, _: Cursor) -> Vec<Geometry> {
        let t = self.tokens;
        let mut frame = Frame::new(renderer, bounds.size());
        crate::chart::paint(&mut frame, self.chart, bounds.size(), self.palette);
        if self.pickable {
            let layout = self.layout(bounds);
            let [_, top, _, height] = layout.plane.map(|v| v as f32);
            if let Some((a, b)) = self.band {
                let (xa, xb) = (self.x(&layout, a), self.x(&layout, b));
                frame.fill_rectangle(Point::new(xa.min(xb), top), Size::new((xb - xa).abs(), height), Color { a: 0.18, ..t.accent });
            }
            if let Some(f) = self.marked.filter(|f| layout.x_at(f64::from(self.x(&layout, *f))).is_some()) {
                let x = self.x(&layout, f);
                frame.stroke(&Path::line(Point::new(x, top), Point::new(x, top + height)), Stroke::default().with_width(1.5).with_color(t.accent));
                frame.fill_text(canvas::Text {
                    content: tr(t!("fields shown")).to_owned(),
                    position: Point::new(x + 6.0, top + 6.0),
                    color: t.accent,
                    size: iced::Pixels(t.font_small),
                    ..canvas::Text::default()
                });
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, press: &Press, bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        let on_plot = cursor.position_in(bounds).and_then(|p| self.frequency(bounds, p.x)).is_some();
        match (self.pickable && on_plot, press.0) {
            (true, Some((_, true))) => mouse::Interaction::ResizingHorizontally,
            (true, _) => mouse::Interaction::Crosshair,
            (false, _) => mouse::Interaction::default(),
        }
    }
}

/// The page.
pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let s = &app.session;
    let v = &app.sview;
    let display = pick_list(Some(v.display), SDisplay::ALL, |d: &SDisplay| shown(*d).to_owned())
        .on_select(|d| send(SAct::Display(d)))
        .text_size(t.font_button)
        .style(look::choice(t));
    let mixed = rules::pairs(s).then(|| action(t, tr(t!("Mixed mode")), v.mixed, Some(send(SAct::Mixed(!v.mixed)))));
    let controls =
        row(std::iter::once(display.boxed()).chain(mixed).chain(crate::style_card::gear(app))).spacing(t.space_8).align_y(Alignment::Center);
    let style = crate::style_card::card(app);
    let chart: Element<Message> = match app.charts.page.as_ref().map(|p| &p.shown) {
        Some(Ok(c)) => {
            let marked = (app.view3d.field.mode.is_none()).then_some(app.view3d.field.frequency);
            let program = SChart {
                chart: c,
                palette: &app.look.theme.palette,
                unit: s.display().frequency,
                marked,
                band: v.band,
                pickable: v.display != SDisplay::Smith && s.results().sweep.is_some(),
                tokens: t,
            };
            canvas_widget(program).width(Fill).height(CHART_H).boxed()
        }
        Some(Err(e)) => {
            let missing = emcraft_ui_core::view::View::SParameters.missing(s, &app.replies);
            crate::parts::empty(t, missing.map_or_else(|| e.clone(), |m| tr(m).to_owned()), None)
        }
        None => Space::new().boxed(),
    };
    let band = refine_bar(app);
    let planes = planes(app);
    let caption = s.results().sweep.is_some().then(|| small(t, tr(t!("Click: the fields there · drag: a band to solve more finely")), t.text_dim));
    let history = rules::lists_history(s).then(|| history(app));
    let toggle = s
        .results()
        .sweep
        .is_some()
        .then(|| action(t, tr(if v.table { t!("Hide the table") } else { t!("Show the table") }), false, Some(send(SAct::Table(!v.table)))));
    let table = v.table.then(|| rules::table(s)).flatten().map(|tb| crate::table::view(t, &tb));
    let coupling =
        app.replies.coupling.as_ref().map(|r| {
            column![text(tr(t!("Coupling matrix"))).size(t.font_body), crate::table::view(t, &rules::coupling(r))].spacing(t.space_4).boxed()
        });
    let parts = std::iter::once(controls.boxed())
        .chain(style)
        .chain(std::iter::once(chart))
        .chain(band)
        .chain(planes)
        .chain(caption)
        .chain(history)
        .chain(coupling)
        .chain(toggle)
        .chain(table);
    iced::widget::scrollable(Column::with_children(parts).spacing(t.space_8).padding(t.space_16)).boxed()
}

/// The band picked: its ends, the points to add, Refine and ✕.
fn refine_bar(app: &App) -> Option<Element<'_, Message>> {
    let t = app.look.tokens;
    let (a, b) = app.sview.band?;
    let fu = app.session.display().frequency;
    let ends = format!("{} – {} {}", emcraft_render::axes::format(fu.from_si(a)), emcraft_render::axes::format(fu.from_si(b)), fu.symbol());
    let points = pick_list(Some(app.sview.points), POINTS, |n: &usize| n.to_string())
        .on_select(|n| send(SAct::Points(n)))
        .text_size(t.font_button)
        .style(look::choice(t));
    let idle = app.job.is_none();
    Some(
        row![
            small(t, ends, t.text),
            points,
            small(t, "pts", t.text_dim),
            action(t, tr(t!("Refine")), true, idle.then_some(send(SAct::Refine))),
            crate::parts::close(t, t!("Remove"), send(SAct::Band(None)))
        ]
        .spacing(t.space_8)
        .align_y(Alignment::Center)
        .boxed(),
    )
}

/// Each port's reference plane (display units): Enter sets it (`model.port.deembed`); S
/// follows at once, or with terminal ports after a new solve.
fn planes(app: &App) -> Option<Element<'_, Message>> {
    let t = app.look.tokens;
    let s = &app.session;
    s.results().sweep.as_ref()?;
    let ports = &s.model().ports;
    if ports.is_empty() {
        return None;
    }
    let unit = s.display().length.symbol();
    let idle = app.job.is_none();
    let fields = ports.iter().filter_map(|p| {
        let (typed, _) = app.planes.get(&p.name)?;
        let name = p.name.clone();
        let input = text_input("", typed).size(t.font_body).width(80).padding([t.space_4, t.space_6]).style(look::field(t));
        let input =
            if idle { input.on_input(move |s| send(SAct::Plane(name.clone(), s))).on_submit(send(SAct::SetPlane(p.name.clone()))) } else { input };
        Some(row![small(t, p.name.clone(), t.text), input, small(t, unit, t.text_dim)].spacing(t.space_4).align_y(Alignment::Center).boxed())
    });
    let how = small(
        t,
        tr(if rules::planes_instant(s) { t!("S follows at once (a phase shift of the port's mode)") } else { t!("terminal ports: solved again") }),
        t.text_dim,
    );
    let head = small(t, tr(t!("Ref. plane")), t.text_dim);
    Some(row(std::iter::once(head).chain(fields).chain(std::iter::once(how))).spacing(t.space_8).align_y(Alignment::Center).wrap().boxed())
}

/// The kept solves, the newest first: a swatch (● the session's, two pens' colours when drawn
/// over the chart, else ○), `#id`, what changed and the deepest |S11|; a click draws it over the
/// chart or takes it off; Restore, Pin, Delete.
fn history(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let s = &app.session;
    let palette = &app.look.theme.palette;
    let rows = emcraft_engine::archive::rows(s).into_iter().rev().map(|r| {
        let drawn = app.sview.compare.iter().position(|c| *c == r.id);
        let (name, what, db) = rules::history_line(&r);
        let ink = if r.current { t.text } else { t.text_dim };
        let swatch: Element<Message> = match (r.current, drawn) {
            (true, _) => text("●").size(t.font_body).color(t.accent).boxed(),
            (false, Some(k)) => {
                let [a, b] = emcraft_engine::view::compare_pens(k).map(|pen| rgb(palette.pen(pen)));
                row![text("━").size(t.font_body).color(a), text("━").size(t.font_body).color(b)].boxed()
            }
            (false, None) => text("○").size(t.font_body).color(t.text_dim).boxed(),
        };
        let stored = s.archive().iter().any(|a| a.id == r.id && !a.in_memory());
        let line = column![
            row![text(name).size(t.font_body).color(ink), Space::new().width(Fill), text(db.unwrap_or_default()).size(t.font_small).color(ink)],
            small(t, what, t.text_dim),
            small(t, rules::history_tip(s, &r, stored), t.text_dim)
        ]
        .spacing(t.space_2);
        let pick = button(row![swatch, line].spacing(t.space_8).align_y(Alignment::Center))
            .style(look::row(t, drawn.is_some()))
            .width(Fill)
            .on_press_maybe((!r.current).then(|| send(SAct::Compare(r.id))));
        let acts = rules::history_actions(r.id, r.pinned)
            .into_iter()
            .map(|(label, command, params)| action(t, tr(label), false, app.job.is_none().then(|| send(SAct::History(command, params)))));
        column![pick, row(acts).spacing(t.space_6)].spacing(t.space_4).boxed()
    });
    let head = text(tr(t!("History"))).size(t.font_body).boxed();
    container(Column::with_children(std::iter::once(head).chain(rows)).spacing(t.space_8))
        .padding(t.space_8)
        .style(look::ground(t))
        .width(Fill)
        .boxed()
}

/// The page's controls by their handles, for scenarios and agents: each display
/// (`s:display:<id>`), mixed mode, the table, each kept solve (`history:<id>` draws it over the
/// chart; `history:<id>:restore`, `:pin`, `:delete`), Refine and ✕ for the band picked.
pub fn handles(app: &App) -> Vec<(String, Message)> {
    if app.page != emcraft_ui_core::view::View::SParameters || app.settings || app.home {
        return Vec::new();
    }
    let v = &app.sview;
    let displays = SDisplay::ALL.into_iter().map(|d| (format!("s:display:{}", d.id()), send(SAct::Display(d))));
    let fixed = [("s:table".to_owned(), send(SAct::Table(!v.table))), ("s:mixed".to_owned(), send(SAct::Mixed(!v.mixed)))];
    let rows = emcraft_engine::archive::rows(&app.session);
    let history = rows.iter().flat_map(|r| {
        let names = ["restore", "pin", "delete"];
        let acts = rules::history_actions(r.id, r.pinned)
            .into_iter()
            .zip(names)
            .map(move |((_, c, p), n)| (format!("history:{}:{n}", r.id), send(SAct::History(c, p))));
        std::iter::once((format!("history:{}", r.id), send(SAct::Compare(r.id)))).chain(acts).collect::<Vec<_>>()
    });
    let band = v.band.map(|_| [("refine:start".to_owned(), send(SAct::Refine)), ("refine:clear".to_owned(), send(SAct::Band(None)))]);
    displays.chain(fixed).chain(history).chain(band.into_iter().flatten()).collect()
}

/// The page's state for `ui.inspect`: the display, mixed mode, the solves drawn over the chart,
/// the table, the band picked (display units) and its points.
pub fn inspect(app: &App) -> Value {
    let v = &app.sview;
    let fu = app.session.display().frequency;
    json!({
        "display": v.display.id(),
        "mixed": v.mixed,
        "compare": v.compare,
        "table": v.table,
        "band": v.band.map(|(a, b)| [fu.from_si(a), fu.from_si(b)]),
        "points": v.points,
        "history": emcraft_engine::archive::rows(&app.session).iter().map(|r| r.id).collect::<Vec<_>>(),
    })
}

fn rgb(c: Rgb) -> Color {
    look::rgb(c)
}

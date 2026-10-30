//! The viewport card: the model's outline (H-plane and E-plane views of the guide or the
//! solid's bounding views, with materials, apertures, ports and sources), and the results — the
//! S-parameter chart against the exact curves, fields on a plane with a phase animation, the
//! port-mode and eigenmode tables, the far-field pattern and the comparison with theory. Charts
//! and fields are `emcraft-render`'s CPU pictures, shown as textures.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use emcraft_engine::view::{self, Plane};
use serde_json::{Value, json};

use crate::handle::{Fixed, Handle};
use crate::lang::{Raw, Shown, shown, tr, trf};
use crate::picture::{Cached, PictureKey};
use crate::state::{FieldState, SharedChart, ViewState};
use crate::theme::Tokens;
use crate::view3d::View3d;
use crate::{Action, Actions, EmcraftApp, Hint, Picture, Style, UiExt, View, scenario};

mod chart_style;
mod dispersion;
mod far_field;
pub mod field_params;
mod field_side;
mod fields;
mod model;
mod sparameters;
mod study;
mod tables;
mod tdr;

use dispersion::*;
use far_field::*;
use fields::*;
use model::*;
use sparameters::*;
use study::*;
use tables::*;
use tdr::*;

pub(crate) use fields::{field_plots, plane_span_of};
pub use study::summary as study_summary;

/// The viewport's own state, lent out of the app while the viewport is drawn: the field view's
/// settings and samples, the result views' charts and settings, the 3D view's camera and
/// pictures.
#[derive(Default)]
pub struct Parts {
    pub field: FieldState,
    pub views: ViewState,
    pub view3d: View3d,
}

impl Parts {
    /// The parts taken out of `app`.
    pub(crate) fn take(app: &mut EmcraftApp) -> Parts {
        Parts { field: std::mem::take(&mut app.field), views: std::mem::take(&mut app.views), view3d: std::mem::take(&mut app.view3d) }
    }

    /// The parts put back into `app`.
    pub(crate) fn put_back(self, app: &mut EmcraftApp) {
        (app.field, app.views, app.view3d) = (self.field, self.views, self.view3d);
    }
}

/// The eigenmode table (`results.eigenmodes`), read again when the session changed.
pub(crate) fn prepare_eigenmodes(app: &mut EmcraftApp) {
    if app.views.eigen_table.as_ref().is_some_and(|e| e.revision == app.revision) || app.session.results().eigen.is_none() {
        return;
    }
    let reply = app.session.execute("results.eigenmodes", &json!({})).map_err(|e| e.to_string());
    app.views.eigen_table = Some(crate::state::EigenTable { revision: app.revision, reply });
}

/// The central card; its rectangle.
pub fn show(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) -> Option<Rect> {
    let t = &Tokens::get();
    let mut shown = None;
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(egui::Margin { left: 6, right: 6, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            let r = ui.max_rect();
            // The viewport's gradient ground.
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(r.left_top(), t.viewport_top);
            mesh.colored_vertex(r.right_top(), t.viewport_top);
            mesh.colored_vertex(r.right_bottom(), t.viewport_bottom);
            mesh.colored_vertex(r.left_bottom(), t.viewport_bottom);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            ui.painter().add(egui::Shape::mesh(mesh));
            ui.painter().rect_stroke(r, t.radius_lg, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
            shown = Some(r);
            scenario::publish_rect(Fixed::Viewport, r);
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink(10.0)));
            view_tabs(app, &mut inner, out);
            inner.add_space(4.0);
            if let Some(text) = app.view.missing(app) {
                empty(&mut inner, text);
                return;
            }
            // Results of a model changed after their run say so over the view, which is dimmed.
            let stale = app.view.is_result() && crate::steps::results_stale(app);
            if stale {
                stale_band(app, &mut inner, out);
                inner.add_space(4.0);
            }
            inner.ctx().data_mut(|d| d.insert_temp(egui::Id::new(STALE), stale));
            // The drawings fill the card; the results scroll when the window is too small for them.
            let body = |ui: &mut egui::Ui, st: &mut Parts, out: &mut Actions| match app.view {
                View::Model => model3d(app, st, ui, out),
                View::Outline => model(app, ui),
                View::SParameters => show_chart::<SParameters>(app, st, ui, out),
                View::Tdr => show_chart::<Tdr>(app, st, ui, out),
                View::Field => field(app, st, ui, out),
                View::PortModes => port_modes(app, st, ui, out),
                View::FarField => show_chart::<FarField>(app, st, ui, out),
                View::Compare => compare(app, ui, out),
                View::Eigenmodes => eigenmodes(app, st, ui, out),
                View::Study => show_chart::<StudyView>(app, st, ui, out),
                View::Dispersion => show_chart::<Dispersion>(app, st, ui, out),
            };
            if matches!(app.view, View::Model | View::Outline) {
                body(&mut inner, st, out);
            } else {
                egui::ScrollArea::both().id_salt(("view", app.view.id())).auto_shrink(false).show(&mut inner, |ui| body(ui, st, out));
            }
        });
    shown
}

/// The views with something to show, as a row of pills.
pub(super) fn view_tabs(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    ui.horizontal_wrapped(|ui| {
        for v in View::ALL.into_iter().filter(|v| *v == app.view || v.available(app)) {
            ui.act(true, pill(shown(v), app.view == v, 12, PillSize::Body), Handle::View(v), "", || Action::ShowView(v), out);
        }
    });
}

/// The text size of a pill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PillSize {
    /// The body's text.
    Body,
    /// The small text.
    Small,
}

/// A pill of a row of options: filled when `on`, its text `size`.
pub(super) fn pill(label: &str, on: bool, radius: u8, size: PillSize) -> egui::Button<'static> {
    let t = &Tokens::get();
    let font = match size {
        PillSize::Body => t.font_body,
        PillSize::Small => t.font_small,
    };
    let text = egui::RichText::new(label).size(font).color(if on { t.selected_ink } else { t.text });
    egui::Button::new(text).fill(if on { t.accent_soft } else { Color32::TRANSPARENT }).corner_radius(radius)
}

/// A chart's height: a share of the room left in the card, within `min..=max` pixels.
#[derive(Clone, Copy)]
pub(super) struct Height {
    pub share: f32,
    pub min: f32,
    pub max: f32,
}

impl Height {
    pub const fn new(share: f32, min: f32, max: f32) -> Height {
        Height { share, min, max }
    }

    fn of(self, ui: &egui::Ui) -> usize {
        (ui.available_height() * self.share).clamp(self.min, self.max) as usize
    }
}

/// A result view drawn around one chart (the S-parameters, TDR, dispersion, far-field and
/// study views): its settings above the chart, which give what the chart shows (its key), the
/// chart drawn from the session and that key, and what goes under it. [`show_chart`] draws
/// any of them.
pub(super) trait ChartView {
    /// What the chart is drawn from besides the session: a change draws it again.
    type Key: ChartKey;
    /// The chart's name on screen.
    const HANDLE: Fixed;
    /// Whose style it is drawn in (`view.chart.style`).
    const CHART: emcraft_engine::views::ChartId;
    /// Its height in the card.
    const HEIGHT: Height;

    /// What goes over the chart (the view's settings, edited in `st`); what it shows, or `None`
    /// when there is nothing to chart.
    fn controls(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) -> Option<Self::Key>;

    /// The chart of `key`.
    fn chart(app: &EmcraftApp, key: &Self::Key) -> Result<emcraft_render::chart::Chart, String>;

    /// What goes under the chart: `chart` is its response, `size` its size in pixels.
    fn below(_app: &EmcraftApp, _st: &mut Parts, _ui: &mut egui::Ui, _chart: &egui::Response, _size: egui::Vec2, _out: &mut Actions) {}

    /// The blocks packed round the chart ([`crate::pack`]): each one's size as it wants it, in
    /// the order they go; none by default (the chart and what goes under it).
    fn blocks(_app: &EmcraftApp, _st: &Parts) -> Vec<egui::Vec2> {
        Vec::new()
    }

    /// Block `k` of [`ChartView::blocks`], in its rectangle.
    fn block(_app: &EmcraftApp, _st: &mut Parts, _ui: &mut egui::Ui, _k: usize, _out: &mut Actions) {}

    /// What the card shows when the chart cannot be drawn: why, by default.
    fn failed(_app: &EmcraftApp, _st: &mut Parts, ui: &mut egui::Ui, error: &str) {
        empty(ui, Raw(error));
    }
}

/// What a chart is drawn from, and where a chart drawn from it is kept between frames.
pub(super) trait ChartKey: Clone + PartialEq {
    fn slot(views: &mut ViewState) -> &mut Cached<PictureKey<Self>>;
}

/// The S-parameter, dispersion and far-field charts, one at a time.
impl ChartKey for SharedChart {
    fn slot(views: &mut ViewState) -> &mut Cached<PictureKey<SharedChart>> {
        &mut views.chart
    }
}

impl ChartKey for crate::state::TdrParams {
    fn slot(views: &mut ViewState) -> &mut Cached<PictureKey<Self>> {
        &mut views.tdr.chart
    }
}

impl ChartKey for (Option<String>, emcraft_engine::study::Slice) {
    fn slot(views: &mut ViewState) -> &mut Cached<PictureKey<Self>> {
        &mut views.study.chart
    }
}

/// The height of a chart with a table under it.
pub(super) const OVER_TABLE: Height = Height::new(0.62, 260.0, 700.0);

/// Draws a chart view: its controls, its chart in the project's style (`view.chart.style`) in
/// the rest of the card as wide as the card allows (the one kept, drawn again when the session,
/// the width, the theme or the view's key changed) with the gear that edits its style, then
/// what goes under it.
pub(super) fn show_chart<V: ChartView>(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) {
    let wants = V::blocks(app, st);
    if wants.is_empty() {
        return chart_column::<V>(app, st, ui, out);
    }
    let Some(key) = V::controls(app, st, ui, out) else { return };
    // The chart and its blocks packed into the rest of the card (the card's scroll bar takes
    // the last few points); what does not fit scrolls.
    let area = vec2(ui.available_width() - 16.0, ui.available_height());
    let a = crate::pack::arrange(area, LEAST_CHART, &wants);
    let (whole, _) = ui.allocate_exact_size(vec2(area.x, a.height), Sense::hover());
    let at = |r: Rect| r.translate(whole.min.to_vec2());
    let mut main = ui.new_child(egui::UiBuilder::new().max_rect(at(a.main)));
    // Under the chart, in its rectangle, the view's own overlays (they need the chart's response).
    let under = 40.0;
    let size = [a.main.width(), (a.main.height() - under).max(LEAST_CHART.y)];
    draw_chart::<V>(app, st, &mut main, key, Some(size), out);
    for (k, r) in a.blocks.iter().enumerate() {
        let mut b = ui.new_child(egui::UiBuilder::new().max_rect(at(*r)));
        V::block(app, st, &mut b, k, out);
    }
}

/// The least size a chart is drawn at among its blocks.
const LEAST_CHART: egui::Vec2 = egui::vec2(360.0, 260.0);

/// A chart view's controls, chart and what goes under it.
fn chart_column<V: ChartView>(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) {
    let Some(key) = V::controls(app, st, ui, out) else { return };
    draw_chart::<V>(app, st, ui, key, None, out);
}

/// The chart of `key`, `size` points (else as wide as `ui` and [`ChartView::HEIGHT`] high),
/// with its gear and what goes under it.
fn draw_chart<V: ChartView>(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, key: V::Key, size: Option<[f32; 2]>, out: &mut Actions) {
    let (width, height) = match size {
        Some([w, h]) => (((w / 10.0).floor() * 10.0).clamp(200.0, 2400.0) as usize, ((h / 10.0).floor() * 10.0).clamp(150.0, 1600.0) as usize),
        None => (picture_width(ui), V::HEIGHT.of(ui)),
    };
    let mut drawn_now = None;
    let mut pk = PictureKey::new(app.revision, width, key.clone());
    pk.height = height;
    let drawn = V::Key::slot(&mut st.views).get_or_draw(pk, || {
        V::chart(app, &key).map(|c| {
            let curves: Vec<(String, String)> = c.curves().into_iter().map(|(k, l)| (k.to_owned(), chart_style::plain(l))).collect();
            let c = emcraft_engine::views::style_chart(&app.session, V::CHART, c);
            drawn_now = Some((emcraft_render::chart::layout_of(&c, width, height), curves));
            emcraft_render::chart::draw(&c, width, height, &crate::theme::palette())
        })
    });
    match drawn {
        Ok(p) => {
            let size = p.size();
            let r = picture(ui, p, V::HANDLE);
            if let Some((layout, curves)) = drawn_now {
                (st.views.layout, st.views.curves) = (Some(layout), curves);
            }
            chart_style::gear(app, st, ui, &r, V::CHART, out);
            V::below(app, st, ui, &r, size, out);
        }
        Err(e) => V::failed(app, st, ui, &e),
    }
}

pub(super) fn empty(ui: &mut egui::Ui, text: impl Shown) {
    ui.centered_and_justified(|ui| ui.text(Style::HeadingDim, text));
}

/// The card's message while a view has nothing to show, with the button (`label`, published
/// as `handle`) that asks for `action`, the action that makes it.
pub(super) fn empty_with_action(ui: &mut egui::Ui, text: &str, label: &str, enabled: bool, handle: Fixed, action: Action, out: &mut Actions) {
    ui.vertical_centered(|ui| {
        ui.add_space(40.0);
        ui.text(Style::HeadingDim, text);
        ui.act(enabled, egui::Button::new(tr(label)), handle.into(), "", || action, out);
    });
}

/// Where the frame notes that the view's results are out of date (its pictures are dimmed).
const STALE: &str = "emcraft_view_stale";

/// The band over a result view whose model changed after its run: its mark, "Out of date" (why
/// on hover) and Re-run — the run the Setup step sets up, as step 7's Run starts it.
pub(crate) fn stale_band(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let r = egui::Frame::NONE.fill(t.accent_soft).corner_radius(t.radius_sm).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (m, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            crate::steps::stale_mark(ui.painter(), m.center(), t);
            ui.label(egui::RichText::new(tr("Out of date")).color(t.selected_ink)).hint("The model changed after this run.");
            let busy = app.busy();
            let hint = if busy { "a job is running" } else { "Solve again (the Setup step's run)" };
            ui.act(!busy, crate::ui_kit::styled("Re-run", true), Fixed::StaleRerun.into(), hint, || Action::StartRun, out);
        });
    });
    scenario::publish_rect(Fixed::Stale, r.response.rect);
}

/// The least height a picture is shrunk to so it fits the card's height: a smaller card scrolls.
const MIN_PICTURE_HEIGHT: f32 = 220.0;

/// A picture in the rest of the card, scaled down to its width and, down to
/// [`MIN_PICTURE_HEIGHT`], its height (dimmed while out of date); the image's response.
pub(super) fn picture(ui: &mut egui::Ui, p: &mut Picture, handle: Fixed) -> egui::Response {
    let size = p.size();
    let avail = ui.available_size();
    let tall = avail.y.max(MIN_PICTURE_HEIGHT.min(size.y));
    let scale = (avail.x / size.x).min(tall / size.y).clamp(0.1, 1.0);
    let id = p.texture(ui.ctx());
    let mut image = egui::Image::new((id, size * scale)).sense(Sense::click_and_drag());
    if ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::new(STALE))).unwrap_or(false) {
        image = image.tint(Color32::WHITE.gamma_multiply(0.55));
    }
    ui.named(image, handle.into())
}

/// Dims what `ui` draws from here on while the view's results are out of date (a result
/// table, as [`picture`] dims pictures).
pub(super) fn dim_if_stale(ui: &mut egui::Ui) {
    if ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::new(STALE))).unwrap_or(false) {
        ui.multiply_opacity(0.55);
    }
}

/// The least width pictures are drawn at (a narrower card shows them scaled down).
const MIN_PICTURE_WIDTH: f32 = 300.0;

/// Below this width a view stacks what it puts side by side (the field's picture and its
/// column, the port-mode table and its pattern).
pub(super) const NARROW: f32 = 640.0;

/// A width for pictures, rounded so resizing does not redraw on every pixel.
pub(super) fn picture_width(ui: &egui::Ui) -> usize {
    (((ui.available_width().max(MIN_PICTURE_WIDTH)) / 50.0).floor() * 50.0).min(2400.0) as usize
}

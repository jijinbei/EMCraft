//! The viewport card: the model's outline (H-plane and E-plane views of the guide or the
//! solid's bounding views, with materials, apertures, ports and sources), and the results — the
//! S-parameter chart against the exact curves, fields on a plane with a phase animation, the
//! port-mode and eigenmode tables, the far-field pattern and the comparison with theory. Charts
//! and fields are `emcraft-render`'s CPU pictures, shown as textures.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use emcraft_engine::view::{self, Plane};
use serde_json::{Value, json};

use crate::picture::{Cached, PictureKey};
use crate::state::{ChartView, FieldState, ViewState};
use crate::theme::Tokens;
use crate::view3d::View3d;
use crate::{Action, Actions, EmcraftApp, Picture, View, scenario};

mod dispersion;
mod far_field;
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
    let t = Tokens::get();
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
            scenario::publish_rect("viewport", r);
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink(10.0)));
            view_tabs(app, &mut inner, out);
            inner.add_space(4.0);
            if let Some(text) = app.view.missing(app) {
                empty(&mut inner, text);
                return;
            }
            let ui = &mut inner;
            match app.view {
                View::Model => model3d(app, st, ui, out),
                View::Outline => model(app, ui),
                View::SParameters => sparameters(app, st, ui, out),
                View::Tdr => tdr(app, st, ui),
                View::Field => field(app, st, ui, out),
                View::PortModes => port_modes(app, st, ui, out),
                View::FarField => far_field(app, st, ui),
                View::Compare => compare(app, ui, out),
                View::Eigenmodes => eigenmodes(app, st, ui, out),
                View::Study => study(app, st, ui),
                View::Dispersion => dispersion(app, st, ui),
            }
        });
    shown
}

/// The views with something to show, as a row of pills.
pub(super) fn view_tabs(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    ui.horizontal(|ui| {
        for v in View::ALL.into_iter().filter(|v| *v == app.view || v.available(app)) {
            if pill(ui, v.label(), app.view == v, format_args!("view:{}", v.id()), 12, PillSize::Body) {
                out.push(Action::ShowView(v));
            }
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

/// A pill of a row of options: filled when `on`, its text `size`; `true` when clicked.
pub(super) fn pill(ui: &mut egui::Ui, label: &str, on: bool, handle: impl std::fmt::Display, radius: u8, size: PillSize) -> bool {
    let t = Tokens::get();
    let font = match size {
        PillSize::Body => t.font_body,
        PillSize::Small => t.font_small,
    };
    let text = egui::RichText::new(label).size(font).color(if on { t.selected_ink } else { t.text });
    let r = ui.add(egui::Button::new(text).fill(if on { t.accent_soft } else { Color32::TRANSPARENT }).corner_radius(radius));
    scenario::publish_target(handle, r.rect);
    r.clicked()
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

/// A result view's chart in the rest of the card, as wide as the card allows: the one kept in
/// `slot`, drawn again from `chart` when the session (`revision`), the width, the theme or the
/// view's settings (`view`) changed. The picture's response and its size in pixels, or the
/// error that kept it from being drawn.
pub(super) fn chart_view<V: PartialEq, E: ToString>(
    ui: &mut egui::Ui,
    slot: &mut Cached<PictureKey<V>>,
    revision: u64,
    view: V,
    height: Height,
    handle: &str,
    chart: impl FnOnce() -> Result<emcraft_render::chart::Chart, E>,
) -> Result<(egui::Response, egui::Vec2), String> {
    let width = picture_width(ui);
    let height = height.of(ui);
    let p = slot.get_or_draw(PictureKey::new(revision, width, view), || chart().map(|c| emcraft_render::chart::draw(&c, width, height)))?;
    let size = p.size();
    Ok((picture(ui, p, handle), size))
}

pub(super) fn empty(ui: &mut egui::Ui, text: &str) {
    let t = Tokens::get();
    ui.centered_and_justified(|ui| {
        ui.label(egui::RichText::new(text).size(t.font_heading).color(t.text_dim));
    });
}

/// The card's message while a view has nothing to show, with the button that makes it; `true`
/// when the button was clicked.
pub(super) fn empty_with_action(ui: &mut egui::Ui, text: &str, button: &str, enabled: bool, handle: &str) -> bool {
    let t = Tokens::get();
    ui.vertical_centered(|ui| {
        ui.add_space(40.0);
        ui.label(egui::RichText::new(text).size(t.font_heading).color(t.text_dim));
        let r = ui.add_enabled(enabled, egui::Button::new(button));
        scenario::publish_target(handle, r.rect);
        r.clicked()
    })
    .inner
}

/// A picture in the rest of the card, scaled down to fit; the image's response.
pub(super) fn picture(ui: &mut egui::Ui, p: &mut Picture, handle: &str) -> egui::Response {
    let size = p.size();
    let avail = ui.available_size();
    let scale = (avail.x / size.x).min(avail.y / size.y).clamp(0.1, 1.0);
    let id = p.texture(ui.ctx());
    let r = ui.add(egui::Image::new((id, size * scale)).sense(Sense::click()));
    scenario::publish_target(handle, r.rect);
    r
}

/// A width for pictures, rounded so resizing does not redraw on every pixel.
pub(super) fn picture_width(ui: &egui::Ui) -> usize {
    (((ui.available_width().max(400.0)) / 50.0).floor() * 50.0).min(2400.0) as usize
}

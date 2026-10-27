//! The viewport card: the model's outline (H-plane and E-plane views of the guide or the
//! solid's bounding views, with materials, apertures, ports and sources), and the results — the
//! S-parameter chart against the exact curves, fields on a plane with a phase animation, the
//! port-mode and eigenmode tables, the far-field pattern and the comparison with theory. Charts
//! and fields are `emcraft-render`'s CPU pictures, shown as textures.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use emcraft_engine::view::{self, Plane};
use serde_json::{Value, json};

use crate::theme::Tokens;
use crate::{EmcraftApp, Picture, View, scenario};

mod far_field;
mod field_side;
mod fields;
mod model;
mod sparameters;
mod tables;
mod tdr;

use far_field::*;
use fields::*;
use model::*;
use sparameters::*;
use tables::*;
use tdr::*;

pub(crate) use fields::{field_plots, plane_span_of};
pub use sparameters::chart_frequency;

/// The central card.
pub fn show(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
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
            app.viewport_rect = Some(r);
            scenario::publish_rect("viewport", r);
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink(10.0)));
            view_tabs(app, &mut inner);
            inner.add_space(4.0);
            match app.view {
                View::Model => model3d(app, &mut inner),
                View::Outline => model(app, &mut inner),
                View::SParameters => sparameters(app, &mut inner),
                View::Tdr => tdr(app, &mut inner),
                View::Field => field(app, &mut inner),
                View::PortModes => port_modes(app, &mut inner),
                View::FarField => far_field(app, &mut inner),
                View::Compare => compare(app, &mut inner),
                View::Eigenmodes => eigenmodes(app, &mut inner),
            }
        });
}

/// Whether view `v` has something to show: the model always, results once solved.
pub(crate) fn available(app: &EmcraftApp, v: View) -> bool {
    let r = &app.session.results;
    match v {
        View::Model | View::Outline => true,
        View::SParameters | View::Tdr => r.sweep.is_some() || r.time_domain.is_some(),
        View::Field => r.sweep.is_some() || r.eigen.is_some() || r.radiation.is_some(),
        View::PortModes => app.port_modes.is_some() || (app.session.model.mesh.is_some() && !app.session.model.ports.is_empty()),
        View::FarField => r.radiation.is_some() || r.td_radiation.is_some(),
        View::Compare => r.sweep.is_some(),
        View::Eigenmodes => r.eigen.is_some(),
    }
}

/// The views with something to show, as a row of pills.
pub(super) fn view_tabs(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        let shown: Vec<View> = View::ALL.into_iter().filter(|v| *v == app.view || available(app, *v)).collect();
        for v in shown {
            let on = app.view == v;
            let text = egui::RichText::new(v.label()).size(t.font_body).color(if on { t.selected_ink } else { t.text });
            let r = ui.add(egui::Button::new(text).fill(if on { t.accent_soft } else { Color32::TRANSPARENT }).corner_radius(12));
            scenario::publish_target(&format!("view:{}", v.id()), r.rect);
            if r.clicked() {
                crate::ribbon::show_view(app, v);
            }
        }
    });
}

pub(super) fn empty(ui: &mut egui::Ui, text: &str) {
    let t = Tokens::get();
    ui.centered_and_justified(|ui| {
        ui.label(egui::RichText::new(text).size(t.font_heading).color(t.text_dim));
    });
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

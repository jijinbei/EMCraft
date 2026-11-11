//! The model in 3D (its section view: `crate::section3d`).

use super::*;
use crate::view::ViewExt;

/// The model in 3D, in the active step's overlay.
pub(super) fn model3d(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let mut overlay = app.place.step().map_or(crate::scene3d::Overlay::Plain, crate::steps::overlay);
    // In Results, the field's plane in the model.
    if app.place.step() == Some(crate::steps::Step::Results) && View::Field.available(app) {
        overlay = crate::scene3d::Overlay::Field;
    }
    crate::view3d::show(app, st, ui, area, overlay, out);
    // The field over the model: which quantity it colours, chosen here as in the Field view (E
    // or H, then its magnitude or a component), at the view's top right.
    if overlay == crate::scene3d::Overlay::Field {
        let t = &crate::theme::Tokens::get();
        // Its width as last drawn (the pills' text sets it), so it sits against the right edge.
        let width_id = egui::Id::new("emcraft_view3d_quantity_width");
        let width = ui.ctx().data(|d| d.get_temp::<f32>(width_id)).unwrap_or(260.0);
        let bar = Rect::from_min_size(pos2(area.right() - 8.0 - width, area.top() + 8.0), vec2(width, 40.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Min)));
        let r = egui::Frame::NONE
            .fill(t.panel.gamma_multiply(0.92))
            .stroke(egui::Stroke::new(1.0, t.border))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(6, 3))
            .show(&mut child, |ui| super::fields::component_choice(&mut st.field, ui));
        let drawn = r.response.rect.width();
        if (drawn - width).abs() > 0.5 {
            ui.ctx().data_mut(|d| d.insert_temp(width_id, drawn));
            ui.ctx().request_repaint();
        }
        scenario::publish_target(Fixed::View3dQuantity, r.response.rect);
    }
}

//! The model in 3D (its section view: `crate::section3d`).

use super::*;

/// The model in 3D, in the active step's overlay.
pub(super) fn model3d(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let mut overlay = app.workflow.step.map_or(crate::scene3d::Overlay::Plain, crate::steps::overlay);
    // In Results, the field's plane in the model.
    if app.workflow.step == Some(crate::steps::Step::Results) && View::Field.available(app) {
        overlay = crate::scene3d::Overlay::Field;
    }
    crate::view3d::show(app, &mut st.view3d, &mut st.field, ui, area, overlay, out);
}

//! The far-field pattern.

use super::*;

pub(super) fn far_field(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let mut phi = app.views.pattern_phi;
    ui.horizontal(|ui| {
        let r = crate::ui_kit::slider(ui, &mut phi, 0.0..=180.0, "azimuth φ (°)");
        scenario::publish_target("pattern:phi", r.rect);
    });
    if (phi - app.views.pattern_phi).abs() > 1e-9 {
        app.views.pattern_phi = phi;
    }
    let phi = app.views.pattern_phi;
    let width = picture_width(ui);
    let key = format!("p|{}|{width}|{phi:.3}", app.revision);
    let session = &app.session;
    let drawn =
        cached_chart(ui, &mut app.views.chart, key, width, Height::new(0.9, 260.0, 700.0), || emcraft_engine::open::pattern_chart(session, phi, 91));
    match drawn {
        Ok(p) => {
            picture(ui, p, "chart:pattern");
        }
        Err(e) => empty(ui, &e),
    }
}

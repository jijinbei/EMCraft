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
    let session = &app.session;
    let height = Height::new(0.9, 260.0, 700.0);
    let drawn = chart_view(ui, &mut app.views.chart, app.revision, ChartView::Pattern { phi }, height, "chart:pattern", || {
        emcraft_engine::open::pattern_chart(session, phi, 91)
    });
    if let Err(e) = drawn {
        empty(ui, &e);
    }
}

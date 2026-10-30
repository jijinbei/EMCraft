//! The far-field pattern at an azimuth.

use super::*;

pub(super) struct FarField;

impl ChartView for FarField {
    type Key = SharedChart;
    const HANDLE: Fixed = Fixed::ChartPattern;
    const HEIGHT: Height = Height::new(0.9, 260.0, 700.0);

    /// The azimuth.
    fn controls(_: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui) -> Option<SharedChart> {
        let mut phi = st.views.pattern_phi;
        ui.horizontal(|ui| {
            let r = crate::ui_kit::slider(ui, &mut phi, 0.0..=180.0, tr("azimuth φ (°)"));
            scenario::publish_target(Fixed::PatternPhi, r.rect);
        });
        if (phi - st.views.pattern_phi).abs() > 1e-9 {
            st.views.pattern_phi = phi;
        }
        Some(SharedChart::Pattern { phi: st.views.pattern_phi })
    }

    fn chart(app: &EmcraftApp, key: &SharedChart) -> Result<emcraft_render::chart::Chart, String> {
        let SharedChart::Pattern { phi } = key else { return Err("not a pattern".into()) };
        emcraft_engine::open::pattern_chart(&app.session, *phi, 91).map_err(|e| e.to_string())
    }
}

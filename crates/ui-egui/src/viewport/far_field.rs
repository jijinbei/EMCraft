//! The far-field pattern.

use super::*;

pub(super) fn far_field(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    if app.session.results.radiation.is_none() && app.session.results.td_radiation.is_none() {
        empty(ui, "No far field yet: SIMULATE › Radiation (an open model with sources or lumped ports).");
        return;
    }
    let mut phi = app.pattern_phi;
    ui.horizontal(|ui| {
        let r = crate::ui_kit::slider(ui, &mut phi, 0.0..=180.0, "azimuth φ (°)");
        scenario::publish_target("pattern:phi", r.rect);
    });
    if (phi - app.pattern_phi).abs() > 1e-9 {
        app.pattern_phi = phi;
    }
    let width = picture_width(ui);
    let key = format!("p|{}|{width}|{:.3}", app.revision, app.pattern_phi);
    if app.chart.as_ref().is_none_or(|c| c.key != key) {
        match emcraft_engine::open::pattern_chart(&app.session, app.pattern_phi, 91) {
            Ok(chart) => {
                let height = (ui.available_height() * 0.9).clamp(260.0, 700.0) as usize;
                app.chart = Some(Picture::new(key, &emcraft_render::chart::draw(&chart, width, height)));
            }
            Err(e) => {
                empty(ui, &e.to_string());
                return;
            }
        }
    }
    if let Some(p) = app.chart.as_mut() {
        picture(ui, p, "chart:pattern");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_clicks_map_to_frequencies() {
        assert_eq!(chart_frequency(90.0 / 1000.0, 1000.0, 5.0, 9.0), Some(5.0));
        assert!((chart_frequency(530.0 / 1000.0, 1000.0, 5.0, 9.0).unwrap_or(0.0) - 7.0).abs() < 1e-12);
        assert_eq!(chart_frequency(0.01, 1000.0, 5.0, 9.0), None);
        assert_eq!(crate::nearest(&[5.0, 6.0, 7.0], 6.4), Some(6.0));
        assert_eq!(crate::nearest(&[], 6.4), None);
    }
}

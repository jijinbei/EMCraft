//! Time-domain reflectometry: a port's reflection transformed to time, low pass (a step at each
//! change of impedance) or band pass (a peak where each reflection comes from).

use emcraft_engine::tdr::Mode;

use super::*;

pub(super) fn tdr(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    if app.session.results.sweep.is_none() && app.session.results.time_domain.is_none() {
        empty(ui, "No sweep yet: SIMULATE › Frequency sweep (a harmonic one, f = k Δf, for low pass).");
        return;
    }
    // The port and the transform.
    let ports: Vec<String> = app.session.model.ports.iter().map(|p| p.name.clone()).collect();
    ui.horizontal(|ui| {
        ui.label("port");
        let shown = app.tdr.0.clone().or_else(|| ports.first().cloned()).unwrap_or_default();
        let r = egui::ComboBox::from_id_salt("tdr:port").selected_text(shown.as_str()).show_ui(ui, |ui| {
            for p in &ports {
                if ui.selectable_label(Some(p) == app.tdr.0.as_ref(), p.as_str()).clicked() {
                    app.tdr.0 = Some(p.clone());
                }
            }
        });
        scenario::publish_target("tdr:port", r.response.rect);
        ui.separator();
        for (mode, label, tip) in [
            (None, "Auto", "Low pass on a harmonic sweep (f = k Δf), else band pass"),
            (Some(Mode::LowPass), "Low pass", "The step response: a step at each change of impedance (lines without a cut-off)"),
            (Some(Mode::BandPass), "Band pass", "|h(t)|: a peak where each reflection comes from (guides with a cut-off)"),
        ] {
            let r = ui.selectable_label(app.tdr.1 == mode, label).on_hover_text(tip);
            scenario::publish_target(&format!("tdr:mode:{}", mode.map_or("auto", Mode::name)), r.rect);
            if r.clicked() {
                app.tdr.1 = mode;
            }
        }
    });
    let mut params = json!({});
    if let Some(p) = &app.tdr.0 {
        params["port"] = json!(p);
    }
    if let Some(m) = app.tdr.1 {
        params["mode"] = json!(m.name());
    }
    let width = picture_width(ui);
    let key = format!("tdr|{params}|{}|{width}|{}", app.revision, crate::theme::is_dark());
    if app.tdr_chart.as_ref().is_none_or(|c| c.key != key) {
        let reader = emcraft_engine::Params::new(&params, app.session.display);
        match emcraft_engine::results_tdr_chart(&app.session, &reader) {
            Ok(chart) => {
                let height = (ui.available_height() * 0.8).clamp(260.0, 700.0) as usize;
                app.tdr_chart = Some(Picture::new(key, &emcraft_render::chart::draw(&chart, width, height)));
            }
            Err(e) => {
                app.tdr_chart = None;
                empty(ui, &e.to_string());
                return;
            }
        }
    }
    if let Some(p) = app.tdr_chart.as_mut() {
        picture(ui, p, "chart:tdr");
    }
    ui.label(
        egui::RichText::new(
            "Time is the round trip from the port's reference plane; resolution about 1/(2 f_max) (low pass) or 1/bandwidth (band pass).",
        )
        .size(t.font_small)
        .color(t.text_dim),
    );
}

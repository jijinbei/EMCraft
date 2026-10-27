//! Time-domain reflectometry: a port's reflection transformed to time, low pass (a step at each
//! change of impedance) or band pass (a peak where each reflection comes from).

use emcraft_engine::tdr::Mode;

use super::*;

/// The transforms offered, with their labels and tips (`None`: the engine picks).
const MODES: [(Option<Mode>, &str, &str); 3] = [
    (None, "Auto", "Low pass on a harmonic sweep (f = k Δf), else band pass"),
    (Some(Mode::LowPass), "Low pass", "The step response: a step at each change of impedance (lines without a cut-off)"),
    (Some(Mode::BandPass), "Band pass", "|h(t)|: a peak where each reflection comes from (guides with a cut-off)"),
];

pub(super) fn tdr(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    controls(app, ui);
    let tdr = &mut app.views.tdr;
    let mut params = json!({});
    if let Some(p) = &tdr.port {
        params["port"] = json!(p);
    }
    if let Some(m) = tdr.mode {
        params["mode"] = json!(m.name());
    }
    let width = picture_width(ui);
    let key = format!("tdr|{params}|{}|{width}|{}", app.revision, crate::theme::is_dark());
    let session = &app.session;
    let chart = cached_chart(ui, &mut tdr.chart, key, width, Height::new(0.8, 260.0, 700.0), || {
        emcraft_engine::results_tdr_chart(session, &emcraft_engine::Params::new(&params, session.display))
    });
    match chart {
        Ok(p) => {
            picture(ui, p, "chart:tdr");
        }
        Err(e) => return empty(ui, &e),
    }
    crate::ui_kit::caption(
        ui,
        "Time is the round trip from the port's reference plane; resolution about 1/(2 f_max) (low pass) or 1/bandwidth (band pass).",
    );
}

/// The port and the transform.
fn controls(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let ports: Vec<String> = app.session.model.ports.iter().map(|p| p.name.clone()).collect();
    let tdr = &mut app.views.tdr;
    ui.horizontal(|ui| {
        ui.label("port");
        let shown = tdr.port.clone().or_else(|| ports.first().cloned()).unwrap_or_default();
        let r = egui::ComboBox::from_id_salt("tdr:port").selected_text(shown.as_str()).show_ui(ui, |ui| {
            for p in &ports {
                if ui.selectable_label(Some(p) == tdr.port.as_ref(), p.as_str()).clicked() {
                    tdr.port = Some(p.clone());
                }
            }
        });
        scenario::publish_target("tdr:port", r.response.rect);
        ui.separator();
        for (mode, label, tip) in MODES {
            let r = ui.selectable_label(tdr.mode == mode, label).on_hover_text(tip);
            scenario::publish_target(&format!("tdr:mode:{}", mode.map_or("auto", Mode::name)), r.rect);
            if r.clicked() {
                tdr.mode = mode;
            }
        }
    });
}

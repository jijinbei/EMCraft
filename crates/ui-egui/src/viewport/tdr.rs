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

pub(super) struct Tdr;

impl ChartView for Tdr {
    type Key = crate::state::TdrParams;
    const HANDLE: Fixed = Fixed::ChartTdr;
    const HEIGHT: Height = Height::new(0.8, 260.0, 700.0);

    fn controls(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui) -> Option<Self::Key> {
        controls(app, &mut st.views.tdr.params, ui);
        Some(st.views.tdr.params.clone())
    }

    fn chart(app: &EmcraftApp, key: &Self::Key) -> Result<emcraft_render::chart::Chart, String> {
        let s = &app.session;
        emcraft_engine::results_tdr_chart(s, &emcraft_engine::Params::new(&key.to_params(), s.display())).map_err(|e| e.to_string())
    }

    fn below(_: &EmcraftApp, _: &mut Parts, ui: &mut egui::Ui, _: &egui::Response, _: egui::Vec2, _: &mut Actions) {
        crate::ui_kit::caption(
            ui,
            "Time is the round trip from the port's reference plane; resolution about 1/(2 f_max) (low pass) or 1/bandwidth (band pass).",
        );
    }
}

/// The port and the transform.
fn controls(app: &EmcraftApp, tdr: &mut crate::state::TdrParams, ui: &mut egui::Ui) {
    let ports: Vec<String> = app.session.sparameter_entries().into_iter().map(|e| e.name).collect();
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
        scenario::publish_target(Fixed::TdrPort, r.response.rect);
        ui.separator();
        for (mode, label, tip) in MODES {
            let r = ui.named(egui::Button::selectable(tdr.mode == mode, label), Handle::TdrMode(mode.map_or("auto", Mode::name))).on_hover_text(tip);
            if r.clicked() {
                tdr.mode = mode;
            }
        }
    });
}

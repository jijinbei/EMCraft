//! A chirped pulse through the line (`results.pulse`): its settings, run on the worker, and its
//! input and output against time — their power, or their instantaneous frequency.

use super::*;

/// The chirps offered, their ids and labels.
const CHIRPS: [(&str, crate::lang::Text, crate::lang::Text); 4] = [
    ("optimized", t!("Optimized"), t!("The better of the best linear and the best matched chirp")),
    ("matched", t!("Matched"), t!("Its frequency follows the line's group delay")),
    ("linear", t!("Linear"), t!("A straight sweep: the band given, else −1/D")),
    ("none", t!("None"), t!("No chirp")),
];

pub(super) struct Pulse;

impl ChartView for Pulse {
    type Key = crate::state::PulseKey;
    const HANDLE: Fixed = Fixed::ChartPulse;
    const CHART: emcraft_engine::views::ChartId = emcraft_engine::views::ChartId::Pulse;
    const HEIGHT: Height = Height::new(0.8, 260.0, 700.0);

    fn controls(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) -> Option<Self::Key> {
        controls(app, &mut st.views.pulse, ui, out);
        app.replies.pulse.as_ref().map(|_| crate::state::PulseKey { run: st.views.pulse.runs, frequency: st.views.pulse.frequency })
    }

    fn chart(app: &EmcraftApp, key: &Self::Key) -> Result<emcraft_render::chart::Chart, String> {
        let reply = app.replies.pulse.as_ref().ok_or("no pulse yet")?;
        Ok(emcraft_engine::view::pulse::chart(reply, key.frequency, app.session.display().frequency.symbol()))
    }
}

/// The carrier, width, length and chirp, Run, and the power or frequency switch.
fn controls(app: &EmcraftApp, pulse: &mut crate::state::PulseState, ui: &mut egui::Ui, out: &mut Actions) {
    let p = &mut pulse.params;
    let fu = app.session.display().frequency.symbol();
    let lu = app.session.display().length.symbol();
    let bloch = app.session.results().revolution.as_ref().is_some_and(|r| r.bloch.is_some());
    ui.horizontal_wrapped(|ui| {
        ui.text(Style::Body, t!("carrier"));
        let mut f = p.frequency.unwrap_or(0.0);
        let r = ui.named(egui::DragValue::new(&mut f).speed(0.01).range(0.0..=1e6).suffix(format!(" {fu}")), Fixed::PulseFrequency.into());
        if r.changed() {
            p.frequency = Some(f).filter(|f| *f > 0.0);
        }
        ui.text(Style::Body, t!("width"));
        ui.named(egui::DragValue::new(&mut p.width).speed(0.1).range(0.01..=1e5).suffix(" ns"), Fixed::PulseWidth.into());
        if bloch {
            ui.text(Style::Body, t!("length"));
            let mut l = p.length.unwrap_or(0.0);
            if ui.named(egui::DragValue::new(&mut l).speed(10.0).range(0.0..=1e7).suffix(format!(" {lu}")), Fixed::PulseLength.into()).changed() {
                p.length = Some(l).filter(|l| *l > 0.0);
            }
        }
        ui.separator();
        let now = p.chirp.clone().unwrap_or_else(|| if p.bandwidth.is_some() { "linear".into() } else { "optimized".into() });
        for (id, label, tip) in CHIRPS {
            if ui.named(egui::Button::selectable(now == id, tr(label)), Handle::PulseChirp(id)).hint(tip).clicked() {
                p.chirp = Some(id.to_owned());
                if id != "linear" {
                    p.bandwidth = None;
                }
            }
        }
        if now == "linear" {
            let mut b = p.bandwidth.unwrap_or(0.0);
            if ui
                .named(egui::DragValue::new(&mut b).speed(0.01).range(-1e6..=1e6).suffix(format!(" {fu}")), Fixed::PulseBandwidth.into())
                .hint(t!("The band the chirp sweeps: rising > 0, falling < 0"))
                .changed()
            {
                p.bandwidth = Some(b);
            }
        }
        ui.separator();
        let busy = app.busy();
        let run = ui.named_enabled(!busy, egui::Button::new(tr(t!("Run"))), Fixed::PulseRun.into());
        if run.clicked() {
            out.push(Action::run("results.pulse", p.to_params()));
        }
        ui.separator();
        for (on, label) in [(false, t!("Power")), (true, t!("Frequency"))] {
            if ui
                .named(egui::Button::selectable(pulse.frequency == on, tr(label)), Handle::PulseShow(if on { "frequency" } else { "power" }))
                .clicked()
            {
                pulse.frequency = on;
            }
        }
    });
}

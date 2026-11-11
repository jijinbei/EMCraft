//! A system's Chain step (where a model's geometry stands): its stages from the source on, each
//! with the chain's gain and noise figure at its output once read, a click editing it here (its
//! dialog docked under), buttons adding each kind; the source and the receiver.

use super::*;
use crate::lang::{Raw, args};
use crate::system_ui;

pub(super) fn chain(app: &EmcraftApp, ui: &mut egui::Ui, has_dialog: bool, out: &mut Actions) {
    let t = &Tokens::get();
    let m = app.session.model();
    let busy = app.busy();
    ui_kit::section(ui, t!("Source"));
    let src = &m.system.source;
    let antenna = src.antenna.clone().unwrap_or_else(|| format!("{} Ω", m.system.receiver.impedance));
    let power = src.power.map_or_else(String::new, |p| format!(", {p} dBm"));
    ui_kit::small(ui, Raw(&format!("{antenna}  ·  {} K{power}", src.temperature)), t.text);
    if ui.named_enabled(!busy, egui::Button::new(tr(t!("Source…"))).small(), Handle::SystemPanel("source")).clicked() {
        out.push(Action::dialog("system.source"));
    }
    ui_kit::section(ui, t!("Stages"));
    if m.system.stages.is_empty() {
        ui_kit::small(ui, t!("No stages yet: add an amplifier, a filter's S-parameters or a line with the buttons below."), t.text_dim);
    }
    for (k, stage) in m.system.stages.iter().enumerate() {
        let read = system_ui::reading(&app.session, &stage.name).map_or_else(String::new, |(g, nf)| {
            let f = |x: Option<f64>| x.map_or_else(|| "—".to_owned(), |v| format!("{v:.2}"));
            trf(t!("  →  total {gain} dB, NF {nf} dB"), args!(gain = f(g), nf = f(nf)))
        });
        let text = format!("{}. {}  ·  {}  ·  {}{read}", k + 1, stage.name, tr(system_ui::kind_name(&stage.part)), system_ui::summary(stage));
        let r =
            ui.named_enabled(!busy, egui::Button::new(egui::RichText::new(text).size(t.font_small)).frame(false), Handle::SystemPanel(&stage.name));
        if r.on_hover_text(tr(t!("Click to change it here"))).clicked() {
            out.extend(system_ui::edit(&stage.name));
        }
    }
    if !has_dialog {
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            for (kind, label, tip) in system_ui::KINDS {
                let b = egui::Button::new(format!("+ {}", tr(label))).small();
                if ui.named_enabled(!busy, b, Handle::SystemPanel(kind)).on_hover_text(tr(tip)).clicked() {
                    out.extend(system_ui::add(kind));
                }
            }
        });
    }
    ui_kit::section(ui, t!("Receiver"));
    let r = &m.system.receiver;
    let fu = app.session.display().frequency;
    let bw = r.bandwidth.map_or_else(|| tr(t!("no bandwidth")).to_owned(), |b| format!("{} {}", fu.from_si(b), fu.symbol()));
    ui_kit::small(ui, Raw(&format!("{} Ω  ·  {bw}  ·  SNR {} dB", r.impedance, r.snr)), t.text);
    if ui.named_enabled(!busy, egui::Button::new(tr(t!("Receiver…"))).small(), Handle::SystemPanel("receiver")).clicked() {
        out.push(Action::dialog("system.receiver"));
    }
}

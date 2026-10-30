//! The Excitation step: the ports and sources, and the port-mode preview.

use super::*;
use crate::Hint;
use crate::lang::args;

pub(super) fn excitation(app: &EmcraftApp, ui: &mut egui::Ui, fu: &str, t: &Tokens, out: &mut Actions) {
    ui_kit::section(ui, "Ports and sources");
    let m = app.session.model();
    if m.ports.is_empty() && m.lumped.is_empty() && m.sources.is_empty() {
        ui_kit::small(ui, "none yet", t.text_dim);
    }
    for p in &m.ports {
        ui_kit::small(ui, crate::browser::port_text(p).shown, t.text);
    }
    for p in &m.lumped {
        ui_kit::small(ui, trf("Lumped port {name}: {impedance} Ω", args!(name = p.name, impedance = p.impedance)), t.text);
    }
    if !m.sources.is_empty() {
        let n = m.sources.len();
        ui_kit::small(ui, crate::lang::plural(n, "{n} source", "{n} sources", args!(n = n)), t.text);
    }
    if m.ports.is_empty() {
        return;
    }
    ui_kit::section(ui, "Port modes");
    let ready = m.mesh.is_some();
    let about = match band(&app.session) {
        Some((_, z)) => trf(
            "Each port's modes at {f} {unit}: propagation, effective permittivity, impedance (seconds, no full solve).",
            args!(f = fmt(z), unit = fu),
        ),
        None => tr("Each port's modes: propagation, effective permittivity, impedance (seconds, no full solve).").to_owned(),
    };
    ui_kit::caption(ui, about);
    let busy = app.busy();
    let hint = if ready { "" } else { "needs a mesh (step 5)" };
    ui.act(ready && !busy, ui_kit::styled("Preview port modes", true), Fixed::ExcitationPreview.into(), hint, || Action::PreviewPortModes, out);
    let Some(reply) = app.replies.port_modes.as_ref() else { return };
    let f = reply.get("frequency").and_then(Value::as_f64).unwrap_or(f64::NAN);
    ui_kit::small(ui, trf("at {f} {unit}", args!(f = fmt(f), unit = fu)), t.text_dim);
    mode_table(app, ui, reply, t, out);
    ui.act(true, ui_kit::styled("Show the mode patterns", false), Fixed::ExcitationPatterns.into(), "", || Action::SetView(View::PortModes), out);
}

/// The port-mode preview: a row per mode. Hovering a row lights its port in the 3D view;
/// clicking it opens the Port modes view at that mode.
fn mode_table(app: &EmcraftApp, ui: &mut egui::Ui, reply: &Value, t: &Tokens, out: &mut Actions) {
    let unit = reply.get("length_unit").and_then(Value::as_str).unwrap_or("m");
    let w = ui.available_width();
    let widths = [w * 0.34, w * 0.24, w * 0.2, w * 0.2];
    let cell = |ui: &mut egui::Ui, width: f32, text: egui::RichText| {
        ui.allocate_ui_with_layout(vec2(width, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_width(width);
            ui.add_space(4.0);
            ui.add(egui::Label::new(text).truncate());
        });
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let per = format!("β (rad/{unit})");
        for (h, width) in [tr("port / mode"), per.as_str(), "εeff", "Z (Ω)"].into_iter().zip(widths) {
            cell(ui, width, Style::SmallDim.rich(h));
        }
    });
    let mut row = 0usize;
    let mut hovered = None;
    let mut clicked = None;
    for p in reply.get("ports").and_then(Value::as_array).into_iter().flatten() {
        let name = p.get("name").and_then(Value::as_str).unwrap_or("?");
        let port = app.session.model().ports.iter().position(|q| q.name == name);
        for (k, mode) in p.get("modes").and_then(Value::as_array).into_iter().flatten().take(8).enumerate() {
            let n = |key: &str| mode.get(key).and_then(Value::as_f64);
            let txt = |s: String| egui::RichText::new(s).size(t.font_small).color(t.text);
            let back = ui.painter().add(egui::Shape::Noop);
            let r = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                cell(ui, widths[0], txt(format!("{name} · {}", mode.get("name").and_then(Value::as_str).unwrap_or(""))));
                if mode.get("propagating").and_then(Value::as_bool) == Some(true) {
                    cell(ui, widths[1], txt(n("beta").map_or("—".into(), |v| format!("{v:.4}"))));
                    cell(ui, widths[2], txt(n("eps_eff").map_or("—".into(), |v| format!("{v:.4}"))));
                    cell(ui, widths[3], txt(n("impedance").map_or("—".into(), |v| format!("{v:.2}"))));
                } else {
                    let a = n("attenuation_db").unwrap_or(f64::NAN);
                    ui_kit::small(ui, trf("cut off, {a} dB/{unit}", args!(a = format!("{a:.1}"), unit = unit)), t.text_dim);
                }
            });
            let rect = Rect::from_min_size(r.response.rect.min, vec2(w, r.response.rect.height()));
            let resp = ui.hit(rect, Handle::PortModeRow(row), Sense::click());
            let resp = resp.hint("Lights this port in the 3D view; click for the mode in the Port modes view");
            let focused = app.workflow.mode_focus.as_ref().is_some_and(|f| f.port == name && f.mode == k);
            let fill = if resp.hovered() {
                t.hover
            } else if focused {
                t.accent_soft
            } else if row % 2 == 1 {
                t.text.gamma_multiply(0.04)
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().set(back, egui::Shape::rect_filled(rect, 3.0, fill));
            if resp.hovered() {
                hovered = port;
            }
            if resp.clicked() {
                clicked = Some((name.to_owned(), k));
            }
            row += 1;
        }
    }
    if let Some(i) = hovered {
        out.push(Action::Hover(crate::scene3d::Pick::Port(i)));
    }
    if let Some((port, mode)) = clicked {
        out.push(Action::FocusMode(ModeFocus { port, mode, scroll: true }));
        out.push(Action::SetView(View::PortModes));
    }
}

//! The Excitation step: the ports and sources, and the port-mode preview.

use super::*;
use crate::Hint;
use crate::lang::args;

pub(super) fn excitation(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let fu = app.session.display().frequency.symbol();
    let t = &Tokens::get();
    // Ports are made from the ribbon and the 3D view's tools; the panel lists them.
    ui_kit::section(ui, "Ports and sources");
    let m = app.session.model();
    if m.ports.is_empty() && m.lumped.is_empty() && m.sources.is_empty() {
        ui_kit::small(ui, "none yet", t.text_dim);
    }
    port_roles(app, ui, out);
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
    let hint = if ready { "" } else { "needs a mesh (step 6)" };
    ui.act(ready && !busy, ui_kit::styled("Preview port modes", true), Fixed::ExcitationPreview.into(), || Action::PreviewPortModes, out).hint(hint);
    let Some(reply) = app.replies.port_modes.as_ref() else { return };
    ui_kit::small(ui, trf("at {f} {unit}", args!(f = fmt(reply.frequency), unit = fu)), t.text_dim);
    // A row opens its mode in the Port modes view.
    mode_table(app, ui, reply, out);
}

/// The waveguide ports, each with its role (the input, whose reflection is S11, or an output
/// and the S-parameter of its transmission) and, for an output, the choice to drive it instead
/// (`view.driven`).
fn port_roles(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let m = app.session.model();
    for (p, role) in m.ports.iter().zip(crate::browser::port_roles(&app.session)) {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 14.0), Sense::hover());
            crate::browser::role_mark(ui.painter(), r, role.input, role.colour());
            let text = crate::browser::port_text(p).shown;
            crate::math::label(ui, &format!("{text}  ·  {}", role.tip()), t.font_small, t.text);
            if !role.input {
                let b = egui::Button::new(egui::RichText::new(tr("Make the input")).size(t.font_small)).small();
                let params = serde_json::json!({"port": role.name});
                ui.act(!app.busy(), b, Handle::DrivePort(&role.name), || Action::run("view.driven", params), out);
            }
        });
    }
}

/// The port-mode preview: a row per mode. Hovering a row lights its port in the 3D view;
/// clicking it opens the Port modes view at that mode.
fn mode_table(app: &EmcraftApp, ui: &mut egui::Ui, reply: &emcraft_engine::PortModes, out: &mut Actions) {
    let t = &Tokens::get();
    let unit = reply.length_unit.as_str();
    let w = ui.available_width();
    let widths = [w * 0.34, w * 0.24, w * 0.2, w * 0.2];
    let cell = |ui: &mut egui::Ui, width: f32, text: egui::WidgetText| {
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
            // εeff and β set as mathematics.
            cell(ui, width, crate::math::rich(h, t.font_small, t.text_dim).into());
        }
    });
    let mut row = 0usize;
    let mut hovered = None;
    let mut clicked = None;
    for p in &reply.ports {
        let name = p.name.as_str();
        let port = app.session.model().ports.iter().position(|q| q.name == name);
        for (k, mode) in p.modes.iter().take(8).enumerate() {
            let txt = |s: String| egui::WidgetText::from(egui::RichText::new(s).size(t.font_small).color(t.text));
            let back = ui.painter().add(egui::Shape::Noop);
            let r = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                cell(ui, widths[0], txt(format!("{name} · {}", mode.name)));
                if mode.propagating {
                    cell(ui, widths[1], txt(mode.beta.map_or_else(|| "—".into(), |v| format!("{v:.4}"))));
                    cell(ui, widths[2], txt(mode.eps_eff.map_or_else(|| "—".into(), |v| format!("{v:.4}"))));
                    cell(ui, widths[3], txt(mode.impedance.map_or_else(|| "—".into(), |v| format!("{v:.2}"))));
                } else {
                    let a = mode.attenuation_db.unwrap_or(f64::NAN);
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

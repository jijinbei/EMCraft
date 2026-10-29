//! The Excitation step: the ports and sources, and the port-mode preview.

use super::*;

pub(super) fn excitation(app: &mut EmcraftApp, ui: &mut egui::Ui, fu: &str, t: &Tokens) {
    ui_kit::section(ui, "Ports and sources");
    let m = app.session.model();
    if m.ports.is_empty() && m.lumped.is_empty() && m.sources.is_empty() {
        ui_kit::small(ui, "none yet", t.text_dim);
    }
    for p in &m.ports {
        let at = match p.face {
            Some(_) => "on a face".to_owned(),
            None => format!("{} end", if p.end == emcraft_model::End::Start { "start" } else { "far" }),
        };
        ui_kit::small(ui, format!("Port {}: {at}, {} modes", p.name, p.modes), t.text);
    }
    for p in &m.lumped {
        ui_kit::small(ui, format!("Lumped port {}: {} Ω", p.name, p.impedance), t.text);
    }
    if !m.sources.is_empty() {
        ui_kit::small(ui, format!("{} source(s)", m.sources.len()), t.text);
    }
    if m.ports.is_empty() {
        return;
    }
    ui_kit::section(ui, "Port modes");
    let ready = m.mesh.is_some();
    let at = band(app).map(|(_, z)| format!(" at {} {fu}", fmt(z))).unwrap_or_default();
    ui_kit::caption(ui, &format!("Each port's modes{at}: propagation, effective permittivity, impedance (seconds, no full solve)."));
    let busy = app.busy();
    if ui_kit::button(ui, "Preview port modes", true, ready && !busy, "excitation:preview")
        .on_hover_text(if ready { "" } else { "needs a mesh (step 5)" })
        .clicked()
    {
        let r =
            if app.session.model().band.is_some() { app.run("solve.port_modes", json!({})).map(|_| ()) } else { app.open_dialog("solve.port_modes") };
        if let Err(e) = r {
            app.error(e);
        }
    }
    let Some(reply) = app.views.port_modes.clone() else { return };
    let f = reply.get("frequency").and_then(Value::as_f64).unwrap_or(f64::NAN);
    ui_kit::small(ui, format!("at {} {fu}", fmt(f)), t.text_dim);
    mode_table(app, ui, &reply, t);
    if ui_kit::button(ui, "Show the mode patterns", false, true, "excitation:patterns").clicked() {
        app.view = View::PortModes;
    }
}

/// The port-mode preview: a row per mode. Hovering a row lights its port in the 3D view;
/// clicking it opens the Port modes view at that mode.
fn mode_table(app: &mut EmcraftApp, ui: &mut egui::Ui, reply: &Value, t: &Tokens) {
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
        for (h, width) in ["port / mode", per.as_str(), "εeff", "Z (Ω)"].into_iter().zip(widths) {
            cell(ui, width, egui::RichText::new(h).size(t.font_small).color(t.text_dim));
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
                    ui_kit::small(ui, format!("cut off, {a:.1} dB/{unit}"), t.text_dim);
                }
            });
            let rect = Rect::from_min_size(r.response.rect.min, vec2(w, r.response.rect.height()));
            let resp = ui.interact(rect, ui.id().with(("port_mode_row", row)), Sense::click());
            let resp = resp.on_hover_text("Lights this port in the 3D view; click for the mode in the Port modes view");
            scenario::publish_target(format_args!("portmode:row:{row}"), rect);
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
        app.view3d.hover = Some(crate::scene3d::Pick::Port(i));
    }
    if let Some((port, mode)) = clicked {
        app.workflow.mode_focus = Some(ModeFocus { port, mode, scroll: true });
        app.view = View::PortModes;
    }
}

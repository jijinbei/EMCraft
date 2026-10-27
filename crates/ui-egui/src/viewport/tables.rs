//! The port-mode, eigenmode and comparison tables.

use super::*;

pub(super) fn port_modes(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let Some(reply) = app.port_modes.clone() else {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(egui::RichText::new("No port modes yet: they need a meshed model with ports.").size(t.font_heading).color(t.text_dim));
            let r = ui.add_enabled(!app.busy(), egui::Button::new("Solve port modes…"));
            scenario::publish_target("port_modes:solve", r.rect);
            if r.clicked()
                && let Err(e) = app.open_dialog("solve.port_modes")
            {
                app.message(e, true);
            }
        });
        return;
    };
    let lu = reply.get("length_unit").and_then(Value::as_str).unwrap_or("mm").to_owned();
    let fu = app.session.display.frequency.symbol();
    ui.label(egui::RichText::new(format!("Port modes at {} {fu}", num(&reply["frequency"], 4))).size(t.font_heading));
    // The mode picked (from the Excitation panel or a click here), else the first port's first.
    if app.workflow.mode_focus.is_none()
        && let Some(name) = reply.pointer("/ports/0/name").and_then(Value::as_str)
    {
        app.workflow.mode_focus = Some(crate::steps::ModeFocus { port: name.to_owned(), mode: 0, scroll: false });
    }
    let focus = app.workflow.mode_focus.clone();
    let scroll = focus.as_ref().is_some_and(|f| f.scroll);
    if let Some(f) = app.workflow.mode_focus.as_mut() {
        f.scroll = false;
    }
    // The table on the left, the picked mode's picture on the right.
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let split = area.left() + (area.width() * 0.42).round();
    let mut table_ui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(area.min, pos2(split - 8.0, area.max.y))));
    let mut picture_ui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(pos2(split + 8.0, area.min.y), area.max)));
    let mut picked = None;
    let ui = &mut table_ui;
    egui::ScrollArea::vertical().id_salt("modes_table").show(ui, |ui| {
        for port in reply.get("ports").and_then(Value::as_array).into_iter().flatten() {
            crate::ui_kit::section(ui, &format!("Port {}", port["name"].as_str().unwrap_or("")));
            egui::Grid::new(("modes", port["name"].as_str().unwrap_or(""))).striped(true).num_columns(6).show(ui, |ui| {
                for h in ["Mode", "Propagates", &format!("β (rad/{lu})"), "εeff", "Z (Ω)", &format!("Attenuation (dB/{lu})")] {
                    ui.label(egui::RichText::new(h).strong());
                }
                ui.end_row();
                let pname = port["name"].as_str().unwrap_or("");
                for (k, m) in port.get("modes").and_then(Value::as_array).into_iter().flatten().enumerate() {
                    // The mode picked in the Excitation panel: marked, and scrolled to once.
                    let focused = focus.as_ref().is_some_and(|f| f.port == pname && f.mode == k);
                    let back = ui.painter().add(egui::Shape::Noop);
                    let cells = [
                        ui.label(m["name"].as_str().unwrap_or("?")),
                        ui.label(if m["propagating"] == json!(true) { "yes" } else { "no" }),
                        ui.label(num(&m["beta"], 5)),
                        ui.label(num(&m["eps_eff"], 5)),
                        ui.label(num(&m["impedance"], 3)),
                        ui.label(num(&m["attenuation_db"], 3)),
                    ];
                    ui.end_row();
                    let row = cells.iter().fold(cells[0].rect, |r, c| r.union(c.rect)).expand2(egui::vec2(4.0, 1.0));
                    let click = ui.interact(row, ui.id().with(("mode_row", pname, k)), Sense::click());
                    scenario::publish_target(&format!("port_modes:row:{pname}:{}", k + 1), row);
                    if click.on_hover_text("Show this mode's pattern").clicked() {
                        picked = Some((pname.to_owned(), k));
                    }
                    if focused {
                        let r = row;
                        ui.painter().set(back, egui::Shape::rect_filled(r, 3.0, t.accent_soft));
                        scenario::publish_target("port_modes:focus", r);
                        if scroll {
                            ui.scroll_to_rect(r, Some(egui::Align::Center));
                        }
                    }
                }
            });
        }
    });
    scenario::publish_rect("table:port_modes", ui.min_rect());
    if let Some((port, mode)) = picked {
        app.workflow.mode_focus = Some(crate::steps::ModeFocus { port, mode, scroll: false });
    }
    mode_picture(app, &mut picture_ui, &reply);
}

/// The picked port mode's pattern: its transverse E across the port's face.
fn mode_picture(app: &mut EmcraftApp, ui: &mut egui::Ui, reply: &Value) {
    let Some(f) = app.workflow.mode_focus.clone() else { return };
    let fu = app.session.display.frequency;
    let Some(frequency) = reply["frequency"].as_f64().map(|v| fu.to_si(v)) else { return };
    let width = picture_width(ui);
    let key = format!("{}|{}|{frequency}|{}|{width}|{}", f.port, f.mode, app.revision, crate::theme::is_dark());
    if app.mode_picture.as_ref().is_none_or(|p| p.key != key) {
        match view::port_mode_plot(&app.session, &f.port, f.mode, frequency, 2) {
            Ok(plot) => app.mode_picture = Some(Picture::new(key, &emcraft_render::plot::draw(&plot, width))),
            Err(e) => {
                app.mode_picture = None;
                empty(ui, &e.to_string());
                return;
            }
        }
    }
    if let Some(p) = app.mode_picture.as_mut() {
        picture(ui, p, "picture:port_mode");
    }
}

pub(super) fn num(v: &Value, digits: usize) -> String {
    v.as_f64().map_or("–".into(), |x| format!("{x:.digits$}"))
}

pub(super) fn eigenmodes(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    if app.session.results.eigen.is_none() {
        empty(ui, "No eigenmodes yet: SIMULATE › Eigenmodes.");
        return;
    }
    if app.eigen_table.as_ref().is_none_or(|(rev, _)| *rev != app.revision) {
        match app.session.execute("results.eigenmodes", &json!({})) {
            Ok(v) => app.eigen_table = Some((app.revision, v)),
            Err(e) => {
                empty(ui, &e.to_string());
                return;
            }
        }
    }
    let Some((_, table)) = app.eigen_table.clone() else { return };
    let fu = app.session.display.frequency.symbol();
    let mut open = None;
    egui::Grid::new("eigen_grid").striped(true).num_columns(4).show(ui, |ui| {
        for h in ["Mode", &format!("f ({fu})"), "Q", ""] {
            ui.label(egui::RichText::new(h).strong());
        }
        ui.end_row();
        for (i, m) in table.get("modes").and_then(Value::as_array).into_iter().flatten().enumerate() {
            ui.label(format!("{}", i + 1));
            ui.label(num(&m["frequency"], 5));
            ui.label(num(&m["q"], 0));
            let r = ui.button("Field");
            scenario::publish_target(&format!("eigen:field:{}", i + 1), r.rect);
            if r.clicked() {
                open = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(exact) = table.get("exact").and_then(Value::as_array) {
        crate::ui_kit::section(ui, "Closed form");
        let list: Vec<String> = exact.iter().take(8).map(|e| format!("{} {}", e["mode"].as_str().unwrap_or(""), num(&e["frequency"], 4))).collect();
        ui.label(egui::RichText::new(list.join(" · ")).size(t.font_body).color(t.text_dim));
    }
    if let Some(i) = open {
        app.field.view.mode = Some(i);
        app.view = View::Field;
    }
}

pub(super) fn compare(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let Some(reply) = app.compare.clone() else {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(egui::RichText::new("Compare the sweep with the exact TE10 solution or mode matching.").size(t.font_heading).color(t.text_dim));
            let r = ui.add_enabled(!app.busy() && app.session.results.sweep.is_some(), egui::Button::new("Compare…"));
            scenario::publish_target("compare:run", r.rect);
            if r.clicked() {
                let _ = app.run("results.compare", json!({}));
            }
        });
        return;
    };
    ui.label(
        egui::RichText::new(format!("Against {} — worst |S − S_ref| = {}", reply["reference"].as_str().unwrap_or("?"), num(&reply["worst"], 6)))
            .size(t.font_heading),
    );
    egui::ScrollArea::vertical().id_salt("compare_table").show(ui, |ui| {
        egui::Grid::new("compare_grid").striped(true).num_columns(5).show(ui, |ui| {
            for h in ["f", "|S11|", "|S11| ref", "|S21|", "|S21| ref", "error S11", "error S21"] {
                ui.label(egui::RichText::new(h).strong());
            }
            ui.end_row();
            for r in reply.get("points").and_then(Value::as_array).into_iter().flatten() {
                ui.label(num(&r["frequency"], 4));
                ui.label(num(&r["s11"]["mag"], 5));
                ui.label(num(&r["exact_s11"]["mag"], 5));
                ui.label(num(&r["s21"]["mag"], 5));
                ui.label(num(&r["exact_s21"]["mag"], 5));
                ui.label(format!("{:.2e}", r["error_s11"].as_f64().unwrap_or(f64::NAN)));
                ui.label(format!("{:.2e}", r["error_s21"].as_f64().unwrap_or(f64::NAN)));
                ui.end_row();
            }
        });
    });
}

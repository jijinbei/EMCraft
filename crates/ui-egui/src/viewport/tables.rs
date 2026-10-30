//! The port-mode, eigenmode and comparison tables.

use super::*;
use crate::lang::args;

/// Below this width the port-mode table and the pattern are stacked: the table's columns and
/// a pattern of [`NARROW`]'s half side by side.
const MODES_NARROW: f32 = 900.0;

pub(super) fn port_modes(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let Some(reply) = app.replies.port_modes.as_ref() else {
        let text = "No port modes yet: they need a meshed model with ports.";
        return empty_with_action(ui, text, "Solve port modes…", !app.busy(), Fixed::PortModesSolve, Action::dialog("solve.port_modes"), out);
    };
    let lu = reply.get("length_unit").and_then(Value::as_str).unwrap_or("mm").to_owned();
    let fu = app.session.display().frequency.symbol();
    ui.label(egui::RichText::new(trf("Port modes at {f} {unit}", args!(f = num(&reply["frequency"], 4), unit = fu))).size(t.font_heading));
    // The mode picked (from the Excitation panel or a click here), else the first port's first;
    // the table scrolls to it once.
    let focus = app.workflow.mode_focus.clone().or_else(|| {
        let name = reply.pointer("/ports/0/name").and_then(Value::as_str)?;
        Some(crate::steps::ModeFocus { port: name.to_owned(), mode: 0, scroll: false })
    });
    let scroll = focus.as_ref().is_some_and(|f| f.scroll);
    if let Some(f) = focus.as_ref().filter(|f| f.scroll || app.workflow.mode_focus.is_none()) {
        out.push(Action::FocusMode(crate::steps::ModeFocus { scroll: false, ..f.clone() }));
    }
    // The table on the left, the picked mode's picture on the right; in a narrow card, the
    // picture under the table, in the card's scroll.
    let area = ui.available_rect_before_wrap();
    let narrow = area.width() < MODES_NARROW;
    let (mut table_ui, mut picture_ui);
    if narrow {
        table_ui = ui.new_child(egui::UiBuilder::new().max_rect(area));
        picture_ui = None;
    } else {
        ui.allocate_rect(area, Sense::hover());
        let split = area.left() + (area.width() * 0.42).round();
        table_ui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(area.min, pos2(split - 8.0, area.max.y))));
        picture_ui = Some(ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(pos2(split + 8.0, area.min.y), area.max))));
    }
    let mut picked = None;
    let tall = if narrow { (area.height() * 0.5).max(160.0) } else { area.height() };
    let outer = ui;
    let ui = &mut table_ui;
    egui::ScrollArea::both().id_salt("modes_table").max_height(tall).show(ui, |ui| {
        for port in reply.get("ports").and_then(Value::as_array).into_iter().flatten() {
            crate::ui_kit::section(ui, trf("Port {name}", args!(name = port["name"].as_str().unwrap_or(""))));
            egui::Grid::new(("modes", port["name"].as_str().unwrap_or(""))).striped(true).num_columns(6).show(ui, |ui| {
                super::dim_if_stale(ui);
                let attenuation = trf("Attenuation (dB/{unit})", args!(unit = lu));
                for h in [tr("Mode"), tr("Propagates"), &format!("β (rad/{lu})"), "εeff", "Z (Ω)", &attenuation] {
                    ui.text(Style::Strong, Raw(h));
                }
                ui.end_row();
                let pname = port["name"].as_str().unwrap_or("");
                for (k, m) in port.get("modes").and_then(Value::as_array).into_iter().flatten().enumerate() {
                    // The mode picked in the Excitation panel: marked, and scrolled to once.
                    let focused = focus.as_ref().is_some_and(|f| f.port == pname && f.mode == k);
                    let back = ui.painter().add(egui::Shape::Noop);
                    let cells = [
                        ui.label(m["name"].as_str().unwrap_or("?")),
                        ui.label(tr(if m["propagating"] == json!(true) { "yes" } else { "no" })),
                        ui.label(num(&m["beta"], 5)),
                        ui.label(num(&m["eps_eff"], 5)),
                        ui.label(num(&m["impedance"], 3)),
                        ui.label(num(&m["attenuation_db"], 3)),
                    ];
                    ui.end_row();
                    let row = cells.iter().fold(cells[0].rect, |r, c| r.union(c.rect)).expand2(egui::vec2(4.0, 1.0));
                    let click = ui.hit(row, Handle::PortModesRow(pname, k + 1), Sense::click());
                    if click.hint("Show this mode's pattern").clicked() {
                        picked = Some((pname.to_owned(), k));
                    }
                    if focused {
                        let r = row;
                        ui.painter().set(back, egui::Shape::rect_filled(r, 3.0, t.accent_soft));
                        scenario::publish_target(Fixed::PortModesFocus, r);
                        if scroll {
                            ui.scroll_to_rect(r, Some(egui::Align::Center));
                        }
                    }
                }
            });
        }
    });
    scenario::publish_rect(Fixed::TablePortModes, ui.min_rect());
    if let Some((port, mode)) = picked {
        out.push(Action::FocusMode(crate::steps::ModeFocus { port, mode, scroll: false }));
    }
    if narrow {
        // The table took its place in the card's flow.
        outer.allocate_rect(table_ui.min_rect(), Sense::hover());
        outer.add_space(8.0);
    }
    if let Some(f) = focus {
        mode_picture(app, &mut st.views.mode_picture, &f, picture_ui.as_mut().unwrap_or(outer), reply);
    }
}

/// The picked port mode's pattern: its transverse E across the port's face.
fn mode_picture(
    app: &EmcraftApp,
    slot: &mut Cached<PictureKey<crate::state::ModePicture>>,
    f: &crate::steps::ModeFocus,
    ui: &mut egui::Ui,
    reply: &Value,
) {
    let fu = app.session.display().frequency;
    let Some(frequency) = reply["frequency"].as_f64().map(|v| fu.to_si(v)) else { return };
    let width = picture_width(ui);
    let key = PictureKey::new(app.revision, width, crate::state::ModePicture { port: f.port.clone(), mode: f.mode, frequency });
    let session = &app.session;
    let drawn = slot.get_or_draw(key, || {
        view::port_mode_plot(session, &f.port, f.mode, frequency, 2).map(|plot| emcraft_render::plot::draw(&plot, width, &crate::theme::palette()))
    });
    match drawn {
        Ok(p) => {
            picture(ui, p, Fixed::PicturePortMode);
        }
        Err(e) => empty(ui, &e),
    }
}

pub(super) fn num(v: &Value, digits: usize) -> String {
    v.as_f64().map_or("–".into(), |x| format!("{x:.digits$}"))
}

pub(super) fn eigenmodes(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let table = match st.views.eigen_table.as_ref().filter(|e| e.revision == app.revision).map(|e| &e.reply) {
        Some(Ok(table)) => table,
        Some(Err(e)) => return empty(ui, e),
        None => return,
    };
    let fu = app.session.display().frequency.symbol();
    let mut open = None;
    egui::Grid::new("eigen_grid").striped(true).num_columns(4).show(ui, |ui| {
        super::dim_if_stale(ui);
        for h in [tr("Mode"), &format!("f ({fu})"), "Q", ""] {
            ui.text(Style::Strong, Raw(h));
        }
        ui.end_row();
        for (i, m) in table.get("modes").and_then(Value::as_array).into_iter().flatten().enumerate() {
            ui.label(format!("{}", i + 1));
            ui.label(num(&m["frequency"], 5));
            ui.label(num(&m["q"], 0));
            if ui.named(egui::Button::new(tr("Field")), Handle::EigenField(i + 1)).clicked() {
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
        st.field.view.mode = Some(i);
        out.push(Action::SetView(View::Field));
    }
}

pub(super) fn compare(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let Some(reply) = app.replies.compare.as_ref() else {
        let ready = !app.busy() && app.session.results().sweep.is_some();
        let text = "Compare the sweep with the exact TE10 solution or mode matching.";
        return empty_with_action(ui, text, "Compare…", ready, Fixed::CompareRun, Action::run("results.compare", json!({})), out);
    };
    ui.label(
        egui::RichText::new(trf(
            "Against {reference} — worst |S − S_ref| = {worst}",
            args!(reference = reply["reference"].as_str().unwrap_or("?"), worst = num(&reply["worst"], 6)),
        ))
        .size(t.font_heading),
    );
    egui::ScrollArea::vertical().id_salt("compare_table").show(ui, |ui| {
        egui::Grid::new("compare_grid").striped(true).num_columns(5).show(ui, |ui| {
            super::dim_if_stale(ui);
            for h in ["f", "|S11|", tr("|S11| ref"), "|S21|", tr("|S21| ref"), tr("error S11"), tr("error S21")] {
                ui.text(Style::Strong, Raw(h));
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

//! The port-mode, eigenmode and comparison tables.

use super::*;
use crate::lang::args;

/// Below this width the port-mode table and the pattern are stacked: the table's columns and
/// a pattern of [`NARROW`]'s half side by side.
const MODES_NARROW: f32 = 900.0;

pub(super) fn port_modes(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let Some(reply) = app.replies.port_modes.as_ref() else {
        let text = t!("No port modes yet: they need a meshed model with ports.");
        return empty_with_action(ui, text, |ui| {
            ui.act(true, egui::Button::new(tr(t!("Solve port modes…"))), Fixed::PortModesSolve.into(), Action::dialog("solve.port_modes"), out)
        });
    };
    let lu = reply.length_unit.as_str();
    let fu = app.session.display().frequency.symbol();
    ui.label(egui::RichText::new(trf(t!("Port modes at {f} {unit}"), args!(f = number(Some(reply.frequency), 4), unit = fu))).size(t.font_heading));
    // The mode picked (from the Excitation panel or a click here), else the first port's first;
    // the table scrolls to it once.
    let focus = app.workflow.mode_focus.clone().or_else(|| {
        let name = &reply.ports.first()?.name;
        Some(crate::steps::ModeFocus { port: name.clone(), mode: 0, scroll: false })
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
        for port in &reply.ports {
            let pname = port.name.as_str();
            crate::ui_kit::section(ui, trf(t!("Port {name}"), args!(name = pname)));
            egui::Grid::new(("modes", pname)).striped(true).num_columns(6).show(ui, |ui| {
                super::dim_if_stale(ui);
                let attenuation = trf(t!("Attenuation (dB/{unit})"), args!(unit = lu));
                for h in [tr(t!("Mode")), tr(t!("Propagates")), &format!("β (rad/{lu})"), "εeff", "Z (Ω)", &attenuation] {
                    ui.text(Style::Strong, Raw(h));
                }
                ui.end_row();
                for (k, m) in port.modes.iter().enumerate() {
                    // The mode picked in the Excitation panel: marked, and scrolled to once.
                    let focused = focus.as_ref().is_some_and(|f| f.port == pname && f.mode == k);
                    let back = ui.painter().add(egui::Shape::Noop);
                    let cells = [
                        ui.label(m.name.as_str()),
                        ui.label(tr(if m.propagating { t!("yes") } else { t!("no") })),
                        ui.label(number(m.beta, 5)),
                        ui.label(number(m.eps_eff, 5)),
                        ui.label(number(m.impedance, 3)),
                        ui.label(number(m.attenuation_db, 3)),
                    ];
                    ui.end_row();
                    let row = cells.iter().fold(cells[0].rect, |r, c| r.union(c.rect)).expand2(egui::vec2(4.0, 1.0));
                    let click = ui.hit(row, Handle::PortModesRow(pname, k + 1), Sense::click());
                    if click.hint(t!("Show this mode's pattern")).clicked() {
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
    reply: &emcraft_engine::PortModes,
) {
    let frequency = app.session.display().frequency.to_si(reply.frequency);
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
    number(v.as_f64(), digits)
}

/// `v` to `digits` places, a dash without one.
pub(super) fn number(v: Option<f64>, digits: usize) -> String {
    v.map_or_else(|| "–".into(), |x| format!("{x:.digits$}"))
}

pub(super) fn eigenmodes(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let Some(cached) = st.views.eigen_table.as_ref().filter(|e| e.revision == app.revision) else { return };
    let table = match &cached.reply {
        Ok(table) => table,
        Err(e) => return empty(ui, e),
    };
    let cavity = &cached.cavity;
    // The cavity's columns when a mode reads as one: T, R/Q and the shunt impedance on the
    // region's axis (`results.cavity`).
    let accelerating = cavity.iter().flatten().any(|c| c["voltage"].as_f64().is_some_and(|v| v > 0.0));
    let fu = app.session.display().frequency.symbol();
    let mut open = None;
    let columns = if accelerating { 7 } else { 4 };
    egui::Grid::new("eigen_grid").striped(true).num_columns(columns).show(ui, |ui| {
        super::dim_if_stale(ui);
        for h in [tr(t!("Mode")), &format!("f ({fu})"), "Q"] {
            ui.text(Style::Strong, Raw(h));
        }
        if accelerating {
            for h in ["$T$", "$R/Q$ (Ω)", "$R_{sh}$ (MΩ)"] {
                ui.label(crate::math::job(h, t.font_body, t.text));
            }
        }
        ui.label("");
        ui.end_row();
        for (i, m) in table.get("modes").and_then(Value::as_array).into_iter().flatten().enumerate() {
            ui.label(format!("{}", i + 1));
            ui.label(num(&m["frequency"], 5));
            ui.label(num(&m["q"], 0));
            if accelerating {
                let c = cavity.get(i).and_then(Option::as_ref);
                let cell = |key: &str, digits: usize, scale: f64| {
                    c.and_then(|c| c[key].as_f64()).map_or_else(|| "—".to_owned(), |v| format!("{:.digits$}", v * scale))
                };
                ui.label(cell("transit", 4, 1.0));
                ui.label(cell("r_over_q", 2, 1.0));
                ui.label(cell("shunt", 4, 1e-6));
            }
            if ui.named(egui::Button::new(tr(t!("Field"))), Handle::EigenField(i + 1)).clicked() {
                open = Some(i);
            }
            ui.end_row();
        }
    });
    if accelerating {
        ui.label(
            egui::RichText::new(tr(t!("On the region's axis along z, for a particle at the speed of light (results.cavity)")))
                .size(t.font_small)
                .color(t.text_dim),
        );
    }
    if let Some(exact) = table.get("exact").and_then(Value::as_array) {
        crate::ui_kit::section(ui, t!("Closed form"));
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
        let ready = app.session.results().sweep.is_some();
        let text = t!("Compare the sweep with the exact TE10 solution or mode matching.");
        return empty_with_action(ui, text, |ui| {
            ui.act(ready, egui::Button::new(tr(t!("Compare…"))), Fixed::CompareRun.into(), Action::run("results.compare", json!({})), out)
        });
    };
    ui.label(
        egui::RichText::new(trf(
            t!("Against {reference} — worst |S − S_ref| = {worst}"),
            args!(reference = reply.reference, worst = format!("{:.6}", reply.worst)),
        ))
        .size(t.font_heading),
    );
    egui::ScrollArea::vertical().id_salt("compare_table").show(ui, |ui| {
        egui::Grid::new("compare_grid").striped(true).num_columns(5).show(ui, |ui| {
            super::dim_if_stale(ui);
            for h in ["f", "|S11|", tr(t!("|S11| ref")), "|S21|", tr(t!("|S21| ref")), tr(t!("error S11")), tr(t!("error S21"))] {
                ui.text(Style::Strong, Raw(h));
            }
            ui.end_row();
            for r in &reply.points {
                for (x, digits) in [(r.frequency, 4), (r.s11.mag, 5), (r.exact_s11.mag, 5), (r.s21.mag, 5), (r.exact_s21.mag, 5)] {
                    ui.label(format!("{x:.digits$}"));
                }
                ui.label(format!("{:.2e}", r.error_s11));
                ui.label(format!("{:.2e}", r.error_s21));
                ui.end_row();
            }
        });
    });
}

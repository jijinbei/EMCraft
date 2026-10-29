//! The active step's panel: its checks, its settings and its commands. Each step's own part
//! is in its module: [`intent`] (the band), [`model`] (bodies, materials, boundaries, the
//! mesh), [`excitation`] (ports and their modes), [`setup`] (the solver, its settings and
//! device), [`run`] (the preflight) and [`results`] (checks and export).

mod excitation;
mod intent;
mod model;
mod results;
mod run;
mod setup;

pub use intent::apply_band;

use super::bar::mark;
use super::*;

/// The active step's panel at the right (with the open dialog docked in it).
pub fn panel(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let Some(step) = app.workflow.step else { return };
    let t = Tokens::get();
    let states = statuses(app);
    let Some(state) = states.get(step.index()).cloned() else { return };
    egui::Panel::right("emcraft_step_panel")
        .exact_size(t.form_card_width + 40.0)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(egui::Margin { left: 0, right: 6, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            let h = ui.available_height();
            t.card(t.panel).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(h - 22.0);
                scenario::publish_rect("step_panel", ui.max_rect());
                header(ui, &state, &t);
                let dialog_h = if app.dialog.is_some() { (h * 0.58).max(260.0) } else { 0.0 };
                egui::ScrollArea::vertical()
                    .id_salt("step_panel_scroll")
                    .max_height((h - dialog_h - 70.0).max(120.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        checks(app, ui, &state, &t);
                        body(app, ui, step, &states, &t);
                    });
                if app.dialog.is_some() {
                    ui.add_space(6.0);
                    t.card(t.dialog_body).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        crate::dialog_ui::embedded(app, ui, dialog_h - 10.0);
                    });
                }
            });
        });
}

fn header(ui: &mut egui::Ui, s: &StepState, t: &Tokens) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        mark(ui.painter(), r.center(), s.status, t);
        ui.label(egui::RichText::new(format!("Step {} · {}", s.step.index(), s.step.label())).size(t.font_heading).strong().color(t.text));
        ui.label(egui::RichText::new(s.status.id()).size(t.font_small).color(status_color(s.status, t)));
    });
    ui.add_space(4.0);
}

/// The step's checks, each with its fix.
fn checks(app: &mut EmcraftApp, ui: &mut egui::Ui, s: &StepState, t: &Tokens) {
    if s.messages.is_empty() {
        ui_kit::small(ui, "Nothing to fix here.", t.success);
        ui.add_space(6.0);
        return;
    }
    let mut chosen = None;
    for (i, m) in s.messages.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&m.text).size(t.font_small).color(status_color(s.status, t)));
            // A fix that only leads back to this step is left out.
            let here = m.command.as_deref().is_some_and(|c| c.starts_with("solve.") && Step::of_command(c) == Some(s.step));
            if let Some(c) = m.command.as_ref().filter(|_| !here) {
                let r = ui.small_button(fix_label(c)).on_hover_text(format!("{c}: {}", crate::label(c)));
                scenario::publish_target(format_args!("step:fix:{i}"), r.rect);
                if r.clicked() {
                    chosen = Some(c.clone());
                }
            }
        });
    }
    ui.add_space(6.0);
    if let Some(c) = chosen {
        fix(app, &c);
    }
}

fn fix_label(command: &str) -> String {
    match Step::of_command(command) {
        Some(step) if command.starts_with("solve.") && command != "solve.port_modes" => format!("Go to {}", step.label()),
        Some(Step::Intent) if command == "project.frequency" => "Set the band".into(),
        _ => format!("Fix: {}", crate::commands::button_label(command)),
    }
}

/// A one-line text field with its error under it; `true` when Enter was pressed in it.
pub(super) fn field(ui: &mut egui::Ui, label: &str, text: &mut String, unit: &str, error: Option<&str>, handle: &str) -> bool {
    let t = Tokens::get();
    let mut enter = false;
    ui_kit::row(ui, label, false, |ui| {
        let mut edit = egui::TextEdit::singleline(text).desired_width((ui.available_width() - 44.0).max(60.0)).char_limit(64);
        if error.is_some() {
            edit = edit.background_color(t.form_error_fill).text_color(t.form_error);
        }
        let r = ui.add(edit);
        scenario::publish_target(handle, r.rect);
        enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if !unit.is_empty() {
            ui_kit::small(ui, unit, t.text_dim);
        }
    });
    if let Some(e) = error {
        ui.horizontal(|ui| {
            ui.add_space(t.form_label_width + 6.0);
            ui_kit::small(ui, e, t.form_error);
        });
    }
    enter
}

pub(super) fn kv(ui: &mut egui::Ui, k: &str, v: String, t: &Tokens) {
    ui.horizontal(|ui| {
        ui.add_sized(vec2(t.form_label_width, 16.0), egui::Label::new(egui::RichText::new(k).size(t.font_small).color(t.text_dim)));
        ui_kit::small(ui, v, t.text);
    });
}

pub(super) fn fmt(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// The step's own settings and information.
fn body(app: &mut EmcraftApp, ui: &mut egui::Ui, step: Step, states: &[StepState], t: &Tokens) {
    let fu = app.session.display.frequency.symbol();
    let lu = app.session.display.length.symbol();
    // The band fields follow the session's band when it changes.
    let b = band(app);
    if app.workflow.fields.band_seen != b {
        if let Some((a, z)) = b {
            app.workflow.fields.start = fmt(a);
            app.workflow.fields.stop = fmt(z);
        }
        app.workflow.fields.band_seen = b;
    }
    match step {
        Step::Intent => intent::intent(app, ui, fu, t),
        Step::Geometry => model::geometry(app, ui, lu, t),
        Step::Materials => model::materials(app, ui, t),
        Step::Excitation => excitation::excitation(app, ui, fu, t),
        Step::Boundaries => model::boundaries(app, ui, lu, t),
        Step::Mesh => model::mesh(app, ui, lu, t),
        Step::Setup => setup::setup(app, ui, fu, t),
        Step::Run => run::run_panel(app, ui, states, t),
        Step::Results => results::results(app, ui, states, t),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_read_their_ids_and_commands() {
        for s in Step::ALL {
            assert_eq!(s.id().parse::<Step>(), Ok(s));
        }
        assert_eq!(Step::of_command("geometry.solid"), Some(Step::Geometry));
        assert_eq!(Step::of_command("solve.port_modes"), Some(Step::Excitation));
        assert_eq!(overlay(Step::Mesh), Overlay::Mesh);
    }
}

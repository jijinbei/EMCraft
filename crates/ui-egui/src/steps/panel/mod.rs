//! The active step's panel: its checks, its settings and its commands. Step 0's are on the
//! project's Home view (`viewport::project`), the panel showing there only for a docked dialog.
//! Each other step's own part is in its module: [`model`] (bodies, materials, boundaries, the
//! mesh), [`excitation`] (ports and their modes), [`setup`] (the solver, its settings and
//! device), [`run`] (the preflight) and [`results`] (checks and export).

mod excitation;
mod model;
mod results;
mod run;
mod setup;

pub use results::export_touchstone;

use super::bar::mark;
use super::*;
use crate::Style;
use crate::lang::{Raw, args, shown, tr, trf};

/// The active step's panel at the right (with the open dialog docked in it): `fields` are its
/// text fields, `dialog` the open dialog.
pub fn panel(app: &EmcraftApp, fields: &mut Fields, dialog: Option<&mut dialogs::Dialog>, ui: &mut egui::Ui, out: &mut Actions) {
    let Some(step) = app.workflow.step else { return };
    let t = &Tokens::get();
    let states = statuses_now(app);
    let Some(state) = states.get(step.index()).cloned() else { return };
    // Home's settings are its view's; the panel holds only a dialog docked there.
    if step == Step::Intent && dialog.is_none() {
        return;
    }
    egui::Panel::right("emcraft_step_panel")
        .exact_size(t.form_card_width + 40.0)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(egui::Margin { left: 0, right: 6, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            let h = ui.available_height();
            t.card(t.panel).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(h - 22.0);
                scenario::publish_rect(Fixed::StepPanel, ui.max_rect());
                header(app, ui, &state, t);
                if let Some(why) = not_needed(app, step) {
                    ui_kit::small(ui, why, t.text_dim);
                    ui.add_space(4.0);
                }
                // A running job stays in view here, whichever step is open.
                if app.job.is_some() {
                    crate::messages::job_card(app, ui, out);
                    ui.add_space(6.0);
                }
                let dialog_h = if dialog.is_some() { (h * 0.58).max(260.0) } else { 0.0 };
                egui::ScrollArea::vertical()
                    .id_salt("step_panel_scroll")
                    .max_height((h - dialog_h - 70.0).max(120.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        // Run's checks are about starting one, not the one going on.
                        if !(step == Step::Run && running(app)) {
                            checks(ui, &state, t, out);
                        }
                        body(app, fields, ui, step, &states, t, out);
                    });
                if let Some(d) = dialog {
                    ui.add_space(6.0);
                    t.card(t.dialog_body).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        crate::dialog_ui::embedded(app, d, ui, dialog_h - 10.0, out);
                    });
                }
            });
        });
}

/// Why step `step` does nothing for the job's kind of analysis, when it does nothing for it.
pub fn not_needed(app: &EmcraftApp, step: Step) -> Option<String> {
    let chosen = app.session.model().analysis?;
    emcraft_engine::workflow::not_needed(Some(chosen))
        .contains(&step)
        .then(|| trf("Not needed for {kind}", args!(kind = tr(crate::kinds::kind_card(chosen).name))))
}

fn header(app: &EmcraftApp, ui: &mut egui::Ui, s: &StepState, t: &Tokens) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        let run_going = s.step == Step::Run && running(app);
        if run_going {
            super::bar::running_mark(ui, r.center(), t);
        } else {
            mark(ui.painter(), r.center(), s.status, t);
        }
        ui.text(Style::Heading, trf("Step {n} · {step}", args!(n = s.step.index(), step = shown(s.step))));
        if run_going {
            ui_kit::small(ui, "running", t.accent);
        } else {
            ui_kit::small(ui, Raw(shown(s.status)), status_color(s.status, t));
        }
    });
    ui.add_space(4.0);
}

/// The step's checks, each with its fix.
fn checks(ui: &mut egui::Ui, s: &StepState, t: &Tokens, out: &mut Actions) {
    if s.messages.is_empty() {
        if s.status == Status::Done {
            ui_kit::small(ui, "Nothing to fix here.", t.success);
            ui.add_space(6.0);
        }
        return;
    }
    for (i, m) in s.messages.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&m.text).size(t.font_small).color(if s.status == Status::Done {
                t.text_dim
            } else {
                status_color(s.status, t)
            }));
            // A fix that only leads back to this step is left out.
            let here = m.command.as_deref().is_some_and(|c| c.starts_with("solve.") && Step::of_command(c) == Some(s.step));
            if let Some(c) = m.command.as_ref().filter(|_| !here) {
                let hint = format!("{c}: {}", crate::label(c));
                ui.act(true, egui::Button::new(fix_label(c)).small(), Handle::StepFix(i), &hint, || Action::Fix(c.clone()), out);
            }
        });
    }
    ui.add_space(6.0);
}

fn fix_label(command: &str) -> String {
    match Step::of_command(command) {
        Some(step) if command.starts_with("solve.") && command != "solve.port_modes" => trf("Go to {step}", args!(step = shown(step))),
        Some(Step::Intent) if command == "project.frequency" => tr("Set the band").to_owned(),
        Some(Step::Intent) if command == "project.analysis" => tr("Choose the analysis").to_owned(),
        _ => trf("Fix: {command}", args!(command = crate::commands::button_label(command))),
    }
}

/// A one-line text field with its error under it; `true` when Enter was pressed in it.
pub(crate) fn field(ui: &mut egui::Ui, label: &str, text: &mut String, unit: &str, error: Option<&str>, handle: Fixed) -> bool {
    let t = &Tokens::get();
    let mut enter = false;
    ui_kit::row(ui, label, false, |ui| {
        let mut edit = egui::TextEdit::singleline(text).desired_width((ui.available_width() - 44.0).max(60.0)).char_limit(64);
        if error.is_some() {
            edit = edit.background_color(t.form_error_fill).text_color(t.form_error);
        }
        let r = ui.named(edit, handle.into());
        enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if !unit.is_empty() {
            ui_kit::small(ui, Raw(unit), t.text_dim);
        }
    });
    if let Some(e) = error {
        ui.horizontal(|ui| {
            ui.add_space(t.form_label_width + 6.0);
            ui_kit::small(ui, Raw(e), t.form_error);
        });
    }
    enter
}

pub(super) fn kv(ui: &mut egui::Ui, k: &str, v: String, t: &Tokens) {
    ui.horizontal(|ui| {
        ui.add_sized(vec2(t.form_label_width, 16.0), egui::Label::new(Style::SmallDim.rich(tr(k))));
        crate::math::label(ui, &v, t.font_small, t.text);
    });
}

pub(crate) fn fmt(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// The step's own settings and information.
fn body(app: &EmcraftApp, f: &mut Fields, ui: &mut egui::Ui, step: Step, states: &[StepState], t: &Tokens, out: &mut Actions) {
    let fu = app.session.display().frequency.symbol();
    let lu = app.session.display().length.symbol();
    follow_band(app, f);
    match step {
        // On the Home view.
        Step::Intent => {}
        Step::Geometry => model::geometry(app, ui, lu, t),
        Step::Materials => model::materials(app, ui, t),
        Step::Excitation => excitation::excitation(app, ui, fu, t, out),
        Step::Boundaries => model::boundaries(app, ui, lu, t),
        Step::Mesh => model::mesh(app, ui, lu, t),
        Step::Setup => setup::setup(app, f, ui, fu, t, out),
        Step::Run => run::run_panel(app, f, ui, states, t, out),
        Step::Results => results::results(app, f, ui, states, t, out),
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

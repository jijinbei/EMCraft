//! The Run step: the preflight's findings, the button that starts the solve, and why it was refused
//! or failed.

use super::*;
use crate::lang::{args, plural};

/// An error band with a heading and its details.
fn refusal_band(ui: &mut egui::Ui, heading: &str, details: impl FnOnce(&mut egui::Ui)) {
    let t = &Tokens::get();
    ui.add_space(4.0);
    let r = egui::Frame::NONE.fill(t.form_error_fill).corner_radius(t.radius_sm).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new(heading).size(t.font_body).strong().color(t.form_error));
        details(ui);
    });
    scenario::publish_rect(Fixed::RunError, r.response.rect);
}

/// A finding's way to its step; `true` when taken.
fn go_to(ui: &mut egui::Ui, step: Step, handle: Handle, out: &mut Actions) -> bool {
    let label = trf(t!("Go to {step}"), args!(step = shown(step)));
    ui.act(true, egui::Button::new(label).small(), handle, Action::OpenStep(Some(step)), out).clicked()
}

/// Run: the preflight's findings and the button that starts the solve.
pub(super) fn run_panel(app: &EmcraftApp, ui: &mut egui::Ui, states: &[StepState], out: &mut Actions) {
    let t = &Tokens::get();
    // While a run goes on, its progress and Cancel over the panel say all there is.
    if running(app) {
        return;
    }
    ui_kit::section(ui, t!("Preflight"));
    let pre = preflight(&app.session, states);
    if pre.blocking.is_empty() && pre.warnings.is_empty() {
        ui_kit::small(ui, t!("Every step is ready."), t.success);
    }
    let mut went = false;
    for (list, color, what) in [(&pre.blocking, t.error, "blocks"), (&pre.warnings, t.warning, "warning")] {
        for (i, (step, text)) in list.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui_kit::small(ui, format!("{} · {text}", shown(*step)), color);
                went |= go_to(ui, *step, Handle::Preflight(what, i), out);
            });
        }
    }
    if went {
        return;
    }
    let solver = app.session.model().analysis();
    ui.add_space(6.0);
    ui_kit::small(ui, trf(t!("Solver: {solver}"), args!(solver = shown(solver))), t.text);
    let (nb, nw) = (pre.blocking.len(), pre.warnings.len());
    let label = match (nb, nw) {
        (0, 0) => tr(t!("Run")).to_owned(),
        (0, nw) => plural(nw, t!("Run, accepting {n} warning"), t!("Run, accepting {n} warnings"), args!(n = nw)),
        (nb, _) => plural(nb, t!("Run — {n} problem to fix first"), t!("Run — {n} problems to fix first"), args!(n = nb)),
    };
    ui.act(true, ui_kit::glyph_button(crate::glyphs::Glyph::Play, &label, true), Fixed::RunStart.into(), Action::StartRun, out);
    // Why Run was refused, under the button: each blocking problem with the way to its step,
    // as long as it blocks.
    let mut went = false;
    if app.workflow.refusal.is_some() && !pre.blocking.is_empty() {
        let heading = plural(nb, t!("Run refused: {n} problem to fix first"), t!("Run refused: {n} problems to fix first"), args!(n = nb));
        refusal_band(ui, &heading, |ui| {
            for (i, (step, text)) in pre.blocking.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui_kit::small(ui, format!("• {} · {text}", shown(*step)), t.form_error);
                    went |= go_to(ui, *step, Handle::Refused(i), out);
                });
            }
        });
    }
    // A run that started and failed says why here too (the reason Run's mark is red).
    if let Some(crate::Failure { command, error: e }) = app.failure.as_ref().filter(|_| run_failure(app).is_some()) {
        refusal_band(ui, &trf(t!("{command} failed"), args!(command = crate::label(command))), |ui| {
            ui_kit::small(ui, e, t.form_error);
        });
    }
    if went {
        return;
    }
    if let Some(e) = run_estimate(app) {
        super::super::estimate_line(ui, e, Fixed::RunEstimate);
    }
}

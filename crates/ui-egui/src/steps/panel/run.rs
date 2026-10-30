//! The Run step: the preflight's findings, the button that starts the solve, and why it was refused
//! or failed.

use super::*;
use crate::lang::{args, plural};

/// An error band with a heading and its details.
fn refusal_band(ui: &mut egui::Ui, heading: &str, t: &Tokens, details: impl FnOnce(&mut egui::Ui)) {
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
    let label = trf("Go to {step}", args!(step = shown(step)));
    ui.act(true, egui::Button::new(label).small(), handle, "", || Action::OpenStep(Some(step)), out).clicked()
}

/// Run: the preflight's findings and the button that starts the solve.
pub(super) fn run_panel(app: &EmcraftApp, f: &mut Fields, ui: &mut egui::Ui, states: &[StepState], t: &Tokens, out: &mut Actions) {
    // While a run goes on, its progress and Cancel over the panel say all there is.
    if running(app) {
        return;
    }
    ui_kit::section(ui, "Preflight");
    let pre = preflight(&app.session, f, states);
    if pre.blocking.is_empty() && pre.warnings.is_empty() {
        ui_kit::small(ui, "Every step is ready.", t.success);
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
    let solver = f.solver.unwrap_or_else(|| Solver::infer(app.session.model()));
    ui.add_space(6.0);
    ui_kit::small(ui, trf("Solver: {solver}", args!(solver = shown(solver))), t.text);
    let (nb, nw) = (pre.blocking.len(), pre.warnings.len());
    let label = if nb > 0 {
        plural(nb, "Run — {n} problem to fix first", "Run — {n} problems to fix first", args!(n = nb))
    } else if nw == 0 {
        tr("Run").to_owned()
    } else {
        plural(nw, "Run, accepting {n} warning", "Run, accepting {n} warnings", args!(n = nw))
    };
    let busy = app.busy();
    ui.act(!busy, ui_kit::styled(&label, true), Fixed::RunStart.into(), "", || Action::StartRun, out);
    // Why Run was refused, under the button: each blocking problem with the way to its step,
    // as long as it blocks.
    if f.refused.is_some() && pre.blocking.is_empty() {
        f.refused = None;
    }
    let mut went = false;
    if f.refused.is_some() {
        let heading = plural(nb, "Run refused: {n} problem to fix first", "Run refused: {n} problems to fix first", args!(n = nb));
        refusal_band(ui, &heading, t, |ui| {
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
        refusal_band(ui, &trf("{command} failed", args!(command = crate::label(command))), t, |ui| {
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

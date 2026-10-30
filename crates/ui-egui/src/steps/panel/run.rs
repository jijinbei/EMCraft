//! The Run step: the preflight's findings, the button that starts the solve, and why it was refused
//! or failed.

use super::*;
use crate::lang::args;

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
    let label = if !pre.blocking.is_empty() {
        trf("Run — {n} problem(s) to fix first", args!(n = pre.blocking.len()))
    } else if pre.warnings.is_empty() {
        tr("Run").to_owned()
    } else {
        trf("Run, accepting {n} warning(s)", args!(n = pre.warnings.len()))
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
        refusal_band(ui, &trf("Run refused: {n} problem(s) to fix first", args!(n = pre.blocking.len())), t, |ui| {
            for (i, (step, text)) in pre.blocking.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui_kit::small(ui, format!("• {} · {text}", shown(*step)), t.form_error);
                    went |= go_to(ui, *step, Handle::Refused(i), out);
                });
            }
        });
    }
    // A solve that started and failed says why here too.
    if !busy && let Some(crate::Failure { error: e, .. }) = app.failure.as_ref().filter(|f| f.command == solver.command()) {
        refusal_band(ui, &trf("{command} failed", args!(command = crate::label(solver.command()))), t, |ui| {
            ui_kit::small(ui, e, t.form_error);
        });
    }
    if went {
        return;
    }
    if busy {
        ui_kit::small(ui, "Running — progress and Cancel below.", t.text_dim);
    }
    if let Some(e) = estimate(app).ok().filter(|_| solver == Solver::Frequency) {
        let n = |k: &str| e.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let points = f.points.trim().parse::<f64>().unwrap_or(11.0);
        let (unknowns, mb, secs) =
            (format!("{:.0}", n("unknowns")), format!("{:.0}", n("memory_mb")), format!("{:.1}", n("seconds_per_frequency") * points));
        ui_kit::caption(
            ui,
            trf(
                "About {unknowns} unknowns, {mb} MB, {secs} s for {points} frequencies (fast sweep: less).",
                args!(unknowns = unknowns, mb = mb, secs = secs, points = points),
            ),
        );
    }
}

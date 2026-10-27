//! The Run step: the preflight's findings, the button that starts the solve, and why it was refused
//! or failed.

use super::*;

/// An error band with a heading and its details.
fn refusal_band(ui: &mut egui::Ui, heading: &str, t: &Tokens, details: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(4.0);
    let r = egui::Frame::NONE.fill(t.form_error_fill).corner_radius(t.radius_sm).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new(heading).size(t.font_body).strong().color(t.form_error));
        details(ui);
    });
    scenario::publish_rect("run:error", r.response.rect);
}

/// Run: the preflight's findings and the button that starts the solve.
pub(super) fn run_panel(app: &mut EmcraftApp, ui: &mut egui::Ui, states: &[StepState], t: &Tokens) {
    ui_kit::section(ui, "Preflight");
    let pre = preflight(app, states);
    if pre.blocking.is_empty() && pre.warnings.is_empty() {
        ui_kit::small(ui, "Every step is ready.", t.success);
    }
    let mut go = None;
    for (list, color, what) in [(&pre.blocking, t.error, "blocks"), (&pre.warnings, t.warning, "warning")] {
        for (i, (step, text)) in list.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui_kit::small(ui, format!("{} · {text}", step.label()), color);
                let r = ui.small_button(format!("Go to {}", step.label()));
                scenario::publish_target(&format!("preflight:{what}:{i}"), r.rect);
                if r.clicked() {
                    go = Some(*step);
                }
            });
        }
    }
    if let Some(s) = go {
        open_step(app, Some(s));
        return;
    }
    let solver = app.workflow.fields.solver.unwrap_or_else(|| Solver::infer(&app.session.model));
    ui.add_space(6.0);
    ui_kit::small(ui, format!("Solver: {}", solver.label()), t.text);
    let label = if !pre.blocking.is_empty() {
        format!("Run — {} problem(s) to fix first", pre.blocking.len())
    } else if pre.warnings.is_empty() {
        "Run".to_owned()
    } else {
        format!("Run, accepting {} warning(s)", pre.warnings.len())
    };
    let busy = app.busy();
    if ui_kit::button(ui, &label, true, !busy, "run:start").clicked() {
        let _ = start_run(app);
    }
    // Why Run was refused, under the button: each blocking problem with the way to its step,
    // as long as it blocks.
    if app.workflow.fields.refused.is_some() && pre.blocking.is_empty() {
        app.workflow.fields.refused = None;
    }
    let mut go = None;
    if app.workflow.fields.refused.is_some() {
        refusal_band(ui, &format!("Run refused: {} problem(s) to fix first", pre.blocking.len()), t, |ui| {
            for (i, (step, text)) in pre.blocking.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui_kit::small(ui, format!("• {} · {text}", step.label()), t.form_error);
                    let r = ui.small_button(format!("Go to {}", step.label()));
                    scenario::publish_target(&format!("run:refused:{i}"), r.rect);
                    if r.clicked() {
                        go = Some(*step);
                    }
                });
            }
        });
    }
    // A solve that started and failed says why here too.
    if !busy && let Some((_, e)) = app.failure.as_ref().filter(|f| f.0 == solver.command()) {
        refusal_band(ui, &format!("{} failed", crate::label(solver.command())), t, |ui| {
            ui_kit::small(ui, e, t.form_error);
        });
    }
    if let Some(s) = go {
        open_step(app, Some(s));
        return;
    }
    if busy {
        ui_kit::small(ui, "Running — progress and Cancel below.", t.text_dim);
    }
    if let Some(e) = estimate(app).ok().filter(|_| solver == Solver::Frequency) {
        let n = |k: &str| e.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let points = app.workflow.fields.points.trim().parse::<f64>().unwrap_or(11.0);
        ui_kit::caption(
            ui,
            &format!(
                "About {:.0} unknowns, {:.0} MB, {:.1} s for {points} frequencies (fast sweep: less).",
                n("unknowns"),
                n("memory_mb"),
                n("seconds_per_frequency") * points
            ),
        );
    }
}

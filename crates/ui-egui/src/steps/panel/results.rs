//! The Results step: passivity and reciprocity, and the Touchstone export.

use super::*;

pub(super) fn results(app: &EmcraftApp, f: &mut Fields, ui: &mut egui::Ui, states: &[StepState], t: &Tokens, out: &mut Actions) {
    let stale = states.get(Step::Results.index()).is_some_and(|s| s.status == Status::Stale);
    if stale {
        ui_kit::error_band(ui, "These results belong to an earlier state of the model.");
    }
    if !app.session.results().any() {
        ui.label(
            egui::RichText::new("No results yet: Run (step 7). The view tabs over the viewport show them.").size(t.font_small).color(t.text_dim),
        );
        return;
    }
    if app.session.results().sweep.is_none() {
        return;
    }
    ui_kit::section(ui, "Checks");
    let busy = app.busy();
    if ui_kit::button(ui, "Passivity and reciprocity", false, !busy, "results:check").clicked() {
        out.push(Action::CheckResults);
    }
    if let Some((_, c)) = &app.workflow.check {
        let n = |k: &str| c.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let passive = c.get("passive").and_then(Value::as_bool) == Some(true);
        kv(ui, "largest σ(S)", format!("{:.6} — {}", n("largest_singular_value"), if passive { "passive" } else { "NOT passive" }), t);
        kv(ui, "max |S − S^T|", format!("{:.3e}", n("reciprocity_error")), t);
    }
    ui_kit::section(ui, "Touchstone");
    let ports = app.session.sparameter_entries().len().max(1);
    if f.touchstone.is_empty() {
        f.touchstone = format!("emcraft.s{ports}p");
    }
    if f.format.is_empty() {
        f.format = "ri".into();
    }
    let ep = f.error("touchstone").map(str::to_owned);
    field(ui, "path", &mut f.touchstone, "", ep.as_deref(), "results:touchstone:path");
    ui.horizontal(|ui| {
        ui.add_space(t.form_label_width + 6.0);
        for (v, label) in [("ri", "real, imaginary"), ("ma", "magnitude, angle")] {
            let r = ui.radio_value(&mut f.format, v.to_owned(), egui::RichText::new(label).size(t.font_small));
            scenario::publish_target(format_args!("results:touchstone:{v}"), r.rect);
        }
    });
    if ui_kit::button(ui, "Export", true, !busy, "results:touchstone").clicked() {
        out.push(Action::ExportTouchstone);
    }
}

/// Writes the Touchstone file the Results step's fields name.
pub fn export_touchstone(app: &mut EmcraftApp) {
    let f = &mut app.workflow.fields;
    let (path, format) = (f.touchstone.trim().to_owned(), f.format.clone());
    if path.is_empty() {
        f.set_error("touchstone", Some("give a file".into()));
        return;
    }
    f.set_error("touchstone", None);
    if let Err(e) = app.run("results.touchstone", json!({"path": path, "format": format})) {
        app.workflow.fields.set_error("touchstone", Some(e.to_string()));
    }
}

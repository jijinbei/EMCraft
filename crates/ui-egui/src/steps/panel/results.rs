//! The Results step: passivity and reciprocity, and the Touchstone export.

use super::*;

pub(super) fn results(app: &EmcraftApp, f: &mut Fields, ui: &mut egui::Ui, states: &[StepState], out: &mut Actions) {
    let t = &Tokens::get();
    let stale = states.get(Step::Results.index()).is_some_and(|s| s.status == Status::Stale);
    if stale {
        crate::viewport::stale_band(ui, out);
    }
    if !app.session.results().any() {
        ui.text(crate::Style::SmallDim, t!("No results yet: Run (step 6). The view tabs over the viewport show them."));
        return;
    }
    if app.session.results().sweep.is_none() {
        return;
    }
    ui_kit::section(ui, t!("Checks"));
    ui.act(true, ui_kit::styled(t!("Passivity and reciprocity"), false), Fixed::ResultsCheck.into(), Action::CheckResults, out);
    if let Some((_, c)) = &app.workflow.check {
        let n = |k: &str| c.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let passive = c.get("passive").and_then(Value::as_bool) == Some(true);
        kv(ui, t!("largest σ(S)"), format!("{:.6} — {}", n("largest_singular_value"), tr(if passive { t!("passive") } else { t!("NOT passive") })));
        kv(ui, t!("max |S − S^T|"), format!("{:.3e}", n("reciprocity_error")));
    }
    ui_kit::section(ui, t!("Mesh accuracy"));
    ui_kit::caption(ui, t!("Physics checks, not the mesh's error: refine and compare."));
    ui.act(!stale, ui_kit::styled(t!("Set up mesh convergence"), false), Fixed::ResultsConvergence.into(), Action::dialog("mesh.adapt"), out);
    ui_kit::caption(ui, t!("Refines the mesh at one frequency; run again on the result."));
    ui_kit::section(ui, t!("Touchstone"));
    let ports = app.session.sparameter_entries().len().max(1);
    if f.touchstone.is_empty() {
        f.touchstone = format!("emcraft.s{ports}p");
    }
    if f.format.is_empty() {
        f.format = "ri".into();
    }
    let ep = f.error("touchstone").map(str::to_owned);
    field(ui, t!("path"), &mut f.touchstone, "", ep.as_deref(), Fixed::ResultsTouchstonePath);
    ui.horizontal(|ui| {
        ui.add_space(t.form_label_width + 6.0);
        for (v, label) in [("ri", t!("real, imaginary")), ("ma", t!("magnitude, angle"))] {
            let r = ui.radio_value(&mut f.format, v.to_owned(), egui::RichText::new(tr(label)).size(t.font_small));
            scenario::publish_target(Handle::Touchstone(v), r.rect);
        }
    });
    ui.act(true, ui_kit::styled(t!("Export"), true), Fixed::ResultsTouchstone.into(), Action::ExportTouchstone, out);
}

/// Writes the Touchstone file the Results step's fields name.
pub fn export_touchstone(app: &mut EmcraftApp) {
    let f = &mut app.workflow.fields;
    let (path, format) = (f.touchstone.trim().to_owned(), f.format.clone());
    if path.is_empty() {
        *f = std::mem::take(f).with_error("touchstone", Some(crate::lang::tr(t!("give a file")).into()));
        return;
    }
    *f = std::mem::take(f).with_error("touchstone", None);
    if let Err(e) = app.run("results.touchstone", json!({"path": path, "format": format})) {
        app.workflow.fields = std::mem::take(&mut app.workflow.fields).with_error("touchstone", Some(e.to_string()));
    }
}

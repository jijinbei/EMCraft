//! Step 0, Intent: the frequency band, and what kind of problem the model is.

use super::*;

pub(super) fn intent(app: &EmcraftApp, f: &mut Fields, ui: &mut egui::Ui, fu: &str, t: &Tokens, out: &mut Actions) {
    ui_kit::section(ui, "Frequency band");
    ui_kit::caption(ui, "Set once, early: it sizes the mesh (elements per wavelength at its top), the sweep and the port-mode preview.");
    let (e0, e1) = (f.error("start").map(str::to_owned), f.error("stop").map(str::to_owned));
    let enter = field(ui, "start", &mut f.start, fu, e0.as_deref(), Fixed::IntentStart)
        | field(ui, "stop", &mut f.stop, fu, e1.as_deref(), Fixed::IntentStop);
    let busy = app.busy();
    ui.act(!busy, ui_kit::styled("Apply", true), Fixed::IntentApply.into(), "", || Action::ApplyBand, out);
    if enter && !busy {
        out.push(Action::ApplyBand);
    }
    ui_kit::section(ui, "Problem");
    let kind = match Solver::infer(app.session.model()) {
        Solver::Eigenmodes => "a resonator (no excitation: its eigenmodes)",
        Solver::Radiation => "an antenna or a scatterer (an open problem)",
        _ => "a guided-wave component (S-parameters between ports)",
    };
    ui_kit::small(ui, kind, t.text);
    let d = app.session.display();
    kv(ui, "units", format!("{}, {}", d.length.symbol(), d.frequency.symbol()), t);
}

/// Checks the band's fields one by one and sets the band (`project.frequency`).
pub fn apply_band(app: &mut EmcraftApp) {
    let f = &mut app.workflow.fields;
    let (a, z) = (positive(&f.start), positive(&f.stop));
    f.set_error("start", a.as_ref().err().cloned());
    f.set_error("stop", z.as_ref().err().cloned());
    let (Ok(a), Ok(z)) = (a, z) else { return };
    if z < a {
        f.set_error("stop", Some(crate::lang::tr("must not be below the start").into()));
        return;
    }
    if let Err(e) = app.run("project.frequency", json!({"start": a, "stop": z})) {
        app.workflow.fields.set_error("stop", Some(e.to_string()));
    }
}

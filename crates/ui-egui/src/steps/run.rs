//! Starting a run: the solver's parameters from the panel's fields and the preflight checks.

use super::*;
use crate::lang::{args, plural};

/// The form's settings as `project.setup` takes them, for the job's kind; each bad field is
/// marked ([`emcraft_ui_core::steps::form::checked`]).
pub fn setup_params(s: &emcraft_engine::Session, f: &mut Fields) -> Result<Value, String> {
    let (params, checked) = emcraft_ui_core::steps::form::checked(s.model().analysis(), s.limits().frequencies as u64, std::mem::take(f));
    *f = checked;
    params
}

/// The Setup step's button: the form's settings confirmed (`project.setup`).
pub fn confirm_setup(app: &mut EmcraftApp) -> Result<Value, UiError> {
    app.workflow.fields.tried = true;
    let params = setup_params(&app.session, &mut app.workflow.fields).map_err(UiError::Input)?;
    app.run("project.setup", params)
}

pub use emcraft_ui_core::steps::run::Preflight;

/// What Run checks first ([`emcraft_ui_core::steps::run::preflight`]).
pub fn preflight(s: &emcraft_engine::Session, states: &[StepState]) -> Preflight {
    emcraft_ui_core::steps::run::preflight(s.model().analysis(), states)
}

/// Run: refuses on blocking problems; warnings are accepted by pressing Run.
pub fn start_run(app: &mut EmcraftApp) -> Result<Value, UiError> {
    if solver(app) == Solver::Pcb {
        app.place = app.place.show(View::Pcb);
        return app.run("pcb.inspect", json!({}));
    }

    super::check_form(app);
    let states = statuses_now(app);
    let pre = preflight(&app.session, &states[..]);
    if let Some(refusal) = pre.refusal() {
        let e = UiError::Refused(refusal);
        app.workflow.refusal = Some(e.to_string());
        app.error(&e);
        return Err(e);
    }
    app.workflow.refusal = None;
    let n = pre.warnings.len();
    if n > 0 {
        app.info(plural(n, t!("Running with {n} accepted warning."), t!("Running with {n} accepted warnings."), args!(n = n)));
    }
    app.run("solve.run", json!({}))
}

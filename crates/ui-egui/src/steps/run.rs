//! Starting a run: the solver's parameters from the panel's fields and the preflight checks.

use super::*;
use crate::lang::{args, plural, shown, tr};

/// The form's settings as `project.setup` takes them, for the job's kind; each bad field is
/// marked. The engine checks the rest when they are confirmed (a band's start above its stop).
pub fn setup_params(s: &emcraft_engine::Session, f: &mut Fields) -> Result<Value, String> {
    let solver = s.model().analysis();
    let limit = s.limits().frequencies as u64;
    f.fill_defaults();
    let mut bad = Vec::new();
    let mut take = |name: &'static str, r: Result<Value, String>, f: &mut Fields| match r {
        Ok(v) => {
            f.set_error(name, None);
            Some(v)
        }
        Err(e) => {
            bad.push(format!("{}: {e}", crate::lang::lookup(name)));
            f.set_error(name, Some(e));
            None
        }
    };
    let number = |text: &str| positive(text).map(|v| json!(v));
    let whole = |text: &str, hi: u64| integer(text, 1, hi).map(|v| json!(v));
    let mut p = json!({"kind": solver.id()});
    let mut put = |key: &str, v: Option<Value>| {
        if let Some(v) = v {
            p[key] = v;
        }
    };
    match solver {
        Solver::Frequency | Solver::TimeDomain => {
            put("start", take("start", number(&f.start.clone()), f));
            put("stop", take("stop", number(&f.stop.clone()), f));
            put("points", take("points", whole(&f.points.clone(), limit), f));
            if solver == Solver::Frequency {
                put("order", take("order", whole(&f.order.clone(), 2), f));
            } else {
                put("cell", take("cell", number(&f.cell.clone()), f));
                if !f.excite.is_empty() {
                    put("excite", Some(json!(f.excite)));
                }
            }
        }
        Solver::Eigenmodes => {
            put("frequency", take("frequency", number(&f.frequency.clone()), f));
            put("count", take("count", whole(&f.count.clone(), emcraft_model::MAX_MODES as u64), f));
            put("order", take("order", whole(&f.order.clone(), 2), f));
        }
        Solver::Radiation => {
            put("frequency", take("frequency", number(&f.frequency.clone()), f));
            put("order", take("order", whole(&f.order.clone(), 2), f));
        }
        Solver::Pcb => return Err(tr(t!("the PCB diagnosis has no run settings")).into()),
        Solver::System => {
            put("start", take("start", number(&f.start.clone()), f));
            put("stop", take("stop", number(&f.stop.clone()), f));
            put("points", take("points", whole(&f.points.clone(), limit), f));
            // The budget's frequency: the band's centre when left empty.
            if !f.frequency.trim().is_empty() {
                put("frequency", take("frequency", number(&f.frequency.clone()), f));
            }
        }
    }
    if let (Ok(a), Ok(z)) = (positive(&f.start), positive(&f.stop))
        && matches!(solver, Solver::Frequency | Solver::TimeDomain | Solver::System)
        && z < a
    {
        let e = tr(t!("must not be below the start")).to_owned();
        bad.push(format!("{}: {e}", tr(t!("stop"))));
        f.set_error("stop", Some(e));
    }
    if bad.is_empty() { Ok(p) } else { Err(bad.join("; ")) }
}

/// The Setup step's button: the form's settings confirmed (`project.setup`).
pub fn confirm_setup(app: &mut EmcraftApp) -> Result<Value, UiError> {
    app.workflow.fields.tried = true;
    let params = setup_params(&app.session, &mut app.workflow.fields).map_err(UiError::Input)?;
    app.run("project.setup", params)
}

/// The preflight: what blocks the run and what it may accept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preflight {
    pub blocking: Vec<(Step, String)>,
    pub warnings: Vec<(Step, String)>,
}

/// Whether a run of `solver` cannot go without step `step` done (the kinds' table,
/// [`emcraft_engine::workflow::required`]).
pub fn required(step: Step, solver: Solver) -> bool {
    emcraft_engine::workflow::required(Some(solver)).contains(&step)
}

/// What Run checks first: a step that needs attention, or one the solver cannot go without left
/// to do, blocks it; warnings and out-of-date results are accepted by pressing Run; other steps
/// left to do and steps waiting on another are not problems.
pub fn preflight(s: &emcraft_engine::Session, states: &[StepState]) -> Preflight {
    let mut out = Preflight::default();
    let solver = s.model().analysis();
    for s in states.iter().filter(|s| s.step.index() < Step::Run.index()) {
        let needed = required(s.step, solver);
        let list = match s.status {
            Status::Missing => &mut out.blocking,
            Status::Todo if needed => &mut out.blocking,
            Status::Warning | Status::Stale => &mut out.warnings,
            // Left to do but not needed (a resonator's excitation; the band, which the solver's
            // own parameters ask for when they need it), or waiting on a step listed already.
            Status::Todo | Status::Waiting | Status::Done => continue,
        };
        if s.messages.is_empty() {
            list.push((s.step, format!("{}: {}", shown(s.step), shown(s.status))));
        }
        for m in &s.messages {
            list.push((s.step, super::check_text(m)));
        }
    }
    out
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
    if !pre.blocking.is_empty() {
        let n = pre.blocking.len();
        let e = UiError::Refused(format!(
            "{n} {} {} it — {}",
            if n == 1 { "problem" } else { "problems" },
            if n == 1 { "blocks" } else { "block" },
            pre.blocking.iter().map(|(s, m)| format!("{} ({})", m, s.label())).collect::<Vec<_>>().join("; ")
        ));
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

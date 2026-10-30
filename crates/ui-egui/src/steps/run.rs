//! Starting a run: the solver's parameters from the panel's fields and the preflight checks.

use super::*;
use crate::lang::{args, plural, shown, tr};

/// The solver and its parameters from the setup's fields (`f`); each bad field is marked.
pub fn run_params(s: &emcraft_engine::Session, f: &mut Fields) -> Result<(Solver, Value), String> {
    let solver = f.solver.unwrap_or_else(|| Solver::infer(s.model()));
    let band = band(s);
    let limit = s.limits().frequencies as u64;
    if f.points.is_empty() {
        f.points = "11".into();
    }
    if f.order.is_empty() {
        f.order = "2".into();
    }
    if f.count.is_empty() {
        f.count = "5".into();
    }
    let order = integer(&f.order, 1, 2);
    let mut bad = Vec::new();
    let mut take = |name: &'static str, r: Result<Value, String>, f: &mut Fields| match r {
        Ok(v) => {
            f.set_error(name, None);
            Some(v)
        }
        Err(e) => {
            bad.push(format!("{name}: {e}"));
            f.set_error(name, Some(e));
            None
        }
    };
    // A frequency field left empty takes the band's middle.
    let freq = |text: &str| {
        if text.trim().is_empty() {
            band.map(|(a, z)| 0.5 * (a + z)).ok_or_else(|| tr("give a frequency (or set the band, step 0)").to_owned())
        } else {
            positive(text)
        }
    };
    let params = match solver {
        Solver::Frequency | Solver::TimeDomain => {
            let points = take("points", integer(&f.points.clone(), 1, limit).map(|v| json!(v)), f);
            let order = if solver == Solver::Frequency { take("order", order.map(|v| json!(v)), f) } else { Some(Value::Null) };
            if band.is_none() {
                bad.push(tr("no frequency band: set it in step 0 (Intent)").into());
            }
            match (points, order) {
                (Some(p), Some(o)) if solver == Solver::Frequency => json!({"points": p, "order": o}),
                (Some(p), Some(_)) => match band {
                    Some((a, z)) => json!({"start": a, "stop": z, "points": p}),
                    None => json!({"points": p}),
                },
                _ => Value::Null,
            }
        }
        Solver::Eigenmodes => {
            let count = take("count", integer(&f.count.clone(), 1, 50).map(|v| json!(v)), f);
            let near = take("frequency", freq(&f.frequency.clone()).map(|v| json!(v)), f);
            let order = take("order", order.map(|v| json!(v)), f);
            match (count, near, order) {
                (Some(c), Some(n), Some(o)) => json!({"near": n, "count": c, "order": o}),
                _ => Value::Null,
            }
        }
        Solver::Radiation => {
            let fr = take("frequency", freq(&f.frequency.clone()).map(|v| json!(v)), f);
            let order = take("order", order.map(|v| json!(v)), f);
            match (fr, order) {
                (Some(fr), Some(o)) => json!({"frequency": fr, "order": o}),
                _ => Value::Null,
            }
        }
    };
    if bad.is_empty() { Ok((solver, params)) } else { Err(bad.join("; ")) }
}

/// The preflight: what blocks the run and what it may accept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preflight {
    pub blocking: Vec<(Step, String)>,
    pub warnings: Vec<(Step, String)>,
}

/// Whether a run of `solver` (any, when `None`) cannot go without step `step` done: the
/// geometry, the mesh and, but for a resonator's eigenmodes, the excitation.
pub fn required(step: Step, solver: Option<Solver>) -> bool {
    match step {
        Step::Geometry | Step::Mesh => true,
        Step::Excitation => solver != Some(Solver::Eigenmodes),
        _ => false,
    }
}

/// What Run checks first: a step that needs attention, or one the solver cannot go without left
/// to do, blocks it; warnings and out-of-date results are accepted by pressing Run; other steps
/// left to do and steps waiting on another are not problems.
pub fn preflight(s: &emcraft_engine::Session, f: &mut Fields, states: &[StepState]) -> Preflight {
    let mut out = Preflight::default();
    let solver = f.solver.unwrap_or_else(|| Solver::infer(s.model()));
    for s in states.iter().filter(|s| s.step.index() <= Step::Setup.index()) {
        let needed = required(s.step, Some(solver));
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
            list.push((s.step, m.text.clone()));
        }
    }
    if let Err(e) = run_params(s, f) {
        out.blocking.push((Step::Setup, e));
    }
    out
}

/// Run: refuses on blocking problems; warnings are accepted by pressing Run.
pub fn start_run(app: &mut EmcraftApp) -> Result<Value, UiError> {
    let states = statuses_now(app);
    let pre = preflight(&app.session, &mut app.workflow.fields, &states);
    if !pre.blocking.is_empty() {
        let n = pre.blocking.len();
        let e = UiError::Refused(format!(
            "{n} {} {} it — {}",
            if n == 1 { "problem" } else { "problems" },
            if n == 1 { "blocks" } else { "block" },
            pre.blocking.iter().map(|(s, m)| format!("{} ({})", m, s.label())).collect::<Vec<_>>().join("; ")
        ));
        app.workflow.fields.refused = Some(e.to_string());
        app.workflow.blocked = true;
        app.error(&e);
        return Err(e);
    }
    app.workflow.fields.refused = None;
    app.workflow.blocked = false;
    let (solver, params) = run_params(&app.session, &mut app.workflow.fields).map_err(UiError::Input)?;
    let n = pre.warnings.len();
    if n > 0 {
        app.info(plural(n, "Running with {n} accepted warning.", "Running with {n} accepted warnings.", args!(n = n)));
    }
    app.run(solver.command(), params)
}

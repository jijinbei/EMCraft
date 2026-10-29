//! Starting a run: the solver's parameters from the panel's fields and the preflight checks.

use super::*;

/// The solver and its parameters from the setup's fields; each bad field is marked.
pub fn run_params(app: &mut EmcraftApp) -> Result<(Solver, Value), String> {
    let solver = app.workflow.fields.solver.unwrap_or_else(|| Solver::infer(app.session.model()));
    let band = band(app);
    let limit = app.session.limits().frequencies as u64;
    let f = &mut app.workflow.fields;
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
            band.map(|(a, z)| 0.5 * (a + z)).ok_or_else(|| "give a frequency (or set the band, step 0)".to_owned())
        } else {
            positive(text)
        }
    };
    let params = match solver {
        Solver::Frequency | Solver::TimeDomain => {
            let points = take("points", integer(&f.points.clone(), 1, limit).map(|v| json!(v)), f);
            let order = if solver == Solver::Frequency { take("order", order.map(|v| json!(v)), f) } else { Some(Value::Null) };
            if band.is_none() {
                bad.push("no frequency band: set it in step 0 (Intent)".into());
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

pub fn preflight(app: &mut EmcraftApp, states: &[StepState]) -> Preflight {
    let mut out = Preflight::default();
    let solver = app.workflow.fields.solver.unwrap_or_else(|| Solver::infer(app.session.model()));
    for s in states.iter().filter(|s| s.step.index() <= Step::Setup.index()) {
        // A resonator needs no excitation.
        if s.step == Step::Excitation && solver == Solver::Eigenmodes && s.status == Status::Missing {
            continue;
        }
        let list = match s.status {
            Status::Missing => &mut out.blocking,
            Status::Warning | Status::Stale => &mut out.warnings,
            Status::Done => continue,
        };
        if s.messages.is_empty() {
            list.push((s.step, format!("{}: {}", s.step.label(), s.status.id())));
        }
        for m in &s.messages {
            list.push((s.step, m.text.clone()));
        }
    }
    if let Err(e) = run_params(app) {
        out.blocking.push((Step::Setup, e));
    }
    out
}

/// Run: refuses on blocking problems; warnings are accepted by pressing Run.
pub fn start_run(app: &mut EmcraftApp) -> Result<Value, UiError> {
    let states = statuses(app);
    let pre = preflight(app, &states);
    if !pre.blocking.is_empty() {
        let e = UiError::Refused(format!(
            "{} problem(s) block it — {}",
            pre.blocking.len(),
            pre.blocking.iter().map(|(s, m)| format!("{} ({})", m, s.label())).collect::<Vec<_>>().join("; ")
        ));
        app.workflow.fields.refused = Some(e.to_string());
        app.error(&e);
        return Err(e);
    }
    app.workflow.fields.refused = None;
    let (solver, params) = run_params(app).map_err(UiError::Input)?;
    if !pre.warnings.is_empty() {
        app.info(format!("Running with {} accepted warning(s).", pre.warnings.len()));
    }
    app.run(solver.command(), params)
}

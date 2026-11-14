//! The job's commands: `project.analysis` chooses the kind, `project.setup` confirms its run's
//! settings ([`Setup`]) and `solve.run` runs them. A job is ready only once its settings are
//! confirmed; choosing another kind takes them back.

use emcraft_model::{Analysis, Band, MAX_MODES, Order, Setup, job};
use serde_json::{Value, json};

use crate::{EngineError, Params, Result, Session};

/// The run's defaults: the sweep's points, the eigenmodes' count and the time domain's cell (m).
pub const DEFAULT_POINTS: usize = 101;
pub const DEFAULT_MODES: usize = 5;
pub const DEFAULT_CELL: f64 = 1e-3;

/// The ports to excite, by name (none: the first).
fn excite(p: &Params) -> Result<Vec<String>> {
    let Some(list) = p.raw("excite") else { return Ok(Vec::new()) };
    let names = list.as_array().ok_or_else(|| p.error("excite", "must be a list of port names"))?;
    if names.len() > 64 {
        return Err(p.error("excite", "names too many ports"));
    }
    names.iter().map(|n| n.as_str().map(|n| crate::text::clip(n, 128)).ok_or_else(|| p.error("excite", "must be a list of port names"))).collect()
}

/// `project.analysis`: the kind of analysis the job is set up for.
pub fn analysis(s: &mut Session, p: &Params) -> Result<Value> {
    let kind: Analysis = p.pick("kind").required()?;
    s.model.set_job(s.model.job.choose(Some(kind)));
    Ok(crate::workflow::analysis_reply(&s.model))
}

/// `project.setup`: confirms the run's settings for the job's kind (or `kind`).
pub fn setup(s: &mut Session, p: &Params) -> Result<Value> {
    let setup = parse(s, p)?;
    s.model.set_job(emcraft_model::Job::Ready(setup));
    Ok(setup_reply(s))
}

/// The settings `p` gives (as `project.setup` takes them), checked.
pub fn parse(s: &Session, p: &Params) -> Result<Setup> {
    let kind = match p.pick::<Analysis>("kind").optional()? {
        Some(k) => k,
        None => s.model.analysis(),
    };
    let max = s.limits.frequencies;
    let order = || -> Result<Order> {
        let n = p.integer("order", 1..=2).or(2)?;
        Ok(if n == 1 { Order::One } else { Order::Two })
    };
    let points = || -> Result<_> { Ok(job::count("points", p.integer("points", 1..=max).or(DEFAULT_POINTS)?, max)?) };
    let band = || -> Result<Band> { Ok(Band::new(p.hertz("start").required()?, p.hertz("stop").required()?)?) };
    let setup = match kind {
        Analysis::Frequency => Setup::Frequency { band: band()?, points: points()?, order: order()? },
        Analysis::TimeDomain => Setup::TimeDomain {
            band: band()?,
            points: points()?,
            cell: p.metres("cell").optional()?.map_or_else(|| emcraft_model::Metres::new(DEFAULT_CELL), Ok)?,
            excite: excite(p)?,
        },
        Analysis::Eigenmodes => Setup::Eigenmodes {
            near: p.hertz("frequency").required()?,
            count: job::count("count", p.integer("count", 1..=MAX_MODES).or(DEFAULT_MODES)?, MAX_MODES)?,
            order: order()?,
            lowest: p.flag("lowest").or(false)?,
        },
        Analysis::Radiation => Setup::Radiation { frequency: p.hertz("frequency").required()?, order: order()? },
        Analysis::Pcb => return Err(p.error("kind", "the PCB diagnosis has no run settings: its conditions are pcb.inspect's")),
        Analysis::System => Setup::System { band: band()?, points: points()?, frequency: p.hertz("frequency").optional()? },
        Analysis::Particles => Setup::Particles {
            time: emcraft_model::Positive::new(p.positive("time").required()? * 1e-9)?,
            cell: p.metres("cell").required()?,
            vay: p.flag("vay").or(false)?,
        },
    };
    Ok(setup)
}

/// The confirmed settings as `project.setup` takes them, in display units (`{}` when none).
pub fn setup_reply(s: &Session) -> Value {
    let Some(setup) = s.model.job.setup() else { return json!({}) };
    let f = |h: emcraft_model::Hertz| s.show_frequency(h.get());
    let order = |o: Order| u8::from(o);
    let mut v = match setup {
        Setup::Frequency { band, points, order: o } => {
            json!({"start": f(band.start), "stop": f(band.stop), "points": points.get(), "order": order(*o)})
        }
        Setup::TimeDomain { band, points, cell, excite } => time_domain(s, *band, *points, *cell, excite),
        Setup::Eigenmodes { near, count, order: o, lowest: false } => json!({"frequency": f(*near), "count": count.get(), "order": order(*o)}),
        Setup::Eigenmodes { near, count, order: o, lowest: true } => {
            json!({"frequency": f(*near), "count": count.get(), "order": order(*o), "lowest": true})
        }
        Setup::Radiation { frequency, order: o } => json!({"frequency": f(*frequency), "order": order(*o)}),
        Setup::System { band, points, frequency } => system(s, *band, *points, *frequency),
        Setup::Particles { time, cell, vay } => particles(s, *time, *cell, *vay),
    };
    v["kind"] = json!(setup.analysis().id());
    v
}

/// The solve the confirmed settings run, and its parameters in display units.
pub fn run_of(s: &Session) -> Result<(&'static str, Value)> {
    let setup = s.model.job.setup().ok_or_else(|| EngineError::failed("the run's settings are not confirmed yet: project.setup"))?;
    Ok(solve_of(s, setup))
}

/// The solve settings `setup` run, and its parameters in display units.
pub fn solve_of(s: &Session, setup: &Setup) -> (&'static str, Value) {
    let f = |h: emcraft_model::Hertz| s.show_frequency(h.get());
    match setup {
        Setup::Frequency { band, points, order } => {
            ("solve.frequency", json!({"start": f(band.start), "stop": f(band.stop), "points": points.get(), "order": u8::from(*order)}))
        }
        Setup::TimeDomain { band, points, cell, excite } => ("solve.time_domain", time_domain(s, *band, *points, *cell, excite)),
        // A periodic cell's modes depend on the phase across it: its dispersion diagram, the
        // bands up to the same shift.
        Setup::Eigenmodes { near, count, order, .. } if s.model.domain.periodic.is_some() => {
            ("solve.dispersion", json!({"near": f(*near), "bands": count.get().min(20), "order": u8::from(*order)}))
        }
        Setup::Eigenmodes { near, count, order, lowest: false } => {
            ("solve.eigenmodes", json!({"near": f(*near), "count": count.get(), "order": u8::from(*order)}))
        }
        Setup::Eigenmodes { count, order, lowest: true, .. } => {
            ("solve.eigenmodes", json!({"lowest": true, "count": count.get(), "order": u8::from(*order)}))
        }
        Setup::Radiation { frequency, order } => ("solve.radiation", json!({"frequency": f(*frequency), "order": u8::from(*order)})),
        Setup::System { band, points, frequency } => ("solve.system", system(s, *band, *points, *frequency)),
        Setup::Particles { time, cell, vay } => ("solve.particles", particles(s, *time, *cell, *vay)),
    }
}

/// A system's settings as its solve takes them, in display units.
fn system(s: &Session, band: Band, points: std::num::NonZeroUsize, frequency: Option<emcraft_model::Hertz>) -> Value {
    let mut v = json!({"start": s.show_frequency(band.start.get()), "stop": s.show_frequency(band.stop.get()), "points": points.get()});
    if let Some(h) = frequency {
        v["frequency"] = json!(s.show_frequency(h.get()));
    }
    v
}

/// A particle run's settings as its solve takes them: the time in ns, the cell in display units.
fn particles(s: &Session, time: emcraft_model::Positive, cell: emcraft_model::Metres, vay: bool) -> Value {
    let mut v = json!({"time": time.get() * 1e9, "cell": s.show_length(cell.get())});
    if vay {
        v["vay"] = json!(true);
    }
    v
}

/// The time domain's settings as its solve takes them, in display units.
fn time_domain(s: &Session, band: Band, points: std::num::NonZeroUsize, cell: emcraft_model::Metres, excite: &[String]) -> Value {
    let mut v = json!({"start": s.show_frequency(band.start.get()), "stop": s.show_frequency(band.stop.get()), "points": points.get(), "cell": s.show_length(cell.get())});
    if !excite.is_empty() {
        v["excite"] = json!(excite);
    }
    v
}

/// `solve.run`: runs the confirmed settings, as their solve runs (its reply, with `run`
/// naming the command and parameters).
pub fn run(s: &mut Session, _: &Params) -> Result<Value> {
    if s.model.job.setup().is_none() && !own_runs(s).is_empty() {
        return run_own(s);
    }
    let (command, params) = run_of(s)?;
    let mut reply = s.execute(command, &params)?;
    if let Value::Object(o) = &mut reply {
        o.insert("run".into(), json!({"command": command, "params": params}));
    }
    Ok(reply)
}

/// The solves a project records that no kind of analysis' settings make (a periodic cell's
/// Bloch waves, a body of revolution's runs, a pulse through the time domain): a sample's own,
/// in the order recorded.
pub fn own_runs(s: &Session) -> Vec<String> {
    let made = |c: &str| crate::workflow::ANALYSES.iter().any(|a| a.solve == c) || c == "solve.dispersion";
    s.runs.iter().filter(|r| !made(&r.command)).map(|r| r.command.clone()).collect()
}

/// `solve.run` with no settings confirmed: the project's own solves ([`own_runs`]) run again,
/// in order (the last one's reply, with `run` naming it).
fn run_own(s: &mut Session) -> Result<Value> {
    let mut last = Value::Null;
    for command in own_runs(s) {
        crate::progress::check()?;
        last = crate::runs::rerun(s, &Params::new(&json!({"command": command}), s.display))?;
    }
    if let Value::Object(o) = &mut last
        && let Some(rerun) = o.remove("rerun")
    {
        o.insert("run".into(), rerun);
    }
    Ok(last)
}

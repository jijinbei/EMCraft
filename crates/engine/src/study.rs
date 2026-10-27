//! Parameter studies: named variables, sweeps and optimization of the model's dimensions and
//! materials.
//!
//! A variable is defined by a command of the history (`project.variable`), and any number
//! parameter of a later command may refer to it by an expression (`"=w/2 + 1"`, [`crate::expr`]).
//! The history keeps the expressions, so the model is rebuilt with other values by replaying it
//! with the definitions' values replaced: changing a variable does that in the session itself;
//! `study.sweep` and `study.optimize` do it for each point in a session of their own, solve it
//! with the command they were given and collect the results, leaving the session's model as it
//! was (the sweep) or at the optimum (the optimization, as one change undo takes back).
//!
//! The optimizer is the downhill simplex of J. A. Nelder and R. Mead, "A simplex method for
//! function minimization", Comput. J. 7(4):308–313, 1965, with the standard coefficients
//! (reflection 1, expansion 2, contraction ½, shrink ½; J. C. Lagarias, J. A. Reeds, M. H. Wright
//! and P. E. Wright, "Convergence properties of the Nelder–Mead simplex method in low
//! dimensions", SIAM J. Optim. 9(1):112–147, 1998, for the order of the tests and the tie
//! rules). The variables are scaled to [0, 1] over their bounds and every trial point clamped
//! into the box; the vertices are ordered by value, ties by age, so the search is the same for
//! the same input. Solves are counted and capped, and points already solved are not solved again.

use std::collections::BTreeMap;
use std::sync::Arc;

use emcraft_model::units::decibels;
use emcraft_render::Rgb;
use emcraft_render::axes::Axis;
use emcraft_render::chart::{Chart, Series, Style};
use emcraft_solver::Complex64 as C;
use serde_json::{Map, Value, json};

use crate::expr;
use crate::project::Effect;
use crate::{EngineError, Params, Result, Session};

/// The most variables a session holds.
pub const MAX_VARIABLES: usize = 200;
/// The most points one sweep solves.
pub const MAX_POINTS: usize = 101;
/// The most solves one optimization runs.
pub const MAX_EVALUATIONS: usize = 500;
/// The most variables one optimization varies.
pub const MAX_OPTIMIZED: usize = 8;

/// What kind of study made the results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sweep,
    Optimize,
}

/// One point of a study: the variables' values and what the solve gave there (display units).
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub values: Vec<f64>,
    /// The solve's frequencies.
    pub frequencies: Vec<f64>,
    /// S-parameters by name (`s11`, `s21`, …), one value per frequency.
    pub s: Vec<(String, Vec<C>)>,
    /// Eigenmodes' frequencies.
    pub modes: Vec<f64>,
    /// The solve's other numbers (radiated power, efficiency, …).
    pub scalars: Vec<(String, f64)>,
    /// The goal's value (optimization).
    pub objective: Option<f64>,
    /// Why this point could not be solved.
    pub error: Option<String>,
}

impl Row {
    /// |`name`| in dB at each frequency.
    pub fn db(&self, name: &str) -> Option<Vec<f64>> {
        self.s.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.iter().map(|c| db(*c)).collect())
    }

    fn to_json(&self, kind: Kind) -> Value {
        let mut row = Map::new();
        match kind {
            Kind::Sweep => row.insert("value".into(), json!(self.values.first())),
            Kind::Optimize => row.insert("values".into(), json!(self.values)),
        };
        if let Some(o) = self.objective {
            row.insert("objective".into(), json!(o));
        }
        if let Some(e) = &self.error {
            row.insert("error".into(), json!(e));
        }
        if !self.frequencies.is_empty() {
            let points: Vec<Value> = self
                .frequencies
                .iter()
                .enumerate()
                .map(|(k, f)| {
                    let mut pt = Map::new();
                    pt.insert("frequency".into(), json!(f));
                    for (name, list) in &self.s {
                        if let Some(c) = list.get(k) {
                            pt.insert(name.clone(), crate::results::complex(*c));
                        }
                    }
                    Value::Object(pt)
                })
                .collect();
            row.insert("points".into(), json!(points));
        }
        if !self.modes.is_empty() {
            row.insert("modes".into(), json!(self.modes));
        }
        if !self.scalars.is_empty() {
            row.insert("scalars".into(), Value::Object(self.scalars.iter().map(|(k, v)| (k.clone(), json!(v))).collect()));
        }
        Value::Object(row)
    }
}

/// A parameter study's results.
#[derive(Clone, Debug)]
pub struct Study {
    pub kind: Kind,
    /// The variables varied, in the rows' order of values.
    pub variables: Vec<String>,
    /// The solve command run at each point.
    pub solve: String,
    pub frequency_unit: String,
    /// The goal minimized (optimization), in words.
    pub goal: Option<String>,
    pub rows: Vec<Row>,
    /// The row of the optimum.
    pub best: Option<usize>,
    /// Whether the optimization's simplex shrank within its tolerance (else it ran out of solves).
    pub converged: bool,
}

impl Study {
    /// The S-parameters' names the rows hold.
    pub fn parameters(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for (n, _) in self.rows.iter().flat_map(|r| &r.s) {
            if !names.contains(n) {
                names.push(n.clone());
            }
        }
        names
    }

    pub fn to_json(&self) -> Value {
        let rows: Vec<Value> = self.rows.iter().map(|r| r.to_json(self.kind)).collect();
        let mut reply = json!({"solve": self.solve, "frequency_unit": self.frequency_unit, "rows": rows});
        match self.kind {
            Kind::Sweep => reply["variable"] = json!(self.variables.first()),
            Kind::Optimize => {
                reply["variables"] = json!(self.variables);
                reply["goal"] = json!(self.goal);
                reply["evaluations"] = json!(self.rows.len());
                reply["converged"] = json!(self.converged);
                if let Some(b) = self.best.and_then(|b| self.rows.get(b)) {
                    reply["best"] = json!({"values": b.values, "objective": b.objective});
                }
            }
        }
        reply
    }
}

fn db(c: C) -> f64 {
    decibels(c.norm().max(1e-15))
}

// ---------------------------------------------------------------- variables

/// The command that defines variables.
const DEFINE: &str = "project.variable";

fn is_definition(step: &Value, name: &str) -> bool {
    step.get("command").and_then(Value::as_str) == Some(DEFINE) && step.pointer("/params/name").and_then(Value::as_str) == Some(name)
}

/// The names the expressions in `v` (a command's parameters) refer to.
fn references(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(t) => {
            if let Some(names) = expr::body(t).and_then(|b| expr::names(b).ok()) {
                out.extend(names);
            }
        }
        Value::Array(list) => list.iter().for_each(|x| references(x, out)),
        Value::Object(map) => map.values().for_each(|x| references(x, out)),
        _ => {}
    }
}

/// The session's history with the definitions of `set`'s variables given their values; an error
/// naming a variable the history does not define.
fn history_with(s: &Session, set: &[(String, Value)]) -> Result<Vec<Value>> {
    let mut history = s.history.clone();
    for (name, value) in set {
        let mut found = false;
        for step in history.iter_mut().filter(|step| is_definition(step, name)) {
            step["params"]["value"] = value.clone();
            found = true;
        }
        if !found {
            return Err(EngineError::Params(format!(
                "no variable `{}`: define it first (project.variable)",
                name.chars().take(expr::MAX_NAME).collect::<String>()
            )));
        }
    }
    Ok(history)
}

/// A session of the model rebuilt by `history`.
fn rebuild(s: &Session, history: Vec<Value>) -> Result<Session> {
    let mut fresh = s.fresh();
    fresh.run_script(&Value::Array(history))?;
    Ok(fresh)
}

/// Makes `rebuilt` the session, keeping what belongs to the job rather than the model: undo, the
/// file, the saved state, the last study.
fn adopt(s: &mut Session, mut rebuilt: Session) {
    rebuilt.undo = std::mem::take(&mut s.undo);
    rebuilt.path = s.path.take();
    rebuilt.sample = s.sample.take();
    rebuilt.saved = std::mem::take(&mut s.saved);
    rebuilt.results.study = s.results.study.take();
    rebuilt.solved_revision = None;
    *s = rebuilt;
}

/// `project.variable`: defines a variable, or changes one and rebuilds the model with it.
pub fn variable(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    if !expr::valid_name(name) {
        return Err(
            p.error("name", format!("must be a letter or `_` then letters, digits or `_`, at most {} long, not a function's name", expr::MAX_NAME))
        );
    }
    let value = p.number("value").required()?;
    let step = json!({"command": DEFINE, "params": p.value()});
    if s.variables.contains_key(name) {
        // The definition changed in place: the commands after it read the new value.
        let mut history = s.history.clone();
        for old in history.iter_mut().filter(|st| is_definition(st, name)) {
            *old = step.clone();
        }
        let rebuilt = rebuild(s, history)?;
        adopt(s, rebuilt);
    } else {
        if s.variables.len() >= MAX_VARIABLES {
            return Err(EngineError::failed(format!("a project holds at most {MAX_VARIABLES} variables")));
        }
        s.variables.insert(name.to_owned(), value);
        s.history.push(step);
    }
    Ok(json!({"name": name, "value": value, "variables": s.variables.len()}))
}

/// `project.variable.delete`: removes a variable no command refers to.
pub fn delete(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    if !s.variables.contains_key(name) {
        return Err(p.error("name", format!("names no variable: `{}`", name.chars().take(expr::MAX_NAME).collect::<String>())));
    }
    for (i, step) in s.history.iter().enumerate() {
        let mut names = Vec::new();
        references(step.get("params").unwrap_or(&Value::Null), &mut names);
        if names.iter().any(|n| n == name) {
            let id = step.get("command").and_then(Value::as_str).unwrap_or("?");
            return Err(EngineError::failed(format!("`{name}` is used by step {} ({id}): change that first", i + 1)));
        }
    }
    s.history.retain(|st| !is_definition(st, name));
    s.variables.remove(name);
    Ok(json!({"name": name, "variables": s.variables.len()}))
}

/// `project.variables`: the variables, their values, definitions and how many commands use them.
pub fn list(s: &mut Session, _: &Params) -> Result<Value> {
    let mut uses: BTreeMap<&str, usize> = BTreeMap::new();
    for step in &s.history {
        let mut names = Vec::new();
        references(step.get("params").unwrap_or(&Value::Null), &mut names);
        names.dedup();
        for n in &names {
            if let Some((k, _)) = s.variables.get_key_value(n.as_str()) {
                *uses.entry(k.as_str()).or_default() += 1;
            }
        }
    }
    let rows: Vec<Value> = s
        .variables
        .iter()
        .map(|(name, value)| {
            let given = s.history.iter().rev().find(|st| is_definition(st, name)).and_then(|st| st.pointer("/params/value"));
            let mut row = json!({"name": name, "value": value, "uses": uses.get(name.as_str()).copied().unwrap_or(0)});
            if let Some(Value::String(e)) = given {
                row["expression"] = json!(e);
            }
            row
        })
        .collect();
    Ok(json!({"variables": rows}))
}

// ---------------------------------------------------------------- solving a point

/// The solve command a study runs at each point.
struct Solve<'a> {
    command: &'a str,
    params: Value,
}

fn solve_of<'a>(p: &Params<'a>) -> Result<Solve<'a>> {
    let v = p.raw("solve").ok_or_else(|| p.error("solve", "is required: {\"command\": \"solve.frequency\", \"params\": {…}}"))?;
    let command =
        v.get("command").and_then(Value::as_str).ok_or_else(|| p.error("solve", "needs a `command` (solve.frequency, solve.eigenmodes, …)"))?;
    let spec = crate::find_command(command).filter(|c| c.effect == Effect::Solve).ok_or_else(|| {
        let solvers: Vec<&str> = crate::COMMANDS.iter().filter(|c| c.effect == Effect::Solve).map(|c| c.id).collect();
        p.error("solve", format!("`command` must be a solve: {}", solvers.join(", ")))
    })?;
    let params = v.get("params").cloned().unwrap_or_else(|| json!({}));
    if !params.is_object() {
        return Err(p.error("solve", "`params` must be an object"));
    }
    Ok(Solve { command: spec.id, params })
}

/// The S-parameter names `outputs` keeps (all when empty).
fn outputs_of(p: &Params) -> Result<Vec<String>> {
    p.list("outputs", 64)
        .or(&[])?
        .iter()
        .map(|v| v.as_str().map(str::to_ascii_lowercase).ok_or_else(|| p.error("outputs", "must be names such as \"s11\"")))
        .collect()
}

/// Whether a failed point stops the whole study: cancelled, or asked wrongly (every point would
/// fail alike).
fn stops(e: &EngineError) -> bool {
    match e {
        EngineError::Failed(m) => m == crate::progress::CANCELLED,
        EngineError::Params(_) | EngineError::UnknownCommand(_) => true,
        EngineError::Panicked(_) => false,
    }
}

/// The model rebuilt with `set` and solved: the point's row (values left to the caller).
fn solve_point(s: &Session, set: &[(String, Value)], solve: &Solve, outputs: &[String]) -> Result<Row> {
    let history = history_with(s, set)?;
    let run = || -> Result<Value> {
        let mut fresh = rebuild(s, history)?;
        fresh.execute(solve.command, &solve.params)
    };
    match run() {
        Ok(reply) => Ok(row_of(&reply, outputs)),
        Err(e) if stops(&e) => Err(e),
        Err(e) => Ok(Row { error: Some(e.to_string()), ..Row::default() }),
    }
}

/// What a solve's reply holds, as a row: its points' S-parameters, its modes, its numbers.
fn row_of(reply: &Value, outputs: &[String]) -> Row {
    let mut row = Row::default();
    let keep = |name: &str| outputs.is_empty() || outputs.iter().any(|o| o == name);
    let is_s = |k: &str| k.len() >= 3 && k.starts_with('s') && k[1..].bytes().all(|b| b.is_ascii_digit());
    let complex = |v: &Value| Some(C::new(v.get("re")?.as_f64()?, v.get("im")?.as_f64()?));
    let add = |row: &mut Row, name: &str, k: usize, c: C| {
        if !keep(name) {
            return;
        }
        let at = match row.s.iter().position(|(n, _)| n == name) {
            Some(at) => at,
            None => {
                row.s.push((name.to_owned(), Vec::new()));
                row.s.len() - 1
            }
        };
        let list = &mut row.s[at].1;
        list.resize(k, C::new(f64::NAN, f64::NAN));
        list.push(c);
    };
    if let Some(points) = reply.get("points").and_then(Value::as_array) {
        for (k, pt) in points.iter().enumerate() {
            row.frequencies.push(pt.get("frequency").and_then(Value::as_f64).unwrap_or(f64::NAN));
            if let Some(map) = pt.as_object() {
                for (name, v) in map.iter().filter(|(n, _)| is_s(n)) {
                    if let Some(c) = complex(v) {
                        add(&mut row, name, k, c);
                    }
                }
            }
        }
    } else if let (Some(f), Some(c)) = (reply.get("frequency").and_then(Value::as_f64), reply.pointer("/feed/s11").and_then(complex)) {
        // An antenna fed by a guide: its S11 at the one frequency.
        row.frequencies.push(f);
        add(&mut row, "s11", 0, c);
    }
    if let Some(modes) = reply.get("modes").and_then(Value::as_array) {
        row.modes = modes.iter().filter_map(|m| m.get("frequency").and_then(Value::as_f64)).collect();
    }
    if let Some(map) = reply.as_object() {
        row.scalars = map.iter().filter_map(|(k, v)| v.as_f64().filter(|x| x.is_finite()).map(|x| (k.clone(), x))).collect();
    }
    row
}

/// The frequency unit of the session's replies.
fn frequency_unit(s: &Session) -> String {
    s.display.frequency.symbol().to_owned()
}

// ---------------------------------------------------------------- sweep

/// `study.sweep`: the model rebuilt and solved at each value of a variable.
pub fn sweep(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("variable").required()?;
    if !s.variables.contains_key(name) {
        return Err(p.error(
            "variable",
            format!("names no variable: `{}` (define it: project.variable)", name.chars().take(expr::MAX_NAME).collect::<String>()),
        ));
    }
    let values: Vec<f64> = if p.has("values") {
        p.list("values", MAX_POINTS).required()?.iter().map(|v| p.number_of("values", v)).collect::<Result<_>>()?
    } else {
        let start = p.number("start").required_or("or give `values`")?;
        let stop = p.number("stop").required_or("or give `values`")?;
        let n = p.integer("points", 1..=MAX_POINTS).or(5)?;
        (0..n).map(|k| if n == 1 { start } else { start + (stop - start) * k as f64 / (n - 1) as f64 }).collect()
    };
    if values.is_empty() {
        return Err(p.error("values", "must hold at least one value"));
    }
    let solve = solve_of(p)?;
    let outputs = outputs_of(p)?;
    let n = values.len();
    let rows = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            crate::progress::report(i * 1000, n * 1000, &format!("{name} = {v}"))?;
            let set = [(name.to_owned(), json!(v))];
            let row = crate::progress::within(i, n, &format!("point {} of {n}: {name} = {v}", i + 1), || solve_point(s, &set, &solve, &outputs))?;
            Ok(Row { values: vec![*v], ..row })
        })
        .collect::<Result<Vec<Row>>>()?;
    let study = Study {
        kind: Kind::Sweep,
        variables: vec![name.to_owned()],
        solve: solve.command.to_owned(),
        frequency_unit: frequency_unit(s),
        goal: None,
        rows,
        best: None,
        converged: true,
    };
    let reply = study.to_json();
    s.results.study = Some(Arc::new(study));
    Ok(reply)
}

// ---------------------------------------------------------------- optimization

/// What an optimization minimizes.
#[derive(Clone, Debug, PartialEq)]
enum Goal {
    /// The largest |S| in dB over the frequencies (a reflection to bring down).
    MinMaxDb(String),
    /// The smallest |S| in dB over the frequencies, raised (a transmission to bring up).
    MaxMinDb(String),
    /// The farthest |S| in dB from `target` over the frequencies.
    TargetDb(String, f64),
    /// An eigenmode's distance from a frequency (display units).
    Eigenfrequency(usize, f64),
    /// One of the solve's numbers, raised or lowered.
    Scalar(String, bool),
}

impl Goal {
    fn read(p: &Params) -> Result<Goal> {
        let v = p.raw("goal").ok_or_else(|| p.error("goal", "is required"))?;
        if !v.is_object() {
            return Err(p.error("goal", "must be an object: {\"type\": \"min_max_db\", \"parameter\": \"s11\"}"));
        }
        let g = p.nested(v, "goal");
        let ty = g.string("type").required()?;
        let parameter = |default: &str| -> Result<String> {
            let name = g.string("parameter").or(default)?.to_ascii_lowercase();
            if name.len() < 3 || !name.starts_with('s') || !name[1..].bytes().all(|b| b.is_ascii_digit()) || name.len() > 8 {
                return Err(g.error("parameter", "must be an S-parameter such as s11 or s21"));
            }
            Ok(name)
        };
        Ok(match ty {
            "min_max_db" => Goal::MinMaxDb(parameter("s11")?),
            "max_min_db" => Goal::MaxMinDb(parameter("s21")?),
            "target_db" => Goal::TargetDb(parameter("s21")?, g.number("target").required()?),
            "eigenfrequency" => Goal::Eigenfrequency(g.integer("mode", 1..=crate::EIGEN_MAX).or(1)?, g.positive("target").required()?),
            "maximize" | "minimize" => {
                let name = g.string("quantity").required_or("a number the solve gives, such as radiated_power")?;
                Goal::Scalar(name.chars().take(64).collect(), ty == "maximize")
            }
            _ => return Err(g.error("type", "must be min_max_db, max_min_db, target_db, eigenfrequency, maximize or minimize")),
        })
    }

    fn describe(&self, unit: &str) -> String {
        match self {
            Goal::MinMaxDb(n) => format!("minimize the largest |{}| (dB)", n.to_uppercase()),
            Goal::MaxMinDb(n) => format!("maximize the smallest |{}| (dB)", n.to_uppercase()),
            Goal::TargetDb(n, t) => format!("|{}| at {t} dB", n.to_uppercase()),
            Goal::Eigenfrequency(m, t) => format!("mode {m} at {t} {unit}"),
            Goal::Scalar(n, true) => format!("maximize {n}"),
            Goal::Scalar(n, false) => format!("minimize {n}"),
        }
    }

    /// The value to minimize at a solved point; an error when the solve does not give what the
    /// goal reads.
    fn value(&self, row: &Row) -> Result<f64> {
        let missing = |what: &str| EngineError::Params(format!("`goal`: the solve gives no {what}"));
        let dbs = |n: &str| -> Result<Vec<f64>> {
            let v = row.db(n).ok_or_else(|| missing(n))?;
            if v.iter().all(|x| x.is_finite()) && !v.is_empty() { Ok(v) } else { Err(missing(n)) }
        };
        Ok(match self {
            Goal::MinMaxDb(n) => dbs(n)?.into_iter().fold(f64::NEG_INFINITY, f64::max),
            Goal::MaxMinDb(n) => -dbs(n)?.into_iter().fold(f64::INFINITY, f64::min),
            Goal::TargetDb(n, t) => dbs(n)?.into_iter().map(|x| (x - t).abs()).fold(0.0, f64::max),
            Goal::Eigenfrequency(m, t) => (row.modes.get(m - 1).ok_or_else(|| missing(&format!("mode {m}")))? - t).abs(),
            Goal::Scalar(n, up) => {
                let x = row.scalars.iter().find(|(k, _)| k == n).map(|(_, x)| *x).ok_or_else(|| missing(n))?;
                if *up { -x } else { x }
            }
        })
    }
}

/// A variable the optimizer varies within its bounds.
struct Bound {
    name: String,
    min: f64,
    max: f64,
}

impl Bound {
    fn value(&self, u: f64) -> f64 {
        self.min + (self.max - self.min) * u.clamp(0.0, 1.0)
    }
}

/// `study.optimize`: the variables that minimize a goal, by the Nelder–Mead simplex; the model
/// left at the best point.
pub fn optimize(s: &mut Session, p: &Params) -> Result<Value> {
    let items = p.items("variables", "variable", MAX_OPTIMIZED)?;
    if items.is_empty() {
        return Err(p.error("variables", "must name at least one variable: [{\"name\", \"min\", \"max\"}]"));
    }
    let mut bounds = Vec::new();
    for item in &items {
        let name = item.string("name").required()?;
        if !s.variables.contains_key(name) {
            return Err(item.error(
                "name",
                format!("names no variable: `{}` (define it: project.variable)", name.chars().take(expr::MAX_NAME).collect::<String>()),
            ));
        }
        if bounds.iter().any(|b: &Bound| b.name == name) {
            return Err(item.error("name", "is given twice"));
        }
        let (min, max) = (item.number("min").required()?, item.number("max").required()?);
        if max <= min {
            return Err(item.error("max", "must be above `min`"));
        }
        bounds.push(Bound { name: name.to_owned(), min, max });
    }
    let goal = Goal::read(p)?;
    let solve = solve_of(p)?;
    let cap = p.integer("max_evaluations", 2..=MAX_EVALUATIONS).or(50)?;
    let tolerance = p.number("tolerance").check(|t| (1e-9..=0.5).contains(t), "from 1e-9 to 0.5").or(1e-3)?;

    // The search, in [0, 1] per variable from the current values.
    let start: Vec<f64> =
        bounds.iter().map(|b| ((s.variables.get(&b.name).copied().unwrap_or(b.min) - b.min) / (b.max - b.min)).clamp(0.0, 1.0)).collect();
    let mut rows: Vec<Row> = Vec::new();
    let mut seen: BTreeMap<Vec<u64>, f64> = BTreeMap::new();
    let session: &Session = s;
    let mut evaluate = |u: &[f64]| -> Result<f64> {
        let u: Vec<f64> = u.iter().map(|x| x.clamp(0.0, 1.0)).collect();
        let key: Vec<u64> = u.iter().map(|x| x.to_bits()).collect();
        if let Some(f) = seen.get(&key) {
            return Ok(*f);
        }
        if rows.len() >= cap {
            return Err(EngineError::Failed(OUT_OF_SOLVES.into()));
        }
        let values: Vec<f64> = bounds.iter().zip(&u).map(|(b, x)| b.value(*x)).collect();
        let set: Vec<(String, Value)> = bounds.iter().zip(&values).map(|(b, v)| (b.name.clone(), json!(v))).collect();
        let i = rows.len();
        let label = bounds.iter().zip(&values).map(|(b, v)| format!("{} = {v:.6}", b.name)).collect::<Vec<_>>().join(", ");
        let mut row =
            crate::progress::within(i, cap, &format!("solve {} of at most {cap}: {label}", i + 1), || solve_point(session, &set, &solve, &[]))?;
        let f = if row.error.is_some() { f64::INFINITY } else { goal.value(&row)? };
        row.values = values;
        row.objective = f.is_finite().then_some(f);
        rows.push(row);
        seen.insert(key, f);
        Ok(f)
    };
    let converged = match nelder_mead(&mut evaluate, &start, tolerance, cap) {
        Ok(c) => c,
        Err(EngineError::Failed(m)) if m == OUT_OF_SOLVES => false,
        Err(e) => return Err(e),
    };
    let best = rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.objective.map(|o| (i, o)))
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(i, _)| i)
        .ok_or_else(|| {
            EngineError::failed(format!("no point could be solved: {}", rows.first().and_then(|r| r.error.clone()).unwrap_or_default()))
        })?;
    let unit = frequency_unit(s);
    let study = Study {
        kind: Kind::Optimize,
        variables: bounds.iter().map(|b| b.name.clone()).collect(),
        solve: solve.command.to_owned(),
        frequency_unit: unit.clone(),
        goal: Some(goal.describe(&unit)),
        rows,
        best: Some(best),
        converged,
    };
    // The model at the optimum: the definitions given the best values, the model rebuilt.
    let set: Vec<(String, Value)> = study.variables.iter().zip(&study.rows[best].values).map(|(n, v)| (n.clone(), json!(v))).collect();
    let rebuilt = rebuild(s, history_with(s, &set)?)?;
    let reply = study.to_json();
    adopt(s, rebuilt);
    s.results.study = Some(Arc::new(study));
    Ok(reply)
}

// ---------------------------------------------------------------- pictures

/// A colour along a ramp from blue to red, `t` from 0 to 1: a sweep's values in order.
fn ramp(t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: f64, b: f64| (a + (b - a) * t).round() as u8;
    Rgb(mix(30.0, 200.0), mix(80.0, 50.0), mix(200.0, 40.0))
}

fn axis_of(label: &str, values: impl Iterator<Item = f64>) -> Axis {
    let (lo, hi) = values.filter(|v| v.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
    if !lo.is_finite() {
        return Axis::new(label, 0.0, 1.0);
    }
    let pad = if hi > lo { 0.05 * (hi - lo) } else { 0.05 * lo.abs().max(1.0) };
    Axis::new(label, lo - pad, hi + pad)
}

/// The study as a chart: a sweep's |`parameter`| in dB against frequency, one curve per value
/// (against the value when it solved one frequency), or its eigenmodes against the value; an
/// optimization's goal at each evaluation with the best so far.
pub fn chart(study: &Study, parameter: Option<&str>) -> Result<Chart> {
    let fu = &study.frequency_unit;
    let none = || EngineError::failed("the study has nothing to chart: no S-parameters, eigenmodes or goal values");
    let ok: Vec<&Row> = study.rows.iter().filter(|r| r.error.is_none()).collect();
    let name = |v: &str| study.variables.first().map_or(v.to_owned(), |n| n.clone());
    if study.kind == Kind::Optimize {
        let mut best = f64::INFINITY;
        let (mut points, mut so_far) = (Vec::new(), Vec::new());
        for (i, r) in study.rows.iter().enumerate() {
            if let Some(o) = r.objective {
                points.push([(i + 1) as f64, o]);
                best = best.min(o);
            }
            if best.is_finite() {
                so_far.push([(i + 1) as f64, best]);
            }
        }
        if points.is_empty() {
            return Err(none());
        }
        let y = axis_of("goal", points.iter().map(|p| p[1]));
        return Ok(Chart {
            title: format!("Optimization: {}", study.goal.clone().unwrap_or_default()),
            x: Axis::new("evaluation", 0.5, study.rows.len().max(2) as f64 + 0.5),
            y,
            series: vec![
                Series { label: "each solve".into(), color: Rgb(140, 140, 150), style: Style::Markers, points },
                Series { label: "best so far".into(), color: Rgb(30, 90, 200), style: Style::Line, points: so_far },
            ],
            marks: Vec::new(),
        });
    }
    let parameters = study.parameters();
    let parameter = match parameter {
        Some(p) => Some(
            parameters
                .iter()
                .find(|n| n.eq_ignore_ascii_case(p))
                .cloned()
                .ok_or_else(|| EngineError::Params(format!("`parameter` must be one the study holds: {}", parameters.join(", "))))?,
        ),
        None => parameters.first().cloned(),
    };
    let tex = |p: &str| format!("$|S_{{{}}}|$", p.trim_start_matches('s'));
    let n = ok.len();
    if let Some(p) = parameter {
        let label = tex(&p);
        let one_frequency = ok.iter().all(|r| r.frequencies.len() == 1);
        let mut series = Vec::new();
        if one_frequency {
            let pts: Vec<[f64; 2]> = ok.iter().filter_map(|r| Some([*r.values.first()?, *r.db(&p)?.first()?])).collect();
            let f = ok.first().and_then(|r| r.frequencies.first()).copied().unwrap_or(f64::NAN);
            series.push(Series { label: format!("{label} at {f} {fu}"), color: Rgb(30, 90, 200), style: Style::Line, points: pts.clone() });
            series.push(Series { label: String::new(), color: Rgb(30, 90, 200), style: Style::Markers, points: pts });
            let y = axis_of(&format!("{label} (dB)"), series.iter().flat_map(|s| s.points.iter().map(|q| q[1])));
            let x = axis_of(&name("value"), ok.iter().filter_map(|r| r.values.first().copied()));
            return Ok(Chart { title: format!("Sweep of {}", name("")), x, y, series, marks: Vec::new() });
        }
        for (i, r) in ok.iter().enumerate() {
            let Some(db) = r.db(&p) else { continue };
            let points: Vec<[f64; 2]> = r.frequencies.iter().zip(db).map(|(f, d)| [*f, d.max(-80.0)]).collect();
            let v = r.values.first().copied().unwrap_or(f64::NAN);
            let t = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
            series.push(Series { label: format!("{} = {v}", name("")), color: ramp(t), style: Style::Line, points });
        }
        if series.is_empty() {
            return Err(none());
        }
        let (f0, f1) =
            series.iter().flat_map(|s| s.points.iter().map(|q| q[0])).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), f| (a.min(f), b.max(f)));
        let x = if f1 > f0 { Axis::new(&format!("frequency ({fu})"), f0, f1) } else { axis_of(&format!("frequency ({fu})"), [f0].into_iter()) };
        let lowest = series.iter().flat_map(|s| s.points.iter().map(|q| q[1])).fold(0.0f64, f64::min);
        let y = Axis::new(&format!("{label} (dB)"), ((lowest / 10.0).floor() * 10.0).clamp(-80.0, -10.0), 2.0);
        return Ok(Chart { title: format!("{label} as {} varies", name("")), x, y, series, marks: Vec::new() });
    }
    let modes = ok.iter().map(|r| r.modes.len()).max().unwrap_or(0);
    if modes == 0 {
        return Err(none());
    }
    let series: Vec<Series> = (0..modes.min(12))
        .map(|m| Series {
            label: format!("mode {}", m + 1),
            color: ramp(m as f64 / (modes.max(2) - 1) as f64),
            style: Style::Line,
            points: ok.iter().filter_map(|r| Some([*r.values.first()?, *r.modes.get(m)?])).collect(),
        })
        .collect();
    let x = axis_of(&name("value"), ok.iter().filter_map(|r| r.values.first().copied()));
    let y = axis_of(&format!("frequency ({fu})"), series.iter().flat_map(|s| s.points.iter().map(|q| q[1])));
    Ok(Chart { title: format!("Eigenmodes as {} varies", name("")), x, y, series, marks: Vec::new() })
}

/// `view.study`: the last study's chart as a PNG.
pub fn view(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let (w, h) = (p.side("width", s.limits.picture_side, 1000)?, p.side("height", s.limits.picture_side, 600)?);
    let study = s.results.study.as_ref().ok_or_else(|| EngineError::failed("no study: run study.sweep or study.optimize first"))?;
    let c = emcraft_render::chart::draw(&chart(study, p.string("parameter").optional()?)?, w, h);
    crate::results::write_png(path, c.png())?;
    Ok(json!({"path": path, "width": c.w, "height": c.h}))
}

const OUT_OF_SOLVES: &str = "the optimization ran out of solves";

/// Minimizes `f` over [0, 1]ⁿ from `start` by the Nelder–Mead simplex (trial points clamped into
/// the box): true once the simplex is within `tolerance` of its best vertex in every coordinate.
/// Stops with `f`'s error (`f` refuses past its budget); at most `cap`·(n + 2) iterations.
fn nelder_mead(f: &mut impl FnMut(&[f64]) -> Result<f64>, start: &[f64], tolerance: f64, cap: usize) -> Result<bool> {
    let n = start.len();
    let clamp = |x: Vec<f64>| -> Vec<f64> { x.into_iter().map(|v| v.clamp(0.0, 1.0)).collect() };
    // The first simplex: the start and a step of a quarter of the range along each axis, inwards.
    let mut simplex: Vec<(Vec<f64>, f64, usize)> = Vec::with_capacity(n + 1);
    simplex.push((start.to_vec(), f(start)?, 0));
    for k in 0..n {
        let mut x = start.to_vec();
        x[k] = if x[k] + 0.25 <= 1.0 { x[k] + 0.25 } else { x[k] - 0.25 };
        let fx = f(&x)?;
        simplex.push((x, fx, k + 1));
    }
    let mut age = n + 1;
    let combine = |a: &[f64], b: &[f64], t: f64| -> Vec<f64> { clamp(a.iter().zip(b).map(|(x, y)| x + t * (y - x)).collect()) };
    for _ in 0..cap.saturating_mul(n + 2) {
        // Best first; equal values by age, the older first.
        simplex.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.2.cmp(&b.2)));
        let best = simplex[0].0.clone();
        let spread = simplex.iter().flat_map(|(x, _, _)| x.iter().zip(&best).map(|(a, b)| (a - b).abs())).fold(0.0, f64::max);
        if spread < tolerance {
            return Ok(true);
        }
        let centroid: Vec<f64> = (0..n).map(|k| simplex[..n].iter().map(|v| v.0[k]).sum::<f64>() / n as f64).collect();
        let (worst, f_worst) = (simplex[n].0.clone(), simplex[n].1);
        let (f_best, f_second) = (simplex[0].1, simplex[n - 1].1);
        let reflected = combine(&centroid, &worst, -1.0);
        let f_r = f(&reflected)?;
        age += 1;
        let replacement = if f_r < f_best {
            let expanded = combine(&centroid, &worst, -2.0);
            let f_e = f(&expanded)?;
            if f_e < f_r { Some((expanded, f_e)) } else { Some((reflected, f_r)) }
        } else if f_r < f_second {
            Some((reflected, f_r))
        } else if f_r < f_worst {
            let outside = combine(&centroid, &reflected, 0.5);
            let f_c = f(&outside)?;
            (f_c <= f_r).then_some((outside, f_c))
        } else {
            let inside = combine(&centroid, &worst, 0.5);
            let f_c = f(&inside)?;
            (f_c < f_worst).then_some((inside, f_c))
        };
        match replacement {
            Some((x, fx)) => simplex[n] = (x, fx, age),
            None => {
                // Shrink towards the best vertex.
                for v in simplex.iter_mut().skip(1) {
                    let x = combine(&best, &v.0, 0.5);
                    let fx = f(&x)?;
                    age += 1;
                    *v = (x, fx, age);
                }
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nelder_mead_finds_a_quadratic_minimum_in_the_box() {
        let mut calls = 0;
        let mut f = |x: &[f64]| -> Result<f64> {
            calls += 1;
            Ok((x[0] - 0.3).powi(2) + 2.0 * (x[1] - 0.7).powi(2) + 0.1 * (x[0] - 0.3) * (x[1] - 0.7))
        };
        let mut best = (f64::INFINITY, vec![]);
        let mut g = |x: &[f64]| -> Result<f64> {
            let v = f(x)?;
            if v < best.0 {
                best = (v, x.to_vec());
            }
            Ok(v)
        };
        assert_eq!(nelder_mead(&mut g, &[0.5, 0.5], 1e-6, 400), Ok(true));
        assert!((best.1[0] - 0.3).abs() < 1e-5 && (best.1[1] - 0.7).abs() < 1e-5, "{best:?}");
        // A minimum outside the box ends on its wall.
        let mut edge = (f64::INFINITY, 0.0);
        let mut h = |x: &[f64]| -> Result<f64> {
            let v = (x[0] - 1.5).powi(2);
            if v < edge.0 {
                edge = (v, x[0]);
            }
            Ok(v)
        };
        nelder_mead(&mut h, &[0.2], 1e-6, 200).unwrap();
        assert_eq!(edge.1, 1.0);
    }

    #[test]
    fn nelder_mead_stops_on_its_budget() {
        let mut count = 0;
        let mut f = |x: &[f64]| -> Result<f64> {
            count += 1;
            if count > 5 {
                return Err(EngineError::Failed(OUT_OF_SOLVES.into()));
            }
            Ok(x[0].sin())
        };
        assert!(nelder_mead(&mut f, &[0.5], 1e-9, 5).is_err());
        // A constant: the simplex shrinks to its start within the tolerance.
        assert_eq!(nelder_mead(&mut |_: &[f64]| Ok(1.0), &[0.5, 0.5], 1e-3, 1000), Ok(true));
    }
}

#[cfg(test)]
#[path = "study_tests.rs"]
mod session_tests;

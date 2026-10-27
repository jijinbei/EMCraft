//! One point of a study: the model rebuilt with the variables' values and solved, its reply read
//! into a row.

use emcraft_solver::Complex64 as C;
use serde_json::{Value, json};

use super::variables::{history_with, rebuild};
use super::{Row, is_sparameter};
use crate::project::Effect;
use crate::{EngineError, Params, Result, Session};

/// The solve command a study runs at each point.
pub(super) struct Solve<'a> {
    pub(super) command: &'a str,
    params: Value,
}

pub(super) fn solve_of<'a>(p: &Params<'a>) -> Result<Solve<'a>> {
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
pub(super) fn outputs_of(p: &Params) -> Result<Vec<String>> {
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
pub(super) fn solve_point(s: &Session, set: &[(String, Value)], solve: &Solve, outputs: &[String]) -> Result<Row> {
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
                for (name, v) in map.iter().filter(|(n, _)| is_sparameter(n)) {
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
pub(super) fn frequency_unit(s: &Session) -> String {
    s.display.frequency.symbol().to_owned()
}

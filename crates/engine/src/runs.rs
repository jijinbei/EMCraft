//! The project's runs: the last solve of each solve command, with the parameters it was given
//! and the display units it read them in. They belong to the project (docs/ux/workflow.md §2.7:
//! the project reproduces the model and its runs): a solve that succeeds records its run, the
//! project file keeps them (docs/project-file.md), opening a project restores them without
//! running them, `project.runs` lists them for the Setup step's fields and `solve.rerun` runs one
//! again. Their results are not kept with them: they are solved again.
//!
//! A run's parameters are kept as they were given (expressions of variables too), in the units
//! they were read in; they are shown and run again in the session's units now
//! ([`in_units`]).

use emcraft_model::units::Display;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::cmd::check_keys;
use crate::expr::{self, Variables};
use crate::{CommandSpec, Effect, EngineError, Params, Quantity, Result, Session, find_command};

/// A solve the project ran.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// The solve command (`solve.frequency`, …).
    pub command: String,
    /// Its parameters as given (an object).
    #[serde(default = "no_params")]
    pub params: Value,
    /// The display units its parameters were read in.
    #[serde(default)]
    pub units: Display,
}

fn no_params() -> Value {
    json!({})
}

/// Records `run`: the last of its command, the latest last.
pub(crate) fn record(runs: &mut Vec<Run>, run: Run) {
    runs.retain(|r| r.command != run.command);
    runs.push(run);
}

/// Checks the runs a project file holds: each a solve command, once, with parameters it knows.
/// A file's text is hostile: anything else would run a command the file names.
pub(crate) fn checked(runs: Vec<Run>) -> std::result::Result<Vec<Run>, String> {
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    for (i, mut run) in runs.into_iter().enumerate() {
        let what = || format!("run {}", i + 1);
        let spec = find_command(&run.command)
            .filter(|c| c.effect == Effect::Solve)
            .ok_or_else(|| format!("{}: `{}` is not a solve command", what(), crate::text::clip(&run.command, 64)))?;
        if run.params.is_null() {
            run.params = no_params();
        }
        if !run.params.is_object() {
            return Err(format!("{}: its parameters must be a JSON object", what()));
        }
        check_keys(spec, &run.params).map_err(|e| format!("{}: {e}", what()))?;
        if out.iter().any(|r| r.command == run.command) {
            return Err(format!("{}: `{}` is run twice (a project keeps the last run of each solve)", what(), run.command));
        }
        out.push(run);
    }
    Ok(out)
}

/// `params` of `spec`, given in display units `from`, in display units `to`: each number of a
/// length or frequency parameter (and each number in an array of them) converted; an expression
/// of variables evaluated first when the units differ, so it keeps standing for the same
/// quantity (it stays an expression when they agree). Other parameters are kept as they are.
///
/// ```
/// use emcraft_engine::{find_command, runs::in_units};
/// use emcraft_model::units::{Display, Frequency};
/// use serde_json::json;
///
/// let spec = find_command("solve.frequency").unwrap();
/// let mhz = Display { frequency: Frequency::Megahertz, ..Display::default() };
/// let vars = [("f0".to_owned(), 9.0)].into();
/// let given = json!({"start": 8, "stop": "=f0", "frequencies": [8.5], "order": 2});
/// let shown = in_units(spec, &given, Display::default(), mhz, &vars)?;
/// assert_eq!(shown, json!({"start": 8000.0, "stop": 9000.0, "frequencies": [8500.0], "order": 2}));
/// // Not an object: refused.
/// assert!(in_units(spec, &json!([1]), Display::default(), mhz, &vars).is_err());
/// # Ok::<(), emcraft_engine::EngineError>(())
/// ```
pub fn in_units(spec: &CommandSpec, params: &Value, from: Display, to: Display, vars: &Variables) -> Result<Value> {
    let mut out = match params {
        Value::Object(o) => o.clone(),
        Value::Null => Map::new(),
        _ => return Err(EngineError::Params(format!("{}: parameters must be a JSON object", spec.id))),
    };
    if from == to {
        return Ok(Value::Object(out));
    }
    let reader = Params::new(params, from).with_variables(vars);
    for p in spec.params.iter().filter(|p| p.quantity != Quantity::None) {
        let unit = |x: f64| match p.quantity {
            Quantity::Length => to.length.from_si(from.length.to_si(x)),
            Quantity::Frequency => to.frequency.from_si(from.frequency.to_si(x)),
            Quantity::None => x,
        };
        if let Some(v) = out.get_mut(p.name) {
            *v = converted(&reader, p.name, v, &unit)?;
        }
    }
    Ok(Value::Object(out))
}

/// `v` (a number, an expression or an array of them) through `unit`; anything else as it is.
fn converted(reader: &Params, name: &str, v: &Value, unit: &dyn Fn(f64) -> f64) -> Result<Value> {
    Ok(match v {
        Value::Number(_) => json!(unit(reader.number_of(name, v)?)),
        Value::String(t) if expr::body(t).is_some() => json!(unit(reader.number_of(name, v)?)),
        Value::Array(list) => Value::Array(list.iter().map(|x| converted(reader, name, x, unit)).collect::<Result<_>>()?),
        other => other.clone(),
    })
}

/// A run's parameters in the session's display units now.
pub(crate) fn shown(s: &Session, run: &Run) -> Result<Value> {
    let spec = find_command(&run.command).ok_or_else(|| EngineError::UnknownCommand(crate::text::clip(&run.command, 100)))?;
    in_units(spec, &run.params, run.units, s.display, &s.variables)
}

/// `project.runs`: the project's runs, the latest last, their parameters in display units now.
pub fn list(s: &mut Session, _: &Params) -> Result<Value> {
    let rows = s
        .runs
        .iter()
        .map(|r| Ok(json!({"command": r.command, "params": shown(s, r)?, "units": crate::reply::units(r.units)})))
        .collect::<Result<Vec<Value>>>()?;
    Ok(json!({"runs": rows, "latest": s.runs.last().map(|r| r.command.as_str())}))
}

/// `solve.rerun`: runs a recorded run again (the latest, or `command`'s), as its command runs.
pub fn rerun(s: &mut Session, p: &Params) -> Result<Value> {
    let run = match p.string("command").optional()? {
        Some(c) => s.runs.iter().find(|r| r.command == c).ok_or_else(|| {
            let what = format_args!("has no run of `{}`", crate::text::clip(c, 64));
            p.error("command", crate::text::unknown(what, "runs: ", s.runs.iter().map(|r| r.command.as_str())))
        })?,
        None => s.runs.last().ok_or_else(|| EngineError::failed("no run to run again: solve first (solve.frequency, solve.eigenmodes, …)"))?,
    };
    let (command, params) = (run.command.clone(), shown(s, run)?);
    let mut reply = s.execute(&command, &params)?;
    if let Value::Object(o) = &mut reply {
        o.insert("rerun".into(), json!({"command": command, "params": params}));
    }
    Ok(reply)
}

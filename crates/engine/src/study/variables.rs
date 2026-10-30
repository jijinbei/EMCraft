//! Named variables: defined, changed and removed by commands of the history, which the model is
//! rebuilt from.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::MAX_VARIABLES;
use crate::expr;
use crate::script::DEFINE;
use crate::{EngineError, Params, Result, Session, Step};

/// What a variable may measure (`quantity`).
const QUANTITIES: [&str; 2] = ["length", "frequency"];

/// A variable's name as errors show it: no longer than a name may be.
fn shown(name: &str) -> String {
    crate::text::clip(name, expr::MAX_NAME)
}

/// The variable parameter `param` names, which must be defined.
pub(super) fn defined<'a>(s: &Session, p: &Params<'a>, param: &str) -> Result<&'a str> {
    let name = p.string(param).required()?;
    if !s.variables.contains_key(name) {
        return Err(p.error(param, format!("names no variable: `{}` (define it: project.variable)", shown(name))));
    }
    Ok(name)
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
pub(super) fn history_with(s: &Session, set: &[(String, Value)]) -> Result<Vec<Step>> {
    let mut history = s.history.clone();
    for (name, value) in set {
        let mut found = false;
        for params in history.iter_mut().filter(|step| step.defines(name)).filter_map(|step| step.params.as_object_mut()) {
            params.insert("value".to_owned(), value.clone());
            found = true;
        }
        if !found {
            return Err(EngineError::Params(format!("no variable `{}`: define it first (project.variable)", shown(name))));
        }
    }
    Ok(history)
}

/// A session of the model rebuilt by `history`.
pub(super) fn rebuild(s: &Session, history: &[Step]) -> Result<Session> {
    let mut fresh = s.fresh();
    fresh.run_steps(history)?;
    Ok(fresh)
}

/// Makes `rebuilt` the session, keeping what belongs to the job rather than the model: undo, the
/// file, the saved state, the runs, the last study.
pub(super) fn adopt(s: &mut Session, mut rebuilt: Session) {
    rebuilt.undo = std::mem::take(&mut s.undo);
    rebuilt.path = s.path.take();
    rebuilt.sample = s.sample.take();
    rebuilt.saved = std::mem::take(&mut s.saved);
    rebuilt.runs = std::mem::take(&mut s.runs);
    rebuilt.archive = std::mem::take(&mut s.archive);
    (rebuilt.archive_next, rebuilt.archive_current) = (s.archive_next, None);
    rebuilt.saved_runs = std::mem::take(&mut s.saved_runs);
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
    let quantity = p.string("quantity").optional()?;
    if quantity.is_some_and(|q| !QUANTITIES.contains(&q)) {
        return Err(p.error("quantity", "must be `length` or `frequency`"));
    }
    let mut params = p.value().clone();
    // A change without a quantity keeps the one the definition had.
    if quantity.is_none()
        && let Some(q) = s.history.iter().rev().find(|st| st.defines(name)).and_then(|st| st.param("quantity").cloned())
        && let Some(map) = params.as_object_mut()
    {
        map.insert("quantity".to_owned(), q);
    }
    let step = Step::new(DEFINE, params);
    if s.variables.contains_key(name) {
        // The definition changed in place: the commands after it read the new value.
        let mut history = s.history.clone();
        for old in history.iter_mut().filter(|st| st.defines(name)) {
            *old = step.clone();
        }
        let rebuilt = rebuild(s, &history)?;
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
        return Err(p.error("name", format!("names no variable: `{}`", shown(name))));
    }
    for (i, step) in s.history.iter().enumerate() {
        let mut names = Vec::new();
        references(&step.params, &mut names);
        if names.iter().any(|n| n == name) {
            return Err(EngineError::failed(format!("`{name}` is used by step {} ({}): change that first", i + 1, step.command)));
        }
    }
    s.history.retain(|st| !st.defines(name));
    s.variables.remove(name);
    Ok(json!({"name": name, "variables": s.variables.len()}))
}

/// `project.variables`: the variables, their values, definitions and how many commands use them.
pub fn list(s: &mut Session, _: &Params) -> Result<Value> {
    let mut uses: BTreeMap<&str, usize> = BTreeMap::new();
    for step in &s.history {
        let mut names = Vec::new();
        references(&step.params, &mut names);
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
            let given = s.history.iter().rev().find(|st| st.defines(name)).and_then(|st| st.param("value"));
            let mut row = json!({"name": name, "value": value, "uses": uses.get(name.as_str()).copied().unwrap_or(0)});
            if let Some(Value::String(e)) = given {
                row["expression"] = json!(e);
            }
            let quantity = s.history.iter().rev().find(|st| st.defines(name)).and_then(|st| st.param("quantity")).and_then(Value::as_str);
            if let Some(q) = quantity {
                let d = s.display();
                row["quantity"] = json!(q);
                row["unit"] = json!(if q == "length" { d.length.symbol() } else { d.frequency.symbol() });
            }
            row
        })
        .collect();
    Ok(json!({"variables": rows}))
}

//! `study.sweep`: the model rebuilt and solved at each value of a variable.

use std::sync::Arc;

use serde_json::{Value, json};

use super::point::{frequency_unit, outputs_of, solve_of, solve_point};
use super::variables::defined;
use super::{Kind, MAX_POINTS, Row, Study};
use crate::{Params, Result, Session};

/// `study.sweep`: the model rebuilt and solved at each value of a variable.
pub fn sweep(s: &mut Session, p: &Params) -> Result<Value> {
    let name = defined(s, p, "variable")?;
    let values = values(p)?;
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

/// The values swept: `values`, or `points` from `start` to `stop`.
fn values(p: &Params) -> Result<Vec<f64>> {
    let values: Vec<f64> = if p.has("values") {
        p.list("values", MAX_POINTS).required()?.iter().map(|v| p.number_of("values", v)).collect::<Result<_>>()?
    } else {
        let start = p.number("start").required_or("or give `values`")?;
        let stop = p.number("stop").required_or("or give `values`")?;
        let n = p.integer("points", 1..=MAX_POINTS).or(5)?;
        crate::numbers::evenly_spaced(start, stop, n)
    };
    if values.is_empty() {
        return Err(p.error("values", "must hold at least one value"));
    }
    Ok(values)
}

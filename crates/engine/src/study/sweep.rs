//! `study.sweep`: the model rebuilt and solved at each value of a variable, or at each point of
//! a grid of several variables' values (their Cartesian product, the last variable varying
//! fastest).

use std::sync::Arc;

use serde_json::{Value, json};

use super::point::{frequency_unit, outputs_of, solve_of, solve_point};
use super::variables::defined;
use super::{Kind, MAX_GRID_POINTS, MAX_POINTS, MAX_SWEPT, Row, Study};
use crate::{Params, Result, Session};

/// `study.sweep`: the model rebuilt and solved at each value of a variable, or at each point of
/// a grid of several.
pub fn sweep(s: &mut Session, p: &Params) -> Result<Value> {
    let axes = axes(s, p)?;
    let solve = solve_of(p)?;
    let outputs = outputs_of(p)?;
    let n = axes.iter().try_fold(1usize, |n, (_, v)| n.checked_mul(v.len()).filter(|n| *n <= MAX_GRID_POINTS)).ok_or_else(|| {
        let sizes: Vec<String> = axes.iter().map(|(_, v)| v.len().to_string()).collect();
        p.error("variables", format!("a grid of {} points is too many; the limit is {MAX_GRID_POINTS}", sizes.join(" × ")))
    })?;
    let rows = (0..n)
        .map(|i| {
            // The point's index along each axis, the last varying fastest.
            let mut rest = i;
            let mut values = vec![0.0; axes.len()];
            for (k, (_, v)) in axes.iter().enumerate().rev() {
                values[k] = v.get(rest % v.len()).copied().unwrap_or(f64::NAN);
                rest /= v.len().max(1);
            }
            let label = axes.iter().zip(&values).map(|((name, _), v)| format!("{name} = {v}")).collect::<Vec<_>>().join(", ");
            crate::progress::report(i * 1000, n * 1000, &label)?;
            let set: Vec<(String, Value)> = axes.iter().zip(&values).map(|((name, _), v)| (name.clone(), json!(v))).collect();
            let row = crate::progress::within(i, n, &format!("point {} of {n}: {label}", i + 1), || solve_point(s, &set, &solve, &outputs))?;
            Ok(Row { values, ..row })
        })
        .collect::<Result<Vec<Row>>>()?;
    let study = Study {
        kind: Kind::Sweep,
        variables: axes.iter().map(|(n, _)| n.clone()).collect(),
        grid: axes.into_iter().map(|(_, v)| v).collect(),
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

/// The variables swept and their values: `variable` with `values` or a range, or `variables`,
/// each `{"name", "values"}` or `{"name", "start", "stop", "points"}`.
fn axes(s: &Session, p: &Params) -> Result<Vec<(String, Vec<f64>)>> {
    if !p.has("variables") {
        let name = defined(s, p, "variable")?;
        return Ok(vec![(name.to_owned(), values(p, "values")?)]);
    }
    if p.has("variable") || p.has("values") || p.has("start") || p.has("stop") || p.has("points") {
        return Err(p.error("variables", "give either `variables` (each with its values) or `variable` with `values` or a range, not both"));
    }
    let items = p.items("variables", "variable", MAX_SWEPT)?;
    if items.is_empty() {
        return Err(
            p.error("variables", "must name at least one variable: [{\"name\", \"values\"}] or [{\"name\", \"start\", \"stop\", \"points\"}]")
        );
    }
    let mut axes: Vec<(String, Vec<f64>)> = Vec::new();
    for item in &items {
        let name = defined(s, item, "name")?;
        if axes.iter().any(|(n, _)| n == name) {
            return Err(item.error("name", "is given twice"));
        }
        axes.push((name.to_owned(), values(item, "values")?));
    }
    Ok(axes)
}

/// The values swept: `values`, or `points` from `start` to `stop`.
fn values(p: &Params, key: &str) -> Result<Vec<f64>> {
    let values: Vec<f64> = if p.has(key) {
        p.list(key, MAX_POINTS).required()?.iter().map(|v| p.number_of(key, v)).collect::<Result<_>>()?
    } else {
        let start = p.number("start").required_or("or give `values`")?;
        let stop = p.number("stop").required_or("or give `values`")?;
        let n = p.integer("points", 1..=MAX_POINTS).or(5)?;
        crate::numbers::evenly_spaced(start, stop, n)
    };
    if values.is_empty() {
        return Err(p.error(key, "must hold at least one value"));
    }
    Ok(values)
}

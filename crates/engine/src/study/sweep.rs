//! `study.sweep`: the model rebuilt and solved at each value of a variable, or at each point of
//! a grid of several variables' values (their Cartesian product, the last variable varying
//! fastest). With a `surrogate` budget only that many points are solved, the rest predicted
//! ([`super::adaptive`]).

use std::sync::Arc;

use serde_json::{Value, json};

use super::point::{frequency_unit, outputs_of, solve_of, solve_point};
use super::variables::defined;
use super::{Kind, MAX_GRID_POINTS, MAX_POINTS, MAX_SWEPT, Row, Study, adaptive};
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
    let budget = budget(p, n)?;
    let points: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            // The point's index along each axis, the last varying fastest.
            let mut rest = i;
            let mut values = vec![0.0; axes.len()];
            for (k, (_, v)) in axes.iter().enumerate().rev() {
                values[k] = v.get(rest % v.len()).copied().unwrap_or(f64::NAN);
                rest /= v.len().max(1);
            }
            values
        })
        .collect();
    // Grid point `i`, the `k`-th of `total` solves.
    let session: &Session = s;
    let solve_at = |i: usize, k: usize, total: usize| -> Result<Row> {
        let values = points.get(i).cloned().unwrap_or_default();
        let label = axes.iter().zip(&values).map(|((name, _), v)| format!("{name} = {v}")).collect::<Vec<_>>().join(", ");
        crate::progress::report(k * 1000, total * 1000, &label)?;
        let set: Vec<(String, Value)> = axes.iter().zip(&values).map(|((name, _), v)| (name.clone(), json!(v))).collect();
        let what = if budget.is_some() { "solve" } else { "point" };
        let row =
            crate::progress::within(k, total, &format!("{what} {} of {total}: {label}", k + 1), || solve_point(session, &set, &solve, &outputs))?;
        Ok(Row { values, ..row })
    };
    let (rows, surrogate) = match budget {
        None => ((0..n).map(|i| solve_at(i, i, n)).collect::<Result<Vec<Row>>>()?, None),
        Some(b) => {
            let grid: Vec<Vec<f64>> = axes.iter().map(|(_, v)| v.clone()).collect();
            let u = adaptive::scaled(&points, &grid);
            if (0..n).any(|i| (0..i).any(|j| u[i] == u[j])) {
                return Err(p.error("surrogate", "needs each point of the grid once: a variable's values repeat"));
            }
            let (rows, g) = adaptive::fill(&points, &grid, b, &mut |i, k| solve_at(i, k, b))?;
            (rows, Some(g))
        }
    };
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
        surrogate,
    };
    let reply = study.to_json();
    s.results.study = Some(Arc::new(study));
    Ok(reply)
}

/// The solves of a surrogate sweep (`surrogate`: {"budget": N}, 2 to the grid's points), none
/// for a sweep solving every point.
fn budget(p: &Params, n: usize) -> Result<Option<usize>> {
    let Some(v) = p.raw("surrogate") else { return Ok(None) };
    if !v.is_object() {
        return Err(p.error("surrogate", "must be an object: {\"budget\": 10}"));
    }
    if n < 3 {
        return Err(p.error("surrogate", format!("needs a sweep of 3 points or more, this one has {n}")));
    }
    let g = p.nested(v, "surrogate");
    Ok(Some(g.integer("budget", 2..=n).required()?))
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

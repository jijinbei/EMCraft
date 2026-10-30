//! `study.optimize`: the variables that minimize a goal, by the Nelder–Mead simplex.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value, json};

use super::point::{frequency_unit, solve_of, solve_point};
use super::simplex::{OUT_OF_SOLVES, nelder_mead};
use super::variables::defined;
use super::variables::{adopt, history_with, rebuild};
use super::{Kind, MAX_EVALUATIONS, MAX_OPTIMIZED, Row, Study, is_sparameter};
use crate::{EngineError, Params, Result, Session};

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
            if !is_sparameter(&name) || name.len() > 8 {
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
                Goal::Scalar(crate::text::clip(name, 64), ty == "maximize")
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
    /// The value at `u` along the range, from 0 (`min`) to 1 (`max`).
    fn value(&self, u: f64) -> f64 {
        self.min + (self.max - self.min) * u.clamp(0.0, 1.0)
    }

    /// Where the variable's value `now` lies along the range, from 0 to 1.
    fn position(&self, now: Option<f64>) -> f64 {
        ((now.unwrap_or(self.min) - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }
}

/// The variables varied (`variables`): defined, each once, `max` above `min`.
fn bounds(s: &Session, p: &Params) -> Result<Vec<Bound>> {
    let items = p.items("variables", "variable", MAX_OPTIMIZED)?;
    if items.is_empty() {
        return Err(p.error("variables", "must name at least one variable: [{\"name\", \"min\", \"max\"}]"));
    }
    let mut bounds = Vec::new();
    for item in &items {
        let name = defined(s, item, "name")?;
        if bounds.iter().any(|b: &Bound| b.name == name) {
            return Err(item.error("name", "is given twice"));
        }
        let (min, max) = (item.number("min").required()?, item.number("max").required()?);
        if max <= min {
            return Err(item.error("max", "must be above `min`"));
        }
        bounds.push(Bound { name: name.to_owned(), min, max });
    }
    Ok(bounds)
}

/// The row of the lowest goal (the earliest of equals); an error saying why the first point
/// failed when none was solved.
fn best_row(rows: &[Row]) -> Result<usize> {
    rows.iter()
        .enumerate()
        .filter_map(|(i, r)| r.objective.map(|o| (i, o)))
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(i, _)| i)
        .ok_or_else(|| EngineError::failed(format!("no point could be solved: {}", rows.first().and_then(|r| r.error.clone()).unwrap_or_default())))
}

/// `study.optimize`: the variables that minimize a goal, by the Nelder–Mead simplex; the model
/// left at the best point.
pub fn optimize(s: &mut Session, p: &Params) -> Result<Value> {
    let bounds = bounds(s, p)?;
    let goal = Goal::read(p)?;
    let solve = solve_of(p)?;
    let cap = p.integer("max_evaluations", 2..=MAX_EVALUATIONS).or(50)?;
    let tolerance = p.number("tolerance").check(|t| (1e-9..=0.5).contains(t), "from 1e-9 to 0.5").or(1e-3)?;

    // The search, in [0, 1] per variable from the current values.
    let start: Vec<f64> = bounds.iter().map(|b| b.position(s.variables.get(&b.name).copied())).collect();
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
            return Err(EngineError::failed(OUT_OF_SOLVES));
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
    let best = best_row(&rows)?;
    let unit = frequency_unit(s);
    let study = Study {
        kind: Kind::Optimize,
        variables: bounds.iter().map(|b| b.name.clone()).collect(),
        grid: Vec::new(),
        solve: solve.command.to_owned(),
        frequency_unit: unit.clone(),
        goal: Some(goal.describe(&unit)),
        rows,
        best: Some(best),
        converged,
    };
    // The model at the optimum: the definitions given the best values, the model rebuilt.
    let set: Vec<(String, Value)> = study.variables.iter().zip(&study.rows[best].values).map(|(n, v)| (n.clone(), json!(v))).collect();
    let rebuilt = rebuild(s, &history_with(s, &set)?)?;
    let reply = study.to_json();
    adopt(s, rebuilt);
    s.results.study = Some(Arc::new(study));
    Ok(reply)
}

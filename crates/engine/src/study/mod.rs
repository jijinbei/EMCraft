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
//!
//! Surrogates cut the solves: `study.optimize` `method: bayesian` lets a Gaussian process of the
//! values solved choose each next solve by its expected improvement (`bayes`, [`gp`]), and
//! `study.sweep` `surrogate` solves a budget of the grid's points and predicts the others with
//! their σ (`adaptive`). Every value reported as solved is a solve.

mod adaptive;
mod bayes;
pub mod gp;
mod optimize;
mod point;
mod simplex;
mod sweep;
mod variables;

use emcraft_model::units::decibels;
use emcraft_solver::Complex64 as C;
use serde_json::{Map, Value, json};

pub use self::optimize::optimize;
pub use self::sweep::sweep;
pub use self::variables::{delete, list, variable};
pub use crate::view::study::{Slice, chart};

/// The most variables a session holds.
pub const MAX_VARIABLES: usize = 200;
/// The most values one variable of a sweep takes.
pub const MAX_POINTS: usize = 101;
/// The most variables one sweep varies on a grid.
pub const MAX_SWEPT: usize = 4;
/// The most points (solves) one sweep's grid holds.
pub const MAX_GRID_POINTS: usize = 400;
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
    /// A surrogate sweep's prediction where it did not solve: |S| in dB and the σs (the modes'
    /// and numbers' means are in `modes` and `scalars`).
    pub prediction: Option<Predicted>,
}

/// What a surrogate predicted at a point it did not solve, with its standard deviations.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Predicted {
    /// Each S-parameter's |S| in dB and its σ at each frequency.
    pub db: Vec<(String, Vec<f64>, Vec<f64>)>,
    /// The σ of each eigenmode's frequency.
    pub modes_sigma: Vec<f64>,
    /// The σ of each of the solve's numbers.
    pub scalars_sigma: Vec<(String, f64)>,
}

/// The Gaussian-process surrogate a study chose its points by.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Surrogate {
    /// The points solved.
    pub solves: usize,
    /// Its length scales, a share of each variable's range.
    pub lengths: Vec<f64>,
    /// An optimization's prediction at its best point: the goal's mean and σ.
    pub at_best: Option<(f64, f64)>,
}

impl Row {
    /// |`name`| in dB at each frequency.
    pub fn db(&self, name: &str) -> Option<Vec<f64>> {
        if let Some(p) = &self.prediction {
            return p.db.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name)).map(|(_, m, _)| m.clone());
        }
        self.s.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.iter().map(|c| db(*c)).collect())
    }

    /// The σ of a predicted |`name`| in dB at each frequency (none for a row solved).
    pub fn db_sigma(&self, name: &str) -> Option<Vec<f64>> {
        self.prediction.as_ref()?.db.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name)).map(|(_, _, s)| s.clone())
    }

    /// Whether the row is a surrogate's prediction rather than a solve.
    pub fn is_predicted(&self) -> bool {
        self.prediction.is_some()
    }

    /// The row in a reply: a one-variable sweep's `value`, else every variable's in `values`.
    fn to_json(&self, kind: Kind, swept: usize) -> Value {
        let mut row = Map::new();
        match kind {
            Kind::Sweep if swept == 1 => row.insert("value".into(), json!(self.values.first())),
            Kind::Sweep | Kind::Optimize => row.insert("values".into(), json!(self.values)),
        };
        if let Some(o) = self.objective {
            row.insert("objective".into(), json!(o));
        }
        if let Some(e) = &self.error {
            row.insert("error".into(), json!(e));
        }
        if let Some(p) = &self.prediction {
            p.to_json(self, &mut row);
            return Value::Object(row);
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
                            pt.insert(name.clone(), crate::reply::complex(*c));
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

impl Predicted {
    /// A predicted row's reply: `predicted` true, each point's |S| as `db` with its `sigma_db`,
    /// the modes and numbers with their σs.
    fn to_json(&self, row: &Row, out: &mut Map<String, Value>) {
        out.insert("predicted".into(), json!(true));
        if !row.frequencies.is_empty() && !self.db.is_empty() {
            let points: Vec<Value> = row
                .frequencies
                .iter()
                .enumerate()
                .map(|(k, f)| {
                    let mut pt = Map::new();
                    pt.insert("frequency".into(), json!(f));
                    for (name, m, s) in &self.db {
                        if let (Some(m), Some(s)) = (m.get(k), s.get(k)) {
                            pt.insert(name.clone(), json!({"db": m, "sigma_db": s}));
                        }
                    }
                    Value::Object(pt)
                })
                .collect();
            out.insert("points".into(), json!(points));
        }
        if !row.modes.is_empty() {
            out.insert("modes".into(), json!(row.modes));
            out.insert("modes_sigma".into(), json!(self.modes_sigma));
        }
        if !row.scalars.is_empty() {
            out.insert("scalars".into(), Value::Object(row.scalars.iter().map(|(k, v)| (k.clone(), json!(v))).collect()));
            out.insert("scalars_sigma".into(), Value::Object(self.scalars_sigma.iter().map(|(k, v)| (k.clone(), json!(v))).collect()));
        }
    }
}

/// A parameter study's results.
#[derive(Clone, Debug)]
pub struct Study {
    pub kind: Kind,
    /// The variables varied, in the rows' order of values.
    pub variables: Vec<String>,
    /// A sweep's values of each variable (the grid's axes, in `variables`' order; the rows run
    /// over their Cartesian product, the last variable fastest); empty for an optimization.
    pub grid: Vec<Vec<f64>>,
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
    /// The surrogate that chose the points: a sweep's `surrogate`, a Bayesian optimization.
    pub surrogate: Option<Surrogate>,
}

impl Study {
    /// Whether the study is a sweep over a grid of several variables.
    pub fn is_grid(&self) -> bool {
        self.kind == Kind::Sweep && self.variables.len() > 1
    }

    /// The rows of a grid sweep along variable `free`, every other variable at its value
    /// `at[k]` (an index into its axis; the first where missing), in the free variable's order.
    pub fn slice(&self, free: usize, at: &[usize]) -> Vec<&Row> {
        let held: Vec<Option<f64>> =
            (0..self.variables.len())
                .map(|k| {
                    if k == free {
                        None
                    } else {
                        self.grid.get(k).and_then(|axis| axis.get(at.get(k).copied().unwrap_or(0)).or(axis.first())).copied()
                    }
                })
                .collect();
        self.rows.iter().filter(|r| held.iter().zip(&r.values).all(|(h, v)| h.is_none_or(|h| h.to_bits() == v.to_bits()))).collect()
    }

    /// The S-parameters' names the rows hold.
    pub fn parameters(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let predicted = self.rows.iter().flat_map(|r| r.prediction.iter().flat_map(|p| p.db.iter().map(|(n, _, _)| n)));
        for n in self.rows.iter().flat_map(|r| r.s.iter().map(|(n, _)| n)).chain(predicted) {
            if !names.contains(n) {
                names.push(n.clone());
            }
        }
        names
    }

    pub fn to_json(&self) -> Value {
        let rows: Vec<Value> = self.rows.iter().map(|r| r.to_json(self.kind, self.variables.len())).collect();
        let mut reply = json!({"solve": self.solve, "frequency_unit": self.frequency_unit, "rows": rows});
        match self.kind {
            Kind::Sweep if self.variables.len() == 1 => reply["variable"] = json!(self.variables.first()),
            Kind::Sweep => {
                reply["variables"] = json!(self.variables);
                reply["grid"] = Value::Object(self.variables.iter().cloned().zip(self.grid.iter().map(|v| json!(v))).collect());
            }
            Kind::Optimize => {
                reply["method"] = json!(if self.surrogate.is_some() { "bayesian" } else { "simplex" });
                reply["variables"] = json!(self.variables);
                reply["goal"] = json!(self.goal);
                reply["evaluations"] = json!(self.rows.len());
                reply["converged"] = json!(self.converged);
                if let Some(b) = self.best.and_then(|b| self.rows.get(b)) {
                    reply["best"] = json!({"values": b.values, "objective": b.objective});
                }
            }
        }
        if let Some(g) = &self.surrogate {
            let mut out = json!({"length_scales": g.lengths});
            if self.kind == Kind::Sweep {
                out["solved"] = json!(g.solves);
                out["predicted"] = json!(self.rows.iter().filter(|r| r.is_predicted()).count());
            }
            if let Some((m, s)) = g.at_best {
                // The surrogate's goal at the best point, beside the solve's.
                reply["best"]["predicted"] = json!({"objective": m, "sigma": s});
            }
            reply["surrogate"] = out;
        }
        reply
    }
}

fn db(c: C) -> f64 {
    decibels(c.norm().max(1e-15))
}

/// Whether `name` names an S-parameter as replies do: `s` and port numbers (`s11`, `s21`).
fn is_sparameter(name: &str) -> bool {
    name.len() >= 3 && name.starts_with('s') && name[1..].bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod surrogate_tests;
#[cfg(test)]
mod tests;

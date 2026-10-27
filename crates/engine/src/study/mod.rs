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
pub use crate::view::study::chart;

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

/// Whether `name` names an S-parameter as replies do: `s` and port numbers (`s11`, `s21`).
fn is_sparameter(name: &str) -> bool {
    name.len() >= 3 && name.starts_with('s') && name[1..].bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests;

//! `study.tolerance`: Monte Carlo tolerance analysis. Each variable is drawn about its value as
//! it stands (normal with its σ, or uniform within ± its tolerance), independently, `samples`
//! times; the model is rebuilt and solved at each draw ([`super::point::solve_point`]) and the
//! S-parameters gathered: their mean, spread and extremes in dB at each frequency, and with a
//! specification (|S| at most or at least so many dB over a band) the yield — the share of the
//! draws that meet it — with its 95 % Wilson score interval (E. B. Wilson, "Probable inference,
//! the law of succession, and statistical inference", J. Amer. Statist. Assoc. 22(158):209–212,
//! 1927).
//!
//! The draws come from a generator of their own, SplitMix64 (G. L. Steele, D. Lea and C. H.
//! Flood, "Fast splittable pseudorandom number generators", OOPSLA 2014), seeded by `seed`: the
//! same seed draws the same points on every machine; normal deviates by the Box–Muller
//! transform (G. E. P. Box and M. E. Muller, "A note on the generation of random normal
//! deviates", Ann. Math. Statist. 29(2):610–611, 1958).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::point::{frequency_unit, outputs_of, solve_of, solve_point};
use super::variables::defined;
use super::{Kind, MAX_GRID_POINTS, MAX_OPTIMIZED, Row, Study};
use crate::{Params, Result, Session};

/// The most specifications one analysis checks.
pub const MAX_SPECS: usize = 16;
/// The draws when `samples` is not given.
const DEFAULT_SAMPLES: usize = 50;
/// z of a two-sided 95 % interval.
const Z95: f64 = 1.959_963_984_540_054;

/// How a variable is drawn about its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Distribution {
    /// Normal, standard deviation `width`.
    Normal,
    /// Uniform within ± `width`.
    Uniform,
}

/// A variable drawn: its name, value as it stood (display units) and how it spreads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spread {
    pub name: String,
    pub nominal: f64,
    pub distribution: Distribution,
    /// σ (normal) or the half-width (uniform), in the variable's units.
    pub width: f64,
}

/// A bound an S-parameter must keep over a band (display frequency units; the whole sweep when
/// left open).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spec {
    pub output: String,
    /// |S| at most this many dB (`max_db`) when true, at least (`min_db`) when false.
    pub at_most: bool,
    pub db: f64,
    pub start: Option<f64>,
    pub stop: Option<f64>,
}

impl Spec {
    /// Whether `row` meets it; `None` when the row holds none of its frequencies.
    fn met(&self, row: &Row) -> Option<bool> {
        let db = row.db(&self.output)?;
        let inside = |f: f64| self.start.is_none_or(|a| f >= a) && self.stop.is_none_or(|b| f <= b);
        let mut seen = false;
        for (f, d) in row.frequencies.iter().zip(&db) {
            if !inside(*f) {
                continue;
            }
            seen = true;
            let ok = if self.at_most { *d <= self.db } else { *d >= self.db };
            if !ok {
                return Some(false);
            }
        }
        seen.then_some(true)
    }

    /// In words: `|s11| ≤ −15 dB from 9 to 11 GHz`.
    fn describe(&self, unit: &str) -> String {
        let band = match (self.start, self.stop) {
            (Some(a), Some(b)) => format!(" from {a} to {b} {unit}"),
            (Some(a), None) => format!(" from {a} {unit}"),
            (None, Some(b)) => format!(" up to {b} {unit}"),
            (None, None) => String::new(),
        };
        format!("|{}| {} {} dB{band}", self.output, if self.at_most { "≤" } else { "≥" }, self.db)
    }
}

/// What a tolerance analysis drew and checked: the spreads, the specifications and the seed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Setup {
    pub spreads: Vec<Spread>,
    pub specs: Vec<Spec>,
    pub seed: u64,
}

/// SplitMix64: a 64-bit state stepped by the golden ratio and mixed.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in (0, 1]: the top 53 bits, plus one, over 2^53.
    fn unit(&mut self) -> f64 {
        ((self.next() >> 11) + 1) as f64 / (1u64 << 53) as f64
    }

    /// A standard normal deviate (Box–Muller, the cosine branch).
    fn normal(&mut self) -> f64 {
        let (u1, u2) = (self.unit(), self.unit());
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }

    /// A deviate of `spread`.
    fn draw(&mut self, spread: &Spread) -> f64 {
        match spread.distribution {
            Distribution::Normal => spread.width * self.normal(),
            Distribution::Uniform => spread.width * (2.0 * self.unit() - 1.0),
        }
    }
}

/// The 95 % Wilson score interval of a share `k` of `n`.
///
/// ```
/// use emcraft_engine::study::wilson;
/// let (lo, hi) = wilson(45, 50);
/// assert!(lo < 0.9 && 0.9 < hi && hi < 1.0);
/// assert_eq!(wilson(0, 0), (0.0, 1.0));
/// ```
pub fn wilson(k: usize, n: usize) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let (n, p) = (n as f64, k as f64 / n as f64);
    let z2 = Z95 * Z95;
    let centre = (p + z2 / (2.0 * n)) / (1.0 + z2 / n);
    let half = Z95 * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / (1.0 + z2 / n);
    // None or all of them: the bound on that side is the share itself, exactly.
    let lo = if k == 0 { 0.0 } else { (centre - half).max(0.0) };
    let hi = if k as f64 == n { 1.0 } else { (centre + half).min(1.0) };
    (lo, hi)
}

/// `study.tolerance`.
pub fn tolerance(s: &mut Session, p: &Params) -> Result<Value> {
    let spreads = spreads(s, p)?;
    let specs = specs(p)?;
    let n = p.integer("samples", 2..=MAX_GRID_POINTS).or(DEFAULT_SAMPLES)?;
    let seed = p.integer("seed", 0..=u32::MAX as usize).or(1)? as u64;
    let solve = solve_of(p)?;
    let outputs = outputs_of(p)?;
    let mut rng = SplitMix(seed);
    let draws: Vec<Vec<f64>> = (0..n).map(|_| spreads.iter().map(|sp| sp.nominal + rng.draw(sp)).collect()).collect();
    let session: &Session = s;
    let rows = draws
        .iter()
        .enumerate()
        .map(|(k, values)| {
            let set: Vec<(String, Value)> = spreads.iter().zip(values).map(|(sp, v)| (sp.name.clone(), json!(v))).collect();
            crate::progress::within(k, n, &format!("sample {} of {n}", k + 1), || solve_point(session, &set, &solve, &outputs))
                .map(|row| Row { values: values.clone(), ..row })
        })
        .collect::<Result<Vec<Row>>>()?;
    let unit = frequency_unit(s);
    let goal = (!specs.is_empty()).then(|| specs.iter().map(|sp| sp.describe(&unit)).collect::<Vec<_>>().join(" and "));
    let study = Study {
        kind: Kind::Tolerance,
        variables: spreads.iter().map(|sp| sp.name.clone()).collect(),
        grid: Vec::new(),
        solve: solve.command.to_owned(),
        frequency_unit: unit,
        goal,
        rows,
        best: None,
        converged: true,
        surrogate: None,
        tolerance: Some(Box::new(Setup { spreads, specs, seed })),
    };
    let reply = study.to_json();
    s.results.study = Some(Arc::new(study));
    Ok(reply)
}

/// The variables drawn: `variables`, each `{"name", "sigma"}` (normal) or `{"name",
/// "tolerance"}` (uniform within ±), about its value as it stands.
fn spreads(s: &Session, p: &Params) -> Result<Vec<Spread>> {
    let items = p.items("variables", "variable", MAX_OPTIMIZED)?;
    if items.is_empty() {
        return Err(
            p.error("variables", "must name at least one variable: [{\"name\": \"w\", \"sigma\": 0.05}] or [{\"name\": \"w\", \"tolerance\": 0.1}]")
        );
    }
    let mut out: Vec<Spread> = Vec::new();
    for item in &items {
        let name = defined(s, item, "name")?;
        if out.iter().any(|sp| sp.name == name) {
            return Err(item.error("name", "is given twice"));
        }
        let (distribution, key) = match (item.has("sigma"), item.has("tolerance")) {
            (true, false) => (Distribution::Normal, "sigma"),
            (false, true) => (Distribution::Uniform, "tolerance"),
            _ => return Err(item.error("sigma", "give either `sigma` (normal) or `tolerance` (uniform within ±), one of them")),
        };
        let width = item.positive(key).required()?;
        let nominal = s.variables.get(name).copied().ok_or_else(|| item.error("name", "has no value"))?;
        out.push(Spread { name: name.to_owned(), nominal, distribution, width });
    }
    Ok(out)
}

/// The specifications: `spec`, each `{"output", "max_db" or "min_db", "start"?, "stop"?}`.
fn specs(p: &Params) -> Result<Vec<Spec>> {
    let items = if p.has("spec") { p.items("spec", "specification", MAX_SPECS)? } else { Vec::new() };
    items
        .iter()
        .map(|item| {
            let output = item.string("output").required()?.to_ascii_lowercase();
            if !super::is_sparameter(&output) {
                return Err(item.error("output", "must name an S-parameter such as \"s11\""));
            }
            let (at_most, db) = match (item.number("max_db").optional()?, item.number("min_db").optional()?) {
                (Some(d), None) => (true, d),
                (None, Some(d)) => (false, d),
                _ => return Err(item.error("max_db", "give either `max_db` (|S| at most) or `min_db` (|S| at least)")),
            };
            if !db.is_finite() {
                return Err(item.error(if at_most { "max_db" } else { "min_db" }, "must be a finite number of dB"));
            }
            let (start, stop) = (item.number("start").optional()?, item.number("stop").optional()?);
            if let (Some(a), Some(b)) = (start, stop)
                && !(a <= b)
            {
                return Err(item.error("stop", "must not be below `start`"));
            }
            Ok(Spec { output, at_most, db, start, stop })
        })
        .collect()
}

/// An S-parameter's spread over the draws at one frequency (dB).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spreading {
    pub frequency: f64,
    pub mean: f64,
    pub sigma: f64,
    pub min: f64,
    pub max: f64,
}

/// What a tolerance analysis found: each S-parameter's spread at each frequency (over the rows
/// solved, alike in their frequencies), and the yield (rows meeting every specification, of
/// those solved), when there are specifications.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Statistics {
    pub spreads: Vec<(String, Vec<Spreading>)>,
    pub passed: usize,
    pub solved: usize,
}

impl Statistics {
    pub fn of(study: &Study) -> Statistics {
        let ok: Vec<&Row> = study.rows.iter().filter(|r| r.error.is_none()).collect();
        let freqs = ok.first().map_or_default(|r| r.frequencies.clone());
        let alike: Vec<&Row> = ok.iter().copied().filter(|r| r.frequencies == freqs).collect();
        let spreads = study
            .parameters()
            .into_iter()
            .map(|name| {
                let dbs: Vec<Vec<f64>> = alike.iter().filter_map(|r| r.db(&name)).collect();
                let at = freqs
                    .iter()
                    .enumerate()
                    .map(|(k, f)| {
                        let xs: Vec<f64> = dbs.iter().filter_map(|d| d.get(k).copied()).collect();
                        let n = xs.len().max(1) as f64;
                        let mean = xs.iter().sum::<f64>() / n;
                        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
                        let min = xs.iter().copied().fold(f64::INFINITY, f64::min);
                        let max = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                        Spreading { frequency: *f, mean, sigma: var.sqrt(), min, max }
                    })
                    .collect();
                (name, at)
            })
            .collect();
        let specs = study.tolerance.as_ref().map_or_default(|t| t.specs.as_slice());
        let passed = ok.iter().filter(|r| !specs.is_empty() && specs.iter().all(|sp| sp.met(r) == Some(true))).count();
        Statistics { spreads, passed, solved: ok.len() }
    }

    /// The reply's `statistics` and, with specifications, `yield`.
    pub(super) fn to_json(&self, study: &Study) -> (Value, Option<Value>) {
        let stats: serde_json::Map<String, Value> = self
            .spreads
            .iter()
            .map(|(name, at)| {
                let points: Vec<Value> = at
                    .iter()
                    .map(|s| json!({"frequency": s.frequency, "mean_db": s.mean, "sigma_db": s.sigma, "min_db": s.min, "max_db": s.max}))
                    .collect();
                (name.clone(), Value::Array(points))
            })
            .collect();
        let has_specs = study.tolerance.as_ref().is_some_and(|t| !t.specs.is_empty());
        let yielded = has_specs.then(|| {
            let (lo, hi) = wilson(self.passed, self.solved);
            let share = if self.solved == 0 { 0.0 } else { self.passed as f64 / self.solved as f64 };
            json!({"passed": self.passed, "solved": self.solved, "fraction": share, "low": lo, "high": hi})
        });
        (Value::Object(stats), yielded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_draws_have_their_distributions_and_repeat_with_their_seed() {
        // Derived here: N(0, 1) has mean 0 and variance 1; U(−1, 1) has variance 1/3. With 20 000
        // draws the sample moments lie within a few standard errors (1/√N ≈ 0.007).
        let normal = Spread { name: "x".into(), nominal: 0.0, distribution: Distribution::Normal, width: 1.0 };
        let uniform = Spread { distribution: Distribution::Uniform, ..normal.clone() };
        for (sp, var) in [(&normal, 1.0), (&uniform, 1.0 / 3.0)] {
            let mut rng = SplitMix(7);
            let xs: Vec<f64> = (0..20_000).map(|_| rng.draw(sp)).collect();
            let mean = xs.iter().sum::<f64>() / xs.len() as f64;
            let v = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (xs.len() - 1) as f64;
            assert!(mean.abs() < 0.03 && (v - var).abs() < 0.05 * var, "{mean} {v}");
            assert!(xs.iter().all(|x| x.is_finite()));
        }
        let draws = |seed| {
            let mut r = SplitMix(seed);
            (0..5).map(|_| r.normal()).collect::<Vec<f64>>()
        };
        assert_eq!(draws(3), draws(3));
        assert_ne!(draws(3), draws(4));
    }

    #[test]
    fn the_wilson_interval_holds_its_share() {
        // Wilson 1927: for 45 of 50 the interval is about 0.786 to 0.957.
        let (lo, hi) = wilson(45, 50);
        assert!((lo - 0.786).abs() < 2e-3 && (hi - 0.957).abs() < 2e-3, "{lo} {hi}");
        assert_eq!(wilson(0, 10).0, 0.0);
        assert_eq!(wilson(10, 10).1, 1.0);
    }

    #[test]
    fn a_spec_reads_its_band() {
        let row = Row {
            frequencies: vec![8.0, 10.0, 12.0],
            s: vec![(
                "s11".into(),
                vec![emcraft_solver::Complex64::new(0.5, 0.0), emcraft_solver::Complex64::new(0.05, 0.0), emcraft_solver::Complex64::new(0.5, 0.0)],
            )],
            ..Row::default()
        };
        let spec = |start, stop| Spec { output: "s11".into(), at_most: true, db: -20.0, start, stop };
        assert_eq!(spec(Some(9.0), Some(11.0)).met(&row), Some(true));
        assert_eq!(spec(None, None).met(&row), Some(false));
        assert_eq!(spec(Some(13.0), None).met(&row), None);
    }
}

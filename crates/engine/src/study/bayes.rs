//! Bayesian optimization over the unit box: efficient global optimization (D. R. Jones,
//! M. Schonlau and W. J. Welch, "Efficient global optimization of expensive black-box
//! functions", J. Global Optim. 13:455–492, 1998). A space-filling start — the current values and
//! the first 2d Halton points — then, while solves remain, a Gaussian-process surrogate
//! ([`super::gp`]) of every value solved, and the point of the largest expected improvement on
//! the best value solved: found over a fixed set of Halton candidates, the best few of them and
//! the best point solved refined by a compass search of halving steps. The search stops when that
//! improvement falls below `tolerance` × the spread of the values solved, when the point it asks
//! for was solved already, or when the solves run out. Every value it reports is a solve; the
//! surrogate only chooses where to solve.

use super::gp::{Gp, MAX_DATA, MAX_DIMENSION, expected_improvement, halton};
use crate::{EngineError, Result};

/// The most Halton candidates the expected improvement is searched over.
pub const MAX_CANDIDATES: usize = 2048;
/// Candidates per variable.
const CANDIDATES_PER_VARIABLE: usize = 256;
/// The candidates refined by the compass search, besides the best point solved.
const REFINED: usize = 3;
/// The compass search's first and last steps, and its most moves.
const FIRST_STEP: f64 = 0.0625;
const LAST_STEP: f64 = 1e-4;
const MAX_MOVES: usize = 200;
/// After this many points the length scales are fitted again only every [`REFIT_EVERY`] solves.
const REFIT_ALWAYS: usize = 60;
const REFIT_EVERY: usize = 10;

/// How a Bayesian search ended.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    /// True when the expected improvement fell below the tolerance (else the solves ran out).
    pub converged: bool,
    /// The surrogate of every value solved, at the best point: its mean and σ.
    pub at_best: Option<(f64, f64)>,
    /// Its length scales, a share of each variable's range.
    pub lengths: Vec<f64>,
}

/// The points solved and their values (+∞ where a solve failed).
struct Solved {
    x: Vec<Vec<f64>>,
    f: Vec<f64>,
}

impl Solved {
    fn has(&self, u: &[f64]) -> bool {
        self.x.iter().any(|p| p.iter().zip(u).all(|(a, b)| a.to_bits() == b.to_bits()))
    }

    /// The best value solved and where (the earliest of equals).
    fn best(&self) -> Option<(usize, f64)> {
        self.f.iter().copied().enumerate().filter(|(_, v)| v.is_finite()).min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
    }

    /// The data a surrogate is fitted to: failed points at the worst value solved, so the search
    /// keeps away from them.
    fn data(&self) -> Option<(Vec<Vec<f64>>, Vec<Vec<f64>>)> {
        let worst = self.f.iter().copied().filter(|v| v.is_finite()).fold(f64::NEG_INFINITY, f64::max);
        if self.f.iter().filter(|v| v.is_finite()).count() < 2 {
            return None;
        }
        Some((self.x.clone(), self.f.iter().map(|v| vec![if v.is_finite() { *v } else { worst }]).collect()))
    }
}

/// Minimizes `f` over [0, 1]ⁿ from `start` by Bayesian optimization with at most `cap` calls of
/// `f`, each at a point not called before; stops with `f`'s error.
pub fn bayesian(f: &mut impl FnMut(&[f64]) -> Result<f64>, start: &[f64], tolerance: f64, cap: usize) -> Result<Outcome> {
    let d = start.len();
    if !(1..=MAX_DIMENSION).contains(&d) {
        return Err(EngineError::Params(format!("the Bayesian search varies 1 to {MAX_DIMENSION} variables, got {d}")));
    }
    if !start.iter().all(|v| v.is_finite()) || !(tolerance.is_finite() && tolerance > 0.0) {
        return Err(EngineError::Params("the Bayesian search needs a finite start and a tolerance > 0".into()));
    }
    let cap = cap.min(MAX_DATA);
    let mut solved = Solved { x: Vec::new(), f: Vec::new() };
    let mut call = |u: Vec<f64>, solved: &mut Solved| -> Result<()> {
        let u: Vec<f64> = u.into_iter().map(|v| v.clamp(0.0, 1.0)).collect();
        if solved.has(&u) || solved.x.len() >= cap {
            return Ok(());
        }
        let v = f(&u)?;
        solved.x.push(u);
        solved.f.push(if v.is_nan() { f64::INFINITY } else { v });
        Ok(())
    };
    // The start: the current values and 2d Halton points.
    call(start.to_vec(), &mut solved)?;
    for i in 1..=2 * d {
        call(halton(i, d), &mut solved)?;
    }
    let candidates: Vec<Vec<f64>> = (1..=(CANDIDATES_PER_VARIABLE * d).min(MAX_CANDIDATES)).map(|i| halton(i, d)).collect();
    // More Halton points for when too few solves succeed to fit a surrogate.
    let mut spare = 2 * d + 1;
    let mut lengths: Vec<f64> = Vec::new();
    let mut converged = false;
    // Each turn solves a point or stops: at most `cap` turns.
    for _ in 0..cap {
        if solved.x.len() >= cap {
            break;
        }
        let (Some((_, best)), Some((x, y))) = (solved.best(), solved.data()) else {
            call(halton(spare, d), &mut solved)?;
            spare += 1;
            continue;
        };
        let refit = lengths.is_empty() || x.len() <= REFIT_ALWAYS || x.len() % REFIT_EVERY == 0;
        let gp = match if refit { Gp::fit(&x, &y) } else { Gp::with_lengths(&x, &y, &lengths) } {
            Ok(gp) => gp,
            Err(_) => {
                call(halton(spare, d), &mut solved)?;
                spare += 1;
                continue;
            }
        };
        lengths = gp.lengths().to_vec();
        let ei = |u: &[f64]| -> f64 { gp.predict(u).map_or(0.0, |p| expected_improvement(best, p.mean[0], p.sigma[0])) };
        let (next, gain) = maximize(&ei, &candidates, &solved, d);
        let spread = solved.f.iter().copied().filter(|v| v.is_finite()).fold(f64::NEG_INFINITY, f64::max) - best;
        let floor = tolerance * if spread > 0.0 { spread } else { best.abs().max(1.0) };
        if gain < floor || solved.has(&next) {
            converged = true;
            break;
        }
        call(next, &mut solved)?;
    }
    let at_best = match (solved.best(), solved.data()) {
        (Some((b, _)), Some((x, y))) => Gp::fit(&x, &y).ok().and_then(|gp| {
            lengths = gp.lengths().to_vec();
            let p = gp.predict(&solved.x[b]).ok()?;
            Some((p.mean[0], p.sigma[0]))
        }),
        _ => None,
    };
    Ok(Outcome { converged, at_best, lengths })
}

/// The point of the largest `ei` (and its value): the best of the candidates, then the best
/// `REFINED` of them and the best point solved each refined by a compass search.
fn maximize(ei: &impl Fn(&[f64]) -> f64, candidates: &[Vec<f64>], solved: &Solved, d: usize) -> (Vec<f64>, f64) {
    let mut scored: Vec<(usize, f64)> = candidates.iter().map(|c| ei(c)).enumerate().collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut starts: Vec<(Vec<f64>, f64)> = scored.iter().take(REFINED).filter_map(|(i, v)| Some((candidates.get(*i)?.clone(), *v))).collect();
    if let Some((b, _)) = solved.best() {
        let x = solved.x[b].clone();
        let v = ei(&x);
        starts.push((x, v));
    }
    let mut top: (Vec<f64>, f64) = (vec![0.5; d], f64::NEG_INFINITY);
    for (mut x, mut v) in starts {
        let mut step = FIRST_STEP;
        let mut moves = 0;
        while step >= LAST_STEP && moves < MAX_MOVES {
            let mut moved = false;
            for k in 0..d {
                for sign in [1.0, -1.0] {
                    let mut y = x.clone();
                    y[k] = (y[k] + sign * step).clamp(0.0, 1.0);
                    let w = ei(&y);
                    if w > v {
                        (x, v, moved) = (y, w, true);
                    }
                }
            }
            moves += 1;
            if !moved {
                step /= 2.0;
            }
        }
        if v > top.1 {
            top = (x, v);
        }
    }
    top
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::study::simplex::nelder_mead;
    use std::f64::consts::PI;

    /// The Branin–Hoo function on [−5, 10] × [0, 15] from the unit square: three minima of
    /// 0.397887 (L. C. W. Dixon and G. P. Szegő, *Towards Global Optimisation 2*, North-Holland
    /// 1978).
    fn branin(u: &[f64]) -> f64 {
        let (x, y) = (-5.0 + 15.0 * u[0], 15.0 * u[1]);
        let b = 5.1 / (4.0 * PI * PI);
        (y - b * x * x + 5.0 / PI * x - 6.0).powi(2) + 10.0 * (1.0 - 1.0 / (8.0 * PI)) * x.cos() + 10.0
    }

    const BRANIN_MIN: f64 = 0.397_887_357_729_738;

    /// The calls until `f` came within `within` of `min` (None if never), searched by `search`.
    fn calls_to(f: impl Fn(&[f64]) -> f64, min: f64, within: f64, search: impl FnOnce(&mut dyn FnMut(&[f64]) -> Result<f64>)) -> Option<usize> {
        let mut calls = 0;
        let mut reached = None;
        let mut g = |u: &[f64]| -> Result<f64> {
            calls += 1;
            let v = f(u);
            if v - min < within && reached.is_none() {
                reached = Some(calls);
            }
            Ok(v)
        };
        search(&mut g);
        reached
    }

    #[test]
    fn the_bayesian_search_finds_branins_minimum_in_far_fewer_solves() {
        let bo = calls_to(branin, BRANIN_MIN, 1e-2, |f| {
            let mut f = |u: &[f64]| f(u);
            bayesian(&mut f, &[0.5, 0.5], 1e-9, 60).unwrap();
        });
        let nm = calls_to(branin, BRANIN_MIN, 1e-2, |f| {
            let mut f = |u: &[f64]| f(u);
            nelder_mead(&mut f, &[0.5, 0.5], 1e-9, 500).unwrap();
        });
        let (bo, nm) = (bo.unwrap(), nm.unwrap());
        eprintln!("Branin within 1e-2: Bayesian {bo} solves, Nelder–Mead {nm}");
        assert!(2 * bo <= nm, "Bayesian {bo} against Nelder–Mead {nm}");
    }

    #[test]
    fn the_bayesian_search_finds_a_quadratic_minimum_and_stops_on_its_own() {
        let q = |u: &[f64]| (u[0] - 0.3).powi(2) + 2.0 * (u[1] - 0.7).powi(2) + 0.1 * (u[0] - 0.3) * (u[1] - 0.7);
        let mut best = (f64::INFINITY, vec![]);
        let mut calls = 0;
        let mut f = |u: &[f64]| -> Result<f64> {
            calls += 1;
            let v = q(u);
            if v < best.0 {
                best = (v, u.to_vec());
            }
            Ok(v)
        };
        let out = bayesian(&mut f, &[0.5, 0.5], 1e-4, 60).unwrap();
        assert!(out.converged, "{out:?} after {calls}");
        assert!(best.0 < 1e-4, "{best:?}");
        let (m, s) = out.at_best.unwrap();
        assert!((m - best.0).abs() < 1e-3 && s < 1e-2, "{m} ± {s}");
        let nm = calls_to(q, 0.0, 1e-4, |f| {
            let mut f = |u: &[f64]| f(u);
            nelder_mead(&mut f, &[0.5, 0.5], 1e-9, 500).unwrap();
        });
        let bo = calls_to(q, 0.0, 1e-4, |f| {
            let mut f = |u: &[f64]| f(u);
            bayesian(&mut f, &[0.5, 0.5], 1e-4, 60).unwrap();
        });
        eprintln!("quadratic within 1e-4: Bayesian {bo:?} solves, Nelder–Mead {nm:?}");
        assert!(bo.unwrap() < nm.unwrap());
    }

    #[test]
    fn the_bayesian_search_is_deterministic_and_bounded() {
        let run = || {
            let mut seen: Vec<Vec<u64>> = Vec::new();
            let mut f = |u: &[f64]| -> Result<f64> {
                seen.push(u.iter().map(|v| v.to_bits()).collect());
                Ok(branin(u))
            };
            let out = bayesian(&mut f, &[0.2, 0.9], 1e-12, 15).unwrap();
            (seen, out)
        };
        let (a, oa) = run();
        let (b, ob) = run();
        assert_eq!(a, b);
        assert_eq!(oa, ob);
        assert_eq!(a.len(), 15);
        assert!(!oa.converged);
        // No point twice.
        let mut sorted = a.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), a.len());
    }

    #[test]
    fn failed_points_are_kept_away_from_and_errors_stop() {
        // Failing on the right half: the search stays on the left, finds the minimum there.
        let mut f = |u: &[f64]| -> Result<f64> { Ok(if u[0] > 0.5 { f64::INFINITY } else { (u[0] - 0.4).powi(2) }) };
        let out = bayesian(&mut f, &[0.9], 1e-6, 30).unwrap();
        assert!(out.at_best.is_some());
        let mut count = 0;
        let mut g = |_: &[f64]| -> Result<f64> {
            count += 1;
            if count > 3 { Err(EngineError::Cancelled) } else { Ok(1.0) }
        };
        assert_eq!(bayesian(&mut g, &[0.5, 0.5], 1e-3, 50), Err(EngineError::Cancelled));
        // Nothing solvable: it runs out of solves without a surrogate.
        let out = bayesian(&mut |_: &[f64]| Ok(f64::INFINITY), &[0.5], 1e-3, 6).unwrap();
        assert_eq!((out.converged, out.at_best), (false, None));
        // Hostile starts.
        let mut ok = |_: &[f64]| Ok(0.0);
        assert!(bayesian(&mut ok, &[], 1e-3, 5).is_err());
        assert!(bayesian(&mut ok, &[0.5; 9], 1e-3, 5).is_err());
        assert!(bayesian(&mut ok, &[f64::NAN], 1e-3, 5).is_err());
        assert!(bayesian(&mut ok, &[0.5], f64::NAN, 5).is_err());
        assert!(bayesian(&mut ok, &[0.5], 0.0, 5).is_err());
    }
}

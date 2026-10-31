//! `study.sweep` with a `surrogate`: a budget of solves spent on the grid's most informative
//! points, the rest predicted. The points solved are chosen as in the design of computer
//! experiments of J. Sacks, W. J. Welch, T. J. Mitchell and H. P. Wynn, "Design and analysis of
//! computer experiments", Statist. Sci. 4(4):409–435, 1989: a space-filling start (the grid
//! point nearest the centre, then each time the one farthest from those chosen — maximin
//! distance, §4; 2d + 1 of them), then each time the grid point where the surrogate of the
//! results so far is least sure (its largest predictive variance, §4's sequential design). Every
//! output a solved row holds (each S-parameter's |S| in dB at each frequency, each eigenmode's
//! frequency, each number the solve gives) is predicted at the other points by one Gaussian
//! process sharing the kernel ([`super::gp`]), with its σ; those rows are marked predicted.
//! Ties go to the earlier grid point, so the same sweep chooses the same points.

use super::gp::Gp;
use super::{Predicted, Row, Surrogate};
use crate::Result;

/// After this many solves the length scales are fitted again only every [`REFIT_EVERY`].
const REFIT_ALWAYS: usize = 60;
const REFIT_EVERY: usize = 10;

/// The grid's points scaled into [0, 1] along each axis (an axis of one value at 0).
pub(super) fn scaled(points: &[Vec<f64>], axes: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let spans: Vec<(f64, f64)> =
        axes.iter().map(|a| (a.iter().copied().fold(f64::INFINITY, f64::min), a.iter().copied().fold(f64::NEG_INFINITY, f64::max))).collect();
    points.iter().map(|p| p.iter().zip(&spans).map(|(v, (lo, hi))| if hi > lo { (v - lo) / (hi - lo) } else { 0.0 }).collect()).collect()
}

fn distance2(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum()
}

/// The point not yet chosen nearest the centre (none chosen) or farthest from those chosen.
fn farthest(u: &[Vec<f64>], chosen: &[bool]) -> Option<usize> {
    let picked: Vec<&Vec<f64>> = u.iter().zip(chosen).filter(|(_, c)| **c).map(|(p, _)| p).collect();
    let score = |p: &[f64]| -> f64 {
        if picked.is_empty() { -distance2(p, &vec![0.5; p.len()]) } else { picked.iter().map(|q| distance2(p, q)).fold(f64::INFINITY, f64::min) }
    };
    u.iter()
        .enumerate()
        .filter(|(i, _)| !chosen.get(*i).copied().unwrap_or(true))
        .map(|(i, p)| (i, score(p)))
        .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(i, _)| i)
}

/// What a row's output is.
#[derive(Clone, Debug, PartialEq)]
enum Key {
    /// |S| in dB of a parameter at the k-th frequency.
    S(String, usize),
    Mode(usize),
    Scalar(String),
}

fn value(row: &Row, key: &Key) -> Option<f64> {
    match key {
        Key::S(name, k) => row.db(name)?.get(*k).copied(),
        Key::Mode(m) => row.modes.get(*m).copied(),
        Key::Scalar(name) => row.scalars.iter().find(|(n, _)| n == name).map(|(_, v)| *v),
    }
}

/// The outputs every row in `good` holds, finite (the first row's order), with the
/// frequencies of its S-parameters (S only where every row solved the same frequencies).
fn keys(good: &[&Row]) -> (Vec<Key>, Vec<f64>) {
    let Some(first) = good.first() else { return (Vec::new(), Vec::new()) };
    let same_frequencies = good.iter().all(|r| {
        r.frequencies.len() == first.frequencies.len() && r.frequencies.iter().zip(&first.frequencies).all(|(a, b)| a.to_bits() == b.to_bits())
    });
    let mut keys = Vec::new();
    if same_frequencies {
        for (name, _) in &first.s {
            let ks: Vec<Key> = (0..first.frequencies.len()).map(|k| Key::S(name.clone(), k)).collect();
            if !ks.is_empty() && ks.iter().all(|k| good.iter().all(|r| value(r, k).is_some_and(f64::is_finite))) {
                keys.extend(ks);
            }
        }
    }
    let modes = good.iter().map(|r| r.modes.len()).min().unwrap_or(0);
    keys.extend((0..modes).map(Key::Mode));
    for (name, _) in &first.scalars {
        let k = Key::Scalar(name.clone());
        if good.iter().all(|r| value(r, &k).is_some_and(f64::is_finite)) {
            keys.push(k);
        }
    }
    let any_s = keys.iter().any(|k| matches!(k, Key::S(..)));
    keys.truncate(super::gp::MAX_OUTPUTS);
    (keys, if any_s { first.frequencies.clone() } else { Vec::new() })
}

/// The surrogate's data from the rows solved without error: their scaled points and outputs.
fn data<'r>(u: &[Vec<f64>], rows: &'r [Option<Row>]) -> (Vec<Vec<f64>>, Vec<&'r Row>) {
    rows.iter().enumerate().filter_map(|(i, r)| r.as_ref().filter(|r| r.error.is_none()).map(|r| (u[i].clone(), r))).unzip()
}

fn fit(x: &[Vec<f64>], good: &[&Row], keys: &[Key], lengths: Option<&[f64]>) -> Option<Gp> {
    if keys.is_empty() || x.len() < 2 {
        return None;
    }
    let y: Vec<Vec<f64>> = good.iter().map(|r| keys.iter().map(|k| value(r, k).unwrap_or(f64::NAN)).collect()).collect();
    match lengths {
        Some(l) => Gp::with_lengths(x, &y, l).ok(),
        None => Gp::fit(x, &y).ok(),
    }
}

/// The rows of grid `points` (their axes `axes`), `budget` of them solved by `solve` (grid
/// index, solve number) and the rest predicted; with what the surrogate was.
pub(super) fn fill(
    points: &[Vec<f64>],
    axes: &[Vec<f64>],
    budget: usize,
    solve: &mut impl FnMut(usize, usize) -> Result<Row>,
) -> Result<(Vec<Row>, Surrogate)> {
    let u = scaled(points, axes);
    let n = u.len();
    let d = axes.len();
    let budget = budget.min(n);
    let mut chosen = vec![false; n];
    let mut rows: Vec<Option<Row>> = vec![None; n];
    let mut lengths: Option<Vec<f64>> = None;
    for k in 0..budget {
        let by_variance = if k < 2 * d + 1 {
            None
        } else {
            let (x, good) = data(&u, &rows);
            let (keys, _) = keys(&good);
            let refit = lengths.is_none() || x.len() <= REFIT_ALWAYS || x.len() % REFIT_EVERY == 0;
            fit(&x, &good, &keys, if refit { None } else { lengths.as_deref() }).and_then(|gp| {
                lengths = Some(gp.lengths().to_vec());
                // The largest variance left, the earliest of equals.
                (0..n)
                    .filter(|i| !chosen[*i])
                    .filter_map(|i| Some((i, gp.variance_left(&u[i]).ok()?)))
                    .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
                    .map(|(i, _)| i)
            })
        };
        let Some(i) = by_variance.or_else(|| farthest(&u, &chosen)) else { break };
        chosen[i] = true;
        rows[i] = Some(solve(i, k)?);
    }
    // The other points predicted from every row solved.
    let (x, good) = data(&u, &rows);
    let (keys, frequencies) = keys(&good);
    let gp = fit(&x, &good, &keys, None);
    let solves = chosen.iter().filter(|c| **c).count();
    let surrogate = Surrogate { solves, lengths: gp.as_ref().map(|g| g.lengths().to_vec()).unwrap_or_default(), at_best: None };
    let mut out = Vec::with_capacity(n);
    for (i, row) in rows.into_iter().enumerate() {
        if let Some(r) = row {
            out.push(r);
            continue;
        }
        let values = points[i].clone();
        let Some(prediction) = gp.as_ref().and_then(|g| g.predict(&u[i]).ok()) else {
            out.push(Row { values, error: Some("not solved, and too few points solved to predict it".into()), ..Row::default() });
            continue;
        };
        let mut row = Row { values, frequencies: frequencies.clone(), ..Row::default() };
        let mut predicted = Predicted::default();
        for (key, (m, s)) in keys.iter().zip(prediction.mean.iter().zip(&prediction.sigma)) {
            match key {
                Key::S(name, _) => match predicted.db.iter_mut().find(|(n, _, _)| n == name) {
                    Some((_, means, sigmas)) => {
                        means.push(*m);
                        sigmas.push(*s);
                    }
                    None => predicted.db.push((name.clone(), vec![*m], vec![*s])),
                },
                Key::Mode(_) => {
                    row.modes.push(*m);
                    predicted.modes_sigma.push(*s);
                }
                Key::Scalar(name) => {
                    row.scalars.push((name.clone(), *m));
                    predicted.scalars_sigma.push((name.clone(), *s));
                }
            }
        }
        row.prediction = Some(predicted);
        out.push(row);
    }
    Ok((out, surrogate))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row as a solve of a smooth |S21| in dB at two frequencies would give, and a mode.
    fn solved(v: &[f64]) -> Row {
        let db = |f: f64| -20.0 * (1.0 + f * (v[0] - 0.3).powi(2) + 0.5 * v.get(1).map_or(0.0, |b| (b - 0.6).powi(2)));
        let c = |x: f64| emcraft_solver::Complex64::new(10f64.powf(x / 20.0), 0.0);
        Row {
            values: v.to_vec(),
            frequencies: vec![6.0, 8.0],
            s: vec![("s21".into(), vec![c(db(1.0)), c(db(2.0))])],
            modes: vec![5.0 + v[0]],
            scalars: vec![("power".into(), 2.0 * v[0])],
            ..Row::default()
        }
    }

    #[test]
    fn a_budget_of_solves_spreads_out_and_predicts_the_rest() {
        let axis: Vec<f64> = (0..21).map(|i| i as f64 / 20.0).collect();
        let points: Vec<Vec<f64>> = axis.iter().map(|v| vec![*v]).collect();
        let mut order = Vec::new();
        let (rows, s) = fill(&points, std::slice::from_ref(&axis), 7, &mut |i, k| {
            order.push((i, k));
            Ok(solved(&points[i]))
        })
        .unwrap();
        // The centre, the ends, then between.
        assert_eq!(order.iter().take(3).map(|o| o.0).collect::<Vec<_>>(), [10, 0, 20]);
        assert_eq!(order.len(), 7);
        assert_eq!(s.solves, 7);
        assert_eq!(rows.len(), 21);
        let predicted: Vec<&Row> = rows.iter().filter(|r| r.prediction.is_some()).collect();
        assert_eq!(predicted.len(), 14);
        for r in &rows {
            let exact = solved(&r.values);
            let (got, want) = (r.db("s21").unwrap(), exact.db("s21").unwrap());
            for k in 0..2 {
                assert!((got[k] - want[k]).abs() < 0.2, "{:?}: {got:?} against {want:?}", r.values);
            }
            assert!((r.modes[0] - exact.modes[0]).abs() < 1e-3 && (r.scalars[0].1 - exact.scalars[0].1).abs() < 1e-3, "{r:?}");
        }
        // Solved rows as they were; predicted ones carry a σ.
        assert!(rows.iter().filter(|r| r.prediction.is_none()).all(|r| format!("{r:?}") == format!("{:?}", solved(&r.values))));
        assert!(predicted.iter().all(|r| r.db_sigma("s21").is_some_and(|s| s.iter().all(|x| x.is_finite() && *x >= 0.0))));
    }

    #[test]
    fn a_grid_is_filled_alike_every_time_and_failures_are_left_out() {
        let a: Vec<f64> = (0..6).map(|i| i as f64).collect();
        let b = vec![10.0, 20.0, 30.0];
        let points: Vec<Vec<f64>> = a.iter().flat_map(|x| b.iter().map(move |y| vec![*x, *y])).collect();
        let axes = vec![a.clone(), b.clone()];
        let run = || {
            let mut order = Vec::new();
            let (rows, _) = fill(&points, &axes, 8, &mut |i, _| {
                order.push(i);
                let p = &points[i];
                if i == 0 {
                    return Ok(Row { values: p.clone(), error: Some("bad".into()), ..Row::default() });
                }
                Ok(solved(&[p[0] / 5.0, p[1] / 30.0]))
            })
            .unwrap();
            (order, format!("{rows:?}"))
        };
        let (o1, r1) = run();
        assert_eq!(run(), (o1.clone(), r1));
        let mut unique = o1.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 8);
    }

    #[test]
    fn too_few_solved_points_leave_the_rest_unpredicted() {
        let axis = vec![1.0, 2.0, 3.0, 4.0];
        let points: Vec<Vec<f64>> = axis.iter().map(|v| vec![*v]).collect();
        let (rows, s) = fill(&points, std::slice::from_ref(&axis), 2, &mut |i, k| {
            Ok(if k == 0 { Row { error: Some("bad".into()), values: points[i].clone(), ..Row::default() } } else { solved(&points[i]) })
        })
        .unwrap();
        assert_eq!(s.solves, 2);
        assert_eq!(rows.iter().filter(|r| r.error.is_some()).count(), 3);
    }
}

//! The downhill simplex of Nelder and Mead over the unit box (the module docs of `study` cite
//! it).

use crate::Result;

pub(super) const OUT_OF_SOLVES: &str = "the optimization ran out of solves";

/// Minimizes `f` over [0, 1]ⁿ from `start` by the Nelder–Mead simplex (trial points clamped into
/// the box): true once the simplex is within `tolerance` of its best vertex in every coordinate.
/// Stops with `f`'s error (`f` refuses past its budget); at most `cap`·(n + 2) iterations.
pub(super) fn nelder_mead(f: &mut impl FnMut(&[f64]) -> Result<f64>, start: &[f64], tolerance: f64, cap: usize) -> Result<bool> {
    let n = start.len();
    let clamp = |x: Vec<f64>| -> Vec<f64> { x.into_iter().map(|v| v.clamp(0.0, 1.0)).collect() };
    // The first simplex: the start and a step of a quarter of the range along each axis, inwards.
    let mut simplex: Vec<(Vec<f64>, f64, usize)> = Vec::with_capacity(n + 1);
    simplex.push((start.to_vec(), f(start)?, 0));
    for k in 0..n {
        let mut x = start.to_vec();
        x[k] = if x[k] + 0.25 <= 1.0 { x[k] + 0.25 } else { x[k] - 0.25 };
        let fx = f(&x)?;
        simplex.push((x, fx, k + 1));
    }
    let mut age = n + 1;
    let combine = |a: &[f64], b: &[f64], t: f64| -> Vec<f64> { clamp(a.iter().zip(b).map(|(x, y)| x + t * (y - x)).collect()) };
    for _ in 0..cap.saturating_mul(n + 2) {
        // Best first; equal values by age, the older first.
        simplex.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.2.cmp(&b.2)));
        let best = simplex[0].0.clone();
        let spread = simplex.iter().flat_map(|(x, _, _)| x.iter().zip(&best).map(|(a, b)| (a - b).abs())).fold(0.0, f64::max);
        if spread < tolerance {
            return Ok(true);
        }
        let centroid: Vec<f64> = (0..n).map(|k| simplex[..n].iter().map(|v| v.0[k]).sum::<f64>() / n as f64).collect();
        let (worst, f_worst) = (simplex[n].0.clone(), simplex[n].1);
        let (f_best, f_second) = (simplex[0].1, simplex[n - 1].1);
        let reflected = combine(&centroid, &worst, -1.0);
        let f_r = f(&reflected)?;
        age += 1;
        let replacement = if f_r < f_best {
            let expanded = combine(&centroid, &worst, -2.0);
            let f_e = f(&expanded)?;
            if f_e < f_r { Some((expanded, f_e)) } else { Some((reflected, f_r)) }
        } else if f_r < f_second {
            Some((reflected, f_r))
        } else if f_r < f_worst {
            let outside = combine(&centroid, &reflected, 0.5);
            let f_c = f(&outside)?;
            (f_c <= f_r).then_some((outside, f_c))
        } else {
            let inside = combine(&centroid, &worst, 0.5);
            let f_c = f(&inside)?;
            (f_c < f_worst).then_some((inside, f_c))
        };
        match replacement {
            Some((x, fx)) => simplex[n] = (x, fx, age),
            None => {
                // Shrink towards the best vertex.
                for v in simplex.iter_mut().skip(1) {
                    let x = combine(&best, &v.0, 0.5);
                    let fx = f(&x)?;
                    age += 1;
                    *v = (x, fx, age);
                }
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineError;

    #[test]
    fn nelder_mead_finds_a_quadratic_minimum_in_the_box() {
        let mut calls = 0;
        let mut f = |x: &[f64]| -> Result<f64> {
            calls += 1;
            Ok((x[0] - 0.3).powi(2) + 2.0 * (x[1] - 0.7).powi(2) + 0.1 * (x[0] - 0.3) * (x[1] - 0.7))
        };
        let mut best = (f64::INFINITY, vec![]);
        let mut g = |x: &[f64]| -> Result<f64> {
            let v = f(x)?;
            if v < best.0 {
                best = (v, x.to_vec());
            }
            Ok(v)
        };
        assert_eq!(nelder_mead(&mut g, &[0.5, 0.5], 1e-6, 400), Ok(true));
        assert!((best.1[0] - 0.3).abs() < 1e-5 && (best.1[1] - 0.7).abs() < 1e-5, "{best:?}");
        // A minimum outside the box ends on its wall.
        let mut edge = (f64::INFINITY, 0.0);
        let mut h = |x: &[f64]| -> Result<f64> {
            let v = (x[0] - 1.5).powi(2);
            if v < edge.0 {
                edge = (v, x[0]);
            }
            Ok(v)
        };
        nelder_mead(&mut h, &[0.2], 1e-6, 200).unwrap();
        assert_eq!(edge.1, 1.0);
    }

    #[test]
    fn nelder_mead_stops_on_its_budget() {
        let mut count = 0;
        let mut f = |x: &[f64]| -> Result<f64> {
            count += 1;
            if count > 5 {
                return Err(EngineError::Failed(OUT_OF_SOLVES.into()));
            }
            Ok(x[0].sin())
        };
        assert!(nelder_mead(&mut f, &[0.5], 1e-9, 5).is_err());
        // A constant: the simplex shrinks to its start within the tolerance.
        assert_eq!(nelder_mead(&mut |_: &[f64]| Ok(1.0), &[0.5, 0.5], 1e-3, 1000), Ok(true));
    }
}

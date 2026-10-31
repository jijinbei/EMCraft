//! A Gaussian-process surrogate of a study's results over its variables scaled to [0, 1]: the
//! regression of C. E. Rasmussen and C. K. I. Williams, *Gaussian Processes for Machine
//! Learning*, MIT Press 2006 — the predictive mean and variance by one Cholesky factorization
//! (Algorithm 2.1, §2.2), the Matérn ν = 5/2 covariance (§4.2, eq. 4.17) with one length scale
//! per variable (automatic relevance determination, §5.1) and a small noise floor (a nugget) on
//! its diagonal, the length scales chosen by the marginal likelihood (§5.4.1).
//!
//! Each output (an |S| in dB at one frequency, an eigenfrequency, a number the solve gives) is
//! centred on its mean and scaled by its spread; all share the correlation R of the points, so
//! one factorization serves them all, each a right-hand side of its own. The signal variance of
//! each is concentrated out of the likelihood (its maximum-likelihood value yᵀR⁻¹y/n, as D. R.
//! Jones, M. Schonlau and W. J. Welch, "Efficient global optimization of expensive black-box
//! functions", J. Global Optim. 13:455–492, 1998, eq. 4–6, do), leaving the length scales: they
//! are picked from a fixed geometric ladder, first one scale for every variable, then each in
//! turn (a coordinate search of a fixed number of passes), so the same data give the same
//! surrogate bit for bit. The likelihood sums over at most [`FIT_OUTPUTS`] outputs spread evenly.
//!
//! Also the deterministic space-filling points ([`halton`]: J. H. Halton, "On the efficiency of
//! certain quasi-random sequences of points in evaluating multi-dimensional integrals", Numer.
//! Math. 2:84–90, 1960) and the expected improvement ([`expected_improvement`], Jones et al.
//! 1998, eq. 15) the Bayesian optimizer and the adaptive sweep choose their points by.

use emcraft_solver::dense::Cholesky;

use crate::{EngineError, Result};

/// The most points a surrogate is fitted to (the order of its matrix).
pub const MAX_DATA: usize = 500;
/// The most variables a surrogate spans.
pub const MAX_DIMENSION: usize = 8;
/// The most outputs one surrogate predicts.
pub const MAX_OUTPUTS: usize = 8192;
/// The most outputs the likelihood that picks the length scales sums over.
pub const FIT_OUTPUTS: usize = 32;
/// The noise floor: the nugget on R's diagonal, relative to each output's signal variance;
/// raised tenfold (at most to [`MAX_NUGGET`]) where R is too near singular to factor.
pub const NUGGET: f64 = 1e-8;
const MAX_NUGGET: f64 = 1e-2;
/// The ladder of length scales tried, a share of each variable's range.
const LADDER: [f64; 11] = [0.1, 0.15, 0.22, 0.33, 0.5, 0.75, 1.1, 1.7, 2.5, 3.8, 5.6];
/// Coordinate passes over the variables after the common scale.
const PASSES: usize = 2;
/// The primes of the Halton sequence's bases, one per coordinate.
const PRIMES: [u64; MAX_DIMENSION] = [2, 3, 5, 7, 11, 13, 17, 19];

/// Point `index` (from 1; 0 is the origin) of the Halton sequence in [0, 1]^`dim`: coordinate k
/// the radical inverse of `index` in the k-th prime's base. `dim` is at most [`MAX_DIMENSION`]
/// (the coordinates past it are left out).
pub fn halton(index: usize, dim: usize) -> Vec<f64> {
    PRIMES
        .iter()
        .take(dim)
        .map(|&b| {
            let (mut i, mut f, mut x) = (index as u64, 1.0, 0.0);
            // At most 64 digits: i shrinks by b ≥ 2 each turn.
            while i > 0 {
                f /= b as f64;
                x += f * (i % b) as f64;
                i /= b;
            }
            x
        })
        .collect()
}

/// The Matérn ν = 5/2 correlation at scaled distance r ≥ 0: (1 + √5 r + 5r²/3) e^{−√5 r}.
fn matern(r: f64) -> f64 {
    let s = 5f64.sqrt() * r;
    (1.0 + s + s * s / 3.0) * (-s).exp()
}

/// The scaled distance between `a` and `b` with length scales `lengths`.
fn distance(a: &[f64], b: &[f64], lengths: &[f64]) -> f64 {
    a.iter().zip(b).zip(lengths).map(|((x, y), l)| ((x - y) / l).powi(2)).sum::<f64>().sqrt()
}

/// One output as the surrogate holds it.
#[derive(Clone, Debug)]
struct Output {
    mean: f64,
    scale: f64,
    /// The signal variance of the centred, scaled output (its maximum-likelihood value).
    variance: f64,
    /// R⁻¹ y.
    alpha: Vec<f64>,
}

/// A fitted surrogate: the points, the length scales, the factor of R and the outputs.
pub struct Gp {
    x: Vec<Vec<f64>>,
    lengths: Vec<f64>,
    nugget: f64,
    factor: Cholesky<f64>,
    outputs: Vec<Output>,
}

/// The surrogate's prediction at a point: each output's mean and standard deviation (the
/// outputs' own units).
#[derive(Clone, Debug, PartialEq)]
pub struct Prediction {
    pub mean: Vec<f64>,
    pub sigma: Vec<f64>,
}

fn refuse(why: impl Into<String>) -> EngineError {
    EngineError::Params(format!("surrogate: {}", why.into()))
}

/// The data checked: 2 to [`MAX_DATA`] points of 1 to [`MAX_DIMENSION`] finite coordinates, no
/// point twice, each with the same number (1 to [`MAX_OUTPUTS`]) of finite outputs.
fn check(x: &[Vec<f64>], y: &[Vec<f64>]) -> Result<(usize, usize)> {
    let n = x.len();
    if !(2..=MAX_DATA).contains(&n) {
        return Err(refuse(format!("needs 2 to {MAX_DATA} points, got {n}")));
    }
    if y.len() != n {
        return Err(refuse("needs one row of outputs per point"));
    }
    let d = x.first().map_or(0, Vec::len);
    if !(1..=MAX_DIMENSION).contains(&d) || x.iter().any(|p| p.len() != d) {
        return Err(refuse(format!("the points need 1 to {MAX_DIMENSION} coordinates each, the same for all")));
    }
    if x.iter().flatten().any(|v| !v.is_finite()) {
        return Err(refuse("a point is not finite"));
    }
    let q = y.first().map_or(0, Vec::len);
    if !(1..=MAX_OUTPUTS).contains(&q) || y.iter().any(|r| r.len() != q) {
        return Err(refuse(format!("needs 1 to {MAX_OUTPUTS} outputs at every point, the same for all")));
    }
    if y.iter().flatten().any(|v| !v.is_finite()) {
        return Err(refuse("an output is not finite"));
    }
    for i in 0..n {
        for j in 0..i {
            if x[i].iter().zip(&x[j]).all(|(a, b)| a.to_bits() == b.to_bits()) {
                return Err(refuse(format!("point {} is point {} again", i + 1, j + 1)));
            }
        }
    }
    Ok((d, q))
}

/// R + nugget·I factored, the nugget raised until it factors; with the nugget used.
fn factor(x: &[Vec<f64>], lengths: &[f64]) -> Option<(Cholesky<f64>, f64)> {
    let n = x.len();
    let mut r = vec![0.0; n * n];
    for i in 0..n {
        for j in 0..=i {
            let c = matern(distance(&x[i], &x[j], lengths));
            r[i * n + j] = c;
            r[j * n + i] = c;
        }
    }
    let mut nugget = NUGGET;
    // At most seven tries: 1e-8 to 1e-2.
    while nugget <= MAX_NUGGET * 1.000_001 {
        let mut a = r.clone();
        for i in 0..n {
            a[i * n + i] += nugget;
        }
        if let Some(l) = Cholesky::new(&a, n) {
            return Some((l, nugget));
        }
        nugget *= 10.0;
    }
    None
}

/// The outputs centred and scaled: per output its mean and scale, and the scaled columns.
fn standardize(y: &[Vec<f64>], q: usize) -> (Vec<(f64, f64)>, Vec<Vec<f64>>) {
    let n = y.len() as f64;
    (0..q)
        .map(|j| {
            let mean = y.iter().map(|r| r[j]).sum::<f64>() / n;
            let var = y.iter().map(|r| (r[j] - mean).powi(2)).sum::<f64>() / n;
            // A constant output keeps a unit scale: its column is zero.
            let scale = if var.sqrt() > 1e-12 * mean.abs().max(1.0) { var.sqrt() } else { 1.0 };
            ((mean, scale), y.iter().map(|r| (r[j] - mean) / scale).collect())
        })
        .unzip()
}

/// The concentrated log likelihood of `columns` (those `fit` names) under R(`lengths`):
/// Σⱼ −n/2 ln σⱼ² − 1/2 ln |R|, σⱼ² = yⱼᵀR⁻¹yⱼ/n (constants dropped); `None` when R does not
/// factor.
fn likelihood(x: &[Vec<f64>], columns: &[Vec<f64>], fit: &[usize], lengths: &[f64]) -> Option<f64> {
    let (l, _) = factor(x, lengths)?;
    let n = x.len() as f64;
    let mut sum = 0.0;
    for &j in fit {
        let z = l.lower_solve(columns.get(j)?);
        let s2 = (z.iter().map(|v| v * v).sum::<f64>() / n).max(1e-300);
        sum += -0.5 * n * s2.ln() - 0.5 * l.log_determinant();
    }
    sum.is_finite().then_some(sum)
}

impl Gp {
    /// The surrogate of outputs `y` (a row per point) at points `x`, its length scales chosen by
    /// the likelihood.
    ///
    /// ```
    /// use emcraft_engine::study::gp::Gp;
    ///
    /// let x: Vec<Vec<f64>> = (0..6).map(|i| vec![i as f64 / 5.0]).collect();
    /// let y: Vec<Vec<f64>> = x.iter().map(|p| vec![(3.0 * p[0]).sin()]).collect();
    /// let gp = Gp::fit(&x, &y)?;
    /// let at = gp.predict(&[0.5])?;
    /// assert!((at.mean[0] - 1.5f64.sin()).abs() < 1e-2 && at.sigma[0] < 1e-1);
    /// // Too few points, a point twice, a NaN: refused.
    /// assert!(Gp::fit(&x[..1], &y[..1]).is_err());
    /// assert!(Gp::fit(&[vec![0.0], vec![0.0]], &[vec![1.0], vec![2.0]]).is_err());
    /// assert!(Gp::fit(&[vec![0.0], vec![1.0]], &[vec![1.0], vec![f64::NAN]]).is_err());
    /// # Ok::<(), emcraft_engine::EngineError>(())
    /// ```
    pub fn fit(x: &[Vec<f64>], y: &[Vec<f64>]) -> Result<Gp> {
        let (d, q) = check(x, y)?;
        let (_, columns) = standardize(y, q);
        // The outputs the likelihood reads: at most FIT_OUTPUTS, evenly spread.
        let fit: Vec<usize> = if q <= FIT_OUTPUTS { (0..q).collect() } else { (0..FIT_OUTPUTS).map(|k| k * q / FIT_OUTPUTS).collect() };
        let score = |lengths: &[f64]| likelihood(x, &columns, &fit, lengths);
        // One scale for all, then each variable's in turn; ties keep the earlier.
        let mut best: Option<(f64, Vec<f64>)> = None;
        for &g in &LADDER {
            let lengths = vec![g; d];
            if let Some(s) = score(&lengths)
                && best.as_ref().is_none_or(|(b, _)| s > *b)
            {
                best = Some((s, lengths));
            }
        }
        let (mut top, mut lengths) = best.ok_or_else(|| EngineError::failed("surrogate: the points' correlation does not factor"))?;
        if d > 1 {
            for _ in 0..PASSES {
                for k in 0..d {
                    for &g in &LADDER {
                        let mut trial = lengths.clone();
                        trial[k] = g;
                        if let Some(s) = score(&trial)
                            && s > top
                        {
                            (top, lengths) = (s, trial);
                        }
                    }
                }
            }
        }
        Gp::with_lengths(x, y, &lengths)
    }

    /// The surrogate with the length scales given (one per coordinate, each > 0).
    pub fn with_lengths(x: &[Vec<f64>], y: &[Vec<f64>], lengths: &[f64]) -> Result<Gp> {
        let (d, q) = check(x, y)?;
        if lengths.len() != d || !lengths.iter().all(|l| l.is_finite() && *l > 0.0) {
            return Err(refuse("needs a finite length scale > 0 per variable"));
        }
        let (scales, columns) = standardize(y, q);
        let (factor, nugget) = factor(x, lengths).ok_or_else(|| EngineError::failed("surrogate: the points' correlation does not factor"))?;
        let n = x.len() as f64;
        let outputs = scales
            .into_iter()
            .zip(&columns)
            .map(|((mean, scale), column)| {
                let z = factor.lower_solve(column);
                let variance = z.iter().map(|v| v * v).sum::<f64>() / n;
                Output { mean, scale, variance, alpha: factor.upper_solve(&z) }
            })
            .collect();
        Ok(Gp { x: x.to_vec(), lengths: lengths.to_vec(), nugget, factor, outputs })
    }

    /// The length scales, a share of each variable's range.
    pub fn lengths(&self) -> &[f64] {
        &self.lengths
    }

    /// The nugget R's factor took.
    pub fn nugget(&self) -> f64 {
        self.nugget
    }

    /// The points' count.
    pub fn len(&self) -> usize {
        self.x.len()
    }

    /// Whether it holds no points (never: a fit needs two).
    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }

    /// The correlations with `at` and the variance left at `at` as a share of the prior's
    /// (1 − kᵀR⁻¹k, from 0 at the data to 1 far from it).
    fn correlations(&self, at: &[f64]) -> Result<(Vec<f64>, f64)> {
        if at.len() != self.lengths.len() || !at.iter().all(|v| v.is_finite()) {
            return Err(refuse("the point predicted at needs one finite coordinate per variable"));
        }
        let k: Vec<f64> = self.x.iter().map(|p| matern(distance(p, at, &self.lengths))).collect();
        let v = self.factor.lower_solve(&k);
        let left = (1.0 - v.iter().map(|x| x * x).sum::<f64>()).clamp(0.0, 1.0);
        Ok((k, left))
    }

    /// The share of the prior variance left at `at`, from 0 (at the data) to 1 (far from it): the
    /// same for every output.
    pub fn variance_left(&self, at: &[f64]) -> Result<f64> {
        Ok(self.correlations(at)?.1)
    }

    /// Every output's mean and standard deviation at `at`.
    pub fn predict(&self, at: &[f64]) -> Result<Prediction> {
        let (k, left) = self.correlations(at)?;
        let (mean, sigma) = self
            .outputs
            .iter()
            .map(|o| {
                let m = o.mean + o.scale * k.iter().zip(&o.alpha).map(|(a, b)| a * b).sum::<f64>();
                (m, o.scale * (o.variance * left).sqrt())
            })
            .unzip();
        Ok(Prediction { mean, sigma })
    }
}

/// erfc(x) to near double precision: the Maclaurin series of erf for |x| < 2.5 and the
/// continued fraction of erfc beyond (M. Abramowitz and I. A. Stegun, *Handbook of Mathematical
/// Functions*, NBS 1964, 7.1.5 and 7.1.14), each of a fixed number of terms.
fn erfc(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x.abs() < 2.5 {
        // erf x = 2/√π Σ (−1)ⁿ x^{2n+1} / (n! (2n+1)).
        let (mut term, mut sum) = (x, x);
        for n in 1..60 {
            term *= -x * x / n as f64;
            sum += term / (2 * n + 1) as f64;
        }
        return 1.0 - 2.0 / std::f64::consts::PI.sqrt() * sum;
    }
    if x < 0.0 {
        return 2.0 - erfc(-x);
    }
    // erfc x = e^{−x²}/√π · 1/(x + (1/2)/(x + 1/(x + (3/2)/(x + …)))), evaluated from its tail.
    let mut f = x;
    for k in (1..=60).rev() {
        f = x + (k as f64 / 2.0) / f;
    }
    (-x * x).exp() / (std::f64::consts::PI.sqrt() * f)
}

/// The standard normal distribution function Φ(z).
pub fn normal_cdf(z: f64) -> f64 {
    0.5 * erfc(-z / std::f64::consts::SQRT_2)
}

/// The standard normal density φ(z).
pub fn normal_pdf(z: f64) -> f64 {
    (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

/// The expected improvement on `best` (minimizing) of a value predicted `mean` ± `sigma`:
/// (best − μ) Φ(z) + σ φ(z), z = (best − μ)/σ (Jones et al. 1998, eq. 15); the plain
/// improvement where σ is 0.
///
/// ```
/// use emcraft_engine::study::gp::expected_improvement;
///
/// assert_eq!(expected_improvement(1.0, 2.0, 0.0), 0.0);
/// assert_eq!(expected_improvement(1.0, 0.5, 0.0), 0.5);
/// // Uncertain and no better on average: still worth something.
/// assert!((expected_improvement(1.0, 1.0, 1.0) - 0.398_942_280_401_432_7).abs() < 1e-15);
/// assert_eq!(expected_improvement(f64::NAN, 1.0, 1.0), 0.0);
/// ```
pub fn expected_improvement(best: f64, mean: f64, sigma: f64) -> f64 {
    let gain = best - mean;
    if !(gain.is_finite() && sigma.is_finite()) {
        return 0.0;
    }
    if sigma <= 1e-300 {
        return gain.max(0.0);
    }
    let z = gain / sigma;
    (gain * normal_cdf(z) + sigma * normal_pdf(z)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid1(n: usize) -> Vec<Vec<f64>> {
        (0..n).map(|i| vec![i as f64 / (n - 1) as f64]).collect()
    }

    #[test]
    fn the_normal_distribution_is_close_to_its_tables() {
        // Abramowitz & Stegun table 26.1 and erfc's known values.
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-16);
        assert!((normal_cdf(1.0) - 0.841_344_746_068_542_9).abs() < 1e-14);
        assert!((normal_cdf(-2.0) - 0.022_750_131_948_179_2).abs() < 1e-15);
        assert!((erfc(3.0) - 2.209_049_699_858_544e-5).abs() < 1e-18);
        assert!((erfc(-3.0) - (2.0 - 2.209_049_699_858_544e-5)).abs() < 1e-14);
        assert!((erfc(6.0) / 2.151_973_671_249_891_3e-17 - 1.0).abs() < 1e-12);
        assert!(normal_cdf(-40.0) >= 0.0 && normal_cdf(40.0) == 1.0);
        assert!(normal_cdf(f64::NAN).is_nan());
    }

    #[test]
    fn halton_points_are_the_radical_inverses() {
        assert_eq!(halton(1, 2), vec![0.5, 1.0 / 3.0]);
        assert_eq!(halton(2, 2), vec![0.25, 2.0 / 3.0]);
        let h = halton(3, 3);
        assert!(h[0] == 0.75 && h[1] == 1.0 / 9.0 && (h[2] - 0.6).abs() < 1e-15, "{h:?}");
        assert_eq!(halton(0, 1), vec![0.0]);
        assert_eq!(halton(usize::MAX, 8).len(), 8);
        assert!(halton(usize::MAX, 8).iter().all(|x| (0.0..=1.0).contains(x)));
        assert_eq!(halton(5, 20).len(), MAX_DIMENSION);
    }

    #[test]
    fn the_surrogate_interpolates_its_data_and_is_sure_only_there() {
        let x = grid1(7);
        let f = |u: f64| (4.0 * u).sin() + 0.5 * u;
        let y: Vec<Vec<f64>> = x.iter().map(|p| vec![f(p[0]), 10.0 * f(p[0]) - 3.0]).collect();
        let gp = Gp::fit(&x, &y).unwrap();
        // Within the noise floor of each output's spread.
        let spread = |j: usize| y.iter().map(|r| r[j]).fold(f64::NEG_INFINITY, f64::max) - y.iter().map(|r| r[j]).fold(f64::INFINITY, f64::min);
        for (p, v) in x.iter().zip(&y) {
            let at = gp.predict(p).unwrap();
            for j in 0..2 {
                assert!((at.mean[j] - v[j]).abs() < 1e-5 * spread(j), "{p:?}: {at:?} against {v:?}");
                assert!(at.sigma[j] < 1e-3 * spread(j), "{p:?}: {at:?}");
            }
        }
        // Between the points, uncertain; farther out, more so.
        let mid = gp.predict(&[1.0 / 12.0]).unwrap().sigma[0];
        let out = gp.predict(&[1.6]).unwrap().sigma[0];
        assert!(mid > 1e-4 && out > 10.0 * mid, "{mid} {out}");
        assert!(gp.variance_left(&[5.0]).unwrap() > 0.99);
        assert!(gp.variance_left(&x[3]).unwrap() < 1e-6);
        // The second output is the first scaled: the same correlations, ten times the σ.
        let at = gp.predict(&[0.3]).unwrap();
        assert!((at.sigma[1] - 10.0 * at.sigma[0]).abs() < 1e-9 * at.sigma[1].max(1e-300));
        assert!((at.mean[1] - (10.0 * at.mean[0] - 3.0)).abs() < 1e-9);
    }

    #[test]
    fn the_surrogate_reproduces_smooth_functions() {
        // 1-D: twelve points of a sine.
        let x = grid1(12);
        let f = |u: f64| (2.0 * std::f64::consts::PI * u).sin();
        let y: Vec<Vec<f64>> = x.iter().map(|p| vec![f(p[0])]).collect();
        let gp = Gp::fit(&x, &y).unwrap();
        let worst = (0..101).map(|i| i as f64 / 100.0).map(|u| (gp.predict(&[u]).unwrap().mean[0] - f(u)).abs()).fold(0.0, f64::max);
        assert!(worst < 5e-3, "{worst}");
        // 2-D: 40 Halton points of a smooth bump, one variable mattering more.
        let g = |p: &[f64]| (-(3.0 * (p[0] - 0.4)).powi(2) - (p[1] - 0.6).powi(2)).exp();
        let x: Vec<Vec<f64>> = (1..=40).map(|i| halton(i, 2)).collect();
        let y: Vec<Vec<f64>> = x.iter().map(|p| vec![g(p)]).collect();
        let gp = Gp::fit(&x, &y).unwrap();
        let mut worst: f64 = 0.0;
        for i in 0..=10 {
            for j in 0..=10 {
                let p = [0.05 + 0.09 * i as f64, 0.05 + 0.09 * j as f64];
                let at = gp.predict(&p).unwrap();
                worst = worst.max((at.mean[0] - g(&p)).abs());
                // The error within a few σ (σ honest), past a floor.
                assert!((at.mean[0] - g(&p)).abs() < 4.0 * at.sigma[0] + 2e-3, "{p:?}: {at:?} against {}", g(&p));
            }
        }
        assert!(worst < 2e-2, "{worst}");
        // The second variable's scale is the longer one: it matters less.
        assert!(gp.lengths()[1] > gp.lengths()[0], "{:?}", gp.lengths());
    }

    #[test]
    fn the_same_data_give_the_same_surrogate_bit_for_bit() {
        let x: Vec<Vec<f64>> = (1..=15).map(|i| halton(i, 3)).collect();
        let y: Vec<Vec<f64>> = x.iter().map(|p| vec![p[0] * p[1] - p[2].cos(), p.iter().sum()]).collect();
        let a = Gp::fit(&x, &y).unwrap();
        let b = Gp::fit(&x, &y).unwrap();
        assert_eq!(a.lengths(), b.lengths());
        let at = [0.31, 0.77, 0.12];
        let (pa, pb) = (a.predict(&at).unwrap(), b.predict(&at).unwrap());
        assert_eq!(pa.mean.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), pb.mean.iter().map(|v| v.to_bits()).collect::<Vec<_>>());
        assert_eq!(pa.sigma.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), pb.sigma.iter().map(|v| v.to_bits()).collect::<Vec<_>>());
    }

    #[test]
    fn hostile_data_are_refused() {
        let ok = grid1(3);
        let y = vec![vec![1.0]; 3];
        assert!(Gp::fit(&ok, &y).is_ok());
        let nan = vec![vec![0.0], vec![f64::NAN], vec![1.0]];
        assert!(Gp::fit(&nan, &y).is_err());
        assert!(Gp::fit(&ok, &[vec![1.0], vec![f64::INFINITY], vec![0.0]]).is_err());
        assert!(Gp::fit(&[vec![0.5], vec![0.2], vec![0.5]], &y).is_err());
        assert!(Gp::fit(&ok, &y[..2]).is_err());
        assert!(Gp::fit(&[vec![0.0], vec![0.1, 0.2], vec![1.0]], &y).is_err());
        assert!(Gp::fit(&vec![vec![]; 3], &y).is_err());
        assert!(Gp::fit(&vec![vec![0.0; 9]; 3], &y).is_err());
        assert!(Gp::fit(&ok, &[vec![], vec![], vec![]]).is_err());
        let many: Vec<Vec<f64>> = (0..=MAX_DATA).map(|i| vec![i as f64]).collect();
        assert!(Gp::fit(&many, &vec![vec![0.0]; MAX_DATA + 1]).is_err());
        assert!(Gp::with_lengths(&ok, &y, &[0.0]).is_err());
        assert!(Gp::with_lengths(&ok, &y, &[f64::NAN]).is_err());
        assert!(Gp::with_lengths(&ok, &y, &[0.1, 0.1]).is_err());
        let gp = Gp::fit(&ok, &y).unwrap();
        assert!(gp.predict(&[f64::NAN]).is_err() && gp.predict(&[0.1, 0.2]).is_err());
        // A constant output: predicted as itself, sure of it.
        let at = gp.predict(&[0.25]).unwrap();
        assert_eq!(at.mean[0], 1.0);
        assert!(at.sigma[0] < 1e-6);
        // Points almost on top of each other still factor (the nugget grows).
        let close = vec![vec![0.5], vec![0.5 + 1e-15], vec![1.0]];
        assert!(Gp::fit(&close, &[vec![0.0], vec![1.0], vec![0.5]]).is_ok());
    }
}

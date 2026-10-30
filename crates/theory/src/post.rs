//! A conducting post across a rectangular waveguide: a round metal cylinder of radius r₀ from
//! one broad wall to the other (an inductive post), centred at (x₀, z₀), in an empty guide of
//! width a, fed by TE₁₀.
//!
//! **The problem is two-dimensional.** The post runs the whole height, so the fields do not
//! depend on y: E = E_y(x, z) ŷ with (∇² + k²) E_y = 0 between the side walls (E_y = 0 at x = 0
//! and x = a) and E_y = 0 on the post (N. Marcuvitz, *Waveguide Handbook*, MIT Rad. Lab. Ser.
//! 10, McGraw-Hill 1951, §5.11; L. Lewin, *Theory of Waveguides*, Butterworth 1975, ch. 6).
//!
//! **Integral equation.** The scattered field is that of the current J_y on the post's surface,
//! E_s = −jωμ₀ ∫ G J dl, with the Green's function of the parallel-plate region in modal form
//! (R. E. Collin, *Field Theory of Guided Waves*, 2nd ed., IEEE Press 1991, ch. 5),
//!
//! G(x, z; x′, z′) = Σ_m sin(mπx/a) sin(mπx′/a) e^{−γ_m|z − z′|} / (a γ_m),
//! γ_m = √((mπ/a)² − k²) (j√(k² − (mπ/a)²) for propagating m).
//!
//! The series converges slowly near the source; its static part (γ_m → mπ/a) sums in closed form
//! (Kummer's transformation of the series; Lewin ch. 6),
//!
//! G₀ = (1/4π) ln[(1 − 2q cos θ₊ + q²)/(1 − 2q cos θ₋ + q²)], q = e^{−π|z − z′|/a},
//! θ± = π(x ± x′)/a,
//!
//! and G = G₀ + Σ_m sin sin [e^{−γ_m|Δz|}/(aγ_m) − e^{−mπ|Δz|/a}/(mπ)], whose terms fall as
//! m⁻³. With σ(t) = jωμ₀ r₀ J(t) on the circle r(t) = (x₀ + r₀ cos t, z₀ + r₀ sin t), the field
//! vanishes on the post when
//!
//! ∫₀^{2π} G(r(t), r(τ)) σ(τ) dτ = E_inc(r(t)),
//!
//! a first-kind equation with a logarithmic kernel: G = −(1/4π) J₀(k|r(t) − r(τ)|) ln(4 sin²((t −
//! τ)/2)) + smooth (the Helmholtz singularity, H₀⁽²⁾ = J₀ − jY₀ with Y₀'s logarithm times J₀).
//!
//! **Nyström method** (R. Kress, *Linear Integral Equations*, 3rd ed., Springer 2014, §12.3 and
//! ch. 13; D. Colton, R. Kress, *Inverse Acoustic and Electromagnetic Scattering Theory*, 3rd ed.,
//! Springer 2013, §3.6): on 2n equidistant nodes t_j = jπ/n the logarithm is integrated by the
//! trigonometric-interpolation weights
//!
//! R_j(t) = −(2π/n) Σ_{m=1}^{n−1} cos(m(t − t_j))/m − (π/n²) cos(n(t − t_j))
//!
//! (times the J₀ factor at the node pair), the smooth rest by the trapezoidal rule; for analytic data the error falls exponentially with
//! n. The reflected and transmitted TE₁₀ amplitudes are the m = 1 term far from the post:
//! S₁₁ = −(1/aγ₁) ∫ sin(πx′/a) e^{−γ₁z′} σ dτ and S₂₁ = (1 − (1/aγ₁) ∫ sin(πx′/a) e^{γ₁z′} σ dτ)
//! e^{−γ₁L}, referred to the planes z = 0 and z = L (time dependence e^{jωt}).
//!
//! Nothing here shares code with the finite element solver: it is the independent reference of
//! the oracle's inductive post.

use std::f64::consts::PI;

use emcraft_model::units::C0;
use num_complex::Complex64 as C;

use crate::TheoryError;

/// The most Nyström nodes (2n).
pub const MAX_NODES: usize = 1024;
/// Terms of the remainder series near the source (its terms fall as m⁻³, so the tail beyond M is
/// about k²a/(4π³M²) relative to G's size: ~10⁻⁸ in X band).
const MAX_TERMS: usize = 20_000;

/// A post across an empty guide between two port planes z = 0 and z = `length`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InductivePost {
    /// The broad wall a (m).
    pub width: f64,
    /// The guide's length between the port planes (m).
    pub length: f64,
    /// The post's centre (m).
    pub x: f64,
    pub z: f64,
    pub radius: f64,
}

/// The post's scattering at one frequency, referred to the port planes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostSolution {
    pub s11: C,
    pub s21: C,
    /// S₂₂ and S₁₂ (port 2 excited): equal to S₁₁ and S₂₁ for a post centred along z, and S₁₂ =
    /// S₂₁ always (reciprocity).
    pub s22: C,
    pub s12: C,
}

impl InductivePost {
    fn check(&self, frequency: f64, nodes: usize) -> Result<(f64, C), TheoryError> {
        let ok = |v: f64| v.is_finite() && v > 0.0;
        if !(ok(self.width) && ok(self.length) && ok(self.radius) && self.x.is_finite() && self.z.is_finite()) {
            return Err(TheoryError::Invalid("post: dimensions must be finite, the width, length and radius positive".into()));
        }
        if !(self.x - self.radius > 0.0 && self.x + self.radius < self.width && self.z - self.radius > 0.0 && self.z + self.radius < self.length) {
            return Err(TheoryError::Invalid("post: it must lie inside the guide, clear of the side walls and the port planes".into()));
        }
        if !ok(frequency) {
            return Err(TheoryError::Invalid(format!("frequency {frequency}")));
        }
        if !(8..=MAX_NODES).contains(&nodes) || !nodes.is_multiple_of(2) {
            return Err(TheoryError::Invalid(format!("post: nodes must be even, 8 to {MAX_NODES}")));
        }
        let k = 2.0 * PI * frequency / C0;
        if k <= PI / self.width {
            return Err(TheoryError::Invalid("post: TE10 is below cut-off".into()));
        }
        // The first-kind equation fails at the disc's interior Dirichlet resonances (k r₀ = 2.405
        // first); keep well below.
        if k * self.radius > 2.0 {
            return Err(TheoryError::Invalid("post: k r₀ > 2 (near the post's interior resonance, where the integral equation is singular)".into()));
        }
        Ok((k, gamma(1, self.width, k)))
    }

    /// The post's S-parameters at `frequency` (Hz) on `nodes` Nyström nodes (even, 8 to
    /// [`MAX_NODES`]; 64 converge to rounding for posts up to a tenth of the guide's width).
    pub fn solve(&self, frequency: f64, nodes: usize) -> Result<PostSolution, TheoryError> {
        let (k, g1) = self.check(frequency, nodes)?;
        let (a, r0) = (self.width, self.radius);
        let n = nodes / 2;
        let t: Vec<f64> = (0..nodes).map(|j| PI * j as f64 / n as f64).collect();
        let pts: Vec<(f64, f64)> = t.iter().map(|&t| (self.x + r0 * t.cos(), self.z + r0 * t.sin())).collect();
        let h = PI / n as f64;
        // The matrix: K₁(t_i, t_j) R_j(t_i) + h K₂(t_i, t_j), K₁ = −J₀(k|r_i − r_j|)/4π.
        let mut m = vec![C::new(0.0, 0.0); nodes * nodes];
        for i in 0..nodes {
            for j in 0..nodes {
                let d = t[i] - t[j];
                let mut r = -(PI / (n * n) as f64) * (n as f64 * d).cos();
                for q in 1..n {
                    r -= (2.0 * PI / n as f64) * (q as f64 * d).cos() / q as f64;
                }
                let dist = (pts[i].0 - pts[j].0).hypot(pts[i].1 - pts[j].1);
                let k1 = -j0(k * dist) / (4.0 * PI);
                let k2 = if i == j {
                    // The limit of G − K₁ ln(4 sin²): G₀'s denominator over 4 sin² tends to
                    // (π r₀/a)², its numerator to 4 sin²(πx/a); the remainder is continuous.
                    let (x, _) = pts[i];
                    let s = (PI * x / a).sin();
                    C::new(((4.0 * s * s).ln() - (PI * r0 / a).powi(2).ln()) / (4.0 * PI), 0.0) + remainder(a, k, pts[i], pts[j])
                } else {
                    let s = (d / 2.0).sin();
                    green(a, k, pts[i], pts[j]) - k1 * (4.0 * s * s).ln()
                };
                m[i * nodes + j] = C::new(k1 * r, 0.0) + k2 * h;
            }
        }
        let mode = |p: (f64, f64), sign: f64| (PI * p.0 / a).sin() * (-g1 * sign * p.1).exp();
        let solve_for = |inc: &dyn Fn((f64, f64)) -> C| -> Result<Vec<C>, TheoryError> {
            let b: Vec<C> = pts.iter().map(|p| inc(*p)).collect();
            dense_solve(m.clone(), b, nodes).ok_or_else(|| TheoryError::Invalid("post: singular Nyström system".into()))
        };
        // Port 1: E_inc = sin(πx/a) e^{−γ₁z}.
        let s1 = solve_for(&|p| mode(p, 1.0))?;
        // Port 2: E_inc = sin(πx/a) e^{γ₁(z − L)}.
        let l = self.length;
        let s2 = solve_for(&|p| mode(p, -1.0) * (-g1 * l).exp())?;
        let project = |sigma: &[C], sign: f64| -> C { pts.iter().zip(sigma).map(|(p, s)| mode(*p, sign) * s).sum::<C>() * h / (a * g1) };
        let back = |sigma: &[C]| -project(sigma, 1.0); // amplitude of sin e^{+γz}
        let forward = |sigma: &[C]| -project(sigma, -1.0); // amplitude of sin e^{−γz}
        let decay = (-g1 * l).exp();
        Ok(PostSolution {
            s11: back(&s1),
            s21: (1.0 + forward(&s1)) * decay,
            // Port 2's incident wave is e^{γ(z − L)}, unit at z = L; its reflection there is
            // c e^{−γL}.
            s22: forward(&s2) * decay,
            s12: back(&s2) + decay,
        })
    }
}

/// γ_m = √((mπ/a)² − k²), on the branch Re ≥ 0, Im ≥ 0 (j√(k² − (mπ/a)²) when it propagates).
fn gamma(m: usize, a: f64, k: f64) -> C {
    let kc = m as f64 * PI / a;
    let d = kc * kc - k * k;
    if d >= 0.0 { C::new(d.sqrt(), 0.0) } else { C::new(0.0, (-d).sqrt()) }
}

/// The Bessel function J₀ by its power series (arguments here are below 2 k r₀ ≤ 4, where 40
/// terms reach rounding).
fn j0(x: f64) -> f64 {
    let q = -x * x / 4.0;
    let (mut term, mut sum) = (1.0, 1.0);
    for k in 1..40 {
        term *= q / (k * k) as f64;
        sum += term;
    }
    sum
}

/// The static part G₀ in closed form.
fn static_green(a: f64, p: (f64, f64), q: (f64, f64)) -> f64 {
    let qq = (-PI * (p.1 - q.1).abs() / a).exp();
    let plus = 1.0 - 2.0 * qq * (PI * (p.0 + q.0) / a).cos() + qq * qq;
    let minus = 1.0 - 2.0 * qq * (PI * (p.0 - q.0) / a).cos() + qq * qq;
    (plus / minus).ln() / (4.0 * PI)
}

/// G − G₀: the modal series less its static part.
fn remainder(a: f64, k: f64, p: (f64, f64), q: (f64, f64)) -> C {
    let dz = (p.1 - q.1).abs();
    let mut sum = C::new(0.0, 0.0);
    for m in 1..=MAX_TERMS {
        let kc = m as f64 * PI / a;
        let decay = (-kc * dz).exp();
        // Beyond the propagating modes both parts are below 10⁻¹⁷ of the first once e^{−mπ|Δz|/a}
        // is.
        if decay < 1e-17 && kc > 2.0 * k {
            break;
        }
        let g = gamma(m, a, k);
        let s = (kc * p.0).sin() * (kc * q.0).sin();
        sum += s * ((-g * dz).exp() / (g * a) - decay / (kc * a));
    }
    sum
}

/// The parallel-plate Green's function at two distinct points.
fn green(a: f64, k: f64, p: (f64, f64), q: (f64, f64)) -> C {
    C::new(static_green(a, p, q), 0.0) + remainder(a, k, p, q)
}

/// Gaussian elimination with partial pivoting (the first largest pivot: deterministic).
pub(crate) fn dense_solve(mut a: Vec<C>, mut b: Vec<C>, n: usize) -> Option<Vec<C>> {
    for c in 0..n {
        let mut piv = c;
        for r in c + 1..n {
            if a[r * n + c].norm() > a[piv * n + c].norm() {
                piv = r;
            }
        }
        if a[piv * n + c].norm() == 0.0 {
            return None;
        }
        if piv != c {
            for j in 0..n {
                a.swap(c * n + j, piv * n + j);
            }
            b.swap(c, piv);
        }
        for r in c + 1..n {
            let f = a[r * n + c] / a[c * n + c];
            if f == C::new(0.0, 0.0) {
                continue;
            }
            for j in c..n {
                let x = a[c * n + j];
                a[r * n + j] -= f * x;
            }
            let x = b[c];
            b[r] -= f * x;
        }
    }
    let mut x = vec![C::new(0.0, 0.0); n];
    for r in (0..n).rev() {
        let mut s = b[r];
        for j in r + 1..n {
            s -= a[r * n + j] * x[j];
        }
        x[r] = s / a[r * n + r];
    }
    x.iter().all(|v| v.re.is_finite() && v.im.is_finite()).then_some(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The oracle's post: WR-90, 3 mm across, centred in 30 mm.
    fn wr90() -> InductivePost {
        InductivePost { width: 22.86e-3, length: 30e-3, x: 11.43e-3, z: 15e-3, radius: 1.5e-3 }
    }

    #[test]
    fn converges_conserves_power_and_is_reciprocal() {
        let post = wr90();
        let coarse = post.solve(10e9, 16).unwrap();
        let mid = post.solve(10e9, 32).unwrap();
        let fine = post.solve(10e9, 64).unwrap();
        // Exponential convergence in the node count.
        assert!((mid.s11 - fine.s11).norm() < 1e-9, "{:?} {:?}", mid.s11, fine.s11);
        assert!((coarse.s11 - fine.s11).norm() < 1e-5);
        let power = fine.s11.norm_sqr() + fine.s21.norm_sqr();
        assert!((power - 1.0).abs() < 1e-9, "{power}");
        assert!((fine.s12 - fine.s21).norm() < 1e-9, "reciprocity");
        assert!((fine.s22 - fine.s11).norm() < 1e-9, "symmetric about z₀");
        // A shunt inductance: referred to the post's centre, Γ = (−1 + 2jX)/(1 + 4X²) with X > 0.
        let k = 2.0 * PI * 10e9 / C0;
        let beta = (k * k - (PI / post.width).powi(2)).sqrt();
        let gamma = fine.s11 * C::new(0.0, 2.0 * beta * post.z).exp();
        assert!(gamma.re < -0.5 && gamma.im > 0.0 && fine.s11.norm() < 1.0, "{gamma:?}");
    }

    #[test]
    fn a_thinner_post_reflects_less() {
        let thick = wr90().solve(10e9, 48).unwrap();
        let thin = InductivePost { radius: 0.5e-3, ..wr90() }.solve(10e9, 48).unwrap();
        assert!(thin.s11.norm() < thick.s11.norm());
        // An offset post couples more weakly to TE10 (whose field is smaller there).
        let off = InductivePost { x: 5e-3, ..wr90() }.solve(10e9, 48).unwrap();
        assert!(off.s11.norm() < thick.s11.norm());
        assert!((off.s11.norm_sqr() + off.s21.norm_sqr() - 1.0).abs() < 1e-8);
    }

    #[test]
    fn the_green_function_is_symmetric_and_vanishes_on_the_walls() {
        let a = 22.86e-3;
        let k = 2.0 * PI * 10e9 / C0;
        let (p, q) = ((3e-3, 1e-3), (17e-3, 2.5e-3));
        assert!((green(a, k, p, q) - green(a, k, q, p)).norm() < 1e-14);
        assert!(green(a, k, (0.0, 1e-3), q).norm() < 1e-12);
        assert!(green(a, k, (a, 1e-3), q).norm() < 1e-9);
        // Far apart it is the TE10 term alone.
        let far = (5e-3, 0.2);
        let te10 = (PI * far.0 / a).sin() * (PI * q.0 / a).sin() * (-gamma(1, a, k) * (far.1 - q.1)).exp() / (a * gamma(1, a, k));
        assert!((green(a, k, far, q) - te10).norm() < 1e-9 * te10.norm());
    }

    #[test]
    fn hostile_posts_are_refused() {
        let ok = wr90();
        assert!(InductivePost { radius: -1.0, ..ok }.solve(10e9, 32).is_err());
        assert!(InductivePost { x: 0.5e-3, ..ok }.solve(10e9, 32).is_err());
        assert!(InductivePost { z: 29.0e-3, ..ok }.solve(10e9, 32).is_err());
        assert!(InductivePost { width: f64::NAN, ..ok }.solve(10e9, 32).is_err());
        assert!(ok.solve(f64::INFINITY, 32).is_err());
        assert!(ok.solve(5e9, 32).is_err(), "below cut-off");
        assert!(ok.solve(10e9, 7).is_err());
        assert!(ok.solve(10e9, 1 << 20).is_err());
        assert!(InductivePost { radius: 10e-3, width: 100e-3, x: 50e-3, z: 50e-3, length: 100e-3 }.solve(10e9, 32).is_err(), "k r₀ > 2");
    }
}

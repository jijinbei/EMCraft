//! Walls of finite conductivity in the time domain: Leontovich's surface-impedance condition
//! E_t = Z_s n̂ × H, Z_s = √(jωμ₀/σ) = (1 + j)√(ωμ₀/2σ) (time dependence e^{jωt}, n̂ out of the
//! wall), the same condition the frequency-domain solver imposes.
//!
//! **The surface admittance as branches.** Z_s ∝ √s (s = jω) has no finite rational form, so
//! its time-domain kernel is a convolution with a t^{−1/2} memory; recursive convolution with a
//! sum of exponentials turns it into a few first-order updates per wall edge (J. G. Maloney,
//! G. S. Smith, *The use of surface impedance concepts in the finite-difference time-domain
//! method*, IEEE Trans. Antennas Propag. 40(1):38–48, 1992; K. S. Oh, J. E. Schutt-Ainé,
//! *An efficient implementation of surface impedance boundary conditions for the
//! finite-difference time-domain method*, IEEE Trans. Antennas Propag. 43(7):660–666, 1995).
//! EMCraft takes the admittance Y_s = 1/Z_s = √(σ/μ₀) s^{−1/2}, whose Stieltjes form
//!
//! s^{−1/2} = (1/π) ∫₀^∞ x^{−1/2} / (s + x) dx = (1/π) ∫ e^{u/2} / (s + e^u) du  (x = e^u)
//!
//! is a continuous sum of first-order (resistor–inductor) branches, each passive. The
//! trapezoidal rule in u converges exponentially (the integrand is analytic in the strip
//! |Im u| < π/2: error ≈ e^{−π²/Δu}; L. N. Trefethen, J. A. C. Weideman, *The exponentially
//! convergent trapezoidal rule*, SIAM Review 56(3):385–458, 2014, §14), the end weights carry
//! the Euler–Maclaurin correction and the two tails beyond the nodes are a branch each, matched
//! to their first two moments. Every weight and pole is positive: the wall stays passive at
//! every frequency, while the fit holds to about 1e-5 over the run's band.
//!
//! **On the grid.** A wall edge (a tangential E edge with cells on one side only, a flat wall)
//! is updated by Ampère's law over the half of its dual face inside the guide: the H beyond the
//! wall is replaced by the wall's tangential H, Y_s E by the condition (the orientations cancel:
//! the wall's surface current only drains the edge). The tangential H half a cell inside stands
//! for the
//! wall's, a second-order approximation since ∂H_t/∂n = 0 on a good conductor. Each branch's
//! current dJ/dt = −x J + √(σ/μ₀) w E is advanced by the trapezoidal rule and enters Ampère's
//! law averaged over the step, solved for Eⁿ⁺¹ together with the edge's other terms
//! ([`crate::dispersion`]): the bilinear map of the admittance, centred in time like the rest of
//! the leapfrog and passive for every branch however fast (a branch far beyond 1/Δt acts as
//! the conductance it is), its frequencies warped by (ωΔt)²/12.

use std::f64::consts::PI;

use emcraft_model::units::MU0;

use crate::dispersion::Term;

/// Node spacing of the trapezoidal rule in u = ln x: aliasing error about e^{−π²/Δu} ≈ 2e-6.
const DU: f64 = 0.75;
/// The nodes reach this factor below the band's lowest angular frequency and above its highest;
/// the tails beyond are one branch each.
const REACH: f64 = 1e4;

/// Branches (w, x) with s^{−1/2} ≈ Σ w / (s + x) for s = jω, ω from `w_lo` to `w_hi` (rad/s,
/// 0 < w_lo ≤ w_hi): every w and x > 0.
pub(crate) fn inverse_sqrt(w_lo: f64, w_hi: f64) -> Vec<(f64, f64)> {
    let (u_lo, u_hi) = ((w_lo / REACH).ln(), (w_hi * REACH).ln());
    let n = ((u_hi - u_lo) / DU).ceil().max(1.0) as usize;
    let du = (u_hi - u_lo) / n as f64;
    let mut out = Vec::with_capacity(n + 3);
    for k in 0..=n {
        let u = u_lo + k as f64 * du;
        // Trapezoidal ends with Euler–Maclaurin's first correction: the integrand there grows or
        // decays as e^{±u/2}, so ∫ − T ≈ (Δu²/24)(g(a) + g(b)).
        let end = if k == 0 || k == n { 0.5 + du / 24.0 } else { 1.0 };
        out.push((du / PI * (0.5 * u).exp() * end, u.exp()));
    }
    let (x_lo, x_hi) = (u_lo.exp(), u_hi.exp());
    // ∫₀^{x_lo}: 2√x_lo/s − (2/3) x_lo^{3/2}/s² + …, one branch at x_lo/3.
    out.push((2.0 * x_lo.sqrt() / PI, x_lo / 3.0));
    // ∫_{x_hi}^∞: 2/√x_hi − (2/3) s x_hi^{−3/2} + …, one branch at 3 x_hi.
    out.push((6.0 * x_hi.sqrt() / PI, 3.0 * x_hi));
    out
}

/// The surface terms of a wall edge with walls of conductivity `sigma` (S/m): for a time step
/// `dt` and the edge's dual step `d` across the wall (its half cell inside, or the full dual step
/// with the edge's ε scaled to match), each branch's Q = (Δt/2d) J by the trapezoidal rule,
/// Qⁿ⁺¹ = a Qⁿ + kb (Eⁿ⁺¹ + Eⁿ), a = (1 − xΔt/2)/(1 + xΔt/2).
pub(crate) fn wall_terms(branches: &[(f64, f64)], sigma: f64, dt: f64, d: f64) -> Vec<Term> {
    let y0 = (sigma / MU0).sqrt();
    branches
        .iter()
        .map(|&(w, x)| {
            let half = 0.5 * x * dt;
            Term::Surface { ka: (1.0 - half) / (1.0 + half), kb: dt / (2.0 * d) * y0 * w * 0.5 * dt / (1.0 + half) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex64 as C;

    fn worst(w_lo: f64, w_hi: f64, over: (f64, f64)) -> f64 {
        let b = inverse_sqrt(w_lo, w_hi);
        assert!(b.iter().all(|&(w, x)| w > 0.0 && x > 0.0 && w.is_finite() && x.is_finite()));
        (0..=200)
            .map(|i| over.0 * (over.1 / over.0).powf(i as f64 / 200.0))
            .map(|w| {
                let s = C::new(0.0, w);
                let fit: C = b.iter().map(|&(a, x)| a / (s + x)).sum();
                (fit * s.sqrt() - 1.0).norm()
            })
            .fold(0.0, f64::max)
    }

    #[test]
    fn the_branches_fit_the_inverse_square_root_over_the_band() {
        let (lo, hi) = (2.0 * PI * 8e9, 2.0 * PI * 12e9);
        let e = worst(lo, hi, (lo, hi));
        assert!(e < 2e-5, "{e:e}");
        // A decade and beyond the band by 10: still close.
        let e = worst(2.0 * PI * 1e9, 2.0 * PI * 1e10, (2.0 * PI * 1e8, 2.0 * PI * 1e11));
        assert!(e < 1e-4, "{e:e}");
        // Some 25 branches for a band.
        assert!(inverse_sqrt(lo, hi).len() < 30);
    }
}

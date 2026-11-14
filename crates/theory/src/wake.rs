//! What a bunch leaves in a closed cavity: the loss factor (docs/adr/0012-particles.md, stage C).
//!
//! A charge q crossing a cavity along a line at speed βc gives each mode the energy k q², its
//! loss factor k = |V|²/(4U), V = ∫E_z e^{jωz/βc} dz along the line and U the mode's stored
//! energy — k = ω(R/Q)/4 with R/Q = |V|²/(ωU) (P. B. Wilson, *Introduction to wakefields and
//! wake potentials*, SLAC-PUB-4547, 1989, §3; T. P. Wangler, *RF Linear Accelerators*, 2nd ed.,
//! Wiley 2008, §11.6). A Gaussian bunch of rms duration σ_t gives mode n the fraction
//! |F(ω)|² = e^{−(ωσ_t)²} of it, so a bunch of charge q leaves q² Σ k_n e^{−(ω_n σ_t)²} in the
//! cavity once it has gone (a closed cavity, the bunch entering through one end wall and
//! leaving through the other: no field of its own is left, only the modes').
//!
//! **The pillbox** of radius a and length L on its axis: the modes with E_z there are TM0np,
//! E_z = J₀(x_n r/a) cos(pπz/L), E_r = (pπ/L)(a/x_n) J₁(x_n r/a) sin(pπz/L), ω² = c²((x_n/a)² +
//! (pπ/L)²), x_n the n-th zero of J₀. With E₀ = 1, U = (ε₀/2) π a² J₁(x_n)² L (c_p +
//! ½ (pπa/(L x_n))²), c_p = 1 for p = 0 and ½ otherwise, and
//! V = ∫₀^L cos(pπz/L) e^{jκz} dz, κ = ω/(βc), in closed form.

use num_complex::Complex64 as C;
use std::f64::consts::PI;

use emcraft_model::units::{C0, EPS0};

use crate::bessel::{j, zeros};

/// A mode's frequency (Hz) and loss factor (V/C).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModeLoss {
    pub n: usize,
    pub p: usize,
    pub frequency: f64,
    pub k: f64,
}

/// ∫₀^L e^{jsz} dz.
fn phase_integral(s: f64, l: f64) -> C {
    if (s * l).abs() < 1e-9 {
        return C::new(l, 0.0);
    }
    (C::from_polar(1.0, s * l) - 1.0) / C::new(0.0, s)
}

/// The TM0np modes of a pillbox of radius `a` and length `l` (m) up to `f_max` (Hz), each with its
/// loss factor (V/C) to a charge crossing on the axis at `beta` c; empty for sizes or speeds
/// that are not finite numbers > 0 (β ≤ 1).
///
/// ```
/// use emcraft_theory::wake::pillbox_modes;
/// // TM010 of a 10 mm, 13.06 mm pillbox: 11.47 GHz; to a charge at c,
/// // k = L T²/(2π ε₀ a² J₁(x₀₁)²) ≈ 3.5 V/pC.
/// let m = pillbox_modes(0.01, 0.01306, 1.0, 20e9);
/// assert!((m[0].frequency / 11.474e9 - 1.0).abs() < 1e-3);
/// assert!((m[0].k * 1e-12 - 3.54).abs() < 0.02);
/// assert!(pillbox_modes(-1.0, 0.01, 1.0, 20e9).is_empty());
/// ```
pub fn pillbox_modes(a: f64, l: f64, beta: f64, f_max: f64) -> Vec<ModeLoss> {
    let ok = |x: f64| x.is_finite() && x > 0.0;
    if !(ok(a) && ok(l) && ok(beta) && beta <= 1.0 && ok(f_max)) {
        return Vec::new();
    }
    let k_max = 2.0 * PI * f_max / C0;
    let roots = zeros(|x| j(0, x), 400, (k_max * a + 3.0).min(1300.0));
    let mut out = Vec::new();
    for (n, x) in roots.iter().enumerate() {
        let kr = x / a;
        if kr > k_max {
            break;
        }
        let j1 = j(1, *x);
        for p in 0..=((k_max * l / PI) as usize).min(2000) {
            let kz = p as f64 * PI / l;
            let omega = C0 * (kr * kr + kz * kz).sqrt();
            if omega > C0 * k_max {
                break;
            }
            let kappa = omega / (beta * C0);
            // cos(kz z) = (e^{j kz z} + e^{−j kz z})/2.
            let v = 0.5 * (phase_integral(kappa + kz, l) + phase_integral(kappa - kz, l));
            let cp = if p == 0 { 1.0 } else { 0.5 };
            let u = 0.5 * EPS0 * PI * a * a * j1 * j1 * l * (cp + 0.5 * (kz / kr).powi(2));
            out.push(ModeLoss { n: n + 1, p, frequency: omega / (2.0 * PI), k: v.norm_sqr() / (4.0 * u) });
        }
    }
    out.sort_by(|x, y| x.frequency.total_cmp(&y.frequency));
    out
}

/// The energy per charge squared (V/C) a Gaussian bunch of rms duration `sigma_t` (s) at
/// `beta` c leaves in the pillbox: Σ k_n e^{−(ω_n σ_t)²}, the modes taken until the bunch's
/// spectrum has fallen below e^{−40}.
///
/// ```
/// use emcraft_theory::wake::{pillbox_loss_factor, pillbox_modes};
/// // A long bunch sees TM010 alone, a short one more modes.
/// let k010 = pillbox_modes(0.01, 0.01306, 1.0, 12e9)[0].k;
/// let long = pillbox_loss_factor(0.01, 0.01306, 1.0, 60e-12).unwrap();
/// assert!(long < k010 * 1e-3);
/// let short = pillbox_loss_factor(0.01, 0.01306, 1.0, 1e-12).unwrap();
/// assert!(short > k010);
/// ```
pub fn pillbox_loss_factor(a: f64, l: f64, beta: f64, sigma_t: f64) -> Option<f64> {
    if !(sigma_t.is_finite() && sigma_t > 0.0) {
        return None;
    }
    let f_max = 40f64.sqrt() / (2.0 * PI * sigma_t);
    let modes = pillbox_modes(a, l, beta, f_max);
    (!modes.is_empty()).then(|| modes.iter().map(|m| m.k * (-(2.0 * PI * m.frequency * sigma_t).powi(2)).exp()).sum())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tm010s_loss_factor_is_its_closed_form() {
        // k = L T²/(2π ε₀ a² J₁(x₀₁)²), T = sin(x)/x, x = ωL/(2βc) (Wangler §2.3, §11.6).
        let (a, l) = (0.01, 0.01306);
        for beta in [1.0, 0.8] {
            let m = pillbox_modes(a, l, beta, 12e9)[0];
            let x = 2.0 * PI * m.frequency * l / (2.0 * beta * C0);
            let t = x.sin() / x;
            let x01 = 2.404_825_557_695_773;
            let want = l * t * t / (2.0 * PI * EPS0 * a * a * j(1, x01).powi(2));
            assert!((m.k / want - 1.0).abs() < 1e-9, "{} against {want}", m.k);
        }
    }

    #[test]
    fn the_sum_converges() {
        let k = |s: f64| pillbox_loss_factor(0.01, 0.01306, 1.0, s).unwrap_or(0.0);
        // Shorter bunches lose more; the sum is settled well inside its cut.
        assert!(k(3e-12) > k(6e-12) && k(6e-12) > k(12e-12));
    }
}

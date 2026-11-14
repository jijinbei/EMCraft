//! Closed forms of plasmas and beams for the particle-in-cell solver's verification
//! (docs/adr/0012-particles.md, stage B).
//!
//! **The plasma frequency** ω_p = √(n q²/(ε₀ m)): a cold plasma's charge displaced by ξ sets up
//! E = n q ξ/ε₀, which pulls it back at ω_p whatever the wavelength (C. K. Birdsall,
//! A. B. Langdon, *Plasma Physics via Computer Simulation*, 1991, §4-2; F. F. Chen,
//! *Introduction to Plasma Physics and Controlled Fusion*, 3rd ed., Springer 2016, §4.3).
//!
//! **The space-charge-limited current of a planar diode** (C. D. Child, Phys. Rev. 32:492,
//! 1911; I. Langmuir, Phys. Rev. 2:450, 1913): particles leaving a plane cathode at rest,
//! accelerated by V across a gap d, carry at most J = (4ε₀/9) √(2q/m) V^{3/2}/d², when the field
//! at the cathode is zero. Without the slow-particle approximation (H. R. Jory, A. W.
//! Trivelpiece, *Exact relativistic solution for the one-dimensional diode*, J. Appl. Phys.
//! 40:3924, 1969): in the steady state Poisson's law φ″ = J/(ε₀ v(φ)), with energy conservation
//! γ(φ) = 1 + qφ/(mc²), integrates once to φ′² = (2Jmc/(ε₀q)) √(γ² − 1) (zero field at the
//! cathode, ∫dφ/v = (mc/q)√(γ² − 1)), and once more to
//!
//! J = (ε₀ q/(2mc d²)) [∫₀^V (γ² − 1)^{−1/4} dφ]²,
//!
//! which tends to Child's law as qV/(mc²) → 0. The integral is taken with φ = V s⁴, which makes
//! its integrand smooth, by Simpson's rule.

use emcraft_model::units::{C0, EPS0};

/// Simpson panels of the relativistic diode's integral: its integrand is smooth after the
/// substitution, so the rule's error is far below 1e-12.
const PANELS: usize = 2000;

/// The plasma frequency ω_p (rad/s) of `density` (m⁻³) particles of `charge` (C) and `mass`
/// (kg).
///
/// ```
/// use emcraft_theory::plasma::plasma_frequency;
/// // 10¹⁶ electrons per m³: f_p = ω_p/2π ≈ 0.898 GHz.
/// let w = plasma_frequency(1e16, -1.602_176_634e-19, 9.109_383_701_5e-31);
/// assert!((w / (2.0 * std::f64::consts::PI) / 0.8979e9 - 1.0).abs() < 1e-3);
/// ```
pub fn plasma_frequency(density: f64, charge: f64, mass: f64) -> f64 {
    (density * charge * charge / (EPS0 * mass)).sqrt()
}

/// Child's space-charge-limited current density (A/m²) of particles of `charge` (C, its size)
/// and `mass` (kg) across `gap` (m) at `voltage` (V), slow particles.
///
/// ```
/// use emcraft_theory::plasma::child_langmuir;
/// // Electrons: the perveance 2.334 µA/V^{3/2} on a 1 m² cathode 1 m away.
/// let j = child_langmuir(1.0, 1.0, 1.602_176_634e-19, 9.109_383_701_5e-31);
/// assert!((j / 2.334e-6 - 1.0).abs() < 1e-3);
/// ```
pub fn child_langmuir(voltage: f64, gap: f64, charge: f64, mass: f64) -> f64 {
    4.0 * EPS0 / 9.0 * (2.0 * charge.abs() / mass).sqrt() * voltage.powf(1.5) / (gap * gap)
}

/// The space-charge-limited current density (A/m²) at any speed (Jory & Trivelpiece 1969), as
/// [`child_langmuir`]; `None` for a voltage, gap, charge or mass that is not a finite number > 0.
///
/// ```
/// use emcraft_theory::plasma::{child_langmuir, child_langmuir_relativistic};
/// let (q, m) = (1.602_176_634e-19, 9.109_383_701_5e-31);
/// // At 10 V Child's law holds; at 100 kV the electrons carry 0.98 of what it says, at 1 MV 0.85.
/// let ratio = |v: f64| child_langmuir_relativistic(v, 1e-2, q, m).unwrap() / child_langmuir(v, 1e-2, q, m);
/// assert!((ratio(10.0) - 1.0).abs() < 1e-4);
/// assert!((ratio(1e5) - 0.9799).abs() < 1e-4 && (ratio(1e6) - 0.8506).abs() < 1e-4);
/// assert!(child_langmuir_relativistic(-1.0, 1e-2, q, m).is_none());
/// ```
pub fn child_langmuir_relativistic(voltage: f64, gap: f64, charge: f64, mass: f64) -> Option<f64> {
    let q = charge.abs();
    let ok = |x: f64| x.is_finite() && x > 0.0;
    if !(ok(voltage) && ok(gap) && ok(q) && ok(mass)) {
        return None;
    }
    // γ² − 1 = a s⁴ (2 + a s⁴) at φ = V s⁴, a = qV/(mc²); dφ = 4V s³ ds, so the integrand is
    // 4V s² (a (2 + a s⁴))^{−1/4}.
    let a = q * voltage / (mass * C0 * C0);
    let f = |s: f64| 4.0 * voltage * s * s * (a * (2.0 + a * s.powi(4))).powf(-0.25);
    let h = 1.0 / PANELS as f64;
    let inner: f64 = (1..PANELS).map(|k| f(k as f64 * h) * if k % 2 == 1 { 4.0 } else { 2.0 }).sum();
    let integral = h / 3.0 * (f(0.0) + inner + f(1.0));
    let j = EPS0 * q / (2.0 * mass * C0 * gap * gap) * integral * integral;
    j.is_finite().then_some(j)
}

#[cfg(test)]
mod tests {
    use super::*;

    const Q: f64 = 1.602_176_634e-19;
    const M: f64 = 9.109_383_701_5e-31;

    #[test]
    fn the_relativistic_diode_tends_to_childs_law_and_falls_below_it() {
        // Below Child's law, the more so the faster the particles.
        let ratio = |v: f64| child_langmuir_relativistic(v, 1e-2, Q, M).unwrap_or(0.0) / child_langmuir(v, 1e-2, Q, M);
        assert!((ratio(1.0) - 1.0).abs() < 1e-6);
        assert!(ratio(1e4) < 1.0 && ratio(1e5) < ratio(1e4) && ratio(1e6) < ratio(1e5));
        // Far above mc², the particles at c: J → 2ε₀cV/d², growing as V, not V^{3/2}.
        let high = |v: f64| child_langmuir_relativistic(v, 1e-2, Q, M).unwrap_or(0.0);
        assert!((high(2e8) / high(1e8) - 2.0).abs() < 0.1);
        assert!((high(1e9) / (2.0 * EPS0 * C0 * 1e9 / 1e-4) - 1.0).abs() < 0.1);
    }
}

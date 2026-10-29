//! Microstrip on a substrate of thickness h and permittivity εr, a strip of width W and no
//! thickness, open above (no cover): the closed-form fits of E. Hammerstad and Ø. Jensen,
//! *Accurate models for microstrip computer-aided design*, IEEE MTT-S International Microwave
//! Symposium Digest, 1980, pp. 407–409 — the quasi-static effective permittivity (within 0.2 %
//! for 0.01 ≤ W/h ≤ 100, εr ≤ 128) and characteristic impedance (within 0.01 % for the air line,
//! W/h ≤ 1000). A published fit, not an exact solution: a reference to the fit's accuracy.

use std::f64::consts::PI;

use emcraft_model::units::{C0, MU0};

/// The quasi-static effective permittivity, u = W/h.
pub fn eps_eff(u: f64, eps_r: f64) -> f64 {
    let a = 1.0 + ((u.powi(4) + (u / 52.0).powi(2)) / (u.powi(4) + 0.432)).ln() / 49.0 + (1.0 + (u / 18.1).powi(3)).ln() / 18.7;
    let b = 0.564 * ((eps_r - 0.9) / (eps_r + 3.0)).powf(0.053);
    (eps_r + 1.0) / 2.0 + (eps_r - 1.0) / 2.0 * (1.0 + 10.0 / u).powf(-a * b)
}

/// The quasi-static characteristic impedance (Ω), u = W/h.
///
/// ```
/// use emcraft_theory::microstrip;
///
/// // A strip as wide as its alumina substrate is thick (εr = 9.8) is close to 50 Ω.
/// let z = microstrip::z0(1.0, 9.8);
/// assert!((45.0..52.0).contains(&z));
/// // Wider strips have lower impedance; air lines are faster than loaded ones.
/// assert!(microstrip::z0(2.0, 9.8) < z);
/// assert!(microstrip::eps_eff(1.0, 9.8) > 1.0 && microstrip::eps_eff(1.0, 9.8) < 9.8);
/// ```
pub fn z0(u: f64, eps_r: f64) -> f64 {
    let eta0 = MU0 * C0;
    let f = 6.0 + (2.0 * PI - 6.0) * (-(30.666 / u).powf(0.7528)).exp();
    eta0 / (2.0 * PI * eps_eff(u, eps_r).sqrt()) * (f / u + (1.0 + (2.0 / u).powi(2)).sqrt()).ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fits_meet_their_limits() {
        // A wide strip: nearly all the field in the substrate; a narrow one: towards half of it
        // in each medium, slowly ((1 + 10/u)^{−ab}, ab ≈ 0.37).
        assert!((eps_eff(1000.0, 9.8) - 9.8).abs() < 0.3);
        let narrow = eps_eff(0.01, 9.8) - (9.8 + 1.0) / 2.0;
        assert!(narrow > 0.0 && narrow < 0.5, "{narrow}");
        // The 50 Ω line on alumina is close to W = h (W/h ≈ 0.95); on εr = 1 the air line,
        // whose impedance is η₀/(2π) ln(f/u + √(1 + 4/u²)).
        assert!((z0(0.95, 9.8) - 50.0).abs() < 1.5, "{}", z0(0.95, 9.8));
        assert!((eps_eff(3.0, 1.0) - 1.0).abs() < 1e-12);
    }
}

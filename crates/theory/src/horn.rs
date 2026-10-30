//! The pyramidal horn's directivity by the standard aperture formula, and the Fresnel integrals
//! it is written with.
//!
//! The aperture a₁ × b₁ of a horn flared from a feed a × b carries the feed's TE₁₀ field with
//! the quadratic phase of a wave spreading from the E-plane and H-plane apexes, ρ₁ and ρ₂
//! behind it; integrating it (Kirchhoff, no currents on the outside walls, no diffraction at
//! the rim) gives (C. A. Balanis, *Antenna Theory*, 3rd ed., Wiley 2005, §13.4.3, from §13.2.3
//! and §13.3.3 for the sectoral horns' D_E and D_H):
//!
//! D_p = (π λ² / (32 a b)) D_E D_H,
//! D_E = (64 a ρ₁ / (π λ b₁)) [C²(q) + S²(q)], q = b₁ / √(2 λ ρ₁),
//! D_H = (4 π b ρ₂ / (λ a₁)) {[C(u) − C(v)]² + [S(u) − S(v)]²},
//! u, v = (1/√2) (√(λρ₂)/a₁ ± a₁/√(λρ₂)),
//!
//! with C and S the Fresnel integrals ∫₀ˣ cos(πt²/2) dt and ∫₀ˣ sin(πt²/2) dt. The feed's a and b
//! cancel: D_p = (8 π ρ₁ ρ₂ / (a₁ b₁)) [C²(q) + S²(q)] {[C(u) − C(v)]² + [S(u) − S(v)]²}. The
//! formula is an approximation (Balanis: about a fraction of a dB for gains above ~15 dB, more
//! for small horns, where the rim's diffraction and the currents outside matter).
//!
//! A horn of axial length h from the feed to the mouth has ρ₁ = h b₁/(b₁ − b) and
//! ρ₂ = h a₁/(a₁ − a) ([`PyramidalHorn::apexes`]).
//!
//! ```
//! use emcraft_theory::horn::{PyramidalHorn, fresnel};
//!
//! // C(∞) = S(∞) = 1/2; C(1) = 0.779893, S(1) = 0.438259 (Abramowitz & Stegun, table 7.7).
//! let (c, s) = fresnel(1.0)?;
//! assert!((c - 0.779_893_400).abs() < 1e-8 && (s - 0.438_259_147).abs() < 1e-8);
//!
//! // A 45 × 35 mm horn 40 mm long on WR-90 at 10 GHz (lengths in metres).
//! let horn = PyramidalHorn { a: 22.86e-3, b: 10.16e-3, a1: 45e-3, b1: 35e-3, length: 40e-3 };
//! let d = horn.directivity(10e9)?;
//! assert!(d > 10.0 && d < 40.0);
//! assert!(PyramidalHorn { a1: 20e-3, ..horn }.directivity(10e9).is_err()); // not flared
//! # Ok::<(), emcraft_theory::TheoryError>(())
//! ```

use emcraft_model::units::C0;

use crate::TheoryError;

/// Largest argument the Fresnel integrals take (the quadrature's cost grows as x²).
pub const MAX_FRESNEL: f64 = 1e3;

/// The Fresnel integrals (C(x), S(x)): ∫₀ˣ cos(πt²/2) dt and ∫₀ˣ sin(πt²/2) dt, by composite
/// Gauss–Legendre quadrature on panels short enough for the oscillation (|x| ≤ [`MAX_FRESNEL`]).
pub fn fresnel(x: f64) -> Result<(f64, f64), TheoryError> {
    if !x.is_finite() || x.abs() > MAX_FRESNEL {
        return Err(TheoryError::Invalid(format!("Fresnel integral argument {x}")));
    }
    let (nodes, weights) = emcraft_fem::quadrature::gauss_legendre(16);
    // About two panels per half period of cos(πt²/2) near t = x.
    let panels = (x.abs() * x.abs()).ceil() as usize * 2 + 4;
    let h = x / panels as f64;
    let (mut c, mut s) = (0.0, 0.0);
    // The rule is on [0, 1]: each panel [kh, (k + 1)h].
    for k in 0..panels {
        for (t, w) in nodes.iter().zip(&weights) {
            let at = (k as f64 + t) * h;
            let phase = std::f64::consts::FRAC_PI_2 * at * at;
            c += w * phase.cos();
            s += w * phase.sin();
        }
    }
    Ok((h * c, h * s))
}

/// A pyramidal horn: the feed a × b (a across the H-plane), the mouth a1 × b1, the axial
/// length from the feed to the mouth; metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PyramidalHorn {
    pub a: f64,
    pub b: f64,
    pub a1: f64,
    pub b1: f64,
    pub length: f64,
}

impl PyramidalHorn {
    /// The distances (ρ₁, ρ₂) from the E-plane and H-plane apexes to the mouth, along the axis.
    pub fn apexes(&self) -> Result<(f64, f64), TheoryError> {
        let all = [self.a, self.b, self.a1, self.b1, self.length];
        if all.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err(TheoryError::Invalid(format!("horn {self:?}: lengths must be finite and positive")));
        }
        if self.a1 <= self.a || self.b1 <= self.b {
            return Err(TheoryError::Invalid(format!("horn {self:?}: the mouth must be larger than the feed both ways")));
        }
        Ok((self.length * self.b1 / (self.b1 - self.b), self.length * self.a1 / (self.a1 - self.a)))
    }

    /// The directivity (a ratio) at `frequency` (Hz) on the axis, by Balanis' formula.
    pub fn directivity(&self, frequency: f64) -> Result<f64, TheoryError> {
        if !frequency.is_finite() || frequency <= 0.0 {
            return Err(TheoryError::Invalid(format!("frequency {frequency}")));
        }
        let (rho1, rho2) = self.apexes()?;
        let lambda = C0 / frequency;
        let (cq, sq) = fresnel(self.b1 / (2.0 * lambda * rho1).sqrt())?;
        let r = (lambda * rho2).sqrt();
        let (cu, su) = fresnel(std::f64::consts::FRAC_1_SQRT_2 * (r / self.a1 + self.a1 / r))?;
        let (cv, sv) = fresnel(std::f64::consts::FRAC_1_SQRT_2 * (r / self.a1 - self.a1 / r))?;
        let e = cq * cq + sq * sq;
        let h = (cu - cv).powi(2) + (su - sv).powi(2);
        Ok(8.0 * std::f64::consts::PI * rho1 * rho2 / (self.a1 * self.b1) * e * h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresnel_integrals_match_tables_and_limits() {
        // Abramowitz & Stegun table 7.7 (x = 0.5, 2.0) and the limits 1/2.
        let (c, s) = fresnel(0.5).unwrap();
        assert!((c - 0.492_344_225_9).abs() < 1e-9 && (s - 0.064_732_432_9).abs() < 1e-9);
        let (c, s) = fresnel(2.0).unwrap();
        assert!((c - 0.488_253_406_1).abs() < 1e-9 && (s - 0.343_415_678_4).abs() < 1e-9);
        let (c, s) = fresnel(200.0).unwrap();
        assert!((c - 0.5).abs() < 2e-3 && (s - 0.5).abs() < 2e-3);
        let (c, s) = fresnel(-1.0).unwrap();
        assert!(c < 0.0 && s < 0.0, "odd functions");
        assert!(fresnel(f64::NAN).is_err() && fresnel(1e9).is_err());
    }

    #[test]
    fn a_long_horn_tends_to_the_uniform_phase_aperture() {
        // With the apexes far behind, the phase error vanishes: the TE10 aperture's directivity
        // (4π/λ²) a₁ b₁ (8/π²).
        let horn = PyramidalHorn { a: 22.86e-3, b: 10.16e-3, a1: 60e-3, b1: 45e-3, length: 100.0 };
        let lambda = C0 / 10e9;
        let uniform = 4.0 * std::f64::consts::PI / (lambda * lambda) * horn.a1 * horn.b1 * 8.0 / std::f64::consts::PI.powi(2);
        let d = horn.directivity(10e9).unwrap();
        assert!((d / uniform - 1.0).abs() < 1e-3, "{d} {uniform}");
    }

    #[test]
    fn balanis_example_13_4() {
        // Balanis, Example 13.4 (3rd ed.): a1 = 5.5λ, b1 = 2.75λ, ρ1 = ρ2 = 6λ (a = 0.5λ,
        // b = 0.25λ): D_p ≈ 75 (18.76 dB, from tabulated Fresnel integrals). Built from
        // ρ1 = ρ2 = 6λ directly.
        let lambda: f64 = 1.0;
        let (a1, b1, rho) = (5.5 * lambda, 2.75 * lambda, 6.0 * lambda);
        let (cq, sq) = fresnel(b1 / (2.0 * lambda * rho).sqrt()).unwrap();
        let r = (lambda * rho).sqrt();
        let (cu, su) = fresnel(std::f64::consts::FRAC_1_SQRT_2 * (r / a1 + a1 / r)).unwrap();
        let (cv, sv) = fresnel(std::f64::consts::FRAC_1_SQRT_2 * (r / a1 - a1 / r)).unwrap();
        let d = 8.0 * std::f64::consts::PI * rho * rho / (a1 * b1) * (cq * cq + sq * sq) * ((cu - cv).powi(2) + (su - sv).powi(2));
        assert!((10.0 * d.log10() - 18.76).abs() < 0.1, "{} dB", 10.0 * d.log10());
    }
}

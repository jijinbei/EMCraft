//! The reflection of a linear taper between two rectangular waveguides by the theory of small
//! reflections.
//!
//! A gradual taper reflects at each step dz a little, dΓ = ½ d(ln Z), the reflections adding
//! with the phase of the path there and back (D. M. Pozar, *Microwave Engineering*, 4th ed.,
//! Wiley 2012, §5.8–5.9; R. E. Collin, *Foundations for Microwave Engineering*, 2nd
//! ed., McGraw-Hill 1992, §5.10):
//!
//! Γ = ½ ∫₀ˡ (d ln Z / dz) e^{−2jθ(z)} dz, θ(z) = ∫₀ᶻ β(z') dz',
//!
//! referred to the taper's start. For the TE₁₀ mode of a guide a(z) × b(z) the local
//! impedance is taken as Z ∝ b/β, β = √(k₀² − (π/a)²): the wave impedance ωμ/β (what the
//! H-plane width changes, through β) times the height b (the E-plane, as for a parallel-plate
//! line). It is first order in the reflection and neglects the higher modes the walls'
//! slope couples to: an approximation, good when the taper is long against the guide
//! wavelength and the reflection small.
//!
//! With a and b linear in z, θ is in closed form: ∫ √(k² − π²/x²) dx =
//! √(k²x² − π²) − π arccos(π/(kx)).
//!
//! ```
//! use emcraft_theory::taper::LinearTaper;
//!
//! // WR-90 to WR-75 over 40 mm at 10 GHz (metres, hertz): a small reflection.
//! let t = LinearTaper { a1: 22.86e-3, b1: 10.16e-3, a2: 19.05e-3, b2: 9.525e-3, length: 40e-3 };
//! let g = t.reflection(10e9)?;
//! assert!(g.norm() > 1e-3 && g.norm() < 0.05);
//! // A uniform guide does not reflect; below cut-off at either end the theory does not apply.
//! assert!(LinearTaper { a2: 22.86e-3, b2: 10.16e-3, ..t }.reflection(10e9)?.norm() < 1e-15);
//! assert!(t.reflection(7e9).is_err());
//! # Ok::<(), emcraft_theory::TheoryError>(())
//! ```

use std::f64::consts::PI;

use emcraft_model::units::C0;
use num_complex::Complex64 as C;

use crate::TheoryError;

/// A guide whose sides go linearly from a1 × b1 at z = 0 to a2 × b2 at z = length (metres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearTaper {
    pub a1: f64,
    pub b1: f64,
    pub a2: f64,
    pub b2: f64,
    pub length: f64,
}

/// Most panels the quadrature takes.
const MAX_PANELS: usize = 20_000;

impl LinearTaper {
    /// Γ at the taper's start (z = 0) at `frequency` (Hz), by the theory of small reflections.
    pub fn reflection(&self, frequency: f64) -> Result<C, TheoryError> {
        let all = [self.a1, self.b1, self.a2, self.b2, self.length];
        if all.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err(TheoryError::Invalid(format!("taper {self:?}: lengths must be finite and positive")));
        }
        if !frequency.is_finite() || frequency <= 0.0 {
            return Err(TheoryError::Invalid(format!("frequency {frequency}")));
        }
        let k = 2.0 * PI * frequency / C0;
        if k * self.a1.min(self.a2) <= PI {
            return Err(TheoryError::Invalid(format!("taper {self:?}: TE10 is cut off at {frequency} Hz")));
        }
        let l = self.length;
        let (da, db) = ((self.a2 - self.a1) / l, (self.b2 - self.b1) / l);
        let width = |z: f64| self.a1 + da * z;
        let beta = |a: f64| (k * k - (PI / a).powi(2)).sqrt();
        // θ(z) in closed form (β z when the width does not change).
        let antiderivative = |x: f64| (k * k * x * x - PI * PI).sqrt() - PI * (PI / (k * x)).acos();
        let theta = |z: f64| if da.abs() < 1e-15 { beta(self.a1) * z } else { (antiderivative(width(z)) - antiderivative(self.a1)) / da };
        // d ln Z / dz = b'/b − β'/β, β' = π² a' / (β a³).
        let slope = |z: f64| {
            let (a, b) = (width(z), self.b1 + db * z);
            let bz = beta(a);
            db / b - PI * PI * da / (bz * bz * a * a * a)
        };
        let (nodes, weights) = emcraft_fem::quadrature::gauss_legendre(16);
        // Several panels per guide wavelength.
        let panels = ((theta(l) / PI).ceil() as usize * 4 + 8).min(MAX_PANELS);
        let h = l / panels as f64;
        let mut sum = C::new(0.0, 0.0);
        // The rule is on [0, 1]: each panel [ph, (p + 1)h].
        for p in 0..panels {
            for (t, w) in nodes.iter().zip(&weights) {
                let z = (p as f64 + t) * h;
                sum += w * slope(z) * C::from_polar(1.0, -2.0 * theta(z));
            }
        }
        Ok(0.5 * h * sum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pure_e_plane_taper_reflects_half_the_log_of_its_heights_at_low_phase() {
        // A short E-plane taper (a fixed): Γ → ½ ln(b2/b1) as the length goes to zero.
        let t = LinearTaper { a1: 22.86e-3, b1: 10.16e-3, a2: 22.86e-3, b2: 8e-3, length: 1e-6 };
        let g = t.reflection(10e9).unwrap();
        assert!((g.re - 0.5 * (8.0f64 / 10.16).ln()).abs() < 1e-6 && g.im.abs() < 1e-4, "{g}");
    }

    #[test]
    fn the_closed_form_phase_matches_quadrature() {
        // θ(l) against a direct sum of β along the taper.
        let t = LinearTaper { a1: 22.86e-3, b1: 10.16e-3, a2: 15.8e-3, b2: 7.9e-3, length: 50e-3 };
        let k = 2.0 * PI * 12e9 / C0;
        let n = 200_000;
        let h = t.length / n as f64;
        let direct: f64 = (0..n)
            .map(|i| {
                let a = t.a1 + (t.a2 - t.a1) * (i as f64 + 0.5) / n as f64;
                (k * k - (PI / a).powi(2)).sqrt() * h
            })
            .sum();
        let anti = |x: f64| (k * k * x * x - PI * PI).sqrt() - PI * (PI / (k * x)).acos();
        let closed = (anti(t.a2) - anti(t.a1)) / ((t.a2 - t.a1) / t.length);
        assert!((direct - closed).abs() < 1e-8, "{direct} {closed}");
        // Longer tapers reflect less, on the whole.
        assert!(LinearTaper { length: 200e-3, ..t }.reflection(12e9).unwrap().norm() < t.reflection(12e9).unwrap().norm());
    }
}

//! A magnetized ferrite: its permeability the Polder tensor (D. Polder, "On the theory of
//! ferromagnetic resonance", Phil. Mag. 40(300):99–115, 1949), for a material saturated along
//! `axis` by an internal field H₀ (D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012,
//! §9.1, eqs. 9.25–9.28; time dependence e^{jωt}). With the bias along ẑ,
//!
//! [μ] = μ₀ [[μ, jκ, 0], [−jκ, μ, 0], [0, 0, 1]], μ = 1 + ω₀ω_m/(ω₀² − ω²),
//! κ = ω ω_m/(ω₀² − ω²),
//!
//! ω₀ = μ₀γH₀ the Larmor frequency and ω_m = μ₀γM_s, γ/2π = 2.8 MHz/Oe (so f₀ = 2.8 MHz × H₀ in
//! oersted, f_m = 2.8 MHz × 4πM_s in gauss). Losses (the resonance's linewidth ΔH) make the
//! Larmor frequency complex, ω₀ → ω₀ + jω_Δ/2 with ω_Δ = μ₀γΔH. Along x̂ or ŷ the tensor's axes
//! turn with it: the bias axis a keeps 1, the two after it (b, c in right-handed order) take
//! μ and ±jκ.
//!
//! The curl–curl term wants [μ]⁻¹ = [[ν_d, ν_o, 0], [−ν_o, ν_d, 0], [0, 0, 1]] (in units of
//! 1/μ₀), ν_d = μ/(μ² − κ²) and ν_o = −jκ/(μ² − κ²): not symmetric, so the system it makes is
//! not either, and gyrotropy makes the structure non-reciprocal.

use num_complex::Complex64 as C;
use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid};
use crate::shape::Axis;
use crate::units::angular;

/// γ/2π for an electron spin, in hertz per oersted (2.8 MHz/Oe; per gauss for 4πM_s alike).
pub const GYROMAGNETIC_HZ_PER_OE: f64 = 2.8e6;
/// The largest saturation magnetization 4πM_s (gauss) a ferrite takes: above any (iron's 21 kG).
pub const MAX_MAGNETIZATION_GAUSS: f64 = 3e4;
/// The largest internal bias field (oersted) or linewidth a ferrite takes.
pub const MAX_FIELD_OE: f64 = 1e6;

/// A saturated ferrite's magnetization and bias.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ferrite {
    /// 4πM_s, gauss (YIG 1780, nickel ferrite 5000).
    pub magnetization: f64,
    /// The internal bias field H₀, oersted (> 0: saturated).
    pub bias: f64,
    /// The resonance linewidth ΔH, oersted (0: lossless).
    #[serde(default)]
    pub linewidth: f64,
    /// The bias' direction.
    pub axis: Axis,
}

/// The inverse Polder tensor's distinct entries ([`Ferrite::nu`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nu {
    /// ν_d: the two transverse diagonal entries.
    pub diagonal: C,
    /// ν_o: the entry (b, c); (c, b) is −ν_o.
    pub off: C,
}

impl Ferrite {
    /// Refuses a magnetization or bias not above 0, a linewidth below 0, or any of them past
    /// its cap or not finite.
    pub fn checked(self) -> Result<Ferrite> {
        let ok = |x: f64, cap: f64, zero: bool| x.is_finite() && x <= cap && if zero { x >= 0.0 } else { x > 0.0 };
        if !ok(self.magnetization, MAX_MAGNETIZATION_GAUSS, false) {
            return Err(invalid(
                "ferrite magnetization",
                format!("4πMs must be above 0 and at most {MAX_MAGNETIZATION_GAUSS} G, got {}", self.magnetization),
            ));
        }
        if !ok(self.bias, MAX_FIELD_OE, false) {
            return Err(invalid("ferrite bias", format!("the internal field must be above 0 and at most {MAX_FIELD_OE} Oe, got {}", self.bias)));
        }
        if !ok(self.linewidth, MAX_FIELD_OE, true) {
            return Err(invalid("ferrite linewidth", format!("ΔH must be 0 or more and at most {MAX_FIELD_OE} Oe, got {}", self.linewidth)));
        }
        Ok(self)
    }

    /// The Polder tensor's μ and κ at `frequency` (Hz), relative to μ₀.
    ///
    /// ```
    /// use emcraft_model::ferrite::Ferrite;
    /// use emcraft_model::Axis;
    /// // YIG (4πMs = 1780 G) biased at 1000 Oe, at 10 GHz, lossless: f₀ = 2.8 GHz, f_m = 4.984 GHz.
    /// let y = Ferrite { magnetization: 1780.0, bias: 1000.0, linewidth: 0.0, axis: Axis::Z };
    /// let (mu, kappa) = y.polder(10e9);
    /// let (f0, fm, f) = (2.8e9, 4.984e9, 10e9);
    /// assert!((mu.re - (1.0 + f0 * fm / (f0 * f0 - f * f))).abs() < 1e-12 && mu.im == 0.0);
    /// assert!((kappa.re - f * fm / (f0 * f0 - f * f)).abs() < 1e-12);
    /// ```
    pub fn polder(&self, frequency: f64) -> (C, C) {
        let omega = angular(frequency);
        let to = |oe: f64| angular(GYROMAGNETIC_HZ_PER_OE * oe);
        let (w0, wm) = (C::new(to(self.bias), 0.5 * to(self.linewidth)), to(self.magnetization));
        let d = w0 * w0 - omega * omega;
        (C::new(1.0, 0.0) + w0 * wm / d, C::new(omega * wm, 0.0) / d)
    }

    /// The inverse tensor's entries at `frequency` (Hz), relative to 1/μ₀: ν_d = μ/(μ² − κ²),
    /// ν_o = −jκ/(μ² − κ²).
    pub fn nu(&self, frequency: f64) -> Nu {
        let (mu, kappa) = self.polder(frequency);
        let det = mu * mu - kappa * kappa;
        Nu { diagonal: mu / det, off: C::new(0.0, -1.0) * kappa / det }
    }

    /// The bias axis and the two transverse axes after it, right-handed: (a, b, c).
    pub fn axes(&self) -> (usize, usize, usize) {
        let a = self.axis.index();
        (a, (a + 1) % 3, (a + 2) % 3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inverse_tensor_undoes_the_tensor() {
        let f = Ferrite { magnetization: 1780.0, bias: 1500.0, linewidth: 40.0, axis: Axis::Y }.checked().unwrap();
        for freq in [3e9, 9e9, 20e9] {
            let (mu, kappa) = f.polder(freq);
            let Nu { diagonal: d, off: o } = f.nu(freq);
            let j = C::new(0.0, 1.0);
            // [[μ, jκ], [−jκ, μ]] · [[ν_d, ν_o], [−ν_o, ν_d]] = I.
            let (a11, a12) = (mu * d - j * kappa * o, mu * o + j * kappa * d);
            let (a21, a22) = (-j * kappa * d - mu * o, -j * kappa * o + mu * d);
            for (got, want) in [(a11, 1.0), (a12, 0.0), (a21, 0.0), (a22, 1.0)] {
                assert!((got - C::new(want, 0.0)).norm() < 1e-12, "{freq}: {got}");
            }
        }
        assert_eq!(f.axes(), (1, 2, 0));
    }

    #[test]
    fn a_linewidth_makes_it_lossy_and_bad_numbers_are_refused() {
        let lossless = Ferrite { magnetization: 1780.0, bias: 1000.0, linewidth: 0.0, axis: Axis::Z };
        let lossy = Ferrite { linewidth: 50.0, ..lossless };
        assert_eq!(lossless.polder(9e9).0.im, 0.0);
        assert!(lossy.polder(9e9).0.im < 0.0, "μ″ > 0 with e^{{jωt}}");
        for bad in [Ferrite { magnetization: 0.0, ..lossless }, Ferrite { bias: -1.0, ..lossless }, Ferrite { linewidth: f64::NAN, ..lossless }] {
            assert!(bad.checked().is_err());
        }
    }
}

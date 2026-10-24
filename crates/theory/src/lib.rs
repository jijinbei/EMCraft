//! Closed-form references for EMCraft's verification (docs/verification.md).
//!
//! [`Te10Line`]: a rectangular waveguide of width a filled along z in sections of different
//! materials, fed by the TE₁₀ mode. Every section has the same cross-section, so the TE₁₀
//! field sin(πx/a) ŷ matches across each interface by itself: no other mode is excited, and the
//! problem is exactly a cascade of transmission lines with propagation constants
//! β_i = √(k₀² εr_i μr_i − (π/a)²) (−jα_i below cut-off) and wave impedances Z_i = ωμ_i/β_i
//! (D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012, §3.3 and §4.4; R. E. Collin,
//! *Foundations for Microwave Engineering*, 2nd ed., McGraw-Hill 1992, §3.12 and §4.2).
//!
//! In section i (starting at z_i), with time dependence e^{jωt},
//!
//! E_y = sin(πx/a) [A_i e^{−jβ_i(z − z_i)} + B_i e^{+jβ_i(z − z_i)}],
//!
//! and E_y and H_x = (1/jωμ_i) ∂E_y/∂z are continuous at each interface. The incident amplitude
//! at z = 0 is A₀ = 1 and nothing enters at the far end (B_last = 0). The S-parameters are
//! referred to the end faces: S₁₁ = B₀, S₂₁ = A_last e^{−jβ_last l_last}.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod bessel;
pub mod modematch;
pub mod radiation;

pub use modematch::{EPlaneLine, ESection, HPlaneLine, HSection};

use std::f64::consts::PI;

use emcraft_model::units::{C0, MU0};
use num_complex::Complex64 as C;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum TheoryError {
    #[error("invalid {0}")]
    Invalid(String),
    #[error("the TE10 mode is at cut-off in section {0}: the solution is singular there")]
    Cutoff(usize),
}

/// A section of the line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub length: f64,
    pub eps_r: f64,
    pub mu_r: f64,
    /// Dielectric loss tangent: ε = εr(1 − j tan δ).
    pub loss_tangent: f64,
}

impl Layer {
    pub fn lossless(length: f64, eps_r: f64, mu_r: f64) -> Layer {
        Layer { length, eps_r, mu_r, loss_tangent: 0.0 }
    }
}

/// √z on the branch with Im ≤ 0 (a wave e^{−jβz} that decays, or does not grow, along +z).
pub fn decaying_sqrt(z: C) -> C {
    let r = z.sqrt();
    if r.im > 0.0 { -r } else { r }
}

/// The TE₁₀ wave through a waveguide of width `a` (m) filled in sections.
#[derive(Clone, Debug, PartialEq)]
pub struct Te10Line {
    pub a: f64,
    pub layers: Vec<Layer>,
}

/// The solution at one frequency.
#[derive(Clone, Debug, PartialEq)]
pub struct Te10Solution {
    pub frequency: f64,
    pub a: f64,
    pub layers: Vec<Layer>,
    /// Section start coordinates z_i.
    pub starts: Vec<f64>,
    pub beta: Vec<C>,
    /// Forward and backward amplitudes (A_i, B_i).
    pub amplitudes: Vec<(C, C)>,
    pub s11: C,
    pub s21: C,
}

/// TE₁₀ cut-off frequency c/(2a√(εr μr)) (Hz).
pub fn te10_cutoff(a: f64, eps_r: f64, mu_r: f64) -> f64 {
    C0 / (2.0 * a * (eps_r * mu_r).sqrt())
}

/// The TE₁₀ attenuation (Np/m) of an a × b guide filled with εr, from walls of conductivity σ
/// (S/m), to first order in the surface resistance R_s = √(ωμ₀/2σ):
/// α_c = R_s (2bπ² + a³k²) / (a³ b β k η), η = η₀/√εr (D. M. Pozar, *Microwave Engineering*,
/// 4th ed., Wiley 2012, eq. 3.96).
pub fn te10_wall_attenuation(a: f64, b: f64, frequency: f64, sigma: f64, eps_r: f64) -> f64 {
    let omega = 2.0 * PI * frequency;
    let rs = (omega * MU0 / (2.0 * sigma)).sqrt();
    let k = omega / C0 * eps_r.sqrt();
    let beta = (k * k - (PI / a).powi(2)).sqrt();
    let eta = MU0 * C0 / eps_r.sqrt();
    rs * (2.0 * b * PI * PI + a.powi(3) * k * k) / (a.powi(3) * b * beta * k * eta)
}

/// The resonances of an a × b × d cavity filled with εr, perfectly conducting walls, lowest
/// first: f = c/(2√εr) √((m/a)² + (n/b)² + (p/d)²) for TE_mnp (m, n not both 0, p ≥ 1) and
/// TM_mnp (m, n ≥ 1, p ≥ 0), with their names (Pozar §6.3).
pub fn cavity_resonances(a: f64, b: f64, d: f64, eps_r: f64, count: usize) -> Vec<(f64, String)> {
    let count = count.min(1000);
    let span = 12u32;
    let mut out = Vec::new();
    for m in 0..=span {
        for n in 0..=span {
            for p in 0..=span {
                let f = C0 / (2.0 * eps_r.sqrt()) * ((m as f64 / a).powi(2) + (n as f64 / b).powi(2) + (p as f64 / d).powi(2)).sqrt();
                if (m > 0 || n > 0) && p >= 1 {
                    out.push((f, format!("TE{m}{n}{p}")));
                }
                if m >= 1 && n >= 1 {
                    out.push((f, format!("TM{m}{n}{p}")));
                }
            }
        }
    }
    out.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
    out.truncate(count);
    out
}

/// Q of the TE101 mode of an a × b × d air-filled cavity from walls of conductivity σ:
/// Q_c = (k a d)³ b η / (2π² R_s (2a³b + 2bd³ + a³d + ad³)) (D. M. Pozar, *Microwave
/// Engineering*, 4th ed., Wiley 2012, eq. 6.46).
pub fn te101_q_walls(a: f64, b: f64, d: f64, sigma: f64) -> f64 {
    let k = PI * ((1.0 / a).powi(2) + (1.0 / d).powi(2)).sqrt();
    let omega = k * C0;
    let rs = (omega * MU0 / (2.0 * sigma)).sqrt();
    let eta = MU0 * C0;
    (k * a * d).powi(3) * b * eta / (2.0 * PI * PI * rs * (2.0 * a.powi(3) * b + 2.0 * b * d.powi(3) + a.powi(3) * d + a * d.powi(3)))
}

/// β = √(k² − (π/a)²) on the branch with Im β ≤ 0 (decaying along +z below cut-off or with
/// losses), k² = k0² εr(1 − j tan δ) μr.
fn beta(k0: f64, a: f64, l: &Layer) -> C {
    let k2 = C::new(1.0, -l.loss_tangent) * (k0 * k0 * l.eps_r * l.mu_r);
    decaying_sqrt(k2 - (PI / a).powi(2))
}

const J: C = C::new(0.0, 1.0);

impl Te10Line {
    pub fn solve(&self, frequency: f64) -> Result<Te10Solution, TheoryError> {
        if !(frequency.is_finite() && frequency > 0.0) {
            return Err(TheoryError::Invalid(format!("frequency {frequency}")));
        }
        if !(self.a.is_finite() && self.a > 0.0) || self.layers.is_empty() {
            return Err(TheoryError::Invalid("line".into()));
        }
        for l in &self.layers {
            if ![l.length, l.eps_r, l.mu_r].iter().all(|v| v.is_finite() && *v > 0.0) || !(l.loss_tangent.is_finite() && l.loss_tangent >= 0.0) {
                return Err(TheoryError::Invalid(format!("layer {l:?}")));
            }
        }
        let k0 = 2.0 * PI * frequency / C0;
        let beta: Vec<C> = self.layers.iter().map(|l| beta(k0, self.a, l)).collect();
        let kc = PI / self.a;
        for (i, b) in beta.iter().enumerate() {
            if b.norm() < 1e-9 * kc {
                return Err(TheoryError::Cutoff(i));
            }
        }
        let n = self.layers.len();
        let mut amp = vec![(C::new(0.0, 0.0), C::new(0.0, 0.0)); n];
        amp[n - 1] = (C::new(1.0, 0.0), C::new(0.0, 0.0));
        // Backwards through the interfaces: from (A', B') at the start of section i + 1 to the
        // amplitudes of section i.
        for i in (0..n - 1).rev() {
            let (a1, b1) = amp[i + 1];
            let e = a1 + b1;
            let d = beta[i + 1] / self.layers[i + 1].mu_r * (a1 - b1);
            let r = d * self.layers[i].mu_r / beta[i];
            let p = (-J * beta[i] * self.layers[i].length).exp();
            amp[i] = ((e + r) / (2.0 * p), (e - r) * p / 2.0);
        }
        let a0 = amp[0].0;
        for x in &mut amp {
            *x = (x.0 / a0, x.1 / a0);
        }
        let mut starts = Vec::with_capacity(n);
        let mut z = 0.0;
        for l in &self.layers {
            starts.push(z);
            z += l.length;
        }
        let s11 = amp[0].1;
        let last = n - 1;
        let s21 = amp[last].0 * (-J * beta[last] * self.layers[last].length).exp();
        Ok(Te10Solution { frequency, a: self.a, layers: self.layers.clone(), starts, beta, amplitudes: amp, s11, s21 })
    }
}

impl Te10Solution {
    /// (S₁₁, S₂₁) normalised to equal power at both ports: the field ratio times
    /// √((β_last/μ_last)/(β₀/μ₀)), since a TE₁₀ wave of amplitude A carries power ∝ |A|² β/μ
    /// (the end sections lossless).
    pub fn s_power(&self) -> (C, C) {
        let n = self.layers.len() - 1;
        let y = |i: usize| self.beta[i] / self.layers[i].mu_r;
        (self.s11, self.s21 * (y(n) / y(0)).sqrt())
    }

    /// The section holding z (clamped to the line).
    fn section(&self, z: f64) -> usize {
        self.starts.partition_point(|s| *s <= z).saturating_sub(1).min(self.layers.len().saturating_sub(1))
    }

    /// F(z) and F'(z), with E_y = sin(πx/a) F(z).
    fn profile(&self, z: f64) -> (C, C) {
        let i = self.section(z);
        let (a, b) = self.amplitudes[i];
        let zeta = z - self.starts[i];
        let f = a * (-J * self.beta[i] * zeta).exp();
        let g = b * (J * self.beta[i] * zeta).exp();
        (f + g, -J * self.beta[i] * (f - g))
    }

    /// The electric field (V/m) at (x, y, z) for a unit incident TE₁₀ amplitude.
    pub fn e(&self, x: f64, _y: f64, z: f64) -> [C; 3] {
        let (f, _) = self.profile(z);
        let zero = C::new(0.0, 0.0);
        [zero, f * (PI * x / self.a).sin(), zero]
    }

    /// The magnetic field (A/m): H = −(1/jωμ) ∇ × E.
    pub fn h(&self, x: f64, _y: f64, z: f64) -> [C; 3] {
        let i = self.section(z);
        let (f, df) = self.profile(z);
        let jwmu = J * 2.0 * PI * self.frequency * MU0 * self.layers[i].mu_r;
        let (s, c) = (PI * x / self.a).sin_cos();
        // ∇ × (ŷ E_y) = (−∂E_y/∂z, 0, ∂E_y/∂x).
        [df * s / jwmu, C::new(0.0, 0.0), -(f * c * PI / self.a) / jwmu]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: f64 = 0.02286;

    fn filter() -> Te10Line {
        let l = |length: f64, eps_r: f64| Layer::lossless(length, eps_r, 1.0);
        Te10Line { a: A, layers: vec![l(0.015, 2.0), l(0.02, 1.0), l(0.015, 2.0)] }
    }

    #[test]
    fn lossless_lines_conserve_power_and_are_reciprocal() {
        let line = filter();
        for f in [5.0e9, 6.0e9, 6.5e9, 7.0e9, 8.0e9, 9.0e9] {
            let s = line.solve(f).unwrap();
            assert!((s.s11.norm_sqr() + s.s21.norm_sqr() - 1.0).abs() < 1e-12, "{f}");
        }
        // An asymmetric line: S21 is the same both ways (equal port media).
        let l = |length: f64, eps_r: f64| Layer::lossless(length, eps_r, 1.0);
        let fwd = Te10Line { a: A, layers: vec![l(0.01, 2.0), l(0.013, 1.0), l(0.004, 3.0), l(0.02, 2.0)] };
        let mut rev = fwd.clone();
        rev.layers.reverse();
        let (p, q) = (fwd.solve(7.3e9).unwrap(), rev.solve(7.3e9).unwrap());
        assert!((p.s21 - q.s21).norm() < 1e-12);
    }

    #[test]
    fn a_single_step_reflects_by_the_impedance_ratio() {
        let l = |length: f64, eps_r: f64| Layer::lossless(length, eps_r, 1.0);
        let line = Te10Line { a: A, layers: vec![l(0.01, 2.0), l(0.03, 1.0)] };
        let f = 8.0e9;
        let s = line.solve(f).unwrap();
        let k0 = 2.0 * PI * f / C0;
        let b1 = (k0 * k0 * 2.0 - (PI / A).powi(2)).sqrt();
        let b2 = (k0 * k0 - (PI / A).powi(2)).sqrt();
        // Referred to the interface at z = 0.01: S11 e^{2jβ₁l₁} = (β₁ − β₂)/(β₁ + β₂).
        let at_interface = s.s11 * (2.0 * J * b1 * 0.01).exp();
        assert!((at_interface - C::new((b1 - b2) / (b1 + b2), 0.0)).norm() < 1e-12);
        // Power: |S11|² + |S21|²(power) = 1 with different port media.
        let (r, t) = s.s_power();
        assert!((r.norm_sqr() + t.norm_sqr() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn below_cutoff_the_middle_section_attenuates_exponentially() {
        let f = 5.5e9;
        let k0 = 2.0 * PI * f / C0;
        let alpha = ((PI / A).powi(2) - k0 * k0).sqrt();
        let at = |l: f64| {
            let mut line = filter();
            line.layers[1].length = l;
            line.solve(f).unwrap().s21.norm()
        };
        // Long sections: |S21| ∝ e^{−αL}.
        let slope = (at(0.25).ln() - at(0.2).ln()) / 0.05;
        assert!((slope + alpha).abs() < 1e-6 * alpha, "{slope} {alpha}");
        assert!(te10_cutoff(A, 1.0, 1.0) > f && te10_cutoff(A, 2.0, 1.0) < f);
    }

    #[test]
    fn fields_are_continuous_and_satisfy_the_wave_equation() {
        let s = filter().solve(7.0e9).unwrap();
        let x = 0.3 * A;
        for z in [0.015, 0.035] {
            let (e0, e1) = (s.e(x, 0.0, z - 1e-12), s.e(x, 0.0, z + 1e-12));
            let (h0, h1) = (s.h(x, 0.0, z - 1e-12), s.h(x, 0.0, z + 1e-12));
            assert!((e0[1] - e1[1]).norm() < 1e-8 && (h0[0] - h1[0]).norm() < 1e-8 * h0[0].norm().max(1e-3));
        }
        // The incident amplitude at z = 0 is 1: E_y(a/2, 0) = 1 + S11.
        let e = s.e(A / 2.0, 0.0, 0.0)[1];
        assert!((e - (1.0 + s.s11)).norm() < 1e-14);
        // ∂²F/∂z² = −β²F inside a section.
        let h = 1e-6;
        let z = 0.025;
        let f = |z: f64| s.e(A / 2.0, 0.0, z)[1];
        let d2 = (f(z + h) - 2.0 * f(z) + f(z - h)) / (h * h);
        let b = s.beta[1];
        assert!((d2 + b * b * f(z)).norm() < 1e-4 * (b * b * f(z)).norm());
    }

    #[test]
    fn a_lossy_slab_absorbs_and_matches_the_complex_propagation_constant() {
        // A long matched-ish lossy line: |S21| ≈ e^{−Im(−β) L}.
        let f = 10.0e9;
        let lossy = Layer { length: 0.1, eps_r: 1.0, mu_r: 1.0, loss_tangent: 0.01 };
        let line = Te10Line { a: A, layers: vec![Layer::lossless(0.01, 1.0, 1.0), lossy, Layer::lossless(0.01, 1.0, 1.0)] };
        let s = line.solve(f).unwrap();
        let (r, t) = s.s_power();
        assert!(r.norm_sqr() + t.norm_sqr() < 1.0);
        let b = s.beta[1];
        assert!(b.im < 0.0);
        // The reflection of a slightly lossy section is tiny; transmission is its decay.
        assert!((t.norm().ln() - b.im * 0.1).abs() < 1e-3 * (b.im * 0.1).abs(), "{} {}", t.norm().ln(), b.im * 0.1);
        // α = k² tan δ / (2β) to first order in tan δ.
        let k0 = 2.0 * PI * f / C0;
        let beta0 = (k0 * k0 - (PI / A).powi(2)).sqrt();
        assert!((-b.im - k0 * k0 * 0.01 / (2.0 * beta0)).abs() < 1e-3 * b.im.abs());
    }

    #[test]
    fn copper_wr90_attenuates_as_tabulated() {
        // WR-90 in copper (σ = 5.8e7 S/m) at 10 GHz, by hand: R_s = 0.02609 Ω, k = 209.58,
        // β = 158.24 rad/m: α = 0.02609 · 0.7253 / 1.5164 = 0.01248 Np/m (0.108 dB/m, the
        // figure usually quoted for WR-90).
        let alpha = te10_wall_attenuation(0.02286, 0.01016, 10.0e9, 5.8e7, 1.0);
        assert!((alpha - 0.01248).abs() < 1e-5, "{alpha}");
    }

    #[test]
    fn cavity_modes_in_order() {
        // 22.86 × 10.16 × 30 mm: TE101 at c/2·√(1/a² + 1/d²) = 1.49896e8 · 54.997 = 8.2439 GHz,
        // then TE102.
        let r = cavity_resonances(0.02286, 0.01016, 0.03, 1.0, 3);
        assert_eq!(r[0].1, "TE101");
        assert!((r[0].0 - 8.2439e9).abs() < 1e6, "{}", r[0].0);
        assert_eq!(r[1].1, "TE102");
        // The cavity's Q from copper walls: several thousand.
        let q = te101_q_walls(0.02286, 0.01016, 0.03, 5.8e7);
        assert!(q > 4000.0 && q < 12000.0, "{q}");
    }

    #[test]
    fn hostile_inputs_are_refused() {
        let line = filter();
        assert!(line.solve(f64::NAN).is_err());
        assert!(line.solve(-1.0).is_err());
        assert!(matches!(line.solve(te10_cutoff(A, 1.0, 1.0)), Err(TheoryError::Cutoff(1))));
        assert!(Te10Line { a: A, layers: vec![] }.solve(1e10).is_err());
        assert!(Te10Line { a: A, layers: vec![Layer { loss_tangent: -1.0, ..Layer::lossless(0.01, 1.0, 1.0) }] }.solve(1e10).is_err());
    }
}

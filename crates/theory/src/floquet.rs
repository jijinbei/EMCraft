//! References for periodic structures.
//!
//! **A plane wave through a stack of layers** ([`Stack`]): the (0, 0) Floquet mode of a unit cell
//! whose layers fill its whole cross-section is the plane wave itself, and the layers are a
//! cascade of transmission lines for its transverse fields (D. M. Pozar, *Microwave
//! Engineering*, 4th ed., Wiley 2012, §4.4 for the ABCD matrices; S. J. Orfanidis, *Electromagnetic
//! Waves and Antennas*, Rutgers 2016, ch. 7–8, for oblique incidence): with the transverse
//! wavenumber k_t = k₀ √(ε₁μ₁) sin θ conserved across the layers (Snell),
//! k_z,i = √(k₀² ε_i μ_i − k_t²) (Im ≤ 0) and the transverse wave impedances
//! Z_TE = ωμ₀μ_i/k_z,i, Z_TM = k_z,i/(ωε₀ε_i), each layer is
//! [[cos k_z d, j Z sin k_z d], [j sin k_z d / Z, cos k_z d]] for (E_t, H_t). Between the first
//! and the last layer's impedances Z₁ and Z₂, S₁₁ = (A Z₂ + B − C Z₁Z₂ − D Z₁)/Δ and
//! S₂₁ = 2√(Z₁Z₂)/Δ, Δ = A Z₂ + B + C Z₁Z₂ + D Z₁: power waves of the transverse fields, the
//! reference planes on the stack's outer faces.
//!
//! **A waveguide loaded periodically along its axis** ([`bloch_cos`], [`bloch_frequencies`]):
//! sections that fill the cross-section keep TE₁₀ by itself, a cascade of lines with
//! β_i = √(k₀² ε_i μ_i − (π/a)²) and Z_i = ωμ₀μ_i/β_i; a Bloch wave of phase ψ per period
//! satisfies cos ψ = (A + D)/2 of the period's ABCD matrix (R. E. Collin, *Foundations for
//! Microwave Engineering*, 2nd ed., McGraw-Hill 1992, the chapter on periodic structures, periodic structures). A guide of one
//! material has the closed form k² = (π/a)² + (ψ/L)² for the lowest band ([`empty_guide_bands`]
//! gives every TE_mn and TM_mn harmonic).

use std::f64::consts::PI;

use emcraft_model::units::{C0, EPS0, MU0};
use num_complex::Complex64 as C;

use crate::{TheoryError, decaying_sqrt};

const J: C = C::new(0.0, 1.0);

/// A polarization relative to the plane of incidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Polarization {
    /// E normal to the plane of incidence.
    Te,
    /// H normal to it.
    Tm,
}

/// A layer of the stack: its thickness (m) and material (ε may be complex, ε′ − jε″).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StackLayer {
    pub thickness: f64,
    pub eps_r: C,
    pub mu_r: f64,
}

/// Plane layers along z, a plane wave incident from the first at the angle θ.
#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    pub layers: Vec<StackLayer>,
    pub theta: f64,
    pub polarization: Polarization,
}

/// A 2 × 2 complex matrix [[a, b], [c, d]].
type Abcd = [C; 4];

fn mul(x: Abcd, y: Abcd) -> Abcd {
    [x[0] * y[0] + x[1] * y[2], x[0] * y[1] + x[1] * y[3], x[2] * y[0] + x[3] * y[2], x[2] * y[1] + x[3] * y[3]]
}

/// A line of propagation constant `kz` and impedance `z`, `d` long.
fn line(kz: C, z: C, d: f64) -> Abcd {
    let (c, s) = ((kz * d).cos(), (kz * d).sin());
    [c, J * z * s, J * s / z, c]
}

impl Stack {
    fn check(&self, frequency: f64) -> Result<(), TheoryError> {
        if !(frequency.is_finite() && frequency > 0.0) {
            return Err(TheoryError::Invalid(format!("frequency {frequency}")));
        }
        if self.layers.is_empty() || !(self.theta.is_finite() && (0.0..PI / 2.0).contains(&self.theta)) {
            return Err(TheoryError::Invalid("stack".into()));
        }
        for l in &self.layers {
            let ok = l.thickness.is_finite()
                && l.thickness >= 0.0
                && l.mu_r.is_finite()
                && l.mu_r > 0.0
                && l.eps_r.re.is_finite()
                && l.eps_r.im.is_finite();
            if !ok || l.eps_r.re <= 0.0 {
                return Err(TheoryError::Invalid(format!("layer {l:?}")));
            }
        }
        Ok(())
    }

    /// Each layer's k_z and transverse wave impedance at `frequency`.
    fn lines(&self, frequency: f64) -> Vec<(C, C)> {
        let omega = 2.0 * PI * frequency;
        let k0 = omega / C0;
        let first = self.layers[0];
        let kt = k0 * (first.eps_r.re * first.mu_r).sqrt() * self.theta.sin();
        self.layers
            .iter()
            .map(|l| {
                let kz = decaying_sqrt(l.eps_r * (k0 * k0 * l.mu_r) - kt * kt);
                let z = match self.polarization {
                    Polarization::Te => omega * MU0 * l.mu_r / kz,
                    Polarization::Tm => kz / (omega * EPS0 * l.eps_r),
                };
                (kz, z)
            })
            .collect()
    }

    /// (S₁₁, S₂₁) at `frequency` (Hz): power waves of the transverse fields, the reference planes
    /// on the stack's faces. The first and last layers must be lossless and propagating.
    pub fn s(&self, frequency: f64) -> Result<(C, C), TheoryError> {
        self.check(frequency)?;
        let lines = self.lines(frequency);
        let total = self
            .layers
            .iter()
            .zip(&lines)
            .fold([C::new(1.0, 0.0), C::new(0.0, 0.0), C::new(0.0, 0.0), C::new(1.0, 0.0)], |m, (l, (kz, z))| mul(m, line(*kz, *z, l.thickness)));
        let (z1, z2) = (lines[0].1, lines[lines.len() - 1].1);
        if !(z1.norm() > 0.0 && z2.norm() > 0.0 && z1.norm().is_finite() && z2.norm().is_finite()) {
            return Err(TheoryError::Invalid("an end layer at grazing incidence".into()));
        }
        let [a, b, c, d] = total;
        let delta = a * z2 + b + c * z1 * z2 + d * z1;
        Ok(((a * z2 + b - c * z1 * z2 - d * z1) / delta, 2.0 * (z1 * z2).sqrt() / delta))
    }
}

/// (A + D)/2 of one period of a rectangular guide of width `a` loaded in sections that fill its
/// cross-section, for TE₁₀ at `frequency`: the cosine of the Bloch phase per period in a pass
/// band (|·| ≤ 1), beyond ±1 in a stop band. `sections`: (length (m), εr, μr).
pub fn bloch_cos(a: f64, sections: &[(f64, f64, f64)], frequency: f64) -> f64 {
    let omega = 2.0 * PI * frequency;
    let k0 = omega / C0;
    let total = sections.iter().fold([C::new(1.0, 0.0), C::new(0.0, 0.0), C::new(0.0, 0.0), C::new(1.0, 0.0)], |m, &(len, eps, mu)| {
        let beta = decaying_sqrt(C::new(k0 * k0 * eps * mu - (PI / a).powi(2), 0.0));
        // At cut-off the line is a short of no impedance: its limit (cos → 1, Z sin → ωμ d).
        let step = if beta.norm() < 1e-12 * k0 {
            [C::new(1.0, 0.0), J * omega * MU0 * mu * len, C::new(0.0, 0.0), C::new(1.0, 0.0)]
        } else {
            line(beta, omega * MU0 * mu / beta, len)
        };
        mul(m, step)
    });
    0.5 * (total[0] + total[3]).re
}

/// The frequencies (Hz, ascending) up to `f_max` at which TE₁₀ Bloch waves of phase `psi`
/// (0 < ψ < π) per period exist in the periodically loaded guide of [`bloch_cos`]: the roots of
/// (A + D)/2 = cos ψ, bracketed on a grid of `f_max`/4000 and bisected to 1e-13 relative.
pub fn bloch_frequencies(a: f64, sections: &[(f64, f64, f64)], psi: f64, f_max: f64) -> Vec<f64> {
    if !(psi.is_finite() && psi > 0.0 && psi < PI && f_max.is_finite() && f_max > 0.0) {
        return Vec::new();
    }
    let target = psi.cos();
    let g = |f: f64| bloch_cos(a, sections, f) - target;
    let steps = 4000;
    let mut out = Vec::new();
    let mut prev = (f_max * 1e-6, g(f_max * 1e-6));
    for i in 1..=steps {
        let f = f_max * i as f64 / steps as f64;
        let v = g(f);
        if prev.1.signum() != v.signum() {
            let (mut lo, mut hi) = (prev.0, f);
            let s_lo = prev.1.signum();
            for _ in 0..200 {
                let mid = 0.5 * (lo + hi);
                if g(mid).signum() == s_lo {
                    lo = mid;
                } else {
                    hi = mid;
                }
                if hi - lo <= 1e-13 * hi {
                    break;
                }
            }
            out.push(0.5 * (lo + hi));
        }
        prev = (f, v);
    }
    out
}

/// An empty rectangular guide a × b (m) of `eps_r`, periodic along z with `period` (m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmptyGuide {
    pub a: f64,
    pub b: f64,
    pub period: f64,
    pub eps_r: f64,
}

/// The eigenfrequencies (Hz, ascending, each mode once, TE and TM counted apart) of an empty
/// rectangular guide a × b of `eps_r`, periodic along z with period `l` and phase `psi` per
/// period: k² = (mπ/a)² + (nπ/b)² + ((ψ + 2πq)/l)², TE_mn (m + n ≥ 1) and TM_mn (m, n ≥ 1);
/// the lowest `count`.
pub fn empty_guide_bands(guide: &EmptyGuide, psi: f64, count: usize) -> Vec<f64> {
    let EmptyGuide { a, b, period: l, eps_r } = *guide;
    let mut k2 = Vec::new();
    for m in 0..12u32 {
        for n in 0..12u32 {
            if m + n == 0 {
                continue;
            }
            let kc2 = (m as f64 * PI / a).powi(2) + (n as f64 * PI / b).powi(2);
            let pol = if m >= 1 && n >= 1 { 2 } else { 1 };
            for q in -6i32..=6 {
                let beta = (psi + 2.0 * PI * q as f64) / l;
                for _ in 0..pol {
                    k2.push(kc2 + beta * beta);
                }
            }
        }
    }
    k2.sort_by(f64::total_cmp);
    k2.into_iter().take(count).map(|k2| k2.sqrt() * C0 / (2.0 * PI * eps_r.sqrt())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn air(d: f64) -> StackLayer {
        StackLayer { thickness: d, eps_r: C::new(1.0, 0.0), mu_r: 1.0 }
    }

    #[test]
    fn a_slab_at_normal_incidence_is_the_fresnel_slab() {
        // A half-wave slab is transparent; a quarter-wave one reflects (n² − 1)/(n² + 1).
        let n: f64 = 2.0;
        let f = 10e9;
        let lam = C0 / f / n;
        for (d, r) in [(lam / 2.0, 0.0), (lam / 4.0, (n * n - 1.0) / (n * n + 1.0))] {
            let st = Stack {
                layers: vec![air(0.0), StackLayer { thickness: d, eps_r: C::new(n * n, 0.0), mu_r: 1.0 }, air(0.0)],
                theta: 0.0,
                polarization: Polarization::Te,
            };
            let (s11, s21) = st.s(f).unwrap();
            assert!((s11.norm() - r).abs() < 1e-12, "{s11}");
            assert!((s11.norm_sqr() + s21.norm_sqr() - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn a_single_interface_gives_fresnel_coefficients_and_brewster() {
        // Air to glass (n = 1.5), half-spaces of no thickness: S₁₁ is Fresnel's r (of E_t for TM).
        let n: f64 = 1.5;
        let glass = StackLayer { thickness: 0.0, eps_r: C::new(n * n, 0.0), mu_r: 1.0 };
        let theta: f64 = 0.6;
        let ct = theta.cos();
        let st = (1.0 - (theta.sin() / n).powi(2)).sqrt();
        let rs = (ct - n * st) / (ct + n * st);
        let te = Stack { layers: vec![air(0.0), glass], theta, polarization: Polarization::Te }.s(1e9).unwrap().0;
        assert!((te - rs).norm() < 1e-12, "{te} {rs}");
        // TM: Z = cos θ/(ωε) per layer → r = (Z₂ − Z₁)/(Z₂ + Z₁) = (cos θ_t/n − cos θ)/(cos θ_t/n + cos θ).
        let zr = (st / n - ct) / (st / n + ct);
        let tm = Stack { layers: vec![air(0.0), glass], theta, polarization: Polarization::Tm }.s(1e9).unwrap().0;
        assert!((tm - zr).norm() < 1e-12, "{tm} {zr}");
        // Brewster's angle: TM reflects nothing.
        let b = Stack { layers: vec![air(0.0), glass], theta: n.atan(), polarization: Polarization::Tm }.s(1e9).unwrap().0;
        assert!(b.norm() < 1e-12, "{b}");
    }

    #[test]
    fn an_empty_periodic_guide_is_te10s_dispersion() {
        let (a, l) = (0.02286, 0.01);
        for psi in [0.3, 1.2, 2.5] {
            let f = bloch_frequencies(a, &[(0.004, 1.0, 1.0), (0.006, 1.0, 1.0)], psi, 30e9);
            let exact = ((PI / a).powi(2) + (psi / l).powi(2)).sqrt() * C0 / (2.0 * PI);
            assert!((f[0] - exact).abs() < 1e-9 * exact, "{f:?} {exact}");
            let bands = empty_guide_bands(&EmptyGuide { a, b: 0.01016, period: l, eps_r: 1.0 }, psi, 1);
            assert!((bands[0] - exact).abs() < 1e-9 * exact);
        }
    }
}

//! Material constants from a sample's S-parameters: the Nicolson–Ross–Weir method.
//!
//! A sample of length d fills a line (a coaxial line's TEM, or a guide's dominant mode of
//! cut-off wavelength λc) between two empty stretches. From S₁₁ and S₂₁ referred to the
//! sample's faces, the reflection Γ of the semi-infinite sample and its transmission T follow
//! (A. M. Nicolson and G. F. Ross, "Measurement of the intrinsic properties of materials by
//! time-domain techniques", IEEE Trans. Instrum. Meas. IM-19(4), 1970, pp. 377–382; W. B. Weir,
//! "Automatic measurement of complex dielectric constant and permeability at microwave
//! frequencies", Proc. IEEE 62(1), 1974, pp. 33–36):
//!
//! X = (S₁₁² − S₂₁² + 1)/(2S₁₁), Γ = X ± √(X² − 1) with |Γ| ≤ 1,
//! T = (S₁₁ + S₂₁ − Γ)/(1 − (S₁₁ + S₂₁)Γ).
//!
//! With time dependence e^{jωt} the sample's propagation constant is γ = ln(1/T)/d, so its
//! guide wavelength is 1/Λ = ln(1/T)/(j2πd), ln taken on branch n: ln|1/T| + j(arg(1/T) + 2πn).
//! The empty line's is 1/λg0 = √(1/λ0² − 1/λc²), and the wave impedance ratio
//! (1 + Γ)/(1 − Γ) = μr λg0/Λ gives
//!
//! μr = (1 + Γ)/(1 − Γ) · (1/Λ)/(1/λg0), εr = λ0² (1/Λ² + 1/λc²)/μr.
//!
//! The method fails where the sample is a whole number of half guide wavelengths long: S₁₁
//! vanishes there and X with it. [`extract`] refuses such a point; [`extract_sweep`] keeps the
//! phase of 1/T continuous along the frequencies, so the branch is chosen once.
//!
//! ```
//! use emcraft_theory::nrw::{Sample, extract};
//! use emcraft_theory::{Layer, Te10Line};
//!
//! // 10 mm of εr = 2.2, tan δ = 0.01 between 5 mm and 7 mm of empty WR-90.
//! let a = 22.86e-3;
//! let line = Te10Line { a, layers: vec![Layer::lossless(5e-3, 1.0, 1.0), Layer::lossy(10e-3, 2.2, 1.0, 0.01), Layer::lossless(7e-3, 1.0, 1.0)] };
//! let s = line.solve(10e9)?;
//! let sample = Sample { length: 10e-3, cutoff: emcraft_theory::te10_cutoff(a, 1.0, 1.0), before: 5e-3, after: 7e-3 };
//! let m = extract(10e9, s.s11, s.s21, &sample, 0).map_err(emcraft_theory::TheoryError::Invalid)?;
//! assert!((m.eps_r.re - 2.2).abs() < 1e-9 && (m.eps_r.im + 0.022).abs() < 1e-9 && (m.mu_r.re - 1.0).abs() < 1e-9);
//! // A sample with no reflection (S11 = 0) tells nothing: refused.
//! assert!(extract(10e9, num_complex::Complex64::new(0.0, 0.0), s.s21, &sample, 0).is_err());
//! # Ok::<(), emcraft_theory::TheoryError>(())
//! ```

use emcraft_model::units::C0;
use num_complex::Complex64 as C;
use std::f64::consts::PI;

/// The smallest |S₁₁| the method reads: below it the sample is too close to a half-wave
/// resonance for X = (S₁₁² − S₂₁² + 1)/(2S₁₁) to mean anything.
pub const MIN_S11: f64 = 1e-9;

/// The sample and where it sits: its length, the empty line's cut-off frequency (Hz, 0 for a
/// TEM line) and the empty stretches (m) from port 1's reference plane to the sample's first
/// face and from its second face to port 2's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub length: f64,
    pub cutoff: f64,
    pub before: f64,
    pub after: f64,
}

/// The sample's relative permittivity and permeability (ε = εr′ − jεr″, e^{jωt}).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub eps_r: C,
    pub mu_r: C,
}

/// 1/λg0 (1/m): the empty line's guide wavelength's inverse at `frequency`, `None` at or below
/// its cut-off.
fn empty_line(frequency: f64, cutoff: f64) -> Option<f64> {
    let (k0, kc) = (frequency / C0, cutoff / C0);
    (k0 > kc).then(|| (k0 * k0 - kc * kc).sqrt())
}

/// S₁₁ and S₂₁ moved from the ports' reference planes onto the sample's faces: the empty
/// stretches' phase taken off (S₁₁ e^{+2jβ₀l₁}, S₂₁ e^{+jβ₀(l₁ + l₂)}).
fn at_faces(s11: C, s21: C, beta0: f64, sample: &Sample) -> (C, C) {
    (s11 * C::from_polar(1.0, 2.0 * beta0 * sample.before), s21 * C::from_polar(1.0, beta0 * (sample.before + sample.after)))
}

/// The sample's Γ and T from S at its faces; `None` where S₁₁ vanishes.
fn reflection_transmission(s11: C, s21: C) -> Option<(C, C)> {
    if s11.norm() < MIN_S11 {
        return None;
    }
    let one = C::new(1.0, 0.0);
    let x = (s11 * s11 - s21 * s21 + one) / (2.0 * s11);
    let root = (x * x - one).sqrt();
    let gamma = if (x + root).norm() <= 1.0 { x + root } else { x - root };
    let t = (s11 + s21 - gamma) / (one - (s11 + s21) * gamma);
    Some((gamma, t))
}

/// εr and μr from Γ, ln(1/T) (its branch chosen) and the line.
fn constants(gamma: C, log_inv_t: C, frequency: f64, sample: &Sample, inv_lg0: f64) -> Material {
    let one = C::new(1.0, 0.0);
    let inv_lambda = log_inv_t / C::new(0.0, 2.0 * PI * sample.length);
    let mu_r = (one + gamma) / (one - gamma) * inv_lambda / inv_lg0;
    let (lambda0, inv_lc) = (C0 / frequency, sample.cutoff / C0);
    let eps_r = (inv_lambda * inv_lambda + inv_lc * inv_lc) * lambda0 * lambda0 / mu_r;
    Material { eps_r, mu_r }
}

/// Checks the sample's numbers: a positive length, a cut-off ≥ 0 and empty stretches ≥ 0,
/// all finite.
fn check(sample: &Sample) -> Result<(), String> {
    let finite = [sample.length, sample.cutoff, sample.before, sample.after].iter().all(|x| x.is_finite());
    if !finite || sample.length <= 0.0 || sample.cutoff < 0.0 || sample.before < 0.0 || sample.after < 0.0 {
        return Err("the sample needs a finite positive length, and a cut-off and offsets ≥ 0".into());
    }
    Ok(())
}

/// The sample's εr and μr at `frequency` (Hz) from S₁₁ and S₂₁ at the ports' reference planes,
/// ln(1/T) on `branch` (0 for a sample shorter than a guide wavelength). An error says why
/// where the method cannot read the point.
pub fn extract(frequency: f64, s11: C, s21: C, sample: &Sample, branch: i32) -> Result<Material, String> {
    check(sample)?;
    let inv_lg0 = empty_line(frequency, sample.cutoff).ok_or("at or below the line's cut-off")?;
    let (s11, s21) = at_faces(s11, s21, 2.0 * PI * inv_lg0, sample);
    let (gamma, t) = reflection_transmission(s11, s21).ok_or("S11 vanishes: the sample is a whole number of half wavelengths long")?;
    let inv_t = C::new(1.0, 0.0) / t;
    let log = C::new(inv_t.norm().ln(), inv_t.arg() + 2.0 * PI * f64::from(branch));
    Ok(constants(gamma, log, frequency, sample, inv_lg0))
}

/// [`extract`] at every frequency (ascending, Hz) of a sweep, the phase of 1/T unwrapped along
/// them from `branch` at the first that reads, so a sample longer than a guide wavelength keeps
/// its branch. Each point gives its constants or why it could not be read.
pub fn extract_sweep(frequencies: &[f64], s11: &[C], s21: &[C], sample: &Sample, branch: i32) -> Vec<Result<Material, String>> {
    if let Err(e) = check(sample) {
        return frequencies.iter().map(|_| Err(e.clone())).collect();
    }
    let mut last: Option<f64> = None;
    frequencies
        .iter()
        .zip(s11.iter().zip(s21))
        .map(|(&f, (&a, &b))| {
            let inv_lg0 = empty_line(f, sample.cutoff).ok_or("at or below the line's cut-off")?;
            let (a, b) = at_faces(a, b, 2.0 * PI * inv_lg0, sample);
            let (gamma, t) = reflection_transmission(a, b).ok_or("S11 vanishes: the sample is a whole number of half wavelengths long")?;
            let inv_t = C::new(1.0, 0.0) / t;
            let principal = inv_t.arg();
            let phase = match last {
                None => principal + 2.0 * PI * f64::from(branch),
                Some(p) => principal + 2.0 * PI * ((p - principal) / (2.0 * PI)).round(),
            };
            last = Some(phase);
            Ok(constants(gamma, C::new(inv_t.norm().ln(), phase), f, sample, inv_lg0))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Layer, Te10Line, te10_cutoff};

    const A: f64 = 22.86e-3;

    fn sample(length: f64) -> Sample {
        Sample { length, cutoff: te10_cutoff(A, 1.0, 1.0), before: 3e-3, after: 4e-3 }
    }

    #[test]
    fn a_magnetic_lossy_sample_is_read_back_from_its_closed_form_s() {
        // Derived here: Te10Line is the exact cascade (Pozar §4.4), so NRW inverts it to rounding;
        // 4 mm keeps βd below π up to 12 GHz (branch 0).
        let layer = Layer { magnetic_loss_tangent: 0.05, ..Layer::lossy(4e-3, 3.0, 1.7, 0.02) };
        let line = Te10Line { a: A, layers: vec![Layer::lossless(3e-3, 1.0, 1.0), layer, Layer::lossless(4e-3, 1.0, 1.0)] };
        for f in [8e9, 10e9, 12e9] {
            let s = line.solve(f).unwrap();
            let m = extract(f, s.s11, s.s21, &sample(4e-3), 0).unwrap();
            assert!((m.eps_r - C::new(3.0, -0.06)).norm() < 1e-9, "{f}: {:?}", m.eps_r);
            assert!((m.mu_r - C::new(1.7, -0.085)).norm() < 1e-9, "{f}: {:?}", m.mu_r);
        }
    }

    #[test]
    fn a_long_sample_keeps_its_branch_along_the_sweep() {
        // 60 mm of εr = 4: βd passes 2π inside the band, where the principal branch jumps.
        let line =
            Te10Line { a: A, layers: vec![Layer::lossless(3e-3, 1.0, 1.0), Layer::lossy(60e-3, 4.0, 1.0, 0.001), Layer::lossless(4e-3, 1.0, 1.0)] };
        let fs: Vec<f64> = (0..41).map(|k| 8e9 + 1e8 * f64::from(k)).collect();
        let solved: Vec<_> = fs.iter().map(|f| line.solve(*f).unwrap()).collect();
        let (s11, s21): (Vec<C>, Vec<C>) = solved.iter().map(|s| (s.s11, s.s21)).unzip();
        // βd at 8 GHz ≈ 2π · 60 mm/λg (λg ≈ 19.6 mm): branch 3.
        let read = extract_sweep(&fs, &s11, &s21, &sample(60e-3), 3);
        let good: Vec<&Material> = read.iter().filter_map(|r| r.as_ref().ok()).collect();
        assert!(good.len() >= 39, "only {} points read", good.len());
        assert!(good.iter().all(|m| (m.eps_r.re - 4.0).abs() < 1e-6), "{:?}", good.iter().map(|m| m.eps_r.re).collect::<Vec<_>>());
    }

    #[test]
    fn hostile_samples_and_points_are_refused() {
        let one = C::new(0.5, 0.1);
        assert!(extract(10e9, one, one, &Sample { length: 0.0, ..sample(1e-3) }, 0).is_err());
        assert!(extract(10e9, one, one, &Sample { cutoff: f64::NAN, ..sample(1e-3) }, 0).is_err());
        assert!(extract(5e9, one, one, &sample(1e-3), 0).is_err(), "below cut-off");
        assert!(extract(10e9, C::new(0.0, 0.0), one, &sample(1e-3), 0).is_err());
        let all = extract_sweep(&[10e9, 11e9], &[one, one], &[one, one], &Sample { before: -1.0, ..sample(1e-3) }, 0);
        assert!(all.iter().all(Result::is_err));
    }
}

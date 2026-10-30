//! Time-domain reflectometry from a frequency sweep, as a network analyser's time-domain option
//! does it (J. Dunsmore, *Handbook of Microwave Component Measurements*, 2nd ed., Wiley 2020,
//! ch. 4, time-domain transforms): the reflection S_pp(f) windowed and inverse
//! Fourier-transformed.
//!
//! - **Low pass** — for lines without a cut-off (coaxial, strip): the sweep must run on a
//!   harmonic grid f_k = k Δf (k = 1 … N); S(0), real, is extrapolated from the first two points;
//!   the impulse response, summed, gives the step response ρ(t), which reads as impedance:
//!   Z = Z₀ (1 + ρ)/(1 − ρ). A step of ρ at time τ is a change of impedance at round-trip delay τ.
//! - **Band pass** — for guides with a cut-off (no DC): |h(t)|, the envelope of the band's
//!   impulse response, normalised so a lone reflection of magnitude |Γ| peaks at about |Γ|:
//!   where reflections come from, not their sign.
//!
//! Both are shown over one alias period 1/Δf, a Hann window taming the band edge's ringing; the
//! resolution is about 1/(f_max − f_min) (band pass) or 1/(2 f_max) (low pass).

use std::f64::consts::PI;

use emcraft_solver::Complex64 as C;

use crate::{EngineError, Result};

emcraft_macros::named_enum! {
    /// The transform's kind.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Mode {
        LowPass = "lowpass",
        BandPass = "bandpass",
    }
}

emcraft_macros::named_serde!(Mode);

/// A transform: times (s) from 0 over one alias period and the response at each.
#[derive(Clone, Debug, PartialEq)]
pub struct Tdr {
    pub mode: Mode,
    pub times: Vec<f64>,
    /// ρ(t), the step response (low pass), or |h(t)| (band pass).
    pub values: Vec<f64>,
    /// About the shortest time apart two reflections are told apart (s).
    pub resolution: f64,
}

/// Fewest and most time samples.
const MIN_SAMPLES: usize = 512;
const MAX_SAMPLES: usize = 1 << 14;

/// Whether `freqs` (ascending) are evenly spaced, and from Δf itself (a harmonic grid).
pub fn spacing(freqs: &[f64]) -> Option<(f64, bool)> {
    let n = freqs.len();
    if n < 2 {
        return None;
    }
    let df = (freqs[n - 1] - freqs[0]) / (n - 1) as f64;
    let even = df > 0.0 && freqs.iter().enumerate().all(|(k, f)| (f - (freqs[0] + k as f64 * df)).abs() <= 1e-6 * df);
    even.then_some((df, (freqs[0] - df).abs() <= 1e-6 * df))
}

/// The transform of the reflection `s` at `freqs` (Hz, ascending).
pub fn transform(freqs: &[f64], s: &[C], mode: Mode) -> Result<Tdr> {
    let bad = |why: &str| EngineError::Failed(format!("time-domain reflectometry: {why}"));
    if freqs.len() != s.len() || freqs.len() < 4 {
        return Err(bad("needs a sweep of at least 4 frequencies"));
    }
    if !s.iter().all(|c| c.re.is_finite() && c.im.is_finite()) {
        return Err(bad("the reflection is not finite"));
    }
    let (df, harmonic) = spacing(freqs).ok_or_else(|| bad("the sweep's frequencies must be evenly spaced"))?;
    let n = freqs.len();
    let m = (8 * n).clamp(MIN_SAMPLES, MAX_SAMPLES);
    let period = 1.0 / df;
    let dt = period / m as f64;
    let times: Vec<f64> = (0..m).map(|i| i as f64 * dt).collect();
    match mode {
        Mode::LowPass => {
            if !harmonic {
                return Err(bad(&format!(
                    "low pass needs a harmonic sweep, f_k = k Δf: start at the spacing ({} points from {:e} Hz to {:e} Hz would do)",
                    n,
                    freqs[n - 1] / n as f64,
                    freqs[n - 1]
                )));
            }
            Ok(Tdr { mode, values: step_response(s, m), times, resolution: 1.0 / (2.0 * freqs[n - 1]) })
        }
        Mode::BandPass => Ok(Tdr { mode, values: impulse_envelope(freqs, s, &times), times, resolution: 1.0 / (freqs[n - 1] - freqs[0]) }),
    }
}

/// The reflection at DC from the first two points of a harmonic sweep, magnitude and
/// (unwrapped) phase apart — a delay turns the phase linearly, which a straight line through
/// the complex values would not follow — and its real part (DC is real).
fn dc(s: &[C]) -> f64 {
    let (p1, mut p2) = (s[0].arg(), s[1].arg());
    while p2 - p1 > PI {
        p2 -= 2.0 * PI;
    }
    while p1 - p2 > PI {
        p2 += 2.0 * PI;
    }
    C::from_polar((2.0 * s[0].norm() - s[1].norm()).max(0.0), 2.0 * p1 - p2).re
}

/// ρ(t) at `m` times over the alias period: the windowed impulse response of the harmonic sweep
/// `s` (f_k = k Δf, k = 1 … N), summed.
fn step_response(s: &[C], m: usize) -> Vec<f64> {
    let n = s.len();
    let s0 = dc(s);
    // The window falls to zero past the top.
    let w = |k: usize| 0.5 * (1.0 + (PI * k as f64 / (n + 1) as f64).cos());
    let mut step = 0.0;
    (0..m)
        .map(|i| {
            let x = 2.0 * PI * i as f64 / m as f64;
            let sum: f64 = s.iter().enumerate().map(|(k, c)| w(k + 1) * (c * C::from_polar(1.0, x * (k + 1) as f64)).re).sum();
            step += (s0 + 2.0 * sum) / m as f64;
            step
        })
        .collect()
}

/// |h(t)| at `times`: the envelope of the band's windowed impulse response, normalised by the
/// window's sum.
fn impulse_envelope(freqs: &[f64], s: &[C], times: &[f64]) -> Vec<f64> {
    let n = freqs.len();
    let w = |k: usize| 0.5 * (1.0 - (2.0 * PI * (k + 1) as f64 / (n + 1) as f64).cos());
    let total: f64 = (0..n).map(w).sum();
    times
        .iter()
        .map(|t| {
            let h: C = s.iter().zip(freqs).enumerate().map(|(k, (c, f))| c * w(k) * C::from_polar(1.0, 2.0 * PI * f * t)).sum();
            h.norm() / total
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line of impedance z1 meeting one of z2 at `delay` (round trip): S(f) = ρ e^{−j2πf delay}.
    fn step_line(freqs: &[f64], rho: f64, delay: f64) -> Vec<C> {
        freqs.iter().map(|f| C::from_polar(rho, -2.0 * PI * f * delay)).collect()
    }

    #[test]
    fn a_change_of_impedance_steps_at_its_delay() {
        // 50 Ω meeting 75 Ω after 0.333 ns (5 cm of air there and back).
        let rho = (75.0 - 50.0) / (75.0 + 50.0);
        let delay = 2.0 * 0.05 / emcraft_model::units::C0;
        let freqs: Vec<f64> = (1..=200).map(|k| k as f64 * 0.1e9).collect();
        let t = transform(&freqs, &step_line(&freqs, rho, delay), Mode::LowPass).unwrap();
        let at = |time: f64| t.values[(time / (t.times[1] - t.times[0])).round() as usize];
        // Before the delay nothing, after it ρ (the window's rise: some resolutions wide), and ρ
        // it stays (DC extrapolated with the delay's phase).
        assert!(at(0.5 * delay).abs() < 0.01, "{}", at(0.5 * delay));
        assert!((at(delay + 10.0 * t.resolution) - rho).abs() < 0.01, "{}", at(delay + 10.0 * t.resolution));
        assert!((at(0.4 / 0.1e9) - rho).abs() < 0.01, "{}", at(0.4 / 0.1e9));
        // Not harmonic: refused for low pass, fine for band pass.
        let band: Vec<f64> = (0..101).map(|k| 8e9 + k as f64 * 0.04e9).collect();
        assert!(transform(&band, &step_line(&band, rho, delay), Mode::LowPass).is_err());
        let b = transform(&band, &step_line(&band, rho, delay), Mode::BandPass).unwrap();
        let (peak_at, peak) = b.times.iter().zip(&b.values).fold((0.0, 0.0), |a, (t, v)| if *v > a.1 { (*t, *v) } else { a });
        assert!((peak_at - delay).abs() < b.resolution && (peak - rho).abs() < 0.01, "{peak_at:e} {peak}");
    }

    #[test]
    fn hostile_sweeps_are_refused() {
        let f = [1e9, 2e9, 3e9, 4e9];
        let s = [C::new(0.1, 0.0); 4];
        assert!(transform(&f[..3], &s[..3], Mode::BandPass).is_err());
        assert!(transform(&[1e9, 2e9, 3.5e9, 4e9], &s, Mode::BandPass).is_err());
        assert!(transform(&f, &[C::new(f64::NAN, 0.0); 4], Mode::BandPass).is_err());
    }
}

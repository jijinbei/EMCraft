//! A chirped pulse through a two-port, from its transmission S_pq(f): pulse compression.
//!
//! **The pulse.** A Gaussian envelope of full width at half power T, carried at f₀ and swept
//! linearly in frequency at μ (Hz/s): a(t) = exp(−α t²), α = 2 ln 2/T² − jπμ, so the
//! instantaneous frequency is f₀ + μt and the band it sweeps μT. Its spectrum about f₀ is exact,
//! A(ν) = √(π/α) exp(−π²ν²/α) (G. P. Agrawal, *Nonlinear Fiber Optics*, 5th ed., Academic Press
//! 2013, §3.2: a chirped Gaussian in a dispersive medium).
//!
//! **Through the two-port.** The output envelope is y(t) = ∫ S(f₀ + ν) A(ν) e^{j2πνt} dν, the
//! integral by the trapezoidal rule on a fine grid over the sweep's band, S between the sweep's
//! points by its magnitude and unwrapped phase, each linear (a guide's delay turns the phase
//! fast; real and imaginary parts would not follow it). The part of the pulse's energy outside
//! the band is reported: past 1 % the answer is refused.
//!
//! **What it reads.** The input's and output's peak power (the input's peak 1), their widths
//! at half power, the output's peak time, the compression |y|²max / |a|²max; and the two-port's
//! group delay τ = −dφ/dω at f₀ with its slope D = dτ/df (s/Hz): a chirp μ = −1/D brings every
//! frequency out at once (the instantaneous frequency f leaves at (f − f₀)/μ and spends τ₀ +
//! D(f − f₀) inside), the chirp that compresses best.
//!
//! An ideal line of group delay τ₀ + Dν (S = e^{−j2π(τ₀ν + Dν²/2)}) gives y in closed form,
//! y(t + τ₀) = √(α′/α) exp(−α′t²) with 1/α′ = 1/α + jD/π: the check of `tests`.

use std::f64::consts::{LN_2, PI};

use emcraft_solver::Complex64 as C;

use crate::{EngineError, Result};

/// The fine frequency grid's points and the output's time samples.
pub const GRID: usize = 4096;
pub const TIMES: usize = 1024;
/// The samples about the output's largest that find its peak.
const PEAK_SAMPLES: usize = 64;

/// The most of the pulse's energy that may lie outside the swept band.
pub const MAX_OUTSIDE: f64 = 0.01;

/// A chirped Gaussian pulse: carrier `f0` (Hz), full width at half power `width` (s), chirp
/// `rate` μ (Hz/s; 0: none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chirp {
    pub f0: f64,
    pub width: f64,
    pub rate: f64,
}

impl Chirp {
    /// α of a(t) = exp(−α t²).
    fn alpha(&self) -> C {
        C::new(2.0 * LN_2 / (self.width * self.width), -PI * self.rate)
    }

    /// The envelope at time t (s).
    pub fn envelope(&self, t: f64) -> C {
        (-self.alpha() * t * t).exp()
    }

    /// The envelope's spectrum at ν = f − f₀ (Hz).
    pub fn spectrum(&self, nu: f64) -> C {
        let a = self.alpha();
        (C::new(PI, 0.0) / a).sqrt() * (-(PI * PI * nu * nu) / a).exp()
    }
}

/// The pulse through the two-port: times (s, from the input's peak), the input's and output's
/// power envelopes (the input's peak 1), and what they read.
#[derive(Clone, Debug, PartialEq)]
pub struct Pulse {
    pub times: Vec<f64>,
    pub input: Vec<f64>,
    pub output: Vec<f64>,
    pub input_width: f64,
    pub output_width: f64,
    pub output_peak: f64,
    pub output_time: f64,
    /// The group delay at f₀ (s) and its slope D (s/Hz).
    pub delay: f64,
    pub dispersion: f64,
    /// The pulse's energy outside the band (a fraction).
    pub outside: f64,
}

impl Pulse {
    /// |y|²max / |a|²max.
    pub fn compression(&self) -> f64 {
        self.output_peak
    }

    /// The chirp that compresses best, −1/D (Hz/s; none without dispersion).
    pub fn best_rate(&self) -> Option<f64> {
        (self.dispersion.abs() > 0.0 && self.dispersion.is_finite()).then(|| -1.0 / self.dispersion)
    }
}

fn bad(why: impl Into<String>) -> EngineError {
    EngineError::failed(format!("pulse: {}", why.into()))
}

/// S at `f` (Hz) from the sweep `freqs`, `s` with its phase `phase` unwrapped: magnitude and phase
/// each linear between the points; none outside the sweep.
fn sample(freqs: &[f64], mag: &[f64], phase: &[f64], f: f64) -> Option<C> {
    let k = freqs.partition_point(|x| *x <= f);
    if k == 0 || k > freqs.len() || (k == freqs.len() && f > freqs[freqs.len() - 1]) {
        return None;
    }
    let k = k.min(freqs.len() - 1);
    let (i, j) = (k - 1, k);
    let w = if freqs[j] > freqs[i] { (f - freqs[i]) / (freqs[j] - freqs[i]) } else { 0.0 };
    Some(C::from_polar(mag[i] * (1.0 - w) + mag[j] * w, phase[i] * (1.0 - w) + phase[j] * w))
}

/// The phase of `s`, unwrapped along the sweep.
pub fn unwrapped(s: &[C]) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::with_capacity(s.len());
    for c in s {
        let p = c.arg();
        let p = match out.last() {
            Some(&q) => p + 2.0 * PI * ((q - p) / (2.0 * PI)).round(),
            None => p,
        };
        out.push(p);
    }
    out
}

/// The width at half power of `y` sampled at `times` (s), between the crossings found by
/// linear interpolation either side of its peak.
fn half_width(times: &[f64], y: &[f64]) -> f64 {
    let Some((k, &peak)) = y.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)) else { return 0.0 };
    let half = 0.5 * peak;
    let cross = |range: &mut dyn Iterator<Item = usize>| -> Option<f64> {
        for i in range {
            let (a, b) = (i, i + 1);
            if (y[a] - half) * (y[b] - half) <= 0.0 && y[a] != y[b] {
                return Some(times[a] + (half - y[a]) / (y[b] - y[a]) * (times[b] - times[a]));
            }
        }
        None
    };
    let left = cross(&mut (0..k).rev());
    let right = cross(&mut (k..y.len().saturating_sub(1)));
    match (left, right) {
        (Some(l), Some(r)) => r - l,
        _ => f64::NAN,
    }
}

/// The pulse `chirp` through the transmission `s` swept at `freqs` (Hz, ascending), its phase
/// unwrapped along the sweep (which must sample it finely enough: less than π between points).
pub fn through(freqs: &[f64], s: &[C], chirp: Chirp) -> Result<Pulse> {
    if !s.iter().all(|c| c.re.is_finite() && c.im.is_finite()) {
        return Err(bad("the sweep's S must be finite"));
    }
    let mag: Vec<f64> = s.iter().map(|c| c.norm()).collect();
    through_phase(freqs, &mag, &unwrapped(s), chirp)
}

/// [`through`] with the transmission given as its magnitude and its phase, already continuous
/// (a Bloch wave's −βL over a long guide, which turns many times between the sweep's points).
pub fn through_phase(freqs: &[f64], mag: &[f64], phase: &[f64], chirp: Chirp) -> Result<Pulse> {
    if freqs.len() != mag.len() || freqs.len() != phase.len() || freqs.len() < 4 {
        return Err(bad("needs a sweep of at least 4 frequencies"));
    }
    if freqs.windows(2).any(|w| !(w[1] > w[0])) || !mag.iter().chain(phase).all(|x| x.is_finite()) {
        return Err(bad("the sweep's frequencies must ascend and its S be finite"));
    }
    let Chirp { f0, width, rate } = chirp;
    let (lo, hi) = (freqs[0], freqs[freqs.len() - 1]);
    if !(f0 > lo && f0 < hi) {
        return Err(bad(format!("the carrier must lie inside the sweep ({lo:e} to {hi:e} Hz)")));
    }
    if !(width.is_finite() && width > 0.0) || !rate.is_finite() {
        return Err(bad("the width must be positive and the chirp finite"));
    }
    // The spectrum on a grid over the band; its energy inside against all of it.
    let dnu = (hi - lo) / (GRID - 1) as f64;
    let nus: Vec<f64> = (0..GRID).map(|i| lo + i as f64 * dnu - f0).collect();
    let spec: Vec<C> = nus.iter().map(|&nu| chirp.spectrum(nu)).collect();
    let inside: f64 = spec.iter().map(C::norm_sqr).sum::<f64>() * dnu;
    // |A(ν)|² = (π/|α|) exp(−2π² Re(1/α) ν²): its whole integral in closed form.
    let a = chirp.alpha();
    let re_inv = (C::new(1.0, 0.0) / a).re;
    let total = PI / a.norm() * (PI / (2.0 * PI * PI * re_inv)).sqrt();
    let outside = (1.0 - inside / total).max(0.0);
    if !(outside <= MAX_OUTSIDE) {
        return Err(bad(format!(
            "{:.1} % of the pulse's energy lies outside the sweep: widen the sweep, lengthen the pulse or chirp less",
            100.0 * outside
        )));
    }
    let transmitted: Vec<C> = nus.iter().zip(&spec).map(|(&nu, a)| sample(freqs, mag, phase, f0 + nu).unwrap_or_default() * a).collect();
    // The group delay and its slope at f₀ from the unwrapped phase, by central differences over
    // the sweep's neighbouring points.
    let tau: Vec<f64> = (0..freqs.len())
        .map(|k| {
            let (i, j) = (k.saturating_sub(1), (k + 1).min(freqs.len() - 1));
            -(phase[j] - phase[i]) / (2.0 * PI * (freqs[j] - freqs[i]))
        })
        .collect();
    let k = freqs.partition_point(|x| *x <= f0).clamp(1, freqs.len() - 1);
    let w = (f0 - freqs[k - 1]) / (freqs[k] - freqs[k - 1]);
    let delay = tau[k - 1] * (1.0 - w) + tau[k] * w;
    let (i, j) = (k.saturating_sub(2), (k + 1).min(freqs.len() - 1));
    let dispersion = (tau[j] - tau[i]) / (freqs[j] - freqs[i]);
    // The times: about the input's spread and the band's delays.
    let (tmin, tmax) = tau.iter().fold((0.0f64, 0.0f64), |(a, b), &t| (a.min(t), b.max(t)));
    // The pulse's band (its Gaussian's and its sweep's) spread by the dispersion, and its own
    // width, either side.
    let band = 0.44 / width + rate.abs() * width;
    let spread = 2.0 * width + 2.0 * dispersion.abs() * band;
    let reach = 1.5 * width.max(1.0 / (hi - lo));
    let (t0, t1) = (tmin.min(0.0) - reach - spread, tmax.max(0.0) + reach + spread);
    let dt = (t1 - t0) / (TIMES - 1) as f64;
    let times: Vec<f64> = (0..TIMES).map(|i| t0 + i as f64 * dt).collect();
    let input: Vec<f64> = times.iter().map(|&t| chirp.envelope(t).norm_sqr()).collect();
    let power = |t: f64| (nus.iter().zip(&transmitted).map(|(&nu, x)| x * C::from_polar(1.0, 2.0 * PI * nu * t)).sum::<C>() * dnu).norm_sqr();
    let output: Vec<f64> = times.iter().map(|&t| power(t)).collect();
    let (kp, _) = output.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).ok_or_else(|| bad("no samples"))?;
    // The peak itself, between the samples either side of the largest: 64 more.
    let (output_peak, output_time) = (0..=PEAK_SAMPLES)
        .map(|i| times[kp] + dt * (2.0 * i as f64 / PEAK_SAMPLES as f64 - 1.0))
        .map(|t| (power(t), t))
        .fold((f64::NEG_INFINITY, times[kp]), |best, x| if x.0 > best.0 { x } else { best });
    Ok(Pulse {
        input_width: half_width(&times, &input),
        output_width: half_width(&times, &output),
        output_peak,
        output_time,
        times,
        input,
        output,
        delay,
        dispersion,
        outside,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ideal dispersive line, S = e^{−j2π(τ₀ν + Dν²/2)}: a chirped Gaussian comes out as the
    /// closed form says — compressed by |α′/α| with 1/α′ = 1/α + jD/π, at τ₀ — and with the
    /// chirp −1/D it is compressed most.
    #[test]
    fn a_chirp_through_a_dispersive_line_compresses_as_the_closed_form_says() {
        let (f0, tau0, d) = (10e9, 5e-9, 8e-18); // group delay 5 ns, rising 8 ns per GHz
        let freqs: Vec<f64> = (0..=4000).map(|k| 8e9 + k as f64 * 1e6).collect();
        let s: Vec<C> = freqs.iter().map(|&f| C::from_polar(1.0, -2.0 * PI * (tau0 * (f - f0) + 0.5 * d * (f - f0) * (f - f0)))).collect();
        let width = 4e-9;
        for rate in [-1.0 / d, -0.5 / d, 0.0] {
            let chirp = Chirp { f0, width, rate };
            let p = through(&freqs, &s, chirp).unwrap_or_else(|e| panic!("{e}"));
            let a = chirp.alpha();
            let inv = C::new(1.0, 0.0) / a + C::new(0.0, d / PI);
            let a2 = C::new(1.0, 0.0) / inv;
            let exact = (a2 / a).norm();
            assert!((p.compression() - exact).abs() < 1e-3 * exact, "rate {rate:e}: {} against {exact}", p.compression());
            assert!((p.output_time - tau0).abs() < 0.02e-9, "{:e}", p.output_time);
            assert!((p.dispersion - d).abs() < 1e-3 * d && (p.delay - tau0).abs() < 1e-3 * tau0);
            // The output's width: that of |exp(−α′t²)|², √(2 ln 2 / (2 Re α′)) × 2.
            let w = 2.0 * (LN_2 / (2.0 * a2.re)).sqrt();
            assert!((p.output_width - w).abs() < 0.01 * w, "{:e} against {w:e}", p.output_width);
        }
        let best = through(&freqs, &s, Chirp { f0, width, rate: -1.0 / d }).map_or(0.0, |p| p.compression());
        let half = through(&freqs, &s, Chirp { f0, width, rate: -0.5 / d }).map_or(0.0, |p| p.compression());
        let none = through(&freqs, &s, Chirp { f0, width, rate: 0.0 }).map_or(0.0, |p| p.compression());
        assert!(best > half && half > none, "{best} {half} {none}");
        let best_rate = through(&freqs, &s, Chirp { f0, width, rate: 0.0 }).ok().and_then(|p| p.best_rate()).unwrap_or(0.0);
        assert!((best_rate * d + 1.0).abs() < 1e-6, "{best_rate:e}");
    }

    #[test]
    fn a_pulse_outside_the_band_or_hostile_numbers_are_refused() {
        let freqs: Vec<f64> = (0..=100).map(|k| 9e9 + k as f64 * 2e7).collect();
        let s = vec![C::new(1.0, 0.0); freqs.len()];
        // 0.2 ns wide: about 2 GHz of band, wider than the sweep's.
        assert!(through(&freqs, &s, Chirp { f0: 10e9, width: 0.2e-9, rate: 0.0 }).is_err());
        assert!(through(&freqs, &s, Chirp { f0: 12e9, width: 5e-9, rate: 0.0 }).is_err());
        assert!(through(&freqs, &s, Chirp { f0: 10e9, width: f64::NAN, rate: 0.0 }).is_err());
        assert!(through(&freqs[..3], &s[..3], Chirp { f0: 9.01e9, width: 5e-9, rate: 0.0 }).is_err());
        // A flat line passes it unchanged: no compression, the same width.
        let p = through(&freqs, &s, Chirp { f0: 10e9, width: 5e-9, rate: 0.0 }).unwrap_or_else(|e| panic!("{e}"));
        assert!((p.compression() - 1.0).abs() < 1e-3 && (p.output_width - 5e-9).abs() < 0.02e-9);
    }
}

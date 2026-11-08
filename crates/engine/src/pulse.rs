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
//!
//! **A chirp matched to the line.** A linear chirp undoes only the slope of the group delay; a
//! line whose delay curves (a guide near its cut-off or a stop band) leaves the rest of it spread.
//! The matched chirp follows the delay itself: its instantaneous frequency f(t) is the one that
//! leaves at t and arrives with all the others, t + s (τ(f) − τ(f₀)) = 0 (stationary phase: each
//! frequency spends τ(f) in the line), on the monotonic stretch of τ about f₀, held at its ends
//! past it; s = 1 is the stationary-phase answer and the search tries others. Where it is held
//! the envelope falls away (within a tenth of the stretch's times): a held frequency arrives all
//! at once, late, and what the Gaussian still carried there would trail the peak as a tail (a
//! guide whose delay turns back below a stop band). Its spectrum is
//! the envelope's transform, taken numerically. [`optimize`] picks, for a given carrier and
//! width, the linear rate and the matched chirp's s that peak the output highest (golden-section
//! searches, each bounded), and returns the better of the two.
//!
//! **Instantaneous frequency.** Of the input and the output, f₀ + (1/2π) d arg(y)/dt where the
//! power is above a hundredth of its peak: the input's sweep, and what the line left of it.

use std::f64::consts::{LN_2, PI};

use emcraft_solver::Complex64 as C;

use crate::{EngineError, Result};

/// The fine frequency grid's points and the output's time samples.
pub const GRID: usize = 4096;
pub const TIMES: usize = 1024;
/// The samples about the output's largest that find its peak.
const PEAK_SAMPLES: usize = 64;

/// The width a pulse is given when nothing says (ns): the pulse view's first.
pub const DEFAULT_WIDTH_NS: f64 = 8.0;

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

/// How the input's frequency moves: linearly at `rate` (Hz/s), or matched to the line's group
/// delay, scaled by `scale` (see the module notes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Law {
    Linear { rate: f64 },
    Matched { scale: f64 },
}

/// An input pulse: a Gaussian envelope of width at half power `width` (s) about the carrier
/// `f0` (Hz), its frequency moving by `law`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    pub f0: f64,
    pub width: f64,
    pub law: Law,
}

impl From<Chirp> for Input {
    fn from(c: Chirp) -> Input {
        Input { f0: c.f0, width: c.width, law: Law::Linear { rate: c.rate } }
    }
}

/// The pulse through the two-port: times (s, from the input's peak), the input's and output's
/// power envelopes (the input's peak 1) and instantaneous frequencies (Hz; NaN where the power
/// is below a hundredth of its peak), and what they read.
#[derive(Clone, Debug, PartialEq)]
pub struct Pulse {
    pub times: Vec<f64>,
    pub input: Vec<f64>,
    pub output: Vec<f64>,
    pub input_frequency: Vec<f64>,
    pub output_frequency: Vec<f64>,
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
pub fn through(freqs: &[f64], s: &[C], chirp: impl Into<Input>) -> Result<Pulse> {
    if !s.iter().all(|c| c.re.is_finite() && c.im.is_finite()) {
        return Err(bad("the sweep's S must be finite"));
    }
    let mag: Vec<f64> = s.iter().map(|c| c.norm()).collect();
    through_phase(freqs, &mag, &unwrapped(s), chirp)
}

/// A line: its sweep (Hz), its transmission's magnitude and continuous phase, its group delay
/// at each point (s).
struct Line<'a> {
    freqs: &'a [f64],
    mag: &'a [f64],
    phase: &'a [f64],
    tau: Vec<f64>,
}

impl<'a> Line<'a> {
    fn new(freqs: &'a [f64], mag: &'a [f64], phase: &'a [f64]) -> Result<Line<'a>> {
        if freqs.len() != mag.len() || freqs.len() != phase.len() || freqs.len() < 4 {
            return Err(bad("needs a sweep of at least 4 frequencies"));
        }
        if freqs.windows(2).any(|w| !(w[1] > w[0])) || !mag.iter().chain(phase).all(|x| x.is_finite()) {
            return Err(bad("the sweep's frequencies must ascend and its S be finite"));
        }
        // The group delay by central differences over the neighbouring points.
        let tau = (0..freqs.len())
            .map(|k| {
                let (i, j) = (k.saturating_sub(1), (k + 1).min(freqs.len() - 1));
                -(phase[j] - phase[i]) / (2.0 * PI * (freqs[j] - freqs[i]))
            })
            .collect();
        Ok(Line { freqs, mag, phase, tau })
    }

    fn band(&self) -> (f64, f64) {
        (self.freqs[0], self.freqs[self.freqs.len() - 1])
    }

    /// The group delay at `f` and its slope there.
    fn delay_at(&self, f: f64) -> (f64, f64) {
        let (freqs, tau) = (self.freqs, &self.tau);
        let k = freqs.partition_point(|x| *x <= f).clamp(1, freqs.len() - 1);
        let w = (f - freqs[k - 1]) / (freqs[k] - freqs[k - 1]);
        let delay = tau[k - 1] * (1.0 - w) + tau[k] * w;
        let (i, j) = (k.saturating_sub(2), (k + 1).min(freqs.len() - 1));
        (delay, (tau[j] - tau[i]) / (freqs[j] - freqs[i]))
    }

    /// The matched chirp's frequency at each of `times`: the f with t = −s (τ(f) − τ(f₀)) on
    /// the monotonic stretch of τ about f₀, held at the stretch's ends past it; with the times
    /// the stretch spans.
    fn matched(&self, f0: f64, scale: f64, times: &[f64]) -> (Vec<f64>, (f64, f64)) {
        let (freqs, tau) = (self.freqs, &self.tau);
        let (tau0, slope) = self.delay_at(f0);
        let k = freqs.partition_point(|x| *x <= f0).clamp(1, freqs.len() - 1);
        let rising = slope >= 0.0;
        let up = |a: f64, b: f64| if rising { b >= a } else { b <= a };
        let mut lo = k - 1;
        while lo > 0 && up(tau[lo - 1], tau[lo]) {
            lo -= 1;
        }
        let mut hi = k;
        while hi + 1 < freqs.len() && up(tau[hi], tau[hi + 1]) {
            hi += 1;
        }
        // (t, f) along the stretch, t ascending.
        let mut table: Vec<(f64, f64)> = (lo..=hi).map(|i| (-scale * (tau[i] - tau0), freqs[i])).collect();
        table.sort_by(|a, b| a.0.total_cmp(&b.0));
        table.dedup_by(|a, b| a.0 == b.0);
        let span = (table.first().map_or(0.0, |x| x.0), table.last().map_or(0.0, |x| x.0));
        let f = times
            .iter()
            .map(|&t| {
                let j = table.partition_point(|x| x.0 <= t);
                match j {
                    0 => table.first().map_or(f0, |x| x.1),
                    j if j >= table.len() => table.last().map_or(f0, |x| x.1),
                    j => {
                        let ((ta, fa), (tb, fb)) = (table[j - 1], table[j]);
                        fa + (fb - fa) * (t - ta) / (tb - ta)
                    }
                }
            })
            .collect();
        (f, span)
    }
}

/// Past its stretch the matched chirp's envelope falls as e^{−(d/e)²}, d how far past and
/// e this share of the stretch's times.
const EDGE: f64 = 0.1;

/// The samples of the matched chirp's envelope taken for its transform, either side of the
/// peak out to three widths.
const ENVELOPE_SAMPLES: usize = 2048;

/// [`through`] with the transmission given as its magnitude and its phase, already continuous
/// (a Bloch wave's −βL over a long guide, which turns many times between the sweep's points).
pub fn through_phase(freqs: &[f64], mag: &[f64], phase: &[f64], input: impl Into<Input>) -> Result<Pulse> {
    let line = Line::new(freqs, mag, phase)?;
    pass(&line, input.into())
}

/// The amplitude of a Gaussian of power width `width` at t.
fn gauss(width: f64, t: f64) -> f64 {
    (-2.0 * LN_2 * t * t / (width * width)).exp()
}

fn pass(line: &Line, input: Input) -> Result<Pulse> {
    let Input { f0, width, law } = input;
    let (lo, hi) = line.band();
    if !(f0 > lo && f0 < hi) {
        return Err(bad(format!("the carrier must lie inside the sweep ({lo:e} to {hi:e} Hz)")));
    }
    let finite_law = match law {
        Law::Linear { rate } => rate.is_finite(),
        Law::Matched { scale } => scale.is_finite() && scale > 0.0 && scale < 100.0,
    };
    if !(width.is_finite() && width > 0.0) || !finite_law {
        return Err(bad("the width must be positive and the chirp finite"));
    }
    let dnu = (hi - lo) / (GRID - 1) as f64;
    let nus: Vec<f64> = (0..GRID).map(|i| lo + i as f64 * dnu - f0).collect();
    // The input: its envelope and its frequency at any time, its spectrum on the grid, its
    // whole energy.
    let (spec, total): (Vec<C>, f64) = match law {
        Law::Linear { rate } => {
            let chirp = Chirp { f0, width, rate };
            let a = chirp.alpha();
            let re_inv = (C::new(1.0, 0.0) / a).re;
            (nus.iter().map(|&nu| chirp.spectrum(nu)).collect(), PI / a.norm() * (PI / (2.0 * PI * PI * re_inv)).sqrt())
        }
        Law::Matched { scale } => {
            let reach = 3.0 * width;
            let dt = 2.0 * reach / (ENVELOPE_SAMPLES - 1) as f64;
            let ts: Vec<f64> = (0..ENVELOPE_SAMPLES).map(|i| -reach + i as f64 * dt).collect();
            let env = matched_envelope(line, f0, width, scale, &ts);
            let total = env.iter().map(C::norm_sqr).sum::<f64>() * dt;
            let spec = nus.iter().map(|&nu| evenly(&env, ts[0], dt, -nu) * dt).collect();
            (spec, total)
        }
    };
    let inside: f64 = spec.iter().map(C::norm_sqr).sum::<f64>() * dnu;
    let outside = (1.0 - inside / total).max(0.0);
    if !(outside <= MAX_OUTSIDE) {
        return Err(bad(format!(
            "{:.1} % of the pulse's energy lies outside the sweep: widen the sweep, lengthen the pulse or chirp less",
            100.0 * outside
        )));
    }
    let transmitted: Vec<C> =
        nus.iter().zip(&spec).map(|(&nu, a)| sample(line.freqs, line.mag, line.phase, f0 + nu).unwrap_or_default() * a).collect();
    let (delay, dispersion) = line.delay_at(f0);
    // The times: about the input's spread and the band's delays.
    let (tmin, tmax) = line.tau.iter().fold((0.0f64, 0.0f64), |(a, b), &t| (a.min(t), b.max(t)));
    let rate = match law {
        Law::Linear { rate } => rate.abs(),
        Law::Matched { .. } => (hi - lo) / width,
    };
    let band = 0.44 / width + rate * width;
    let spread = 2.0 * width + 2.0 * dispersion.abs() * band;
    let reach = 1.5 * width.max(1.0 / (hi - lo));
    let (t0, t1) = (tmin.min(0.0) - reach - spread, tmax.max(0.0) + reach + spread);
    let dt = (t1 - t0) / (TIMES - 1) as f64;
    let times: Vec<f64> = (0..TIMES).map(|i| t0 + i as f64 * dt).collect();
    let input_env: Vec<C> = match law {
        Law::Linear { rate } => times.iter().map(|&t| Chirp { f0, width, rate }.envelope(t)).collect(),
        Law::Matched { scale } => matched_envelope(line, f0, width, scale, &times),
    };
    let field = |t: f64| evenly(&transmitted, nus[0], dnu, t) * dnu;
    let output_env: Vec<C> = times.iter().map(|&t| field(t)).collect();
    let input: Vec<f64> = input_env.iter().map(C::norm_sqr).collect();
    let output: Vec<f64> = output_env.iter().map(C::norm_sqr).collect();
    let (kp, _) = output.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).ok_or_else(|| bad("no samples"))?;
    // The peak itself, between the samples either side of the largest: 64 more.
    let (output_peak, output_time) = (0..=PEAK_SAMPLES)
        .map(|i| times[kp] + dt * (2.0 * i as f64 / PEAK_SAMPLES as f64 - 1.0))
        .map(|t| (field(t).norm_sqr(), t))
        .fold((f64::NEG_INFINITY, times[kp]), |best, x| if x.0 > best.0 { x } else { best });
    Ok(Pulse {
        input_width: half_width(&times, &input),
        output_width: half_width(&times, &output),
        input_frequency: instantaneous(&input_env, dt, f0),
        output_frequency: instantaneous(&output_env, dt, f0),
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

/// Σ x_k e^{j2π (x₀ + k dx) y}: the samples x at evenly spaced points from x₀, each turned by
/// the phase their point makes with y — the exponentials by recurrence (one rotation a step,
/// renormalized every 256 against rounding), not each its own sine and cosine.
fn evenly(x: &[C], x0: f64, dx: f64, y: f64) -> C {
    let step = C::from_polar(1.0, 2.0 * PI * dx * y);
    let mut turn = C::from_polar(1.0, 2.0 * PI * x0 * y);
    let mut sum = C::new(0.0, 0.0);
    for (k, v) in x.iter().enumerate() {
        sum += v * turn;
        turn *= step;
        if k % 256 == 255 {
            turn = C::from_polar(1.0, 2.0 * PI * (x0 + (k + 1) as f64 * dx) * y);
        }
    }
    sum
}

/// The matched chirp's envelope at `times`: the Gaussian with the phase 2π ∫ (f(t′) − f₀) dt′
/// from the peak (t = 0), f the matched frequency.
fn matched_envelope(line: &Line, f0: f64, width: f64, scale: f64, times: &[f64]) -> Vec<C> {
    let (f, (lo, hi)) = line.matched(f0, scale, times);
    let edge = (EDGE * (hi - lo)).max(f64::MIN_POSITIVE);
    let taper = |t: f64| {
        let out = (lo - t).max(t - hi).max(0.0) / edge;
        (-out * out).exp()
    };
    // The phase by the trapezoidal rule along the times, then shifted to be 0 at t = 0.
    let mut psi = vec![0.0; times.len()];
    for i in 1..times.len() {
        psi[i] = psi[i - 1] + PI * (f[i] + f[i - 1] - 2.0 * f0) * (times[i] - times[i - 1]);
    }
    let k = times.partition_point(|t| *t < 0.0).min(times.len().saturating_sub(1));
    let zero = psi.get(k).copied().unwrap_or(0.0);
    times.iter().zip(&psi).map(|(&t, &p)| C::from_polar(gauss(width, t) * taper(t), p - zero)).collect()
}

/// f₀ + (1/2π) d arg(y)/dt at each sample (central differences of the phase between
/// neighbours), NaN where |y|² is below a hundredth of its peak.
fn instantaneous(y: &[C], dt: f64, f0: f64) -> Vec<f64> {
    let peak = y.iter().map(C::norm_sqr).fold(0.0, f64::max);
    (0..y.len())
        .map(|k| {
            let (i, j) = (k.saturating_sub(1), (k + 1).min(y.len() - 1));
            if y[k].norm_sqr() < 1e-2 * peak || i == j {
                return f64::NAN;
            }
            let turn = (y[j] * y[i].conj()).arg();
            f0 + turn / (2.0 * PI * (j - i) as f64 * dt)
        })
        .collect()
}

/// A pulse measured in the time domain: its power envelopes (the incident's peak 1) at
/// `times` (s, from the launch), their widths at half power and the delay between their peaks.
#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    pub times: Vec<f64>,
    pub input: Vec<f64>,
    pub output: Vec<f64>,
    pub input_width: f64,
    pub output_width: f64,
    pub compression: f64,
    pub delay: f64,
}

/// The power envelope of a signal carried at `f0` (Hz) sampled `dt` (s) apart: the signal
/// times 2e^{−j2πf₀t}, its double-frequency part taken out by two running means each one
/// carrier period long (each nulls 2f₀ and its neighbours fall twice as fast), |·|².
fn envelope(signal: &[f64], dt: f64, f0: f64) -> Vec<f64> {
    let n = ((1.0 / (f0 * dt)).round() as usize).max(1);
    let base: Vec<C> = signal.iter().enumerate().map(|(k, &v)| C::from_polar(2.0 * v, -2.0 * PI * f0 * k as f64 * dt)).collect();
    let mean = |x: &[C]| -> Vec<C> {
        let mut out = vec![C::new(0.0, 0.0); x.len()];
        let mut sum = C::new(0.0, 0.0);
        for k in 0..x.len() {
            sum += x[k];
            if k >= n {
                sum -= x[k - n];
            }
            // Centred: the mean of the n samples ending at k belongs to k − n/2.
            if let Some(o) = out.get_mut(k.saturating_sub(n / 2)) {
                *o = sum / n as f64;
            }
        }
        out
    };
    mean(&mean(&base)).iter().map(C::norm_sqr).collect()
}

/// The incident and transmitted signals of a time-domain run (`dt` apart, carried at `f0`)
/// measured as pulses.
///
/// ```
/// use std::f64::consts::PI;
/// // A 2 ns pulse at 10 GHz, transmitted 3 ns later at half the amplitude.
/// let (dt, f0) = (1e-12, 10e9);
/// let wave = |t: f64| (-2.0 * 2f64.ln() * (t / 2e-9).powi(2)).exp() * (2.0 * PI * f0 * t).cos();
/// let inc: Vec<f64> = (0..20000).map(|k| wave(k as f64 * dt - 5e-9)).collect();
/// let out: Vec<f64> = (0..20000).map(|k| 0.5 * wave(k as f64 * dt - 8e-9)).collect();
/// let m = emcraft_engine::pulse::measure(dt, f0, &inc, &out).unwrap();
/// assert!((m.compression - 0.25).abs() < 1e-3 && (m.delay - 3e-9).abs() < 2e-12);
/// assert!((m.input_width - 2e-9).abs() < 1e-11);
/// assert!(emcraft_engine::pulse::measure(dt, f0, &[], &[]).is_err());
/// ```
pub fn measure(dt: f64, f0: f64, incident: &[f64], transmitted: &[f64]) -> Result<Measured> {
    if !(dt > 0.0 && f0 > 0.0 && dt.is_finite() && f0.is_finite()) || incident.len() < 4 || incident.len() != transmitted.len() {
        return Err(bad("needs two signals of the same length, at least 4 samples"));
    }
    let (a, b) = (envelope(incident, dt, f0), envelope(transmitted, dt, f0));
    let peak = |y: &[f64]| y.iter().enumerate().fold((0, f64::NEG_INFINITY), |m, (k, &v)| if v > m.1 { (k, v) } else { m });
    let ((ka, pa), (kb, pb)) = (peak(&a), peak(&b));
    if !(pa > 0.0 && pa.is_finite() && pb.is_finite()) {
        return Err(bad("the incident signal carries nothing at the carrier"));
    }
    // A peak between samples: the parabola through it and its neighbours.
    let refine = |y: &[f64], k: usize| {
        if k == 0 || k + 1 >= y.len() {
            return k as f64 * dt;
        }
        let (l, c, r) = (y[k - 1], y[k], y[k + 1]);
        let d = l - 2.0 * c + r;
        (k as f64 + if d < 0.0 { 0.5 * (l - r) / d } else { 0.0 }) * dt
    };
    let times: Vec<f64> = (0..a.len()).map(|k| k as f64 * dt).collect();
    let input: Vec<f64> = a.iter().map(|v| v / pa).collect();
    let output: Vec<f64> = b.iter().map(|v| v / pa).collect();
    Ok(Measured {
        input_width: half_width(&times, &input),
        output_width: half_width(&times, &output),
        compression: pb / pa,
        delay: refine(&b, kb) - refine(&a, ka),
        times,
        input,
        output,
    })
}

/// The signal `incident` (sampled `dt` apart from 0) through the two-port of transmission
/// magnitude `mag` and continuous phase `phase` at `freqs` (Hz): its transform on a fine grid
/// over the sweep's band, times S, back in time (twice the real part: the signal is real). With
/// the share of its energy outside the band (Parseval), which the answer leaves out.
pub fn predict(dt: f64, incident: &[f64], freqs: &[f64], mag: &[f64], phase: &[f64]) -> Result<(Vec<f64>, f64)> {
    let line = Line::new(freqs, mag, phase)?;
    if !(dt > 0.0 && dt.is_finite()) || incident.is_empty() || !incident.iter().all(|x| x.is_finite()) {
        return Err(bad("needs a finite signal and a time step > 0"));
    }
    let (lo, hi) = line.band();
    let df = (hi - lo) / (GRID - 1) as f64;
    let x: Vec<C> = incident.iter().map(|&v| C::new(v, 0.0)).collect();
    let spectrum: Vec<C> = (0..GRID).map(|i| lo + i as f64 * df).map(|f| evenly(&x, 0.0, dt, -f) * dt).collect();
    let total: f64 = incident.iter().map(|v| v * v).sum::<f64>() * dt;
    let inside = 2.0 * spectrum.iter().map(C::norm_sqr).sum::<f64>() * df;
    let outside = if total > 0.0 { (1.0 - inside / total).max(0.0) } else { 1.0 };
    let through: Vec<C> = spectrum.iter().enumerate().map(|(i, a)| sample(freqs, mag, phase, lo + i as f64 * df).unwrap_or_default() * a).collect();
    let out = (0..incident.len()).map(|k| 2.0 * (evenly(&through, lo, df, k as f64 * dt) * df).re).collect();
    Ok((out, outside))
}

/// The most passes each golden-section search of [`optimize`] makes.
const SEARCH_STEPS: usize = 24;

/// The golden-section search for the largest of `f` on [a, b] (a unimodal guess), bounded.
fn golden(mut a: f64, mut b: f64, f: impl Fn(f64) -> f64) -> (f64, f64) {
    let g = 0.5 * (5f64.sqrt() - 1.0);
    let (mut c, mut d) = (b - g * (b - a), a + g * (b - a));
    let (mut fc, mut fd) = (f(c), f(d));
    for _ in 0..SEARCH_STEPS {
        if fc > fd {
            (b, d, fd) = (d, c, fc);
            c = b - g * (b - a);
            fc = f(c);
        } else {
            (a, c, fc) = (c, d, fd);
            d = a + g * (b - a);
            fd = f(d);
        }
    }
    if fc > fd { (c, fc) } else { (d, fd) }
}

/// The input of carrier `f0` and width `width` that the line compresses most: the better of the
/// best linear chirp (its rate searched about −1/D within what the band holds) and the best
/// matched chirp (its scale searched from 0.2 to 2).
pub fn optimize(freqs: &[f64], mag: &[f64], phase: &[f64], f0: f64, width: f64) -> Result<(Input, Pulse)> {
    let line = Line::new(freqs, mag, phase)?;
    let (lo, hi) = line.band();
    let peak = |input: Input| pass(&line, input).map_or(0.0, |p| p.compression());
    // A linear chirp's band at most what the sweep holds about the carrier.
    let room = 2.0 * (f0 - lo).min(hi - f0).max(0.0);
    let reach = room / width;
    let (_, d) = line.delay_at(f0);
    let towards = if d > 0.0 { -1.0 } else { 1.0 };
    let (rate, linear) = golden(0.0, reach, |r| peak(Input { f0, width, law: Law::Linear { rate: towards * r } }));
    let (scale, matched) = golden(0.2, 2.0, |s| peak(Input { f0, width, law: Law::Matched { scale: s } }));
    let best = if matched > linear {
        Input { f0, width, law: Law::Matched { scale } }
    } else {
        Input { f0, width, law: Law::Linear { rate: towards * rate } }
    };
    Ok((best, pass(&line, best)?))
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

    /// A line whose delay curves, τ = τ₀ + Dν + Cν², ν = f − f₀: a linear chirp undoes only D,
    /// the matched chirp the whole curve, so it compresses more; the optimizer finds the
    /// matched one, its scale near 1 (stationary phase), and on a line of constant slope a linear
    /// chirp within a few per cent of the closed form's best.
    #[test]
    fn the_matched_chirp_undoes_a_curved_delay_and_the_optimizer_finds_it() {
        let (f0, tau0, d, c) = (10e9, 5e-9, 8e-18, 1e-26);
        let freqs: Vec<f64> = (0..=2000).map(|k| 9e9 + k as f64 * 1e6).collect();
        // φ = −2π ∫ τ dν.
        let phase: Vec<f64> = freqs.iter().map(|&f| f - f0).map(|nu| -2.0 * PI * (tau0 * nu + 0.5 * d * nu * nu + c * nu * nu * nu / 3.0)).collect();
        let mag = vec![1.0; freqs.len()];
        let width = 10e-9;
        let linear = through_phase(&freqs, &mag, &phase, Chirp { f0, width, rate: -1.0 / d }).map_or(0.0, |p| p.compression());
        let matched = through_phase(&freqs, &mag, &phase, Input { f0, width, law: Law::Matched { scale: 1.0 } }).map_or(0.0, |p| p.compression());
        assert!(matched > linear * 1.05, "matched {matched}, linear {linear}");
        let (best, p) = optimize(&freqs, &mag, &phase, f0, width).unwrap_or_else(|e| panic!("{e}"));
        assert!(p.compression() >= matched * 0.99, "{} against {matched}", p.compression());
        assert!(matches!(best.law, Law::Matched { scale } if (scale - 1.0).abs() < 0.3), "{best:?}");
        // The input's frequency sweeps down (the delay rises with f): high first.
        let swept: Vec<f64> = p.input_frequency.iter().copied().filter(|x| x.is_finite()).collect();
        assert!(swept.first() > swept.last(), "{:?} … {:?}", swept.first(), swept.last());
        // A straight slope: the best linear chirp, as the closed form's −1/D.
        let straight: Vec<f64> = freqs.iter().map(|&f| f - f0).map(|nu| -2.0 * PI * (tau0 * nu + 0.5 * d * nu * nu)).collect();
        let exact = through_phase(&freqs, &mag, &straight, Chirp { f0, width, rate: -1.0 / d }).map_or(0.0, |p| p.compression());
        let (_, q) = optimize(&freqs, &mag, &straight, f0, width).unwrap_or_else(|e| panic!("{e}"));
        assert!(q.compression() > 0.97 * exact, "{} against {exact}", q.compression());
    }

    #[test]
    fn a_delay_that_turns_back_leaves_no_tail_behind_the_matched_pulse() {
        // τ = τ₀ + a (ν − ν_m)²: falling below ν_m, rising above it; the carrier above.
        let (f0, tau0, a, nu_m) = (10e9, 5e-9, 2e-26, -0.3e9);
        let freqs: Vec<f64> = (0..=2000).map(|k| 9e9 + k as f64 * 1e6).collect();
        let phase: Vec<f64> =
            freqs.iter().map(|&f| f - f0).map(|nu| -2.0 * PI * (tau0 * nu + a * ((nu - nu_m).powi(3) - (-nu_m).powi(3)) / 3.0)).collect();
        let mag = vec![1.0; freqs.len()];
        let p = through_phase(&freqs, &mag, &phase, Input { f0, width: 20e-9, law: Law::Matched { scale: 1.0 } }).unwrap_or_else(|e| panic!("{e}"));
        // Its energy within 1.5 widths of the peak.
        let near = |p: &Pulse| {
            let all: f64 = p.output.iter().sum();
            let at: f64 = p.times.iter().zip(&p.output).filter(|(t, _)| (*t - p.output_time).abs() <= 1.5 * p.output_width).map(|(_, y)| y).sum();
            at / all
        };
        assert!(p.compression() > 3.0 && near(&p) > 0.8, "×{} with {} near the peak", p.compression(), near(&p));
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

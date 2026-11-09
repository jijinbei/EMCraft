//! `results.noise`: the noise figure of a passive two-port from its S-parameters.
//!
//! A passive network at one physical temperature T is in thermal equilibrium: its noise waves'
//! correlation matrix is kT(I − SSᴴ) (H. Hillbrand and P. H. Russer, "An efficient method for
//! computer aided noise analysis of linear amplifier networks", IEEE Trans. Circuits Syst.
//! CAS-23(4), 1976, pp. 235–238; R. Q. Twiss, J. Appl. Phys. 26, 1955, p. 599), so a two-port
//! cut from it (the other ports terminated in matched loads at T) has the noise temperature
//! Te = T(1 − G)/G and noise figure F = 1 + Te/T₀, G its available gain from the source
//! (D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012, §10.1). From a
//! matched source G = |S₂₁|²/(1 − |S₂₂|²); the least F is that of the conjugately matched
//! source, G = G_max = |S₂₁/S₁₂|(K − √(K² − 1)) (Pozar §12.2, the maximum gain of a conjugate match), K Rollett's
//! stability factor.

use emcraft_solver::Complex64 as C;
use serde_json::{Value, json};

use crate::network::{port_pair, source};
use crate::{EngineError, Params, Result, Session};

/// T₀ (K): the standard noise temperature of the noise figure.
pub const T0: f64 = 290.0;

/// The hottest network (K) a reading takes.
const MAX_TEMPERATURE: f64 = 1e4;

/// Gains below this (−200 dB) are taken as no transmission: the noise figure is unbounded.
const MIN_GAIN: f64 = 1e-20;

/// A two-port's S (port 1 the input).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TwoPort {
    pub s11: C,
    pub s12: C,
    pub s21: C,
    pub s22: C,
}

/// A passive two-port's noise at temperature T: its available gain and noise figure from a
/// matched source, and the least noise figure (the conjugately matched source's, `None` where
/// the network is not unconditionally stable or does not transmit backwards).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noise {
    pub gain: f64,
    pub figure: f64,
    pub temperature: f64,
    pub min_figure: Option<f64>,
}

impl TwoPort {
    /// Its noise at `temperature` (K); `None` where it transmits nothing or gains (not passive).
    ///
    /// ```
    /// use emcraft_engine::noise::{T0, TwoPort};
    /// use emcraft_solver::Complex64 as C;
    /// // A matched 3 dB attenuator at T₀: F = L = 2, Te = (L − 1)T₀.
    /// let t = std::f64::consts::FRAC_1_SQRT_2;
    /// let pad = TwoPort { s11: C::new(0.0, 0.0), s12: C::new(t, 0.0), s21: C::new(t, 0.0), s22: C::new(0.0, 0.0) };
    /// let n = pad.noise(T0).unwrap();
    /// assert!((n.figure - 2.0).abs() < 1e-12 && (n.temperature - T0).abs() < 1e-9);
    /// assert!((n.min_figure.unwrap() - 2.0).abs() < 1e-12);
    /// // A gain is not passive: refused.
    /// let amp = TwoPort { s21: C::new(2.0, 0.0), ..pad };
    /// assert!(amp.noise(T0).is_none());
    /// ```
    pub fn noise(&self, temperature: f64) -> Option<Noise> {
        let gain = self.s21.norm_sqr() / (1.0 - self.s22.norm_sqr());
        if !(gain.is_finite() && gain > MIN_GAIN && gain <= 1.0 + 1e-9) {
            return None;
        }
        let figure_of = |g: f64| 1.0 + temperature / T0 * (1.0 / g - 1.0);
        Some(Noise { gain, figure: figure_of(gain), temperature: temperature * (1.0 / gain - 1.0), min_figure: self.max_gain().map(figure_of) })
    }

    /// The maximum available gain |S₂₁/S₁₂|(K − √(K² − 1)), K = (1 − |S₁₁|² − |S₂₂|² + |Δ|²)/(2|S₁₂S₂₁|),
    /// Δ = S₁₁S₂₂ − S₁₂S₂₁: `None` unless K > 1 and |Δ| < 1 (unconditionally stable).
    fn max_gain(&self) -> Option<f64> {
        let delta = self.s11 * self.s22 - self.s12 * self.s21;
        let product = (self.s12 * self.s21).norm();
        if product < MIN_GAIN {
            return None;
        }
        let k: f64 = (1.0 - self.s11.norm_sqr() - self.s22.norm_sqr() + delta.norm_sqr()) / (2.0 * product);
        (k >= 1.0 && delta.norm() < 1.0).then(|| (self.s21 / self.s12).norm() * (k - (k * k - 1.0).max(0.0).sqrt())).filter(|g| *g > MIN_GAIN)
    }
}

/// A power ratio in dB.
fn db(x: f64) -> f64 {
    10.0 * x.log10()
}

/// `results.noise`.
pub fn noise(s: &mut Session, p: &Params) -> Result<Value> {
    let src = source(s, p)?;
    let d = &src.data;
    let (a, b) = port_pair(p, d)?;
    let temperature = p.positive("temperature").or(T0)?;
    if temperature > MAX_TEMPERATURE {
        return Err(p.error("temperature", format!("at most {MAX_TEMPERATURE} K")));
    }
    let points: Vec<Value> = (0..d.frequencies().len())
        .map(|k| {
            let f = s.show_frequency(d.frequencies()[k]);
            let get = |p: usize, q: usize| d.get(k, p, q).unwrap_or_default();
            let net = TwoPort { s11: get(a, a), s12: get(a, b), s21: get(b, a), s22: get(b, b) };
            match net.noise(temperature) {
                Some(n) => json!({
                    "frequency": f,
                    "gain_db": db(n.gain),
                    "nf_db": db(n.figure),
                    "noise_temperature": n.temperature,
                    "nf_min_db": n.min_figure.map(db),
                }),
                None => json!({"frequency": f, "why": "no transmission, or a gain: not a passive two-port"}),
            }
        })
        .collect();
    if points.is_empty() {
        return Err(EngineError::failed("the data has no frequencies"));
    }
    Ok(json!({"temperature": temperature, "ports": [a + 1, b + 1], "points": points}))
}

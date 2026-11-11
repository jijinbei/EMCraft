//! `results.noise`: the noise figure of a two-port — a passive one's from its S-parameters, an
//! active one's from its noise parameters — and the chain noise of `circuit.cascade`.
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
pub(crate) const MAX_TEMPERATURE: f64 = 1e4;

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

/// A two-port's noise as its chain (ABCD) form's sources at the input — a voltage v in series
/// and a current i across, before the noiseless network — by their correlations
/// `[⟨|v|²⟩, ⟨v i*⟩; ⟨i v*⟩, ⟨|i|²⟩]` over 4kT₀Δf (Ω, 1 and S): Hillbrand and Russer's chain
/// correlation matrix (IEEE Trans. CAS-23(4), 1976). Two-ports in a chain add as
/// `C = C₁ + A₁ C₂ A₁ᴴ`, A₁ the first's chain matrix; a source of impedance Z_s sees the noise
/// factor F = 1 + (C₁₁ + 2 Re(C₁₂ Z_s*) + C₂₂ |Z_s|²)/Re Z_s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChainNoise {
    pub vv: f64,
    pub vi: C,
    pub ii: f64,
}

/// A two-port's noise parameters: the least noise factor `f_min` (a ratio, not dB), the source
/// admittance giving it (S) and the noise resistance `rn` (Ω), F = F_min + (R_n/G_s)|Y_s − Y_opt|².
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoiseParameters {
    pub f_min: f64,
    pub y_opt: C,
    pub rn: f64,
}

impl ChainNoise {
    /// No noise: a lossless two-port's.
    pub const NONE: ChainNoise = ChainNoise { vv: 0.0, vi: C::new(0.0, 0.0), ii: 0.0 };

    /// A two-port's from its noise parameters: C₁₁ = R_n, C₁₂ = (F_min − 1)/2 − R_n Y_opt*,
    /// C₂₂ = R_n |Y_opt|² (Hillbrand and Russer, eq. 10).
    pub fn of(p: NoiseParameters) -> ChainNoise {
        ChainNoise { vv: p.rn, vi: C::new((p.f_min - 1.0) / 2.0, 0.0) - p.y_opt.conj() * p.rn, ii: p.rn * p.y_opt.norm_sqr() }
    }

    /// An impedance `z` in series at temperature `t` (K): its thermal voltage, 4kT Re z.
    pub fn series(z: C, t: f64) -> ChainNoise {
        ChainNoise { vv: t / T0 * z.re, ..ChainNoise::NONE }
    }

    /// An admittance `y` across the line at temperature `t` (K): its thermal current, 4kT Re y.
    pub fn shunt(y: C, t: f64) -> ChainNoise {
        ChainNoise { ii: t / T0 * y.re, ..ChainNoise::NONE }
    }

    /// A passive two-port's at temperature `t` (K) from its S on real references `z0` (Ω) and
    /// its chain matrix: in equilibrium its noise waves (b = S a + c) correlate as
    /// ⟨c cᴴ⟩ = kTΔf (I − S Sᴴ) (Twiss; S. W. Wedge and D. B. Rutledge, IEEE Trans. MTT 40(11),
    /// 1992), and with both ports matched the chain sources give c = M (v, i), so
    /// C = (T/4T₀) M⁻¹ (I − S Sᴴ) M⁻ᴴ. `None` where the waves do not fix the sources (M
    /// singular), or the network gains (I − S Sᴴ not positive: not passive).
    pub fn passive(s: &TwoPort, z0: [f64; 2], chain: [C; 4], t: f64) -> Option<ChainNoise> {
        let [a11, a12, a21, a22] = chain;
        let (p, q) = (a11 * z0[1] + a12, a21 * z0[1] + a22);
        let d = p + q * z0[0];
        if d.norm() == 0.0 {
            return None;
        }
        // c₁ = −√z₀₁ I₁ and c₂ = −√z₀₂ I₂ with I₂ = −(v + z₀₁ i)/D, I₁ = −Q I₂ − i.
        let (r1, r2) = (z0[0].sqrt(), z0[1].sqrt());
        let m = [-(q / d) * r1, -(q * z0[0] / d - 1.0) * r1, ONE / d * r2, ONE / d * z0[0] * r2];
        let det = m[0] * m[3] - m[1] * m[2];
        if det.norm() == 0.0 {
            return None;
        }
        let inv = [m[3] / det, -m[1] / det, -m[2] / det, m[0] / det];
        let w = |a: C, b: C, c: C, d: C| a * b.conj() + c * d.conj();
        let k = [
            C::new(1.0 - s.s11.norm_sqr() - s.s12.norm_sqr(), 0.0),
            -w(s.s11, s.s21, s.s12, s.s22),
            -w(s.s21, s.s11, s.s22, s.s12),
            C::new(1.0 - s.s21.norm_sqr() - s.s22.norm_sqr(), 0.0),
        ];
        if k[0].re < -1e-9 || k[3].re < -1e-9 || (k[0] * k[3] - k[1] * k[2]).re < -1e-9 {
            return None;
        }
        let x = hermitian(inv, k);
        let f = t / (4.0 * T0);
        Some(ChainNoise { vv: f * x[0].re, vi: x[1] * f, ii: f * x[3].re })
    }

    /// This two-port, of chain matrix `a`, followed by `next`: C₁ + A₁ C₂ A₁ᴴ.
    pub fn then(self, a: [C; 4], next: ChainNoise) -> ChainNoise {
        let x = hermitian(a, next.matrix());
        ChainNoise { vv: self.vv + x[0].re, vi: self.vi + x[1], ii: self.ii + x[3].re }
    }

    fn matrix(&self) -> [C; 4] {
        [C::new(self.vv, 0.0), self.vi, self.vi.conj(), C::new(self.ii, 0.0)]
    }

    /// The noise factor F (a ratio) from a source of impedance `zs` (Re > 0).
    pub fn figure(&self, zs: C) -> f64 {
        1.0 + (self.vv + 2.0 * (self.vi * zs.conj()).re + self.ii * zs.norm_sqr()) / zs.re
    }

    /// Its noise parameters (Hillbrand and Russer, eq. 11): R_n = C₁₁, B_opt = Im C₁₂/R_n,
    /// G_opt = √(C₂₂/R_n − B_opt²), F_min = 1 + 2(Re C₁₂ + R_n G_opt); `None` without a noise
    /// voltage (R_n = 0: no finite best source).
    pub fn parameters(&self) -> Option<NoiseParameters> {
        let rn = self.vv;
        if !(rn > 0.0) {
            return None;
        }
        let b = self.vi.im / rn;
        let g = (self.ii / rn - b * b).max(0.0).sqrt();
        Some(NoiseParameters { f_min: 1.0 + 2.0 * (self.vi.re + rn * g), y_opt: C::new(g, b), rn })
    }
}

/// X K Xᴴ for 2 × 2 matrices, row-major.
fn hermitian(x: [C; 4], k: [C; 4]) -> [C; 4] {
    let xk = [x[0] * k[0] + x[1] * k[2], x[0] * k[1] + x[1] * k[3], x[2] * k[0] + x[3] * k[2], x[2] * k[1] + x[3] * k[3]];
    let h = [x[0].conj(), x[2].conj(), x[1].conj(), x[3].conj()];
    [xk[0] * h[0] + xk[1] * h[2], xk[0] * h[1] + xk[1] * h[3], xk[2] * h[0] + xk[3] * h[2], xk[2] * h[1] + xk[3] * h[3]]
}

const ONE: C = C::new(1.0, 0.0);

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
    // A measured two-port's own noise, read from port 1 to port 2 on port 1's reference.
    let measured = (!src.noise.is_empty() && (a, b) == (0, 1)).then(|| (&src.noise, C::new(src.references[0].unwrap_or(50.0), 0.0)));
    let points: Vec<Value> = (0..d.frequencies().len())
        .map(|k| {
            let hz = d.frequencies()[k];
            let f = s.show_frequency(hz);
            let get = |p: usize, q: usize| d.get(k, p, q).unwrap_or_default();
            let net = TwoPort { s11: get(a, a), s12: get(a, b), s21: get(b, a), s22: get(b, b) };
            if let Some((points, zs)) = measured {
                let gain = net.s21.norm_sqr() / (1.0 - net.s22.norm_sqr());
                return match crate::network::noise_at(points, hz) {
                    Some(n) if gain.is_finite() && gain > 0.0 => {
                        let figure = n.figure(zs);
                        json!({
                            "frequency": f,
                            "gain_db": db(gain),
                            "nf_db": db(figure),
                            "noise_temperature": T0 * (figure - 1.0),
                            "nf_min_db": n.parameters().map(|q| db(q.f_min)),
                        })
                    }
                    Some(_) => json!({"frequency": f, "why": "no available gain from a matched source"}),
                    None => json!({"frequency": f, "why": "outside the noise data's frequencies"}),
                };
            }
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
    Ok(
        json!({"temperature": temperature, "ports": [a + 1, b + 1], "from": if measured.is_some() { "noise data" } else { "passive" }, "points": points}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    const Z0: f64 = 50.0;

    fn c(re: f64, im: f64) -> C {
        C::new(re, im)
    }

    /// A two-port's S on Z0 at both ports from its chain matrix (Pozar Table 4.2).
    fn s_of([a, b, cc, d]: [C; 4]) -> TwoPort {
        let delta = a + b / Z0 + cc * Z0 + d;
        let det = a * d - b * cc;
        TwoPort { s11: (a + b / Z0 - cc * Z0 - d) / delta, s12: 2.0 * det / delta, s21: 2.0 / delta, s22: (-a + b / Z0 - cc * Z0 + d) / delta }
    }

    fn times(x: [C; 4], y: [C; 4]) -> [C; 4] {
        [x[0] * y[0] + x[1] * y[2], x[0] * y[1] + x[1] * y[3], x[2] * y[0] + x[3] * y[2], x[2] * y[1] + x[3] * y[3]]
    }

    #[test]
    fn a_resistors_noise_from_its_waves_is_its_thermal_voltage_or_current() {
        // 4kTR in series, 4kTG across (Nyquist): from S alone, by the waves' equilibrium.
        let z = c(25.0, 10.0);
        let a = [ONE, z, C::new(0.0, 0.0), ONE];
        let n = ChainNoise::passive(&s_of(a), [Z0, Z0], a, 2.0 * T0).unwrap();
        assert_abs_diff_eq!(n.vv, 50.0, epsilon = 1e-9);
        assert!(n.vi.norm() < 1e-9 && n.ii.abs() < 1e-12, "{n:?}");
        let y = c(0.01, -0.02);
        let a = [ONE, C::new(0.0, 0.0), y, ONE];
        let n = ChainNoise::passive(&s_of(a), [Z0, Z0], a, T0).unwrap();
        assert_abs_diff_eq!(n.ii, 0.01, epsilon = 1e-12);
        assert!(n.vv.abs() < 1e-9 && n.vi.norm() < 1e-9, "{n:?}");
        // A gain is not passive.
        let amp = TwoPort { s11: c(0.0, 0.0), s12: c(0.0, 0.0), s21: c(3.0, 0.0), s22: c(0.0, 0.0) };
        assert!(ChainNoise::passive(&amp, [Z0, Z0], [c(1.0 / 3.0, 0.0), c(0.0, 0.0), c(0.0, 0.0), c(1.0 / 3.0, 0.0)], T0).is_none());
    }

    #[test]
    fn a_resistive_pad_has_the_noise_figure_of_its_loss_whichever_way_it_is_read() {
        // A matched 6 dB π pad on 50 Ω (shunt 150.48 Ω, series 37.35 Ω, shunt 150.48 Ω): at T₀
        // F = L (Pozar §10.1), from its resistors' own noise chained, or from its S.
        let (g, r) = (1.0 / 150.476_5, 37.352_5);
        let shunt = [ONE, c(0.0, 0.0), c(g, 0.0), ONE];
        let series = [ONE, c(r, 0.0), c(0.0, 0.0), ONE];
        let pad = times(times(shunt, series), shunt);
        let chained = ChainNoise::shunt(c(g, 0.0), T0)
            .then(shunt, ChainNoise::series(c(r, 0.0), T0))
            .then(times(shunt, series), ChainNoise::shunt(c(g, 0.0), T0));
        let s = s_of(pad);
        let loss = 1.0 / s.s21.norm_sqr();
        assert_abs_diff_eq!(10.0 * loss.log10(), 6.0, epsilon = 1e-4);
        let waves = ChainNoise::passive(&s, [Z0, Z0], pad, T0).unwrap();
        for n in [chained, waves] {
            assert_abs_diff_eq!(n.figure(c(Z0, 0.0)), loss, epsilon = 1e-9);
        }
        assert_abs_diff_eq!(chained.vv, waves.vv, epsilon = 1e-9);
        assert!((chained.vi - waves.vi).norm() < 1e-9);
        assert_abs_diff_eq!(chained.ii, waves.ii, epsilon = 1e-12);
    }

    #[test]
    fn an_amplifiers_parameters_come_back_and_friis_holds_after_a_pad() {
        let p = NoiseParameters { f_min: 1.5, y_opt: c(0.012, -0.004), rn: 8.0 };
        let n = ChainNoise::of(p);
        let back = n.parameters().unwrap();
        assert_abs_diff_eq!(back.f_min, p.f_min, epsilon = 1e-12);
        assert!((back.y_opt - p.y_opt).norm() < 1e-12 && (back.rn - p.rn).abs() < 1e-12);
        // F = F_min + (R_n/G_s)|Y_s − Y_opt|² at any source.
        let ys = c(0.03, 0.01);
        assert_abs_diff_eq!(n.figure(ONE / ys), p.f_min + p.rn / ys.re * (ys - p.y_opt).norm_sqr(), epsilon = 1e-12);
        assert_abs_diff_eq!(n.figure(ONE / p.y_opt), p.f_min, epsilon = 1e-12);
        // A matched pad of loss L, then an amplifier whose best source is 50 Ω: F = L·F₂ (Friis).
        let amp = ChainNoise::of(NoiseParameters { f_min: 1.4, y_opt: c(1.0 / Z0, 0.0), rn: 12.0 });
        let t = 0.5f64.sqrt();
        let pad = TwoPort { s11: c(0.0, 0.0), s12: c(t, 0.0), s21: c(t, 0.0), s22: c(0.0, 0.0) };
        // Its chain matrix: A = D = (1 + t²)/2t, B = Z₀(1 − t²)/2t, C = (1 − t²)/(2t Z₀).
        let (ad, bc) = ((1.0 + t * t) / (2.0 * t), (1.0 - t * t) / (2.0 * t));
        let a = [c(ad, 0.0), c(Z0 * bc, 0.0), c(bc / Z0, 0.0), c(ad, 0.0)];
        let s = s_of(a);
        assert!((s.s21 - pad.s21).norm() < 1e-12 && s.s11.norm() < 1e-12, "{s:?}");
        let chain = ChainNoise::passive(&s, [Z0, Z0], a, T0).unwrap().then(a, amp);
        assert_abs_diff_eq!(chain.figure(c(Z0, 0.0)), 2.0 * 1.4, epsilon = 1e-9);
    }
}

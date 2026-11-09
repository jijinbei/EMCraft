//! Coupled-resonator filters by their coupling matrix ([`emcraft_solver::coupling`]):
//! `filter.synthesize` makes one from a generalized Chebyshev response, `results.coupling`
//! extracts one from the sweep's or a measurement's S (by |S| alone, or by the Cauchy method). A band-pass filter's frequency f maps
//! to the low-pass prototype's ω = (f₀/Δf)(f/f₀ − f₀/f), f₀ its centre and Δf its bandwidth.

use std::sync::Arc;

use emcraft_solver::Complex64 as C;
use emcraft_solver::coupling::{self, Characteristic, Coupling};
use serde_json::{Value, json};

use crate::network::{Measured, keep, kept_name, source};
use crate::sdata::SData;
use crate::{EngineError, Params, Result, Session};

/// The frequencies a kept response is drawn at.
const RESPONSE_POINTS: usize = 401;

emcraft_macros::named_enum! {
    /// How `results.coupling` fits the data.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Method {
        /// |S11|²/|S21|² = ε²F²/P², phase-free: the ports' feed lines need no removal.
        Magnitude = "magnitude",
        /// The Cauchy method on complex S, the ports' phase loading removed first.
        Cauchy = "cauchy",
        /// The Cauchy method on S as it is (its reference planes at the first and last resonators).
        CauchyPlain = "cauchy_plain",
    }
}

/// The band-pass mapping of a filter centred at `f0` with bandwidth `bw` (Hz), of the guide
/// frequency f′ = √(f² − f_c²) when its resonators are guides above a cut-off `fc` (the
/// guide-wavelength mapping: their phase goes as β ∝ f′, not f).
#[derive(Clone, Copy, Debug)]
struct Band {
    f0: f64,
    bw: f64,
    fc: f64,
}

impl Band {
    /// The centre and bandwidth from `center` and `bandwidth` (display units), and `cutoff`
    /// (none by default); the band inside positive frequencies, above the cut-off.
    fn read(p: &Params) -> Result<Band> {
        let f0 = p.frequency("center").required()?;
        let bw = p.frequency("bandwidth").required()?;
        let fc = p.frequency("cutoff").optional()?.unwrap_or(0.0);
        if !(f0 > 0.0 && bw > 0.0 && bw < 2.0 * f0) {
            return Err(p.error("bandwidth", "positive and less than twice the centre"));
        }
        if !(fc >= 0.0 && fc < Band::edges(f0, bw).0) {
            return Err(p.error("cutoff", "below the band"));
        }
        Ok(Band { f0, bw, fc })
    }

    /// The band edges f₁ < f₂ with f₁ f₂ = f₀² and f₂ − f₁ = Δf.
    fn edges(f0: f64, bw: f64) -> (f64, f64) {
        let f2 = 0.5 * (bw + (bw * bw + 4.0 * f0 * f0).sqrt());
        (f2 - bw, f2)
    }

    /// The guide frequency √(f² − f_c²).
    fn guide(self, f: f64) -> f64 {
        (f * f - self.fc * self.fc).max(0.0).sqrt()
    }

    /// The mapping's own centre and bandwidth: the edges' guide frequencies'.
    fn mapped(self) -> (f64, f64) {
        let (f1, f2) = Band::edges(self.f0, self.bw);
        let (g1, g2) = (self.guide(f1), self.guide(f2));
        ((g1 * g2).sqrt(), g2 - g1)
    }

    /// The low-pass ω of `f` (Hz).
    fn omega(self, f: f64) -> f64 {
        let (g0, gw) = self.mapped();
        let g = self.guide(f);
        if g <= 0.0 {
            return f64::NEG_INFINITY;
        }
        (g0 / gw) * (g / g0 - g0 / g)
    }

    /// The frequency (Hz) of low-pass `w`: the guide frequency the positive root of
    /// g² − (w Δg) g − g₀² = 0, then f = √(g² + f_c²).
    fn frequency(self, w: f64) -> f64 {
        let (g0, gw) = self.mapped();
        let b = w * gw;
        let g = 0.5 * (b + (b * b + 4.0 * g0 * g0).sqrt());
        (g * g + self.fc * self.fc).sqrt()
    }
}

/// A matrix with its rows named S, 1, …, N, L, for replies.
fn matrix(m: &Coupling) -> Value {
    let n = m.size();
    Value::Array((0..n).map(|i| Value::Array((0..n).map(|j| json!(m.get(i, j))).collect())).collect())
}

/// The rows' names: S, 1, …, N, L.
fn names(order: usize) -> Vec<String> {
    std::iter::once("S".to_string()).chain((1..=order).map(|k| k.to_string())).chain(std::iter::once("L".to_string())).collect()
}

/// The response of `m` over `band` ± 1.5 Δf as a two-port's S (S22 by reciprocity and the
/// lossless matrix: the response's own), kept with the measurements as `name`.
fn keep_response(s: &mut Session, m: &Coupling, band: Band, name: String, what: &str) -> Result<()> {
    let ws: Vec<f64> = (0..RESPONSE_POINTS).map(|k| -3.0 + 6.0 * k as f64 / (RESPONSE_POINTS - 1) as f64).collect();
    let mut freqs = Vec::with_capacity(ws.len());
    let mut values = Vec::with_capacity(4 * ws.len());
    for w in ws {
        let Some((s11, s21)) = m.s(w) else { continue };
        // S22: the mirrored matrix's S11.
        let s22 = mirrored(m).s(w).map_or(s11, |x| x.0);
        freqs.push(band.frequency(w));
        values.extend([s11, s21, s21, s22]);
    }
    let data = SData::square(freqs, 2, values).map_err(EngineError::failed)?;
    keep(s, Measured { name, file: what.to_string(), references: vec![50.0, 50.0], data: Arc::new(data) })
}

/// The matrix seen from the load: rows and columns reversed.
fn mirrored(m: &Coupling) -> Coupling {
    let n = m.size();
    Coupling { order: m.order, m: (0..n * n).map(|k| m.get(n - 1 - k / n, n - 1 - k % n)).collect() }
}

/// What a characteristic and its matrix are, for replies.
fn described(c: &Characteristic, m: &Coupling) -> Value {
    json!({
        "order": c.order,
        "names": names(c.order),
        "matrix": matrix(m),
        "reflection_zeros": c.reflection_zeros,
        "transmission_zeros": c.transmission_zeros,
        "eps": c.eps,
        "eps_r": c.eps_r,
    })
}

/// The transmission zeros `zeros` gives (low-pass ω, a list of numbers).
fn zeros_of(p: &Params, order: usize) -> Result<Vec<f64>> {
    let list = p.list("zeros", coupling::MAX_ORDER).or(&[])?;
    let zeros: Vec<f64> = list.iter().map(|v| p.number_of("zeros", v)).collect::<Result<_>>()?;
    if zeros.len() > order {
        return Err(p.error("zeros", format!("at most {order} transmission zeros")));
    }
    Ok(zeros)
}

/// `filter.synthesize`: the folded coupling matrix of a generalized Chebyshev filter; with a
/// centre and bandwidth, its response kept as a measurement drawn over the S-parameter chart.
pub fn synthesize(s: &mut Session, p: &Params) -> Result<Value> {
    let order = p.integer("order", 1..=coupling::MAX_ORDER).required()?;
    let rl = p.positive("return_loss").or(20.0)?;
    let zeros = zeros_of(p, order)?;
    let c = coupling::chebyshev(order, rl, &zeros).map_err(EngineError::failed)?;
    let m = coupling::synthesize(&c).map_err(EngineError::failed)?;
    let mut reply = described(&c, &m);
    if p.has("center") || p.has("bandwidth") {
        let band = Band::read(p)?;
        let name = kept_name(p, "prototype")?;
        keep_response(s, &m, band, name.clone(), "(filter.synthesize)")?;
        if let Some(o) = reply.as_object_mut() {
            o.insert("kept".into(), json!(name));
        }
    }
    Ok(reply)
}

/// `results.coupling`: the folded coupling matrix whose response best fits the sweep's (or a
/// measurement's) S11 and S21 (`method`); its response kept as a measurement drawn over the
/// chart.
pub fn extract(s: &mut Session, p: &Params) -> Result<Value> {
    let order = p.integer("order", 1..=coupling::MAX_ORDER).required()?;
    let nz = p.integer("zeros", 0..=coupling::MAX_ORDER).or(0)?;
    if nz > order {
        return Err(p.error("zeros", format!("at most {order}, the order")));
    }
    let band = Band::read(p)?;
    let method = p.pick("method").or(Method::Magnitude)?;
    let src = source(s, p)?;
    let d = &src.data;
    if d.outputs() != 2 || d.inputs().len() != 2 {
        return Err(EngineError::failed(format!("a two-port with both ports excited; this has {} ports", d.outputs())));
    }
    // The frequencies fitted: those from `start` to `stop` when given.
    let lo = p.frequency("start").optional()?.unwrap_or(0.0);
    let hi = p.frequency("stop").optional()?.unwrap_or(f64::INFINITY);
    let picked: Vec<usize> =
        (0..d.frequencies().len()).filter(|k| (lo..=hi).contains(&d.frequencies()[*k]) && d.frequencies()[*k] > band.fc).collect();
    let omega: Vec<f64> = picked.iter().map(|k| band.omega(d.frequencies()[*k])).collect();
    let get = |pq: (usize, usize)| picked.iter().map(|k| d.get(*k, pq.0, pq.1).unwrap_or_default()).collect::<Vec<C>>();
    let (s11, s21, s22) = (get((0, 0)), get((1, 0)), get((1, 1)));
    if omega.iter().filter(|w| w.abs() <= 1.0).count() < order {
        return Err(p.error("bandwidth", format!("fewer than {order} frequencies inside the passband: solve more finely, or check the band")));
    }
    let (c, loading) = match method {
        Method::Magnitude => (coupling::fit_magnitude(&omega, &s11, &s21, order, nz).map_err(EngineError::failed)?, None),
        Method::Cauchy => {
            let x = coupling::fit_loaded(&omega, &s11, &s21, &s22, order, nz).map_err(EngineError::failed)?;
            (x.characteristic, Some(x.loading))
        }
        Method::CauchyPlain => (coupling::fit(&omega, &s11, &s21, order, nz).map_err(EngineError::failed)?, None),
    };
    let m = coupling::synthesize(&c).map_err(EngineError::failed)?;
    // How well the model's |S| follows the data's.
    let mut e11 = 0.0;
    let mut e21 = 0.0;
    for (k, w) in omega.iter().enumerate() {
        let (a, b) = c.s(*w);
        e11 += (a.norm() - s11[k].norm()).powi(2);
        e21 += (b.norm() - s21[k].norm()).powi(2);
    }
    let n = omega.len().max(1) as f64;
    let name = kept_name(p, "coupling model")?;
    keep_response(s, &m, band, name.clone(), "(results.coupling)")?;
    let mut reply = described(&c, &m);
    if let Some(o) = reply.as_object_mut() {
        o.insert("rms_s11".into(), json!((e11 / n).sqrt()));
        o.insert("rms_s21".into(), json!((e21 / n).sqrt()));
        o.insert("loading".into(), json!(loading.map(|l| l.map(|x| json!({"offset": x.a, "slope": x.b})))));
        o.insert("kept".into(), json!(name));
        o.insert("frequencies".into(), json!(omega.len()));
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_band_maps_to_the_prototype_and_back() {
        let b = Band { f0: 10e9, bw: 0.5e9, fc: 0.0 };
        assert!(b.omega(10e9).abs() < 1e-12);
        for w in [-3.0, -1.0, 0.3, 1.0, 2.5] {
            assert!((b.omega(b.frequency(w)) - w).abs() < 1e-9);
        }
        // The band edges are the geometric band's: f₁ f₂ = f₀², f₂ − f₁ = Δf.
        let (f1, f2) = (b.frequency(-1.0), b.frequency(1.0));
        assert!((f1 * f2 - 1e20).abs() < 1e6 && (f2 - f1 - 0.5e9).abs() < 1.0);
        // Above a cut-off: the edges still at ω = ±1, the mapping the guide frequency's.
        let g = Band { fc: 6.557e9, ..b };
        let (e1, e2) = Band::edges(10e9, 0.5e9);
        assert!((g.omega(e1) + 1.0).abs() < 1e-9 && (g.omega(e2) - 1.0).abs() < 1e-9);
        for w in [-2.0, 0.4, 2.0] {
            assert!((g.omega(g.frequency(w)) - w).abs() < 1e-9);
        }
    }
}

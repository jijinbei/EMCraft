//! Circuits round the solved S-parameters: `circuit.cascade` chains two-ports — the sweep, a
//! measurement, lumped elements in series or shunt, transmission lines — by their chain (ABCD)
//! matrices; `results.equivalent` fits lumped RLC branches to a one-port's impedance or a
//! two-port's π or T network (Pozar, Microwave Engineering, 4th ed., §4.4, Table 4.1–4.2).
//! Results are kept with the measurements, drawn over the S-parameter chart.

use std::sync::Arc;

use emcraft_solver::{Complex64 as C, SMatrix};
use serde_json::{Value, json};

use crate::network::{Measured, NoisePoint, keep, kept_name, source};
use crate::noise::{Budget, MAX_TEMPERATURE, T0};
use crate::sdata::SData;
use crate::system::{Block, s_of_chain};
use crate::{EngineError, Params, Result, Session};

/// The most blocks a cascade chains.
const MAX_BLOCKS: usize = 32;

const ONE: C = C::new(1.0, 0.0);

/// The blocks a cascade's `blocks` lists ([`crate::system::part`]'s settings each), and the
/// frequencies the first S block gives.
fn blocks(s: &Session, p: &Params, z0: f64) -> Result<(Vec<Block>, Option<Vec<f64>>)> {
    let items = p.items("blocks", "block", MAX_BLOCKS)?;
    if items.is_empty() {
        return Err(p.error("blocks", "at least one block"));
    }
    let list = items.iter().map(|b| Block::of(s, &crate::system::part(s, b)?, z0)).collect::<Result<Vec<_>>>()?;
    let freqs = list.iter().find_map(Block::frequencies).map(<[f64]>::to_vec);
    Ok((list, freqs))
}

/// `circuit.cascade`: the two-ports of `blocks` in a chain, port 2 of each to port 1 of the
/// next, its S on `impedance` at each frequency of its first S block (or `start` to `stop` in
/// `points`), kept as a measurement named `name`.
pub fn cascade(s: &mut Session, p: &Params) -> Result<Value> {
    let z0 = p.positive("impedance").or(50.0)?;
    let (list, freqs) = blocks(s, p, z0)?;
    let freqs = match freqs {
        Some(f) => f,
        None => {
            let (a, b) = (p.frequency("start").required()?, p.frequency("stop").required()?);
            let n = p.integer("points", 2..=crate::system::MAX_POINTS).or(201)?;
            if !(a > 0.0 && b > a) {
                return Err(p.error("stop", "above `start`, both positive"));
            }
            crate::numbers::evenly_spaced(a, b, n)
        }
    };
    let temperature = p.positive("temperature").or(T0)?;
    if temperature > MAX_TEMPERATURE {
        return Err(p.error("temperature", format!("at most {MAX_TEMPERATURE} K")));
    }
    let mut values = Vec::with_capacity(4 * freqs.len());
    let mut noise: Vec<NoisePoint> = Vec::new();
    let mut why: Option<String> = None;
    for f in &freqs {
        let Some(last) = crate::system::walk(&list, *f, temperature)?.pop() else {
            return Err(p.error("blocks", "at least one block"));
        };
        let [s11, s12, s21, s22] = s_of_chain(last.chain, z0).ok_or_else(|| EngineError::failed("the chain has no S on this impedance"))?;
        values.extend([s11, s12, s21, s22]);
        match last.noise {
            Ok(n) => noise.push(NoisePoint { frequency: *f, noise: n }),
            Err(e) => {
                why.get_or_insert(e);
            }
        }
    }
    let data = SData::square(freqs, 2, values).map_err(EngineError::failed)?;
    let name = kept_name(p, "cascade")?;
    // The noise figure from the source (the antenna's impedance, or `impedance`) where both
    // are known.
    let antenna = crate::network::source_impedance(s, p)?;
    let budget = Budget::read(p)?;
    let figures: Vec<(f64, f64)> = noise
        .iter()
        .filter_map(|x| {
            let zs = match &antenna {
                Some(a) => a.at(x.frequency)?,
                None => C::new(z0, 0.0),
            };
            Some((x.frequency, x.noise.figure(zs)))
        })
        .collect();
    let db = |f: f64| 10.0 * f.log10();
    let least = figures.iter().copied().min_by(|a, b| a.1.total_cmp(&b.1));
    let most = figures.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1));
    let reply = json!({
        "name": name,
        "blocks": list.len(),
        "frequencies": data.frequencies().len(),
        "impedance": z0,
        "noise": least.zip(most).map(|(l, m)| json!({
            "temperature": temperature,
            "frequencies": figures.len(),
            "source": antenna.as_ref().map(|a| a.name.clone()),
            "nf_db_least": db(l.1),
            "at": s.show_frequency(l.0),
            "nf_db_most": db(m.1),
            "sensitivity_dbm_best": budget.map(|b| b.reading(l.1).sensitivity_dbm),
            "sensitivity_dbm_worst": budget.map(|b| b.reading(m.1).sensitivity_dbm),
        })),
        "noise_why": why,
    });
    keep(s, Measured { name, file: "(circuit.cascade)".into(), references: vec![z0, z0], data: Arc::new(data), noise })?;
    Ok(reply)
}

/// A branch's fitted parts (SI), how they are arranged and how far the fit is off.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fitted {
    /// R (Ω) of a series branch, or G (S) of a parallel one.
    loss: f64,
    /// L (H) and C (F); none where the fit has no such part.
    l: Option<f64>,
    c: Option<f64>,
    parallel: bool,
    /// RMS |fit − data| over RMS |data|.
    error: f64,
}

impl Fitted {
    /// The branch's impedance (series) or admittance (parallel) at angular frequency `w`.
    fn value(&self, w: f64) -> C {
        let (l, c) = (self.l, self.c);
        if self.parallel {
            C::new(self.loss, w * c.unwrap_or(0.0) - l.map_or(0.0, |l| 1.0 / (w * l)))
        } else {
            C::new(self.loss, w * l.unwrap_or(0.0) - c.map_or(0.0, |c| 1.0 / (w * c)))
        }
    }

    fn json(&self, branch: &str) -> Value {
        let (r, g) = if self.parallel { (None, Some(self.loss)) } else { (Some(self.loss), None) };
        json!({
            "branch": branch,
            "arrangement": if self.parallel { "parallel" } else { "series" },
            "resistance": r,
            "conductance": g,
            "inductance_nh": self.l.map(|l| l * 1e9),
            "capacitance_pf": self.c.map(|c| c * 1e12),
            "relative_error": self.error,
        })
    }
}

/// The parts of R + j(ωL − 1/(ωC)) (series) or G + j(ωC − 1/(ωL)) (parallel) that fit `v` at
/// angular frequencies `w` best by least squares: the loss its mean real part, the reactive
/// parts ≥ 0 (the best of both, either one or neither). A part whose contribution stays below
/// 10⁻⁹ of `scale` (z₀ for a series branch, 1/z₀ for a parallel one) over the band is round-off
/// and left out.
fn fit_branch(w: &[f64], v: &[C], parallel: bool, scale: f64) -> Option<Fitted> {
    let n = w.len();
    if n == 0 || v.len() != n {
        return None;
    }
    let loss = v.iter().map(|x| x.re).sum::<f64>() / n as f64;
    // x = a ω + b (−1/ω): series a = L, b = 1/C; parallel a = C, b = 1/L.
    let (u1, u2): (Vec<f64>, Vec<f64>) = w.iter().map(|x| (*x, -1.0 / x)).unzip();
    let y: Vec<f64> = v.iter().map(|x| x.im).collect();
    let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let mut candidates: Vec<(f64, f64)> = vec![(0.0, 0.0)];
    let (a11, a22, a12) = (dot(&u1, &u1), dot(&u2, &u2), dot(&u1, &u2));
    let (b1, b2) = (dot(&u1, &y), dot(&u2, &y));
    if a11 > 0.0 {
        candidates.push((b1 / a11, 0.0));
    }
    if a22 > 0.0 {
        candidates.push((0.0, b2 / a22));
    }
    let det = a11 * a22 - a12 * a12;
    if det.abs() > 1e-12 * a11 * a22 {
        candidates.push(((b1 * a22 - b2 * a12) / det, (a11 * b2 - a12 * b1) / det));
    }
    let residual = |(a, b): (f64, f64)| y.iter().zip(&u1).zip(&u2).map(|((y, p), q)| (y - a * p - b * q).powi(2)).sum::<f64>();
    let (a, b) = candidates
        .into_iter()
        .filter(|(a, b)| *a >= 0.0 && *b >= 0.0 && a.is_finite() && b.is_finite())
        .min_by(|x, y| residual(*x).total_cmp(&residual(*y)))?;
    let tiny = 1e-9 * scale;
    let top = w.iter().fold(0.0f64, |m, x| m.max(*x));
    let bottom = w.iter().fold(f64::INFINITY, |m, x| m.min(*x));
    let (a, b) = (if a * top < tiny { 0.0 } else { a }, if b / bottom < tiny { 0.0 } else { b });
    let loss = if loss.abs() < tiny { 0.0 } else { loss };
    let positive = |x: f64| (x > 0.0).then_some(x);
    let (l, c) = if parallel { (positive(b).map(|b| 1.0 / b), positive(a)) } else { (positive(a), positive(b).map(|b| 1.0 / b)) };
    let mut f = Fitted { loss, l, c, parallel, error: 0.0 };
    let norm = v.iter().map(C::norm_sqr).sum::<f64>().sqrt();
    let diff = w.iter().zip(v).map(|(x, y)| (f.value(*x) - y).norm_sqr()).sum::<f64>().sqrt();
    f.error = if norm > tiny * (n as f64).sqrt() { diff / norm } else { diff / scale };
    Some(f)
}

emcraft_macros::named_enum! {
    /// The network `results.equivalent` fits.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Topology {
        /// A one-port's impedance as R, L and C in series.
        Series = "series",
        /// A one-port's admittance as G, L and C in parallel.
        Shunt = "shunt",
        /// A two-port as a π: shunt branches at each port (parallel), a series branch between.
        Pi = "pi",
        /// A two-port as a T: series arms at each port, a shunt branch between.
        T = "t",
    }
}

/// `results.equivalent`: lumped branches fitted to the sweep's (or a measurement's) network.
pub fn equivalent(s: &mut Session, p: &Params) -> Result<Value> {
    let src = source(s, p)?;
    let d = &src.data;
    let n = d.outputs();
    let topology = p.pick("topology").or(if n == 1 { Topology::Series } else { Topology::Pi })?;
    let given = p.positive("impedance").optional()?;
    let z0: Vec<f64> = src.references.iter().map(|r| r.or(given).unwrap_or(50.0)).collect();
    let w: Vec<f64> = d.frequencies().iter().map(|f| 2.0 * std::f64::consts::PI * f).collect();
    let mats = |k: usize| SMatrix::from_fn(n, |i, j| d.at(k)[i * n + j]);
    let fail = |what: &str| EngineError::failed(format!("no {what} at some frequency: the network is open or shorted there"));
    let (branches, model): (Vec<(String, Fitted)>, Vec<C>) = match topology {
        Topology::Series | Topology::Shunt => {
            let port = p.integer("port", 1..=n).or(1)? - 1;
            if d.inputs().iter().all(|q| *q != port) {
                return Err(p.error("port", "was not excited"));
            }
            let z: Vec<C> = (0..w.len())
                .map(|k| {
                    let g = d.get(k, port, port).unwrap_or_default();
                    (ONE + g) / (ONE - g) * z0[port]
                })
                .collect();
            let parallel = topology == Topology::Shunt;
            let v: Vec<C> = if parallel { z.iter().map(|x| ONE / x).collect() } else { z };
            let f = fit_branch(&w, &v, parallel, if parallel { 1.0 / z0[port] } else { z0[port] }).ok_or_else(|| fail("impedance"))?;
            let model = w
                .iter()
                .map(|x| {
                    let y = f.value(*x);
                    let zz = if parallel { ONE / y } else { y };
                    (zz - z0[port]) / (zz + z0[port])
                })
                .collect();
            (vec![(format!("port {}", port + 1), f)], model)
        }
        Topology::Pi | Topology::T => {
            if n != 2 || d.inputs().len() != 2 {
                return Err(p.error("topology", format!("π and T are a two-port's; this has {n} ports")));
            }
            let zz = [z0[0], z0[1]];
            let pi = topology == Topology::Pi;
            let mut arms = [Vec::new(), Vec::new(), Vec::new()];
            for k in 0..w.len() {
                let m = if pi { mats(k).admittance(&zz) } else { mats(k).impedance(&zz) };
                let m = m.ok_or_else(|| fail(if pi { "Y" } else { "Z" }))?;
                // π: Y11 + Y12, Y22 + Y12 shunt, −1/Y12 series. T: Z11 − Z12, Z22 − Z12 series, Z12 shunt.
                let (a, b, c) = if pi { (m[0] + m[1], m[3] + m[1], -ONE / m[1]) } else { (m[0] - m[1], m[3] - m[1], m[1]) };
                arms[0].push(a);
                arms[1].push(b);
                arms[2].push(c);
            }
            let z = 0.5 * (zz[0] + zz[1]);
            let arm = if pi { 1.0 / z } else { z };
            let fits = [
                fit_branch(&w, &arms[0], pi, arm).ok_or_else(|| fail("branch"))?,
                fit_branch(&w, &arms[1], pi, arm).ok_or_else(|| fail("branch"))?,
                fit_branch(&w, &arms[2], false, z).ok_or_else(|| fail("branch"))?,
            ];
            let mut model = Vec::with_capacity(4 * w.len());
            for x in &w {
                let [a, b, c] = fits.map(|f| f.value(*x));
                let m = if pi {
                    let y12 = -ONE / c;
                    SMatrix::from_admittance(&[a - y12, y12, y12, b - y12], &zz)
                } else {
                    SMatrix::from_impedance(&[a + c, c, c, b + c], &zz)
                };
                let m = m.ok_or_else(|| fail("S of the fitted network"))?;
                model.extend(m.rows().flatten().copied());
            }
            let names = if pi { ["shunt at port 1", "shunt at port 2", "series"] } else { ["series at port 1", "series at port 2", "shunt"] };
            (names.into_iter().map(str::to_string).zip(fits).collect(), model)
        }
    };
    let ports = if matches!(topology, Topology::Series | Topology::Shunt) { 1 } else { 2 };
    let data = SData::square(d.frequencies().to_vec(), ports, model).map_err(EngineError::failed)?;
    let name = kept_name(p, "equivalent")?;
    let refs = if ports == 1 { vec![z0[p.integer("port", 1..=n).or(1)? - 1]] } else { vec![z0[0], z0[1]] };
    keep(s, Measured { name: name.clone(), file: "(results.equivalent)".into(), references: refs, data: Arc::new(data), noise: Vec::new() })?;
    Ok(json!({
        "topology": topology.id(),
        "references": z0,
        "branches": branches.iter().map(|(b, f)| f.json(b)).collect::<Vec<_>>(),
        "kept": name,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_series_rlc_is_found_again_from_its_impedance() {
        // R = 5 Ω, L = 2 nH, C = 1 pF: Z = R + j(ωL − 1/(ωC)).
        let w: Vec<f64> = (1..=40).map(|k| 2.0 * std::f64::consts::PI * 1e8 * f64::from(k)).collect();
        let z: Vec<C> = w.iter().map(|x| C::new(5.0, x * 2e-9 - 1.0 / (x * 1e-12))).collect();
        let f = fit_branch(&w, &z, false, 50.0).unwrap();
        assert!((f.loss - 5.0).abs() < 1e-9 && (f.l.unwrap() - 2e-9).abs() < 1e-18 && (f.c.unwrap() - 1e-12).abs() < 1e-21, "{f:?}");
        assert!(f.error < 1e-12);
        // An inductor alone: no capacitor.
        let l: Vec<C> = w.iter().map(|x| C::new(0.0, x * 3e-9)).collect();
        let f = fit_branch(&w, &l, false, 50.0).unwrap();
        assert!(f.c.is_none() && (f.l.unwrap() - 3e-9).abs() < 1e-18);
    }
}

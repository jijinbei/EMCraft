//! Circuits round the solved S-parameters: `circuit.cascade` chains two-ports — the sweep, a
//! measurement, lumped elements in series or shunt, transmission lines — by their chain (ABCD)
//! matrices; `results.equivalent` fits lumped RLC branches to a one-port's impedance or a
//! two-port's π or T network (Pozar, Microwave Engineering, 4th ed., §4.4, Table 4.1–4.2).
//! Results are kept with the measurements, drawn over the S-parameter chart.

use std::sync::Arc;

use emcraft_model::{Arrangement, LumpedElement, Point3};
use emcraft_solver::{Complex64 as C, SMatrix};
use serde_json::{Value, json};

use crate::network::{Measured, interpolate, keep, kept_name, source};
use crate::sdata::SData;
use crate::{EngineError, Params, Result, Session};

/// The most blocks a cascade chains.
const MAX_BLOCKS: usize = 32;

/// The most frequencies a cascade without S blocks is evaluated at.
const MAX_POINTS: usize = 4001;

/// The speed of light in vacuum (m/s).
const C0: f64 = 299_792_458.0;

const ONE: C = C::new(1.0, 0.0);
const ZERO: C = C::new(0.0, 0.0);

/// A two-port of a cascade.
enum Block {
    /// S over frequency on its ports' reference impedances; `flip` swaps its ports.
    Data { data: SData, z0: [f64; 2], flip: bool, name: String },
    /// A lumped element in series along the line, or shunt across it.
    Element { element: LumpedElement, shunt: bool },
    /// A uniform line: its impedance (Ω), length (m) and relative permittivity.
    Line { z0: f64, length: f64, eps_r: f64 },
}

/// A lumped element from a block's `resistance` (Ω), `inductance` (nH), `capacitance` (pF) and
/// `arrangement`, checked as `model.element.create` checks its own.
fn element(p: &Params) -> Result<LumpedElement> {
    let part = |name: &str, si: f64| -> Result<f64> {
        let v = p.number(name).or(0.0)?;
        if !(v.is_finite() && v >= 0.0) {
            return Err(p.error(name, "must be a finite number ≥ 0"));
        }
        Ok(v * si)
    };
    let e = LumpedElement {
        name: "block".into(),
        from: Point3::ORIGIN,
        to: Point3::new(0.0, 0.0, 1e-3),
        resistance: part("resistance", 1.0)?,
        inductance: part("inductance", 1e-9)?,
        capacitance: part("capacitance", 1e-12)?,
        arrangement: p.pick("arrangement").or(Arrangement::Series)?,
        across: None,
    };
    e.check().map_err(|err| p.error("resistance", err.to_string()))?;
    Ok(e)
}

/// The chain matrix [A, B, C, D] of a block at `f` (Hz).
fn chain(block: &Block, f: f64) -> Result<[C; 4]> {
    match block {
        Block::Element { element, shunt } => {
            let y = element.admittance(f);
            if *shunt {
                Ok([ONE, ZERO, y, ONE])
            } else if y.norm() == 0.0 {
                Err(EngineError::failed("a series element open at this frequency: nothing goes through"))
            } else {
                Ok([ONE, ONE / y, ZERO, ONE])
            }
        }
        Block::Line { z0, length, eps_r } => {
            let theta = 2.0 * std::f64::consts::PI * f * eps_r.sqrt() / C0 * length;
            let (c, s) = (theta.cos(), theta.sin());
            Ok([C::new(c, 0.0), C::new(0.0, z0 * s), C::new(0.0, s / z0), C::new(c, 0.0)])
        }
        Block::Data { data, z0, flip, name } => {
            let at = |p: usize, q: usize| interpolate(data, p, q, f);
            let s = [at(0, 0), at(0, 1), at(1, 0), at(1, 1)];
            let [Some(s11), Some(s12), Some(s21), Some(s22)] = s else {
                return Err(EngineError::failed(format!("`{}` has no S at {:.6} GHz: outside its band", crate::text::clip(name, 40), f / 1e9)));
            };
            let m = if *flip { [s22, s21, s12, s11] } else { [s11, s12, s21, s22] };
            let z = if *flip { [z0[1], z0[0]] } else { *z0 };
            SMatrix::from_fn(2, |i, j| m[2 * i + j])
                .chain(z)
                .ok_or_else(|| EngineError::failed(format!("`{}` passes nothing (S21 = 0) at {:.6} GHz", crate::text::clip(name, 40), f / 1e9)))
        }
    }
}

/// The blocks a cascade's `blocks` lists, and the frequencies the first S block gives.
fn blocks(s: &Session, p: &Params, z_default: f64) -> Result<(Vec<Block>, Option<Vec<f64>>)> {
    let items = p.items("blocks", "block", MAX_BLOCKS)?;
    if items.is_empty() {
        return Err(p.error("blocks", "at least one block"));
    }
    let mut out = Vec::new();
    let mut freqs = None;
    for b in &items {
        let kind = b.string("type").required()?;
        let block = match kind {
            "series" | "shunt" => Block::Element { element: element(b)?, shunt: kind == "shunt" },
            "line" => {
                let z0 = b.positive("impedance").or(50.0)?;
                let length = b.positive_length("length").required()?;
                let eps_r = b.positive("eps_r").or(1.0)?;
                Block::Line { z0, length, eps_r }
            }
            "data" => {
                let src = source(s, b)?;
                if src.data.outputs() != 2 || src.data.inputs().len() != 2 {
                    return Err(b.error("data", format!("a two-port with both ports excited; this has {} ports", src.data.outputs())));
                }
                let z = |k: usize| src.references[k].unwrap_or(z_default);
                freqs.get_or_insert_with(|| src.data.frequencies().to_vec());
                let name = b.string("data").optional()?.unwrap_or("sweep").to_string();
                Block::Data { z0: [z(0), z(1)], flip: b.flag("flip").or(false)?, data: src.data, name }
            }
            other => return Err(b.error("type", format!("`{}` is not a block: `data`, `series`, `shunt` or `line`", crate::text::clip(other, 20)))),
        };
        out.push(block);
    }
    Ok((out, freqs))
}

/// S on `z0` at both ports from a chain matrix (Pozar Table 4.2).
fn s_of_chain([a, b, c, d]: [C; 4], z0: f64) -> Option<[C; 4]> {
    let delta = a + b / z0 + c * z0 + d;
    if delta.norm() == 0.0 {
        return None;
    }
    let det = a * d - b * c;
    Some([(a + b / z0 - c * z0 - d) / delta, 2.0 * det / delta, 2.0 / delta, (-a + b / z0 - c * z0 + d) / delta])
}

/// 2 × 2 chain matrices multiplied.
fn times(x: [C; 4], y: [C; 4]) -> [C; 4] {
    [x[0] * y[0] + x[1] * y[2], x[0] * y[1] + x[1] * y[3], x[2] * y[0] + x[3] * y[2], x[2] * y[1] + x[3] * y[3]]
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
            let n = p.integer("points", 2..=MAX_POINTS).or(201)?;
            if !(a > 0.0 && b > a) {
                return Err(p.error("stop", "above `start`, both positive"));
            }
            crate::numbers::evenly_spaced(a, b, n)
        }
    };
    let mut values = Vec::with_capacity(4 * freqs.len());
    for f in &freqs {
        let mut m = [ONE, ZERO, ZERO, ONE];
        for b in &list {
            m = times(m, chain(b, *f)?);
        }
        let [s11, s12, s21, s22] = s_of_chain(m, z0).ok_or_else(|| EngineError::failed("the chain has no S on this impedance"))?;
        values.extend([s11, s12, s21, s22]);
    }
    let data = SData::square(freqs, 2, values).map_err(EngineError::failed)?;
    let name = kept_name(p, "cascade")?;
    let reply = json!({"name": name, "blocks": list.len(), "frequencies": data.frequencies().len(), "impedance": z0});
    keep(s, Measured { name, file: "(circuit.cascade)".into(), references: vec![z0, z0], data: Arc::new(data) })?;
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
    keep(s, Measured { name: name.clone(), file: "(results.equivalent)".into(), references: refs, data: Arc::new(data) })?;
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

    #[test]
    fn a_matched_lines_chain_matrix_is_its_own() {
        // λ/4 line: A = D = 0, B = jZ₀, C = j/Z₀; two make S21 = −1 (Pozar, Microwave Engineering,
        // 4th ed., §4.4, Tables 4.1–4.2).
        // λ/4 at 1 GHz in vacuum: A = D = 0, B = j Z₀, C = j/Z₀.
        let m = chain(&Block::Line { z0: 50.0, length: C0 / 4e9, eps_r: 1.0 }, 1e9).unwrap();
        assert!(m[0].norm() < 1e-12 && (m[1] - C::new(0.0, 50.0)).norm() < 1e-9);
        // Two in a row: a half wave, S21 = −1.
        let s = s_of_chain(times(m, m), 50.0).unwrap();
        assert!((s[2] + ONE).norm() < 1e-12 && s[0].norm() < 1e-12);
    }
}

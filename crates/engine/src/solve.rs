//! The solvers' commands: the driven sweep, the time domain, eigenmodes and port modes.

use std::f64::consts::{LOG10_E, PI};
use std::sync::Arc;

use emcraft_fdtd_gpu::{Backend, Gpu, Precision};
use emcraft_model::units::{C0, Length};
use emcraft_solver::Driven;
use serde_json::{Value, json};

use crate::results::{complex, sparameter_table};
use crate::{EIGEN_MAX, EngineError, Params, Result, Session, Sweep};

/// The frequencies (Hz, ascending, distinct) of `frequencies`, or of `points` from `start` to
/// `stop` (the band by default).
fn frequencies(s: &Session, p: &Params) -> Result<Vec<f64>> {
    let mut out = if p.has("frequencies") {
        let unit = s.display.frequency;
        p.list("frequencies", s.limits.frequencies)
            .required()?
            .iter()
            .map(|v| {
                v.as_f64().filter(|f| f.is_finite() && *f > 0.0).map(|f| unit.to_si(f)).ok_or_else(|| p.error("frequencies", "must be numbers > 0"))
            })
            .collect::<Result<Vec<f64>>>()?
    } else {
        // The job's band when neither is given.
        let hint = "give `frequencies`, or `start` and `stop`, or set the band: project.frequency";
        let start = p.frequency("start").or_maybe(s.model.band.map(|b| b.start)).required_or(hint)?;
        let stop = p.frequency("stop").or_maybe(s.model.band.map(|b| b.stop)).required_or(hint)?;
        let n = p.integer("points", 1..=s.limits.frequencies).or(11)?;
        if stop < start {
            return Err(p.error("stop", "must not be below `start`"));
        }
        (0..n).map(|k| if n == 1 { start } else { start + (stop - start) * k as f64 / (n - 1) as f64 }).collect()
    };
    if out.is_empty() {
        return Err(EngineError::Params("no frequencies".into()));
    }
    out.sort_by(f64::total_cmp);
    out.dedup();
    Ok(out)
}

/// How a sweep solves its frequencies (`sweep`).
#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Strategy {
    Fast,
    Full,
}

pub fn frequency(s: &mut Session, p: &Params) -> Result<Value> {
    let freqs = frequencies(s, p)?;
    let order = p.order()?;
    let driven = Driven::new(&s.model, order)?;
    // Three frequencies or more: the reduced-basis sweep, unless asked for a full solve at each.
    let fast = match p.choice("sweep").or(Strategy::Fast)? {
        Strategy::Fast => freqs.len() >= 3,
        Strategy::Full => false,
    };
    let (results, full) = if fast {
        crate::progress::report(0, freqs.len(), "fast sweep")?;
        let r = emcraft_solver::sweep::fast_sweep(&driven, &freqs, &Default::default())?;
        (r.results, r.full)
    } else {
        let results = freqs
            .iter()
            .enumerate()
            .map(|(k, f)| {
                crate::progress::report(k, freqs.len(), "frequency")?;
                Ok(driven.solve(*f)?)
            })
            .collect::<Result<Vec<_>>>()?;
        let full = vec![true; results.len()];
        (results, full)
    };
    let unknowns = driven.dofs.unknowns;
    s.results.sweep = Some(Sweep { order, driven: Arc::new(driven), results });
    let mut reply = sparameter_table(s)?;
    reply["unknowns"] = json!(unknowns);
    reply["order"] = json!(order.as_u8());
    reply["solved_in_full"] = json!(full.iter().filter(|f| **f).count());
    if let Some(points) = reply["points"].as_array_mut() {
        for (pt, f) in points.iter_mut().zip(&full) {
            pt["full"] = json!(f);
        }
    }
    Ok(reply)
}

/// The time-domain solver's devices by the names commands use: the CPU's threads, then the
/// GPU crate's backends.
const DEVICES: [(&str, Option<Backend>); 3] = [("cpu", None), ("cuda", Some(Backend::Cuda)), ("cubecl-cpu", Some(Backend::Cpu))];

pub fn time_domain(s: &mut Session, p: &Params) -> Result<Value> {
    let freqs = frequencies(s, p)?;
    let cell = p.positive_length("cell").or(Length::Millimetre.to_si(1.0))?;
    let device = p.string("device").or("cpu")?;
    let backend = DEVICES.iter().find(|(name, _)| *name == device).ok_or_else(|| p.error("device", "must be cpu, cuda or cubecl-cpu"))?.1;
    let precision = match p.string("precision").optional()? {
        None => Precision::Single,
        Some(name) => Precision::parse(name).ok_or_else(|| p.error("precision", "must be single or double"))?,
    };
    if backend.is_none() && p.has("precision") && precision == Precision::Single {
        return Err(p.error("precision", "the CPU's threads run in double precision (single needs a GPU device)"));
    }
    if s.model.open.is_some() {
        if backend.is_some() {
            return Err(emcraft_fdtd::FdtdError::Unsupported(format!("open models on {device} (run them on cpu)")).into());
        }
        return crate::open::solve_time_domain_open(s, &freqs, cell);
    }
    let gpu = backend.map(|backend| Gpu { backend, precision });
    let r =
        emcraft_fdtd::s_parameters_with(&s.model, &emcraft_fdtd::Options::new(cell), &freqs, gpu.as_ref().map(|g| g as &dyn emcraft_fdtd::Stepper))?;
    let points: Vec<Value> = r
        .frequencies
        .iter()
        .zip(r.s11.iter().zip(&r.s21))
        .map(|(f, (a, b))| json!({"frequency": s.show_frequency(*f), "s11": complex(*a), "s21": complex(*b)}))
        .collect();
    let precision = gpu.map_or("double", |g| g.precision.name());
    let reply = json!({"cells": r.cells, "steps": r.steps, "reference_steps": r.reference_steps, "converged": r.converged, "device": device, "precision": precision, "frequency_unit": s.display.frequency.symbol(), "points": points});
    s.results.time_domain = Some(r);
    Ok(reply)
}

pub fn devices(_: &mut Session, _: &Params) -> Result<Value> {
    let gpu = emcraft_fdtd_gpu::available();
    let list: Vec<Value> = DEVICES
        .iter()
        .map(|(name, backend)| match backend.and_then(|b| gpu.iter().find(|a| a.backend == b)) {
            None => json!({"name": name, "available": true, "precisions": ["double"]}),
            Some(a) => {
                let precisions: Vec<&str> = a.precisions.iter().map(|p| p.name()).collect();
                json!({"name": name, "available": a.problem.is_none(), "precisions": precisions, "problem": a.problem})
            }
        })
        .collect();
    Ok(json!({"devices": list}))
}

pub fn eigenmodes(s: &mut Session, p: &Params) -> Result<Value> {
    let near = p.frequency("near").required()?;
    let count = p.integer("count", 1..=EIGEN_MAX).or(5)?;
    let e = emcraft_solver::eigen::eigenmodes(&s.model, p.order()?, count, near)?;
    s.results.eigen = Some(Arc::new(e));
    crate::results::eigenmode_table(s)
}

pub fn port_modes(s: &mut Session, p: &Params) -> Result<Value> {
    let f = p.frequency("frequency").or_maybe(s.model.band.map(|b| b.stop)).required_or("or set the band: project.frequency")?;
    let list = emcraft_solver::driven::port_modes(&s.model, p.order()?, f)?;
    let k0 = 2.0 * PI * f / C0;
    // Per display length: a quantity per metre times the metres in one unit.
    let per_unit = s.display.length.to_si(1.0);
    let ports: Vec<Value> = list
        .iter()
        .map(|(name, modes)| {
            let rows: Vec<Value> = modes
                .iter()
                .map(|m| {
                    if m.beta2 > 0.0 {
                        let beta = m.beta2.sqrt();
                        json!({"name": m.name, "propagating": true, "beta": beta * per_unit, "eps_eff": m.beta2 / (k0 * k0), "impedance": m.impedance, "closed_form": m.closed_form})
                    } else {
                        let alpha = (-m.beta2).sqrt();
                        json!({"name": m.name, "propagating": false, "attenuation_db": 20.0 * LOG10_E * alpha * per_unit, "closed_form": m.closed_form})
                    }
                })
                .collect();
            json!({"name": name, "modes": rows})
        })
        .collect();
    Ok(json!({"frequency": s.show_frequency(f), "length_unit": s.display.length.symbol(), "ports": ports}))
}

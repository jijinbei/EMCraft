//! The solvers' commands: the driven sweep, the time domain, eigenmodes and port modes.

use std::f64::consts::{LOG10_E, PI};
use std::sync::Arc;

use emcraft_gpu::{Backend, Fronts, Gpu, Precision};
use emcraft_model::units::{C0, Length};
use emcraft_solver::Driven;
use emcraft_solver::accel::Accelerator;
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

/// The devices by the names commands use: the CPU's threads, then the GPU crate's backends.
const DEVICES: [(&str, Option<Backend>); 3] = [("cpu", None), ("cuda", Some(Backend::Cuda)), ("cubecl-cpu", Some(Backend::Cpu))];

/// The device named `name` (`None` for the CPU's threads).
fn device_named(p: &Params, name: &str) -> Result<Option<Backend>> {
    Ok(DEVICES.iter().find(|(n, _)| *n == name).ok_or_else(|| p.error("device", "must be cpu, cuda or cubecl-cpu"))?.1)
}

/// The name commands use for `backend`.
fn device_name(backend: Option<Backend>) -> &'static str {
    DEVICES.iter().find(|(_, b)| *b == backend).map_or("cpu", |(n, _)| n)
}

/// Where the solvers run: the time domain's leapfrog (in `precision`) and the factorization's
/// large fronts (in complex double), on the CPU's threads or a GPU crate backend. The machine's,
/// not the model's: kept across new projects, never saved with them.
#[derive(Clone, Default)]
pub struct Compute {
    pub backend: Option<Backend>,
    pub precision: Precision,
    fronts: Option<Arc<Fronts>>,
}

impl std::fmt::Debug for Compute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Compute({}, {})", device_name(self.backend), self.precision)
    }
}

impl Compute {
    /// The device's name as commands give it.
    pub fn device(&self) -> &'static str {
        device_name(self.backend)
    }

    /// The fronts the device has held since it was set.
    pub fn fronts_taken(&self) -> usize {
        self.fronts.as_ref().map_or(0, |f| f.taken())
    }

    /// The factorization's accelerator, if the fronts go to a device.
    pub fn accelerator(&self) -> Option<Arc<dyn Accelerator>> {
        self.fronts.clone().map(|f| f as Arc<dyn Accelerator>)
    }
}

pub fn device(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("device").required()?;
    let backend = device_named(p, name)?;
    let precision = match p.string("precision").optional()? {
        None => Precision::Single,
        Some(n) => Precision::parse(n).ok_or_else(|| p.error("precision", "must be single or double"))?,
    };
    let fronts = match backend {
        None => None,
        Some(b) => {
            let here = emcraft_gpu::available().into_iter().find(|a| a.backend == b);
            if let Some(problem) = here.as_ref().and_then(|a| a.problem.clone()) {
                return Err(emcraft_fdtd::FdtdError::Device(format!("{name} cannot run here: {problem}")).into());
            }
            if !here.is_some_and(|a| a.precisions.contains(&precision)) {
                return Err(p.error("precision", format!("{name} does not run {precision} precision here")));
            }
            {
                // Without `min_front`, the last setting's stays.
                let min_front = p.integer("min_front", 1..=emcraft_gpu::MAX_ROWS).optional()?.or(s.compute.fronts.as_ref().map(|f| f.min_rows()));
                Some(Arc::new(Fronts::new(b, min_front)?))
            }
        }
    };
    s.compute = Compute { backend, precision, fronts };
    Ok(json!({"device": name, "precision": precision.name(), "factorization": s.compute.fronts.as_ref().map_or("cpu".to_owned(), |f| f.name())}))
}

pub fn time_domain(s: &mut Session, p: &Params) -> Result<Value> {
    if s.model.periodic.is_some() {
        return Err(crate::EngineError::failed(
            "not supported yet: periodic boundaries in the time domain (clear them with model.periodic, clear true)",
        ));
    }
    let freqs = frequencies(s, p)?;
    let cell = p.positive_length("cell").or(Length::Millimetre.to_si(1.0))?;
    let device = p.string("device").or(device_name(s.compute.backend))?;
    let backend = device_named(p, device)?;
    let precision = match p.string("precision").optional()? {
        None => s.compute.precision,
        Some(name) => Precision::parse(name).ok_or_else(|| p.error("precision", "must be single or double"))?,
    };
    if backend.is_none() && p.has("precision") && precision == Precision::Single {
        return Err(p.error("precision", "the CPU's threads run in double precision (single needs a GPU device)"));
    }
    let gpu = backend.map(|backend| Gpu { backend, precision });
    if s.model.open.is_some() {
        let mut reply = crate::open::solve_time_domain_open(s, &freqs, cell, gpu.as_ref().map(|g| g as &dyn emcraft_fdtd::OpenStepper))?;
        reply["device"] = json!(device);
        reply["precision"] = json!(gpu.map_or("double", |g| g.precision.name()));
        return Ok(reply);
    }
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
    let gpu = emcraft_gpu::available();
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

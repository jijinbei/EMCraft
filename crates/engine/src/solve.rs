//! The solvers' commands: the driven sweep, the time domain, eigenmodes and port modes.

use std::f64::consts::{LOG10_E, PI};
use std::sync::Arc;

use emcraft_gpu::{Backend, Fronts, Gpu, Precision};
use emcraft_model::units::{C0, Length};
use emcraft_solver::accel::Accelerator;
use emcraft_solver::{Driven, FrequencyResult};
use serde_json::{Value, json};

use crate::reply::complex;
use crate::results::sparameter_table;
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
                p.number_of("frequencies", v)
                    .ok()
                    .filter(|f| *f > 0.0)
                    .map(|f| unit.to_si(f))
                    .ok_or_else(|| p.error("frequencies", "must be numbers > 0"))
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
        crate::numbers::evenly_spaced(start, stop, n)
    };
    if out.is_empty() {
        return Err(EngineError::Params("no frequencies".into()));
    }
    out.sort_by(f64::total_cmp);
    out.dedup();
    Ok(out)
}

emcraft_macros::named_enum! {
    /// How a sweep solves its frequencies (`sweep`).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(crate) enum Strategy {
        Fast = "fast",
        Full = "full",
    }
}

emcraft_macros::named_serde!(Strategy);

emcraft_macros::named_enum! {
    /// Where the solvers run (`device`): the CPU's threads, or a GPU crate backend.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum Device {
        /// The CPU's threads.
        #[default]
        Cpu = "cpu",
        /// An NVIDIA GPU.
        Cuda = "cuda",
        /// The GPU's kernels on the CPU (to check them without a GPU).
        CubeclCpu = "cubecl-cpu",
    }
}

impl Device {
    /// The GPU crate's backend (`None` for the CPU's threads).
    pub fn backend(self) -> Option<Backend> {
        match self {
            Device::Cpu => None,
            Device::Cuda => Some(Backend::Cuda),
            Device::CubeclCpu => Some(Backend::Cpu),
        }
    }

    /// The device of `backend` (`None` for the CPU's threads).
    pub fn of(backend: Option<Backend>) -> Device {
        Device::ALL.iter().copied().find(|d| d.backend() == backend).unwrap_or_default()
    }
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
    let (results, full) = if fast { fast_sweep(&driven, &freqs)? } else { full_sweep(&driven, &freqs)? };
    let unknowns = driven.dofs.unknowns;
    s.results.sweep = Some(Arc::new(Sweep { order, driven: Arc::new(driven), results }));
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

/// The results at `freqs` by the reduced-basis sweep, and which were solved in full.
fn fast_sweep(driven: &Driven, freqs: &[f64]) -> Result<(Vec<FrequencyResult>, Vec<bool>)> {
    crate::progress::report(0, freqs.len(), "fast sweep")?;
    let r = emcraft_solver::sweep::fast_sweep(driven, freqs, &Default::default())?;
    Ok((r.results, r.full))
}

/// The results at `freqs`, each solved in full.
fn full_sweep(driven: &Driven, freqs: &[f64]) -> Result<(Vec<FrequencyResult>, Vec<bool>)> {
    let results = freqs
        .iter()
        .enumerate()
        .map(|(k, f)| {
            crate::progress::report(k, freqs.len(), "frequency")?;
            Ok(driven.solve(*f)?)
        })
        .collect::<Result<Vec<_>>>()?;
    let full = vec![true; results.len()];
    Ok((results, full))
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
        write!(f, "Compute({}, {})", Device::of(self.backend).id(), self.precision)
    }
}

impl Compute {
    /// The device's name as commands give it.
    pub fn device(&self) -> &'static str {
        Device::of(self.backend).id()
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
    let device: Device = p.pick("device").required()?;
    let (name, backend) = (device.id(), device.backend());
    let precision = p.pick("precision").or(Precision::Single)?;
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
    Ok(json!({"device": name, "precision": precision.id(), "factorization": s.compute.fronts.as_ref().map_or("cpu".to_owned(), |f| f.name())}))
}

pub fn time_domain(s: &mut Session, p: &Params) -> Result<Value> {
    if s.model.periodic.is_some() {
        return Err(crate::EngineError::failed(
            "not supported yet: periodic boundaries in the time domain (clear them with model.periodic, clear true)",
        ));
    }
    let freqs = frequencies(s, p)?;
    let cell = p.positive_length("cell").or(Length::Millimetre.to_si(1.0))?;
    let device = p.pick("device").or(Device::of(s.compute.backend))?;
    let backend = device.backend();
    let precision = p.pick("precision").or(s.compute.precision)?;
    if backend.is_none() && p.has("precision") && precision == Precision::Single {
        return Err(p.error("precision", "the CPU's threads run in double precision (single needs a GPU device)"));
    }
    let gpu = backend.map(|backend| Gpu { backend, precision });
    if s.model.open.is_some() {
        let mut reply = crate::open::solve_time_domain_open(s, &freqs, cell, gpu.as_ref().map(|g| g as &dyn emcraft_fdtd::OpenStepper))?;
        reply["device"] = json!(device.id());
        reply["precision"] = json!(gpu.map_or("double", |g| g.precision.id()));
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
    let precision = gpu.map_or("double", |g| g.precision.id());
    let mut reply = json!({"cells": r.cells, "steps": r.steps, "reference_steps": r.reference_steps, "converged": r.converged, "device": device.id(), "precision": precision, "frequency_unit": s.display.frequency.symbol(), "points": points});
    // Where the time domain matches the loss tangents (as conductivities), when there are any.
    if s.model.materials.iter().any(|m| m.loss_tangent != 0.0 || m.is_magnetically_lossy()) {
        reply["loss_reference"] = json!(s.show_frequency(r.loss_reference));
    }
    s.results.time_domain = Some(Arc::new(r));
    Ok(reply)
}

pub fn devices(_: &mut Session, _: &Params) -> Result<Value> {
    let gpu = emcraft_gpu::available();
    let list: Vec<Value> = Device::ALL
        .iter()
        .map(|d| (d.id(), d.backend()))
        .map(|(name, backend)| match backend.and_then(|b| gpu.iter().find(|a| a.backend == b)) {
            None => json!({"name": name, "available": true, "precisions": ["double"]}),
            Some(a) => {
                let precisions: Vec<&str> = a.precisions.iter().map(|p| p.id()).collect();
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
    let f = crate::args::frequency_or_band_top(s, p, "frequency", crate::args::SET_THE_BAND)?;
    let list = emcraft_solver::driven::port_modes(&s.model, p.order()?, f)?;
    let k0 = 2.0 * PI * f / C0;
    // Per display length: a quantity per metre times the metres in one unit.
    let per_unit = s.display.length.to_si(1.0);
    let ports: Vec<Value> = list
        .iter()
        .map(|(name, modes)| {
            let rows: Vec<Value> = modes.iter().map(|m| port_mode_row(m, k0, per_unit)).collect();
            json!({"name": name, "modes": rows})
        })
        .collect();
    Ok(json!({"frequency": s.show_frequency(f), "length_unit": s.display.length.symbol(), "ports": ports}))
}

/// A port mode in a reply: β (per display length `per_unit` m) and the effective permittivity
/// at free-space wavenumber `k0` when it propagates, its attenuation when it does not.
fn port_mode_row(m: &emcraft_solver::driven::ModeInfo, k0: f64, per_unit: f64) -> Value {
    // In a lossy medium: γ = α + jβ, propagating while β > α.
    if let Some(g) = m.gamma {
        let attenuation_db = 20.0 * LOG10_E * g.re * per_unit;
        let eps_eff = -(g * g).re / (k0 * k0);
        return json!({"name": m.name, "propagating": g.im > g.re, "beta": g.im * per_unit, "attenuation_db": attenuation_db, "eps_eff": eps_eff, "impedance": m.impedance, "closed_form": m.closed_form});
    }
    if m.beta2 > 0.0 {
        let beta = m.beta2.sqrt();
        json!({"name": m.name, "propagating": true, "beta": beta * per_unit, "eps_eff": m.beta2 / (k0 * k0), "impedance": m.impedance, "closed_form": m.closed_form})
    } else {
        let alpha = (-m.beta2).sqrt();
        json!({"name": m.name, "propagating": false, "attenuation_db": 20.0 * LOG10_E * alpha * per_unit, "closed_form": m.closed_form})
    }
}

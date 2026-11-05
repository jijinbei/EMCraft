//! The solvers' commands: the driven sweep, the time domain, eigenmodes and port modes.

use emcraft_model::units::wavenumber;
use std::f64::consts::LOG10_E;
use std::sync::Arc;

use emcraft_gpu::{Backend, Fronts, Gpu, Precision};
use emcraft_model::units::Length;
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
        let hint = "give `frequencies`, or `start` and `stop`, or confirm the run's settings: project.setup";
        let start = p.frequency("start").or_maybe(s.model.job.range().map(|b| b.start.get())).required_or(hint)?;
        let stop = p.frequency("stop").or_maybe(s.model.job.range().map(|b| b.stop.get())).required_or(hint)?;
        let n = p.integer("points", 1..=s.limits.frequencies).or(DEFAULT_POINTS)?;
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

/// The frequencies a sweep takes from `start` to `stop` when `points` is not given: enough to
/// see a response's shape (a resonance, a cut-off); the fast sweep solves a few of them in full
/// and the rest from their reduced model, so they cost little more than a handful.
pub const DEFAULT_POINTS: usize = 101;

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
    /// How a sweep's systems are solved (`solver`).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum LinearSolver {
        Auto = "auto",
        Direct = "direct",
        Iterative = "iterative",
    }
}

emcraft_macros::named_serde!(LinearSolver);

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

emcraft_macros::named_serde!(Device);

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

/// The last sweep's system, when it is the model's still: the model unchanged since it was
/// solved (the same revision), the same `order`, its reference planes the ones it was built
/// with. A sweep run again on it skips the assembly (and its ordering, kept in the system).
fn reusable(s: &Session, order: emcraft_fem::Order) -> Option<Arc<Driven>> {
    let sw = s.results.sweep.as_ref()?;
    let planes = sw.driven.ports.iter().map(|p| p.deembed).eq(sw.deembed.iter().copied());
    (!s.results.is_stale(crate::ResultKind::Sweep) && sw.order == order && planes).then(|| sw.driven.clone())
}

pub fn frequency(s: &mut Session, p: &Params) -> Result<Value> {
    let freqs = frequencies(s, p)?;
    let order = p.order()?;
    let asked = p.choice("solver").or(LinearSolver::Auto)?;
    // Second-order systems too large to factor whole are solved iteratively by default.
    let two = order == emcraft_fem::Order::Two;
    let iterative = |unknowns: usize| match asked {
        LinearSolver::Auto => Ok(two && unknowns > emcraft_solver::driven::MAX_UNKNOWNS),
        LinearSolver::Direct => Ok(false),
        LinearSolver::Iterative if two => Ok(true),
        LinearSolver::Iterative => Err(p.error("solver", "iterative needs second-order elements (`order` 2)")),
    };
    let driven = match reusable(s, order) {
        Some(d) if d.iterative == iterative(d.dofs.unknowns)? => d,
        _ => {
            let mut d = Driven::new(&s.model, order)?;
            d.iterative = iterative(d.dofs.unknowns)?;
            Arc::new(d)
        }
    };
    // Three frequencies or more: the reduced-basis sweep, unless asked for a full solve at each.
    let fast = match p.choice("sweep").or(Strategy::Fast)? {
        Strategy::Fast => freqs.len() >= 3,
        Strategy::Full => false,
    };
    let results = if fast { fast_sweep(&driven, &freqs)? } else { full_sweep(&driven, &freqs)? };
    let full = in_full(&results);
    let unknowns = driven.dofs.unknowns;
    s.results.sweep = Some(Arc::new(Sweep::new(order, driven, results)));
    let mut reply = sparameter_table(s)?;
    reply["unknowns"] = json!(unknowns);
    reply["order"] = json!(order.as_u8());
    reply["solved_in_full"] = json!(full);
    Ok(reply)
}

/// `solve.refine`: more frequencies in the sweep — `points` evenly from `start` to `stop`, or
/// `frequencies` — solved on the sweep's own system (its mesh and order, no assembly again),
/// referred to its reference planes and merged with it. The run recorded is the merged sweep's
/// (`solve.frequency` with its frequencies), so running it again gives the same sweep.
pub fn refine(s: &mut Session, p: &Params) -> Result<Value> {
    let sw = s.results.sweep.clone().ok_or(EngineError::NotSolved { what: "sweep", run: "solve.frequency" })?;
    if s.results.is_stale(crate::ResultKind::Sweep) {
        return Err(EngineError::failed("the model changed since the sweep: run the sweep again (solve.frequency)"));
    }
    let new: Vec<f64> = frequencies(s, p)?.into_iter().filter(|f| sw.at(*f, false).is_none()).collect();
    let total = sw.results.len() + new.len();
    if total > s.limits.frequencies {
        return Err(EngineError::Params(format!("the sweep would hold {total} frequencies, more than {}", s.limits.frequencies)));
    }
    let results = match new.len() {
        0 => Vec::new(),
        n if n >= 3 => fast_sweep(&sw.driven, &new)?,
        _ => full_sweep(&sw.driven, &new)?,
    };
    let full = in_full(&results);
    let mut merged = (*sw).clone();
    merged.results.extend(sw.referred(results)?);
    merged.results.sort_by(|a, b| a.frequency.total_cmp(&b.frequency));
    // The run as the merged sweep: the last sweep's settings, its frequencies listed.
    let fu = s.display.frequency;
    let mut params = s.runs.iter().rev().find(|r| r.command == "solve.frequency").map_or_else(|| json!({}), |r| r.params.clone());
    if let Some(o) = params.as_object_mut() {
        for k in ["start", "stop", "points", "frequencies"] {
            o.remove(k);
        }
    }
    params["frequencies"] = json!(merged.results.iter().map(|r| fu.from_si(r.frequency)).collect::<Vec<f64>>());
    params["order"] = json!(merged.order.as_u8());
    s.next_run = Some(crate::runs::Run { command: "solve.frequency".into(), params, units: s.display });
    s.results.sweep = Some(Arc::new(merged));
    let mut reply = sparameter_table(s)?;
    reply["added"] = json!(new.iter().map(|f| fu.from_si(*f)).collect::<Vec<f64>>());
    reply["solved_in_full"] = json!(full);
    reply["order"] = json!(sw.order.as_u8());
    Ok(reply)
}

/// How many of `results` were solved in full (each says: [`FrequencyResult::full`]).
fn in_full(results: &[FrequencyResult]) -> usize {
    results.iter().filter(|r| r.full).count()
}

/// The results at `freqs` by the reduced-basis sweep, each saying whether it was solved in full.
fn fast_sweep(driven: &Driven, freqs: &[f64]) -> Result<Vec<FrequencyResult>> {
    // Its full solves are not known in advance: an open count, with the residual's trend.
    let each = |s: emcraft_solver::sweep::SweepStep| crate::progress::report_open(s.full, s.max_full, "fast sweep", s.residual);
    Ok(emcraft_solver::sweep::fast_sweep_each::<crate::EngineError>(driven, freqs, &Default::default(), each)?.results)
}

/// The results at `freqs`, each solved in full.
fn full_sweep(driven: &Driven, freqs: &[f64]) -> Result<Vec<FrequencyResult>> {
    freqs
        .iter()
        .enumerate()
        .map(|(k, f)| {
            crate::progress::report(k, freqs.len(), "frequency")?;
            Ok(driven.solve(*f)?)
        })
        .collect()
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
    /// The device the solvers run on (`.id()` is its name as commands give it).
    pub fn device(&self) -> Device {
        Device::of(self.backend)
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
                let min_front =
                    p.integer("min_front", 1..=emcraft_gpu::MAX_ROWS).optional()?.or_else(|| s.compute.fronts.as_ref().map(|f| f.min_rows()));
                Some(Arc::new(Fronts::new(b, min_front)?))
            }
        }
    };
    s.compute = Compute { backend, precision, fronts };
    Ok(
        json!({"device": name, "precision": precision.id(), "factorization": s.compute.fronts.as_ref().map_or_else(|| "cpu".to_owned(), |f| f.name())}),
    )
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
        // A plane wave's split runs on the CPU's threads.
        let (device, gpu) = if crate::open::plane_wave(s).is_some() { (Device::Cpu, None) } else { (device, gpu) };
        reply["device"] = json!(device.id());
        reply["precision"] = json!(gpu.map_or("double", |g| g.precision.id()));
        return Ok(reply);
    }
    let options = emcraft_fdtd::Options::new(cell);
    // A solid's ports on its faces (the guide's own sections otherwise): on the CPU's threads.
    let solid = s.model.waveguide.is_none() && s.model.solid.is_some();
    let r = if solid {
        let inputs = excited(s, p)?;
        emcraft_fdtd::s_parameters_solid(&s.model, &options, &freqs, &inputs)?
    } else {
        if p.has("excite") {
            return Err(p.error("excite", "a guide's run excites port 1 (a solid's ports take `excite`)"));
        }
        emcraft_fdtd::s_parameters_with(&s.model, &options, &freqs, gpu.as_ref().map(|g| g as &dyn emcraft_fdtd::Stepper))?
    };
    let names: Vec<&str> = s.model.ports.iter().map(|p| p.name.as_str()).collect();
    let points: Vec<Value> = r
        .frequencies
        .iter()
        .enumerate()
        .map(|(k, f)| {
            let mut point = json!({"frequency": s.show_frequency(*f)});
            for (c, &q) in r.inputs.iter().enumerate() {
                for p in 0..r.ports {
                    if let Some(x) = r.column(c, p).get(k) {
                        point[format!("s{}{}", p + 1, q + 1)] = complex(*x);
                    }
                }
            }
            point
        })
        .collect();
    let (device, precision) = if solid { (Device::Cpu, "double") } else { (device, gpu.map_or("double", |g| g.precision.id())) };
    let excited: Vec<&str> = r.inputs.iter().filter_map(|&q| names.get(q).copied()).collect();
    let mut reply = json!({"cells": r.cells, "steps": r.steps, "reference_steps": r.reference_steps, "converged": r.converged, "device": device.id(), "precision": precision, "frequency_unit": s.display.frequency.symbol(), "ports": names, "excited": excited, "points": points});
    // Where the time domain matches the loss tangents (as conductivities), when there are any.
    if s.model.materials.iter().any(|m| m.loss_tangent.get() != 0.0 || m.is_magnetically_lossy()) {
        reply["loss_reference"] = json!(s.show_frequency(r.loss_reference));
    }
    s.results.time_domain = Some(Arc::new(r));
    Ok(reply)
}

/// The ports `excite` names (indices into the model's), the first when it is left out.
fn excited(s: &Session, p: &Params) -> Result<Vec<usize>> {
    let Some(list) = p.list("excite", emcraft_fdtd::MAX_EXCITED).optional()? else { return Ok(vec![0]) };
    let mut out = Vec::with_capacity(list.len());
    for v in list {
        let name = v.as_str().ok_or_else(|| p.error("excite", "lists port names"))?;
        let q = s.model.ports.iter().position(|x| x.name == name).ok_or_else(|| p.error("excite", format!("names no port `{name}`")))?;
        if out.contains(&q) {
            return Err(p.error("excite", format!("names port `{name}` twice")));
        }
        out.push(q);
    }
    if out.is_empty() {
        return Err(p.error("excite", "names no port"));
    }
    Ok(out)
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

/// `solve.port_modes`' reply: the frequency (display units) and each port's modes.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PortModes {
    pub frequency: f64,
    /// The display length β and the attenuation are per.
    pub length_unit: String,
    pub ports: Vec<PortModeList>,
}

/// A port's modes, the fundamental first.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PortModeList {
    pub name: String,
    pub modes: Vec<PortModeRow>,
}

/// A port mode: β (rad per display length) and the effective permittivity when it propagates,
/// its attenuation (dB per display length) when it does not or in a lossy medium, and the line
/// impedance of a TEM or quasi-TEM mode.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PortModeRow {
    pub name: String,
    pub propagating: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beta: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eps_eff: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impedance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attenuation_db: Option<f64>,
    pub closed_form: bool,
}

/// `solve.revolution`.
pub fn revolution(s: &mut Session, p: &Params) -> Result<Value> {
    let ring = p.pairs("points", 4096).required()?;
    let near = p.hertz("frequency").required()?.get();
    let m = p.integer("m", 0..=64).or(0)? as u32;
    let count = p.integer("count", 1..=50).or(5)?;
    let positive = |name: &str| -> Result<f64> {
        let v = p.number(name).or(1.0)?;
        if v.is_finite() && v > 0.0 { Ok(v) } else { Err(p.error(name, "must be a finite number > 0")) }
    };
    let (eps, mu) = (positive("eps_r")?, positive("mu_r")?);
    let extent = ring.iter().fold([f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY], |b, q| {
        [b[0].min(q[0]), b[1].max(q[0]), b[2].min(q[1]), b[3].max(q[1])]
    });
    let span = (extent[1] - extent[0]).max(extent[3] - extent[2]);
    let wavelength = emcraft_model::units::C0 / (near * (eps * mu).sqrt());
    let size = p.positive_length("size").optional()?.unwrap_or_else(|| (0.1 * wavelength).min(span / 20.0));
    let (points, triangles) = emcraft_mesh::planar::polygon(&ring, size)?;
    let material = vec![(eps, mu); triangles.len()];
    let profile = emcraft_solver::bor::Profile { points, triangles, material };
    let modes = emcraft_solver::bor::eigenmodes(&profile, m, count, near)?;
    let shown: Vec<Value> = modes.iter().map(|md| json!({"frequency": s.show_frequency(md.frequency), "m": md.m})).collect();
    Ok(json!({"modes": shown, "triangles": profile.triangles.len(), "points": profile.points.len()}))
}

/// `solve.cyclic`: a rotationally symmetric cavity's modes per harmonic, from its sector.
pub fn cyclic(s: &mut Session, p: &Params) -> Result<Value> {
    let rot = s.model.rotation.ok_or_else(|| EngineError::failed("no rotational symmetry: set it with model.rotation"))?;
    let near = crate::args::frequency_or_band_top(s, p, "frequency", crate::args::SET_THE_BAND)?;
    let count = p.integer("count", 1..=emcraft_solver::bloch::MAX_BANDS).or(4)?;
    let order = p.order()?;
    let harmonics: Vec<u32> = match p.integer("m", 0..=rot.n as usize - 1).optional()? {
        Some(m) => vec![m as u32],
        None => (0..=rot.n / 2).collect(),
    };
    let mut rows = Vec::with_capacity(harmonics.len());
    for (k, &m) in harmonics.iter().enumerate() {
        crate::progress::report(k, harmonics.len(), "harmonic")?;
        // The sector's mesh is the same for every harmonic: only the cut faces' phase moves.
        let mut model = s.model.clone();
        model.rotation = Some(emcraft_model::Rotation { m, ..rot });
        let modes = emcraft_solver::bloch::cyclic_modes(&model, order, count, near)?;
        let shown: Vec<f64> = modes.iter().map(|&f| s.show_frequency(f)).collect();
        rows.push(json!({"m": m, "modes": shown}));
    }
    Ok(json!({"n": rot.n, "order": order.as_u8(), "harmonics": rows}))
}

pub fn port_modes(s: &mut Session, p: &Params) -> Result<Value> {
    Ok(json!(port_modes_of(s, p)?))
}

/// [`port_modes`] as its type.
fn port_modes_of(s: &Session, p: &Params) -> Result<PortModes> {
    let f = crate::args::frequency_or_band_top(s, p, "frequency", crate::args::SET_THE_BAND)?;
    let list = emcraft_solver::driven::port_modes(&s.model, p.order()?, f)?;
    let k0 = wavenumber(f);
    // Per display length: a quantity per metre times the metres in one unit.
    let per_unit = s.display.length.to_si(1.0);
    let ports = list
        .iter()
        .map(|(name, modes)| PortModeList { name: name.clone(), modes: modes.iter().map(|m| port_mode_row(m, k0, per_unit)).collect() })
        .collect();
    Ok(PortModes { frequency: s.show_frequency(f), length_unit: s.display.length.symbol().to_owned(), ports })
}

/// A port mode at free-space wavenumber `k0`, per display length `per_unit` m.
fn port_mode_row(m: &emcraft_solver::driven::ModeInfo, k0: f64, per_unit: f64) -> PortModeRow {
    let row = PortModeRow {
        name: m.name.clone(),
        propagating: false,
        beta: None,
        eps_eff: None,
        impedance: None,
        attenuation_db: None,
        closed_form: m.closed_form,
    };
    // In a lossy medium: γ = α + jβ, propagating while β > α.
    if let Some(g) = m.gamma {
        return PortModeRow {
            propagating: g.im > g.re,
            beta: Some(g.im * per_unit),
            attenuation_db: Some(20.0 * LOG10_E * g.re * per_unit),
            eps_eff: Some(-(g * g).re / (k0 * k0)),
            impedance: m.impedance,
            ..row
        };
    }
    if m.beta2 > 0.0 {
        PortModeRow { propagating: true, beta: Some(m.beta2.sqrt() * per_unit), eps_eff: Some(m.beta2 / (k0 * k0)), impedance: m.impedance, ..row }
    } else {
        PortModeRow { attenuation_db: Some(20.0 * LOG10_E * (-m.beta2).sqrt() * per_unit), ..row }
    }
}

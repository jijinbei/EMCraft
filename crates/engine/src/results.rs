//! Commands that read results: S-parameters, eigenmodes, the comparison with a reference,
//! field errors and probes, and time-domain reflectometry. Their pictures are `view`'s.

use emcraft_model::{CVec3, Shape};
use emcraft_solver::Complex64 as C;
use serde_json::{Map, Value, json};

use crate::reply::complex;
use crate::{EngineError, Params, Result, Session, Sweep, args, view};

pub fn eigenmodes(s: &mut Session, _: &Params) -> Result<Value> {
    eigenmode_table(s)
}

/// The eigenmodes solved, with the closed form's for a primitive cavity.
pub(crate) fn eigenmode_table(s: &Session) -> Result<Value> {
    let e = s.results.eigen.as_ref().ok_or(EngineError::NotSolved { what: "eigenmodes", run: "solve.eigenmodes" })?;
    let modes: Vec<Value> = e
        .modes
        .iter()
        .enumerate()
        .map(|(i, m)| {
            json!({"mode": i + 1, "frequency": s.show_frequency(m.frequency), "q": m.q(), "q_walls": m.q_walls, "q_dielectric": m.q_dielectric, "q_magnetic": m.q_magnetic})
        })
        .collect();
    let mut reply =
        json!({"frequency_unit": s.display.frequency.symbol(), "unknowns": e.driven.dofs.unknowns, "iterations": e.iterations, "modes": modes});
    let exact = exact_resonances(s);
    if !exact.is_empty() {
        let rows: Vec<Value> = exact.into_iter().map(|(f, name)| json!({"mode": name, "frequency": s.show_frequency(f)})).collect();
        reply["exact"] = json!(rows);
    }
    Ok(reply)
}

/// The closed-form resonances (Hz, name) of a primitive cavity or a cavity of one full-width
/// waveguide section; none for other models.
fn exact_resonances(s: &Session) -> Vec<(f64, String)> {
    let m = &s.model;
    // Cut by a symmetry plane, the cavity holds only the modes of one parity: the whole list
    // would mislead.
    if !m.domain.symmetry.is_empty() {
        return Vec::new();
    }
    if let Some(w) = m.waveguide()
        && w.sections.len() == 1
        && w.uniform()
        && let Ok((_, mat)) = m.material(&w.sections[0].material)
    {
        return emcraft_theory::cavity_resonances(w.width.get(), w.height.get(), w.length(), mat.eps_r.get() * mat.mu_r.get(), 12);
    }
    if let Some(solid) = m.domain.solid.as_ref()
        && m.inclusions().is_empty()
        && m.features().is_empty()
        && let Ok((_, mat)) = m.material(&solid.material)
    {
        let er = mat.eps_r.get() * mat.mu_r.get();
        return match solid.shape {
            Shape::Box { size } => emcraft_theory::cavity_resonances(size[0], size[1], size[2], er, 12),
            Shape::Cylinder { radius, length } => emcraft_theory::bessel::cylinder_cavity_resonances(radius.get(), length.get(), er, 12),
            Shape::Sphere { radius } => emcraft_theory::bessel::sphere_cavity_resonances(radius.get(), er, 12),
            Shape::Surface { .. } | Shape::Sheet { .. } | Shape::Board(_) => Vec::new(),
        };
    }
    Vec::new()
}

pub fn sparameters(s: &mut Session, _: &Params) -> Result<Value> {
    sparameter_table(s)
}

/// The sweep's S-parameters, backward errors, timings and port impedances.
pub(crate) fn sparameter_table(s: &Session) -> Result<Value> {
    let sw = s.results.sweep()?;
    let entries = sw.entries();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    let rows: Vec<Value> = sw
        .results
        .iter()
        .map(|r| {
            let mut m = Map::new();
            m.insert("frequency".into(), json!(s.show_frequency(r.frequency)));
            for ((i, j), v) in r.s.entries() {
                m.insert(format!("s{}{}", i + 1, j + 1), complex(v));
            }
            m.insert("backward_error".into(), json!(r.backward_error));
            m.insert("full".into(), json!(r.full));
            let t = &r.timings;
            m.insert("seconds".into(), json!({"assembly": t.assembly, "analysis": t.analysis, "factor": t.factor, "solve": t.solve}));
            if r.impedances.iter().any(Option::is_some) {
                m.insert("impedances".into(), json!(r.impedances));
            }
            Value::Object(m)
        })
        .collect();
    let mut reply = json!({"ports": names, "frequency_unit": s.display.frequency.symbol(), "points": rows});
    if sw.driven.has_terminals() {
        let refs: Vec<Option<f64>> = entries.iter().map(|e| e.reference).collect();
        reply["references"] = json!(refs);
    }
    Ok(reply)
}

/// `results.compare`'s reply: the sweep against its reference, frequency by frequency.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Comparison {
    /// The reference's name (the exact TE10 solution, mode matching).
    pub reference: String,
    pub points: Vec<ComparePoint>,
    /// The worst |S − S_ref| over the points.
    pub worst: f64,
}

/// One frequency of a [`Comparison`] (display units): the sweep's S11 and S21, the reference's
/// and their differences |S − S_ref|.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ComparePoint {
    pub frequency: f64,
    pub s11: crate::Wave,
    pub s21: crate::Wave,
    pub exact_s11: crate::Wave,
    pub exact_s21: crate::Wave,
    pub error_s11: f64,
    pub error_s21: f64,
}

pub fn compare(s: &mut Session, p: &Params) -> Result<Value> {
    let modes = p.integer("modes", 10..=1000).or(view::MODE_MATCHING_MODES)?;
    let sw = s.results.sweep()?;
    let reference = view::reference(s)?;
    let mut worst: f64 = 0.0;
    let rows = sw
        .results
        .iter()
        .map(|r| {
            let (s11, s21) = reference.s(r.frequency, modes)?;
            // The reference is at the ports' faces: moved to the de-embedded planes as the
            // solver moves its own.
            let (a, b) = (deembedding(sw, 0, r.frequency)?, deembedding(sw, 1, r.frequency)?);
            let (s11, s21) = (s11 * a * a, s21 * a * b);
            let (f11, f21) = (r.s.get(0, 0).unwrap_or_default(), r.s.get(1, 0).unwrap_or_default());
            let (e11, e21) = ((f11 - s11).norm(), (f21 - s21).norm());
            worst = worst.max(e11).max(e21);
            Ok(ComparePoint {
                frequency: s.show_frequency(r.frequency),
                s11: crate::Wave::of(f11),
                s21: crate::Wave::of(f21),
                exact_s11: crate::Wave::of(s11),
                exact_s21: crate::Wave::of(s21),
                error_s11: e11,
                error_s21: e21,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    serde_json::to_value(Comparison { reference: reference.label().to_owned(), points: rows, worst }).map_err(EngineError::failed)
}

/// The factor e^{γd} port `port` de-embedded by d moves a wave by at `frequency` (Hz), γ its first
/// mode's; 1 without de-embedding.
pub(crate) fn deembedding(sw: &Sweep, port: usize, frequency: f64) -> Result<C> {
    let d = sw.deembed.get(port).copied().unwrap_or(0.0);
    if d == 0.0 {
        return Ok(C::new(1.0, 0.0));
    }
    let gamma = sw.driven.port_terms(port, frequency)?.first().map_or(C::new(0.0, 0.0), |t| t.gamma);
    Ok((gamma * d).exp())
}

/// The sweep's result at frequency `frequency`, which must have been solved.
fn solved_at<'s>(s: &'s Session, p: &Params) -> Result<&'s emcraft_solver::FrequencyResult> {
    let f = p.frequency("frequency").required()?;
    s.results
        .sweep()?
        .at(f, false)
        .ok_or_else(|| p.error("frequency", format!("{} {} was not solved", s.show_frequency(f), s.display.frequency.symbol())))
}

pub fn field_error(s: &mut Session, p: &Params) -> Result<Value> {
    let r = solved_at(s, p)?;
    let driven = &s.results.sweep()?.driven;
    let x = r.fields.first().ok_or_else(|| EngineError::failed("no field"))?;
    let exact = view::te10_line(s)?.at(r.frequency).solve(r.frequency)?;
    let (e, en) = driven.l2_error(x, r.frequency, emcraft_solver::Field::E, |q| exact.e(q))?;
    let (h, hn) = driven.l2_error(x, r.frequency, emcraft_solver::Field::H, |q| exact.h(q))?;
    Ok(json!({"frequency": s.show_frequency(r.frequency), "e": e / en, "h": h / hn}))
}

/// Most points one probe reads.
const MAX_PROBES: usize = 100_000;

pub fn probe(s: &mut Session, p: &Params) -> Result<Value> {
    let q = args::excitation(s, p)?;
    let r = solved_at(s, p)?;
    let driven = &s.results.sweep()?.driven;
    let x = r.fields.get(q).ok_or_else(|| EngineError::failed("no field for that port"))?;
    let parts = |v: CVec3| v.iter().map(|c| json!([c.re, c.im])).collect::<Vec<_>>();
    let out = p
        .list("at", MAX_PROBES)
        .required()?
        .iter()
        .map(|pt| {
            let (e, h) = driven.fields(x, r.frequency, p.point_of("at", pt)?).ok_or_else(|| p.error("at", format!("{pt} is outside the mesh")))?;
            Ok(json!({"e": parts(e), "h": parts(h)}))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"frequency": s.show_frequency(r.frequency), "points": out}))
}

/// The reflection at port `k` over its frequencies: the frequency sweep's, else the time
/// domain's (port 1 only).
fn reflection(s: &Session, k: usize) -> Result<(Vec<f64>, Vec<emcraft_solver::Complex64>)> {
    let (data, what) = match (s.results.sweep.as_ref().filter(|sw| !sw.results.is_empty()), s.results.time_domain.as_ref()) {
        (Some(sw), _) => (sw.sdata().map_err(EngineError::failed)?, "the sweep"),
        (None, Some(td)) => (crate::sdata::of_time_domain(td).map_err(EngineError::failed)?, "the time domain's results"),
        (None, None) => return Err(EngineError::NotSolved { what: "sweep", run: "solve.frequency or solve.time_domain" }),
    };
    let (f, r): (Vec<f64>, Vec<_>) = data.curve(k, k).unzip();
    if f.len() != data.frequencies().len() || f.is_empty() {
        return Err(EngineError::failed(format!("{what} hold no reflection at port {}", k + 1)));
    }
    Ok((f, r))
}

/// The transform of port `port`'s reflection (the first port by default), in `mode` (low pass
/// on a harmonic sweep by default, else band pass); with the port's name and reference
/// impedance.
pub(crate) fn tdr_of(s: &Session, p: &Params) -> Result<(crate::tdr::Tdr, String, Option<f64>)> {
    let (k, port) = match args::entry(s, p, "port")? {
        Some(named) => named,
        None => s.sparameter_entries().into_iter().enumerate().next().ok_or_else(|| EngineError::failed("no ports"))?,
    };
    let (freqs, refl) = reflection(s, k)?;
    let mode = match p.choice::<crate::tdr::Mode>("mode").optional()? {
        Some(m) => m,
        None if crate::tdr::spacing(&freqs).is_some_and(|(_, harmonic)| harmonic) => crate::tdr::Mode::LowPass,
        None => crate::tdr::Mode::BandPass,
    };
    Ok((crate::tdr::transform(&freqs, &refl, mode)?, port.name.clone(), port.reference))
}

/// Most points a TDR reply lists.
const TDR_POINTS: usize = 400;

pub fn tdr(s: &mut Session, p: &Params) -> Result<Value> {
    let (t, port, z0) = tdr_of(s, p)?;
    let step = t.times.len().div_ceil(TDR_POINTS).max(1);
    let points: Vec<Value> = t
        .times
        .iter()
        .zip(&t.values)
        .step_by(step)
        .map(|(time, v)| {
            let mut pt = json!({"t": time * 1e9, "value": v});
            if let (Some(z), crate::tdr::Mode::LowPass) = (z0, t.mode) {
                pt["impedance"] = json!(z * (1.0 + v) / (1.0 - v));
            }
            pt
        })
        .collect();
    Ok(json!({"port": port, "mode": t.mode.id(), "time_unit": "ns", "resolution": t.resolution * 1e9, "reference": z0, "points": points}))
}

/// Below this V₀²/(ωU) (Ω) a mode has no field along the line to speak of.
const MIN_FIELD_R_OVER_Q: f64 = 1e-3;

/// The most intervals `results.cavity` integrates along its line in.
const MAX_CAVITY_POINTS: usize = 20_000;

/// `results.cavity`: an eigenmode as an accelerating cavity — along a straight line (its
/// region's axis along z by default), the voltage a particle crossing at speed βc sees,
/// V = |∫ E_z e^{jωz/(βc)} dz|, against the field's own, V₀ = |∫ E_z dz|; the transit-time
/// factor T = V/V₀; the stored energy U = ½ ε₀ ∫ εr |E|² dV (the images' through symmetry
/// planes counted); R/Q = V²/(ωU) (the linac convention) and, with lossy walls, the shunt
/// impedance R = (R/Q) Q₀ (T. P. Wangler, *RF Linear Accelerators*, 2nd ed., Wiley 2008,
/// §2.2–2.3). The mode's amplitude is arbitrary: the voltages are given for 1 J stored.
pub fn cavity(s: &mut Session, p: &Params) -> Result<Value> {
    let e = s.results.eigen.as_ref().ok_or(EngineError::NotSolved { what: "eigenmodes", run: "solve.eigenmodes" })?;
    let index = p.integer("mode", 1..=e.modes.len().max(1)).or(1)?;
    let m = e.modes.get(index - 1).ok_or_else(|| p.error("mode", format!("must be 1 to {}", e.modes.len())))?;
    if s.model.domain.rotation.is_some() || s.model.domain.periodic.is_some() {
        return Err(EngineError::failed("not supported yet: a cavity's voltage on a rotational sector or a periodic cell"));
    }
    let beta = p.number("beta").or(1.0)?;
    if !(beta > 0.0 && beta <= 1.0) {
        return Err(p.error("beta", "must be greater than 0 and at most 1 (v/c)"));
    }
    let n = p.integer("points", 1..=MAX_CAVITY_POINTS).or(400)?;
    let line = match (p.point("from").optional()?, p.point("to").optional()?) {
        (Some(a), Some(b)) => (a, b),
        (None, None) => {
            let b = s.model.bounds().ok_or_else(|| EngineError::failed("no geometry"))?;
            let (cx, cy) = (0.5 * (b.min.x + b.max.x), 0.5 * (b.min.y + b.max.y));
            (emcraft_model::Point3::new(cx, cy, b.min.z), emcraft_model::Point3::new(cx, cy, b.max.z))
        }
        _ => return Err(p.error("to", "`from` and `to` go together (or neither: the region's axis along z)")),
    };
    let driven = e.driven_of(m);
    let (x, f) = (&m.vector, m.frequency);
    let omega = 2.0 * std::f64::consts::PI * f;
    let k = omega / (beta * emcraft_model::units::C0);
    let field = driven.line_integral(x, f, line, n, |_| C::new(1.0, 0.0))?;
    let felt = driven.line_integral(x, f, line, n, |z| C::from_polar(1.0, k * z))?;
    // Each symmetry plane halves the region solved; its image holds as much energy again.
    let images = 2f64.powi(i32::try_from(s.model.domain.symmetry.len()).unwrap_or(0));
    let energy = driven.stored_energy(x, f)? * images;
    if !(energy > 0.0 && energy.is_finite()) {
        return Err(EngineError::failed("the mode stores no energy"));
    }
    let (v0, v) = (field.norm(), felt.norm());
    let r_over_q = v * v / (omega * energy);
    let q = m.q();
    let per_joule = 1.0 / energy.sqrt();
    let show = |pt: emcraft_model::Point3| json!([s.show_length(pt.x), s.show_length(pt.y), s.show_length(pt.z)]);
    Ok(json!({
        "mode": index,
        "frequency": s.show_frequency(f),
        "from": show(line.0),
        "to": show(line.1),
        "beta": beta,
        "voltage": v * per_joule,
        "field_voltage": v0 * per_joule,
        // A mode with (next to) no field along the line (a TE mode on the axis) has no
        // transit time: V and V₀ are both its numerical noise there.
        "transit": (v0 * v0 / (omega * energy) >= MIN_FIELD_R_OVER_Q).then(|| v / v0),
        "r_over_q": r_over_q,
        "q": q,
        "shunt": q.map(|q| r_over_q * q),
    }))
}

/// The session's S-parameters as a set: its sweep's, else its body of revolution's, else its
/// time domain's; and which it is.
pub(crate) fn sdata(s: &Session) -> Result<(crate::sdata::SData, &'static str)> {
    if let Some(sw) = s.results.sweep.as_ref().filter(|sw| !sw.results.is_empty()) {
        return Ok((sw.sdata().map_err(EngineError::failed)?, "sweep"));
    }
    if let Some(b) = s.results.revolution.as_ref() {
        return Ok((b.data.clone(), "revolution"));
    }
    if let Some(td) = s.results.time_domain.as_ref() {
        return Ok((crate::sdata::of_time_domain(td).map_err(EngineError::failed)?, "time_domain"));
    }
    Err(EngineError::NotSolved { what: "sweep", run: "solve.frequency, solve.revolution.frequency or solve.time_domain" })
}

/// The most points `results.pulse` lists of each envelope.
const PULSE_POINTS: usize = 512;

/// `results.pulse`: a chirped pulse through the two-port ([`crate::pulse`]).
pub fn pulse(s: &mut Session, p: &Params) -> Result<Value> {
    let (data, source) = sdata(s)?;
    let entries = s.sparameter_entries();
    let from = args::entry(s, p, "from")?.map_or(0, |(k, _)| k);
    let to = args::entry(s, p, "to")?.map_or_else(|| usize::from(from == 0), |(k, _)| k);
    if from == to {
        return Err(p.error("to", "must be another port than `from`: the pulse is transmitted"));
    }
    let (freqs, trans): (Vec<f64>, Vec<_>) = data.curve(to, from).unzip();
    if freqs.len() != data.frequencies().len() {
        return Err(EngineError::failed(format!("the {source} holds no transmission from port {} to port {}", from + 1, to + 1)));
    }
    let (lo, hi) = (freqs.first().copied().unwrap_or(0.0), freqs.last().copied().unwrap_or(0.0));
    let f0 = p.frequency("frequency").optional()?.unwrap_or(0.5 * (lo + hi));
    let width = p.number("width").required()?;
    if !(width.is_finite() && width > 0.0 && width < 1e6) {
        return Err(p.error("width", "must be a positive number of ns"));
    }
    let width = width * 1e-9;
    // The chirp: the one that compresses best unless one is given.
    let fu = s.display.frequency;
    let rate = match p.number("bandwidth").optional()? {
        Some(b) if b.is_finite() && fu.to_si(b.abs()) < 1e15 => fu.to_si(b) / width,
        Some(_) => return Err(p.error("bandwidth", "must be a finite frequency")),
        None => {
            let flat = crate::pulse::through(&freqs, &trans, crate::pulse::Chirp { f0, width, rate: 0.0 })?;
            flat.best_rate().ok_or_else(|| EngineError::failed("the guide does not disperse at the carrier: give `bandwidth`"))?
        }
    };
    let r = crate::pulse::through(&freqs, &trans, crate::pulse::Chirp { f0, width, rate })?;
    let step = r.times.len().div_ceil(PULSE_POINTS).max(1);
    let points: Vec<Value> =
        (0..r.times.len()).step_by(step).map(|i| json!({"t": r.times[i] * 1e9, "input": r.input[i], "output": r.output[i]})).collect();
    let name = |k: usize| entries.get(k).map_or_else(|| (k + 1).to_string(), |e| e.name.clone());
    Ok(json!({
        "source": source,
        "from": name(from),
        "to": name(to),
        "frequency": fu.from_si(f0),
        "bandwidth": fu.from_si(rate * width),
        "best_bandwidth": r.best_rate().map(|b| fu.from_si(b * width)),
        "compression": r.compression(),
        "input_width": r.input_width * 1e9,
        "output_width": r.output_width * 1e9,
        "output_time": r.output_time * 1e9,
        "delay": r.delay * 1e9,
        // ns of delay per display unit of frequency.
        "dispersion": r.dispersion * 1e9 * fu.to_si(1.0),
        "outside": r.outside,
        "time_unit": "ns",
        "points": points,
    }))
}

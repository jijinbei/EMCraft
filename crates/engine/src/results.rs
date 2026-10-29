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
    if !m.symmetry.is_empty() {
        return Vec::new();
    }
    if let Some(w) = m.waveguide.as_ref()
        && w.sections.len() == 1
        && w.uniform()
        && let Ok((_, mat)) = m.material(&w.sections[0].material)
    {
        return emcraft_theory::cavity_resonances(w.width, w.height, w.length(), mat.eps_r * mat.mu_r, 12);
    }
    if let Some(solid) = m.solid.as_ref()
        && m.inclusions.is_empty()
        && m.features.is_empty()
        && let Ok((_, mat)) = m.material(&solid.material)
    {
        let er = mat.eps_r * mat.mu_r;
        return match solid.shape {
            Shape::Box { size } => emcraft_theory::cavity_resonances(size[0], size[1], size[2], er, 12),
            Shape::Cylinder { radius, length } => emcraft_theory::bessel::cylinder_cavity_resonances(radius, length, er, 12),
            Shape::Sphere { radius } => emcraft_theory::bessel::sphere_cavity_resonances(radius, er, 12),
            Shape::Surface { .. } => Vec::new(),
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
            Ok(json!({
                "frequency": s.show_frequency(r.frequency),
                "s11": complex(f11), "s21": complex(f21),
                "exact_s11": complex(s11), "exact_s21": complex(s21),
                "error_s11": e11, "error_s21": e21,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"reference": reference.label(), "points": rows, "worst": worst}))
}

/// The factor e^{γd} port `port` de-embedded by d moves a wave by at `frequency` (Hz), γ its first
/// mode's; 1 without de-embedding.
fn deembedding(sw: &Sweep, port: usize, frequency: f64) -> Result<C> {
    let d = sw.driven.ports.get(port).map_or(0.0, |x| x.deembed);
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
    if let Some(sw) = s.results.sweep.as_ref().filter(|sw| !sw.results.is_empty()) {
        let pairs: Option<Vec<_>> = sw.results.iter().map(|r| r.s.get(k, k).map(|c| (r.frequency, c))).collect();
        let pairs = pairs.ok_or_else(|| EngineError::Failed(format!("the sweep has no reflection at port {}", k + 1)))?;
        return Ok(pairs.into_iter().unzip());
    }
    match s.results.time_domain.as_ref() {
        Some(td) if k == 0 => Ok((td.frequencies.clone(), td.s11.clone())),
        Some(_) => Err(EngineError::Failed("the time domain's results reflect at port 1 only".into())),
        None => Err(EngineError::NotSolved { what: "sweep", run: "solve.frequency or solve.time_domain" }),
    }
}

/// The transform of port `port`'s reflection (the first port by default), in `mode` (low pass
/// on a harmonic sweep by default, else band pass); with the port's name and reference
/// impedance.
pub(crate) fn tdr_of(s: &Session, p: &Params) -> Result<(crate::tdr::Tdr, String, Option<f64>)> {
    let (k, port) = match args::entry(s, p, "port")? {
        Some(named) => named,
        None => s.sparameter_entries().into_iter().enumerate().next().ok_or_else(|| EngineError::Failed("no ports".into()))?,
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
    Ok(json!({"port": port, "mode": t.mode.name(), "time_unit": "ns", "resolution": t.resolution * 1e9, "reference": z0, "points": points}))
}

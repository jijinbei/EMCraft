//! Commands that read results: S-parameters, eigenmodes, the comparison with a reference,
//! field errors and probes, and pictures of fields and S-parameters.

use emcraft_model::units::decibels;
use emcraft_model::{CVec3, Shape};
use emcraft_solver::Complex64 as C;
use serde_json::{Map, Value, json};

use crate::view::{self, FieldView, Plane};
use crate::{EIGEN_MAX, EngineError, Params, Result, Session};

/// A complex value in a reply: parts, magnitude (also in dB) and phase in degrees.
pub(crate) fn complex(c: C) -> Value {
    json!({"re": c.re, "im": c.im, "mag": c.norm(), "db": decibels(c.norm()), "deg": c.arg().to_degrees()})
}

pub fn eigenmodes(s: &mut Session, _: &Params) -> Result<Value> {
    eigenmode_table(s)
}

/// The eigenmodes solved, with the closed form's for a primitive cavity.
pub(crate) fn eigenmode_table(s: &Session) -> Result<Value> {
    let e = s.results.eigen.as_ref().ok_or_else(|| EngineError::failed("no eigenmodes: solve first (solve.eigenmodes)"))?;
    let modes: Vec<Value> = e
        .modes
        .iter()
        .enumerate()
        .map(|(i, m)| json!({"mode": i + 1, "frequency": s.show_frequency(m.frequency), "q": m.q(), "q_walls": m.q_walls, "q_dielectric": m.q_dielectric}))
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
    let names: Vec<&str> = s.model.ports.iter().map(|p| p.name.as_str()).collect();
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
    Ok(json!({"ports": names, "frequency_unit": s.display.frequency.symbol(), "points": rows}))
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
    let exact = view::te10_line(s)?.solve(r.frequency)?;
    let (e, en) = driven.l2_error(x, r.frequency, emcraft_solver::Field::E, |q| exact.e(q))?;
    let (h, hn) = driven.l2_error(x, r.frequency, emcraft_solver::Field::H, |q| exact.h(q))?;
    Ok(json!({"frequency": s.show_frequency(r.frequency), "e": e / en, "h": h / hn}))
}

/// The port `excitation` names (the first by default).
fn excitation(s: &Session, p: &Params) -> Result<usize> {
    p.string("excitation").optional()?.map_or(Ok(0), |name| Ok(s.model.port(name)?.0))
}

/// Most points one probe reads.
const MAX_PROBES: usize = 100_000;

pub fn probe(s: &mut Session, p: &Params) -> Result<Value> {
    let q = excitation(s, p)?;
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

pub(crate) fn write_png(path: &str, png: Option<Vec<u8>>) -> Result<()> {
    if path.trim().is_empty() {
        return Err(EngineError::Params("`path` must not be empty".into()));
    }
    let bytes = png.ok_or_else(|| EngineError::failed("could not encode the picture"))?;
    std::fs::write(path, bytes).map_err(|e| EngineError::Failed(format!("cannot write {path}: {e}")))
}

/// A field view from command parameters.
pub fn field_view(s: &Session, p: &Params) -> Result<FieldView> {
    let plane = p.choice("plane").or(Plane::H)?;
    let mut v = match p.integer("mode", 1..=EIGEN_MAX).optional()? {
        Some(m) => FieldView::mode(m - 1, plane),
        None => FieldView::new(p.frequency("frequency").required_or("or `mode`")?, plane),
    };
    v.at = p.length("at").optional()?;
    if let Some(c) = p.string("component").optional()? {
        v.component = c.parse()?;
    }
    if let Some(list) = p.list("arrows", 16).optional()? {
        v.show_e = list.iter().any(|x| x.as_str() == Some("e"));
        v.show_h = list.iter().any(|x| x.as_str() == Some("h"));
    }
    v.phase = p.number("phase").or(0.0)?.to_radians();
    v.excitation = excitation(s, p)?;
    Ok(v)
}

pub fn view_field(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let width = p.side("width", s.limits.picture_side, 1100)?;
    let plot = view::field_plot(s, &field_view(s, p)?)?;
    let c = emcraft_render::plot::draw(&plot, width);
    write_png(path, c.png())?;
    Ok(json!({"path": path, "width": c.w, "height": c.h}))
}

pub fn view_sparameters(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let (w, h) = (p.side("width", s.limits.picture_side, 1000)?, p.side("height", s.limits.picture_side, 600)?);
    let c = emcraft_render::chart::draw(&view::sparameter_chart(s)?, w, h);
    write_png(path, c.png())?;
    Ok(json!({"path": path, "width": c.w, "height": c.h}))
}

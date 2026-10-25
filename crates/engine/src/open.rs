//! Commands of open problems: the perfectly matched layer, sources, the radiation solve, far
//! fields and pattern charts.

use std::f64::consts::PI;
use std::sync::Arc;

use emcraft_fem::Order;
use emcraft_model::units::{C0, MU0};
use emcraft_model::{Open, Shape, Source};
use emcraft_render::Rgb;
use emcraft_render::axes::Axis;
use emcraft_render::chart::{Chart, Series, Style};
use emcraft_solver::radiation::{FarField, OpenResult};
use serde_json::{Value, json};

use crate::cmd::write_png;
use crate::params::{get, integer, number, required_string};
use crate::{EngineError, Result, Session};

/// An open problem solved at one frequency.
#[derive(Debug)]
pub struct Radiated {
    pub driven: emcraft_solver::Driven,
    pub result: OpenResult,
    /// The power (W) through the Huygens surface: radiated, or scattered for a plane wave.
    pub power: f64,
}

fn vector(p: &Value, name: &str) -> Result<Option<[f64; 3]>> {
    let Some(v) = get(p, name) else { return Ok(None) };
    let list = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| EngineError::Params(format!("`{name}` must be [x, y, z]")))?;
    let mut out = [0.0; 3];
    for (d, v) in out.iter_mut().zip(list) {
        *d = v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| EngineError::Params(format!("`{name}` must be numbers")))?;
    }
    Ok(Some(out))
}

pub fn model_open(s: &mut Session, p: &Value) -> Result<Value> {
    let open = s.positive_length(p, "thickness")?.map(|thickness| Open { thickness });
    s.model.set_open(open)?;
    s.radiation = None;
    Ok(json!({"open": open.is_some(), "thickness": open.map(|o| s.display.length.from_si(o.thickness))}))
}

pub fn source_dipole(s: &mut Session, p: &Value) -> Result<Value> {
    let at = vector(p, "at")?.ok_or_else(|| EngineError::Params("`at` is required".into()))?.map(|v| s.display.length.to_si(v));
    let direction = vector(p, "direction")?.unwrap_or([0.0, 0.0, 1.0]);
    let length = s.positive_length(p, "length")?.ok_or_else(|| EngineError::Params("`length` is required".into()))?;
    let current = number(p, "current")?.unwrap_or(1.0);
    s.model.add_source(Source::Dipole { at, direction, length, current })?;
    s.radiation = None;
    Ok(json!({"sources": s.model.sources.len()}))
}

pub fn source_plane_wave(s: &mut Session, p: &Value) -> Result<Value> {
    let direction = vector(p, "direction")?.unwrap_or([0.0, 0.0, 1.0]);
    let polarization = vector(p, "polarization")?.unwrap_or([1.0, 0.0, 0.0]);
    s.model.add_source(Source::PlaneWave { direction, polarization })?;
    s.radiation = None;
    Ok(json!({"sources": s.model.sources.len()}))
}

pub fn port_lumped(s: &mut Session, p: &Value) -> Result<Value> {
    let name = required_string(p, "name")?.to_owned();
    let from = vector(p, "from")?.ok_or_else(|| EngineError::Params("`from` is required".into()))?.map(|v| s.display.length.to_si(v));
    let to = vector(p, "to")?.ok_or_else(|| EngineError::Params("`to` is required".into()))?.map(|v| s.display.length.to_si(v));
    let impedance = number(p, "impedance")?.unwrap_or(50.0);
    let amplitude = number(p, "voltage")?.unwrap_or(1.0);
    let phase = number(p, "phase")?.unwrap_or(0.0).to_radians();
    if !(amplitude.is_finite() && phase.is_finite()) {
        return Err(EngineError::Params("`voltage` and `phase` must be finite".into()));
    }
    let radius = s.positive_length(p, "radius")?.unwrap_or(0.0);
    s.model.add_lumped_port(emcraft_model::LumpedPort {
        name,
        from,
        to,
        impedance,
        voltage: [amplitude * phase.cos(), amplitude * phase.sin()],
        radius,
    })?;
    s.radiation = None;
    Ok(json!({"lumped_ports": s.model.lumped.len()}))
}

/// A centre-fed wire dipole the induced-EMF closed form applies to: two coaxial conducting
/// cylinders of one radius with the model's one lumped port across the gap between them, in
/// vacuum: (total length, radius).
fn wire_dipole(s: &Session) -> Option<(f64, f64)> {
    let vacuum = s.model.solid.as_ref().is_some_and(|x| s.model.material(&x.material).is_ok_and(|(_, m)| m.eps_r == 1.0 && m.mu_r == 1.0));
    let ([a, b], [port]) = (&s.model.inclusions[..], &s.model.lumped[..]) else { return None };
    if !vacuum || !s.model.sources.is_empty() || !s.model.features.is_empty() || a.material.is_some() || b.material.is_some() {
        return None;
    }
    let (Shape::Cylinder { radius: ra, length: la }, Shape::Cylinder { radius: rb, length: lb }) = (&a.shape, &b.shape) else { return None };
    let tol = 1e-9 * (la + lb);
    let coaxial = (a.at[0] - b.at[0]).abs() < tol && (a.at[1] - b.at[1]).abs() < tol && (ra - rb).abs() < tol;
    let on_axis = [port.from, port.to].iter().all(|q| (q[0] - a.at[0]).abs() < tol && (q[1] - a.at[1]).abs() < tol);
    if !(coaxial && on_axis) {
        return None;
    }
    let lo = a.at[2].min(b.at[2]);
    let hi = (a.at[2] + la).max(b.at[2] + lb);
    Some((hi - lo, *ra))
}

pub fn source_clear(s: &mut Session, _: &Value) -> Result<Value> {
    s.model.sources.clear();
    s.radiation = None;
    Ok(json!({"sources": 0}))
}

/// The plane wave driving the model, if one does.
fn plane_wave(s: &Session) -> Option<([f64; 3], [f64; 3])> {
    s.model.sources.iter().find_map(|x| match *x {
        Source::PlaneWave { direction, polarization } => Some((direction, polarization)),
        Source::Dipole { .. } => None,
    })
}

/// The one dipole in an empty box, whose closed form applies: (moment I·l, axis).
fn lone_dipole(s: &Session) -> Option<(f64, [f64; 3])> {
    let vacuum = s.model.solid.as_ref().is_some_and(|x| s.model.material(&x.material).is_ok_and(|(_, m)| m.eps_r == 1.0 && m.mu_r == 1.0));
    match s.model.sources[..] {
        [Source::Dipole { direction, length, current, .. }] if vacuum && s.model.inclusions.is_empty() && s.model.features.is_empty() => {
            Some((current * length, direction))
        }
        _ => None,
    }
}

/// The one sphere a plane wave lights, whose Mie series applies: (radius, εr), εr `None` for a
/// conductor.
fn lone_sphere(s: &Session) -> Option<(f64, Option<f64>)> {
    plane_wave(s)?;
    match &s.model.inclusions[..] {
        [inc] if s.model.features.is_empty() && s.model.sources.len() == 1 => match (&inc.shape, &inc.material) {
            (Shape::Sphere { radius }, Some(name)) => {
                let (_, m) = s.model.material(name).ok()?;
                (m.mu_r == 1.0 && m.loss_tangent == 0.0).then_some((*radius, Some(m.eps_r)))
            }
            (Shape::Sphere { radius }, None) => Some((*radius, None)),
            _ => None,
        },
        _ => None,
    }
}

pub fn solve_radiation(s: &mut Session, p: &Value) -> Result<Value> {
    let f = s.frequency(p, "frequency")?.ok_or_else(|| EngineError::Params("`frequency` is required".into()))?;
    let order = Order::from_u8(integer(p, "order", 1..=2)?.unwrap_or(2) as u8).unwrap_or(Order::Two);
    let driven = emcraft_solver::Driven::open(&s.model, order)?;
    let result = driven.solve_open(&s.model, f)?;
    let power = driven.radiated_power(&s.model, &result.field, f)?;
    let mut reply = json!({"frequency": s.show_frequency(f), "unknowns": driven.dofs.unknowns, "backward_error": result.backward_error});
    let eta = MU0 * C0;
    if plane_wave(s).is_some() {
        // σ_sca = P_sca / (|E₀|²/2η), in display units of area.
        let sigma = power * 2.0 * eta;
        let l = s.display.length.from_si(1.0);
        reply["scattering_cross_section"] = json!(sigma * l * l);
        if let Some((a, er)) = lone_sphere(s) {
            let x = 2.0 * PI * f / C0 * a;
            let q = emcraft_theory::radiation::mie_efficiencies(x, er.map(f64::sqrt)).0;
            reply["exact_scattering_cross_section"] = json!(q * PI * a * a * l * l);
        }
    } else {
        reply["radiated_power"] = json!(power);
        if let Some((il, _)) = lone_dipole(s) {
            reply["exact_radiated_power"] = json!(emcraft_theory::radiation::dipole_power(il, f));
        }
    }
    if !result.ports.is_empty() {
        let c = |z: emcraft_solver::Complex64| json!({"re": z.re, "im": z.im});
        let exact = wire_dipole(s).map(|(l, a)| emcraft_theory::radiation::dipole_impedance(l, a, f));
        let ports: Vec<Value> = result
            .ports
            .iter()
            .map(|pr| {
                let mut v = json!({
                    "name": pr.name, "impedance": c(pr.impedance), "current": c(pr.current), "voltage": c(pr.voltage),
                    "s11": {"re": pr.s11.re, "im": pr.s11.im, "mag": pr.s11.norm(), "db": emcraft_model::units::decibels(pr.s11.norm())},
                    "power": pr.power,
                });
                if let Some(z) = exact {
                    // The thin-wire, sinusoidal-current estimate: a guide, not a reference (a
                    // wire a sixtieth of a wavelength thick differs by tens of percent).
                    v["induced_emf_impedance"] = c(z);
                }
                v
            })
            .collect();
        let input: f64 = result.ports.iter().map(|pr| pr.power).sum();
        reply["ports"] = json!(ports);
        reply["input_power"] = json!(input);
        reply["radiation_efficiency"] = json!(if input > 0.0 { power / input } else { f64::NAN });
    }
    s.radiation = Some(Arc::new(Radiated { driven, result, power }));
    Ok(reply)
}

/// Directivity (dBi) for sources, radar cross-section (display area, and dB of m²) for a plane
/// wave, in one direction; with the closed form's where it applies.
fn describe(s: &Session, r: &Radiated, ff: &FarField) -> Value {
    let eta = MU0 * C0;
    let f = r.result.frequency;
    let k = 2.0 * PI * f / C0;
    let (st, ct, sp, cp) = (ff.theta.sin(), ff.theta.cos(), ff.phi.sin(), ff.phi.cos());
    let dir = [st * cp, st * sp, ct];
    let c = |z: emcraft_solver::Complex64| json!({"re": z.re, "im": z.im, "mag": z.norm()});
    let mut out = json!({"theta": ff.theta.to_degrees(), "phi": ff.phi.to_degrees(), "e_theta": c(ff.e_theta), "e_phi": c(ff.e_phi)});
    if let Some((kk, e)) = plane_wave(s) {
        let rcs = 4.0 * PI * ff.magnitude_squared();
        let l = s.display.length.from_si(1.0);
        out["rcs"] = json!(rcs * l * l);
        out["rcs_dbsm"] = json!(10.0 * rcs.log10());
        if let Some((a, er)) = lone_sphere(s) {
            // The scattering angle from the incident direction, and the azimuth from its
            // polarization in the plane across it.
            let cos_t = (kk[0] * dir[0] + kk[1] * dir[1] + kk[2] * dir[2]).clamp(-1.0, 1.0);
            let h = cross(kk, e);
            let (pe, ph) = (dir[0] * e[0] + dir[1] * e[1] + dir[2] * e[2], dir[0] * h[0] + dir[1] * h[1] + dir[2] * h[2]);
            let cphi2 = if pe * pe + ph * ph > 0.0 { pe * pe / (pe * pe + ph * ph) } else { 1.0 };
            let (s1, s2) = match er {
                Some(er) => emcraft_theory::radiation::mie_amplitudes(k * a, er.sqrt(), cos_t.acos()),
                None => emcraft_theory::radiation::mie_amplitudes_pec(k * a, cos_t.acos()),
            };
            let exact = 4.0 * PI * (s2.norm_sqr() * cphi2 + s1.norm_sqr() * (1.0 - cphi2)) / (k * k);
            out["exact_rcs"] = json!(exact * l * l);
        }
    } else {
        let d = 4.0 * PI * ff.magnitude_squared() / (2.0 * eta * r.power);
        out["directivity"] = json!(d);
        out["directivity_dbi"] = json!(10.0 * d.log10());
        if let Some((il, axis)) = lone_dipole(s) {
            let cos_a = (axis[0] * dir[0] + axis[1] * dir[1] + axis[2] * dir[2]).clamp(-1.0, 1.0);
            out["exact_far_field"] = json!(emcraft_theory::radiation::dipole_far_field(il, f, cos_a.acos()));
            out["exact_directivity"] = json!(1.5 * (1.0 - cos_a * cos_a));
        }
    }
    out
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn solved(s: &Session) -> Result<Arc<Radiated>> {
    s.radiation.clone().ok_or_else(|| EngineError::Failed("no open problem solved: run solve.radiation first".into()))
}

fn angles(p: &Value) -> Result<(Vec<f64>, f64)> {
    let theta: Vec<f64> = match get(p, "theta") {
        None => (0..=18).map(|i| 10.0 * i as f64).collect(),
        Some(Value::Array(list)) => list
            .iter()
            .map(|v| v.as_f64().filter(|x| x.is_finite()))
            .collect::<Option<_>>()
            .ok_or_else(|| EngineError::Params("`theta` must be numbers".into()))?,
        Some(v) => vec![v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| EngineError::Params("`theta` must be a number or a list".into()))?],
    };
    if theta.is_empty() || theta.len() > 3601 {
        return Err(EngineError::Params("`theta` needs 1 to 3601 angles".into()));
    }
    Ok((theta, number(p, "phi")?.unwrap_or(0.0)))
}

pub fn results_far_field(s: &mut Session, p: &Value) -> Result<Value> {
    let r = solved(s)?;
    let (theta, phi) = angles(p)?;
    let dirs: Vec<(f64, f64)> = theta.iter().map(|t| (t.to_radians(), phi.to_radians())).collect();
    let ff = r.driven.far_field(&s.model, &r.result.field, r.result.frequency, &dirs)?;
    let points: Vec<Value> = ff.iter().map(|x| describe(s, &r, x)).collect();
    Ok(json!({"frequency": s.show_frequency(r.result.frequency), "power": r.power, "points": points}))
}

/// The far-field pattern of the last radiation solve against θ at azimuth `phi` (degrees),
/// `points` angles from 0 to 180°: directivity for sources, radar cross-section for a plane
/// wave, over the closed form where it applies.
pub fn pattern_chart(s: &Session, phi: f64, points: usize) -> Result<Chart> {
    let r = solved(s)?;
    let n = points.clamp(13, 721);
    let dirs: Vec<(f64, f64)> = (0..n).map(|i| ((180.0 * i as f64 / (n - 1) as f64).to_radians(), phi.to_radians())).collect();
    let ff = r.driven.far_field(&s.model, &r.result.field, r.result.frequency, &dirs)?;
    let rows: Vec<Value> = ff.iter().map(|x| describe(s, &r, x)).collect();
    let scattering = plane_wave(s).is_some();
    let (key, exact_key, label) =
        if scattering { ("rcs_dbsm", "exact_rcs", "RCS (dBsm)") } else { ("directivity_dbi", "exact_directivity", "directivity (dBi)") };
    let l = s.display.length.from_si(1.0);
    let db = |v: &Value| v.as_f64().map(|x| if scattering { 10.0 * (x / (l * l)).log10() } else { 10.0 * x.log10() });
    let ours: Vec<[f64; 2]> = rows.iter().filter_map(|row| Some([row["theta"].as_f64()?, row[key].as_f64()?])).collect();
    let exact: Vec<[f64; 2]> = rows.iter().filter_map(|row| Some([row["theta"].as_f64()?, db(&row[exact_key])?])).collect();
    let all: Vec<f64> = ours.iter().chain(&exact).map(|q| q[1]).filter(|v| v.is_finite()).collect();
    let top = all.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let top = if top.is_finite() { (top / 5.0).ceil() * 5.0 } else { 0.0 };
    let mut series = Vec::new();
    if !exact.is_empty() {
        let name = if scattering { "Mie series" } else { "Hertzian dipole" };
        series.push(Series { label: name.into(), color: Rgb(120, 120, 120), style: Style::Line, points: exact });
    }
    series.push(Series { label: "FEM".into(), color: Rgb(200, 40, 40), style: Style::Markers, points: ours });
    let title = format!("Far field at {} {}, φ = {phi}°", s.show_frequency(r.result.frequency), s.display.frequency.symbol());
    Ok(Chart { title, x: Axis::new("θ (degrees)", 0.0, 180.0), y: Axis::new(label, top - 40.0, top), series, marks: Vec::new() })
}

pub fn view_pattern(s: &mut Session, p: &Value) -> Result<Value> {
    let path = required_string(p, "path")?;
    let phi = number(p, "phi")?.unwrap_or(0.0);
    let n = integer(p, "points", 13..=721)?.unwrap_or(91) as usize;
    let chart = pattern_chart(s, phi, n)?;
    let side = 200..=s.limits.picture_side as u64;
    let w = integer(p, "width", side.clone())?.unwrap_or(1000) as usize;
    let h = integer(p, "height", side)?.unwrap_or(600) as usize;
    let c = emcraft_render::chart::draw(&chart, w, h);
    write_png(path, c.png())?;
    Ok(json!({"path": path, "width": c.w, "height": c.h}))
}

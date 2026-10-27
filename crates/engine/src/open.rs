//! Commands of open problems: the perfectly matched layer, sources, the radiation solve and far
//! fields. Their pattern chart is `view`'s.

use std::f64::consts::PI;
use std::sync::Arc;

use emcraft_model::units::{C0, MU0};
use emcraft_model::{Open, OpenShape, Shape, Source, Vec3};
use emcraft_solver::radiation::{FarField, OpenResult};
use emcraft_theory::radiation::Huygens;
use serde_json::{Value, json};

use crate::{Change, EngineError, Params, Result, Session};

/// An open problem solved at one frequency.
#[derive(Debug)]
pub struct Radiated {
    pub driven: emcraft_solver::Driven,
    pub result: OpenResult,
    /// The fields on the Huygens box, sampled once for every far field asked of it.
    pub huygens: Huygens,
    /// The power (W) through the Huygens surface: radiated, or scattered for a plane wave.
    pub power: f64,
}

pub fn model_open(s: &mut Session, p: &Params) -> Result<Value> {
    let reflection = p.number("reflection").optional()?;
    let shape: OpenShape = p.choice("shape").or(OpenShape::Box)?;
    let open = p.positive_length("thickness").optional()?.map(|thickness| Open { thickness, reflection, shape });
    s.model.set_open(open)?;
    s.results.invalidate(Change::Open);
    Ok(json!({"open": open.is_some(), "thickness": open.map(|o| s.show_length(o.thickness)), "shape": open.map(|o| o.shape)}))
}

pub fn source_dipole(s: &mut Session, p: &Params) -> Result<Value> {
    let dipole = Source::Dipole {
        at: p.point("at").required()?,
        direction: p.vector("direction").or(Vec3::unit(2))?,
        length: p.positive_length("length").required()?,
        current: p.number("current").or(1.0)?,
    };
    s.model.add_source(dipole)?;
    s.results.invalidate(Change::Open);
    Ok(json!({"sources": s.model.sources.len()}))
}

pub fn source_plane_wave(s: &mut Session, p: &Params) -> Result<Value> {
    let wave = Source::PlaneWave { direction: p.vector("direction").or(Vec3::unit(2))?, polarization: p.vector("polarization").or(Vec3::unit(0))? };
    s.model.add_source(wave)?;
    s.results.invalidate(Change::Open);
    Ok(json!({"sources": s.model.sources.len()}))
}

pub fn port_lumped(s: &mut Session, p: &Params) -> Result<Value> {
    let amplitude = p.number("voltage").or(1.0)?;
    let phase = p.number("phase").or(0.0)?.to_radians();
    s.model.add_lumped_port(emcraft_model::LumpedPort {
        name: p.string("name").required()?.to_owned(),
        from: p.point("from").required()?,
        to: p.point("to").required()?,
        impedance: p.number("impedance").or(50.0)?,
        voltage: [amplitude * phase.cos(), amplitude * phase.sin()],
        radius: p.positive_length("radius").or(0.0)?,
    })?;
    s.results.invalidate(Change::Open);
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
    let on_axis = |q: emcraft_model::Point3| (q.x - a.at.x).abs() < tol && (q.y - a.at.y).abs() < tol;
    if !(on_axis(b.at) && (ra - rb).abs() < tol && on_axis(port.from) && on_axis(port.to)) {
        return None;
    }
    let lo = a.at.z.min(b.at.z);
    let hi = (a.at.z + la).max(b.at.z + lb);
    Some((hi - lo, *ra))
}

pub fn source_clear(s: &mut Session, _: &Params) -> Result<Value> {
    s.model.sources.clear();
    s.results.invalidate(Change::Open);
    Ok(json!({"sources": 0}))
}

/// The plane wave driving the model, if one does.
pub(crate) fn plane_wave(s: &Session) -> Option<(Vec3, Vec3)> {
    s.model.sources.iter().find_map(|x| match *x {
        Source::PlaneWave { direction, polarization } => Some((direction, polarization)),
        Source::Dipole { .. } => None,
    })
}

/// The one dipole in an empty box, whose closed form applies: (moment I·l, axis).
fn lone_dipole(s: &Session) -> Option<(f64, Vec3)> {
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

pub fn solve_radiation(s: &mut Session, p: &Params) -> Result<Value> {
    let f = p.frequency("frequency").required()?;
    let order = p.order()?;
    let driven = emcraft_solver::Driven::open(&s.model, order)?;
    let result = driven.solve_open(&s.model, f)?;
    let huygens = driven.huygens(&s.model, &result.field, f)?;
    let power = huygens.power();
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
    if let Some((s11, accepted)) = result.feed {
        reply["feed"] = json!({
            "port": s.model.ports.first().map(|p| p.name.clone()),
            "s11": {"re": s11.re, "im": s11.im, "mag": s11.norm(), "db": emcraft_model::units::decibels(s11.norm())},
            "accepted_power": accepted,
        });
        reply["radiation_efficiency"] = json!(if accepted > 0.0 { power / accepted } else { f64::NAN });
    }
    s.results.radiation = Some(Arc::new(Radiated { driven, result, huygens, power }));
    Ok(reply)
}

/// The closed form a far field is compared with, prepared once for all its directions.
enum Exact {
    /// A sphere a plane wave lights: k, the incidence and polarization, its Mie coefficients.
    Mie {
        k: f64,
        incidence: Vec3,
        polarization: Vec3,
        coefficients: Vec<(emcraft_solver::Complex64, emcraft_solver::Complex64)>,
    },
    /// The one dipole in an empty box: moment I·l and axis.
    Dipole {
        moment: f64,
        axis: Vec3,
    },
    None,
}

impl Exact {
    fn of(s: &Session, frequency: f64) -> Exact {
        let k = 2.0 * PI * frequency / C0;
        if let Some((incidence, polarization)) = plane_wave(s) {
            return match lone_sphere(s) {
                Some((a, Some(er))) => {
                    Exact::Mie { k, incidence, polarization, coefficients: emcraft_theory::radiation::mie_coefficients(k * a, er.sqrt()) }
                }
                Some((a, None)) => Exact::Mie { k, incidence, polarization, coefficients: emcraft_theory::radiation::mie_coefficients_pec(k * a) },
                None => Exact::None,
            };
        }
        lone_dipole(s).map_or(Exact::None, |(moment, axis)| Exact::Dipole { moment, axis })
    }
}

/// Directivity (dBi) for sources, radar cross-section (display area, and dB of m²) for a plane
/// wave, in one direction; with the closed form's where it applies.
fn describe(s: &Session, r: &Radiated, exact: &Exact, ff: &FarField) -> Value {
    let eta = MU0 * C0;
    let f = r.result.frequency;
    let [dir, _, _] = Vec3::spherical(ff.theta, ff.phi);
    let c = |z: emcraft_solver::Complex64| json!({"re": z.re, "im": z.im, "mag": z.norm()});
    let mut out = json!({"theta": ff.theta.to_degrees(), "phi": ff.phi.to_degrees(), "e_theta": c(ff.e_theta), "e_phi": c(ff.e_phi)});
    let l = s.display.length.from_si(1.0);
    if plane_wave(s).is_some() {
        let rcs = 4.0 * PI * ff.magnitude_squared();
        out["rcs"] = json!(rcs * l * l);
        out["rcs_dbsm"] = json!(10.0 * rcs.log10());
    } else {
        let d = 4.0 * PI * ff.magnitude_squared() / (2.0 * eta * r.power);
        out["directivity"] = json!(d);
        out["directivity_dbi"] = json!(10.0 * d.log10());
    }
    match exact {
        Exact::Mie { k, incidence, polarization, coefficients } => {
            // The scattering angle from the incident direction, and the azimuth from its
            // polarization in the plane across it.
            let cos_t = incidence.dot(dir).clamp(-1.0, 1.0);
            let (pe, ph) = (dir.dot(*polarization), dir.dot(incidence.cross(*polarization)));
            let cphi2 = if pe * pe + ph * ph > 0.0 { pe * pe / (pe * pe + ph * ph) } else { 1.0 };
            let (s1, s2) = emcraft_theory::radiation::amplitudes(coefficients, cos_t.acos());
            let rcs = 4.0 * PI * (s2.norm_sqr() * cphi2 + s1.norm_sqr() * (1.0 - cphi2)) / (k * k);
            out["exact_rcs"] = json!(rcs * l * l);
        }
        Exact::Dipole { moment, axis } => {
            let cos_a = axis.dot(dir).clamp(-1.0, 1.0);
            out["exact_far_field"] = json!(emcraft_theory::radiation::dipole_far_field(*moment, f, cos_a.acos()));
            out["exact_directivity"] = json!(1.5 * (1.0 - cos_a * cos_a));
        }
        Exact::None => {}
    }
    out
}

fn solved(s: &Session) -> Result<Arc<Radiated>> {
    s.results.radiation.clone().ok_or_else(|| EngineError::Failed("no open problem solved: run solve.radiation first".into()))
}

/// The polar angles `theta` (degrees; every 10° by default) and the azimuth `phi`.
fn angles(p: &Params) -> Result<(Vec<f64>, f64)> {
    let theta: Vec<f64> = match p.raw("theta") {
        None => (0..=18).map(|i| 10.0 * f64::from(i)).collect(),
        Some(Value::Array(list)) => {
            list.iter().map(|v| p.number_of("theta", v).ok()).collect::<Option<_>>().ok_or_else(|| p.error("theta", "must be numbers"))?
        }
        Some(_) => vec![p.number("theta").required()?],
    };
    if theta.is_empty() || theta.len() > 3601 {
        return Err(p.error("theta", "needs 1 to 3601 angles"));
    }
    Ok((theta, p.number("phi").or(0.0)?))
}

/// Which solver's far field to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FarSolver {
    /// The frequency domain's (`solve.radiation`).
    Fem,
    /// The time domain's (`solve.time_domain` on an open model).
    TimeDomain,
}

/// Far-field rows in the directions given (radians), from the frequency-domain solve or, when
/// asked (`solver` "time_domain") or the only one there, the time domain's at the frequency
/// nearest `frequency` (Hz): (frequency, power, rows).
pub(crate) fn far_rows(s: &Session, dirs: &[(f64, f64)], solver: Option<FarSolver>, frequency: Option<f64>) -> Result<(f64, f64, Vec<Value>)> {
    let use_td = match solver {
        Some(solver) => solver == FarSolver::TimeDomain,
        None => s.results.radiation.is_none() && s.results.td_radiation.is_some(),
    };
    if !use_td {
        let r = solved(s)?;
        let exact = Exact::of(s, r.result.frequency);
        let rows = emcraft_solver::radiation::far_fields(&r.huygens, dirs).iter().map(|x| describe(s, &r, &exact, x)).collect();
        return Ok((r.result.frequency, r.power, rows));
    }
    let r = s
        .results
        .td_radiation
        .clone()
        .ok_or_else(|| EngineError::Failed("no time-domain open problem solved: run solve.time_domain first".into()))?;
    let fi = frequency.and_then(|f| crate::numbers::nearest(&r.frequencies, f)).unwrap_or(0);
    let f = r.frequencies.get(fi).copied().ok_or_else(|| EngineError::Failed("no frequencies".into()))?;
    let power = r.radiated.get(fi).copied().unwrap_or(f64::NAN);
    let lone = lone_dipole(s);
    let rows = dirs
        .iter()
        .map(|&(theta, phi)| {
            let (et, ep) = r.far_field(fi, theta, phi).unwrap_or_default();
            let d = r.directivity(fi, theta, phi).unwrap_or(f64::NAN);
            let c = |z: emcraft_solver::Complex64| json!({"re": z.re, "im": z.im, "mag": z.norm()});
            let mut v = json!({"theta": theta.to_degrees(), "phi": phi.to_degrees(), "e_theta": c(et), "e_phi": c(ep), "directivity": d, "directivity_dbi": 10.0 * d.log10()});
            if let Some((_, axis)) = lone {
                let cos_a = axis.dot(Vec3::spherical(theta, phi)[0]).clamp(-1.0, 1.0);
                v["exact_directivity"] = json!(1.5 * (1.0 - cos_a * cos_a));
            }
            v
        })
        .collect();
    Ok((f, power, rows))
}

pub fn results_far_field(s: &mut Session, p: &Params) -> Result<Value> {
    let (theta, phi) = angles(p)?;
    let dirs: Vec<(f64, f64)> = theta.iter().map(|t| (t.to_radians(), phi.to_radians())).collect();
    let (f, power, points) = far_rows(s, &dirs, p.choice("solver").optional()?, p.frequency("frequency").optional()?)?;
    Ok(json!({"frequency": s.show_frequency(f), "power": power, "points": points}))
}

pub use crate::view::{pattern_chart, pattern_chart_of};

/// The time domain's open problem: lumped ports and dipoles on a Yee grid with a CPML.
pub fn solve_time_domain_open(s: &mut Session, freqs: &[f64], cell: f64, device: Option<&dyn emcraft_fdtd::OpenStepper>) -> Result<Value> {
    let r = emcraft_fdtd::open::radiate_with(&s.model, &emcraft_fdtd::open::OpenOptions::new(cell), freqs, device)?;
    let c = |z: emcraft_solver::Complex64| json!({"re": z.re, "im": z.im, "mag": z.norm()});
    let points: Vec<Value> = r
        .frequencies
        .iter()
        .enumerate()
        .map(|(fi, f)| {
            let ports: Vec<Value> = r
                .ports
                .iter()
                .zip(&s.model.lumped)
                .map(|(pr, lp)| {
                    let z = pr.impedance[fi];
                    let s11 = (z - lp.impedance) / (z + lp.impedance);
                    json!({"name": pr.name, "impedance": c(z), "voltage": c(pr.voltage[fi]), "current": c(pr.current[fi]), "s11": {"re": s11.re, "im": s11.im, "mag": s11.norm(), "db": emcraft_model::units::decibels(s11.norm())}, "power": pr.power[fi]})
                })
                .collect();
            let input: f64 = r.ports.iter().map(|pr| pr.power[fi]).sum();
            let mut v = json!({"frequency": s.show_frequency(*f), "radiated_power": r.radiated[fi], "ports": ports});
            if input > 0.0 {
                v["radiation_efficiency"] = json!(r.radiated[fi] / input);
            }
            v
        })
        .collect();
    let reply =
        json!({"cells": r.cells, "steps": r.steps, "converged": r.converged, "frequency_unit": s.display.frequency.symbol(), "points": points});
    s.results.td_radiation = Some(Arc::new(r));
    Ok(reply)
}

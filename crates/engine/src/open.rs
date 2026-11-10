//! Commands of open problems: the perfectly matched layer, sources, the radiation solve and far
//! fields. Their pattern chart is `view`'s.

use emcraft_model::units::wavenumber;
use std::f64::consts::PI;
use std::sync::Arc;

use emcraft_model::units::{C0, MU0};
use emcraft_model::{Ohms, Open, OpenShape, Shape, Source, Vec3};
use emcraft_solver::radiation::OpenResult;
use emcraft_theory::radiation::Huygens;
use serde_json::{Value, json};

use crate::reply::{parts, phasor, reflection};
pub use crate::view::{pattern_chart, pattern_chart_of};
use crate::{Params, Result, Session};

mod far;

pub(crate) use far::{Antenna, FarAsk, FarRow, FarRows, Measure, co_polar, far_rows};
pub use far::{FarSolver, results_far_field, strongest_azimuth};

/// The impedance of free space (Ω).
const ETA0: f64 = MU0 * C0;

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
    let open = p.metres("thickness").optional()?.map(|thickness| Open { thickness, reflection, shape });
    s.model.set_open(open)?;
    Ok(json!({"open": open.is_some(), "thickness": open.map(|o| s.show_length(o.thickness.get())), "shape": open.map(|o| o.shape)}))
}

pub fn source_dipole(s: &mut Session, p: &Params) -> Result<Value> {
    let dipole = Source::Dipole {
        at: p.point("at").required()?,
        direction: p.vector("direction").or(Vec3::unit(2))?,
        length: p.metres("length").required()?,
        current: p.number("current").or(1.0)?,
    };
    // With no region yet, an automatic one round it (ADR 0005).
    if s.model.domain.solid.is_none() {
        s.model.domain.region = emcraft_model::Region::Auto { margin: None, thickness: None };
    }
    if let Some(at) = dipole.position() {
        s.model.refresh_region_with(Some(emcraft_model::Aabb::point(at)))?;
    }
    s.model.add_source(dipole)?;
    Ok(json!({"sources": s.model.sources.len()}))
}

pub fn source_plane_wave(s: &mut Session, p: &Params) -> Result<Value> {
    let wave = Source::PlaneWave { direction: p.vector("direction").or(Vec3::unit(2))?, polarization: p.vector("polarization").or(Vec3::unit(0))? };
    s.model.add_source(wave)?;
    Ok(json!({"sources": s.model.sources.len()}))
}

pub fn port_lumped(s: &mut Session, p: &Params) -> Result<Value> {
    let amplitude = p.number("voltage").or(1.0)?;
    let phase = p.number("phase").or(0.0)?.to_radians();
    s.model.add_lumped_port(emcraft_model::LumpedPort {
        name: p.string("name").required()?.to_owned(),
        from: p.point("from").required()?,
        to: p.point("to").required()?,
        impedance: p.checked("impedance").or(Ohms::FIFTY)?,
        voltage: [amplitude * phase.cos(), amplitude * phase.sin()],
        radius: p.positive_length("radius").or(0.0)?,
        across: None,
    })?;
    Ok(json!({"lumped_ports": s.model.lumped.len()}))
}

/// Whether the region the fields fill is vacuum, as the closed forms take it.
fn in_vacuum(s: &Session) -> bool {
    s.model.domain.solid.as_ref().is_some_and(|x| s.model.material(&x.material).is_ok_and(|(_, m)| m.eps_r.get() == 1.0 && m.mu_r.get() == 1.0))
}

/// A centre-fed wire dipole the induced-EMF closed form applies to: two coaxial conducting
/// cylinders of one radius with the model's one lumped port across the gap between them, in
/// vacuum: (total length, radius).
fn wire_dipole(s: &Session) -> Option<(f64, f64)> {
    let ([a, b], [port]) = (s.model.inclusions(), &s.model.lumped[..]) else { return None };
    if !in_vacuum(s) || !s.model.sources.is_empty() || !s.model.features().is_empty() || a.material.is_some() || b.material.is_some() {
        return None;
    }
    let (&Shape::Cylinder { radius: ra, length: la }, &Shape::Cylinder { radius: rb, length: lb }) = (&a.shape, &b.shape) else { return None };
    let tol = 1e-9 * (la + lb);
    let on_axis = |q: emcraft_model::Point3| (q.x - a.at.x).abs() < tol && (q.y - a.at.y).abs() < tol;
    if !(on_axis(b.at) && (ra - rb).abs() < tol && on_axis(port.from) && on_axis(port.to)) {
        return None;
    }
    let lo = a.at.z.min(b.at.z);
    let hi = (a.at.z + la).max(b.at.z + lb);
    Some((hi - lo, ra.get()))
}

pub fn source_clear(s: &mut Session, _: &Params) -> Result<Value> {
    s.model.clear_sources();
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
    let vacuum = in_vacuum(s);
    match s.model.sources[..] {
        [Source::Dipole { direction, length, current, .. }] if vacuum && s.model.inclusions().is_empty() && s.model.features().is_empty() => {
            Some((current * length, direction))
        }
        _ => None,
    }
}

/// The one sphere a plane wave lights, whose Mie series applies: (radius, εr), εr `None` for a
/// conductor.
fn lone_sphere(s: &Session) -> Option<(f64, Option<f64>)> {
    plane_wave(s)?;
    match s.model.inclusions() {
        [inc] if s.model.features().is_empty() && s.model.sources.len() == 1 => match (&inc.shape, &inc.material) {
            (Shape::Sphere { radius }, Some(name)) => {
                let (_, m) = s.model.material(name).ok()?;
                (m.mu_r.get() == 1.0 && m.loss_tangent.get() == 0.0 && !m.is_magnetically_lossy()).then_some((radius.get(), Some(m.eps_r.get())))
            }
            (Shape::Sphere { radius }, None) => Some((radius.get(), None)),
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
    if plane_wave(s).is_some() {
        scattered(s, f, power, &mut reply);
    } else {
        reply["radiated_power"] = json!(power);
        if let Some((il, _)) = lone_dipole(s) {
            reply["exact_radiated_power"] = json!(emcraft_theory::radiation::dipole_power(il, f));
        }
    }
    if !result.ports.is_empty() {
        lumped_ports(s, &result, f, power, &mut reply);
    }
    if let Some((s11, accepted)) = result.feed {
        reply["feed"] = json!({"port": s.model.ports.first().map(|p| p.name.clone()), "s11": reflection(s11), "accepted_power": accepted});
        reply["radiation_efficiency"] = json!(if accepted > 0.0 { power / accepted } else { f64::NAN });
    }
    s.results.radiation = Some(Arc::new(Radiated { driven, result, huygens, power }));
    Ok(reply)
}

/// The scattering cross-section σ = P_sca / (|E₀|²/2η) of the `power` a 1 V/m plane wave's
/// scattered field carries at `f` (Hz), in display units of area; with the Mie series' for a lone
/// sphere.
fn scattered(s: &Session, f: f64, power: f64, reply: &mut Value) {
    let sigma = power * 2.0 * ETA0;
    let l = s.display.length.from_si(1.0);
    reply["scattering_cross_section"] = json!(sigma * l * l);
    if let Some((a, er)) = lone_sphere(s) {
        let x = wavenumber(f) * a;
        let q = emcraft_theory::radiation::mie_efficiencies(x, er.map(f64::sqrt)).0;
        reply["exact_scattering_cross_section"] = json!(q * PI * a * a * l * l);
    }
}

/// The lumped ports' impedances, currents, voltages, S11 and powers at `f` (Hz), the power they
/// put in and the share of it `power` radiates; for a wire dipole, the induced-EMF estimate of
/// its impedance.
fn lumped_ports(s: &Session, result: &OpenResult, f: f64, power: f64, reply: &mut Value) {
    let exact = wire_dipole(s).map(|(l, a)| emcraft_theory::radiation::dipole_impedance(l, a, f));
    let ports: Vec<Value> = result
        .ports
        .iter()
        .map(|pr| {
            let mut v = json!({
                "name": pr.name, "impedance": parts(pr.impedance), "current": parts(pr.current), "voltage": parts(pr.voltage),
                "s11": reflection(pr.s11), "power": pr.power,
            });
            if let Some(z) = exact {
                // The thin-wire, sinusoidal-current estimate: a guide, not a reference (a
                // wire a sixtieth of a wavelength thick differs by tens of percent).
                v["induced_emf_impedance"] = parts(z);
            }
            v
        })
        .collect();
    let input: f64 = result.ports.iter().map(|pr| pr.power).sum();
    reply["ports"] = json!(ports);
    reply["input_power"] = json!(input);
    reply["radiation_efficiency"] = json!(if input > 0.0 { power / input } else { f64::NAN });
}

/// The time domain's open problem: lumped ports and dipoles on a Yee grid with a CPML.
pub fn solve_time_domain_open(s: &mut Session, freqs: &[f64], cell: f64, device: Option<&dyn emcraft_fdtd::OpenStepper>) -> Result<Value> {
    let r = emcraft_fdtd::open::radiate_with(&s.model, &emcraft_fdtd::open::OpenOptions::new(cell), freqs, device)?;
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
                    let s11 = (z - lp.impedance.get()) / (z + lp.impedance.get());
                    json!({"name": pr.name, "impedance": phasor(z), "voltage": phasor(pr.voltage[fi]), "current": phasor(pr.current[fi]), "s11": reflection(s11), "power": pr.power[fi]})
                })
                .collect();
            let input: f64 = r.ports.iter().map(|pr| pr.power[fi]).sum();
            let mut v = json!({"frequency": s.show_frequency(*f), "radiated_power": r.radiated[fi], "ports": ports});
            if input > 0.0 {
                v["radiation_efficiency"] = json!(r.radiated[fi] / input);
            }
            // A plane wave's: the power scattered per volt per metre incident.
            if plane_wave(s).is_some() {
                scattered(s, *f, r.radiated[fi], &mut v);
            }
            v
        })
        .collect();
    let reply =
        json!({"cells": r.cells, "steps": r.steps, "converged": r.converged, "frequency_unit": s.display.frequency.symbol(), "points": points});
    s.results.td_radiation = Some(Arc::new(r));
    Ok(reply)
}

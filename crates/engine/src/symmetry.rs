//! Symmetry's commands: a body of revolution's modes, S-parameters and field on its meridian
//! half-plane (`solve.revolution`, `solve.revolution.frequency`, `view.revolution_field`) and a
//! rotationally symmetric model solved on its sector per harmonic (`solve.cyclic`,
//! `solve.harmonics`).

use serde_json::{Value, json};

use crate::reply::complex;
use crate::solve::frequencies;
use crate::view::commands::write_plot;
use crate::{EngineError, Params, Result, Session, args};

/// `solve.revolution`.
pub fn revolution(s: &mut Session, p: &Params) -> Result<Value> {
    let ring = p.pairs("points", emcraft_model::symmetry::meridian::MAX_PROFILE_POINTS).optional()?;
    let near = p.hertz("frequency").required()?.get();
    let m = p.integer("m", 0..=64).or(0)? as u32;
    let count = p.integer("count", 1..=50).or(5)?;
    let positive = |name: &str, model: f64| -> Result<f64> {
        let v = p.number(name).or(model)?;
        if v.is_finite() && v > 0.0 { Ok(v) } else { Err(p.error(name, "must be a finite number > 0")) }
    };
    // Without `points`, the model's own profile: its solid and the features about its axis.
    let model = match &ring {
        Some(_) => None,
        None => Some(s.model.meridian_profile(p.positive_length("size").optional()?.unwrap_or(1e-3))?),
    };
    let (eps, mu) = match &model {
        Some(pr) => {
            if p.has("eps_r") || p.has("mu_r") {
                return Err(p.error("eps_r", "is the model's material's when the profile is the model's"));
            }
            (pr.densest, 1.0)
        }
        None => (positive("eps_r", 1.0)?, positive("mu_r", 1.0)?),
    };
    let corners: &[[f64; 2]] = ring.as_deref().or_else(|| model.as_ref().map(|pr| pr.points.as_slice())).unwrap_or(&[]);
    let extent = corners.iter().fold([f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY], |b, q| {
        [b[0].min(q[0]), b[1].max(q[0]), b[2].min(q[1]), b[3].max(q[1])]
    });
    let span = (extent[1] - extent[0]).max(extent[3] - extent[2]);
    let wavelength = emcraft_model::units::C0 / (near * (eps * mu).sqrt());
    let size = p.positive_length("size").optional()?.unwrap_or_else(|| (0.1 * wavelength).min(span / 20.0));
    let profile = match (&ring, &model) {
        (Some(ring), _) => {
            let (points, triangles) = emcraft_mesh::planar::polygon(ring, size)?;
            emcraft_solver::bor::Profile { material: vec![(eps, mu); triangles.len()], points, triangles }
        }
        (None, Some(_)) => profile_mesh(&s.model, &s.model.meridian_profile(size)?, size)?,
        (None, None) => return Err(p.error("points", "is required without a model to take the profile of")),
    };
    let modes = emcraft_solver::bor::eigenmodes(&profile, m, count, near)?;
    let shown: Vec<Value> = modes.iter().map(|md| json!({"frequency": s.show_frequency(md.frequency), "m": md.m})).collect();
    Ok(json!({"modes": shown, "triangles": profile.triangles.len(), "points": profile.points.len()}))
}

/// The model's meridian profile in triangles of about `size`, each with what fills it there
/// (the inclusions' materials where they are).
fn profile_mesh(
    model: &emcraft_model::Model,
    pr: &emcraft_model::symmetry::meridian::MeridianProfile,
    size: f64,
) -> Result<emcraft_solver::bor::Profile> {
    let (points, triangles) = emcraft_mesh::planar::pslg_keep(&pr.points, &pr.segments, size, &|c| model.revolution_fill(c).is_some())?;
    let material = triangles
        .iter()
        .map(|t| {
            let q = t.map(|v| points.get(v as usize).copied().unwrap_or_default());
            model.revolution_fill([(q[0][0] + q[1][0] + q[2][0]) / 3.0, (q[0][1] + q[1][1] + q[2][1]) / 3.0]).unwrap_or((pr.eps_r, pr.mu_r))
        })
        .collect();
    Ok(emcraft_solver::bor::Profile { points, triangles, material })
}

/// `solve.revolution.frequency`: the model as a body of revolution with its ports on its ends,
/// its S-parameters in one azimuthal order on the meridian half-plane.
pub fn revolution_frequency(s: &mut Session, p: &Params) -> Result<Value> {
    let freqs = frequencies(s, p)?;
    let m = p.integer("m", 0..=64).or(1)? as u32;
    let top = freqs.last().copied().unwrap_or(1e10);
    let (profile, ports) = revolution_setup(s, p, top)?;
    let mut rows = Vec::with_capacity(freqs.len());
    let mut names = Vec::new();
    let mut unknowns = 0;
    let n = ports.len();
    let mut values = Vec::with_capacity(freqs.len() * n * n);
    for (k, &f) in freqs.iter().enumerate() {
        crate::progress::report(k, freqs.len(), "frequency")?;
        let r = emcraft_solver::bor::s_parameters(&profile, m, &ports, f)?;
        values.extend((0..n * n).map(|i| r.s.get(i / n, i % n).unwrap_or_default()));
        let mut row = serde_json::Map::new();
        row.insert("frequency".into(), json!(s.show_frequency(f)));
        for a in 0..ports.len() {
            for b in 0..ports.len() {
                row.insert(format!("s{}{}", a + 1, b + 1), complex(r.s.get(a, b).unwrap_or_default()));
            }
        }
        rows.push(Value::Object(row));
        names = r.modes;
        unknowns = r.unknowns;
    }
    // Kept as the session's result: its charts, its pulse (`results.pulse`), its file.
    let data = crate::sdata::SData::square(freqs.clone(), n, values).map_err(EngineError::failed)?;
    s.results.revolution = Some(std::sync::Arc::new(crate::Revolution { m, modes: names.clone(), data, unknowns }));
    Ok(json!({"m": m, "modes": names, "points": rows, "triangles": profile.triangles.len(), "unknowns": unknowns}))
}

/// The most modes a cell's ends carry for its Bloch wave.
const MAX_BLOCH_MODES: usize = 8;

/// `solve.revolution.bloch`: the model as one cell of a periodic body of revolution (its ports
/// on its two ends, one period apart): the Bloch wave of the infinite repetition at each
/// frequency ([`emcraft_solver::bor::bloch_lambda`]), γ = α + jβ, β unwrapped along the sweep from
/// the branch nearest the smooth guide's at the first frequency; kept with the cell's own S as
/// the session's result, for `results.pulse` over any length of it.
pub fn revolution_bloch(s: &mut Session, p: &Params) -> Result<Value> {
    let freqs = frequencies(s, p)?;
    let m = p.integer("m", 0..=64).or(1)? as u32;
    let modes = p.integer("modes", 1..=MAX_BLOCH_MODES).or(3)?;
    let top = freqs.last().copied().unwrap_or(1e10);
    let (profile, ports) = revolution_setup(s, p, top)?;
    let [near, far] = ports.as_slice() else {
        return Err(EngineError::failed("a cell needs its two ports, one on each end"));
    };
    let (near, far) = (emcraft_solver::bor::BorPort { modes, ..*near }, emcraft_solver::bor::BorPort { modes, ..*far });
    let period = (far.z - near.z).abs();
    // The guide's radius at the near end: the profile's widest point on that plane.
    let radius = profile.points.iter().filter(|q| (q[1] - near.z).abs() <= 1e-9 * period.max(1e-300)).map(|q| q[0]).fold(0.0, f64::max);
    // The smooth guide's first mode of order m: TE_m1 (the first zero of J_m′), TM01 for m = 0.
    let x = if m == 0 {
        2.404_825_557_695_773
    } else {
        emcraft_fem::bessel::zeros(|t| emcraft_fem::bessel::jp(m as usize, t), 1, 100.0).first().copied().unwrap_or(1.841_183_781_340_659)
    };
    let mut gammas = Vec::with_capacity(freqs.len());
    let mut values = Vec::with_capacity(freqs.len() * 4);
    let mut rows = Vec::with_capacity(freqs.len());
    let (mut names, mut unknowns) = (Vec::new(), 0);
    let mut last: Option<f64> = None;
    for (k, &f) in freqs.iter().enumerate() {
        crate::progress::report(k, freqs.len(), "frequency")?;
        let (point, _) = emcraft_solver::bor::generalized(&profile, m, &[near, far], f)?;
        values.extend((0..4).map(|i| point.s.get(i / 2, i % 2).unwrap_or_default()));
        let lambda = emcraft_solver::bor::bloch_lambda(&profile, m, near, far, f)?;
        let alpha = -lambda.norm().ln() / period;
        let raw = -lambda.arg() / period;
        // The branch: nearest the last (continuity), else the smooth guide's.
        let k0 = 2.0 * std::f64::consts::PI * f / emcraft_model::units::C0;
        let guess = last.unwrap_or_else(|| (k0 * k0 - (x / radius.max(1e-300)).powi(2)).max(0.0).sqrt());
        let turn = 2.0 * std::f64::consts::PI / period;
        let beta = raw + turn * ((guess - raw) / turn).round();
        last = Some(beta);
        gammas.push(emcraft_solver::Complex64::new(alpha, beta));
        rows.push(json!({"frequency": s.show_frequency(f), "alpha": alpha, "beta": beta, "phase": (beta * period).to_degrees()}));
        names = point.modes;
        unknowns = point.unknowns;
    }
    let data = crate::sdata::SData::square(freqs.clone(), 2, values).map_err(EngineError::failed)?;
    s.results.revolution = Some(std::sync::Arc::new(crate::Revolution { m, modes: names.clone(), data, unknowns }));
    let wave = crate::BlochWave { name: format!("m = {m}"), gamma: gammas };
    s.results.bloch = Some(std::sync::Arc::new(crate::Bloch { period, frequencies: freqs, waves: vec![wave] }));
    Ok(json!({"m": m, "period": s.show_length(period), "modes": names, "points": rows, "triangles": profile.triangles.len(), "unknowns": unknowns}))
}

/// The model as a body of revolution with its ports on its lowest and highest ends: its profile in
/// triangles of `size` (default a fortieth of the wavelength at `top` Hz in its densest
/// material, at most a twentieth of the profile) and its ports.
pub(crate) fn revolution_setup(s: &Session, p: &Params, top: f64) -> Result<(emcraft_solver::bor::Profile, Vec<emcraft_solver::bor::BorPort>)> {
    if s.model.ports.is_empty() {
        return Err(EngineError::failed("no ports: create them on the body's ends with model.port.create"));
    }
    let first = s.model.meridian_profile(1e-3)?;
    let span = first.points.iter().fold(0.0f64, |a, q| a.max(q[0]).max(q[1].abs()));
    let wavelength = emcraft_model::units::C0 / (top * first.densest.sqrt());
    let size = p.positive_length("size").optional()?.unwrap_or_else(|| (wavelength / 40.0).min(span / 20.0));
    // The outline's curves in chords of the triangles' size.
    let pr = s.model.meridian_profile(size)?;
    let ports = bor_ports(&s.model, &pr)?;
    Ok((profile_mesh(&s.model, &pr, size)?, ports))
}

/// The model's ports as a body of revolution's: on the profile's lowest or highest end, or on the
/// plane z = `at` of a face across z (a guide's closed end inside the body).
fn bor_ports(model: &emcraft_model::Model, pr: &emcraft_model::symmetry::meridian::MeridianProfile) -> Result<Vec<emcraft_solver::bor::BorPort>> {
    let [lo, hi] = pr.points.iter().fold([f64::INFINITY, f64::NEG_INFINITY], |b, q| [b[0].min(q[1]), b[1].max(q[1])]);
    model
        .ports
        .iter()
        .map(|x| {
            let z = match &x.face {
                None if x.end == emcraft_model::End::Start => lo,
                None => hi,
                Some(f) if f.axis == emcraft_model::Axis::Z => f.at,
                Some(_) => {
                    return Err(EngineError::failed(format!(
                        "port {}'s face is not across z: a body of revolution's ports lie across its axis",
                        x.name
                    )));
                }
            };
            Ok(emcraft_solver::bor::BorPort { z, modes: x.modes as usize })
        })
        .collect()
}

/// `solve.revolution.radiation`: the open model as a body of revolution, its one source, the far
/// field.
pub fn revolution_radiation(s: &mut Session, p: &Params) -> Result<Value> {
    use emcraft_solver::bor::open::{BorSource, Layer, LayerShape};
    let frequency = args::frequency_or_band_top(s, p, "frequency", args::SET_THE_BAND)?;
    let open = s.model.domain.open.as_ref().ok_or_else(|| EngineError::failed("no open boundary: line the model with model.open"))?;
    let shape = match (s.model.domain.solid.as_ref().map(|x| &x.shape), open.shape) {
        (Some(emcraft_model::Shape::Cylinder { radius, length }), emcraft_model::OpenShape::Box) => {
            LayerShape::Cylinder { radius: radius.get(), z0: 0.0, z1: length.get() }
        }
        (Some(emcraft_model::Shape::Sphere { radius }), emcraft_model::OpenShape::Sphere) => LayerShape::Sphere { radius: radius.get() },
        _ => return Err(EngineError::failed("a body of revolution's layer lines a cylinder (layer shape box) or a sphere (shape sphere)")),
    };
    let layer = Layer::new(shape, open.thickness.get(), open.reflection.unwrap_or(emcraft_solver::driven::PML_REFLECTION))?;
    if !s.model.lumped.is_empty() {
        return Err(EngineError::failed("not supported yet: lumped ports on a body of revolution (feed it by a waveguide port or a source)"));
    }
    if !s.model.ports.is_empty() {
        return revolution_fed(s, p, &layer, frequency);
    }
    let tol = 1e-9 * layer.thickness;
    // The source and the turn of the wave's polarization from x̂ (the far field's φ shifts by it).
    let (source, turn) = match s.model.sources.as_slice() {
        [emcraft_model::Source::Dipole { at, direction, length, current }] => {
            if at.x.abs() > tol || at.y.abs() > tol || direction.x.abs() > 1e-12 || direction.y.abs() > 1e-12 {
                return Err(EngineError::failed(
                    "not supported yet: a dipole off the axis or across it (a body of revolution takes one on the axis along z)",
                ));
            }
            (BorSource::AxialDipole { z: at.z, moment: current * length.get() * direction.z.signum() }, 0.0)
        }
        [emcraft_model::Source::PlaneWave { direction, polarization }] => {
            if direction.x.abs() > 1e-12 || direction.y.abs() > 1e-12 {
                return Err(EngineError::failed("not supported yet: a plane wave off the axis (a body of revolution takes one along ±z)"));
            }
            (BorSource::PlaneWave { up: direction.z > 0.0 }, polarization.y.atan2(polarization.x))
        }
        [] => return Err(EngineError::failed("no source: add one with model.source")),
        _ => return Err(EngineError::failed("not supported yet: more than one source on a body of revolution")),
    };
    let first = s.model.meridian_profile(1e-3)?;
    let span = first.points.iter().fold(0.0f64, |a, q| a.max(q[0]).max(q[1].abs()));
    let wavelength = emcraft_model::units::C0 / (frequency * first.densest.sqrt());
    let size = p.positive_length("size").optional()?.unwrap_or_else(|| (wavelength / 25.0).min(span / 20.0));
    let pr = s.model.meridian_profile(size)?;
    let profile = profile_mesh(&s.model, &pr, size)?;
    let r = emcraft_solver::bor::open::radiate(&profile, &layer, &source, frequency)?;
    let thetas = polar_angles(p)?;
    let power = r.huygens.power();
    let l = s.display.length.from_si(1.0);
    let wave = matches!(source, BorSource::PlaneWave { .. });
    let points: Vec<Value> = thetas
        .iter()
        .map(|&deg| {
            let theta = deg.to_radians();
            let (et, ep) = r.huygens.far_field(theta, -turn);
            let mut v = json!({"theta": deg, "e_theta": complex(et), "e_phi": complex(ep)});
            if wave {
                for (key, phi) in [("rcs_e_plane", 0.0), ("rcs_h_plane", std::f64::consts::FRAC_PI_2)] {
                    let (a, b) = r.huygens.far_field(theta, phi - turn);
                    v[key] = json!(4.0 * std::f64::consts::PI * (a.norm_sqr() + b.norm_sqr()) * l * l);
                }
            } else {
                v["directivity"] = json!(r.huygens.directivity(theta, 0.0));
            }
            v
        })
        .collect();
    Ok(
        json!({"frequency": s.show_frequency(frequency), "m": r.m, "power": power, "points": points, "triangles": profile.triangles.len(), "unknowns": r.unknowns}),
    )
}

/// `solve.revolution.time_domain`: the closed model as a body of revolution rung in one order on
/// the meridian half-plane's (r, z) grid, its resonances in the band.
pub fn revolution_time_domain(s: &mut Session, p: &Params) -> Result<Value> {
    if s.model.domain.open.is_some() || !s.model.ports.is_empty() || !s.model.sources.is_empty() || !s.model.lumped.is_empty() {
        return Err(EngineError::failed(
            "not supported yet: ports, sources or an open boundary in a body of revolution's time domain (a closed cavity rings)",
        ));
    }
    let hint = "give `start` and `stop`, or confirm the run's settings: project.setup";
    let start = p.frequency("start").or_maybe(s.model.job.range().map(|b| b.start.get())).required_or(hint)?;
    let stop = p.frequency("stop").or_maybe(s.model.job.range().map(|b| b.stop.get())).required_or(hint)?;
    let m = p.integer("m", 0..=64).or(0)? as u32;
    let periods = p.number("periods").or(300.0)?;
    let first = s.model.meridian_profile(1e-3)?;
    let [r1, z0, z1] = first.points.iter().fold([0.0f64, f64::INFINITY, f64::NEG_INFINITY], |b, q| [b[0].max(q[0]), b[1].min(q[1]), b[2].max(q[1])]);
    let wavelength = emcraft_model::units::C0 / (stop * first.densest.sqrt());
    let cell = p.positive_length("cell").optional()?.unwrap_or_else(|| (wavelength / 30.0).min(r1.max(z1 - z0) / 20.0));
    let count = |span: f64| ((span / cell).ceil() as usize).max(2);
    let (nr, nz) = (count(r1), count(z1 - z0));
    if nr.saturating_mul(nz) > emcraft_fdtd::MAX_CELLS {
        return Err(p.error("cell", format!("{nr} × {nz} cells are more than {}", emcraft_fdtd::MAX_CELLS)));
    }
    let (dr, dz) = (r1 / nr as f64, (z1 - z0) / nz as f64);
    let cells = (0..nr * nz).map(|k| s.model.revolution_fill([(k % nr) as f64 * dr + 0.5 * dr, z0 + (k / nr) as f64 * dz + 0.5 * dz])).collect();
    let grid = emcraft_fdtd::bor::BorGrid { nr, nz, dr, dz, z0, cells };
    let r = emcraft_fdtd::bor::ring(&grid, m, (start, stop), periods)?;
    let modes: Vec<Value> = r.frequencies.iter().map(|&f| json!({"frequency": s.show_frequency(f), "m": m})).collect();
    Ok(json!({"modes": modes, "cells": [nr, nz], "cell": s.show_length(dr.max(dz)), "steps": r.steps, "dt": r.dt}))
}

/// `theta` (degrees, 0 to 180; default 0 to 180 by 15).
fn polar_angles(p: &Params) -> Result<Vec<f64>> {
    Ok(match p.list("theta", 721).optional()? {
        Some(list) => list
            .iter()
            .map(|v| {
                v.as_f64()
                    .filter(|t| t.is_finite() && (0.0..=180.0).contains(t))
                    .ok_or_else(|| p.error("theta", "must hold angles from 0 to 180 degrees"))
            })
            .collect::<Result<_>>()?,
        None => (0..=12).map(|k| 15.0 * f64::from(k)).collect(),
    })
}

/// [`revolution_radiation`] fed by the model's ports: order `m` (default 1, the linearly polarized
/// TE11 feed; E along x at φ = 0), a unit wave of `port`'s first mode in, S between the ports' first
/// modes and the directivity in the E-plane (φ = 0) and the H-plane (φ = 90°).
fn revolution_fed(s: &Session, p: &Params, layer: &emcraft_solver::bor::open::Layer, frequency: f64) -> Result<Value> {
    if !s.model.sources.is_empty() {
        return Err(EngineError::failed("not supported yet: ports and sources together on a body of revolution"));
    }
    let m = p.integer("m", 0..=64).or(1)? as u32;
    let excitation = match p.string("port").optional()? {
        Some(name) => s.model.ports.iter().position(|x| x.name == name).ok_or_else(|| p.error("port", format!("no port `{name}`")))?,
        None => 0,
    };
    let first = s.model.meridian_profile(1e-3)?;
    let span = first.points.iter().fold(0.0f64, |a, q| a.max(q[0]).max(q[1].abs()));
    let wavelength = emcraft_model::units::C0 / (frequency * first.densest.sqrt());
    let size = p.positive_length("size").optional()?.unwrap_or_else(|| (wavelength / 25.0).min(span / 20.0));
    let pr = s.model.meridian_profile(size)?;
    let ports = bor_ports(&s.model, &pr)?;
    let profile = profile_mesh(&s.model, &pr, size)?;
    let (point, r) = emcraft_solver::bor::open::radiate_ports(&profile, layer, &ports, m, excitation, frequency)?;
    let mut sp = serde_json::Map::new();
    for a in 0..ports.len() {
        for b in 0..ports.len() {
            sp.insert(format!("s{}{}", a + 1, b + 1), complex(point.s.get(a, b).unwrap_or_default()));
        }
    }
    let points: Vec<Value> = polar_angles(p)?
        .iter()
        .map(|&deg| {
            let theta = deg.to_radians();
            let (et, ep) = r.huygens.far_field(theta, 0.0);
            json!({
                "theta": deg,
                "e_theta": complex(et),
                "e_phi": complex(ep),
                "directivity": r.huygens.directivity(theta, 0.0),
                "directivity_h": r.huygens.directivity(theta, std::f64::consts::FRAC_PI_2),
            })
        })
        .collect();
    Ok(json!({
        "frequency": s.show_frequency(frequency),
        "m": m,
        "modes": point.modes,
        "s": Value::Object(sp),
        "power": r.huygens.power(),
        "points": points,
        "triangles": profile.triangles.len(),
        "unknowns": r.unknowns,
    }))
}

/// `solve.cyclic`: a rotationally symmetric cavity's modes per harmonic, from its sector.
pub fn cyclic(s: &mut Session, p: &Params) -> Result<Value> {
    let rot = s.model.domain.rotation.ok_or_else(|| EngineError::failed("no rotational symmetry: set it with model.rotation"))?;
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
        model.domain.rotation = Some(emcraft_model::Rotation { m, ..rot });
        let modes = emcraft_solver::bloch::cyclic_modes(&model, order, count, near)?;
        let shown: Vec<f64> = modes.iter().map(|&f| s.show_frequency(f)).collect();
        rows.push(json!({"m": m, "modes": shown}));
    }
    Ok(json!({"n": rot.n, "order": order.as_u8(), "harmonics": rows}))
}

/// `solve.harmonics`: a rotationally symmetric model with ports on its axis solved on its sector
/// in each harmonic, and its dominant mode's S over both rotations (m = 1 and n − 1) in the
/// standing polarizations the whole model's ports carry.
pub fn harmonics(s: &mut Session, p: &Params) -> Result<Value> {
    let rot = s.model.domain.rotation.ok_or_else(|| EngineError::failed("no rotational symmetry: set it with model.rotation"))?;
    if s.model.ports.is_empty() {
        return Err(EngineError::failed("no ports: create them on the solid's ends with model.port.create"));
    }
    let freqs = frequencies(s, p)?;
    let order = p.order()?;
    let wanted: Vec<u32> = match p.list("m", 64).optional()? {
        Some(list) => list
            .iter()
            .map(|v| {
                v.as_u64()
                    .filter(|&m| m < u64::from(rot.n))
                    .map(|m| m as u32)
                    .ok_or_else(|| p.error("m", format!("must hold harmonics 0 to {}", rot.n - 1)))
            })
            .collect::<Result<_>>()?,
        None => (0..rot.n).collect(),
    };
    let ports = s.model.ports.len();
    let mut solved: Vec<(u32, Vec<String>, Vec<Option<emcraft_solver::SMatrix>>)> = Vec::new();
    let total = wanted.len() * freqs.len();
    for (i, &m) in wanted.iter().enumerate() {
        let mut model = s.model.clone();
        model.domain.rotation = Some(emcraft_model::Rotation { m, ..rot });
        let d = emcraft_solver::Driven::new(&model, order)?;
        let names: Vec<String> = d
            .ports
            .iter()
            .map(|port| match &port.modes {
                emcraft_solver::driven::PortModes::Cyclic(c) => {
                    c.modes.first().map(emcraft_solver::driven::cyclic::RotatingMode::name).unwrap_or_default()
                }
                _ => String::new(),
            })
            .collect();
        let mut points = Vec::with_capacity(freqs.len());
        for (k, &f) in freqs.iter().enumerate() {
            crate::progress::report(i * freqs.len() + k, total, "harmonic")?;
            // A harmonic whose first modes do not all propagate there carries no wave in.
            let carries = (0..ports).map(|q| d.complex_mode_propagates(q, f)).collect::<std::result::Result<Vec<_>, _>>()?;
            points.push(if carries.iter().all(|c| *c == Some(true)) { Some(d.solve(f)?.s) } else { None });
        }
        solved.push((m, names, points));
    }
    let label = |q: usize| s.model.ports.get(q).map_or_else(String::new, |x| x.name.clone());
    let table = |sm: &emcraft_solver::SMatrix| {
        let mut row = serde_json::Map::new();
        for a in 0..ports {
            for b in 0..ports {
                row.insert(format!("s{}{}", a + 1, b + 1), complex(sm.get(a, b).unwrap_or_default()));
            }
        }
        Value::Object(row)
    };
    let shown: Vec<Value> = solved
        .iter()
        .map(|(m, names, points)| {
            let pts: Vec<Value> = freqs
                .iter()
                .zip(points)
                .map(|(f, sm)| match sm {
                    Some(sm) => {
                        let mut v = table(sm);
                        v["frequency"] = json!(s.show_frequency(*f));
                        v
                    }
                    None => json!({"frequency": s.show_frequency(*f), "propagating": false}),
                })
                .collect();
            json!({"m": m, "modes": names, "points": pts})
        })
        .collect();
    let mut reply = json!({"n": rot.n, "order": order.as_u8(), "ports": (0..ports).map(label).collect::<Vec<_>>(), "harmonics": shown});
    // The dominant pair: the mode of order +1 in harmonic 1 and its counter-rotating twin in
    // n − 1, as the standing cos (c) and sin (s) polarizations (e± = (e_c ± j e_s)/√2 for TE,
    // ∓ for TM): S_cc = S_ss = (S₊ + S₋)/2, S_sc = ±j(S₊ − S₋)/2, S_cs = ∓j(S₊ − S₋)/2.
    let find = |m: u32| solved.iter().find(|(k, _, _)| *k == m);
    if let (Some((_, plus, sp)), Some((_, minus, sm))) = (find(1), find(rot.n - 1)) {
        let base = |name: &str| name.trim_end_matches(['+', '−']).to_owned();
        let same = plus.iter().zip(minus).all(|(a, b)| a.ends_with('+') && b.ends_with('−') && base(a) == base(b));
        if same && rot.n >= 3 {
            let te = plus.first().is_some_and(|n| n.starts_with("TE"));
            let j = emcraft_solver::Complex64::new(0.0, if te { 1.0 } else { -1.0 });
            let pts: Vec<Value> = freqs
                .iter()
                .zip(sp.iter().zip(sm))
                .map(|(f, pair)| {
                    let mut row = serde_json::Map::new();
                    row.insert("frequency".into(), json!(s.show_frequency(*f)));
                    if let (Some(a), Some(b)) = pair {
                        for x in 0..ports {
                            for y in 0..ports {
                                let (up, down) = (a.get(x, y).unwrap_or_default(), b.get(x, y).unwrap_or_default());
                                let (even, odd) = ((up + down) * 0.5, (up - down) * 0.5);
                                for (key, v) in [("cc", even), ("ss", even), ("sc", j * odd), ("cs", -j * odd)] {
                                    let (o, i) = (&key[..1], &key[1..]);
                                    row.insert(format!("s{}{o}{}{i}", x + 1, y + 1), complex(v));
                                }
                            }
                        }
                    }
                    Value::Object(row)
                })
                .collect();
            reply["standing"] = json!({"mode": base(plus.first().map_or("", String::as_str)), "points": pts});
        }
    }
    Ok(reply)
}

/// `view.revolution_field`: the field of the model as a body of revolution on its meridian
/// half-plane for a wave into one of its ports: |E| in colour, (E_r, E_z) as arrows (φ = 0, phase
/// 0), the axis on the left.
pub fn revolution_field(s: &mut Session, p: &Params) -> Result<Value> {
    use emcraft_render::plot::{Arrows, FieldPlot, Scalar, ScalarKind};
    let path = p.string("path").required()?;
    let width = p.side("width", s.limits.picture_side, 600)?;
    let frequency = args::frequency_or_band_top(s, p, "frequency", args::SET_THE_BAND)?;
    let m = p.integer("m", 0..=64).or(1)? as u32;
    let palette = crate::theme::palette_of(p)?;
    let (profile, ports) = crate::symmetry::revolution_setup(s, p, frequency)?;
    let excitation = match p.string("port").optional()? {
        Some(name) => s.model.ports.iter().position(|x| x.name == name).ok_or_else(|| p.error("port", format!("no port `{name}`")))?,
        None => 0,
    };
    let (r0, r1, z0, z1) = profile.points.iter().fold((f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY), |b, q| {
        (b.0.min(q[0]), b.1.max(q[0]), b.2.min(q[1]), b.3.max(q[1]))
    });
    // Square cells: the longer side 160 points.
    let side = 160.0 / (r1 - r0).max(z1 - z0).max(f64::MIN_POSITIVE);
    let (nx, ny) = (((r1 - r0) * side).round().max(2.0) as usize, ((z1 - z0) * side).round().max(2.0) as usize);
    let f = emcraft_solver::bor::field(&profile, m, &ports, frequency, excitation, [nx, ny])?;
    let l = s.display.length;
    let lu = l.symbol();
    let magnitude: Vec<f64> = f.e.iter().map(|e| e.map_or(f64::NAN, |e| (e[0].norm_sqr() + e[1].norm_sqr() + e[2].norm_sqr()).sqrt())).collect();
    let step = (nx.max(ny) / 20).max(1);
    let mut arrows = Vec::new();
    for j in (step / 2..ny).step_by(step) {
        for i in (step / 2..nx).step_by(step) {
            if let Some(e) = f.e[j * nx + i] {
                let q = [r0 + (r1 - r0) * i as f64 / (nx - 1) as f64, z0 + (z1 - z0) * j as f64 / (ny - 1) as f64];
                arrows.push(([l.from_si(q[0]), l.from_si(q[1])], [e[0].re, e[1].re]));
            }
        }
    }
    let fu = s.display.frequency;
    let name = f.point.modes.get(excitation).cloned().unwrap_or_default();
    let plot = FieldPlot {
        title: format!(
            "Body of revolution, m = {m}, {name} into port {}  f = {:.4} {}",
            s.model.ports.get(excitation).map_or("", |x| x.name.as_str()),
            fu.from_si(frequency),
            fu.symbol()
        ),
        subtitle: "the meridian half-plane (axis on the left): |E| in colour, (E_r, E_z) at φ = 0".into(),
        x: emcraft_render::Axis::new(&format!("r ({lu})"), l.from_si(r0), l.from_si(r1)),
        y: emcraft_render::Axis::new(&format!("z ({lu})"), l.from_si(z0), l.from_si(z1)),
        scalar: Some(Scalar { label: "$|E|$".into(), nx, ny, values: magnitude, kind: ScalarKind::Magnitude, range: None, db: None }),
        arrows: vec![Arrows { label: "E (in plane)".into(), pen: emcraft_render::Pen::EArrows, arrows, scale: None }],
        lines: Vec::new(),
        arrow_length: l.from_si((r1 - r0).max(z1 - z0)) / 20.0 * 0.9,
    };
    let mut reply = write_plot(path, &plot, width, &palette)?;
    reply["mode"] = json!(name);
    Ok(reply)
}

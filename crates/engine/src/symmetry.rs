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
    for (k, &f) in freqs.iter().enumerate() {
        crate::progress::report(k, freqs.len(), "frequency")?;
        let r = emcraft_solver::bor::s_parameters(&profile, m, &ports, f)?;
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
    Ok(json!({"m": m, "modes": names, "points": rows, "triangles": profile.triangles.len(), "unknowns": unknowns}))
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
    let [lo, hi] = pr.points.iter().fold([f64::INFINITY, f64::NEG_INFINITY], |b, q| [b[0].min(q[1]), b[1].max(q[1])]);
    let ports = s
        .model
        .ports
        .iter()
        .map(|x| match x.face {
            None => Ok(emcraft_solver::bor::BorPort { z: if x.end == emcraft_model::End::Start { lo } else { hi }, modes: x.modes as usize }),
            Some(_) => {
                Err(EngineError::failed(format!("not supported yet: port {} on a face (a body of revolution's ports lie on its ends)", x.name)))
            }
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((profile_mesh(&s.model, &pr, size)?, ports))
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

/// `solve.harmonics`: a rotationally symmetric model with ports on its axis solved on its sector
/// in each harmonic, and its dominant mode's S over both rotations (m = 1 and n − 1) in the
/// standing polarizations the whole model's ports carry.
pub fn harmonics(s: &mut Session, p: &Params) -> Result<Value> {
    let rot = s.model.rotation.ok_or_else(|| EngineError::failed("no rotational symmetry: set it with model.rotation"))?;
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
        model.rotation = Some(emcraft_model::Rotation { m, ..rot });
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
        scalar: Some(Scalar { label: "$|E|$".into(), nx, ny, values: magnitude, kind: ScalarKind::Magnitude, range: None }),
        arrows: vec![Arrows { label: "E (in plane)".into(), pen: emcraft_render::Pen::EArrows, arrows, scale: None }],
        lines: Vec::new(),
        arrow_length: l.from_si((r1 - r0).max(z1 - z0)) / 20.0 * 0.9,
    };
    let mut reply = write_plot(path, &plot, width, &palette)?;
    reply["mode"] = json!(name);
    Ok(reply)
}

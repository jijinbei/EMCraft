//! Editing and deleting the model's items: materials, waveguide and lumped ports and sources
//! (bodies are [`crate::bodies`]'). An item is found by its name (a source, which has none, by
//! its number, 1 = the first); an edit changes only the parameters it is given and checks the
//! item again against the rest of the model ([`emcraft_model::edit`]); a material still in use
//! is not deleted. Each is a change of the model, in the history and undone like any other.
//!
//! A parameter given as `project.values` shows it keeps its value exactly
//! ([`Params::edit_length`]), so an edit dialog's OK with nothing changed changes nothing.

use emcraft_model::units::Display;
use emcraft_model::{Material, MaterialUse, Model, Pole, Port, PortFace, Source};
use serde_json::{Value, json};

use crate::geometry::{POLE_KINDS, pole_count, pole_kind, poles_of};
use crate::{Change, EngineError, Params, Result, Session};

/// The most names an error lists.
const LISTED: usize = 20;

/// "names no `kind`" with the names there are (at most [`LISTED`]).
fn none_named<'a>(p: &Params, param: &str, kind: &str, wanted: &str, names: impl IntoIterator<Item = &'a str>) -> EngineError {
    let names: Vec<&str> = names.into_iter().collect();
    let what = format_args!("names no {kind}: `{}`", crate::text::clip(wanted, 64));
    if names.is_empty() {
        return p.error(param, format!("{what} (there is none)"));
    }
    p.error(param, crate::text::unknown(what, "there are ", crate::text::listed(&names, LISTED)))
}

/// The material parameter `name` names, and its index.
pub(crate) fn material_named<'m>(m: &'m Model, p: &Params, param: &str) -> Result<(usize, &'m Material)> {
    let name = p.string(param).required()?;
    m.material(name).map_err(|_| none_named(p, param, "material", name, m.materials.iter().map(|x| x.name.as_str())))
}

/// A material's poles of one kind as `model.material.create` takes them, in display units.
pub(crate) fn shown_poles(m: &Material, kind: &str, d: Display) -> Vec<Value> {
    let f = |hz: f64| d.frequency.from_si(hz);
    m.dispersion
        .iter()
        .filter(|pole| pole_kind(pole) == kind)
        .map(|pole| match *pole {
            Pole::Debye { delta_eps, tau } => json!({"delta_eps": delta_eps, "frequency": f(1.0 / (2.0 * std::f64::consts::PI * tau))}),
            Pole::Lorentz { delta_eps, frequency, damping } => json!({"delta_eps": delta_eps, "frequency": f(frequency), "damping": f(damping)}),
            Pole::Drude { plasma_frequency, damping } => json!({"plasma_frequency": f(plasma_frequency), "damping": f(damping)}),
        })
        .collect()
}

/// Where a material is used, in words: "section 2", "the region", "body 3".
fn uses_shown(m: &Model, uses: &[MaterialUse]) -> Vec<String> {
    let bodies = crate::bodies::bodies(m);
    uses.iter()
        .map(|u| match *u {
            MaterialUse::Section(k) => format!("section {}", k + 1),
            MaterialUse::Layer(k) => format!("layer {}", k + 1),
            MaterialUse::Region => "the region (the solid)".to_owned(),
            MaterialUse::Inclusion(k) => bodies
                .iter()
                .find(|b| b.at == crate::bodies::BodyRef::Inclusion(k))
                .map_or_else(|| format!("inclusion {}", k + 1), |b| b.name.clone()),
        })
        .collect()
}

/// `model.material.edit`.
pub fn material_edit(s: &mut Session, p: &Params) -> Result<Value> {
    let (i, old) = material_named(&s.model, p, "name").map(|(i, m)| (i, m.clone()))?;
    let name = p.string("rename").optional()?.unwrap_or(&old.name);
    let mut dispersion = Vec::new();
    for kind in POLE_KINDS {
        let kept = (!p.has(kind) && !p.cleared(kind)) || p.raw(kind) == Some(&Value::Array(shown_poles(&old, kind, s.display)));
        if kept {
            dispersion.extend(old.dispersion.iter().copied().filter(|pole| pole_kind(pole) == kind));
        } else {
            dispersion.extend(poles_of(p, kind)?);
        }
    }
    let material = Material::new(name, p.number("eps_r").or(old.eps_r)?, p.number("mu_r").or(old.mu_r)?)?
        .with_loss_tangent(p.number("loss_tangent").or(old.loss_tangent)?)?
        .with_magnetic_loss_tangent(p.number("magnetic_loss_tangent").or(old.magnetic_loss_tangent)?)?
        .with_dispersion(pole_count(dispersion)?)?;
    // A new name alone changes no result; anything else every one.
    let physics = Material { name: old.name.clone(), ..material.clone() } != old;
    s.model.replace_material(i, material)?;
    if physics {
        s.results.invalidate(Change::Geometry);
    }
    Ok(json!({"name": name, "renamed": name != old.name, "results_dropped": physics}))
}

/// `model.material.delete`.
pub fn material_delete(s: &mut Session, p: &Params) -> Result<Value> {
    let (_, m) = material_named(&s.model, p, "name")?;
    let name = m.name.clone();
    if name == Material::vacuum().name {
        return Err(EngineError::failed("material `vacuum` is built in: every model keeps it"));
    }
    let uses = s.model.material_uses(&name);
    if !uses.is_empty() {
        let users = crate::text::listed(&uses_shown(&s.model, &uses), LISTED).join(", ");
        return Err(EngineError::failed(format!("material `{name}` is used by {users}: give them another material first")));
    }
    s.model.remove_material(&name)?;
    Ok(json!({"name": name, "materials": s.model.materials.len()}))
}

/// The waveguide port parameter `name` names, and its index; a lumped port's name says which
/// command it takes.
pub(crate) fn port_named<'m>(m: &'m Model, p: &Params, param: &str) -> Result<(usize, &'m Port)> {
    let name = p.string(param).required()?;
    if let Some(found) = m.ports.iter().enumerate().find(|(_, x)| x.name == name) {
        return Ok(found);
    }
    if m.lumped.iter().any(|x| x.name == name) {
        return Err(p.error(
            param,
            format!("names a lumped port, `{}`: model.port.lumped.edit and model.port.lumped.delete take it", crate::text::clip(name, 64)),
        ));
    }
    Err(none_named(p, param, "waveguide port", name, m.ports.iter().map(|x| x.name.as_str())))
}

/// The lumped port parameter `name` names, and its index.
pub(crate) fn lumped_named<'m>(m: &'m Model, p: &Params, param: &str) -> Result<(usize, &'m emcraft_model::LumpedPort)> {
    let name = p.string(param).required()?;
    if let Some(found) = m.lumped.iter().enumerate().find(|(_, x)| x.name == name) {
        return Ok(found);
    }
    if m.ports.iter().any(|x| x.name == name) {
        return Err(
            p.error(param, format!("names a waveguide port, `{}`: model.port.edit and model.port.delete take it", crate::text::clip(name, 64)))
        );
    }
    Err(none_named(p, param, "lumped port", name, m.lumped.iter().map(|x| x.name.as_str())))
}

/// A port change drops the open problem's results too when the port feeds one.
fn drop_open_results(s: &mut Session) {
    if s.model.open.is_some() {
        s.results.invalidate(Change::Open);
    }
}

/// `model.port.edit`.
pub fn port_edit(s: &mut Session, p: &Params) -> Result<Value> {
    let (i, old) = port_named(&s.model, p, "name").map(|(i, x)| (i, x.clone()))?;
    let axis: Option<emcraft_model::Axis> = p.choice("axis").optional()?;
    let face = if p.cleared("face") {
        None
    } else if p.has("face") {
        let was = old.face.map_or([0.0; 5], |f| [f.at, f.u0, f.v0, f.a, f.b]);
        let list = p.list("face", 5).required()?;
        if list.len() != 5 {
            return Err(p.error("face", "must be [at, u0, v0, a, b]"));
        }
        // Each coordinate kept exactly when given as it shows.
        let unit = s.display.length;
        let mut v = [0.0; 5];
        for (k, x) in list.iter().enumerate() {
            let given = p.number_of("face", x)?;
            v[k] = if given == unit.from_si(was[k]) { was[k] } else { unit.to_si(given) };
        }
        Some(PortFace { axis: axis.or(old.face.map(|f| f.axis)).unwrap_or_default(), at: v[0], u0: v[1], v0: v[2], a: v[3], b: v[4] })
    } else {
        match (old.face, axis) {
            (Some(f), Some(axis)) => Some(PortFace { axis, ..f }),
            (None, Some(a)) if a != emcraft_model::Axis::Z => return Err(p.error("axis", "x or y needs `face` (a port's end lies across z)")),
            (f, _) => f,
        }
    };
    let port = Port {
        name: p.string("rename").optional()?.unwrap_or(&old.name).to_owned(),
        end: p.choice("end").or(old.end)?,
        modes: p.integer("modes", 1..=emcraft_model::MAX_PORT_MODES as usize).optional()?.map_or(old.modes, |n| n as u32),
        face,
        mode_solver: p.choice("mode_solver").or(old.mode_solver)?,
        reference: if p.cleared("impedance") { None } else { p.number("impedance").optional()?.or(old.reference) },
        deembed: p.edit_length("deembed", old.deembed)?,
        terminals: p.choice("terminals").or(old.terminals)?,
    };
    let name = port.name.clone();
    s.model.replace_port(i, port)?;
    drop_open_results(s);
    Ok(json!({"name": name, "ports": s.model.ports.len()}))
}

/// `model.port.deembed`: port `name`'s reference plane `deembed` inside the model (as
/// `model.port.edit` sets it), the sweep's S-parameters re-referred to it at once — no solve —
/// where its ports allow (each port's first mode's e^{γΔd}); a sweep with terminal ports or
/// periodic boundaries is dropped instead, to be solved again.
pub fn port_deembed(s: &mut Session, p: &Params) -> Result<Value> {
    let (i, old) = port_named(&s.model, p, "name").map(|(i, x)| (i, x.clone()))?;
    let deembed = p.edit_length("deembed", old.deembed)?;
    let name = old.name.clone();
    s.model.replace_port(i, Port { deembed, ..old })?;
    let mut rerun = false;
    if let Some(sw) = s.results.sweep.clone() {
        if sw.driven.has_terminals() || sw.driven.periodic.is_some() || i >= sw.deembed.len() {
            s.results.sweep = None;
            rerun = true;
        } else {
            let mut to = sw.deembed.clone();
            if let Some(d) = to.get_mut(i) {
                *d = deembed;
            }
            let results = crate::session::rereference(&sw.driven, sw.results.clone(), &sw.deembed, &to)?;
            s.results.sweep = Some(std::sync::Arc::new(crate::Sweep { results, deembed: to, ..(*sw).clone() }));
        }
    }
    let l = s.display.length;
    Ok(json!({"name": name, "deembed": l.from_si(deembed), "rerun": rerun}))
}

/// `model.port.delete`.
pub fn port_delete(s: &mut Session, p: &Params) -> Result<Value> {
    let (i, _) = port_named(&s.model, p, "name")?;
    let gone = s.model.remove_port(i)?;
    drop_open_results(s);
    Ok(json!({"name": gone.name, "ports": s.model.ports.len()}))
}

/// A lumped port's source as `voltage` (amplitude, V) and `phase` (degrees).
pub(crate) fn polar(v: [f64; 2]) -> (f64, f64) {
    (v[0].hypot(v[1]), v[1].atan2(v[0]).to_degrees())
}

/// `model.port.lumped.edit`.
pub fn lumped_edit(s: &mut Session, p: &Params) -> Result<Value> {
    let (i, old) = lumped_named(&s.model, p, "name").map(|(i, x)| (i, x.clone()))?;
    let (a0, phase0) = polar(old.voltage);
    let (a, phase) = (p.number("voltage").or(a0)?, p.number("phase").or(phase0)?);
    let voltage = if (a, phase) == (a0, phase0) { old.voltage } else { [a * phase.to_radians().cos(), a * phase.to_radians().sin()] };
    let port = emcraft_model::LumpedPort {
        name: p.string("rename").optional()?.unwrap_or(&old.name).to_owned(),
        from: p.edit_point("from", old.from)?,
        to: p.edit_point("to", old.to)?,
        impedance: p.number("impedance").or(old.impedance)?,
        voltage,
        radius: p.edit_length("radius", old.radius)?,
    };
    let name = port.name.clone();
    s.model.replace_lumped_port(i, port)?;
    Ok(json!({"name": name, "lumped_ports": s.model.lumped.len()}))
}

/// `model.port.lumped.delete`.
pub fn lumped_delete(s: &mut Session, p: &Params) -> Result<Value> {
    let (i, _) = lumped_named(&s.model, p, "name")?;
    let gone = s.model.remove_lumped_port(i)?;
    Ok(json!({"name": gone.name, "lumped_ports": s.model.lumped.len()}))
}

/// The source parameter `index` numbers (1 = the first) and its index from 0.
pub(crate) fn source_numbered(m: &Model, p: &Params) -> Result<(usize, Source)> {
    let n = m.sources.len();
    if n == 0 {
        return Err(p.error("index", "names no source: the model has none (source.dipole, source.plane_wave)"));
    }
    let k = p.integer("index", 1..=n).required()?;
    m.sources.get(k - 1).map(|x| (k - 1, *x)).ok_or_else(|| p.error("index", format!("must be 1 to {n}")))
}

/// `source.edit`: a dipole takes `at`, `direction`, `length` and `current`; a plane wave
/// `direction` and `polarization`.
pub fn source_edit(s: &mut Session, p: &Params) -> Result<Value> {
    let (i, old) = source_numbered(&s.model, p)?;
    let source = match old {
        Source::Dipole { at, direction, length, current } => {
            if p.has("polarization") {
                return Err(p.error("polarization", "is a plane wave's: this source is a dipole"));
            }
            Source::Dipole {
                at: p.edit_point("at", at)?,
                direction: p.vector("direction").or(direction)?,
                length: p.edit_positive_length("length", length)?,
                current: p.number("current").or(current)?,
            }
        }
        Source::PlaneWave { direction, polarization } => {
            if let Some(k) = ["at", "length", "current"].into_iter().find(|k| p.has(k)) {
                return Err(p.error(k, "is a dipole's: this source is a plane wave"));
            }
            Source::PlaneWave { direction: p.vector("direction").or(direction)?, polarization: p.vector("polarization").or(polarization)? }
        }
    };
    s.model.replace_source(i, source)?;
    Ok(json!({"index": i + 1, "sources": s.model.sources.len()}))
}

/// `source.delete`.
pub fn source_delete(s: &mut Session, p: &Params) -> Result<Value> {
    let (i, _) = source_numbered(&s.model, p)?;
    s.model.remove_source(i)?;
    Ok(json!({"index": i + 1, "sources": s.model.sources.len()}))
}

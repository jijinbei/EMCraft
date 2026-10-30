//! `project.values`: the parameters that reproduce the model's current state for a command, in
//! display units, so an edit dialog opens on the model rather than on an example: running the
//! command with them changes nothing in the model (to the last bit for the item edits,
//! [`Params::edit_length`]; through the display unit and back for the others). Solve commands
//! give their last run's ([`crate::runs`]), the mesh commands the parameters of the history's
//! last of them. A command with no current state (nothing set yet, or one that sets nothing)
//! gives an empty object.

use emcraft_model::units::Display;
use emcraft_model::{Model, OpenShape, Polarization, Shape, Source};
use serde_json::{Map, Value, json};

use crate::bodies::{BodyRef, METAL};
use crate::items::{lumped_named, material_named, polar, port_named, shown_poles, source_numbered};
use crate::{EngineError, Params, Result, Session, Step, find_command};

/// `project.values`.
pub fn values(s: &mut Session, p: &Params) -> Result<Value> {
    let id = p.string("command").required()?;
    let spec = find_command(id).ok_or_else(|| p.error("command", format!("names no command: `{}`", crate::text::clip(id, 100))))?;
    let m = &s.model;
    let l = |x: f64| s.display.length.from_si(x);
    let f = |x: f64| s.display.frequency.from_si(x);
    let pt = |q: emcraft_model::Point3| json!([l(q.x), l(q.y), l(q.z)]);
    let v = match spec.id {
        "project.frequency" => m.band.map_or_else(|| json!({}), |b| json!({"start": f(b.start), "stop": f(b.stop)})),
        "units.set" => crate::reply::units(s.display),
        "solve.device" => json!({"device": s.compute.device(), "precision": s.compute.precision.id()}),
        "model.walls" => m.wall_conductivity.map_or_else(|| json!({}), |c| json!({"conductivity": c})),
        "model.open" => match m.open {
            None => json!({}),
            Some(o) => {
                let mut v = json!({"thickness": l(o.thickness), "shape": match o.shape { OpenShape::Box => "box", OpenShape::Sphere => "sphere" }});
                if let Some(r) = o.reflection {
                    v["reflection"] = json!(r);
                }
                v
            }
        },
        "model.symmetry" => {
            let axis = p.string("axis").optional()?;
            match m.symmetry.iter().find(|q| axis.is_none_or(|a| q.axis.id() == a)) {
                Some(q) => json!({"axis": q.axis.id(), "kind": q.kind, "at": l(q.at)}),
                None => json!({}),
            }
        }
        "model.periodic" => match m.periodic {
            None => json!({}),
            Some(q) => json!({
                "x": q.x, "y": q.y, "z": q.z, "theta": q.theta.to_degrees(), "phi": q.phi.to_degrees(),
                "polarization": match q.polarization { Polarization::Te => "te", Polarization::Tm => "tm" },
            }),
        },
        "geometry.waveguide" => waveguide(m, s.display),
        "geometry.solid" => match &m.solid {
            Some(solid) if !matches!(solid.shape, Shape::Surface { .. }) => {
                let mut v = shape(&solid.shape, s.display);
                v["material"] = json!(solid.material);
                v
            }
            _ => json!({}),
        },
        "mesh.generate" | "mesh.adapt" => last_step(s, spec.id)?,
        "model.material.edit" => {
            let (_, mat) = material_named(m, p, "name")?;
            let mut v = json!({
                "name": mat.name, "eps_r": mat.eps_r, "mu_r": mat.mu_r,
                "loss_tangent": mat.loss_tangent, "magnetic_loss_tangent": mat.magnetic_loss_tangent,
            });
            for kind in crate::geometry::POLE_KINDS {
                let poles = shown_poles(mat, kind, s.display);
                if !poles.is_empty() {
                    v[kind] = json!(poles);
                }
            }
            v
        }
        "model.port.edit" => {
            let (_, port) = port_named(m, p, "name")?;
            let mut v = json!({
                "name": port.name, "end": port.end, "modes": port.modes, "mode_solver": port.mode_solver,
                "deembed": l(port.deembed), "terminals": port.terminals,
            });
            if let Some(face) = port.face {
                v["face"] = json!([l(face.z), l(face.x0), l(face.y0), l(face.a), l(face.b)]);
            }
            if let Some(r) = port.reference {
                v["impedance"] = json!(r);
            }
            v
        }
        "model.port.lumped.edit" => {
            let (_, port) = lumped_named(m, p, "name")?;
            let (voltage, phase) = polar(port.voltage);
            json!({
                "name": port.name, "from": pt(port.from), "to": pt(port.to), "impedance": port.impedance,
                "voltage": voltage, "phase": phase, "radius": l(port.radius),
            })
        }
        "source.edit" => {
            let (i, src) = source_numbered(m, p)?;
            match src {
                Source::Dipole { at, direction, length, current } => {
                    json!({"index": i + 1, "at": pt(at), "direction": [direction.x, direction.y, direction.z], "length": l(length), "current": current})
                }
                Source::PlaneWave { direction, polarization } => json!({
                    "index": i + 1, "direction": [direction.x, direction.y, direction.z],
                    "polarization": [polarization.x, polarization.y, polarization.z],
                }),
            }
        }
        "geometry.body.edit" => {
            let b = crate::bodies::body_named(m, p)?;
            let (body_shape, at, axis) = crate::bodies::placement(m, b.at).ok_or_else(|| EngineError::failed("the body is gone from the model"))?;
            let mut v = shape(&body_shape, s.display);
            v["body"] = json!(b.name);
            v["at"] = pt(at);
            v["axis"] = json!(axis.id());
            // An intersection (`clip`) has no material to give; a conducting inclusion is metal.
            let material = match b.at {
                BodyRef::Inclusion(i) => m.inclusions.get(i).map(|inc| inc.material.clone().unwrap_or_else(|| METAL.to_owned())),
                BodyRef::Feature(_) => (b.material != "clip").then(|| b.material.clone()),
            };
            if let Some(mat) = material {
                v["material"] = json!(mat);
            }
            v
        }
        _ if spec.effect == crate::Effect::Solve => match s.runs.iter().find(|r| r.command == spec.id) {
            Some(run) => crate::runs::shown(s, run)?,
            None => json!({}),
        },
        _ => json!({}),
    };
    Ok(v)
}

/// A primitive's `shape` and dimensions, as geometry.solid and geometry.body take them; an
/// imported surface's nothing (its shape is its file's).
fn shape(shape: &Shape, d: Display) -> Value {
    let l = |x: f64| d.length.from_si(x);
    match shape {
        Shape::Box { size } => json!({"shape": "box", "size": [l(size.x), l(size.y), l(size.z)]}),
        Shape::Cylinder { radius, length } => json!({"shape": "cylinder", "radius": l(*radius), "length": l(*length)}),
        Shape::Sphere { radius } => json!({"shape": "sphere", "radius": l(*radius)}),
        Shape::Surface { .. } => json!({}),
    }
}

/// The model's waveguide as geometry.waveguide's parameters; nothing without one.
fn waveguide(m: &Model, d: Display) -> Value {
    let Some(w) = &m.waveguide else { return json!({}) };
    let l = |x: f64| d.length.from_si(x);
    let sections: Vec<Value> = w
        .sections
        .iter()
        .map(|sec| {
            let mut o = Map::new();
            o.insert("length".into(), json!(l(sec.length)));
            o.insert("material".into(), json!(sec.material));
            for (k, v) in [("width", sec.width), ("offset", sec.offset), ("height", sec.height), ("offset_y", sec.offset_y)] {
                if let Some(v) = v {
                    o.insert(k.into(), json!(l(v)));
                }
            }
            Value::Object(o)
        })
        .collect();
    let mut v = json!({"width": l(w.width), "height": l(w.height), "sections": sections});
    if !w.layers.is_empty() {
        v["layers"] = w.layers.iter().map(|x| json!({"thickness": l(x.thickness), "material": x.material})).collect();
    }
    if !w.strips.is_empty() {
        v["strips"] = w.strips.iter().map(|x| json!({"y": l(x.y), "x0": l(x.x0), "width": l(x.width), "z0": l(x.z0), "z1": l(x.z1)})).collect();
    }
    if !w.vias.is_empty() {
        v["vias"] =
            w.vias.iter().map(|x| json!({"x0": l(x.x0), "z0": l(x.z0), "wx": l(x.wx), "wz": l(x.wz), "y0": l(x.y0), "y1": l(x.y1)})).collect();
    }
    v
}

/// The parameters of the history's last `id` step, in the display units now; nothing without
/// one.
fn last_step(s: &Session, id: &str) -> Result<Value> {
    let Some(k) = s.history.iter().rposition(|st| st.command == id) else { return Ok(json!({})) };
    let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_owned()))?;
    let (Some(step), Some(before)) = (s.history.get(k), s.history.get(..k)) else { return Ok(json!({})) };
    crate::runs::in_units(spec, &step.params, display_at(before), s.display, &s.variables)
}

/// The display units after `steps` (a history from a fresh session's units): its `units.set`
/// steps in turn.
pub(crate) fn display_at(steps: &[Step]) -> Display {
    let mut d = Display::default();
    for st in steps.iter().filter(|st| st.command == "units.set") {
        if let Some(length) = st.param("length").and_then(|v| serde_json::from_value(v.clone()).ok()) {
            d.length = length;
        }
        if let Some(frequency) = st.param("frequency").and_then(|v| serde_json::from_value(v.clone()).ok()) {
            d.frequency = frequency;
        }
    }
    d
}

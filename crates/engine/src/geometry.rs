//! Commands that build the model: materials, the waveguide, solids, imported surfaces,
//! inclusions and features, walls and ports.

use emcraft_model::{
    Axis, Feature, Inclusion, Layer, Material, Point3, Port, PortFace, Section, Shape, Solid, Strip, Symmetry, SymmetryKind, Waveguide,
};
use serde_json::{Value, json};

use crate::{EngineError, Params, Result, Session};

/// Most items in one list parameter (sections, layers, strips).
const MAX_ITEMS: usize = 1000;

pub fn material(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    let material =
        Material::new(name, p.number("eps_r").required()?, p.number("mu_r").or(1.0)?)?.with_loss_tangent(p.number("loss_tangent").or(0.0)?)?;
    s.model.add_material(material)?;
    Ok(json!({"name": name}))
}

pub fn waveguide(s: &mut Session, p: &Params) -> Result<Value> {
    let width = p.positive_length("width").required()?;
    let height = p.positive_length("height").required()?;
    p.list("sections", MAX_ITEMS).required()?;
    let sections = p
        .items("sections", "section", MAX_ITEMS)?
        .iter()
        .map(|sec| {
            Ok(Section {
                length: sec.positive_length("length").required()?,
                material: sec.string("material").required()?.to_owned(),
                width: sec.positive_length("width").optional()?,
                offset: sec.length("offset").optional()?,
                height: sec.positive_length("height").optional()?,
                offset_y: sec.length("offset_y").optional()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let total: f64 = sections.iter().map(|x| x.length).sum();
    let layers = p
        .items("layers", "layer", MAX_ITEMS)?
        .iter()
        .map(|l| Ok(Layer { thickness: l.positive_length("thickness").required()?, material: l.string("material").required()?.to_owned() }))
        .collect::<Result<Vec<_>>>()?;
    let strips = p
        .items("strips", "strip", MAX_ITEMS)?
        .iter()
        .map(|st| {
            Ok(Strip {
                y: st.length("y").required()?,
                x0: st.length("x0").required()?,
                width: st.positive_length("width").required()?,
                z0: st.length("z0").or(0.0)?,
                z1: st.length("z1").or(total)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    s.model.set_waveguide(Waveguide { width, height, sections, layers, strips })?;
    s.results.sweep = None;
    Ok(json!({"length": s.show_length(s.model.waveguide.as_ref().map_or(0.0, Waveguide::length))}))
}

/// A primitive shape (`shape`).
#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Primitive {
    Box,
    Cylinder,
    Sphere,
}

/// A shape from `shape`, `size`, `radius` and `length`.
fn shape(p: &Params) -> Result<Shape> {
    let dimension = |name| p.positive_length(name).required_or("for this shape");
    Ok(match p.choice("shape").required()? {
        Primitive::Box => Shape::Box { size: p.point("size").required()?.to_vec() },
        Primitive::Cylinder => Shape::Cylinder { radius: dimension("radius")?, length: dimension("length")? },
        Primitive::Sphere => Shape::Sphere { radius: dimension("radius")? },
    })
}

pub fn inclusion(s: &mut Session, p: &Params) -> Result<Value> {
    let inclusion =
        Inclusion { shape: shape(p)?, at: p.point("at").or(Point3::ORIGIN)?, material: p.string("material").optional()?.map(str::to_owned) };
    s.model.add_inclusion(inclusion)?;
    s.results.sweep = None;
    s.results.eigen = None;
    Ok(json!({"inclusions": s.model.inclusions.len()}))
}

/// Largest geometry file read.
const MAX_IMPORT_BYTES: u64 = 512 << 20;

/// A file's bytes, refusing one over `max` bytes before reading it.
pub(crate) fn read_capped(path: &str, max: u64) -> Result<Vec<u8>> {
    let len = std::fs::metadata(path).map_err(|e| EngineError::Failed(format!("`{path}`: {e}")))?.len();
    if len > max {
        return Err(EngineError::Failed(format!("`{path}` is {} MB; at most {} MB are read", len >> 20, max >> 20)));
    }
    std::fs::read(path).map_err(|e| EngineError::Failed(format!("`{path}`: {e}")))
}

pub fn import(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let unit = s.display.length.to_si(p.positive("unit").or(1.0)?);
    let angle = p.number("angle").check(|a| *a > 0.0 && *a < 180.0, "between 0 and 180 degrees").or(30.0)?;
    let material = p.string("material").or("vacuum")?.to_owned();
    let bytes = read_capped(path, MAX_IMPORT_BYTES)?;
    let mut surface = emcraft_io::read(path, &bytes, unit)?;
    let faces = emcraft_io::split_faces(&mut surface, angle);
    let triangles = surface.triangles.len();
    let shape = Shape::Surface { name: surface.name, positions: surface.positions, triangles: surface.triangles, tags: surface.tags };
    s.model.set_solid(Solid { shape, material })?;
    s.results.sweep = None;
    s.results.eigen = None;
    s.results.time_domain = None;
    let bounds = s.model.bounds().unwrap_or_default();
    Ok(json!({
        "triangles": triangles,
        "faces": faces,
        "volume": s.model.solid.as_ref().map(|x| x.shape.volume()),
        "bounds": bounds.corners().map(|c| c.map(|v| s.show_length(v))),
    }))
}

pub fn feature(s: &mut Session, p: &Params) -> Result<Value> {
    let feature = Feature {
        combine: p.choice("combine").required()?,
        shape: shape(p)?,
        at: p.point("at").or(Point3::ORIGIN)?,
        axis: p.choice("axis").or_default()?,
    };
    s.model.add_feature(feature)?;
    s.results.sweep = None;
    s.results.eigen = None;
    s.results.time_domain = None;
    Ok(json!({"features": s.model.features.len()}))
}

pub fn solid(s: &mut Session, p: &Params) -> Result<Value> {
    let shape = shape(p)?;
    if let Shape::Box { size } = shape
        && size.iter().any(|v| !(v > 0.0))
    {
        return Err(p.error("size", "must be positive"));
    }
    s.model.set_solid(Solid { shape, material: p.string("material").or("vacuum")?.to_owned() })?;
    s.results.sweep = None;
    s.results.eigen = None;
    s.results.time_domain = None;
    Ok(json!({"volume": s.model.solid.as_ref().map(|x| x.shape.volume())}))
}

pub fn walls(s: &mut Session, p: &Params) -> Result<Value> {
    let sigma = p.number("conductivity").optional()?;
    s.model.set_wall_conductivity(sigma)?;
    s.results.sweep = None;
    Ok(json!({"conductivity": sigma}))
}

/// A plane's axis for `model.symmetry` (z is not one: the ports lie on z planes).
#[derive(serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum PlaneAxis {
    X,
    Y,
}

pub fn symmetry(s: &mut Session, p: &Params) -> Result<Value> {
    let clear = p.flag("clear").or(false)?;
    let axis = p.choice::<PlaneAxis>("axis").optional()?.map(|a| match a {
        PlaneAxis::X => Axis::X,
        PlaneAxis::Y => Axis::Y,
    });
    let mut planes = s.model.symmetry.clone();
    if clear {
        if p.has("kind") || p.has("at") {
            return Err(p.error("clear", "takes only `axis` (the plane to remove; all of them without it)"));
        }
        planes.retain(|q| axis.is_some_and(|a| q.axis != a));
    } else {
        let axis = axis.ok_or_else(|| p.error("axis", "is required (or `clear`)"))?;
        let kind: SymmetryKind = p.choice("kind").required()?;
        // The guide's middle by default: the only plane its walls are symmetric about.
        let middle = s.model.bounds().map(|b| 0.5 * (b.min[axis.index()] + b.max[axis.index()]));
        let at = p.length("at").or_maybe(middle).required_or("no geometry to take the middle of")?;
        planes.retain(|q| q.axis != axis);
        planes.push(Symmetry { axis, at, kind });
        planes.sort_by_key(|q| q.axis.index());
    }
    s.model.set_symmetry(planes)?;
    s.results.sweep = None;
    s.results.eigen = None;
    s.results.time_domain = None;
    let shown: Vec<Value> = s
        .model
        .symmetry
        .iter()
        .map(|q| json!({"axis": if q.axis == Axis::X { "x" } else { "y" }, "at": s.show_length(q.at), "kind": q.kind}))
        .collect();
    Ok(json!({"planes": shown, "fraction": s.model.symmetry_fraction()}))
}

pub fn port(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    let face = p
        .lengths("face")
        .and_then(|f| match f[..] {
            [z, x0, y0, a, b] => Ok(PortFace { z, x0, y0, a, b }),
            _ => Err(p.error("face", "must be [z, x0, y0, a, b]")),
        })
        .optional()?;
    let port = Port {
        name: name.to_owned(),
        end: p.choice("end").required()?,
        modes: p.integer("modes", 1..=emcraft_model::MAX_PORT_MODES as usize).or(4)? as u32,
        face,
        mode_solver: p.choice("mode_solver").or_default()?,
        reference: p.number("impedance").optional()?,
    };
    s.model.add_port(port)?;
    s.results.sweep = None;
    Ok(json!({"name": name}))
}

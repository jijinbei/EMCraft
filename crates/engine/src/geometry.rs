//! Commands that build the model: materials, the waveguide, solids, imported surfaces,
//! inclusions and features, walls and ports.

use emcraft_model::{
    Axis, Feature, Inclusion, Layer, MAX_POLES, Material, Point3, Pole, Port, PortFace, Section, Shape, Solid, Strip, Symmetry, SymmetryKind, Via,
    Waveguide,
};
use serde_json::{Value, json};

use crate::{Change, EngineError, Params, Result, Session};

/// Most items in one list parameter (sections, layers, strips, vias).
const MAX_ITEMS: usize = 1000;

pub fn material(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    let material =
        Material::new(name, p.number("eps_r").required()?, p.number("mu_r").or(1.0)?)?.with_loss_tangent(p.number("loss_tangent").or(0.0)?)?;
    let material = material.with_dispersion(poles(p)?)?;
    s.model.add_material(material)?;
    Ok(json!({"name": name}))
}

/// A material's dispersion poles from `debye`, `lorentz` and `drude` (frequencies in display
/// units): a Debye pole's relaxation frequency f_r gives τ = 1/(2π f_r).
fn poles(p: &Params) -> Result<Vec<Pole>> {
    let mut poles = Vec::new();
    for d in p.items("debye", "Debye pole", MAX_POLES)? {
        let delta_eps = d.positive("delta_eps").required()?;
        let f = d.frequency("frequency").required()?;
        poles.push(Pole::Debye { delta_eps, tau: 1.0 / (2.0 * std::f64::consts::PI * f) });
    }
    for l in p.items("lorentz", "Lorentz pole", MAX_POLES)? {
        let delta_eps = l.positive("delta_eps").required()?;
        let frequency = l.frequency("frequency").required()?;
        let damping = l.frequency("damping").required()?;
        poles.push(Pole::Lorentz { delta_eps, frequency, damping });
    }
    for d in p.items("drude", "Drude pole", MAX_POLES)? {
        let plasma_frequency = d.frequency("plasma_frequency").required()?;
        let damping = d.frequency("damping").required()?;
        poles.push(Pole::Drude { plasma_frequency, damping });
    }
    if poles.len() > MAX_POLES {
        return Err(EngineError::Params(format!("a material holds at most {MAX_POLES} poles, got {}", poles.len())));
    }
    Ok(poles)
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
    let vias = p
        .items("vias", "via", MAX_ITEMS)?
        .iter()
        .map(|v| {
            Ok(Via {
                x0: v.length("x0").required()?,
                z0: v.length("z0").required()?,
                wx: v.positive_length("wx").required()?,
                wz: v.positive_length("wz").required()?,
                y0: v.length("y0").or(0.0)?,
                y1: v.length("y1").or(height)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    s.model.set_waveguide(Waveguide { width, height, sections, layers, strips, vias })?;
    s.results.invalidate(Change::Geometry);
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
pub(crate) fn shape(p: &Params) -> Result<Shape> {
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
    s.results.invalidate(Change::Geometry);
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
    let mut bodies = emcraft_io::read_bodies(path, &bytes, unit)?;
    if bodies.is_empty() {
        return Err(EngineError::Failed(format!("`{path}` holds no body")));
    }
    let faces: u32 = bodies.iter_mut().map(|b| emcraft_io::split_faces(b, angle)).sum();
    let triangles: usize = bodies.iter().map(|b| b.triangles.len()).sum();
    let materials = body_materials(p, &bodies)?;
    // An open problem: the box of air lined by a perfectly matched layer, a quarter of the
    // band's longest wavelength of air between the parts and the layer and as much layer.
    let open = p.flag("open").or(false)?;
    let quarter = s.model.band.map(|b| emcraft_model::units::C0 / b.start / 4.0);
    let thickness = match (open, p.positive_length("thickness").optional()?) {
        (false, _) => None,
        (true, Some(t)) => Some(t),
        (true, None) => Some(quarter.ok_or_else(|| {
            p.error("thickness", "or set the band (project.frequency): the layer and the air before it are a quarter wavelength each")
        })?),
    };
    // The region the fields fill: one body alone (or the one named), else a box of air around
    // them all (always for an open problem).
    let region = p
        .string("region")
        .optional()?
        .map(str::to_owned)
        .unwrap_or_else(|| if bodies.len() == 1 && !open { bodies[0].name.clone() } else { "box".into() });
    if open && region != "box" {
        return Err(p.error("region", "an open problem lies in a box of air (`box`)"));
    }
    if region == "box" {
        let margin = p.positive_length("margin").optional()?;
        s.model.set_solid(Solid { shape: air_box(p, &mut bodies, margin, thickness, quarter)?, material: material.clone() })?;
    } else {
        let k = bodies.iter().position(|b| b.name == region).ok_or_else(|| p.error("region", format!("names no body `{region}` (or `box`)")))?;
        let body = bodies.remove(k);
        s.model.set_solid(Solid { shape: surface(body), material: material.clone() })?;
    }
    let mut placed = Vec::new();
    for b in bodies {
        let m = materials.iter().find(|(n, _)| *n == b.name).map_or(crate::bodies::METAL.to_owned(), |(_, m)| m.clone());
        let name = b.name.clone();
        let kind = crate::bodies::place(&mut s.model, surface(b), Point3::ORIGIN, Axis::Z, &m)?;
        placed.push(json!({"body": name, "material": m, "as": kind}));
    }
    if let Some(t) = thickness {
        s.model.set_open(Some(emcraft_model::Open { thickness: t, reflection: None, shape: emcraft_model::OpenShape::Box }))?;
    }
    s.results.invalidate(Change::Geometry);
    let bounds = s.model.bounds().unwrap_or_default();
    Ok(json!({
        "triangles": triangles,
        "faces": faces,
        "region": region,
        "bodies": placed,
        "open": thickness.map(|t| s.show_length(t)),
        "volume": s.model.solid.as_ref().map(|x| x.shape.volume()),
        "bounds": bounds.corners().map(|c| c.map(|v| s.show_length(v))),
    }))
}

/// An imported body as a shape.
fn surface(b: emcraft_io::Surface) -> Shape {
    Shape::Surface { name: b.name, positions: b.positions, triangles: b.triangles, tags: b.tags }
}

/// The bodies' materials from `materials` (body name → material); the others are metal.
fn body_materials(p: &Params, bodies: &[emcraft_io::Surface]) -> Result<Vec<(String, String)>> {
    let mut materials: Vec<(String, String)> = Vec::new();
    if let Some(v) = p.raw("materials") {
        let o = v.as_object().ok_or_else(|| p.error("materials", "must map body names to materials (or \"metal\")"))?;
        for (body, m) in o {
            let m = m.as_str().ok_or_else(|| p.error("materials", format!("`{body}`: the material must be a name")))?;
            if !bodies.iter().any(|b| &b.name == body) {
                return Err(p.error("materials", format!("names no body `{body}`")));
            }
            materials.push((body.clone(), m.to_owned()));
        }
    }
    Ok(materials)
}

/// The box of air around `bodies`, from the origin, the bodies moved into it: `margin` beyond
/// them all; by default a quarter of their largest extent, or with a layer of `thickness` as
/// much layer again and a `quarter` wavelength of air.
fn air_box(p: &Params, bodies: &mut [emcraft_io::Surface], margin: Option<f64>, thickness: Option<f64>, quarter: Option<f64>) -> Result<Shape> {
    let lo = bodies.iter().flat_map(|b| b.positions.iter()).fold([f64::INFINITY; 3], |m, q| [0, 1, 2].map(|d| m[d].min(q[d])));
    let hi = bodies.iter().flat_map(|b| b.positions.iter()).fold([f64::NEG_INFINITY; 3], |m, q| [0, 1, 2].map(|d| m[d].max(q[d])));
    let extent = (0..3).map(|d| hi[d] - lo[d]).fold(0.0f64, f64::max);
    let margin = match (margin, thickness) {
        (Some(m), Some(t)) if m <= t => {
            return Err(p.error("margin", "must exceed the layer's thickness: air must lie between the parts and the layer"));
        }
        (Some(m), _) => m,
        (None, Some(t)) => t + quarter.unwrap_or(t),
        (None, None) => 0.25 * extent,
    };
    let shift = emcraft_model::Vec3::new(margin - lo[0], margin - lo[1], margin - lo[2]);
    for b in bodies.iter_mut() {
        b.positions.iter_mut().for_each(|q| *q += shift);
    }
    let size = emcraft_model::Vec3::new(hi[0] - lo[0] + 2.0 * margin, hi[1] - lo[1] + 2.0 * margin, hi[2] - lo[2] + 2.0 * margin);
    Ok(Shape::Box { size })
}

pub fn feature(s: &mut Session, p: &Params) -> Result<Value> {
    let feature = Feature {
        combine: p.choice("combine").required()?,
        shape: shape(p)?,
        at: p.point("at").or(Point3::ORIGIN)?,
        axis: p.choice("axis").or_default()?,
    };
    s.model.add_feature(feature)?;
    s.results.invalidate(Change::Geometry);
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
    s.results.invalidate(Change::Geometry);
    Ok(json!({"volume": s.model.solid.as_ref().map(|x| x.shape.volume())}))
}

pub fn walls(s: &mut Session, p: &Params) -> Result<Value> {
    let sigma = p.number("conductivity").optional()?;
    s.model.set_wall_conductivity(sigma)?;
    s.results.invalidate(Change::Walls);
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
    s.results.invalidate(Change::Geometry);
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
        deembed: p.length("deembed").or(0.0)?,
    };
    s.model.add_port(port)?;
    s.results.invalidate(Change::Ports);
    Ok(json!({"name": name}))
}

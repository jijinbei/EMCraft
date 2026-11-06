//! Bodies placed in the field's region by what they are made of, the way a model is drawn in
//! CAD: metal (a perfect conductor, which the region loses: its surface becomes wall), the
//! region's own material (giving back what metal took, an opening in a metal body), or another
//! material (a dielectric body, meshed apart and joined to the region). Later bodies win where
//! they overlap. Underneath they are the solid's features and inclusions.

use emcraft_model::{Axis, BoxSize, Combine, Feature, Inclusion, Model, Point3, Shape};
use serde_json::{Value, json};

use crate::{EngineError, Params, Result, Session};

/// The material name for a perfect conductor.
pub const METAL: &str = "metal";

/// The material name for an interior: the inside of a housing drawn as its air, the region.
pub const INTERIOR: &str = "interior";

/// Where a body lives in the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyRef {
    Feature(usize),
    Inclusion(usize),
}

/// A body as the user sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    /// An imported body's own name, else `body <n>`.
    pub name: String,
    /// `metal`, the region's material, another material, or `clip` (an intersection).
    pub material: String,
    pub shape: &'static str,
    pub at: BodyRef,
}

/// The model's bodies, features then inclusions, in the order they were placed.
pub fn bodies(m: &Model) -> Vec<Body> {
    let region = m.domain.solid.as_ref().map_or("vacuum", |s| s.material.as_str());
    let name = |shape: &Shape, n: usize| match shape {
        Shape::Surface { name, .. } if !name.is_empty() => name.clone(),
        _ => format!("body {n}"),
    };
    let mut out: Vec<Body> = Vec::with_capacity(m.features().len() + m.inclusions().len());
    for (i, f) in m.features().iter().enumerate() {
        let material = match f.combine {
            Combine::Subtract => METAL.to_owned(),
            Combine::Add => region.to_owned(),
            Combine::Intersect => "clip".to_owned(),
        };
        out.push(Body { name: name(&f.shape, out.len() + 1), material, shape: f.shape.kind(), at: BodyRef::Feature(i) });
    }
    for (i, inc) in m.inclusions().iter().enumerate() {
        let material = inc.material.clone().unwrap_or_else(|| METAL.to_owned());
        out.push(Body { name: name(&inc.shape, out.len() + 1), material, shape: inc.shape.kind(), at: BodyRef::Inclusion(i) });
    }
    out
}

/// An imported part's walls or region body as the user sees it: its name and `metal` (walls
/// round the region) or `interior` (the region).
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub name: String,
    pub material: &'static str,
}

/// The imported part's walls and region body, in their order.
pub fn parts(m: &Model) -> Vec<Part> {
    m.parts()
        .map(|(_, b)| Part {
            name: b.name.clone().unwrap_or_default(),
            material: if b.kind == emcraft_model::Kind::Interior { INTERIOR } else { METAL },
        })
        .collect()
}

/// An imported part's body `name` (its walls, or the region): placed by the import, its
/// surface kept to be drawn round the region.
pub(crate) fn part(name: &str, shape: Shape, interior: bool) -> emcraft_model::Body {
    use emcraft_model::{Fill, Kind};
    let (fill, kind) = if interior { (Fill::Region, Kind::Interior) } else { (Fill::Metal, Kind::Walls) };
    emcraft_model::Body { shape, at: Point3::ORIGIN, axis: Axis::Z, fill, kind, name: Some(name.to_owned()) }
}

/// What a body was placed as (`as` in replies).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Placed {
    /// A perfect conductor the region loses.
    Metal,
    /// The region's own material, given back.
    Region,
    /// Another material, meshed apart.
    Dielectric,
    /// The region itself: inside it is solved, outside it is metal.
    Interior,
}

/// The region an interior body makes: a box anywhere (turned with its axis), a prism or a
/// revolved profile moved to `at` and turned, a cylinder or a sphere standing on the origin.
fn interior_shape(shape: Shape, at: Point3, axis: Axis) -> Result<Shape> {
    let reach = Feature { combine: Combine::Add, shape: shape.clone(), at, axis }.bounds();
    match shape {
        Shape::Box { .. } => Ok(Shape::block(reach.min, reach.max)?),
        Shape::Surface { name, positions, triangles, tags, surfaces } => {
            let place = |p: Point3| at + axis.turn(p.to_vec());
            let surfaces = surfaces.into_iter().map(|f| f.filter(|_| axis == Axis::Z).map(|f| f.moved(at.to_vec()))).collect();
            Ok(Shape::Surface { name, positions: positions.into_iter().map(place).collect(), triangles, tags, surfaces })
        }
        other if at == Point3::ORIGIN && axis == Axis::Z => Ok(other),
        _ => Err(EngineError::failed("an interior cylinder or sphere stands on the origin along z: elsewhere, draw it as a revolved profile")),
    }
}

/// Places a body of `material` in the model: metal as a subtracted feature, the region's
/// material as an added one, another material as an inclusion.
pub(crate) fn place(m: &mut Model, shape: Shape, at: Point3, axis: Axis, material: &str) -> Result<Placed> {
    // An interior is the region: inside solved, outside metal; the bodies placed stay.
    if material == INTERIOR {
        let shape = interior_shape(shape, at, axis)?;
        shape.check()?;
        let filling = m
            .domain
            .solid
            .as_ref()
            .map(|s| s.material.clone())
            .or_else(|| m.domain.background.clone())
            .unwrap_or_else(|| emcraft_model::region::BACKGROUND.to_owned());
        m.domain.solid = Some(emcraft_model::Solid { shape, material: filling });
        m.domain.region = emcraft_model::Region::Pinned;
        m.domain.open = None;
        m.waveguide = None;
        m.mesh = None;
        return Ok(Placed::Interior);
    }
    // The first body with no region: the region is automatic round the bodies (ADR 0005); an
    // automatic region grows to hold the body before it is placed.
    if m.domain.solid.is_none() {
        m.domain.region = emcraft_model::Region::Auto { margin: None, thickness: None };
    }
    let reach = Feature { combine: Combine::Add, shape: shape.clone(), at, axis }.bounds();
    m.refresh_region_with(Some(reach))?;
    let region = m.domain.solid.as_ref().map(|s| s.material.clone()).ok_or_else(|| EngineError::failed("a body needs a region to lie in"))?;
    if material == METAL {
        m.add_feature(Feature { combine: Combine::Subtract, shape, at, axis })?;
        Ok(Placed::Metal)
    } else if material == region {
        m.add_feature(Feature { combine: Combine::Add, shape, at, axis })?;
        Ok(Placed::Region)
    } else {
        if axis != Axis::Z {
            return Err(EngineError::failed("not supported yet: a dielectric body turned off the z axis"));
        }
        m.add_inclusion(Inclusion { shape, at, material: Some(material.to_owned()) })?;
        Ok(Placed::Dielectric)
    }
}

pub fn body(s: &mut Session, p: &Params) -> Result<Value> {
    let shape = crate::geometry::shape(p)?;
    let material = p.string("material").required()?;
    let kind = place(&mut s.model, shape, p.point("at").or(Point3::ORIGIN)?, p.choice("axis").or_default()?, material)?;
    Ok(json!({"bodies": bodies(&s.model).len(), "as": kind}))
}

/// Gives body `body` (its name) another material: it is taken out and placed again with it,
/// last (so it wins where it overlaps others).
pub fn body_material(s: &mut Session, p: &Params) -> Result<Value> {
    let wanted = p.string("body").required()?;
    let material = p.string("material").required()?;
    if parts(&s.model).iter().any(|b| b.name == wanted) {
        return part_body_material(s, wanted, material);
    }
    let list = bodies(&s.model);
    let b = list
        .iter()
        .find(|b| b.name == wanted)
        .ok_or_else(|| p.error("body", crate::text::unknown("names no body", "bodies: ", list.iter().map(|b| b.name.as_str()))))?;
    let gone = || EngineError::failed(format!("body `{wanted}` is gone from the model"));
    let (shape, at, axis) = match b.at {
        BodyRef::Feature(i) => {
            let f = s.model.remove_feature(i).ok_or_else(gone)?;
            (f.shape, f.at, f.axis)
        }
        BodyRef::Inclusion(i) => {
            let inc = s.model.remove_inclusion(i).ok_or_else(gone)?;
            (inc.shape, inc.at, Axis::Z)
        }
    };
    let kind = place(&mut s.model, shape, at, axis, material)?;
    Ok(json!({"body": wanted, "as": kind}))
}

/// An imported part's wall or region body given another material: the part read again as
/// geometry.import read it, the body's material changed and the region chosen anew (the body
/// `interior` the region, else the air the metal encloses, else a box of air round them). The
/// ports stay; the bodies drawn since are placed again after it, in their order (`dropped`
/// counts those the new region cannot hold, from 1).
fn part_body_material(s: &mut Session, name: &str, material: &str) -> Result<Value> {
    let step = s
        .history
        .iter()
        .rev()
        .find(|st| st.command == "geometry.import")
        .ok_or_else(|| EngineError::failed(format!("`{name}` came from an import no longer in the history")))?;
    let mut params = step.params.clone();
    let Some(o) = params.as_object_mut() else { return Err(EngineError::failed("the import's parameters are not an object")) };
    // The region follows the materials now.
    o.remove("region");
    o.remove("keep");
    let materials = o.entry("materials").or_insert_with(|| json!({}));
    let Some(m) = materials.as_object_mut() else { return Err(EngineError::failed("the import's `materials` are not an object")) };
    m.insert(name.to_owned(), json!(material));
    // The bodies drawn since (not the part's own, which carry their names) are placed again
    // after it, in their order; one the new region cannot hold is left out and named.
    let drawn: Vec<emcraft_model::Body> = s.model.bodies().iter().filter(|b| !b.is_part() && b.name.is_none()).cloned().collect();
    let vars = s.variables().clone();
    let reader = Params::new(&params, s.display).with_variables(&vars);
    let reply = crate::geometry::import(s, &reader)?;
    let mut dropped = Vec::new();
    for (k, b) in drawn.into_iter().enumerate() {
        let placed = match (b.feature(), b.inclusion()) {
            (Some(f), _) => s.model.add_feature(f),
            (None, Some(inc)) => s.model.add_inclusion(inc),
            (None, None) => Ok(()),
        };
        if placed.is_err() {
            dropped.push(k + 1);
        }
    }
    Ok(json!({"body": name, "material": material, "region": reply["region"], "import": params, "dropped": dropped}))
}

/// The body parameter `body` names.
pub(crate) fn body_named(m: &Model, p: &Params) -> Result<Body> {
    let wanted = p.string("body").required()?;
    let list = bodies(m);
    let names: Vec<&str> = list.iter().map(|b| b.name.as_str()).collect();
    match list.iter().find(|b| b.name == wanted) {
        Some(b) => Ok(b.clone()),
        None if names.is_empty() => Err(p.error("body", format!("names no body: `{}` (the model has none)", crate::text::clip(wanted, 64)))),
        None => Err(p.error(
            "body",
            crate::text::unknown(format_args!("names no body: `{}`", crate::text::clip(wanted, 64)), "bodies: ", crate::text::listed(&names, 20)),
        )),
    }
}

/// A body's shape, placement and axis.
pub(crate) fn placement(m: &Model, at: BodyRef) -> Option<(Shape, Point3, Axis)> {
    match at {
        BodyRef::Feature(i) => m.features().get(i).map(|f| (f.shape.clone(), f.at, f.axis)),
        BodyRef::Inclusion(i) => m.inclusions().get(i).map(|inc| (inc.shape.clone(), inc.at, Axis::Z)),
    }
}

/// `old` with the dimensions an edit gives: a primitive of the same kind keeps those it is not
/// given; another kind takes its own, as geometry.body does. An imported body's shape is its
/// file's.
fn edited_shape(p: &Params, old: &Shape) -> Result<Shape> {
    use crate::geometry::Primitive;
    let kind: Option<Primitive> = p.choice("shape").optional()?;
    Ok(match (kind, old) {
        (None | Some(Primitive::Box), Shape::Box { size }) => {
            let given = p.edit_point("size", Point3::new(size.x, size.y, size.z))?;
            Shape::Box { size: BoxSize::new(given.to_vec())? }
        }
        (None | Some(Primitive::Cylinder), Shape::Cylinder { radius, length }) => {
            Shape::Cylinder { radius: p.edit_metres("radius", *radius)?, length: p.edit_metres("length", *length)? }
        }
        (None | Some(Primitive::Sphere), Shape::Sphere { radius }) => Shape::Sphere { radius: p.edit_metres("radius", *radius)? },
        (None, Shape::Surface { .. }) => {
            if let Some(k) = ["size", "radius", "length"].into_iter().find(|k| p.has(k)) {
                return Err(p.error(k, "is a primitive's: an imported body's shape is its file's (its `at`, `axis` and `material` change)"));
            }
            old.clone()
        }
        (Some(_), _) => crate::geometry::shape(p)?,
    })
}

/// `geometry.body.edit`: a body's shape, placement, axis or material. It keeps its place among
/// the bodies unless its new material makes it another kind of body (metal, the region's,
/// a dielectric): then it is placed again last, as geometry.body.material does.
pub fn body_edit(s: &mut Session, p: &Params) -> Result<Value> {
    let b = body_named(&s.model, p)?;
    let gone = || EngineError::failed(format!("body `{}` is gone from the model", b.name));
    let (old_shape, old_at, old_axis) = placement(&s.model, b.at).ok_or_else(gone)?;
    let shape = edited_shape(p, &old_shape)?;
    let at = p.edit_point("at", old_at)?;
    let axis: Axis = p.choice("axis").or(old_axis)?;
    let region = s.model.domain.solid.as_ref().map(|x| x.material.clone()).unwrap_or_default();
    let material = p.string("material").optional()?.unwrap_or(&b.material).to_owned();
    // What the body is with its material: a subtracted or an added feature, else an inclusion.
    let combine = |m: &str| {
        if m == METAL {
            Some(Combine::Subtract)
        } else if m == region {
            Some(Combine::Add)
        } else {
            None
        }
    };
    let placed = match b.at {
        BodyRef::Feature(i) => {
            let now = s.model.features().get(i).map(|f| f.combine).ok_or_else(gone)?;
            if material == b.material || combine(&material) == Some(now) {
                s.model.replace_feature(i, Feature { combine: now, shape, at, axis })?;
                None
            } else {
                s.model.remove_feature(i).ok_or_else(gone)?;
                Some(place(&mut s.model, shape, at, axis, &material)?)
            }
        }
        BodyRef::Inclusion(i) => {
            // A conducting inclusion stays one; a dielectric one may take another dielectric.
            let conductor = b.material == METAL && material == METAL;
            if conductor || combine(&material).is_none() {
                if axis != Axis::Z {
                    return Err(p.error("axis", "not supported yet: a dielectric body turned off the z axis"));
                }
                let filling = (!conductor).then(|| material.clone());
                s.model.replace_inclusion(i, Inclusion { shape, at, material: filling })?;
                None
            } else {
                s.model.remove_inclusion(i).ok_or_else(gone)?;
                Some(place(&mut s.model, shape, at, axis, &material)?)
            }
        }
    };
    Ok(json!({"body": b.name, "material": material, "placed_again": placed.is_some(), "as": placed}))
}

/// `geometry.body.delete`.
pub fn body_delete(s: &mut Session, p: &Params) -> Result<Value> {
    let b = body_named(&s.model, p)?;
    let gone = || EngineError::failed(format!("body `{}` is gone from the model", b.name));
    match b.at {
        BodyRef::Feature(i) => s.model.remove_feature(i).map(|_| ()).ok_or_else(gone)?,
        BodyRef::Inclusion(i) => s.model.remove_inclusion(i).map(|_| ()).ok_or_else(gone)?,
    }
    Ok(json!({"body": b.name, "bodies": bodies(&s.model).len()}))
}

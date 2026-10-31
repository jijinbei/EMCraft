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
    let region = m.solid.as_ref().map_or("vacuum", |s| s.material.as_str());
    let name = |shape: &Shape, n: usize| match shape {
        Shape::Surface { name, .. } if !name.is_empty() => name.clone(),
        _ => format!("body {n}"),
    };
    let mut out: Vec<Body> = Vec::with_capacity(m.features.len() + m.inclusions.len());
    for (i, f) in m.features.iter().enumerate() {
        let material = match f.combine {
            Combine::Subtract => METAL.to_owned(),
            Combine::Add => region.to_owned(),
            Combine::Intersect => "clip".to_owned(),
        };
        out.push(Body { name: name(&f.shape, out.len() + 1), material, shape: f.shape.kind(), at: BodyRef::Feature(i) });
    }
    for (i, inc) in m.inclusions.iter().enumerate() {
        let material = inc.material.clone().unwrap_or_else(|| METAL.to_owned());
        out.push(Body { name: name(&inc.shape, out.len() + 1), material, shape: inc.shape.kind(), at: BodyRef::Inclusion(i) });
    }
    out
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
}

/// Places a body of `material` in the model: metal as a subtracted feature, the region's
/// material as an added one, another material as an inclusion.
pub(crate) fn place(m: &mut Model, shape: Shape, at: Point3, axis: Axis, material: &str) -> Result<Placed> {
    let region = m
        .solid
        .as_ref()
        .map(|s| s.material.clone())
        .ok_or_else(|| EngineError::failed("a body needs a region to lie in: geometry.solid or geometry.import"))?;
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
        BodyRef::Feature(i) => m.features.get(i).map(|f| (f.shape.clone(), f.at, f.axis)),
        BodyRef::Inclusion(i) => m.inclusions.get(i).map(|inc| (inc.shape.clone(), inc.at, Axis::Z)),
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
    let region = s.model.solid.as_ref().map(|x| x.material.clone()).unwrap_or_default();
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
            let now = s.model.features.get(i).map(|f| f.combine).ok_or_else(gone)?;
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

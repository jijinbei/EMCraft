//! Bodies placed in the field's region by what they are made of, the way a model is drawn in
//! CAD: metal (a perfect conductor, which the region loses: its surface becomes wall), the
//! region's own material (giving back what metal took, an opening in a metal body), or another
//! material (a dielectric body, meshed apart and joined to the region). Later bodies win where
//! they overlap. Underneath they are the solid's features and inclusions.

use emcraft_model::{Axis, Combine, Feature, Inclusion, Model, Point3, Shape};
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
        .ok_or_else(|| EngineError::Failed("a body needs a region to lie in: geometry.solid or geometry.import".into()))?;
    if material == METAL {
        m.add_feature(Feature { combine: Combine::Subtract, shape, at, axis })?;
        Ok(Placed::Metal)
    } else if material == region {
        m.add_feature(Feature { combine: Combine::Add, shape, at, axis })?;
        Ok(Placed::Region)
    } else {
        if axis != Axis::Z {
            return Err(EngineError::Failed("not supported yet: a dielectric body turned off the z axis".into()));
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

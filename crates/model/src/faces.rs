//! The names of an imported design's faces (docs/adr/0010-geometry-from-solidcraft.md): each
//! body imported from a SolidCraft design keeps, for each of its triangles, the B-rep face it lies
//! on, and each face's persistent name, so that a port put on a face by its name finds that face
//! again after the design changes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::port::{Frame, FrameAxes};
use crate::{Aabb, Axis, Model, Point3, PortFace, Shape};

/// A body's named faces: each face's name (by its index in the design) and, for each triangle of
/// the body's surface in order, the index of the face it lies on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NamedFaces {
    pub names: Vec<String>,
    pub source: Vec<u32>,
}

/// The bodies' named faces, by body name.
pub type Faces = BTreeMap<String, NamedFaces>;

/// The most characters of a name an error quotes.
const QUOTED: usize = 64;

/// The most face names an error lists.
const LISTED: usize = 12;

/// A name as an error quotes it: at most [`QUOTED`] characters.
fn clip(name: &str) -> String {
    name.chars().take(QUOTED).collect()
}

/// Triangle `t`'s corners among `positions`, none when it names a vertex there is not.
fn corners(positions: &[Point3], t: &[u32; 3]) -> Option<[Point3; 3]> {
    Some([*positions.get(t[0] as usize)?, *positions.get(t[1] as usize)?, *positions.get(t[2] as usize)?])
}

/// The surfaces a face name may lie on: the region's and every body's, by name.
fn surfaces(m: &Model) -> impl Iterator<Item = (&str, &[Point3], &[[u32; 3]])> {
    let region = m.domain.solid.as_ref().map(|s| &s.shape);
    region.into_iter().chain(m.bodies().iter().map(|b| &b.shape)).filter_map(|shape| match shape {
        Shape::Surface { name, positions, triangles, .. } => Some((name.as_str(), positions.as_slice(), triangles.as_slice())),
        _ => None,
    })
}

/// The triangles (their corners, m) of the face named `name`, on whatever body has it.
pub fn named(m: &Model, name: &str) -> Vec<[Point3; 3]> {
    surfaces(m)
        .filter_map(|(body, positions, triangles)| {
            let faces = m.faces.get(body)?;
            let k = faces.names.iter().position(|n| n == name)?;
            Some(triangles.iter().zip(&faces.source).filter(move |(_, s)| **s as usize == k).filter_map(move |(t, _)| corners(positions, t)))
        })
        .flatten()
        .collect()
}

/// Where a port on the face named `name` goes: the rectangle it spans across the axis it is flat
/// across, or why it cannot take one (no such face; a face not flat across x, y or z).
///
/// ```
/// use emcraft_model::{Model, faces::port_face};
/// assert!(port_face(&Model::default(), "Face1").unwrap_err().contains("no face"));
/// ```
pub fn port_face(m: &Model, name: &str) -> Result<PortFace, String> {
    let tris = named(m, name);
    let Some(b) = Aabb::around(tris.iter().flatten().copied()) else {
        let known: Vec<&str> = m.faces.values().flat_map(|f| f.names.iter().map(String::as_str)).take(LISTED).collect();
        return Err(format!("no face named `{}` (faces: {})", clip(name), known.join(", ")));
    };
    let span = (0..3).map(|d| b.max[d] - b.min[d]).fold(0.0, f64::max).max(1e-12);
    let axis = [Axis::X, Axis::Y, Axis::Z].into_iter().find(|a| b.max[a.index()] - b.min[a.index()] <= 1e-9 * span);
    let Some(axis) = axis else { return Err(format!("face `{}` is not flat across x, y or z", clip(name))) };
    let FrameAxes { u, v, .. } = Frame::new(axis).axes();
    PortFace::from_values(axis, [b.min[axis.index()], b.min[u], b.min[v], b.max[u] - b.min[u], b.max[v] - b.min[v]]).map_err(|e| e.to_string())
}

/// The name of the face flat across `axis` that holds `at` (within `tol`, m), if a named one does.
pub fn name_at(m: &Model, axis: Axis, at: Point3, tol: f64) -> Option<String> {
    let k = axis.index();
    let FrameAxes { u, v, .. } = Frame::new(axis).axes();
    // On the triangle's plane across the axis, and inside it as seen along the axis.
    let holds = |[a, b, c]: [Point3; 3]| {
        let flat = [a, b, c].iter().all(|q| (q[k] - at[k]).abs() <= tol);
        let cross = |p: Point3, q: Point3| (q[u] - p[u]) * (at[v] - p[v]) - (q[v] - p[v]) * (at[u] - p[u]);
        let sides = [cross(a, b), cross(b, c), cross(c, a)];
        let e = tol * tol;
        flat && (sides.iter().all(|d| *d >= -e) || sides.iter().all(|d| *d <= e))
    };
    surfaces(m).find_map(|(body, positions, triangles)| {
        let faces = m.faces.get(body)?;
        let (_, s) = triangles.iter().zip(&faces.source).find(|(t, _)| corners(positions, t).is_some_and(holds))?;
        faces.names.get(*s as usize).cloned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube's region whose faces are named by side: the face `top` (y = 1) found by its
    /// name as a port face across y, and by a point on it; a side's face named; a name that is
    /// none refused.
    #[test]
    fn a_face_is_found_by_its_name_and_its_place() {
        let mut m = Model::default();
        let positions: Vec<Point3> = (0..8).map(|i| Point3::new((i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64)).collect();
        let quads = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
        let triangles: Vec<[u32; 3]> = quads.iter().flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect();
        let tags: Vec<u32> = (0..12).map(|i| i / 2).collect();
        let shape = Shape::Surface { name: "cube".into(), positions, triangles, tags: tags.clone(), surfaces: Vec::new() };
        m.set_solid(crate::Solid { shape, material: "vacuum".into() }).unwrap();
        let names = ["left", "right", "bottom", "top", "back", "front"].map(String::from).to_vec();
        m.faces.insert("cube".into(), NamedFaces { names, source: tags });
        let top = port_face(&m, "top").unwrap();
        assert_eq!((top.axis, top.at, top.a.get(), top.b.get()), (Axis::Y, 1.0, 1.0, 1.0));
        assert_eq!(name_at(&m, Axis::Y, Point3::new(0.3, 1.0, 0.6), 1e-9).as_deref(), Some("top"));
        assert_eq!(name_at(&m, Axis::X, Point3::new(1.0, 0.5, 0.5), 1e-9).as_deref(), Some("right"));
        assert_eq!(name_at(&m, Axis::Y, Point3::new(0.3, 0.5, 0.6), 1e-9), None);
        assert!(port_face(&m, "nothing").unwrap_err().contains("top"));
    }
}

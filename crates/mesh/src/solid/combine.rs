//! A feature combined with the solid's tessellation by the mesh Boolean ([`crate::csg`]): its
//! body put in general position against the solid (flush faces moved, `tessellation::
//! feature_body` and [`crate::flush`]; grid lines kept off the other bodies' planes,
//! `super::grid`; turned and shifted by a hair), then combined; a union sharing planes with the
//! region that no move of a face keeps ([`crate::clip`]) grown past them, united and cut back.

use emcraft_model::{Aabb, Axis, Combine, Feature, Point3, Shape, Vec3};
use emcraft_tetmesh::tet::geometry::AnalyticSurface;

use super::grid::{self, Planes};
use super::tessellation::{Tessellation, feature_body, grid_axes, tessellate, transformed};
use crate::csg::{Op, Surface};
use crate::flush::Triangles;
use crate::{MeshError, Result};

/// How far flush faces move (a tenth of the element size) and within what they count as flush
/// (`crate::flush`).
pub(super) fn gaps(size: f64) -> crate::flush::Gaps {
    crate::flush::Gaps { step: 0.1 * size, flush: 1e-5 * size, mm: 1e3 }
}

fn triangles(t: &Tessellation) -> Triangles<'_> {
    Triangles { positions: &t.positions, triangles: &t.triangles }
}

/// The solid's tessellation combined with a feature: the feature's faces get tags after the
/// solid's, with their exact surfaces. The feature's grid lines are kept off `others`, the
/// planes of the solid's and the other features' flat faces (`super::grid`). A union refused
/// because its faces share a plane with the region's is tried again bounded by that plane
/// ([`crate::clip`]); when that does not apply or fails too, the first refusal stands.
pub(super) fn combined(main: Tessellation, f: &Feature, k: usize, solid: &Shape, size: f64, others: &Planes) -> Result<Tessellation> {
    if f.combine != Combine::Add {
        return direct(main, f, k, solid, size, others);
    }
    match direct(main.clone(), f, k, solid, size, others) {
        Ok(t) => Ok(t),
        Err(refused) => match bounded_union(main, f, k, size, others) {
            Ok(Some(t)) => Ok(t),
            Ok(None) | Err(_) => Err(refused),
        },
    }
}

/// [`combined`], its flush faces moved off the solid's.
fn direct(main: Tessellation, f: &Feature, k: usize, solid: &Shape, size: f64, others: &Planes) -> Result<Tessellation> {
    let (shape, at) = feature_body(f, solid, size)?;
    // An imported body: in place, its faces flush with the region's (or an earlier body's) moved
    // off them (`crate::flush`).
    let imported = matches!(shape, Shape::Surface { .. });
    let (shape, at, axis) = match shape {
        Shape::Surface { name, positions, triangles: tris, tags, surfaces } => {
            let placed: Vec<Point3> = positions.iter().map(|p| at + f.axis.turn(p.to_vec())).collect();
            let positions = crate::flush::moved(&name, Triangles { positions: &placed, triangles: &tris }, triangles(&main), f.combine, gaps(size))?;
            (Shape::Surface { name, positions, triangles: tris, tags, surfaces }, Point3::ORIGIN, Axis::Z)
        }
        other => (other, at, f.axis),
    };
    let mut tool = tool(&shape, at, axis, k, size, others);
    // A primitive in an imported solid (a prism, an STL part): `feature_body` knows only a box's
    // or a cylinder's faces, so its faces flush with the solid's (a post from floor to roof) are
    // moved off them here, on the triangles, as an imported body's are.
    if !imported && matches!(solid, Shape::Surface { .. }) {
        tool.positions = crate::flush::moved(&format!("feature {}", k + 1), triangles(&tool), triangles(&main), f.combine, gaps(size))?;
    }
    // A union's face still in the plane of the solid's after the moves (a stub's sides once its
    // foot is pushed into the floor) would leave slivers a millionth of the size wide: refused
    // here, so `combined` tries it bounded by that plane.
    if f.combine == Combine::Add && crate::flush::touching(triangles(&tool), triangles(&main), gaps(size).flush) {
        return Err(MeshError::invalid(
            "feature",
            format!(
                "not supported yet: feature {} has a face in the plane of one of the region's faces that reaches past it, while something lies beyond that plane, so moving the face would change the shape (let the feature reach through the wall or keep it clear of the plane, or draw the outline as one prism)",
                k + 1
            ),
        ));
    }
    finish(main, tool, f.combine, imported)
}

/// The feature's body tessellated in place, in general position against the solid's
/// tessellation: a round body turned about its own axis by an odd angle (it is the same body;
/// an imported one is not round, and stays as it is), every feature moved by up to a millionth
/// of the element size (far below the mesh's resolution, far above rounding), so no vertex of
/// one lies exactly on a grid line or plane of the other, and its grid lines kept off `others`.
fn tool(shape: &Shape, at: Point3, axis: Axis, k: usize, size: f64, others: &Planes) -> Tessellation {
    let mut own = tessellate(shape, size);
    if matches!(shape, Shape::Cylinder { .. } | Shape::Sphere { .. }) {
        let (sn, cs) = 0.2345f64.sin_cos();
        for p in &mut own.positions {
            *p = Point3::new(cs * p.x - sn * p.y, sn * p.x + cs * p.y, p.z);
        }
    }
    // Each feature by its own amount (the fractional parts of multiples of irrational numbers),
    // so two features' grids that line up exactly do not stay lined up.
    let jitter = Vec3::new(0.618_033_988_75, 0.414_213_562_37, 0.732_050_807_57).map(|v| ((k + 1) as f64 * v).fract() * 1e-6 * size);
    let mut tool = transformed(own, axis, at + jitter);
    grid::keep_off(&mut tool.positions, grid_axes(shape, axis), others, grid::NEAR * size);
    tool
}

/// The Boolean of the two tessellations in general position, the tool's faces tagged after the
/// solid's with their exact surfaces.
fn finish(main: Tessellation, tool: Tessellation, combine: Combine, imported: bool) -> Result<Tessellation> {
    let base = main.tri_face.iter().map(|t| t + 1).max().unwrap_or(0).max(main.analytic.len() as u32);
    let op = match combine {
        Combine::Add => Op::Union,
        Combine::Subtract => Op::Difference,
        Combine::Intersect => Op::Intersection,
    };
    let a = Surface { positions: main.positions, triangles: main.triangles, tags: main.tri_face };
    let b = Surface { positions: tool.positions, triangles: tool.triangles, tags: tool.tri_face };
    // An imported body's flat faces cross the region's in straight lines: the points its
    // triangles' edges leave along them are collapsed away.
    let out = if imported { crate::csg::combine_simplified(&a, &b, op, base)? } else { crate::csg::combine(&a, &b, op, base)? };
    let mut analytic = main.analytic;
    analytic.resize(base as usize, None);
    // A difference's tool faces now face the other way.
    analytic.extend(tool.analytic.into_iter().map(|a| match (a, op) {
        (Some(AnalyticSurface::Plane { normal, point }), Op::Difference) => Some(AnalyticSurface::Plane { normal: normal.map(|v| -v), point }),
        (a, _) => a,
    }));
    Ok(Tessellation {
        positions: out.positions,
        triangles: out.triangles,
        tri_face: out.tags,
        analytic,
        start_face: main.start_face,
        end_face: main.end_face,
    })
}

/// A union whose faces lie in planes of the region's that both lie behind ([`crate::clip`]):
/// the region grown 0.35 of the size past each plane, the feature a tenth (its other flush faces
/// then moved as usual), united, and cut back at the planes. `None` when no such plane is found.
fn bounded_union(main: Tessellation, f: &Feature, k: usize, size: f64, others: &Planes) -> Result<Option<Tessellation>> {
    let tol = gaps(size).flush;
    let imported = matches!(f.shape, Shape::Surface { .. });
    let mut tool = tool(&f.shape, f.at, f.axis, k, size, others);
    let found = crate::clip::bounds(triangles(&main), &main.tri_face, triangles(&tool), tol);
    if found.is_empty() {
        return Ok(None);
    }
    let step = gaps(size).step;
    let grown = Tessellation { positions: crate::clip::grown(&main.positions, &found, 3.5 * step, tol), ..main };
    tool.positions = crate::clip::grown(&tool.positions, &found, step, tol);
    let name = match &f.shape {
        Shape::Surface { name, .. } => name.clone(),
        _ => format!("feature {}", k + 1),
    };
    tool.positions = crate::flush::moved(&name, triangles(&tool), triangles(&grown), Combine::Add, gaps(size))?;
    if crate::flush::touching(triangles(&tool), triangles(&grown), tol) {
        return Err(MeshError::invalid("feature", "a face still lies in one of the region's planes"));
    }
    let united = finish(grown, tool, Combine::Add, imported)?;
    let around = Aabb::around(united.positions.iter().copied()).ok_or_else(|| MeshError::invalid("feature", "the union is empty"))?;
    let margin = (around.max - around.min).norm() + size;
    let cut = crate::csg::combine(
        &Surface { positions: united.positions, triangles: united.triangles, tags: united.tri_face },
        &crate::clip::half_space(&found, around, margin),
        Op::Intersection,
        0,
    )?;
    Ok(Some(Tessellation { positions: cut.positions, triangles: cut.triangles, tri_face: cut.tags, ..united }))
}

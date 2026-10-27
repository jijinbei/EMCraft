//! The crossing into the tetrahedral mesher ([`emcraft_tetmesh`]): its own point and vector
//! types, and one region meshed from its tessellation.

use emcraft_model::{Point3, Vec3};
use emcraft_tetmesh::cdt::{CdtOptions, tetrahedralize};
use emcraft_tetmesh::size::LocalSize;
use emcraft_tetmesh::surface::{BoundarySurface, SurfaceOptions};
use emcraft_tetmesh::tet::{QualityOptions, quality_mesh};

use super::tessellation::Tessellation;
use crate::{MAX_TETS, MeshError, Result};

/// The mesher's own point and vector types: `emcraft_tetmesh` depends on no workspace crate, so
/// points cross into it and back here, component for component (no arithmetic).
pub(super) type MPoint = emcraft_tetmesh::Point3;
pub(super) type MVec = emcraft_tetmesh::Vec3;

pub(super) fn mesher_point(p: Point3) -> MPoint {
    MPoint::new(p.x, p.y, p.z)
}

pub(super) fn model_point(p: MPoint) -> Point3 {
    Point3::new(p.x, p.y, p.z)
}

pub(super) fn mesher_vec(v: Vec3) -> MVec {
    MVec::new(v.x, v.y, v.z)
}

pub(super) fn model_vec(v: MVec) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// One region's mesh: points, tetrahedra and boundary faces.
pub(super) struct RegionMesh {
    pub(super) points: Vec<Point3>,
    pub(super) tets: Vec<[u32; 4]>,
    pub(super) boundary: Vec<RegionFace>,
}

/// A boundary face of a region's mesh: its corners, its tetrahedron and its face tag.
#[derive(Clone, Copy, Debug)]
pub(super) struct RegionFace {
    pub(super) nodes: [u32; 3],
    pub(super) tet: u32,
    pub(super) tag: u32,
}

/// Meshes the region a tessellation bounds, keeping the triangles of the `frozen` faces as they
/// are (interfaces another region shares).
pub(super) fn region(t: &Tessellation, size: f64, frozen: Vec<u32>, locals: &[LocalSize]) -> Result<RegionMesh> {
    let fail = |e: emcraft_tetmesh::Error| MeshError::Invalid { what: "tetrahedral meshing".into(), why: e.to_string() };
    let positions: Vec<MPoint> = t.positions.iter().map(|&p| mesher_point(p)).collect();
    let surface = BoundarySurface::new(&positions, &t.triangles, &t.tri_face, &SurfaceOptions::default()).map_err(fail)?;
    let cdt = tetrahedralize(&surface, &CdtOptions::default()).map_err(fail)?;
    let options = QualityOptions { size: Some(size), max_tets: MAX_TETS, frozen_faces: frozen, locals: locals.to_vec(), ..QualityOptions::default() };
    let mut analytic = t.analytic.clone();
    analytic.resize(t.tri_face.iter().map(|f| *f as usize + 1).max().unwrap_or(0).max(analytic.len()), None);
    let q = quality_mesh(&surface, &cdt, analytic, &options).map_err(fail)?;
    let tets: Vec<[u32; 4]> = q.elements.iter().filter_map(|e| <[u32; 4]>::try_from(e.as_slice()).ok()).collect();
    if tets.len() != q.elements.len() {
        return Err(MeshError::Invalid { what: "tetrahedral meshing".into(), why: "elements other than linear tetrahedra".into() });
    }
    let boundary = q
        .boundary
        .iter()
        .filter_map(|b| {
            let tet = tets.get(b.tet as usize)?;
            let k = b.opposite as usize;
            let n: Vec<u32> = (0..4).filter(|&i| i != k).map(|i| tet[i]).collect();
            Some(RegionFace { nodes: [n[0], n[1], n[2]], tet: b.tet, tag: b.face })
        })
        .collect();
    Ok(RegionMesh { points: q.points.into_iter().map(model_point).collect(), tets, boundary })
}

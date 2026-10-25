//! The tetrahedral mesh: nodes (m), positively oriented tetrahedra with their material, and the
//! boundary triangles with what closes them (a perfect electric conductor or a port).

use crate::geom::{Point3, Vec3};

/// What a boundary triangle lies on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    /// A perfect electric conductor: tangential E = 0.
    Pec,
    /// The port with this index in [`crate::Model::ports`]'s order of ends (0 = start, 1 = end).
    Port(u32),
}

/// A boundary triangle: its nodes, the tetrahedron it bounds and its condition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundaryFace {
    pub nodes: [u32; 3],
    pub tet: u32,
    pub tag: Boundary,
}

/// The structured grid a mesh was made on: coordinate lines along x, y and z, each cell split
/// into [`crate::mesh::TETS_PER_CELL`] tetrahedra, consecutive from the cell's first. Lets points
/// be located without a search tree.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Grid {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
    /// The first tetrahedron of each cell (cells numbered x fastest), `u32::MAX` for a cell left
    /// out (outside a section's aperture).
    pub cells: Vec<u32>,
}

/// Tetrahedra per grid cell.
pub const TETS_PER_CELL: usize = 6;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Mesh {
    pub nodes: Vec<Point3>,
    /// Corner nodes, ordered so the signed volume is positive.
    pub tets: Vec<[u32; 4]>,
    /// The material of each tetrahedron (an index into [`crate::Model::materials`]).
    pub tet_material: Vec<u32>,
    pub boundary: Vec<BoundaryFace>,
    pub grid: Option<Grid>,
    /// For an unstructured mesh: tetrahedra binned on a uniform grid of buckets, for locating
    /// points.
    pub buckets: Option<Buckets>,
    /// Edges that follow a curved surface: their nodes (ascending) and the point on the surface
    /// their midpoint moves to, sorted by edge. The tetrahedra holding them are curved
    /// (quadratic geometry).
    pub curved: Vec<([u32; 2], Point3)>,
}

/// Tetrahedra binned by their bounding boxes on a uniform grid over the mesh.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Buckets {
    pub lo: Point3,
    pub step: Vec3,
    pub dims: [usize; 3],
    /// Bucket b holds `items[start[b]..start[b + 1]]` (buckets numbered x fastest).
    pub start: Vec<u32>,
    pub items: Vec<u32>,
}

impl Mesh {
    /// The curved midpoint of edge (a, b), if it is curved.
    pub fn midpoint(&self, a: u32, b: u32) -> Option<Point3> {
        let key = [a.min(b), a.max(b)];
        self.curved.binary_search_by(|(e, _)| e.cmp(&key)).ok().map(|i| self.curved[i].1)
    }

    /// The corner coordinates of a tetrahedron, if it exists.
    pub fn corners(&self, t: usize) -> Option<[Point3; 4]> {
        let tet = self.tets.get(t)?;
        let mut out = [Point3::ORIGIN; 4];
        for (o, &n) in out.iter_mut().zip(tet) {
            *o = *self.nodes.get(n as usize)?;
        }
        Some(out)
    }

    /// The node of index `n`, if it exists.
    pub fn node(&self, n: u32) -> Option<Point3> {
        self.nodes.get(n as usize).copied()
    }

    /// The signed volume of tetrahedron `t` (positive for the mesh's orientation).
    pub fn volume(&self, t: usize) -> Option<f64> {
        let [a, b, c, d] = self.corners(t)?;
        Some(crate::geom::triple(b - a, c - a, d - a) / 6.0)
    }

    /// The centroid of tetrahedron `t`.
    pub fn centroid(&self, t: usize) -> Option<Point3> {
        Point3::centroid(self.corners(t)?)
    }
}

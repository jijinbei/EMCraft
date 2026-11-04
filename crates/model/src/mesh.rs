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
    /// A symmetry plane closed by an electric or magnetic wall ([`crate::Symmetry`]).
    Symmetry(crate::SymmetryKind),
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

/// Most tetrahedra of a mesh read from a file (a board's layered mesh may pass the mesher's own
/// cap for solids).
pub const MAX_READ_TETS: usize = 1 << 25;

impl Mesh {
    /// Checks a mesh read from a file (hostile) against a model of `materials` materials: sizes
    /// within [`MAX_READ_TETS`] and agreeing, every index in range (nodes, tetrahedra,
    /// materials, a port's end 0 or 1), every number finite, the curved edges sorted, the grid
    /// ascending with each cell's tetrahedra inside the mesh. The tetrahedra's shapes are the
    /// mesher's to check (`emcraft_mesh::check_tetrahedra`).
    ///
    /// ```
    /// use emcraft_model::{Mesh, Point3};
    /// let mut m = Mesh { nodes: vec![Point3::new(0.0, 0.0, 0.0); 4], tets: vec![[0, 1, 2, 3]], tet_material: vec![0], ..Mesh::default() };
    /// assert!(m.check(1).is_ok());
    /// m.tets[0][3] = 4;
    /// assert!(m.check(1).is_err());
    /// ```
    pub fn check(&self, materials: usize) -> Result<(), String> {
        let (n, t) = (self.nodes.len(), self.tets.len());
        if t > MAX_READ_TETS || n > 4 * MAX_READ_TETS || self.boundary.len() > 4 * MAX_READ_TETS || self.curved.len() > 6 * MAX_READ_TETS {
            return Err(format!("a mesh has at most {MAX_READ_TETS} tetrahedra"));
        }
        let node = |i: u32| (i as usize) < n;
        if !self.nodes.iter().all(|p| p.is_finite()) {
            return Err("a mesh's nodes must be finite points".into());
        }
        if !self.tets.iter().flatten().all(|&i| node(i)) {
            return Err("a tetrahedron names a node the mesh does not have".into());
        }
        if self.tet_material.len() != t || self.tet_material.iter().any(|&m| m as usize >= materials) {
            return Err(format!("each tetrahedron has one of the model's {materials} materials"));
        }
        for f in &self.boundary {
            if !f.nodes.iter().all(|&i| node(i)) || f.tet as usize >= t || matches!(f.tag, Boundary::Port(k) if k > 1) {
                return Err("a boundary face names a node or tetrahedron the mesh does not have, or a port end other than 0 or 1".into());
            }
        }
        if !self.curved.iter().all(|([a, b], p)| a < b && node(*b) && p.is_finite()) || self.curved.windows(2).any(|w| w[0].0 >= w[1].0) {
            return Err("a mesh's curved edges are pairs of its nodes, ascending, sorted, each with a finite point".into());
        }
        if let Some(g) = &self.grid {
            let lines = [&g.x, &g.y, &g.z];
            if lines.iter().any(|l| l.len() < 2 || !l.iter().all(|x| x.is_finite()) || l.windows(2).any(|w| w[1] <= w[0])) {
                return Err("a mesh's grid lines must be finite and ascending, at least 2 along each axis".into());
            }
            let cells = lines.iter().map(|l| l.len() - 1).try_fold(1usize, usize::checked_mul);
            if cells != Some(g.cells.len()) || g.cells.iter().any(|&c| c != u32::MAX && (c as usize).saturating_add(TETS_PER_CELL) > t) {
                return Err("a mesh's grid has one entry a cell, each the first of its tetrahedra or none".into());
            }
        }
        Ok(())
    }

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

//! The tetrahedral mesh: nodes (m), positively oriented tetrahedra with their material, and the
//! boundary triangles with what closes them (a perfect electric conductor or a port).

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
/// into [`crate::mesh::TETS_PER_CELL`] tetrahedra numbered `TETS_PER_CELL · cell + t`, cells
/// numbered x fastest. Lets points be located without a search tree.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Grid {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
}

/// Tetrahedra per grid cell.
pub const TETS_PER_CELL: usize = 6;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Mesh {
    pub nodes: Vec<[f64; 3]>,
    /// Corner nodes, ordered so the signed volume is positive.
    pub tets: Vec<[u32; 4]>,
    /// The material of each tetrahedron (an index into [`crate::Model::materials`]).
    pub tet_material: Vec<u32>,
    pub boundary: Vec<BoundaryFace>,
    pub grid: Option<Grid>,
}

impl Mesh {
    /// The corner coordinates of a tetrahedron, if it exists.
    pub fn corners(&self, t: usize) -> Option<[[f64; 3]; 4]> {
        let tet = self.tets.get(t)?;
        let mut out = [[0.0; 3]; 4];
        for (o, &n) in out.iter_mut().zip(tet) {
            *o = *self.nodes.get(n as usize)?;
        }
        Some(out)
    }
}

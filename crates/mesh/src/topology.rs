//! Global edges and faces of a tetrahedral mesh, numbered in the order the tetrahedra first meet
//! them (deterministic). Every edge and face is stored with its nodes ascending: that order
//! orients the vector basis functions on it, the same from every tetrahedron sharing it.

use std::collections::HashMap;

use emcraft_model::Mesh;

/// The local edges of a tetrahedron whose corners are sorted by global node: pairs of local
/// corners, the lower first.
pub const LOCAL_EDGES: [[usize; 2]; 6] = [[0, 1], [0, 2], [0, 3], [1, 2], [1, 3], [2, 3]];
/// The local faces of such a tetrahedron: corner triples, ascending (face k leaves out corner
/// 3 − k).
pub const LOCAL_FACES: [[usize; 3]; 4] = [[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]];

#[derive(Clone, Debug, Default)]
pub struct Topology {
    /// Each tetrahedron's corners sorted by global node.
    pub sorted: Vec<[u32; 4]>,
    /// Edge nodes, ascending.
    pub edges: Vec<[u32; 2]>,
    /// Face nodes, ascending.
    pub faces: Vec<[u32; 3]>,
    /// Each tetrahedron's edges in [`LOCAL_EDGES`] order.
    pub tet_edges: Vec<[u32; 6]>,
    /// Each tetrahedron's faces in [`LOCAL_FACES`] order.
    pub tet_faces: Vec<[u32; 4]>,
    /// The (at most two) tetrahedra of each face; [`NO_TET`] for none.
    pub face_tets: Vec<[u32; 2]>,
}

/// The marker in [`Topology::face_tets`] where a face has no (second) tetrahedron.
pub const NO_TET: u32 = u32::MAX;

impl Topology {
    pub fn new(mesh: &Mesh) -> Topology {
        let mut t = Topology::default();
        let mut edge_id: HashMap<[u32; 2], u32> = HashMap::with_capacity(mesh.tets.len() * 2);
        let mut face_id: HashMap<[u32; 3], u32> = HashMap::with_capacity(mesh.tets.len() * 3);
        for (ti, tet) in mesh.tets.iter().enumerate() {
            let mut s = *tet;
            s.sort_unstable();
            t.sorted.push(s);
            let mut te = [0u32; 6];
            for (e, [a, b]) in te.iter_mut().zip(LOCAL_EDGES) {
                let key = [s[a], s[b]];
                let next = t.edges.len() as u32;
                *e = *edge_id.entry(key).or_insert_with(|| {
                    t.edges.push(key);
                    next
                });
            }
            t.tet_edges.push(te);
            let mut tf = [0u32; 4];
            for (f, [a, b, c]) in tf.iter_mut().zip(LOCAL_FACES) {
                let key = [s[a], s[b], s[c]];
                let next = t.faces.len() as u32;
                let id = *face_id.entry(key).or_insert_with(|| {
                    t.faces.push(key);
                    t.face_tets.push([NO_TET; 2]);
                    next
                });
                if let Some(owners) = t.face_tets.get_mut(id as usize) {
                    if owners[0] == NO_TET {
                        owners[0] = ti as u32;
                    } else {
                        owners[1] = ti as u32;
                    }
                }
                *f = id;
            }
            t.tet_faces.push(tf);
        }
        t
    }

    /// The local index (in [`LOCAL_FACES`]) of global face `face` in tetrahedron `tet`.
    pub fn local_face(&self, tet: usize, face: u32) -> Option<usize> {
        self.tet_faces.get(tet)?.iter().position(|&f| f == face)
    }

    /// The global face with these nodes (in any order).
    pub fn find_face(&self, tet: usize, nodes: [u32; 3]) -> Option<(usize, u32)> {
        let mut key = nodes;
        key.sort_unstable();
        let tf = self.tet_faces.get(tet)?;
        tf.iter().enumerate().find(|(_, f)| self.faces.get(**f as usize) == Some(&key)).map(|(l, f)| (l, *f))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_model::{Model, Section, Waveguide};

    #[test]
    fn euler_characteristic_and_sharing() {
        let mut m = Model::new();
        m.set_waveguide(Waveguide {
            width: 0.02,
            height: 0.01,
            sections: vec![Section::new(0.03, "vacuum")],
            layers: Vec::new(),
            strips: Vec::new(),
            vias: Vec::new(),
        })
        .unwrap();
        let mesh = crate::waveguide(&m, 0.005).unwrap();
        let t = Topology::new(&mesh);
        // A ball: V − E + F − T = 1.
        let chi = mesh.nodes.len() as i64 - t.edges.len() as i64 + t.faces.len() as i64 - mesh.tets.len() as i64;
        assert_eq!(chi, 1);
        // Interior faces have two tets, boundary faces one; the boundary faces are the mesh's.
        let boundary = t.face_tets.iter().filter(|o| o[1] == u32::MAX).count();
        assert_eq!(boundary, mesh.boundary.len());
        assert!(t.face_tets.iter().all(|o| o[0] != u32::MAX));
        // Ascending nodes on every edge and face; no duplicates.
        assert!(t.edges.iter().all(|e| e[0] < e[1]));
        assert!(t.faces.iter().all(|f| f[0] < f[1] && f[1] < f[2]));
        let mut e = t.edges.clone();
        e.sort();
        e.dedup();
        assert_eq!(e.len(), t.edges.len());
        // Grid edges along x, y, z plus one diagonal per cell face and the cell diagonal.
        let g = mesh.grid.as_ref().unwrap();
        let (nx, ny, nz) = (g.x.len() - 1, g.y.len() - 1, g.z.len() - 1);
        let axis = nx * (ny + 1) * (nz + 1) + (nx + 1) * ny * (nz + 1) + (nx + 1) * (ny + 1) * nz;
        let diag = nx * ny * (nz + 1) + nx * (ny + 1) * nz + (nx + 1) * ny * nz + nx * ny * nz;
        assert_eq!(t.edges.len(), axis + diag);
        let (l, f) = t.find_face(0, [t.sorted[0][3], t.sorted[0][1], t.sorted[0][2]]).unwrap();
        assert_eq!(l, 3);
        assert_eq!(t.local_face(0, f), Some(3));
    }
}

//! The degrees of freedom: per edge and per face of the mesh, those on conducting walls
//! eliminated, the rest numbered as the unknowns.

use emcraft_fem::Order;
use emcraft_fem::nedelec::{LOCAL_EDGES, LOCAL_FACES};
use emcraft_mesh::Topology;
use emcraft_model::{Mesh, Point3};

use super::{MAX_UNKNOWNS, NONE, Result, SolveError, invalid};

/// Degrees of freedom: per edge and per face, those on conducting walls eliminated.
#[derive(Clone, Debug)]
pub struct Dofs {
    pub order: Order,
    pub edges: usize,
    pub faces: usize,
    /// Global DOF → unknown, [`NONE`] when eliminated.
    unknown: Vec<u32>,
    pub unknowns: usize,
    /// A point per unknown (edge midpoints, face centroids), for the ordering.
    pub points: Vec<Point3>,
}

impl Dofs {
    /// `fixed_faces`: the boundary faces (indices into the mesh's) whose tangential E is zero
    /// (perfectly conducting walls).
    pub(crate) fn new(order: Order, mesh: &Mesh, topo: &Topology, fixed_faces: &[usize]) -> Result<Dofs> {
        let (pe, pf) = (order.per_edge(), order.per_face());
        let (ne, nf) = (topo.edges.len(), topo.faces.len());
        let total = ne * pe + nf * pf;
        let mut fixed = vec![false; total];
        for b in fixed_faces.iter().filter_map(|&i| mesh.boundary.get(i)) {
            let tet = b.tet as usize;
            let Some((local, face)) = topo.find_face(tet, b.nodes) else {
                return Err(invalid("mesh", "a boundary face is not a face of its tetrahedron"));
            };
            for j in 0..pf {
                fixed[ne * pe + face as usize * pf + j] = true;
            }
            let corners = LOCAL_FACES[local];
            for (e, [a, c]) in LOCAL_EDGES.iter().enumerate() {
                if corners.contains(a) && corners.contains(c) {
                    let edge = topo.tet_edges[tet][e] as usize;
                    for j in 0..pe {
                        fixed[edge * pe + j] = true;
                    }
                }
            }
        }
        let free = fixed.iter().filter(|f| !**f).count();
        if free > MAX_UNKNOWNS {
            return Err(SolveError::TooLarge { unknowns: free, limit: MAX_UNKNOWNS });
        }
        let mut unknown = vec![NONE; total];
        let mut points = Vec::with_capacity(free);
        let node = |n: u32| mesh.nodes[n as usize];
        for (g, f) in fixed.iter().enumerate() {
            if *f {
                continue;
            }
            unknown[g] = points.len() as u32;
            let centre =
                if g < ne * pe { Point3::centroid(topo.edges[g / pe].map(node)) } else { Point3::centroid(topo.faces[(g - ne * pe) / pf].map(node)) };
            points.push(centre.unwrap_or(Point3::ORIGIN));
        }
        Ok(Dofs { order, edges: ne, faces: nf, unknown, unknowns: free, points })
    }

    /// The unknown of global degree of freedom `slot`, `None` when it is eliminated.
    pub(crate) fn unknown_of(&self, slot: usize) -> Option<usize> {
        self.unknown.get(slot).copied().filter(|u| *u != NONE).map(|u| u as usize)
    }

    /// The global degrees of freedom (slots, eliminated or not) of a tetrahedron's local basis
    /// functions, in the order of [`Dofs::of_tet`].
    pub(crate) fn slots_of_tet(&self, topo: &Topology, tet: usize) -> impl Iterator<Item = usize> + use<> {
        let (pe, pf, edges) = (self.order.per_edge(), self.order.per_face(), self.edges);
        let (te, tf) = (topo.tet_edges[tet], topo.tet_faces[tet]);
        (0..pe)
            .flat_map(move |j| te.into_iter().map(move |e| e as usize * pe + j))
            .chain(tf.into_iter().flat_map(move |f| (0..pf).map(move |j| edges * pe + f as usize * pf + j)))
    }

    /// The unknowns of a tetrahedron's local basis functions ([`NONE`] for eliminated ones).
    pub(crate) fn of_tet<'a>(&'a self, topo: &Topology, tet: usize) -> impl Iterator<Item = u32> + use<'a> {
        self.slots_of_tet(topo, tet).map(|s| self.unknown.get(s).copied().unwrap_or(NONE))
    }
}

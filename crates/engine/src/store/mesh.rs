//! The mesh in a project file: its counts in `project.json`, its numbers array parts under
//! `mesh/` — nodes (m), tetrahedra and their materials, boundary faces (3 nodes, the
//! tetrahedron, the condition as a kind and a value), curved edges and the structured grid. The
//! point-location buckets are not written: they are built again from the nodes and tetrahedra.

use emcraft_io::package::{Reader, Writer};
use emcraft_model::{Boundary, BoundaryFace, Grid, Mesh, Point3, SymmetryKind};
use serde::{Deserialize, Serialize};

/// A mesh's counts, as `project.json` gives them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredMesh {
    pub nodes: usize,
    pub tetrahedra: usize,
    pub boundary: usize,
    pub curved: usize,
    /// The grid's lines along x, y and z, for a structured mesh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<[usize; 3]>,
    /// Whether points are located through buckets (built again on reading).
    pub buckets: bool,
}

fn points(p: &[Point3]) -> impl Iterator<Item = f64> + '_ {
    p.iter().flat_map(|q| [q.x, q.y, q.z])
}

/// A boundary condition as two numbers: its kind and its value.
fn tag(b: Boundary) -> [u32; 2] {
    match b {
        Boundary::Pec => [0, 0],
        Boundary::Port(k) => [1, k],
        Boundary::Symmetry(s) => [2, SymmetryKind::ALL.iter().position(|x| *x == s).unwrap_or(0) as u32],
    }
}

fn untag([kind, value]: [u32; 2]) -> Result<Boundary, String> {
    match kind {
        0 => Ok(Boundary::Pec),
        1 => Ok(Boundary::Port(value)),
        2 => SymmetryKind::ALL.get(value as usize).copied().map(Boundary::Symmetry).ok_or_else(|| "a boundary face names no symmetry wall".into()),
        _ => Err("a boundary face's condition is not one EMCraft knows".into()),
    }
}

impl StoredMesh {
    /// `mesh` written as parts under `mesh/`.
    pub(crate) fn write(mesh: &Mesh, w: &mut Writer) -> Result<StoredMesh, String> {
        let e = |e: emcraft_io::Error| e.to_string();
        w.values("mesh/nodes.f64", points(&mesh.nodes)).map_err(e)?;
        w.values("mesh/tets.u32", mesh.tets.iter().flatten().copied()).map_err(e)?;
        w.values("mesh/tet_material.u32", mesh.tet_material.iter().copied()).map_err(e)?;
        let faces = mesh.boundary.iter().flat_map(|f| {
            let [kind, value] = tag(f.tag);
            [f.nodes[0], f.nodes[1], f.nodes[2], f.tet, kind, value]
        });
        w.values("mesh/boundary.u32", faces).map_err(e)?;
        w.values("mesh/curved.u32", mesh.curved.iter().flat_map(|(edge, _)| *edge)).map_err(e)?;
        w.values("mesh/curved.f64", mesh.curved.iter().flat_map(|(_, p)| [p.x, p.y, p.z])).map_err(e)?;
        if let Some(g) = &mesh.grid {
            w.values("mesh/grid_x.f64", g.x.iter().copied()).map_err(e)?;
            w.values("mesh/grid_y.f64", g.y.iter().copied()).map_err(e)?;
            w.values("mesh/grid_z.f64", g.z.iter().copied()).map_err(e)?;
            w.values("mesh/grid_cells.u32", g.cells.iter().copied()).map_err(e)?;
        }
        Ok(StoredMesh {
            nodes: mesh.nodes.len(),
            tetrahedra: mesh.tets.len(),
            boundary: mesh.boundary.len(),
            curved: mesh.curved.len(),
            grid: mesh.grid.as_ref().map(|g| [g.x.len(), g.y.len(), g.z.len()]),
            buckets: mesh.buckets.is_some(),
        })
    }

    /// The mesh, read for a model of `materials` materials and checked: its sizes and indices
    /// ([`Mesh::check`]) and its tetrahedra's shapes.
    pub(crate) fn read(self, r: &mut Reader, materials: usize) -> Result<Mesh, String> {
        let cap = emcraft_model::mesh::MAX_READ_TETS;
        if self.tetrahedra > cap || self.nodes > 4 * cap || self.boundary > 4 * cap || self.curved > 6 * cap {
            return Err(format!("a mesh has at most {cap} tetrahedra"));
        }
        let exactly =
            |v: Vec<u32>, n: usize, what: &str| if v.len() == n { Ok(v) } else { Err(format!("the mesh's {what}: {} numbers, not {n}", v.len())) };
        let exactly_f =
            |v: Vec<f64>, n: usize, what: &str| if v.len() == n { Ok(v) } else { Err(format!("the mesh's {what}: {} numbers, not {n}", v.len())) };
        let e = |e: emcraft_io::Error| e.to_string();
        let as_points = |v: Vec<f64>| v.as_chunks::<3>().0.iter().map(|&[x, y, z]| Point3::new(x, y, z)).collect::<Vec<_>>();
        let nodes = as_points(exactly_f(r.values("mesh/nodes.f64", 3 * self.nodes).map_err(e)?, 3 * self.nodes, "nodes")?);
        let tets = exactly(r.values("mesh/tets.u32", 4 * self.tetrahedra).map_err(e)?, 4 * self.tetrahedra, "tetrahedra")?;
        let tets = tets.as_chunks::<4>().0.to_vec();
        let tet_material = exactly(r.values("mesh/tet_material.u32", self.tetrahedra).map_err(e)?, self.tetrahedra, "materials")?;
        let faces = exactly(r.values("mesh/boundary.u32", 6 * self.boundary).map_err(e)?, 6 * self.boundary, "boundary")?;
        let boundary = faces
            .as_chunks::<6>()
            .0
            .iter()
            .map(|&[a, b, c, tet, kind, value]| Ok(BoundaryFace { nodes: [a, b, c], tet, tag: untag([kind, value])? }))
            .collect::<Result<Vec<_>, String>>()?;
        let edges = exactly(r.values("mesh/curved.u32", 2 * self.curved).map_err(e)?, 2 * self.curved, "curved edges")?;
        let mids = as_points(exactly_f(r.values("mesh/curved.f64", 3 * self.curved).map_err(e)?, 3 * self.curved, "curved edges")?);
        let curved = edges.as_chunks::<2>().0.iter().copied().zip(mids).collect();
        let grid = match self.grid {
            Some([nx, ny, nz]) => {
                if [nx, ny, nz].iter().any(|k| *k > 4 * cap) {
                    return Err("a mesh's grid has too many lines".into());
                }
                let line = |r: &mut Reader, name: &str, n: usize| exactly_f(r.values(name, n).map_err(e)?, n, "grid");
                let (x, y, z) = (line(r, "mesh/grid_x.f64", nx)?, line(r, "mesh/grid_y.f64", ny)?, line(r, "mesh/grid_z.f64", nz)?);
                let cells = r.values("mesh/grid_cells.u32", 4 * cap).map_err(e)?;
                Some(Grid { x, y, z, cells })
            }
            None => None,
        };
        let mut mesh = Mesh { nodes, tets, tet_material, boundary, grid, buckets: None, curved };
        emcraft_mesh::check_mesh(&mesh, materials)?;
        if self.buckets {
            mesh.buckets = Some(emcraft_mesh::solid::buckets(&mesh));
        }
        Ok(mesh)
    }
}

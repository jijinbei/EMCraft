//! One extrusion mesher: a cross-section — a structured grid of rectangles or a triangulation —
//! extruded through levels along z, each cell at each level filled with a material or left out
//! ([`extrude`]). The waveguide's structured mesh ([`crate::waveguide`]) and a board's
//! ([`crate::planar::extrude`]) are both this, with their own lines, fills and boundaries.
//!
//! The nodes are each level's points in the cross-section's order, level by level from the
//! lowest; the cells are filled level by level, each level's in the cross-section's order, so
//! the same cross-section and levels give the same mesh, bit for bit.
//!
//! A rectangle between two levels (a box) is split into six tetrahedra around its main diagonal
//! (the Kuhn, or Freudenthal, triangulation of the cube: H. W. Kuhn, *Some combinatorial lemmas
//! in topology*, IBM J. Res. Dev. 4(5):518–524, 1960): every face is cut along the diagonal from
//! its lowest to its highest corner, the same in both boxes sharing it, so the mesh conforms. A
//! triangle between two levels (a prism) is split into three by the rule of J. Dompierre,
//! P. Labbé, M.-G. Vallet, R. Camarero, *How to subdivide pyramids, prisms and hexahedra into
//! tetrahedra*, Proc. 8th International Meshing Roundtable, 1999: each quadrilateral side is cut
//! by its diagonal from its node of least global number; with the triangle's nodes a < b < c
//! and each level above the one below, the prism (a, b, c, a′, b′, c′) is (a, b, c, c′),
//! (a, b, b′, c′), (a, a′, b′, c′).

use emcraft_model::geom::triple;
use emcraft_model::mesh::TETS_PER_CELL;
use emcraft_model::{Mesh, Point3};

use crate::{CELL_LEFT_OUT, MeshError, Result};

/// A cross-section in the plane (x, y).
#[derive(Clone, Copy, Debug)]
pub enum Profile<'a> {
    /// The rectangles between grid lines: cell `i + nx·j` is [x_i, x_{i+1}] × [y_j, y_{j+1}],
    /// point `i + (nx + 1)·j` is (x_i, y_j).
    Grid { x: &'a [f64], y: &'a [f64] },
    /// Triangles over points.
    Triangles { points: &'a [[f64; 2]], triangles: &'a [[u32; 3]] },
}

impl Profile<'_> {
    /// How many points it has.
    pub fn points(&self) -> usize {
        match self {
            Profile::Grid { x, y } => x.len() * y.len(),
            Profile::Triangles { points, .. } => points.len(),
        }
    }

    /// How many cells it has.
    pub fn cells(&self) -> usize {
        match self {
            Profile::Grid { x, y } => x.len().saturating_sub(1) * y.len().saturating_sub(1),
            Profile::Triangles { triangles, .. } => triangles.len(),
        }
    }

    /// How many tetrahedra a cell is split into between two levels.
    pub fn tets_per_cell(&self) -> usize {
        match self {
            Profile::Grid { .. } => TETS_PER_CELL,
            Profile::Triangles { .. } => 3,
        }
    }

    /// Point `k` at height `z`.
    fn point(&self, k: usize, z: f64) -> Point3 {
        match self {
            Profile::Grid { x, .. } => {
                let n = x.len().max(1);
                Point3::new(self.grid_x(k % n), self.grid_y(k / n), z)
            }
            Profile::Triangles { points, .. } => points.get(k).map_or_else(|| Point3::new(0.0, 0.0, z), |p| Point3::new(p[0], p[1], z)),
        }
    }

    fn grid_x(&self, i: usize) -> f64 {
        match self {
            Profile::Grid { x, .. } => x.get(i).copied().unwrap_or(0.0),
            Profile::Triangles { .. } => 0.0,
        }
    }

    fn grid_y(&self, j: usize) -> f64 {
        match self {
            Profile::Grid { y, .. } => y.get(j).copied().unwrap_or(0.0),
            Profile::Triangles { .. } => 0.0,
        }
    }
}

/// The six tetrahedra of a box as corner offsets (dx, dy, dz) ∈ {0,1}³ encoded as dx + 2dy + 4dz:
/// one per axis order of the path from corner 0 to corner 7.
const KUHN: [[usize; 4]; TETS_PER_CELL] = {
    const AXES: [[usize; 3]; 6] = [[1, 2, 4], [1, 4, 2], [2, 1, 4], [2, 4, 1], [4, 1, 2], [4, 2, 1]];
    let mut out = [[0; 4]; 6];
    let mut i = 0;
    while i < 6 {
        let a = AXES[i];
        out[i] = [0, a[0], a[0] + a[1], 7];
        i += 1;
    }
    out
};

/// An extruded mesh: its nodes, tetrahedra and their materials (no boundary yet: each caller
/// tags its own), and the first tetrahedron of each cell at each level ([`CELL_LEFT_OUT`] for
/// one left out), by level then cell.
#[derive(Clone, Debug, PartialEq)]
pub struct Extruded {
    pub mesh: Mesh,
    pub first: Vec<u32>,
}

/// `profile` extruded through the levels `z` (rising): cell `c` between levels `l` and `l + 1`
/// is `fill(l, c)`'s material, or left out for `None`. The caller caps the size first (the
/// tetrahedra are `profile.tets_per_cell()` per cell filled).
///
/// ```
/// use emcraft_mesh::extrude::{Profile, extrude};
///
/// // A 2 × 1 grid of 1 mm squares through two levels, the second cell left out.
/// let (x, y) = ([0.0, 0.001, 0.002], [0.0, 0.001]);
/// let e = extrude(&Profile::Grid { x: &x, y: &y }, &[0.0, 0.001], |_, c| (c == 0).then_some(0))?;
/// assert_eq!((e.mesh.nodes.len(), e.mesh.tets.len()), (12, 6));
/// assert_eq!(e.first, [0, u32::MAX]);
/// // Levels that do not rise are refused.
/// assert!(extrude(&Profile::Grid { x: &x, y: &y }, &[0.0, 0.0], |_, _| Some(0)).is_err());
/// # Ok::<(), emcraft_mesh::MeshError>(())
/// ```
pub fn extrude(profile: &Profile, z: &[f64], mut fill: impl FnMut(usize, usize) -> Option<u32>) -> Result<Extruded> {
    if z.len() < 2 || !z.windows(2).all(|w| w[1] > w[0]) || !z.iter().all(|v| v.is_finite()) {
        return Err(MeshError::invalid("extrusion", "needs two or more rising, finite levels"));
    }
    let np = profile.points();
    if (np as f64) * (z.len() as f64) > u32::MAX as f64 / 2.0 {
        return Err(MeshError::invalid("extrusion", "has too many nodes"));
    }
    let nodes: Vec<Point3> = z.iter().flat_map(|&h| (0..np).map(move |k| profile.point(k, h))).collect();
    let node = |level: usize, k: usize| (level * np + k) as u32;
    let cells = profile.cells();
    let mut mesh = Mesh { nodes, ..Mesh::default() };
    let mut first = Vec::with_capacity(cells * (z.len() - 1));
    for level in 0..z.len() - 1 {
        for c in 0..cells {
            let Some(material) = fill(level, c) else {
                first.push(CELL_LEFT_OUT);
                continue;
            };
            first.push(mesh.tets.len() as u32);
            match profile {
                Profile::Grid { x, .. } => {
                    let nx = x.len() - 1;
                    let (i, j) = (c % nx, c / nx);
                    let corner = |k: usize| node(level + ((k >> 2) & 1), i + (k & 1) + (nx + 1) * (j + ((k >> 1) & 1)));
                    for pattern in KUHN {
                        mesh.tets.push(positive(&mesh.nodes, pattern.map(corner))?);
                        mesh.tet_material.push(material);
                    }
                }
                Profile::Triangles { triangles, .. } => {
                    let mut v = triangles.get(c).copied().unwrap_or_default();
                    v.sort_unstable();
                    let [a, b, cc] = v.map(|k| node(level, k as usize));
                    let [a2, b2, c2] = v.map(|k| node(level + 1, k as usize));
                    for t in [[a, b, cc, c2], [a, b, b2, c2], [a, a2, b2, c2]] {
                        mesh.tets.push(positive(&mesh.nodes, t)?);
                        mesh.tet_material.push(material);
                    }
                }
            }
        }
    }
    Ok(Extruded { mesh, first })
}

/// Tetrahedron `t` with its last two nodes swapped when its volume is negative; a flat one is
/// refused.
fn positive(nodes: &[Point3], t: [u32; 4]) -> Result<[u32; 4]> {
    let p = |i: u32| nodes.get(i as usize).copied().ok_or_else(|| MeshError::invalid("extrusion", "a node out of range"));
    let [a, b, c, d] = [p(t[0])?, p(t[1])?, p(t[2])?, p(t[3])?];
    let v = triple(b - a, c - a, d - a);
    if v > 0.0 {
        Ok(t)
    } else if v < 0.0 {
        Ok([t[0], t[1], t[3], t[2]])
    } else {
        Err(MeshError::invalid("extrusion", "a flat cell (a triangle of no area or levels of no height)"))
    }
}

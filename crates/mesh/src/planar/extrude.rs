//! The planar mesh extruded through a board's stack of slabs ([`crate::extrude`]): each
//! triangle a column of prisms, each prism three tetrahedra (with the triangle's nodes a < b < c,
//! (a, b, c, c′), (a, b, b′, c′), (a, a′, b′, c′)), the copper a conducting sheet on the
//! triangles it covers.

use emcraft_model::Mesh;
use emcraft_model::mesh::{Boundary, BoundaryFace};

use super::{NO_NET, Planar};
use crate::{MAX_TETS, MeshError, Result};

/// The most cells across one slab.
pub const MAX_CELLS: usize = 64;

/// A slab of the stack: its thickness (m), the material of its tetrahedra and the cells across
/// it, each `ratio` times the height of the one below (1: even; above 1 growing upwards, away
/// from copper under the slab; below 1 shrinking towards copper over it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slab {
    pub thickness: f64,
    pub material: u32,
    pub cells: usize,
    pub ratio: f64,
}

impl Slab {
    /// The heights of the slab's levels over its bottom, the last its thickness.
    fn levels(&self) -> Vec<f64> {
        let n = self.cells;
        let r = self.ratio;
        // The first cell h₀ with h₀ (1 + r + … + rⁿ⁻¹) = thickness.
        let sum = if (r - 1.0).abs() < 1e-12 { n as f64 } else { (r.powi(n as i32) - 1.0) / (r - 1.0) };
        let mut at = 0.0;
        let mut out: Vec<f64> = (0..n)
            .map(|k| {
                at += self.thickness * r.powi(k as i32) / sum;
                at
            })
            .collect();
        if let Some(last) = out.last_mut() {
            *last = self.thickness;
        }
        out
    }
}

/// The stack from the bottom (at `z0`) up, and where the copper lies: each sheet the layer's
/// index into [`Planar::nets`] and the interface it lies on (0 the bottom of the first slab, k
/// the top of slab k − 1); and the holes conductors make through it.
#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    pub z0: f64,
    pub slabs: Vec<Slab>,
    pub sheets: Vec<(usize, usize)>,
    pub holes: Vec<Hole>,
}

/// A conductor through the stack between two interfaces (a via's barrel): its triangles'
/// prisms left out, their walls conducting.
#[derive(Clone, Debug, PartialEq)]
pub struct Hole {
    pub triangles: Vec<u32>,
    pub from: usize,
    pub to: usize,
}

fn invalid(why: impl Into<String>) -> MeshError {
    MeshError::invalid("board stack", why)
}

/// The tetrahedral mesh of `planar` through `stack`: its outer faces and the holes' walls
/// conductors (the caller retags the ports'), the copper's triangles conducting sheets inside,
/// and each sheet face's net in the second vector (in the order of the sheet faces among the
/// boundary faces).
pub fn extrude(planar: &Planar, stack: &Stack) -> Result<(Mesh, Vec<u32>)> {
    if stack.slabs.is_empty() {
        return Err(invalid("a stack needs at least one slab"));
    }
    let fine = |s: &Slab| s.thickness > 0.0 && s.thickness.is_finite() && (1..=MAX_CELLS).contains(&s.cells) && (0.1..=10.0).contains(&s.ratio);
    if !stack.z0.is_finite() || !stack.slabs.iter().all(fine) {
        return Err(invalid(format!("each slab needs a positive thickness, 1 to {MAX_CELLS} cells across and a ratio between 0.1 and 10")));
    }
    let levels: usize = stack.slabs.iter().map(|s| s.cells).sum::<usize>() + 1;
    let tets = 3.0 * planar.triangles.len() as f64 * (levels - 1) as f64;
    crate::too_large(tets)?;
    if (planar.points.len() as f64) * (levels as f64) > u32::MAX as f64 / 2.0 || tets > MAX_TETS as f64 {
        return Err(MeshError::TooLarge { tets: tets as usize, limit: MAX_TETS });
    }
    // The heights of the levels, and the level of each interface.
    let mut z = vec![stack.z0];
    let mut interface = vec![0usize];
    for s in &stack.slabs {
        let base = *z.last().unwrap_or(&stack.z0);
        z.extend(s.levels().into_iter().map(|h| base + h));
        interface.push(z.len() - 1);
    }
    let np = planar.points.len() as u32;
    let node = |level: usize, v: u32| level as u32 * np + v;
    let top = levels - 1;
    let count = planar.triangles.len();
    // The prisms the holes take out, by level then triangle.
    let mut gone = vec![false; top * count];
    for h in &stack.holes {
        let (Some(&from), Some(&to)) = (interface.get(h.from), interface.get(h.to)) else {
            return Err(invalid("a hole names an interface the stack does not have"));
        };
        for level in from.min(to)..from.max(to) {
            for &t in &h.triangles {
                if let Some(g) = gone.get_mut(level * count + t as usize) {
                    *g = true;
                }
            }
        }
    }
    // Each level's slab's material.
    let material: Vec<u32> = stack.slabs.iter().flat_map(|s| std::iter::repeat_n(s.material, s.cells)).collect();
    let profile = crate::extrude::Profile::Triangles { points: &planar.points, triangles: &planar.triangles };
    let fill = |level: usize, t: usize| if gone[level * count + t] { None } else { material.get(level).copied() };
    let crate::extrude::Extruded { mut mesh, first } = crate::extrude::extrude(&profile, &z, fill)?;
    let prism = |level: usize, t: usize| first.get(level * count + t).copied().filter(|&f| f != crate::CELL_LEFT_OUT);
    // Each triangle's neighbour across each of its edges (none on the outline).
    let mut edges: Vec<([u32; 2], usize)> = planar
        .triangles
        .iter()
        .enumerate()
        .flat_map(|(t, v)| (0..3).map(move |k| ([v[k].min(v[(k + 1) % 3]), v[k].max(v[(k + 1) % 3])], t)))
        .collect();
    edges.sort_unstable();
    let across = |e: [u32; 2], t: usize| {
        let i = edges.partition_point(|x| x.0 < e);
        edges[i..].iter().take_while(|x| x.0 == e).find(|x| x.1 != t).map(|x| x.1)
    };
    // The walls: every face of a prism left with no prism across it — the box's outside, and
    // the holes' sides and ends — a conductor.
    for (t, tri) in planar.triangles.iter().enumerate() {
        let mut v = *tri;
        v.sort_unstable();
        for level in 0..top {
            let Some(f) = prism(level, t) else { continue };
            // Below: the first tetrahedron's base (a, b, c); above: the third's (a′, b′, c′).
            if level == 0 || prism(level - 1, t).is_none() {
                mesh.boundary.push(BoundaryFace { nodes: v.map(|x| node(level, x)), tet: f, tag: Boundary::Pec });
            }
            if level + 1 == top || prism(level + 1, t).is_none() {
                mesh.boundary.push(BoundaryFace { nodes: v.map(|x| node(level + 1, x)), tet: f + 2, tag: Boundary::Pec });
            }
            for k in 0..3 {
                let (p, q) = (v[k].min(v[(k + 1) % 3]), v[k].max(v[(k + 1) % 3]));
                if across([p, q], t).is_some_and(|n| prism(level, n).is_some()) {
                    continue;
                }
                // The quadrilateral (p, q, q′, p′) cut along p–q′ (p the lesser).
                let (pl, ql, pu, qu) = (node(level, p), node(level, q), node(level + 1, p), node(level + 1, q));
                for face in [[pl, ql, qu], [pl, pu, qu]] {
                    mesh.boundary.push(BoundaryFace { nodes: face, tet: holder(&mesh, f, face)?, tag: Boundary::Pec });
                }
            }
        }
    }
    // The copper: a sheet face per covered triangle on its interface, between two prisms (a
    // face with a prism on one side only is a wall already).
    let mut nets = Vec::new();
    for &(layer, at) in &stack.sheets {
        let (Some(covered), Some(&level)) = (planar.nets.get(layer), interface.get(at)) else {
            return Err(invalid("a sheet names a layer or an interface the stack does not have"));
        };
        if level == 0 || level == top {
            continue;
        }
        for (t, (tri, &net)) in planar.triangles.iter().zip(covered).enumerate() {
            let (Some(above), Some(_)) = (prism(level, t), prism(level - 1, t)) else { continue };
            if net == NO_NET {
                continue;
            }
            let mut v = *tri;
            v.sort_unstable();
            mesh.boundary.push(BoundaryFace { nodes: v.map(|x| node(level, x)), tet: above, tag: Boundary::Pec });
            nets.push(net);
        }
    }
    Ok((mesh, nets))
}

/// Which of prism `first`'s three tetrahedra holds `face`.
fn holder(mesh: &Mesh, first: u32, face: [u32; 3]) -> Result<u32> {
    (first..first + 3)
        .find(|&t| mesh.tets.get(t as usize).is_some_and(|tet| face.iter().all(|v| tet.contains(v))))
        .ok_or_else(|| invalid("a side face held by no tetrahedron"))
}

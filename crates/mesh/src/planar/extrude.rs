//! The planar mesh extruded through a board's stack of slabs: each triangle a column of prisms,
//! each prism three tetrahedra, the copper a conducting sheet on the triangles it covers.
//!
//! Prisms are split by the rule of J. Dompierre, P. Labbé, M.-G. Vallet, R. Camarero, *How to
//! subdivide pyramids, prisms and hexahedra into tetrahedra*, Proc. 8th International Meshing
//! Roundtable, 1999: each quadrilateral side is cut by its diagonal from its node of least
//! global number, which two prisms sharing the side agree on, so the tetrahedra conform. With the
//! triangle's nodes numbered a < b < c and each level above the one below, the cuts are a–b′,
//! b–c′ and a–c′, and the prism (a, b, c, a′, b′, c′) is (a, b, c, c′), (a, b, b′, c′),
//! (a, a′, b′, c′).

use emcraft_model::geom::triple;
use emcraft_model::mesh::{Boundary, BoundaryFace};
use emcraft_model::{Mesh, Point3};

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
/// the top of slab k − 1).
#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    pub z0: f64,
    pub slabs: Vec<Slab>,
    pub sheets: Vec<(usize, usize)>,
}

fn invalid(why: impl Into<String>) -> MeshError {
    MeshError::invalid("board stack", why)
}

/// The tetrahedral mesh of `planar` through `stack`: its outer faces conductors (the caller
/// retags the ports'), the copper's triangles conducting sheets inside, and each sheet face's
/// net in the second vector (in the order of the sheet faces among the boundary faces).
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
    let nodes: Vec<Point3> = z.iter().flat_map(|&h| planar.points.iter().map(move |p| Point3::new(p[0], p[1], h))).collect();
    let node = |level: usize, v: u32| level as u32 * np + v;
    let mut mesh = Mesh { nodes, ..Mesh::default() };
    // The first tetrahedron of each prism, by level then triangle.
    let prism = |level: usize, t: usize| (3 * (level * planar.triangles.len() + t)) as u32;
    for (slab, s) in stack.slabs.iter().enumerate() {
        for level in interface[slab]..interface[slab] + s.cells {
            for tri in &planar.triangles {
                let mut v = *tri;
                v.sort_unstable();
                let [a, b, c] = v.map(|x| node(level, x));
                let [a2, b2, c2] = v.map(|x| node(level + 1, x));
                for t in [[a, b, c, c2], [a, b, b2, c2], [a, a2, b2, c2]] {
                    mesh.tets.push(positive(&mesh.nodes, t)?);
                    mesh.tet_material.push(s.material);
                }
            }
        }
    }
    let top = levels - 1;
    // The bottom and top: the bottom faces of the first level's (a, b, c, c′), the top faces of
    // the last level's (a, a′, b′, c′).
    for (t, tri) in planar.triangles.iter().enumerate() {
        let mut v = *tri;
        v.sort_unstable();
        mesh.boundary.push(BoundaryFace { nodes: v.map(|x| node(0, x)), tet: prism(0, t), tag: Boundary::Pec });
        mesh.boundary.push(BoundaryFace { nodes: v.map(|x| node(top, x)), tet: prism(top - 1, t) + 2, tag: Boundary::Pec });
    }
    // The sides: the planar mesh's outline edges, each a quadrilateral per level cut as the
    // prism cuts it.
    let mut edges: Vec<([u32; 2], usize)> = planar
        .triangles
        .iter()
        .enumerate()
        .flat_map(|(t, v)| (0..3).map(move |k| ([v[k].min(v[(k + 1) % 3]), v[k].max(v[(k + 1) % 3])], t)))
        .collect();
    edges.sort_unstable();
    for (k, &(e, t)) in edges.iter().enumerate() {
        let shared = edges.get(k + 1).is_some_and(|x| x.0 == e) || (k > 0 && edges.get(k - 1).is_some_and(|x| x.0 == e));
        if shared {
            continue;
        }
        let [p, q] = e;
        for level in 0..top {
            // The quadrilateral (p, q, q′, p′) cut along p–q′ (p the lesser).
            let (pl, ql, pu, qu) = (node(level, p), node(level, q), node(level + 1, p), node(level + 1, q));
            for face in [[pl, ql, qu], [pl, pu, qu]] {
                let tet = holder(&mesh, prism(level, t), face)?;
                mesh.boundary.push(BoundaryFace { nodes: face, tet, tag: Boundary::Pec });
            }
        }
    }
    // The copper: a sheet face per covered triangle on its interface.
    let mut nets = Vec::new();
    for &(layer, at) in &stack.sheets {
        let (Some(covered), Some(&level)) = (planar.nets.get(layer), interface.get(at)) else {
            return Err(invalid("a sheet names a layer or an interface the stack does not have"));
        };
        // Copper on the bottom or the top is on the conducting wall already.
        if level == 0 || level == top {
            continue;
        }
        for (t, (tri, &net)) in planar.triangles.iter().zip(covered).enumerate() {
            if net == NO_NET {
                continue;
            }
            let mut v = *tri;
            v.sort_unstable();
            // Held by the prism above it as its first tetrahedron's base.
            mesh.boundary.push(BoundaryFace { nodes: v.map(|x| node(level, x)), tet: prism(level, t), tag: Boundary::Pec });
            nets.push(net);
        }
    }
    Ok((mesh, nets))
}

/// Tetrahedron `t` with its last two nodes swapped when its volume is negative; a flat one is
/// refused.
fn positive(nodes: &[Point3], t: [u32; 4]) -> Result<[u32; 4]> {
    let p = |i: u32| nodes.get(i as usize).copied().ok_or_else(|| invalid("a node out of range"));
    let [a, b, c, d] = [p(t[0])?, p(t[1])?, p(t[2])?, p(t[3])?];
    let v = triple(b - a, c - a, d - a);
    if v > 0.0 {
        Ok(t)
    } else if v < 0.0 {
        Ok([t[0], t[1], t[3], t[2]])
    } else {
        Err(invalid("a flat prism (a triangle of no area or a slab of no thickness)"))
    }
}

/// Which of prism `first`'s three tetrahedra holds `face`.
fn holder(mesh: &Mesh, first: u32, face: [u32; 3]) -> Result<u32> {
    (first..first + 3)
        .find(|&t| mesh.tets.get(t as usize).is_some_and(|tet| face.iter().all(|v| tet.contains(v))))
        .ok_or_else(|| invalid("a side face held by no tetrahedron"))
}

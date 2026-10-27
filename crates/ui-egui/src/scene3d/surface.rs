//! What each boundary face of a mesh is: the air round an open problem, a closed problem's
//! walls, a conductor inside the body, or a face of the cell that periodic boundaries tie.

use emcraft_model::Model;
use emcraft_model::mesh::Boundary;

/// What a boundary face is: the air around an open problem (its outer surface), a closed
/// problem's walls, or a conductor inside the body (a subtracted feature, a conducting
/// inclusion, a post).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Air,
    Wall,
    Conductor,
}

/// Tells a body's outer surface from the conductors inside it. The boundary faces (ports
/// apart, which join an inner conductor to the outer wall at a coaxial end) fall into pieces
/// joined by shared edges; the pieces reaching all six sides of the model's box are its outer
/// surface: air around an open problem, else walls. The rest are conductors inside.
pub(super) struct Surface {
    /// Per boundary face: whether it lies on the outer surface.
    outer: Vec<bool>,
    open: bool,
}

impl Surface {
    pub(super) fn of(m: &Model, mesh: &emcraft_model::Mesh) -> Surface {
        let open = m.open.is_some();
        let n = mesh.boundary.len();
        // A waveguide's faces are all walls.
        if m.waveguide.is_some() || n == 0 {
            return Surface { outer: vec![true; n], open };
        }
        let joins = |f: &emcraft_model::BoundaryFace| !matches!(f.tag, Boundary::Port(_));
        // Union–find over faces sharing an edge.
        let mut parent: Vec<usize> = (0..n).collect();
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        let mut edges: Vec<([u32; 2], usize)> = Vec::with_capacity(3 * n);
        for (i, f) in mesh.boundary.iter().enumerate().filter(|(_, f)| joins(f)) {
            let v = f.nodes;
            for (a, b) in [(v[0], v[1]), (v[1], v[2]), (v[2], v[0])] {
                edges.push(([a.min(b), a.max(b)], i));
            }
        }
        edges.sort_unstable();
        for w in edges.windows(2) {
            if w[0].0 == w[1].0 {
                let (a, b) = (root(&mut parent, w[0].1), root(&mut parent, w[1].1));
                parent[a] = b;
            }
        }
        // Each piece's box.
        let mut lo = vec![[f64::INFINITY; 3]; n];
        let mut hi = vec![[f64::NEG_INFINITY; 3]; n];
        for (i, f) in mesh.boundary.iter().enumerate().filter(|(_, f)| joins(f)) {
            let r = root(&mut parent, i);
            for v in f.nodes {
                if let Some(p) = mesh.nodes.get(v as usize) {
                    for d in 0..3 {
                        lo[r][d] = lo[r][d].min(p[d]);
                        hi[r][d] = hi[r][d].max(p[d]);
                    }
                }
            }
        }
        let (blo, bhi) = mesh.nodes.iter().fold(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]), |(mut l, mut h), p| {
            for d in 0..3 {
                l[d] = l[d].min(p[d]);
                h[d] = h[d].max(p[d]);
            }
            (l, h)
        });
        let tol = 1e-6 * (0..3).map(|d| bhi[d] - blo[d]).fold(0.0f64, f64::max);
        let outer = (0..n)
            .map(|i| {
                if !joins(&mesh.boundary[i]) {
                    return true;
                }
                let r = root(&mut parent, i);
                (0..3).all(|d| lo[r][d] <= blo[d] + tol && hi[r][d] >= bhi[d] - tol)
            })
            .collect();
        Surface { outer, open }
    }

    /// Boundary face `i`'s kind.
    pub(super) fn kind(&self, i: usize) -> Kind {
        match (self.outer.get(i).copied().unwrap_or(true), self.open) {
            (true, true) => Kind::Air,
            (true, false) => Kind::Wall,
            (false, _) => Kind::Conductor,
        }
    }
}

/// The faces of the cell that a mesh's periodic boundaries tie: per axis, the mesh's extent,
/// if the axis is tied.
pub(super) struct PeriodicPlanes([Option<(f64, f64)>; 3]);

impl PeriodicPlanes {
    pub(super) fn of(m: &Model, mesh: &emcraft_model::Mesh) -> PeriodicPlanes {
        let Some(p) = &m.periodic else { return PeriodicPlanes([None; 3]) };
        let tied = [p.x, p.y, p.z];
        let mut out = [None; 3];
        for d in 0..3 {
            if tied[d] {
                let (lo, hi) = mesh.nodes.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(q[d]), b.max(q[d])));
                if lo < hi {
                    out[d] = Some((lo, hi));
                }
            }
        }
        PeriodicPlanes(out)
    }

    /// Whether the face through `nodes` lies on a tied face of the cell.
    pub(super) fn holds(&self, mesh: &emcraft_model::Mesh, nodes: &[u32; 3]) -> bool {
        self.0.iter().enumerate().any(|(d, pl)| {
            pl.is_some_and(|(lo, hi)| {
                let tol = 1e-9 * (hi - lo);
                [lo, hi].iter().any(|c| nodes.iter().all(|n| mesh.nodes.get(*n as usize).is_some_and(|q| (q[d] - c).abs() <= tol)))
            })
        })
    }
}

//! Points a mesh Boolean leaves in the middle of a straight crease, removed. Where a body's flat
//! face crosses another surface's, the intersection is a straight line, but each triangle edge of
//! either face that crosses the line adds a point on it: a box's face extended past a wall by a
//! small step has its diagonal cross the wall a hair from the corner, leaving a segment far
//! shorter than the element size that the mesher would resolve with many tiny tetrahedra. Such a
//! point — its triangles in exactly two planes, one face tag on each side, and its two neighbours
//! along the crease on a straight line through it — is collapsed into one of those neighbours
//! (an edge collapse that keeps every triangle in its plane and facing the same way), which
//! leaves the surface the same set of points.
//!
//! The edge collapse is Hoppe et al.'s (H. Hoppe, T. DeRose, T. Duchamp, J. McDonald,
//! W. Stuetzle, *Mesh optimization*, SIGGRAPH 1993), allowed only under the link condition that
//! keeps the surface a manifold of the same topology (T. K. Dey, H. Edelsbrunner, S. Guha,
//! D. V. Nekhayev, *Topology preserving edge contraction*, Publ. Inst. Math. 66(80), 1999).

use std::collections::BTreeSet;

use emcraft_model::{Point3, Vec3};

use crate::csg::Surface;

/// Normals within this angle (radians, about) are one plane; points within it of a line are
/// on it. Far above rounding of the Boolean's crossing points, far below any modelled angle.
const FLAT: f64 = 1e-9;

/// Passes over the candidates, each removing what it can (a cap; one or two do it).
const MAX_PASSES: usize = 8;

struct Work<'a> {
    s: &'a mut Surface,
    alive: Vec<bool>,
    around: Vec<Vec<usize>>,
}

impl Work<'_> {
    fn normal(&self, t: [u32; 3]) -> Vec3 {
        let p = |i: u32| self.s.positions.get(i as usize).copied().unwrap_or(Point3::ORIGIN);
        (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]))
    }

    fn live(&self, v: usize) -> Vec<usize> {
        self.around.get(v).map(|l| l.iter().copied().filter(|&t| self.alive[t]).collect()).unwrap_or_default()
    }

    fn neighbours(&self, v: usize) -> BTreeSet<u32> {
        self.live(v).iter().flat_map(|&t| self.s.triangles[t]).filter(|&x| x as usize != v).collect()
    }

    /// The crease neighbours of `v` when it lies inside a straight crease between two planes,
    /// one tag on each side.
    fn crease(&self, v: usize) -> Option<[u32; 2]> {
        let tris = self.live(v);
        if tris.len() < 2 {
            return None;
        }
        // The planes (unit normals) and each one's tag.
        let mut planes: Vec<(Vec3, u32)> = Vec::new();
        let mut plane_of = Vec::with_capacity(tris.len());
        for &t in &tris {
            let n = self.normal(self.s.triangles[t]).normalized()?;
            let tag = *self.s.tags.get(t)?;
            match planes.iter().position(|(m, _)| (*m - n).norm() <= FLAT) {
                Some(k) if planes[k].1 == tag => plane_of.push(k),
                Some(_) => return None,
                None => {
                    planes.push((n, tag));
                    plane_of.push(planes.len() - 1);
                }
            }
        }
        if planes.len() != 2 {
            return None;
        }
        // Edges from v whose two triangles lie in different planes.
        let mut sides: Vec<(u32, usize)> = Vec::new();
        for (&t, &k) in tris.iter().zip(&plane_of) {
            for x in self.s.triangles[t] {
                if x as usize != v {
                    sides.push((x, k));
                }
            }
        }
        sides.sort_unstable();
        let mut crease = Vec::new();
        for w in sides.chunk_by(|a, b| a.0 == b.0) {
            match w {
                [(x, a), (_, b)] if a != b => crease.push(*x),
                [_, _] => {}
                _ => return None,
            }
        }
        let [u, w] = crease[..] else { return None };
        let p = |i: u32| self.s.positions.get(i as usize).copied();
        let (pv, pu, pw) = (p(v as u32)?, p(u)?, p(w)?);
        let (a, b) = (pu - pv, pw - pv);
        let straight = a.cross(b).norm() <= FLAT * a.norm() * b.norm() && a.dot(b) < 0.0;
        straight.then_some([u, w])
    }

    /// Collapses v into u when that keeps the surface a manifold and every triangle facing as it
    /// did; whether it did.
    fn collapse(&mut self, v: usize, u: u32) -> bool {
        let tris = self.live(v);
        let (with_u, rest): (Vec<usize>, Vec<usize>) = tris.iter().partition(|&&t| self.s.triangles[t].contains(&u));
        if with_u.len() != 2 {
            return false;
        }
        // The link condition: u and v share exactly the two corners opposite their edge.
        let opposite: BTreeSet<u32> = with_u.iter().flat_map(|&t| self.s.triangles[t]).filter(|&x| x != u && x as usize != v).collect();
        let shared: BTreeSet<u32> = self.neighbours(v).intersection(&self.neighbours(u as usize)).copied().collect();
        if shared != opposite || opposite.len() != 2 {
            return false;
        }
        let mut renamed = Vec::with_capacity(rest.len());
        for &t in &rest {
            let old = self.s.triangles[t];
            let new = old.map(|x| if x as usize == v { u } else { x });
            let (n0, n1) = (self.normal(old), self.normal(new));
            if !(n0.dot(n1) > 0.0) || (n1.normalized().zip(n0.normalized())).is_none_or(|(a, b)| (a - b).norm() > FLAT) {
                return false;
            }
            renamed.push((t, new));
        }
        for t in with_u {
            self.alive[t] = false;
        }
        if let Some(l) = self.around.get_mut(v) {
            l.clear();
        }
        for (t, new) in renamed {
            self.s.triangles[t] = new;
            if let Some(l) = self.around.get_mut(u as usize) {
                l.push(t);
            }
        }
        true
    }
}

/// Removes the points of `s` marked in `candidates` that lie inside straight creases (see the
/// module notes); the surface keeps its shape, tags and orientation.
pub(crate) fn simplify(s: &mut Surface, candidates: &[bool]) {
    let n = s.positions.len();
    let mut around = vec![Vec::new(); n];
    for (t, tri) in s.triangles.iter().enumerate() {
        for &v in tri {
            if let Some(l) = around.get_mut(v as usize) {
                l.push(t);
            }
        }
    }
    let mut w = Work { alive: vec![true; s.triangles.len()], around, s };
    for _ in 0..MAX_PASSES {
        let mut changed = false;
        for v in 0..n {
            if !candidates.get(v).copied().unwrap_or(false) {
                continue;
            }
            let Some([a, b]) = w.crease(v) else { continue };
            if w.collapse(v, a) || w.collapse(v, b) {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // Keep the live triangles and the points they use, in their order.
    let Work { alive, s, .. } = w;
    let mut index = vec![u32::MAX; n];
    let mut positions = Vec::new();
    let mut triangles = Vec::new();
    let mut tags = Vec::new();
    for (t, tri) in s.triangles.iter().enumerate() {
        if !alive[t] {
            continue;
        }
        let ids = tri.map(|v| match index.get_mut(v as usize) {
            Some(slot) => {
                if *slot == u32::MAX {
                    positions.push(s.positions.get(v as usize).copied().unwrap_or(Point3::ORIGIN));
                    *slot = (positions.len() - 1) as u32;
                }
                *slot
            }
            None => 0,
        });
        triangles.push(ids);
        tags.push(s.tags.get(t).copied().unwrap_or(0));
    }
    *s = Surface { positions, triangles, tags };
}

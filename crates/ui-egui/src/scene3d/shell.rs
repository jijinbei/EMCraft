//! The walls' thickness, drawn: the metal round the inside is never meshed (the solver sees it
//! as a surface impedance), so its outer surface is made here from the inside's, offset outward
//! by the thickness t, with a rim where the walls end at a port or a symmetry plane.
//!
//! Each vertex moves by the d that best meets d · n̂ = t for the walls round it and d · n̂ = 0
//! for the open faces (ports, symmetry planes) it lies on, n̂ their distinct unit normals, in
//! the least-squares sense (AᵀA + λI) d = Aᵀb: one wall moves it t along its normal, two or
//! three meeting at an edge or a corner to where their offset planes meet (a mitred offset), and
//! a vertex on a port stays in the port's plane, so the rim lies flush with it. The move is
//! capped at [`MAX_STRETCH`] t, so a sharp spike does not shoot out.

use std::collections::BTreeMap;

use emcraft_model::Model;

use super::style::sc;
use super::surface::Kind;
use super::{Built, Overlay, Pick};

/// The longest move, in thicknesses.
pub(super) const MAX_STRETCH: f64 = 3.0;

/// A surface round the inside: welded vertices (m) and triangles, each a wall or open.
#[derive(Default)]
pub(super) struct Inside {
    pub positions: Vec<[f64; 3]>,
    pub tris: Vec<[u32; 3]>,
    pub open: Vec<bool>,
    index: BTreeMap<[i64; 3], u32>,
}

impl Inside {
    /// The vertex at `p`, welded to one already there (within a nanometre).
    pub(super) fn vertex(&mut self, p: [f64; 3]) -> u32 {
        let key = p.map(|c| (c * 1e9).round() as i64);
        let next = self.positions.len() as u32;
        *self.index.entry(key).or_insert_with(|| {
            self.positions.push(p);
            next
        })
    }

    /// A triangle through these points.
    pub(super) fn tri(&mut self, p: [[f64; 3]; 3], open: bool) {
        let v = p.map(|q| self.vertex(q));
        if v[0] != v[1] && v[1] != v[2] && v[0] != v[2] {
            self.tris.push(v);
            self.open.push(open);
        }
    }

    /// A quadrilateral through these points, in order round it.
    pub(super) fn quad(&mut self, q: [[f64; 3]; 4], open: bool) {
        self.tri([q[0], q[1], q[2]], open);
        self.tri([q[0], q[2], q[3]], open);
    }
}

/// The walls' outer surface and their rims, as triangles, and the outline edges (the rims'
/// outer edges, the outer surface's sharp folds).
#[derive(Default, Debug)]
pub(super) struct Shell {
    pub outer: Vec<[[f64; 3]; 3]>,
    pub rim: Vec<[[f64; 3]; 3]>,
    pub edges: Vec<([f64; 3], [f64; 3])>,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// A triangle's unit normal (its corners' turn), if it has an area.
fn unit_normal(p: [[f64; 3]; 3]) -> Option<[f64; 3]> {
    let n = cross(sub(p[1], p[0]), sub(p[2], p[0]));
    let len = dot(n, n).sqrt();
    (len > 0.0 && len.is_finite()).then(|| n.map(|c| c / len))
}

/// The 3 × 3 system m d = r, by Cramer's rule; `None` when it is singular.
fn solve3(m: [[f64; 3]; 3], r: [f64; 3]) -> Option<[f64; 3]> {
    let det = |a: [[f64; 3]; 3]| dot(a[0], cross(a[1], a[2]));
    let d = det(m);
    if !(d.abs() > 1e-300) {
        return None;
    }
    let mut out = [0.0; 3];
    for (k, o) in out.iter_mut().enumerate() {
        let mut a = m;
        for row in 0..3 {
            a[row][k] = r[row];
        }
        *o = det(a) / d;
    }
    Some(out)
}

/// The walls of `inside` offset outward by `thickness` (m). Wall triangles must turn so their
/// normals point out of the inside, into the metal; open ones may turn either way.
pub(super) fn shell(inside: &Inside, thickness: f64) -> Shell {
    let mut out = Shell::default();
    if !(thickness > 0.0 && thickness.is_finite()) {
        return out;
    }
    let n = inside.positions.len();
    let normals: Vec<Option<[f64; 3]>> =
        inside.tris.iter().map(|t| unit_normal(t.map(|v| inside.positions.get(v as usize).copied().unwrap_or([0.0; 3])))).collect();
    // Per vertex: the distinct normals round it, each with where it wants the vertex (t or 0).
    let mut wants: Vec<Vec<([f64; 3], f64)>> = vec![Vec::new(); n];
    for ((t, nrm), open) in inside.tris.iter().zip(&normals).zip(&inside.open) {
        let Some(nrm) = *nrm else { continue };
        let target = if *open { 0.0 } else { thickness };
        for v in t {
            let Some(w) = wants.get_mut(*v as usize) else { continue };
            // An open face's sign does not matter: it only holds the vertex in its plane.
            let same = |(m, b): &([f64; 3], f64)| *b == target && (dot(*m, nrm) > 1.0 - 1e-9 || (*open && dot(*m, nrm) < -1.0 + 1e-9));
            if !w.iter().any(same) {
                w.push((nrm, target));
            }
        }
    }
    let moved: Vec<[f64; 3]> = inside
        .positions
        .iter()
        .zip(&wants)
        .map(|(p, w)| {
            if !w.iter().any(|(_, b)| *b > 0.0) {
                return *p;
            }
            let lambda = 1e-9;
            let mut m = [[0.0; 3]; 3];
            let mut r = [0.0; 3];
            for (nrm, b) in w {
                for i in 0..3 {
                    for j in 0..3 {
                        m[i][j] += nrm[i] * nrm[j];
                    }
                    r[i] += nrm[i] * b;
                }
            }
            for (i, row) in m.iter_mut().enumerate() {
                row[i] += lambda;
            }
            let d = solve3(m, r).unwrap_or([0.0; 3]);
            let len = dot(d, d).sqrt();
            let cap = MAX_STRETCH * thickness;
            let d = if len > cap { d.map(|c| c * cap / len) } else { d };
            add(*p, d)
        })
        .collect();
    let at = |v: u32| inside.positions.get(v as usize).copied().unwrap_or([0.0; 3]);
    let to = |v: u32| moved.get(v as usize).copied().unwrap_or([0.0; 3]);
    // Edges: which triangles have each, the walls' in their turn.
    let mut edges: BTreeMap<[u32; 2], Vec<(usize, [u32; 2])>> = BTreeMap::new();
    for (i, t) in inside.tris.iter().enumerate() {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            edges.entry([a.min(b), a.max(b)]).or_default().push((i, [a, b]));
        }
    }
    let wall = |i: usize| !inside.open.get(i).copied().unwrap_or(true);
    for (_, t) in inside.tris.iter().enumerate().filter(|(i, _)| wall(*i)) {
        out.outer.push(t.map(to));
    }
    let cos_sharp = 25f64.to_radians().cos();
    for faces in edges.values() {
        let walls: Vec<&(usize, [u32; 2])> = faces.iter().filter(|(i, _)| wall(*i)).collect();
        match walls.as_slice() {
            // A wall's edge on no other wall: the rim, from it out to its moved copy.
            [(_, [a, b])] => {
                out.rim.push([at(*a), at(*b), to(*b)]);
                out.rim.push([at(*a), to(*b), to(*a)]);
                out.edges.push((to(*a), to(*b)));
                out.edges.push((at(*a), to(*a)));
                out.edges.push((at(*b), to(*b)));
            }
            [(i, [a, b]), (j, _)] => {
                let sharp = normals.get(*i).copied().flatten().zip(normals.get(*j).copied().flatten()).is_some_and(|(p, q)| dot(p, q) < cos_sharp);
                if sharp {
                    out.edges.push((to(*a), to(*b)));
                }
            }
            _ => {}
        }
    }
    out
}

/// The model's walls drawn `thickness` thick round `inside`, in the overlay's wall style, the
/// rims more solid so the thickness reads at the ports.
pub(super) fn draw(m: &Model, overlay: Overlay, inside: &Inside, thickness: f64, out: &mut Built) {
    let s = shell(inside, thickness);
    let (color, alpha) = super::style::wall_style(m, overlay, false, 0, Kind::Wall);
    let rim_alpha = alpha.max(if overlay == Overlay::Field { 120 } else { 230 });
    let pick = if m.waveguide.is_some() { Pick::Guide } else { Pick::Solid };
    out.scene.tagged(pick.tag(), |scene| {
        for p in &s.outer {
            scene.tri(p[0], p[1], p[2], color, alpha);
        }
        for p in &s.rim {
            scene.tri(p[0], p[1], p[2], sc().metal, rim_alpha);
        }
    });
    for (a, b) in s.edges {
        out.scene.line(a, b, sc().edge, 200);
    }
}

/// Whether a point lies on one of the model's ports' faces (within a micrometre of its plane).
pub(super) fn on_ports(m: &Model) -> impl Fn([f64; 3]) -> bool {
    let tris: Vec<[[f64; 3]; 3]> = m.ports.iter().flat_map(|p| super::extras::port_face(m, p)).collect();
    move |c: [f64; 3]| {
        tris.iter().any(|t| {
            let Some(n) = unit_normal(*t) else { return false };
            if dot(sub(c, t[0]), n).abs() > 1e-6 {
                return false;
            }
            // Inside: on the inner side of each edge (a hair of slack).
            (0..3).all(|k| dot(cross(sub(t[(k + 1) % 3], t[k]), sub(c, t[k])), n) >= -1e-12)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box [0, a] × [0, b] × [0, l], its faces turned outward, the z ends open.
    fn guide(a: f64, b: f64, l: f64) -> Inside {
        let mut s = Inside::default();
        let c = |x: f64, y: f64, z: f64| [x, y, z];
        // Walls, outward: y = 0 (−y), y = b (+y), x = 0 (−x), x = a (+x).
        s.quad([c(0.0, 0.0, 0.0), c(a, 0.0, 0.0), c(a, 0.0, l), c(0.0, 0.0, l)], false);
        s.quad([c(0.0, b, 0.0), c(0.0, b, l), c(a, b, l), c(a, b, 0.0)], false);
        s.quad([c(0.0, 0.0, 0.0), c(0.0, 0.0, l), c(0.0, b, l), c(0.0, b, 0.0)], false);
        s.quad([c(a, 0.0, 0.0), c(a, b, 0.0), c(a, b, l), c(a, 0.0, l)], false);
        // The ends.
        s.quad([c(0.0, 0.0, 0.0), c(0.0, b, 0.0), c(a, b, 0.0), c(a, 0.0, 0.0)], true);
        s.quad([c(0.0, 0.0, l), c(a, 0.0, l), c(a, b, l), c(0.0, b, l)], true);
        s
    }

    #[test]
    fn a_guide_gets_a_mitred_shell_flush_with_its_ports() {
        let (a, b, l, t) = (0.02286, 0.01016, 0.05, 0.00127);
        let s = shell(&guide(a, b, l), t);
        assert_eq!(s.outer.len(), 8);
        // Every outer corner is where the offset planes meet: t outside two walls, on an end.
        for p in s.outer.iter().flatten() {
            assert!(((p[0] + t).abs() < 1e-10 || (p[0] - a - t).abs() < 1e-10), "{p:?}");
            assert!(((p[1] + t).abs() < 1e-10 || (p[1] - b - t).abs() < 1e-10), "{p:?}");
            assert!(p[2].abs() < 1e-9 || (p[2] - l).abs() < 1e-9, "{p:?}");
        }
        // A rim round each end, in its plane: the eight edges round the two ends.
        assert_eq!(s.rim.len(), 16);
        assert!(s.rim.iter().flatten().all(|p| p[2].abs() < 1e-9 || (p[2] - l).abs() < 1e-9));
        // Its area is the frame's, (a + 2t)(b + 2t) − ab, at each end.
        let area: f64 =
            s.rim.iter().map(|p| 0.5 * dot(cross(sub(p[1], p[0]), sub(p[2], p[0])), cross(sub(p[1], p[0]), sub(p[2], p[0]))).sqrt()).sum();
        let frame = (a + 2.0 * t) * (b + 2.0 * t) - a * b;
        assert!((area - 2.0 * frame).abs() < 1e-6 * frame, "{area} against {}", 2.0 * frame);
    }

    #[test]
    fn a_closed_box_has_no_rim_and_nothing_breaks_on_nonsense() {
        let mut g = guide(1.0, 1.0, 1.0);
        g.open.iter_mut().for_each(|o| *o = false);
        let s = shell(&g, 0.1);
        assert!(s.rim.is_empty() && s.outer.len() == 12);
        for t in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let s = shell(&g, t);
            assert!(s.outer.is_empty() && s.rim.is_empty());
        }
        assert!(shell(&Inside::default(), 0.1).outer.is_empty());
    }
}

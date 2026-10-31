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
use emcraft_model::geom::{Point3, Vec3, triple};

use super::faces::unit_normal;
use super::style::sc;
use super::surface::Kind;
use super::{Built, Overlay, Pick};

/// The longest move, in thicknesses.
pub(super) const MAX_STRETCH: f64 = 3.0;

/// A triangle of the inside's surface: a wall, open (a port, a symmetry plane), or inner — metal
/// with the inside again just beyond it (an iris, a post, a wall two guides share), which the
/// outer walls' thickness does not concern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Side {
    Wall,
    Open,
    Inner,
}

impl Side {
    /// Open where `open` holds, else a wall.
    pub(super) fn open_if(open: bool) -> Side {
        if open { Side::Open } else { Side::Wall }
    }
}

/// A surface round the inside: welded vertices (m) and triangles, each a wall or open.
#[derive(Default)]
pub(super) struct Inside {
    positions: Vec<Point3>,
    tris: Vec<([u32; 3], Side)>,
    index: BTreeMap<[i64; 3], u32>,
}

impl Inside {
    /// The vertex at `p`, welded to one already there (within a nanometre).
    fn vertex(&mut self, p: [f64; 3]) -> u32 {
        let key = p.map(|c| (c * 1e9).round() as i64);
        let next = self.positions.len() as u32;
        *self.index.entry(key).or_insert_with(|| {
            self.positions.push(p.into());
            next
        })
    }

    /// A triangle through these points (turned out of the inside), a wall made inner where
    /// `interior` says the inside lies just beyond it.
    pub(super) fn wall_or_inner(&mut self, p: [[f64; 3]; 3], side: Side, interior: &impl Fn([[f64; 3]; 3]) -> bool) {
        let side = if side == Side::Wall && interior(p) { Side::Inner } else { side };
        self.tri(p, side);
    }

    /// A triangle through these points.
    pub(super) fn tri(&mut self, p: [[f64; 3]; 3], side: Side) {
        let v = p.map(|q| self.vertex(q));
        if v[0] != v[1] && v[1] != v[2] && v[0] != v[2] {
            self.tris.push((v, side));
        }
    }

    /// A quadrilateral through these points, in order round it.
    #[cfg(test)]
    pub(super) fn quad(&mut self, q: [[f64; 3]; 4], side: Side) {
        self.tri([q[0], q[1], q[2]], side);
        self.tri([q[0], q[2], q[3]], side);
    }

    /// Vertex `v`'s position (the origin for one out of range).
    fn at(&self, v: u32) -> Point3 {
        self.positions.get(v as usize).copied().unwrap_or_default()
    }

    /// Whether triangle `i` is a wall.
    fn wall(&self, i: usize) -> bool {
        self.side(i) == Some(Side::Wall)
    }

    /// Triangle `i`'s side.
    fn side(&self, i: usize) -> Option<Side> {
        self.tris.get(i).map(|(_, side)| *side)
    }

    /// The planes each vertex should lie on once moved: the distinct normals of the triangles
    /// round it, a wall's `thickness` out, an open face's through the vertex.
    fn planes(&self, normals: &[Option<Vec3>], thickness: f64) -> Vec<Vec<Plane>> {
        let mut planes: Vec<Vec<Plane>> = vec![Vec::new(); self.positions.len()];
        for ((t, side), normal) in self.tris.iter().zip(normals).filter(|((_, side), _)| *side != Side::Inner) {
            let Some(normal) = *normal else { continue };
            let open = *side == Side::Open;
            let offset = if open { 0.0 } else { thickness };
            for v in t {
                let Some(w) = planes.get_mut(*v as usize) else { continue };
                // An open face's sign does not matter: it only holds the vertex in its plane.
                let same = |p: &Plane| p.offset == offset && (p.normal.dot(normal) > 1.0 - 1e-9 || (open && p.normal.dot(normal) < -1.0 + 1e-9));
                if !w.iter().any(same) {
                    w.push(Plane { normal, offset });
                }
            }
        }
        planes
    }

    /// Each edge with the triangles that have it, each in that triangle's turn.
    fn edges(&self) -> BTreeMap<[u32; 2], Vec<(usize, [u32; 2])>> {
        let mut edges: BTreeMap<[u32; 2], Vec<(usize, [u32; 2])>> = BTreeMap::new();
        for (i, (t, _)) in self.tris.iter().enumerate() {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                edges.entry([a.min(b), a.max(b)]).or_default().push((i, [a, b]));
            }
        }
        edges
    }
}

/// A plane a moved vertex should lie on: d · `normal` = `offset` for its move d.
#[derive(Clone, Copy, Debug)]
struct Plane {
    normal: Vec3,
    offset: f64,
}

/// The move d that best meets every plane, (AᵀA + λI) d = Aᵀb, capped at [`MAX_STRETCH`]
/// `thickness`; `None` when no wall is among them (the vertex stays).
fn mitre(planes: &[Plane], thickness: f64) -> Option<Vec3> {
    const LAMBDA: f64 = 1e-9;
    if !planes.iter().any(|p| p.offset > 0.0) {
        return None;
    }
    let mut m = [[0.0; 3]; 3];
    let mut r = [0.0; 3];
    for Plane { normal: n, offset } in planes {
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += n[i] * n[j];
            }
            r[i] += n[i] * offset;
        }
    }
    for (i, row) in m.iter_mut().enumerate() {
        row[i] += LAMBDA;
    }
    let d = solve3(m, r).unwrap_or(Vec3::ZERO);
    let len = d.norm();
    let cap = MAX_STRETCH * thickness;
    Some(if len > cap { d.map(|c| c * cap / len) } else { d })
}

/// The 3 × 3 system m d = r, by Cramer's rule; `None` when it is singular.
fn solve3(m: [[f64; 3]; 3], r: [f64; 3]) -> Option<Vec3> {
    let det = |a: [[f64; 3]; 3]| triple(a[0].into(), a[1].into(), a[2].into());
    let d = det(m);
    if !(d.abs() > 1e-300) {
        return None;
    }
    let mut out = Vec3::ZERO;
    for k in 0..3 {
        let mut a = m;
        for (row, rk) in a.iter_mut().zip(r) {
            row[k] = rk;
        }
        out[k] = det(a) / d;
    }
    Some(out)
}

/// The walls' outer surface and their rims, as triangles, and the outline edges (the rims'
/// outer edges, the outer surface's sharp folds).
#[derive(Default, Debug)]
pub(super) struct Shell {
    pub outer: Vec<[[f64; 3]; 3]>,
    pub rim: Vec<[[f64; 3]; 3]>,
    pub edges: Vec<([f64; 3], [f64; 3])>,
}

/// The walls of `inside` offset outward by `thickness` (m). Wall triangles must turn so their
/// normals point out of the inside, into the metal; open ones may turn either way.
pub(super) fn shell(inside: &Inside, thickness: f64) -> Shell {
    let mut out = Shell::default();
    if !(thickness > 0.0 && thickness.is_finite()) {
        return out;
    }
    let normals: Vec<Option<Vec3>> = inside.tris.iter().map(|(t, _)| unit_normal(t.map(|v| inside.at(v)))).collect();
    let moved: Vec<Point3> =
        inside.positions.iter().zip(inside.planes(&normals, thickness)).map(|(p, planes)| mitre(&planes, thickness).map_or(*p, |d| *p + d)).collect();
    let at = |v: u32| inside.at(v).to_array();
    let to = |v: u32| moved.get(v as usize).copied().unwrap_or_default().to_array();
    for (t, _) in inside.tris.iter().filter(|(_, side)| *side == Side::Wall) {
        out.outer.push(t.map(to));
    }
    let cos_sharp = 25f64.to_radians().cos();
    for faces in inside.edges().values() {
        let walls: Vec<&(usize, [u32; 2])> = faces.iter().filter(|(i, _)| inside.wall(*i)).collect();
        let opens = faces.iter().any(|(i, _)| inside.side(*i) == Some(Side::Open));
        match walls.as_slice() {
            // A wall's edge on an open face and no other wall: the rim, from it out to its moved
            // copy. (Where a wall meets an inner face, the metal goes on: no rim.)
            [(_, [a, b])] if opens || faces.len() == 1 => {
                out.rim.push([at(*a), at(*b), to(*b)]);
                out.rim.push([at(*a), to(*b), to(*a)]);
                out.edges.push((to(*a), to(*b)));
                out.edges.push((at(*a), to(*a)));
                out.edges.push((at(*b), to(*b)));
            }
            [(i, [a, b]), (j, _)] => {
                let normal = |k: usize| normals.get(k).copied().flatten();
                if normal(*i).zip(normal(*j)).is_some_and(|(p, q)| p.dot(q) < cos_sharp) {
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

/// Whether the inside lies again just beyond a wall triangle (turned out of the inside): points
/// along its normal from its centre, out to half the model's smallest side, tested against the
/// geometry. An iris, a post or a shared wall is metal that thin; the guide's outer walls have
/// nothing beyond them.
pub(super) fn interior(m: &Model) -> impl Fn([[f64; 3]; 3]) -> bool + '_ {
    const SAMPLES: usize = 8;
    let reach = m.bounds().map_or(0.0, |b| {
        let s = b.max - b.min;
        0.5 * s.x.min(s.y).min(s.z)
    });
    move |p: [[f64; 3]; 3]| {
        let p = p.map(Point3::from);
        let Some(n) = unit_normal(p) else { return false };
        let c = Point3::new((p[0].x + p[1].x + p[2].x) / 3.0, (p[0].y + p[1].y + p[2].y) / 3.0, (p[0].z + p[1].z + p[2].z) / 3.0);
        (1..=SAMPLES).any(|k| inside(m, c + n * (reach * k as f64 / SAMPLES as f64)))
    }
}

/// Whether a point lies inside the model's guide or region.
fn inside(m: &Model, p: Point3) -> bool {
    if let Some(w) = &m.waveguide {
        let mut z = 0.0;
        for (i, s) in w.sections.iter().enumerate() {
            let end = z + s.length.get();
            if p.z > z && p.z < end {
                let (x, y) = (w.aperture(i).unwrap_or(w.across_x()), w.aperture_y(i).unwrap_or(w.across_y()));
                return p.x > x.start && p.x < x.end() && p.y > y.start && p.y < y.end();
            }
            z = end;
        }
        return false;
    }
    m.material_at(p).is_some()
}

/// Whether a point lies on one of the model's ports' faces (within a micrometre of its plane).
pub(super) fn on_ports(m: &Model) -> impl Fn([f64; 3]) -> bool {
    let faces: Vec<([Point3; 3], Vec3)> = m
        .ports
        .iter()
        .flat_map(|p| super::extras::port_face(m, p))
        .filter_map(|t| {
            let t = t.map(Point3::from);
            Some((t, unit_normal(t)?))
        })
        .collect();
    move |c: [f64; 3]| {
        let c = Point3::from(c);
        faces.iter().any(|(t, n)| {
            // In its plane, and on the inner side of each edge (a hair of slack).
            !((c - t[0]).dot(*n).abs() > 1e-6) && (0..3).all(|k| (t[(k + 1) % 3] - t[k]).cross(c - t[k]).dot(*n) >= -1e-12)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box [0, a] × [0, b] × [0, l], its faces turned outward, the z ends `ends`.
    fn guide(a: f64, b: f64, l: f64, ends: Side) -> Inside {
        let mut s = Inside::default();
        let c = |x: f64, y: f64, z: f64| [x, y, z];
        // Walls, outward: y = 0 (−y), y = b (+y), x = 0 (−x), x = a (+x).
        s.quad([c(0.0, 0.0, 0.0), c(a, 0.0, 0.0), c(a, 0.0, l), c(0.0, 0.0, l)], Side::Wall);
        s.quad([c(0.0, b, 0.0), c(0.0, b, l), c(a, b, l), c(a, b, 0.0)], Side::Wall);
        s.quad([c(0.0, 0.0, 0.0), c(0.0, 0.0, l), c(0.0, b, l), c(0.0, b, 0.0)], Side::Wall);
        s.quad([c(a, 0.0, 0.0), c(a, b, 0.0), c(a, b, l), c(a, 0.0, l)], Side::Wall);
        // The ends.
        s.quad([c(0.0, 0.0, 0.0), c(0.0, b, 0.0), c(a, b, 0.0), c(a, 0.0, 0.0)], ends);
        s.quad([c(0.0, 0.0, l), c(a, 0.0, l), c(a, b, l), c(0.0, b, l)], ends);
        s
    }

    #[test]
    fn a_guide_gets_a_mitred_shell_flush_with_its_ports() {
        let (a, b, l, t) = (0.02286, 0.01016, 0.05, 0.00127);
        let s = shell(&guide(a, b, l, Side::Open), t);
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
        let area: f64 = s
            .rim
            .iter()
            .map(|p| {
                let [a, b, c] = p.map(Point3::from);
                0.5 * (b - a).cross(c - a).norm()
            })
            .sum();
        let frame = (a + 2.0 * t) * (b + 2.0 * t) - a * b;
        assert!((area - 2.0 * frame).abs() < 1e-6 * frame, "{area} against {}", 2.0 * frame);
    }

    #[test]
    fn a_closed_box_has_no_rim_and_nothing_breaks_on_nonsense() {
        let g = guide(1.0, 1.0, 1.0, Side::Wall);
        let s = shell(&g, 0.1);
        assert!(s.rim.is_empty() && s.outer.len() == 12);
        for t in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let s = shell(&g, t);
            assert!(s.outer.is_empty() && s.rim.is_empty());
        }
        assert!(shell(&Inside::default(), 0.1).outer.is_empty());
    }
}

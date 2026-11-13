//! Where the pointer lands on the model while a tool is on: the face under it (the nearest tagged
//! triangle, through the scene's index, and its face, [`Face`]) and what on it the pointer snaps
//! to — a corner, an edge's middle or the face's centre within [`SNAP_PX`], else an edge within
//! [`EDGE_PX`], else the face where it was hit. Points are on the view, from its corner.

use emcraft_model::geom::{Point3, Vec3};
use emcraft_render::view3d::{self, Camera, Pixel};

use super::port::{PortTarget, port_target};
use super::{Hover, Snap, SnapKind, Tool};
use crate::geom::{Point, Vector};
use crate::pick::Pick;
use crate::scene3d::Built;
use crate::scene3d::faces::Face;

/// Points from a snap point within which the pointer snaps to it.
pub const SNAP_PX: f32 = 10.0;
/// Points from an edge within which the pointer picks it.
pub const EDGE_PX: f32 = 5.0;
/// Most faces [`FaceCache`] keeps.
const CACHED_FACES: usize = 4;
/// Most triangles behind see-through faces a port's face is looked for among.
const BEHIND: usize = 32;

/// What the pointer looks at: the scene drawn (its generation), the camera and the view's size
/// (points).
pub struct Sight<'a> {
    pub built: &'a Built,
    pub generation: u64,
    pub camera: &'a Camera,
    pub size: Vector,
}

impl Sight<'_> {
    /// The pixel at `at` (points from the view's corner).
    fn pixel(&self, at: Point) -> Pixel {
        Pixel::new(f64::from(at.x), f64::from(at.y), f64::from(self.size.x), f64::from(self.size.y))
    }
}

/// Where `p` (m) lands on a view of `size` through `cam` (points from its corner), when in front.
///
/// ```
/// use emcraft_render::view3d::Camera;
/// use emcraft_ui_core::geom::Vector;
/// let c = Camera::default();
/// let q = emcraft_ui_core::tools::screen(&c, Vector::new(800.0, 600.0), c.target);
/// assert_eq!(q.map(|q| (q.x.round(), q.y.round())), Some((400.0, 300.0)));
/// ```
pub fn screen(cam: &Camera, size: Vector, p: [f64; 3]) -> Option<Point> {
    let q = cam.project(p, f64::from(size.x), f64::from(size.y))?;
    q.iter().all(|c| c.is_finite()).then(|| Point::new(q[0] as f32, q[1] as f32))
}

/// The distance from `p` to the segment `a`–`b` on screen, and how far along it.
fn to_segment(p: Point, a: Point, b: Point) -> (f32, f32) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (a.lerp(b, t).distance(p), t)
}

/// The point of segment `a`–`b` nearest the ray through the pointer (`t` on screen when the
/// ray runs along it).
fn on_segment(ray: Option<([f64; 3], [f64; 3])>, a: [f64; 3], b: [f64; 3], t: f32) -> [f64; 3] {
    let (a, b) = (Point3::from(a), Point3::from(b));
    let Some((o, d)) = ray else { return a.lerp(b, f64::from(t)).to_array() };
    let (u, w, d) = (b - a, a - Point3::from(o), Vec3::from(d));
    let (uu, ud, dd) = (u.dot(u), u.dot(d), d.dot(d));
    let den = uu * dd - ud * ud;
    if den.abs() <= 1e-12 * uu * dd {
        return a.lerp(b, f64::from(t)).to_array();
    }
    let s = ((ud * w.dot(d) - dd * w.dot(u)) / den).clamp(0.0, 1.0);
    a.lerp(b, s).to_array()
}

/// The point halfway along `a`–`b`.
fn halfway(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    Point3::from(a).lerp(Point3::from(b), 0.5).to_array()
}

/// The unit vector from `a` to `b`, if they differ.
fn direction(a: [f64; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    (Point3::from(b) - Point3::from(a)).normalized().map(Vec3::to_array)
}

/// What the pointer at `at` snaps to on `face`, hit at `hit`: a corner, an edge's middle or the
/// face's centre within [`SNAP_PX`], else an edge within [`EDGE_PX`], else the face where it
/// was hit (just off the face, past its outline, only its corners and edges).
pub fn snap(face: &Face, hit: Option<[f64; 3]>, cam: &Camera, size: Vector, at: Point) -> Option<Snap> {
    let points = face
        .corners
        .iter()
        .map(|c| (*c, SnapKind::Vertex, None))
        .chain(face.edges.iter().map(|(a, b)| (halfway(*a, *b), SnapKind::Midpoint, None)))
        .chain(std::iter::once((face.centre, SnapKind::Centre, Some(face.normal))));
    let near = points
        .filter_map(|(p, kind, dir)| screen(cam, size, p).map(|s| (s.distance(at), Snap { at: p, kind, dir })))
        .filter(|(d, _)| *d <= SNAP_PX)
        .fold(None, |best: Option<(f32, Snap)>, (d, s)| if best.as_ref().is_none_or(|(b, _)| d < *b) { Some((d, s)) } else { best });
    if let Some((_, s)) = near {
        return Some(s);
    }
    let ray = cam.ray(Pixel::new(f64::from(at.x), f64::from(at.y), f64::from(size.x), f64::from(size.y)));
    let edge = face
        .edges
        .iter()
        .filter_map(|(a, b)| {
            let (sa, sb) = (screen(cam, size, *a)?, screen(cam, size, *b)?);
            let (d, t) = to_segment(at, sa, sb);
            (d <= EDGE_PX).then(|| (d, Snap { at: on_segment(ray, *a, *b, t), kind: SnapKind::Edge, dir: direction(*a, *b) }))
        })
        .fold(None, |best: Option<(f32, Snap)>, (d, s)| if best.as_ref().is_none_or(|(b, _)| d < *b) { Some((d, s)) } else { best });
    edge.map(|(_, s)| s).or_else(|| hit.map(|at| Snap { at, kind: SnapKind::Face, dir: Some(face.normal) }))
}

/// The faces last found (the latest last), by the scene generation and triangle each was found
/// from: finding a face floods the scene's triangles, so the pointer moving over one face finds
/// it here again.
#[derive(Clone, Debug, Default)]
pub struct FaceCache(Vec<((u64, usize), Face)>);

impl FaceCache {
    /// The face of `built`'s triangle `index` in generation `now`, looked up or found.
    fn find(&mut self, built: &Built, now: u64, index: usize) -> Option<Face> {
        let key = (now, index);
        if let Some(i) = self.0.iter().position(|(k, _)| *k == key) {
            // The latest last.
            let found = self.0.remove(i);
            let f = found.1.clone();
            self.0.push(found);
            return Some(f);
        }
        let f = Face::of_indexed(&built.scene, built.cache.index(&built.scene), index)?;
        self.0.push((key, f.clone()));
        if self.0.len() > CACHED_FACES {
            self.0.remove(0);
        }
        Some(f)
    }

    /// The face last found, if in generation `now`.
    fn last(&self, now: u64) -> Option<&Face> {
        self.0.last().filter(|((g, _), _)| *g == now).map(|(_, f)| f)
    }
}

/// What the pointer at `at` is over for `tool` on model `m` seen in `sight`, the faces found
/// kept in `faces`.
pub fn over(m: &emcraft_model::Model, sight: &Sight, tool: Tool, at: Point, faces: &mut FaceCache) -> Option<Hover> {
    let (cam, size, now) = (sight.camera, sight.size, sight.generation);
    // Near a corner, an edge's middle or the centre of the face last under the pointer, the
    // snap holds (on a body's outline the pointer may as well hit what lies behind).
    if tool != Tool::Port
        && let Some(last) = faces.last(now)
        && let Some(s) = snap(last, None, cam, size, at).filter(|s| !matches!(s.kind, SnapKind::Edge | SnapKind::Face))
    {
        return Some(Hover { face: last.clone(), snap: s, port: None });
    }
    let built = sight.built;
    let hits = built.hits(cam, sight.pixel(at));
    let mut face_of = |hit: &view3d::Hit| faces.find(built, now, hit.index);
    // Points are picked through see-through walls, on what they show inside, as a click selects.
    // A port goes on the face in front when one may go there (a free end, a port's face), else
    // on what shows through it (a body's face inside an open box).
    let hit = match hits {
        None => None,
        Some(view3d::Hits { nearest: near, solid }) if tool == Tool::Port => {
            let air = |h: &view3d::Hit| m.domain.open.is_some() && Pick::from_tag(h.tag) == Some(Pick::Solid);
            let mut usable = |h: &view3d::Hit| !air(h) && face_of(h).is_some_and(|f| !matches!(port_target(m, &f), PortTarget::No(_)));
            if usable(&near) {
                Some(near)
            } else {
                // Behind see-through faces (a guide's sides in front of its far end): the first
                // face along the ray a port may go on, else what shows through.
                let behind = cam.ray(sight.pixel(at)).map_or_default(|r| view3d::hits_in_order(&built.scene, r, BEHIND));
                Some(behind.into_iter().find(|h| usable(h)).unwrap_or_else(|| solid.unwrap_or(near)))
            }
        }
        Some(h) => Some(h.picked()),
    };
    let face = match hit {
        Some(hit) => face_of(&hit)?,
        // Just off the face last under the pointer, its corners and edges still snap.
        None if tool != Tool::Port => faces.last(now)?.clone(),
        None => return None,
    };
    let s = snap(&face, hit.map(|h| h.at), cam, size, at)?;
    let port = (tool == Tool::Port).then(|| port_target(m, &face));
    Some(Hover { face, snap: s, port })
}

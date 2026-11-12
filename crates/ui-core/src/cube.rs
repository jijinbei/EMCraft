//! The view cube in the 3D view's corner, after SolidCraft's navigation cube
//! (`crates/ui-egui/src/navigation_cube.rs` and `assets/icons/ui/source/navigation.rs`, MIT OR
//! Apache-2.0; ATTRIBUTION.md), whatever window draws it: a unit cube with a camera of its own
//! (turned with the view's, its zoom and framing left out, foreshortened only in perspective),
//! each face cut into nine coplanar patches. A face's middle and its four side strips turn the
//! view to look at that face, its corner squares to look from that corner, so a click reaches
//! one of 14 views. Only the cube's own edges are outlined (the patches share a face without
//! seams); a face's middle carries its name, laid on the face. Around it: four arrows that turn
//! the view a quarter turn, Home and Fit, and a small triad of the axes. Points are on the
//! view, from the cube's middle; the design's units are scaled to [`SIZE`].

use emcraft_render::view3d::{Camera, Projection};

use crate::geom::{Point, Vector};
use crate::navigation::Goal;

/// The cube's square, in points, and the design's units across it.
pub const SIZE: f32 = 112.0;
pub const DESIGN: f32 = 164.0;
/// Half the cube's side, in design units.
pub const HALF: f32 = 40.0;
/// Where a face's corner squares begin (a share of the half side).
const INSET: f64 = 0.68;

/// The faces: the outward normal and the two axes across it its name reads along.
const FACES: [([f64; 3], [f64; 3], [f64; 3]); 6] = [
    ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
    ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
    ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
];

/// One patch as drawn: the direction it turns the view to look from, its corners on screen
/// (in order round it), which of its sides are the cube's own edges, its middle, its tilt toward
/// the eye (1 square on), and on a face's middle the face's name with the directions its text
/// runs along and down on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Facet {
    pub dir: [i8; 3],
    pub points: Vec<Point>,
    pub outer: Vec<bool>,
    pub centre: Point,
    pub depth: f64,
    pub label: Option<(&'static str, Vector, Vector)>,
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The perspective's strength: the inverse of the cube's own focal length (0 orthographic). The
/// field of view spans the whole design square, so the front face keeps its size beside the
/// arrows.
fn inverse_focal(cam: &Camera) -> f64 {
    let half_height = f64::from(DESIGN) / (2.0 * f64::from(HALF));
    match cam.projection {
        Projection::Perspective if cam.fov.is_finite() => (cam.fov.clamp(0.0, std::f64::consts::FRAC_PI_2) * 0.5).tan() / half_height,
        _ => 0.0,
    }
}

/// A point of the unit cube on screen, `scale` points a unit, from its middle.
fn project(cam: &Camera, v: [f64; 3], scale: f64) -> Vector {
    let b = cam.basis();
    let back = cam.back();
    let d = 1.0 + inverse_focal(cam) * (1.0 - dot(v, back));
    Vector::new((dot(v, b.right) * scale / d) as f32, (-dot(v, b.up) * scale / d) as f32)
}

/// How a direction runs on screen at the cube's point `at` (its local derivative there).
fn project_direction(cam: &Camera, at: [f64; 3], direction: [f64; 3]) -> Vector {
    let b = cam.basis();
    let back = cam.back();
    let f = inverse_focal(cam);
    let d = 1.0 + f * (1.0 - dot(at, back));
    let p = project(cam, at, 1.0);
    let k = (f * dot(direction, back)) as f32;
    let along = Vector::new(dot(direction, b.right) as f32 + p.x * k, -dot(direction, b.up) as f32 + p.y * k);
    Vector::new(along.x / d as f32, along.y / d as f32)
}

/// A face's name (as its label reads, looked up in the window's language).
pub fn face_name(d: [i8; 3]) -> &'static str {
    match d {
        [1, 0, 0] => "RIGHT",
        [-1, 0, 0] => "LEFT",
        [0, 1, 0] => "TOP",
        [0, -1, 0] => "BOTTOM",
        [0, 0, 1] => "FRONT",
        _ => "BACK",
    }
}

/// A patch's view in words: a face's name, or a corner's three (`TOP · FRONT · RIGHT`).
pub fn view_name(d: [i8; 3]) -> String {
    [1usize, 2, 0]
        .into_iter()
        .filter(|&k| d[k] != 0)
        .map(|k| {
            let mut e = [0i8; 3];
            e[k] = d[k];
            crate::lang::lookup(face_name(e))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The patches facing the eye through `cam`, far first (hit near first), the cube's middle at
/// `c`, `half` points from its middle to a face.
///
/// ```
/// use emcraft_render::view3d::{Camera, Projection, StandardView};
/// use emcraft_ui_core::cube::{facets, hit};
/// use emcraft_ui_core::geom::Point;
/// let mut c = Camera { projection: Projection::Orthographic, ..Camera::default() };
/// c.standard(StandardView::Front);
/// let faces = facets(&c, Point::ORIGIN, 40.0);
/// // Square on: one face, its nine patches, only its rim outlined.
/// assert_eq!(faces.len(), 9);
/// assert_eq!(faces.iter().flat_map(|f| f.outer.iter()).filter(|o| **o).count(), 12);
/// let front = hit(&faces, Point::ORIGIN).map(|i| faces[i].dir);
/// assert_eq!(front, Some([0, 0, 1]));
/// // A corner square looks from the corner.
/// assert_eq!(hit(&faces, Point::new(36.0, -36.0)).map(|i| faces[i].dir), Some([1, 1, 1]));
/// ```
pub fn facets(cam: &Camera, c: Point, half: f32) -> Vec<Facet> {
    let half = f64::from(half);
    let back = cam.back();
    let f = inverse_focal(cam);
    let eye = f / (1.0 + f);
    let bands = [(-1.0, -INSET, -1.0), (-INSET, INSET, 0.0), (INSET, 1.0, 1.0)];
    let mut out: Vec<Facet> = FACES
        .into_iter()
        .filter(|(n, _, _)| dot(*n, back) > eye + 1e-5)
        .flat_map(|(n, r, u)| {
            bands.into_iter().flat_map(move |(x0, x1, sx)| bands.into_iter().map(move |(y0, y1, sy)| (n, r, u, x0, x1, sx, y0, y1, sy)))
        })
        .map(|(n, r, u, x0, x1, sx, y0, y1, sy)| {
            let on = |x: f64, y: f64| [0, 1, 2].map(|i| n[i] + r[i] * x + u[i] * y);
            let corner = sx != 0.0 && sy != 0.0;
            let dir = if corner { on(sx, sy) } else { n };
            let middle3 = on(0.5 * (x0 + x1), 0.5 * (y0 + y1));
            let middle = project(cam, middle3, half);
            // Round the patch in its order on screen (a mirror would turn it about).
            let mut corners: Vec<[f64; 3]> = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].into_iter().map(|(x, y)| on(x, y)).collect();
            corners.sort_by(|a, b| {
                let (a, b) = (project(cam, *a, half), project(cam, *b, half));
                (a.y - middle.y).atan2(a.x - middle.x).total_cmp(&(b.y - middle.y).atan2(b.x - middle.x))
            });
            let outer = (0..4)
                .map(|j| {
                    let (a, b) = (corners[j], corners[(j + 1) % 4]);
                    (0..3).filter(|&k| a[k].abs() == 1.0 && a[k] == b[k]).count() >= 2
                })
                .collect();
            let label = (sx == 0.0 && sy == 0.0)
                .then(|| (face_name(n.map(|v| v as i8)), project_direction(cam, n, r), project_direction(cam, n, u.map(|v| -v))));
            Facet {
                dir: dir.map(|v| v.round().clamp(-1.0, 1.0) as i8),
                points: corners.iter().map(|q| c + project(cam, *q, half)).collect(),
                outer,
                centre: c + middle,
                depth: dot(n, back),
                label,
            }
        })
        .collect();
    out.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    out
}

/// Whether `p` lies inside the convex polygon `points`.
fn contains(points: &[Point], p: Point) -> bool {
    if !p.is_finite() || points.len() < 3 {
        return false;
    }
    let mut sign = 0.0f32;
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        if cross.abs() > 1e-3 {
            if sign != 0.0 && sign.signum() != cross.signum() {
                return false;
            }
            sign = cross;
        }
    }
    sign != 0.0
}

/// The nearest patch under `p`.
pub fn hit(faces: &[Facet], p: Point) -> Option<usize> {
    faces.iter().enumerate().rev().find(|(_, f)| contains(&f.points, p)).map(|(i, _)| i)
}

/// What a click on a patch looking from `dir` asks for.
pub fn goal(dir: [i8; 3]) -> Goal {
    Goal::From(dir.map(f64::from))
}

/// What a control round the cube is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// An arrow pointing along a direction on screen (unit).
    Arrow(Vector),
    Home,
    Fit,
}

/// A control round the cube: its name, what it is, where (design units from the cube's
/// middle), its size (design units), what it asks for and its tip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Control {
    pub name: &'static str,
    pub kind: Kind,
    pub at: Vector,
    pub size: f32,
    pub goal: Goal,
    pub tip: crate::lang::Text,
}

/// The controls round the cube: the four quarter turns (nearer when a face is square on),
/// Home and Fit.
pub fn controls(faces: &[Facet]) -> [Control; 6] {
    let square_on = faces.iter().filter(|f| f.label.is_some()).count() == 1;
    let (d, size) = if square_on { (54.0, 20.0) } else { (74.0, 16.0) };
    let turn = |right: f64, up: f64| Goal::Turn { right, up, degrees: 90.0 };
    let arrow = |name, at: Vector, dir: Vector, goal, tip| Control { name, kind: Kind::Arrow(dir), at, size, goal, tip };
    [
        arrow("left", Vector::new(-d, 0.0), Vector::new(1.0, 0.0), turn(-1.0, 0.0), t!("Orbit left 90°")),
        arrow("right", Vector::new(d, 0.0), Vector::new(-1.0, 0.0), turn(1.0, 0.0), t!("Orbit right 90°")),
        arrow("up", Vector::new(0.0, -d), Vector::new(0.0, 1.0), turn(0.0, 1.0), t!("Orbit up 90°")),
        arrow("down", Vector::new(0.0, d), Vector::new(0.0, -1.0), turn(0.0, -1.0), t!("Orbit down 90°")),
        Control { name: "home", kind: Kind::Home, at: Vector::new(-65.0, -65.0), size: 26.0, goal: Goal::Home, tip: t!("Home view (isometric)") },
        Control {
            name: "fit",
            kind: Kind::Fit,
            at: Vector::new(67.0, 67.0),
            size: 22.0,
            goal: Goal::Fit,
            tip: t!("Fit the model to the view (double-click the view)"),
        },
    ]
}

/// The control under `p` (design units from the cube's middle).
pub fn control_at(controls: &[Control], p: Vector) -> Option<Control> {
    controls.iter().find(|c| (p.x - c.at.x).abs() <= c.size / 2.0 && (p.y - c.at.y).abs() <= c.size / 2.0).copied()
}

/// The axes' triad below the cube: where it starts (design units from the cube's middle) and
/// each axis's tip from there (design units), x, y, z.
pub fn triad(cam: &Camera) -> (Vector, [Vector; 3]) {
    let b = cam.basis();
    let tip = |k: usize| Vector::new((b.right[k] * 17.0) as f32, (-b.up[k] * 17.0) as f32);
    (Vector::new(-60.0, 52.0), [tip(0), tip(1), tip(2)])
}

/// A label's opacity on a face tilted `depth` toward the eye: none edge-on, full past a third.
pub fn label_opacity(depth: f64) -> f32 {
    ((depth - 0.18) / 0.17).clamp(0.0, 1.0) as f32
}

/// A patch's shade: its face's colour darkened as it turns away (1 square on).
pub fn shade(depth: f64) -> f32 {
    (0.72 + 0.28 * depth.clamp(0.0, 1.0)) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_render::view3d::StandardView;

    /// Every patch picks its own middle in every standard view, in both projections; 14 views in
    /// all, and nothing outside the cube.
    #[test]
    fn patches_pick_their_middles_and_reach_fourteen_views() {
        let mut dirs: Vec<[i8; 3]> = Vec::new();
        for projection in [Projection::Perspective, Projection::Orthographic] {
            for v in StandardView::ALL {
                let mut cam = Camera { projection, ..Camera::default() };
                cam.standard(v);
                let faces = facets(&cam, Point::ORIGIN, HALF);
                assert!(!faces.is_empty());
                for (i, f) in faces.iter().enumerate() {
                    assert_eq!(hit(&faces, f.centre), Some(i), "{v:?} {projection:?} {:?}", f.dir);
                    let axes = f.dir.iter().filter(|d| **d != 0).count();
                    assert!(axes == 1 || axes == 3, "edges are no targets: {:?}", f.dir);
                    if !dirs.contains(&f.dir) {
                        dirs.push(f.dir);
                    }
                }
                assert!(hit(&faces, Point::new(80.0, 80.0)).is_none());
            }
        }
        assert_eq!(dirs.len(), 14);
        // In perspective the far edge is drawn smaller than the near one.
        let mut cam = Camera::default();
        cam.standard(StandardView::Front);
        let (near, far) = (project(&cam, [0.5, 0.0, 1.0], 40.0), project(&cam, [0.5, 0.0, -1.0], 40.0));
        assert!(far.x < near.x && (near.x - 20.0).abs() < 1e-4);
    }

    #[test]
    fn controls_and_names() {
        let faces = facets(&Camera::default(), Point::ORIGIN, HALF);
        let c = controls(&faces);
        assert_eq!(control_at(&c, Vector::new(-65.0, -65.0)).map(|c| c.goal), Some(Goal::Home));
        assert_eq!(control_at(&c, Vector::new(0.0, 0.0)), None);
        assert_eq!(view_name([1, 1, 1]), "TOP · FRONT · RIGHT");
        assert_eq!(faces.iter().filter(|f| f.label.is_some()).count(), 3, "the isometric view shows three faces' names");
    }
}

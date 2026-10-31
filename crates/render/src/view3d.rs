//! The 3D view's scene and camera, without drawing it: triangles (colour, opacity, tag) and
//! lines, an orbit camera (yaw, pitch, distance and target; perspective or orthographic) with its
//! projection and rays, picking by ray (Möller–Trumbore, through the scene's bounding-volume
//! hierarchy, [`bvh`]), the view changes ([`navigate`]), the coarser copy drawn while moving
//! ([`simplify`]) and the section view ([`section::section`]: the scene cut by a plane, the far
//! side removed and the cut bodies capped). The window's GPU draws it (`emcraft-ui-egui`'s
//! `gpu3d`, docs/adr/0004-gpu-viewport.md).
//!
//! Deterministic: one thread, a fixed order, the same input gives the same scene and picks.

use crate::Rgb;

pub mod bvh;
pub mod navigate;
pub mod section;
pub mod simplify;

pub use bvh::SceneIndex;
pub use navigate::{CameraAnim, Drag, cube_directions, ease_in_out};
pub use section::{Keep, Section, SectionError};

/// Most triangles or lines drawn from one scene (the rest are left out).
pub const MAX_PRIMITIVES: usize = 4_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    Perspective,
    Orthographic,
}

emcraft_macros::named_enum! {
    /// The standard views. The vertical is +y; the front view looks from +z.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum StandardView {
        Iso = "iso",
        Front = "front",
        Back = "back",
        Top = "top",
        Bottom = "bottom",
        Left = "left",
        Right = "right",
    }
}

impl StandardView {
    /// (yaw, pitch) in radians.
    pub fn angles(self) -> (f64, f64) {
        use std::f64::consts::{FRAC_PI_2, PI};
        match self {
            StandardView::Front => (0.0, 0.0),
            StandardView::Back => (PI, 0.0),
            StandardView::Top => (0.0, FRAC_PI_2),
            StandardView::Bottom => (0.0, -FRAC_PI_2),
            StandardView::Left => (-FRAC_PI_2, 0.0),
            StandardView::Right => (FRAC_PI_2, 0.0),
            // Down the body diagonal: 45° round, arctan(1/√2) up.
            StandardView::Iso => (std::f64::consts::FRAC_PI_4, (0.5f64.sqrt()).atan()),
        }
    }
}

/// An orbit camera. The eye sits `distance` from `target` in the direction given by `yaw`
/// (about +y, 0 on +z, π/2 on +x) and `pitch` (up from the horizontal plane).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub target: [f64; 3],
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
    /// The vertical field of view (radians); under orthographic projection it sets the visible
    /// height at the target: 2 · distance · tan(fov / 2).
    pub fov: f64,
    pub projection: Projection,
}

impl Default for Camera {
    fn default() -> Self {
        let (yaw, pitch) = StandardView::Iso.angles();
        Camera { target: [0.0; 3], yaw, pitch, distance: 1.0, fov: 30f64.to_radians(), projection: Projection::Perspective }
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalized(a: [f64; 3]) -> Option<[f64; 3]> {
    let n = dot(a, a).sqrt();
    (n > 0.0 && n.is_finite()).then(|| [a[0] / n, a[1] / n, a[2] / n])
}

/// The camera's frame: eye, right, up and forward (unit vectors).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Basis {
    pub eye: [f64; 3],
    pub right: [f64; 3],
    pub up: [f64; 3],
    pub forward: [f64; 3],
}

impl Camera {
    /// The camera with every non-finite or out-of-range number replaced by a usable one.
    pub fn sanitized(self) -> Camera {
        let d = Camera::default();
        let fin = |v: f64, alt: f64| if v.is_finite() { v } else { alt };
        let target = if self.target.iter().all(|v| v.is_finite() && v.abs() < 1e12) { self.target } else { d.target };
        let distance = if self.distance.is_finite() && self.distance > 0.0 { self.distance.clamp(1e-12, 1e12) } else { d.distance };
        Camera {
            target,
            yaw: fin(self.yaw, d.yaw).rem_euclid(std::f64::consts::TAU),
            pitch: fin(self.pitch, d.pitch).clamp(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2),
            distance,
            fov: fin(self.fov, d.fov).clamp(1f64.to_radians(), 120f64.to_radians()),
            projection: self.projection,
        }
    }

    pub fn basis(&self) -> Basis {
        let c = self.sanitized();
        let (sy, cy) = c.yaw.sin_cos();
        let (sp, cp) = c.pitch.sin_cos();
        let back = [cp * sy, sp, cp * cy];
        let eye = [c.target[0] + c.distance * back[0], c.target[1] + c.distance * back[1], c.target[2] + c.distance * back[2]];
        let forward = [-back[0], -back[1], -back[2]];
        let right = [cy, 0.0, -sy];
        let up = cross(right, forward);
        Basis { eye, right, up, forward }
    }

    /// Looks at `v` (keeping target and distance).
    pub fn standard(&mut self, v: StandardView) {
        (self.yaw, self.pitch) = v.angles();
    }

    /// Frames the box `lo`–`hi` in a picture of `aspect` = width / height, keeping the direction.
    pub fn fit(&mut self, lo: [f64; 3], hi: [f64; 3], aspect: f64) {
        if !lo.iter().chain(&hi).all(|v| v.is_finite()) {
            return;
        }
        self.target = [0.5 * (lo[0] + hi[0]), 0.5 * (lo[1] + hi[1]), 0.5 * (lo[2] + hi[2])];
        let r = (0.5 * dot(sub(hi, lo), sub(hi, lo)).sqrt()).max(1e-9);
        let c = self.sanitized();
        let half = (0.5 * c.fov).tan();
        // The narrower of the two half-angles holds the sphere round the box.
        let aspect = if aspect.is_finite() && aspect > 0.0 { aspect.clamp(0.05, 20.0) } else { 1.0 };
        let half = half * aspect.min(1.0);
        self.distance = match c.projection {
            Projection::Perspective => 1.08 * r / half.atan().sin(),
            Projection::Orthographic => 1.08 * r / half,
        };
    }

    /// Turns the view by screen pixels dragged (a full drag across ~600 px is about a half turn).
    pub fn orbit(&mut self, dx: f64, dy: f64) {
        if !(dx.is_finite() && dy.is_finite()) {
            return;
        }
        self.yaw = (self.yaw - dx * 0.008).rem_euclid(std::f64::consts::TAU);
        self.pitch = (self.pitch + dy * 0.008).clamp(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
    }

    /// The world length one pixel spans at the target, in a picture `height` pixels high.
    pub fn pixel_size(&self, height: f64) -> f64 {
        let c = self.sanitized();
        2.0 * c.distance * (0.5 * c.fov).tan() / height.max(1.0)
    }

    /// Moves the target with the picture by screen pixels dragged.
    pub fn pan(&mut self, dx: f64, dy: f64, height: f64) {
        if !(dx.is_finite() && dy.is_finite()) {
            return;
        }
        let b = self.basis();
        let s = self.pixel_size(height);
        for k in 0..3 {
            self.target[k] += (-dx * b.right[k] + dy * b.up[k]) * s;
        }
    }

    /// Zooms by `steps` (positive closer); each step is 10 %.
    pub fn zoom(&mut self, steps: f64) {
        if steps.is_finite() {
            self.distance = (self.distance * (-0.1 * steps.clamp(-100.0, 100.0)).exp()).clamp(1e-12, 1e12);
        }
    }

    /// Where `p` lands in a `w` × `h` picture: x, y (pixels, y down) and the depth key (larger is
    /// nearer); `None` behind the eye.
    pub fn project(&self, p: [f64; 3], w: f64, h: f64) -> Option<[f64; 3]> {
        Projector::new(self, w, h).project(p, 0.0)
    }

    /// The ray through pixel (x, y) of a w × h picture: its origin and unit direction.
    pub fn ray(&self, x: f64, y: f64, w: f64, h: f64) -> Option<([f64; 3], [f64; 3])> {
        let pr = Projector::new(self, w, h);
        let (u, v) = ((x - pr.cx) / pr.scale, (pr.cy - y) / pr.scale);
        let b = pr.b;
        let along = |o: [f64; 3], d: [f64; 3], k: f64| [o[0] + d[0] * k, o[1] + d[1] * k, o[2] + d[2] * k];
        let (origin, dir) =
            if pr.persp { (b.eye, along(along(b.forward, b.right, u), b.up, v)) } else { (along(along(b.eye, b.right, u), b.up, v), b.forward) };
        let len = dot(dir, dir).sqrt();
        (len > 0.0 && origin.iter().chain(&dir).all(|c| c.is_finite())).then(|| (origin, dir.map(|c| c / len)))
    }
}

impl Camera {
    /// Where the ray through pixel (x, y) of a w × h picture meets the plane through the target
    /// facing the camera: what a zoom over empty space keeps under the cursor.
    ///
    /// ```
    /// use emcraft_render::view3d::Camera;
    /// let c = Camera { target: [1.0, 2.0, 3.0], ..Camera::default() };
    /// let p = c.on_target_plane(50.0, 50.0, 100.0, 100.0).unwrap();
    /// assert!(p.iter().zip(c.target).all(|(a, b)| (a - b).abs() < 1e-9));
    /// ```
    pub fn on_target_plane(&self, x: f64, y: f64, w: f64, h: f64) -> Option<[f64; 3]> {
        let (o, d) = self.ray(x, y, w, h)?;
        let n = self.basis().forward;
        let den = dot(d, n);
        if den.abs() < 1e-9 {
            return None;
        }
        let t = dot(sub(self.sanitized().target, o), n) / den;
        let p = [0, 1, 2].map(|i| o[i] + d[i] * t);
        p.iter().all(|v| v.is_finite()).then_some(p)
    }
}

/// Where a ray meets a triangle: the distance along it (Möller and Trumbore, *Fast, minimum
/// storage ray–triangle intersection*, J. Graphics Tools 2(1), 1997).
fn hit(origin: [f64; 3], dir: [f64; 3], t: &Tri) -> Option<f64> {
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let (e1, e2) = (sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0]));
    let p = cross(dir, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-300 {
        return None;
    }
    let s = sub(origin, t.p[0]);
    let u = dot(s, p) / det;
    let q = cross(s, e1);
    let v = dot(dir, q) / det;
    let d = dot(e2, q) / det;
    ((0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0 && d > 0.0).then_some(d)
}

/// A tagged triangle a ray through a pixel meets: its index in the scene, where (m) and its tag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub index: usize,
    pub at: [f64; 3],
    pub tag: u32,
}

/// The tagged triangles under pixel (x, y): the nearest the ray through it meets, and the
/// nearest of the solid ones (opacity ≥ 200).
///
/// ```
/// use emcraft_render::view3d::{Camera, Scene, StandardView, hits};
/// use emcraft_render::Rgb;
/// let mut s = Scene::default();
/// s.tagged(7, |s| s.quad([[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]], Rgb(200, 200, 200), 255));
/// let mut cam = Camera::default();
/// cam.standard(StandardView::Front);
/// cam.fit([-1.0, -1.0, 0.0], [1.0, 1.0, 0.0], 1.0);
/// // The middle pixel of a 100 × 100 picture looks at the quad's middle.
/// let (near, solid) = hits(&s, &cam, 50.0, 50.0, 100.0, 100.0).unwrap();
/// assert_eq!((near.tag, solid.map(|h| h.tag)), (7, Some(7)));
/// assert!(near.at.iter().all(|c| c.abs() < 1e-9));
/// // Off the quad, nothing.
/// assert!(hits(&s, &cam, 1.0, 1.0, 100.0, 100.0).is_none());
/// ```
pub fn hits(scene: &Scene, camera: &Camera, x: f64, y: f64, w: f64, h: f64) -> Option<(Hit, Option<Hit>)> {
    let (origin, dir) = camera.ray(x, y, w, h)?;
    let mut solid: Option<(f64, usize)> = None;
    let mut any: Option<(f64, usize)> = None;
    for (i, t) in scene.tris.iter().enumerate().filter(|(_, t)| t.tag != 0) {
        let Some(d) = hit(origin, dir, t) else { continue };
        let keep = |best: &mut Option<(f64, usize)>| {
            if best.is_none_or(|(b, _)| d < b) {
                *best = Some((d, i));
            }
        };
        keep(&mut any);
        if t.alpha >= 200 {
            keep(&mut solid);
        }
    }
    let at = |(d, i): (f64, usize)| {
        let tag = scene.tris.get(i).map_or(0, |t| t.tag);
        Hit { index: i, at: [origin[0] + dir[0] * d, origin[1] + dir[1] * d, origin[2] + dir[2] * d], tag }
    };
    any.map(|a| (at(a), solid.map(at)))
}

/// The point of the scene under pixel (x, y), for zooming toward it or orbiting about it: where
/// the ray through it meets the nearest solid triangle (opacity ≥ 200), else the nearest
/// see-through one, whatever its tag; `None` over empty space.
///
/// ```
/// use emcraft_render::view3d::{Camera, Scene, StandardView, surface_at};
/// use emcraft_render::Rgb;
/// let mut s = Scene::default();
/// s.quad([[-1.0, -1.0, 0.5], [1.0, -1.0, 0.5], [1.0, 1.0, 0.5], [-1.0, 1.0, 0.5]], Rgb(9, 9, 9), 255);
/// let mut cam = Camera::default();
/// cam.standard(StandardView::Front);
/// cam.fit([-1.0, -1.0, 0.0], [1.0, 1.0, 1.0], 1.0);
/// let p = surface_at(&s, &cam, 50.0, 50.0, 100.0, 100.0).unwrap();
/// assert!((p[2] - 0.5).abs() < 1e-9);
/// assert!(surface_at(&s, &cam, 0.0, 0.0, 100.0, 100.0).is_none());
/// ```
pub fn surface_at(scene: &Scene, camera: &Camera, x: f64, y: f64, w: f64, h: f64) -> Option<[f64; 3]> {
    let (origin, dir) = camera.ray(x, y, w, h)?;
    let (mut solid, mut any): (Option<f64>, Option<f64>) = (None, None);
    for t in &scene.tris {
        let Some(d) = hit(origin, dir, t) else { continue };
        let keep = |best: &mut Option<f64>| {
            if best.is_none_or(|b| d < b) {
                *best = Some(d);
            }
        };
        keep(&mut any);
        if t.alpha >= 200 {
            keep(&mut solid);
        }
    }
    solid.or(any).map(|d| [0, 1, 2].map(|i| origin[i] + dir[i] * d)).filter(|p| p.iter().all(|v| v.is_finite()))
}

/// The tag under pixel (x, y): the nearest tagged triangle the ray through it meets, the
/// nearer of the solid ones (opacity ≥ 200) if any, so see-through walls let what is inside be
/// picked.
pub fn pick(scene: &Scene, camera: &Camera, x: f64, y: f64, w: f64, h: f64) -> Option<u32> {
    hits(scene, camera, x, y, w, h).map(|(any, solid)| solid.unwrap_or(any).tag)
}

/// A camera's projection onto one picture size.
struct Projector {
    b: Basis,
    persp: bool,
    scale: f64,
    near: f64,
    cx: f64,
    cy: f64,
}

impl Projector {
    fn new(c: &Camera, w: f64, h: f64) -> Projector {
        let c = c.sanitized();
        let half = (0.5 * c.fov).tan();
        let persp = c.projection == Projection::Perspective;
        let scale = if persp { 0.5 * h / half } else { 0.5 * h / (c.distance * half) };
        Projector { b: c.basis(), persp, scale, near: c.distance * 1e-3, cx: 0.5 * w, cy: 0.5 * h }
    }

    /// Screen x, y and depth key; `nudge` moves the point towards the eye (world length).
    fn project(&self, p: [f64; 3], nudge: f64) -> Option<[f64; 3]> {
        let v = sub(p, self.b.eye);
        let (x, y, z) = (dot(v, self.b.right), dot(v, self.b.up), dot(v, self.b.forward) - nudge);
        if !(x.is_finite() && y.is_finite() && z.is_finite()) {
            return None;
        }
        if self.persp {
            if z < self.near {
                return None;
            }
            Some([self.cx + x * self.scale / z, self.cy - y * self.scale / z, 1.0 / z])
        } else {
            Some([self.cx + x * self.scale, self.cy - y * self.scale, -z])
        }
    }
}

/// A triangle: corners (world), colour and opacity (255 opaque), and the tag of what it
/// belongs to (0 for nothing that can be picked).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tri {
    pub p: [[f64; 3]; 3],
    pub color: Rgb,
    pub alpha: u8,
    pub tag: u32,
}

/// A line segment (world) with its colour and opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub color: Rgb,
    pub alpha: u8,
}

/// What is drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub tris: Vec<Tri>,
    pub lines: Vec<Line>,
    /// The tag the triangles added now get ([`Scene::tagged`]).
    pub tag: u32,
}

impl Scene {
    pub fn tri(&mut self, a: [f64; 3], b: [f64; 3], c: [f64; 3], color: Rgb, alpha: u8) {
        self.tris.push(Tri { p: [a, b, c], color, alpha, tag: self.tag });
    }

    /// Tags what `draw` adds (for picking), then goes back to the tag before.
    pub fn tagged(&mut self, tag: u32, draw: impl FnOnce(&mut Scene)) {
        let before = std::mem::replace(&mut self.tag, tag);
        draw(self);
        self.tag = before;
    }

    /// A planar quad a b c d (in order round it) as two triangles.
    pub fn quad(&mut self, q: [[f64; 3]; 4], color: Rgb, alpha: u8) {
        self.tri(q[0], q[1], q[2], color, alpha);
        self.tri(q[0], q[2], q[3], color, alpha);
    }

    pub fn line(&mut self, a: [f64; 3], b: [f64; 3], color: Rgb, alpha: u8) {
        self.lines.push(Line { a, b, color, alpha });
    }

    /// The six faces of the axis-aligned box `lo`–`hi` (a closed surface, two triangles a
    /// face, wound outward when `lo` < `hi`), no edges.
    pub fn cuboid(&mut self, lo: [f64; 3], hi: [f64; 3], color: Rgb, alpha: u8) {
        for k in 0..3 {
            let (i, j) = ((k + 1) % 3, (k + 2) % 3);
            for (side, out) in [(lo[k], false), (hi[k], true)] {
                let at = |a: f64, b: f64| {
                    let mut p = [side; 3];
                    (p[i], p[j]) = (a, b);
                    p
                };
                let (b, d) = ([hi[i], lo[j]], [lo[i], hi[j]]);
                let (b, d) = if out { (b, d) } else { (d, b) };
                self.quad([at(lo[i], lo[j]), at(b[0], b[1]), at(hi[i], hi[j]), at(d[0], d[1])], color, alpha);
            }
        }
    }

    /// The bounding box of everything drawn (finite points only).
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let pts = self.tris.iter().flat_map(|t| t.p).chain(self.lines.iter().flat_map(|l| [l.a, l.b]));
        let mut out: Option<([f64; 3], [f64; 3])> = None;
        for p in pts.filter(|p| p.iter().all(|v| v.is_finite())) {
            let (lo, hi) = out.get_or_insert((p, p));
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        out
    }
}

/// The three axes' directions on screen (x right, y down; unit length for an axis across the
/// picture, shorter for one pointing at the eye), for an orientation indicator.
pub fn axes_on_screen(camera: &Camera) -> [[f64; 2]; 3] {
    let b = camera.basis();
    [0, 1, 2].map(|k| [b.right[k], -b.up[k]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picking_finds_the_nearest_solid_tag_under_the_pixel() {
        // Two unit squares facing the camera along z, one behind the other, and a see-through
        // one in front of both.
        let mut s = Scene::default();
        let square = |s: &mut Scene, z: f64, alpha: u8| s.quad([[-1.0, -1.0, z], [1.0, -1.0, z], [1.0, 1.0, z], [-1.0, 1.0, z]], Rgb(1, 2, 3), alpha);
        s.tagged(1, |s| square(s, 0.0, 255));
        s.tagged(2, |s| square(s, 1.0, 255));
        s.tagged(3, |s| square(s, 2.0, 40));
        s.quad([[-9.0, -9.0, 3.0], [9.0, -9.0, 3.0], [9.0, 9.0, 3.0], [-9.0, 9.0, 3.0]], Rgb(0, 0, 0), 255);
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let mut c = Camera { projection, ..Camera::default() };
            c.standard(StandardView::Front);
            c.fit([-1.0, -1.0, 0.0], [1.0, 1.0, 2.0], 1.0);
            let b = c.basis();
            // The camera looks along its forward; the square nearer the eye is the solid hit.
            let nearer = if b.forward[2] < 0.0 { 2 } else { 1 };
            assert_eq!(pick(&s, &c, 50.0, 50.0, 100.0, 100.0), Some(nearer), "{projection:?}");
            // Off the squares: nothing tagged.
            assert_eq!(pick(&s, &c, 1.0, 1.0, 100.0, 100.0), None, "{projection:?}");
            // The ray through the centre passes the target.
            let (o, d) = c.ray(50.0, 50.0, 100.0, 100.0).unwrap();
            let to = sub(c.target, o);
            let off = sub(to, d.map(|x| x * dot(to, d)));
            assert!(dot(off, off).sqrt() < 1e-9);
        }
        assert!(Camera::default().ray(f64::NAN, 0.0, 10.0, 10.0).is_none());
    }

    fn front(projection: Projection) -> Camera {
        Camera { target: [0.0; 3], yaw: 0.0, pitch: 0.0, distance: 10.0, fov: 30f64.to_radians(), projection }
    }

    #[test]
    fn the_target_projects_to_the_centre_and_a_known_point_where_expected() {
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let c = front(projection);
            let p = c.project([0.0, 0.0, 0.0], 200.0, 100.0).unwrap();
            assert!((p[0] - 100.0).abs() < 1e-9 && (p[1] - 50.0).abs() < 1e-9);
            // A point at the top of the view at the target's depth: tan(15°) · 10 up.
            let top = 10.0 * (15f64.to_radians()).tan();
            let q = c.project([0.0, top, 0.0], 200.0, 100.0).unwrap();
            assert!((q[0] - 100.0).abs() < 1e-9 && q[1].abs() < 1e-9, "{projection:?}: {q:?}");
            // +x is to the right in the front view.
            assert!(c.project([1.0, 0.0, 0.0], 200.0, 100.0).unwrap()[0] > 100.0);
        }
        // Behind the eye is not drawn.
        assert!(front(Projection::Perspective).project([0.0, 0.0, 20.0], 10.0, 10.0).is_none());
        // The top view looks down y with -z up the picture.
        let mut c = front(Projection::Orthographic);
        c.standard(StandardView::Top);
        let q = c.project([0.0, 0.0, -1.0], 100.0, 100.0).unwrap();
        assert!(q[1] < 50.0 && (q[0] - 50.0).abs() < 1e-9);
    }

    #[test]
    fn fit_frames_the_box() {
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let mut c = front(projection);
            c.standard(StandardView::Iso);
            c.fit([0.0, 0.0, 0.0], [0.02, 0.01, 0.05], 1.5);
            for corner in 0..8 {
                let p = [0.02 * (corner & 1) as f64, 0.01 * ((corner >> 1) & 1) as f64, 0.05 * ((corner >> 2) & 1) as f64];
                let q = c.project(p, 150.0, 100.0).unwrap();
                assert!(q[0] > 0.0 && q[0] < 150.0 && q[1] > 0.0 && q[1] < 100.0, "{projection:?} corner {corner}: {q:?}");
            }
        }
    }
}

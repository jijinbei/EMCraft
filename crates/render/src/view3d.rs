//! 3D pictures without a GPU: flat-shaded triangles (Lambert, lit from beside the eye, both
//! sides) and lines with a depth buffer, seen through an orbit camera (yaw, pitch, distance and
//! target; perspective or orthographic). The desktop app shows the same picture as a texture, so
//! headless renders (`ui.render`, the scenarios) and the window agree.
//!
//! The method is the classic scan-conversion pipeline (see e.g. Foley, van Dam, Feiner and
//! Hughes, *Computer Graphics: Principles and Practice*, ch. 3 and 15; Pineda, "A parallel
//! algorithm for polygon rasterization", SIGGRAPH 1988, for the edge functions): vertices to
//! camera space, the projection, edge-function coverage at pixel centres, a depth buffer of
//! inverse depth (linear in screen space under perspective). Opaque triangles first, then
//! translucent triangles and lines together sorted back to front (lines nudged towards the eye so
//! edges on a face show), blended without writing depth — the order the GPU path's `flatten`
//! draws them in, so a still picture and a dragged one show the same edges.
//!
//! Deterministic: one thread, a fixed order, the same input gives the same bytes.
//!
//! A section view (the scene cut by a plane, the far side removed and the cut bodies capped) is
//! a scene too: [`section::section`] makes it, and both renderers draw it as any other.

use crate::{Canvas, Rgb};

pub mod bvh;
pub mod navigate;
pub mod section;
pub mod simplify;

pub use bvh::SceneIndex;
pub use navigate::{CameraAnim, Drag, cube_directions, ease_in_out};
pub use section::{Keep, Section, SectionError};

/// The largest picture side, in pixels.
pub const MAX_SIDE: usize = 4096;
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

/// How a picture is made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub width: usize,
    pub height: usize,
    /// Samples per pixel along each side (1–3): 2 smooths edges at four times the work.
    pub supersample: usize,
    /// The ground's colour at the top and at the bottom.
    pub background: (Rgb, Rgb),
    /// Line width in output pixels.
    pub line_width: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options { width: 800, height: 600, supersample: 1, background: (Rgb::WHITE, Rgb::WHITE), line_width: 1.0 }
    }
}

/// The working buffers.
struct Target {
    w: usize,
    h: usize,
    color: Vec<[f32; 3]>,
    depth: Vec<f32>,
}

impl Target {
    fn fill(&mut self, x: usize, y: usize, key: f32, c: [f32; 3], alpha: f32, write: bool) {
        let i = y * self.w + x;
        let (Some(d), Some(px)) = (self.depth.get_mut(i), self.color.get_mut(i)) else { return };
        if key < *d {
            return;
        }
        if write {
            *d = key;
        }
        if alpha >= 1.0 {
            *px = c;
        } else {
            for k in 0..3 {
                px[k] += (c[k] - px[k]) * alpha;
            }
        }
    }
}

fn rgbf(c: Rgb) -> [f32; 3] {
    [f32::from(c.0), f32::from(c.1), f32::from(c.2)]
}

/// The shade of a triangle: ambient plus Lambert from a light just above and right of the eye,
/// both sides lit alike.
fn shade(t: &Tri, light: [f64; 3]) -> Option<[f32; 3]> {
    let n = normalized(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])))?;
    let k = (0.42 + 0.58 * dot(n, light).abs()) as f32;
    let c = rgbf(t.color);
    Some([c[0] * k, c[1] * k, c[2] * k])
}

/// Fills a screen triangle: `s` are (x, y, key) corners.
fn raster_tri(t: &mut Target, s: [[f64; 3]; 3], c: [f32; 3], alpha: f32, write: bool) {
    let area = (s[1][0] - s[0][0]) * (s[2][1] - s[0][1]) - (s[2][0] - s[0][0]) * (s[1][1] - s[0][1]);
    if !(area.abs() > 1e-12) || !area.is_finite() {
        return;
    }
    let lim = 1e7;
    if s.iter().any(|p| p[0].abs() > lim || p[1].abs() > lim) {
        return;
    }
    let xs = [s[0][0], s[1][0], s[2][0]];
    let ys = [s[0][1], s[1][1], s[2][1]];
    let x0 = xs.iter().copied().fold(f64::INFINITY, f64::min).floor().max(0.0) as usize;
    let x1 = (xs.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil().min(t.w as f64)) as usize;
    let y0 = ys.iter().copied().fold(f64::INFINITY, f64::min).floor().max(0.0) as usize;
    let y1 = (ys.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil().min(t.h as f64)) as usize;
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let (inv, sign) = (1.0 / area.abs(), area.signum());
    // Edge functions e_k(x, y) = a x + b y + c over the edge opposite corner k, times the
    // orientation: positive inside, their sum the area. The same edge of a neighbour gives
    // exactly the negated value, so a pixel centre on a shared edge goes to the one triangle the
    // top-left rule picks.
    let edge = |i: usize, j: usize| {
        let (a, b) = (s[i][1] - s[j][1], s[j][0] - s[i][0]);
        let c = s[i][0] * s[j][1] - s[j][0] * s[i][1];
        (a * sign, b * sign, c * sign)
    };
    let e = [edge(1, 2), edge(2, 0), edge(0, 1)];
    let owns = e.map(|(a, b, _)| a > 0.0 || (a == 0.0 && b > 0.0));
    let z = [s[0][2], s[1][2], s[2][2]];
    for py in y0..y1 {
        let y = py as f64 + 0.5;
        for px in x0..x1 {
            let x = px as f64 + 0.5;
            let w = [e[0].0 * x + e[0].1 * y + e[0].2, e[1].0 * x + e[1].1 * y + e[1].2, e[2].0 * x + e[2].1 * y + e[2].2];
            if (0..3).any(|k| w[k] < 0.0 || (w[k] == 0.0 && !owns[k])) {
                continue;
            }
            let key = ((w[0] * z[0] + w[1] * z[1] + w[2] * z[2]) * inv) as f32;
            t.fill(px, py, key, c, alpha, write);
        }
    }
}

/// Draws a screen segment `width` pixels wide, depth-tested.
fn raster_line(t: &mut Target, a: [f64; 3], b: [f64; 3], width: f64, c: [f32; 3], alpha: f32) {
    let lim = 1e6;
    if [a[0], a[1], b[0], b[1]].iter().any(|v| !(v.abs() < lim)) {
        return;
    }
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let n = dx.abs().max(dy.abs()).ceil().clamp(1.0, 4.0 * MAX_SIDE as f64) as usize;
    let r = (0.5 * width).max(0.5);
    let (lo, hi) = ((0.5 - r).round() as i64, (r - 0.5).round() as i64);
    let steep = dy.abs() > dx.abs();
    for k in 0..=n {
        let f = k as f64 / n as f64;
        let (x, y) = (a[0] + dx * f, a[1] + dy * f);
        let key = (a[2] + (b[2] - a[2]) * f) as f32;
        for o in lo..=hi {
            let (px, py) = if steep { (x.floor() as i64 + o, y.floor() as i64) } else { (x.floor() as i64, y.floor() as i64 + o) };
            if px >= 0 && py >= 0 && (px as usize) < t.w && (py as usize) < t.h {
                t.fill(px as usize, py as usize, key, c, alpha, false);
            }
        }
    }
}

/// Renders `scene` through `camera`.
/// A primitive projected for a painter that has no depth buffer (the desktop app draws these
/// with its GPU): screen points in pixels of a w × h picture and the shaded colour with its
/// opacity. [`flatten`] lists them farthest first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Flat {
    Tri { p: [[f32; 2]; 3], rgba: [u8; 4] },
    Line { a: [f32; 2], b: [f32; 2], rgba: [u8; 4] },
}

/// The scene projected and shaded as [`render`] shades it, sorted farthest first (the
/// painter's algorithm: right for the convex, separate bodies of these scenes, approximate
/// where faces cross).
pub fn flatten(scene: &Scene, camera: &Camera, w: f64, h: f64) -> Vec<Flat> {
    let cam = camera.sanitized();
    let pr = Projector::new(&cam, w, h);
    let b = pr.b;
    let light = normalized([
        -b.forward[0] + 0.35 * b.up[0] + 0.25 * b.right[0],
        -b.forward[1] + 0.35 * b.up[1] + 0.25 * b.right[1],
        -b.forward[2] + 0.35 * b.up[2] + 0.25 * b.right[2],
    ])
    .unwrap_or([0.0, 0.0, 1.0]);
    let depth = |p: [f64; 3]| dot(sub(p, b.eye), b.forward);
    let xy = |q: [f64; 3]| [q[0] as f32, q[1] as f32];
    let byte = |c: f32| c.round().clamp(0.0, 255.0) as u8;
    let nudge = cam.distance * 4e-3;
    let mut out: Vec<(f64, usize, Flat)> = Vec::new();
    for (i, t) in scene.tris.iter().take(MAX_PRIMITIVES).enumerate().filter(|(_, t)| t.alpha > 0) {
        let (Some(a), Some(bb), Some(c), Some(col)) = (pr.project(t.p[0], 0.0), pr.project(t.p[1], 0.0), pr.project(t.p[2], 0.0), shade(t, light))
        else {
            continue;
        };
        let d = (depth(t.p[0]) + depth(t.p[1]) + depth(t.p[2])) / 3.0;
        out.push((d, i, Flat::Tri { p: [xy(a), xy(bb), xy(c)], rgba: [byte(col[0]), byte(col[1]), byte(col[2]), t.alpha] }));
    }
    let base = scene.tris.len();
    for (i, l) in scene.lines.iter().take(MAX_PRIMITIVES).enumerate().filter(|(_, l)| l.alpha > 0) {
        let (Some(a), Some(z)) = (pr.project(l.a, nudge), pr.project(l.b, nudge)) else { continue };
        let d = 0.5 * (depth(l.a) + depth(l.b)) - nudge;
        out.push((d, base + i, Flat::Line { a: xy(a), b: xy(z), rgba: [l.color.0, l.color.1, l.color.2, l.alpha] }));
    }
    // Farthest first; ties keep the scene's order.
    out.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)));
    out.into_iter().map(|(_, _, f)| f).collect()
}

/// A translucent triangle or a line, by its index, in the back-to-front pass.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Layer {
    Tri(usize),
    Line(usize),
}

pub fn render(scene: &Scene, camera: &Camera, opt: &Options) -> Canvas {
    let (w, h) = (opt.width.clamp(1, MAX_SIDE), opt.height.clamp(1, MAX_SIDE));
    let ss = opt.supersample.clamp(1, 3);
    let (sw, sh) = (w * ss, h * ss);
    let mut t = Target { w: sw, h: sh, color: Vec::with_capacity(sw * sh), depth: vec![f32::NEG_INFINITY; sw * sh] };
    let (top, bottom) = (rgbf(opt.background.0), rgbf(opt.background.1));
    for y in 0..sh {
        let f = y as f32 / (sh.max(2) - 1) as f32;
        let c = [top[0] + (bottom[0] - top[0]) * f, top[1] + (bottom[1] - top[1]) * f, top[2] + (bottom[2] - top[2]) * f];
        t.color.extend(std::iter::repeat_n(c, sw));
    }
    let cam = camera.sanitized();
    let pr = Projector::new(&cam, sw as f64, sh as f64);
    let b = pr.b;
    let light = normalized([
        -b.forward[0] + 0.35 * b.up[0] + 0.25 * b.right[0],
        -b.forward[1] + 0.35 * b.up[1] + 0.25 * b.right[1],
        -b.forward[2] + 0.35 * b.up[2] + 0.25 * b.right[2],
    ])
    .unwrap_or([0.0, 0.0, 1.0]);
    let tris = &scene.tris[..scene.tris.len().min(MAX_PRIMITIVES)];
    let lines = &scene.lines[..scene.lines.len().min(MAX_PRIMITIVES)];
    let screen = |tri: &Tri| -> Option<[[f64; 3]; 3]> { Some([pr.project(tri.p[0], 0.0)?, pr.project(tri.p[1], 0.0)?, pr.project(tri.p[2], 0.0)?]) };
    // Opaque triangles.
    for tri in tris.iter().filter(|t| t.alpha == 255) {
        if let (Some(s), Some(c)) = (screen(tri), shade(tri, light)) {
            raster_tri(&mut t, s, c, 1.0, true);
        }
    }
    // Translucent triangles and lines together, farthest first (ties by their order in the
    // scene, triangles before lines), as `flatten` orders them: a line in front of a
    // see-through face is drawn over it, not washed out by it. Lines a little towards the eye
    // so edges lying on faces win the depth test against opaque ones.
    let nudge = cam.distance * 4e-3;
    let lw = opt.line_width.clamp(0.5, 8.0) * ss as f64;
    let along = |p: [f64; 3]| dot(sub(p, b.eye), b.forward);
    let glass = tris.iter().enumerate().filter(|(_, t)| t.alpha > 0 && t.alpha < 255).map(|(i, t)| {
        let c = [(t.p[0][0] + t.p[1][0] + t.p[2][0]) / 3.0, (t.p[0][1] + t.p[1][1] + t.p[2][1]) / 3.0, (t.p[0][2] + t.p[1][2] + t.p[2][2]) / 3.0];
        (along(c), Layer::Tri(i))
    });
    let edges = lines.iter().enumerate().filter(|(_, l)| l.alpha > 0).map(|(i, l)| (0.5 * (along(l.a) + along(l.b)) - nudge, Layer::Line(i)));
    let mut layers: Vec<(f64, Layer)> = glass.chain(edges).collect();
    layers.sort_by(|a, z| z.0.total_cmp(&a.0).then(a.1.cmp(&z.1)));
    for (_, layer) in layers {
        match layer {
            Layer::Tri(i) => {
                let Some(tri) = tris.get(i) else { continue };
                if let (Some(s), Some(c)) = (screen(tri), shade(tri, light)) {
                    raster_tri(&mut t, s, c, f32::from(tri.alpha) / 255.0, false);
                }
            }
            Layer::Line(i) => {
                let Some(l) = lines.get(i) else { continue };
                if let (Some(a), Some(z)) = (pr.project(l.a, nudge), pr.project(l.b, nudge)) {
                    raster_line(&mut t, a, z, lw, rgbf(l.color), f32::from(l.alpha) / 255.0);
                }
            }
        }
    }
    // Down to the output size, a box filter over each pixel's samples.
    let mut rgba = Vec::with_capacity(w * h * 4);
    let norm = 1.0 / (ss * ss) as f32;
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0f32; 3];
            for sy in 0..ss {
                for sx in 0..ss {
                    if let Some(p) = t.color.get((y * ss + sy) * sw + x * ss + sx) {
                        for (a, v) in acc.iter_mut().zip(p) {
                            *a += v;
                        }
                    }
                }
            }
            for v in acc {
                rgba.push((v * norm).round().clamp(0.0, 255.0) as u8);
            }
            rgba.push(255);
        }
    }
    Canvas { w, h, rgba }
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
    fn flattened_scenes_are_sorted_far_to_near_and_shaded_as_rendered() {
        let mut s = Scene::default();
        s.quad([[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]], Rgb(200, 0, 0), 255);
        s.quad([[-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]], Rgb(0, 200, 0), 128);
        s.line([-1.0, 0.0, 0.5], [1.0, 0.0, 0.5], Rgb(0, 0, 0), 255);
        let mut c = Camera::default();
        c.standard(StandardView::Front);
        c.fit([-1.0, -1.0, 0.0], [1.0, 1.0, 1.0], 1.0);
        let f = flatten(&s, &c, 100.0, 100.0);
        assert_eq!(f.len(), 5);
        let colour = |x: &Flat| match x {
            Flat::Tri { rgba, .. } | Flat::Line { rgba, .. } => *rgba,
        };
        // The square nearer the eye comes last.
        let near_is_green = c.basis().forward[2] < 0.0;
        let last = colour(&f[4]);
        assert_eq!(last[1] > last[0], near_is_green, "{f:?}");
        // The line sits between the squares.
        assert!(matches!(f[2], Flat::Line { .. }), "{f:?}");
        assert_eq!(colour(&f[0])[3] == 128, !near_is_green);
    }

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

    fn square(z: f64, c: Rgb) -> Scene {
        let mut s = Scene::default();
        s.quad([[-1.0, -1.0, z], [1.0, -1.0, z], [1.0, 1.0, z], [-1.0, 1.0, z]], c, 255);
        s
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
    fn the_nearer_triangle_wins_whatever_the_order() {
        let (red, blue) = (Rgb(255, 0, 0), Rgb(0, 0, 255));
        let mut a = square(1.0, red);
        a.tris.extend(square(-1.0, blue).tris);
        let mut b = square(-1.0, blue);
        b.tris.extend(square(1.0, red).tris);
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let opt = Options { width: 64, height: 48, ..Default::default() };
            let (pa, pb) = (render(&a, &front(projection), &opt), render(&b, &front(projection), &opt));
            assert_eq!(pa, pb);
            let px = pa.pixel(32, 24).unwrap();
            assert!(px.0 > 100 && px.2 == 0, "{px:?}");
        }
    }

    #[test]
    fn an_edge_on_a_see_through_face_is_drawn_over_it() {
        // A light, see-through square with a black edge across it, and one behind it: the edge
        // on the face shows black (the face is not blended over it), the one behind is dimmed.
        let mut s = Scene::default();
        s.quad([[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]], Rgb(230, 230, 230), 110);
        s.line([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0], Rgb(0, 0, 0), 255);
        s.line([0.0, -1.0, -2.0], [0.0, 1.0, -2.0], Rgb(0, 0, 0), 255);
        let opt = Options { width: 81, height: 61, background: (Rgb(40, 40, 60), Rgb(40, 40, 60)), ..Default::default() };
        let c = render(&s, &front(Projection::Orthographic), &opt);
        let (on, behind) = (c.pixel(34, 30).unwrap(), c.pixel(40, 25).unwrap());
        assert!(on.0 < 10, "the edge on the face: {on:?}");
        assert!(behind.0 > 60, "the edge behind it, through the face: {behind:?}");
    }

    #[test]
    fn rendering_is_deterministic_and_lines_show_on_faces() {
        let mut s = square(0.0, Rgb(200, 200, 200));
        s.line([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0], Rgb(0, 0, 0), 255);
        // A line behind the square is hidden.
        s.line([0.0, -1.0, -2.0], [0.0, 1.0, -2.0], Rgb(255, 0, 0), 255);
        let mut c = front(Projection::Perspective);
        let opt = Options { width: 81, height: 61, supersample: 2, ..Default::default() };
        let one = render(&s, &c, &opt);
        assert_eq!(one.png(), render(&s, &c, &opt).png());
        assert_eq!(one.pixel(35, 30), Some(Rgb(0, 0, 0)));
        let col: Vec<Rgb> = (22..39).filter_map(|y| one.pixel(40, y)).collect();
        assert!(col.iter().all(|p| p.0 == p.1), "the hidden red line shows: {col:?}");
        // Orbiting changes the picture; hostile cameras draw the ground only, no panic.
        c.orbit(40.0, 10.0);
        assert_ne!(one, render(&s, &c, &opt));
        let bad =
            Camera { target: [f64::NAN; 3], yaw: f64::INFINITY, pitch: f64::NAN, distance: -1.0, fov: 0.0, projection: Projection::Orthographic };
        let _ = render(&s, &bad, &opt);
        let mut tri = Scene::default();
        tri.tri([f64::NAN, 0.0, 0.0], [1e300, 0.0, 0.0], [0.0, 1.0, 0.0], Rgb::BLACK, 255);
        tri.line([f64::INFINITY; 3], [0.0; 3], Rgb::BLACK, 255);
        let p = render(&tri, &front(Projection::Perspective), &Options { width: 0, height: 100_000, ..Default::default() });
        assert_eq!((p.w, p.h), (1, MAX_SIDE));
    }

    #[test]
    fn translucent_faces_blend_over_what_is_behind() {
        let mut s = square(-1.0, Rgb(0, 0, 255));
        s.quad([[-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]], Rgb(255, 0, 0), 128);
        let p = render(&s, &front(Projection::Orthographic), &Options { width: 40, height: 40, ..Default::default() });
        let c = p.pixel(20, 20).unwrap();
        assert!(c.0 > 40 && c.2 > 40, "{c:?}");
    }

    #[test]
    fn shared_edges_are_filled_once() {
        // A translucent quad over a flat ground: a pixel on its diagonal blended twice (or not at
        // all) would differ from the rest.
        let mut s = Scene::default();
        s.quad([[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]], Rgb(0, 0, 0), 128);
        let opt = Options { width: 64, height: 64, background: (Rgb::WHITE, Rgb::WHITE), ..Default::default() };
        let mut c = front(Projection::Orthographic);
        c.distance = 1.0 / (15f64.to_radians()).tan();
        let p = render(&s, &c, &opt);
        let inside: Vec<Rgb> = (2..62).flat_map(|y| (2..62).map(move |x| (x, y))).filter_map(|(x, y)| p.pixel(x, y)).collect();
        assert!(inside.iter().all(|q| *q == inside[0]), "{:?}", inside.iter().find(|q| **q != inside[0]));
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

//! Moving the orbit camera as a person does: zooming toward a point that keeps its place on
//! screen, orbiting about a pivot, drags whose result depends only on where the pointer is (not
//! on the path it took), the nearest axis view, and view changes animated with an ease-in-out
//! that turns the short way round.
//!
//! Adapted from SolidCraft's camera (`crates/render/src/camera.rs`: `zoom_at`, `orbit_about`,
//! `frame_coords` / `place`, `looking_from`, `interpolate`, `CameraAnim`, `ease_in_out`; MIT OR
//! Apache-2.0, see ATTRIBUTION.md), on EMCraft's y-up camera without roll.
//!
//! - **Zoom to a point.** Scaling the eye's offset from a fixed point `a` by `k` (the target to
//!   `a + k (t − a)`, the distance to `k d`) keeps `a` on the same ray from the eye under
//!   perspective, and at the same place under orthographic projection, whose visible height is
//!   proportional to the distance.
//! - **Orbit about a pivot.** A rigid turn of the camera about the pivot leaves the pivot's
//!   coordinates in the camera's frame unchanged; turning the camera about its target and then
//!   moving the target so the pivot has its old frame coordinates is that turn.
//! - **Ease.** The cubic smoothstep 3t² − 2t³ (Hermite interpolation with zero end slopes; see
//!   Ebert, Musgrave, Peachey, Perlin and Worley, *Texturing and Modeling*, 3rd ed., 2003, §2.1).

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use super::{Camera, dot, normalized, sub};

/// The nearest the eye comes to the target, in metres.
pub const MIN_DISTANCE: f64 = 1e-9;
/// The farthest the eye goes from the target, in metres.
pub const MAX_DISTANCE: f64 = 1e9;

/// The angle one pixel of a drag turns the view (radians): a drag across ~600 px is about half a
/// turn.
pub const RADIANS_PER_PIXEL: f64 = 0.008;

/// How much nearer (in the cosine of the angle) another view must be before a snapped drag
/// leaves the one it shows.
const SNAP_MARGIN: f64 = 0.025;

/// Smooth ease-in-out on [0, 1]: the cubic smoothstep, at its fastest mid-way (1.5 times the
/// average speed). Out-of-range and non-finite inputs give the nearer end (NaN gives 1).
///
/// ```
/// use emcraft_render::view3d::ease_in_out;
/// assert_eq!((ease_in_out(0.0), ease_in_out(0.5), ease_in_out(1.0)), (0.0, 0.5, 1.0));
/// assert!(ease_in_out(0.1) < 0.1 && ease_in_out(0.9) > 0.9);
/// assert_eq!((ease_in_out(-3.0), ease_in_out(7.0), ease_in_out(f64::NAN)), (0.0, 1.0, 1.0));
/// ```
pub fn ease_in_out(t: f64) -> f64 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 1.0 };
    t * t * (3.0 - 2.0 * t)
}

/// `a` turned to `b` the short way round, as a signed angle in (−π, π].
fn short_turn(a: f64, b: f64) -> f64 {
    let d = (b - a + PI).rem_euclid(TAU) - PI;
    if d.is_finite() { d } else { 0.0 }
}

/// The 26 directions of an axis-aligned cube's faces, edges and corners (unit vectors), in a
/// fixed order: the views a snap and the view cube go to.
///
/// ```
/// use emcraft_render::view3d::cube_directions;
/// let d = cube_directions();
/// assert_eq!(d.len(), 26);
/// assert!(d.iter().all(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2] - 1.0).abs() < 1e-12));
/// ```
pub fn cube_directions() -> Vec<[f64; 3]> {
    let s = [-1.0, 0.0, 1.0];
    let mut out = Vec::with_capacity(26);
    for x in s {
        for y in s {
            for z in s {
                out.extend(normalized([x, y, z]));
            }
        }
    }
    out
}

impl Camera {
    /// The direction from the target to the eye (unit).
    pub fn back(&self) -> [f64; 3] {
        self.basis().forward.map(|c| -c)
    }

    /// Zooms by `factor` (below 1 closer) toward `anchor`, which keeps its place on screen in
    /// either projection; without an anchor, toward the target. The distance stays within
    /// `limits` (metres, and always within [`MIN_DISTANCE`]–[`MAX_DISTANCE`]); a clamped zoom
    /// moves only as far as the limit, the anchor still fixed. A factor that is not a positive
    /// number does nothing; one step is held to 1/20–20.
    ///
    /// ```
    /// use emcraft_render::view3d::{Camera, StandardView};
    /// let mut c = Camera { distance: 2.0, ..Camera::default() };
    /// c.standard(StandardView::Front);
    /// let a = [0.3, 0.2, 0.0];
    /// let before = c.project(a, 200.0, 100.0).unwrap();
    /// c.zoom_at(Some(a), 0.5, [0.1, 10.0]);
    /// let after = c.project(a, 200.0, 100.0).unwrap();
    /// assert!((c.distance - 1.0).abs() < 1e-12);
    /// assert!((before[0] - after[0]).abs() < 1e-9 && (before[1] - after[1]).abs() < 1e-9);
    /// // Held at the limit; nonsense refused.
    /// c.zoom_at(None, 1e-6, [0.1, 10.0]);
    /// assert!((c.distance - 0.1).abs() < 1e-12);
    /// c.zoom_at(None, f64::NAN, [0.1, 10.0]);
    /// c.zoom_at(None, -2.0, [0.1, 10.0]);
    /// assert!((c.distance - 0.1).abs() < 1e-12);
    /// ```
    pub fn zoom_at(&mut self, anchor: Option<[f64; 3]>, factor: f64, limits: [f64; 2]) {
        if !(factor.is_finite() && factor > 0.0) {
            return;
        }
        let c = self.sanitized();
        let lo = if limits[0].is_finite() { limits[0].clamp(MIN_DISTANCE, MAX_DISTANCE) } else { MIN_DISTANCE };
        let hi = if limits[1].is_finite() { limits[1].clamp(lo, MAX_DISTANCE) } else { MAX_DISTANCE };
        let nd = (c.distance * factor.clamp(0.05, 20.0)).clamp(lo, hi);
        let k = nd / c.distance;
        if let Some(a) = anchor.filter(|a| a.iter().all(|v| v.is_finite() && v.abs() < 1e12)) {
            self.target = [0, 1, 2].map(|i| a[i] + (c.target[i] - a[i]) * k);
        }
        self.distance = nd;
    }

    /// Where `p` lies from the target along the camera's right, up and back axes.
    ///
    /// ```
    /// use emcraft_render::view3d::{Camera, StandardView};
    /// let mut c = Camera::default();
    /// c.standard(StandardView::Front);
    /// // From the front, right is +x, up +y and back (towards the eye) +z.
    /// let k = c.frame_coords([1.0, 2.0, 3.0]);
    /// assert!((k[0] - 1.0).abs() < 1e-12 && (k[1] - 2.0).abs() < 1e-12 && (k[2] - 3.0).abs() < 1e-12);
    /// ```
    pub fn frame_coords(&self, p: [f64; 3]) -> [f64; 3] {
        let b = self.basis();
        let d = sub(p, self.target);
        [dot(d, b.right), dot(d, b.up), -dot(d, b.forward)]
    }

    /// Moves the target so `p` lies at `coords` ([`Camera::frame_coords`]) in the camera's frame:
    /// with the distance kept, `p` is where it was on screen.
    pub fn place(&mut self, p: [f64; 3], coords: [f64; 3]) {
        if !p.iter().chain(&coords).all(|v| v.is_finite()) {
            return;
        }
        let b = self.basis();
        self.target = [0, 1, 2].map(|i| p[i] - (b.right[i] * coords[0] + b.up[i] * coords[1] - b.forward[i] * coords[2]));
    }

    /// Orbits by screen pixels dragged ([`Camera::orbit`]) about `pivot`: the camera turns rigidly
    /// about it, so the pivot keeps its place on screen in either projection.
    ///
    /// ```
    /// use emcraft_render::view3d::Camera;
    /// let mut c = Camera { distance: 3.0, ..Camera::default() };
    /// let pivot = [0.4, -0.2, 0.1];
    /// let before = c.project(pivot, 300.0, 200.0).unwrap();
    /// c.orbit_about(80.0, -30.0, pivot);
    /// let after = c.project(pivot, 300.0, 200.0).unwrap();
    /// assert!((before[0] - after[0]).abs() < 1e-9 && (before[1] - after[1]).abs() < 1e-9);
    /// ```
    pub fn orbit_about(&mut self, dx: f64, dy: f64, pivot: [f64; 3]) {
        let keep = self.frame_coords(pivot);
        self.orbit(dx, dy);
        self.place(pivot, keep);
    }

    /// Turns the view by `yaw` and `pitch` radians about the target (pitch held to the poles).
    pub fn turn(&mut self, yaw: f64, pitch: f64) {
        if yaw.is_finite() && pitch.is_finite() {
            self.yaw = (self.yaw + yaw).rem_euclid(TAU);
            self.pitch = (self.pitch + pitch).clamp(-FRAC_PI_2, FRAC_PI_2);
        }
    }

    /// The view turned `degrees` (0–180) toward the screen's `right` and `up` (each −1, 0 or 1),
    /// as the arrow keys and the view cube's arrows turn it: the eye moves that way round the
    /// target (over a pole, the view comes down the other side). Other angles do nothing.
    ///
    /// ```
    /// use emcraft_render::view3d::{Camera, StandardView};
    /// let mut c = Camera::default();
    /// c.standard(StandardView::Front);
    /// // A quarter turn right looks from the right side.
    /// let r = c.stepped(1.0, 0.0, 90.0);
    /// assert!((r.yaw - std::f64::consts::FRAC_PI_2).abs() < 1e-9 && r.pitch.abs() < 1e-9);
    /// assert_eq!(c.stepped(1.0, 0.0, f64::NAN), c);
    /// ```
    pub fn stepped(&self, right: f64, up: f64, degrees: f64) -> Camera {
        if !(degrees.is_finite() && degrees > 0.0 && degrees <= 180.0 && right.is_finite() && up.is_finite()) {
            return *self;
        }
        let (sin, cos) = if degrees == 90.0 { (1.0, 0.0) } else { degrees.to_radians().sin_cos() };
        let b = self.basis();
        let back = self.back();
        let toward = [0, 1, 2].map(|i| b.right[i] * right + b.up[i] * up);
        self.looking_from([0, 1, 2].map(|i| back[i] * cos + toward[i] * sin))
    }

    /// The same camera looking from direction `back` (target to eye). Straight up or down keeps
    /// the yaw, rounded to the nearest quarter turn so the view stays square; a zero or
    /// non-finite direction keeps the camera.
    ///
    /// ```
    /// use emcraft_render::view3d::{Camera, StandardView};
    /// let c = Camera::default().looking_from([1.0, 0.0, 0.0]);
    /// let mut right = Camera::default();
    /// right.standard(StandardView::Right);
    /// assert!((c.yaw - right.yaw).abs() < 1e-12 && c.pitch.abs() < 1e-12);
    /// assert_eq!(Camera::default().looking_from([0.0; 3]), Camera::default());
    /// ```
    pub fn looking_from(&self, back: [f64; 3]) -> Camera {
        let Some(b) = normalized(back) else { return *self };
        let pitch = b[1].clamp(-1.0, 1.0).asin();
        let yaw = if b[0].hypot(b[2]) < 1e-9 { nearest_quarter(self.yaw) } else { b[0].atan2(b[2]).rem_euclid(TAU) };
        Camera { yaw, pitch, ..*self }
    }

    /// The camera turned to the nearest of the cube's 26 views (faces, edges, corners;
    /// [`cube_directions`]).
    ///
    /// ```
    /// use emcraft_render::view3d::Camera;
    /// let c = Camera { yaw: 0.2, pitch: -0.1, ..Camera::default() }.snapped();
    /// assert!(c.yaw.abs() < 1e-12 && c.pitch.abs() < 1e-12);
    /// ```
    pub fn snapped(&self) -> Camera {
        let b = self.back();
        let best = cube_directions().into_iter().max_by(|p, q| dot(*p, b).total_cmp(&dot(*q, b)));
        best.map_or(*self, |d| self.looking_from(d))
    }

    /// The camera `s` of the way (0–1) from `a` to `b`: the target and the angles linearly, the
    /// yaw the short way round, the distance geometrically (so a large fit change moves at an
    /// even pace), the projection `b`'s.
    ///
    /// ```
    /// use emcraft_render::view3d::Camera;
    /// let a = Camera { yaw: 6.2, distance: 1.0, ..Camera::default() };
    /// let b = Camera { yaw: 0.1, distance: 4.0, ..Camera::default() };
    /// let m = Camera::interpolate(&a, &b, 0.5);
    /// // Across 2π, the short way: half-way from 6.2 to 0.1 + 2π is just past 0.
    /// let want = (6.2 + 0.5 * (0.1 + std::f64::consts::TAU - 6.2)) % std::f64::consts::TAU;
    /// assert!((m.yaw - want).abs() < 1e-9);
    /// assert!((m.distance - 2.0).abs() < 1e-12);
    /// assert_eq!(Camera::interpolate(&a, &b, 1.0), b);
    /// ```
    pub fn interpolate(a: &Camera, b: &Camera, s: f64) -> Camera {
        if !(s > 0.0) {
            return *a;
        }
        if s >= 1.0 {
            return *b;
        }
        let (a, b) = (a.sanitized(), b.sanitized());
        let lerp = |x: f64, y: f64| x + (y - x) * s;
        Camera {
            target: [0, 1, 2].map(|i| lerp(a.target[i], b.target[i])),
            yaw: (a.yaw + short_turn(a.yaw, b.yaw) * s).rem_euclid(TAU),
            pitch: lerp(a.pitch, b.pitch),
            distance: (a.distance.ln() + (b.distance.ln() - a.distance.ln()) * s).exp(),
            fov: lerp(a.fov, b.fov),
            projection: b.projection,
        }
    }
}

/// `yaw` rounded to the nearest quarter turn, in [0, 2π).
fn nearest_quarter(yaw: f64) -> f64 {
    if yaw.is_finite() { ((yaw / FRAC_PI_2).round() * FRAC_PI_2).rem_euclid(TAU) } else { 0.0 }
}

/// A view change in progress: from one camera to another over `duration` seconds from `start`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraAnim {
    pub from: Camera,
    pub to: Camera,
    pub start: f64,
    pub duration: f64,
}

impl CameraAnim {
    /// A view change's usual length, in seconds.
    pub const DURATION: f64 = 0.5;

    /// A view change starting at `now` (seconds): half a second, up to twice that for turns past
    /// a quarter turn, so a large turn never sweeps fast.
    ///
    /// ```
    /// use emcraft_render::view3d::{Camera, CameraAnim};
    /// let a = Camera { yaw: 0.0, pitch: 0.0, ..Camera::default() };
    /// let small = CameraAnim::new(a, Camera { yaw: 0.3, ..a }, 10.0);
    /// let far = CameraAnim::new(a, Camera { yaw: 3.0, ..a }, 10.0);
    /// assert_eq!(small.duration, 0.5);
    /// assert!((far.duration - 0.5 * 3.0 / std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    /// let half = CameraAnim::new(a, Camera { yaw: 3.0, ..a }, 10.0);
    /// let (mid, done) = half.sample(10.0 + 0.5 * half.duration);
    /// assert!(!done && (mid.yaw - 1.5).abs() < 1e-9);
    /// assert_eq!(half.sample(11.0), (half.to, true));
    /// // A clock that is not a number ends the move.
    /// assert_eq!(half.sample(f64::NAN), (half.to, true));
    /// ```
    pub fn new(from: Camera, to: Camera, now: f64) -> CameraAnim {
        let turn = short_turn(from.yaw, to.yaw).abs().max((to.pitch - from.pitch).abs());
        let k = if turn.is_finite() { (turn / FRAC_PI_2).clamp(1.0, 2.0) } else { 1.0 };
        CameraAnim { from, to, start: if now.is_finite() { now } else { 0.0 }, duration: CameraAnim::DURATION * k }
    }

    /// The camera at time `now`, and whether the move has ended.
    pub fn sample(&self, now: f64) -> (Camera, bool) {
        let t = if self.duration > 0.0 { (now - self.start) / self.duration } else { 1.0 };
        if !(t < 1.0) {
            return (self.to, true);
        }
        (Camera::interpolate(&self.from, &self.to, ease_in_out(t)), false)
    }
}

/// An orbit drag: the camera it began with, the pointer's whole move since (pixels) and the
/// pivot chosen when it began. Each frame's camera is the starting one turned by the whole move,
/// so the view depends on where the pointer is, not on the path it took: a drag past a pole and
/// back, or many tiny steps, end where one straight move would. A camera changed by something
/// else meanwhile (a zoom) starts the sum again from there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub start: Camera,
    pub dx: f64,
    pub dy: f64,
    pub pivot: Option<[f64; 3]>,
    last: Camera,
    /// The view a snapped drag shows, kept until the drag clearly leaves it.
    shown: Option<Camera>,
}

impl Drag {
    /// A drag from `camera` about `pivot` (`None`: about the target).
    pub fn new(camera: Camera, pivot: Option<[f64; 3]>) -> Drag {
        Drag { start: camera, dx: 0.0, dy: 0.0, pivot, last: camera, shown: None }
    }

    /// Adds the pointer's move (`dx`, `dy` pixels) and turns `camera`: the starting camera orbited
    /// by the whole move, then with `snap` turned to the nearest of the cube's views
    /// ([`Camera::snapped`]), the pivot kept on screen.
    ///
    /// ```
    /// use emcraft_render::view3d::{Camera, Drag};
    /// let start = Camera::default();
    /// let mut cam = start;
    /// let mut drag = Drag::new(cam, None);
    /// // Up past the pole and back down: the pitch is held at the pole on the way, and the
    /// // camera comes back to where it began.
    /// drag.orbit(&mut cam, 0.0, 900.0, false);
    /// assert!((cam.pitch - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    /// drag.orbit(&mut cam, 0.0, -900.0, false);
    /// assert!((cam.pitch - start.pitch).abs() < 1e-12);
    /// ```
    pub fn orbit(&mut self, camera: &mut Camera, dx: f64, dy: f64, snap: bool) {
        if *camera != self.last {
            *self = Drag::new(*camera, self.pivot);
        }
        if dx.is_finite() && dy.is_finite() {
            self.dx = (self.dx + dx).clamp(-1e7, 1e7);
            self.dy = (self.dy + dy).clamp(-1e7, 1e7);
        }
        let mut c = self.start;
        match self.pivot {
            Some(p) => c.orbit_about(self.dx, self.dy, p),
            None => c.orbit(self.dx, self.dy),
        }
        if snap {
            let keep = self.pivot.map(|p| (p, c.frame_coords(p)));
            let free = c.back();
            let mut to = c.snapped();
            // The view already shown stays unless the orbit has clearly left it (no flicker at a
            // boundary between two views).
            if let Some(s) = self.shown
                && dot(s.back(), free) + SNAP_MARGIN >= dot(to.back(), free)
            {
                (to.yaw, to.pitch) = (s.yaw, s.pitch);
            }
            self.shown = Some(to);
            c = to;
            if let Some((p, k)) = keep {
                c.place(p, k);
            }
        } else {
            self.shown = None;
        }
        *camera = c;
        self.last = c;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view3d::{Projection, StandardView};

    fn close(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() <= tol)
    }

    #[test]
    fn zoom_keeps_the_anchor_on_screen_in_both_projections() {
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let mut c = Camera { target: [0.01, 0.02, -0.03], distance: 0.2, projection, ..Camera::default() };
            let (w, h) = (640.0, 480.0);
            // The point the ray through a pixel meets on the plane through the target.
            let (o, d) = c.ray(500.0, 120.0, w, h).unwrap();
            let n = c.back();
            let t = dot(sub(c.target, o), n) / dot(d, n);
            let a = [0, 1, 2].map(|i| o[i] + d[i] * t);
            let before = c.project(a, w, h).unwrap();
            for f in [0.5, 0.8, 1.7, 3.0] {
                c.zoom_at(Some(a), f, [1e-4, 10.0]);
                let after = c.project(a, w, h).unwrap();
                assert!((before[0] - after[0]).abs() < 1e-6 && (before[1] - after[1]).abs() < 1e-6, "{projection:?} {f}");
            }
        }
    }

    #[test]
    fn a_pivot_orbit_is_rigid_and_a_drag_is_path_independent() {
        let pivot = [0.3, 0.1, -0.2];
        let start = Camera { distance: 2.0, ..Camera::default() };
        let mut one = start;
        Drag::new(start, Some(pivot)).orbit(&mut one, 120.0, -40.0, false);
        let mut many = start;
        let mut drag = Drag::new(start, Some(pivot));
        for k in 0..240 {
            // A wandering path ending at the same total move.
            let wobble = if k % 2 == 0 { 7.0 } else { -7.0 };
            drag.orbit(&mut many, 0.5 + wobble, -40.0 / 240.0, false);
        }
        assert!((one.yaw - many.yaw).abs() < 1e-9 && (one.pitch - many.pitch).abs() < 1e-9 && close(one.target, many.target, 1e-9));
        // The pivot stays in the camera's frame, so on screen.
        assert!(close(start.frame_coords(pivot), many.frame_coords(pivot), 1e-9));
        // A camera changed meanwhile (a zoom) starts the sum again from there.
        many.zoom_at(None, 0.5, [1e-3, 10.0]);
        let zoomed = many;
        drag.orbit(&mut many, 0.0, 0.0, false);
        assert_eq!(many, zoomed);
    }

    #[test]
    fn a_snapped_drag_reaches_the_nearest_view_and_keeps_the_pivot() {
        let pivot = [0.1, 0.0, 0.0];
        let mut front = Camera::default();
        front.standard(StandardView::Front);
        let mut cam = front;
        let mut drag = Drag::new(front, Some(pivot));
        drag.orbit(&mut cam, 10.0, 5.0, true);
        assert!(cam.yaw.abs() < 1e-12 && cam.pitch.abs() < 1e-12, "{cam:?}");
        assert!(close(front.frame_coords(pivot), cam.frame_coords(pivot), 1e-9));
        // Far enough round, the next view: 45° to the side is the edge view.
        drag.orbit(&mut cam, -(std::f64::consts::FRAC_PI_4 / RADIANS_PER_PIXEL) - 10.0, -5.0, true);
        assert!((cam.yaw - std::f64::consts::FRAC_PI_4).abs() < 1e-9, "{}", cam.yaw.to_degrees());
    }

    #[test]
    fn animations_ease_turn_the_short_way_and_end_on_time() {
        let a = Camera { yaw: 0.1, pitch: 0.0, ..Camera::default() };
        let b = Camera { yaw: TAU - 0.1, pitch: 0.4, distance: 3.0, ..Camera::default() };
        let anim = CameraAnim::new(a, b, 2.0);
        assert_eq!(anim.duration, CameraAnim::DURATION);
        let mut last = a.yaw + 1.0;
        for k in 0..30 {
            let (c, done) = anim.sample(2.0 + k as f64 / 60.0);
            assert!(!done);
            // Through zero, never round the long way.
            let y = if c.yaw > PI { c.yaw - TAU } else { c.yaw };
            assert!(y <= last + 1e-12 && y.abs() <= 0.1 + 1e-12, "{k}: {y}");
            last = y;
        }
        assert_eq!(anim.sample(2.5), (b, true));
        // Large turns take longer, at most twice as long.
        let far = CameraAnim::new(a, Camera { pitch: -FRAC_PI_2, yaw: a.yaw + PI, ..a }, 0.0);
        assert_eq!(far.duration, 2.0 * CameraAnim::DURATION);
        // Hostile cameras and clocks give finite cameras.
        let bad = CameraAnim::new(Camera { yaw: f64::NAN, distance: -1.0, ..a }, Camera { target: [f64::INFINITY; 3], ..b }, f64::NAN);
        let (c, _) = bad.sample(0.25);
        assert!(c.yaw.is_finite() && c.distance > 0.0 && c.target.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn the_cube_views_include_the_standard_ones() {
        let dirs = cube_directions();
        for v in StandardView::ALL {
            let mut c = Camera::default();
            c.standard(v);
            assert!(dirs.iter().any(|d| close(*d, c.back(), 1e-12)), "{v:?}");
            let s = c.snapped();
            assert!(close(s.back(), c.back(), 1e-12), "{v:?}");
        }
    }
}

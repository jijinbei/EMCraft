//! The scene and the camera as the GPU reads them: vertex bytes made once per scene, uniforms
//! made each frame. Plain arithmetic, no device, so it is checked against the camera's
//! projection (`emcraft_render::view3d::Camera::project`) without a GPU.

use std::collections::BTreeMap;

use emcraft_render::Rgb;
use emcraft_render::view3d::{Camera, Projection, Scene};

/// Bytes per triangle vertex: position (3 × f32, from the scene's origin), the face's unit
/// normal (3 × f32), its rank among coplanar faces (f32, [`Packed::new`]), the colour with its
/// opacity (4 × u8) and its tag (u32, for the outline and picking).
pub const TRI_VERTEX: usize = 36;
/// The most coplanar groups ranked apart in one plane.
pub const MAX_RANK: u32 = 15;
/// Bytes per line: both ends (2 × 3 × f32) and the colour with its opacity (4 × u8).
pub const LINE: usize = 28;
/// Bytes of [`Uniforms`].
pub const UNIFORM_BYTES: usize = 64 + 5 * 16;
/// The most vertex bytes one scene uploads (each kind): beyond, the rest is left out, as the CPU
/// renderer leaves out primitives beyond `MAX_PRIMITIVES`.
pub const MAX_BYTES: usize = 256 << 20;

/// A scene in the GPU's vertex layout: opaque and see-through triangles apart, the lines, all
/// relative to `origin` (the middle of its box) so f32 keeps the detail of a small part far
/// from zero.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Packed {
    /// The scene this was made from ([`crate::view3d::View3d`]'s scene generation).
    pub id: u64,
    pub origin: [f64; 3],
    /// Half the box's diagonal (m): the depth range is fitted round it.
    pub radius: f64,
    pub opaque: Vec<u8>,
    pub glass: Vec<u8>,
    pub lines: Vec<u8>,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn put3(out: &mut Vec<u8>, v: [f64; 3]) {
    for c in v {
        out.extend_from_slice(&(c as f32).to_le_bytes());
    }
}

impl Packed {
    /// `scene` packed: triangles with no area or a non-finite corner and anything fully
    /// transparent are left out.
    ///
    /// Opaque faces lying in one plane (the section view's caps of nested bodies, the faces two
    /// touching bodies share) are ranked by their runs in it: each time the colour and tag change
    /// within the plane the rank goes up (to [`MAX_RANK`]). The vertex shader moves a higher rank
    /// a little nearer, so the later face is drawn, not whichever the depth buffer's rounding picks.
    ///
    /// ```
    /// use emcraft_render::{Rgb, view3d::Scene};
    /// use emcraft_ui_egui::gpu3d::{Packed, TRI_VERTEX};
    /// let mut s = Scene::default();
    /// s.tri([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], Rgb(200, 0, 0), 255);
    /// s.tri([0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [0.0, 1.0, 1.0], Rgb(0, 200, 0), 100);
    /// s.tri([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], Rgb(0, 0, 0), 255); // no area
    /// s.tri([0.0, 0.0, f64::NAN], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], Rgb(0, 0, 0), 255);
    /// let p = Packed::new(&s, 1);
    /// assert_eq!((p.opaque.len(), p.glass.len()), (3 * TRI_VERTEX, 3 * TRI_VERTEX));
    /// ```
    pub fn new(scene: &Scene, id: u64) -> Packed {
        Packed::with_field(scene, id, None)
    }

    /// As [`Packed::new`], with the opaque triangles tagged `field.0` lying in the plane where
    /// coordinate `field.1` is `field.2` (m) marked as the field's plane: the GPU colours them
    /// from the field's phasors (`gpu3d::field`).
    pub fn with_field(scene: &Scene, id: u64, field: Option<(u32, usize, f64)>) -> Packed {
        let (origin, radius) = match scene.bounds() {
            Some((lo, hi)) => {
                let o = [0.5 * (lo[0] + hi[0]), 0.5 * (lo[1] + hi[1]), 0.5 * (lo[2] + hi[2])];
                (o, 0.5 * dot(sub(hi, lo), sub(hi, lo)).sqrt())
            }
            None => ([0.0; 3], 0.0),
        };
        let (origin, radius) = if origin.iter().all(|v| v.abs() < 1e12) && radius < 1e12 { (origin, radius) } else { ([0.0; 3], 0.0) };
        let mut p = Packed { id, origin, radius, ..Default::default() };
        // Each plane's last group (tag and colour) and its run's rank.
        let mut planes: BTreeMap<[i64; 4], ((u32, [u8; 3]), u32)> = BTreeMap::new();
        // Planes within 10⁻⁵ of the scene apart count as one: the depth buffer cannot tell them
        // apart (a body's cap a rounding below another's), where the CPU's f64 can.
        let step = (radius * 1e-5).max(1e-15);
        for t in scene.tris.iter().filter(|t| t.alpha > 0) {
            if !t.p.iter().flatten().all(|v| v.is_finite()) {
                continue;
            }
            let n = cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0]));
            let len = dot(n, n).sqrt();
            if !(len > 0.0 && len.is_finite()) {
                continue;
            }
            let n = n.map(|c| c / len);
            let on_field = field.is_some_and(|(tag, axis, at)| {
                t.tag == tag
                    && t.alpha == 255
                    && t.p.iter().all(|q| q.get(axis).is_some_and(|c| (c - at).abs() <= 1e-9 * (1.0 + at.abs()) + radius * 1e-7))
            });
            let rank = if on_field {
                -1.0
            } else if t.alpha == 255 {
                // The plane, its normal turned one way.
                let flip = n.iter().find(|c| c.abs() > 1e-9).is_some_and(|c| *c < 0.0);
                let u = if flip { n.map(|c| -c) } else { n };
                let d = dot(u, t.p[0]) / step;
                let q = |v: f64| if v.is_finite() { v.round().clamp(-9e18, 9e18) as i64 } else { 0 };
                let key = [q(u[0] * 1e6), q(u[1] * 1e6), q(u[2] * 1e6), q(d)];
                let g = (t.tag, [t.color.0, t.color.1, t.color.2]);
                let run = planes.entry(key).or_insert((g, 0));
                if run.0 != g {
                    *run = (g, (run.1 + 1).min(MAX_RANK));
                }
                run.1 as f32
            } else {
                0.0
            };
            let out = if t.alpha == 255 { &mut p.opaque } else { &mut p.glass };
            if out.len() + 3 * TRI_VERTEX > MAX_BYTES {
                continue;
            }
            for q in t.p {
                put3(out, sub(q, origin));
                put3(out, n);
                out.extend_from_slice(&rank.to_le_bytes());
                out.extend_from_slice(&[t.color.0, t.color.1, t.color.2, t.alpha]);
                out.extend_from_slice(&t.tag.to_le_bytes());
            }
        }
        for l in scene.lines.iter().filter(|l| l.alpha > 0) {
            if !l.a.iter().chain(&l.b).all(|v| v.is_finite()) || p.lines.len() + LINE > MAX_BYTES {
                continue;
            }
            put3(&mut p.lines, sub(l.a, origin));
            put3(&mut p.lines, sub(l.b, origin));
            p.lines.extend_from_slice(&[l.color.0, l.color.1, l.color.2, l.alpha]);
        }
        p
    }

    pub fn opaque_vertices(&self) -> u32 {
        (self.opaque.len() / TRI_VERTEX) as u32
    }

    pub fn glass_vertices(&self) -> u32 {
        (self.glass.len() / TRI_VERTEX) as u32
    }

    pub fn line_count(&self) -> u32 {
        (self.lines.len() / LINE) as u32
    }
}

/// What a frame draws with, as the shaders read it (`scene.wgsl`'s `U`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Uniforms {
    /// From the scene's origin to clip space (columns).
    pub view_proj: [[f32; 4]; 4],
    /// The camera's forward direction and how far lines move towards the eye (m), so edges on
    /// faces win the depth test.
    pub forward_nudge: [f32; 4],
    /// The light's direction (towards it).
    pub light: [f32; 4],
    /// The target's width and height (pixels), half a line's width (pixels) and the nearest
    /// clip w kept (lines crossing it are cut there).
    pub viewport: [f32; 4],
    /// The ground's colour at the top and the bottom (0–1, the bytes' scale).
    pub top: [f32; 4],
    pub bottom: [f32; 4],
    /// Not in the scene's uniforms: the window depth's terms (perspective d = a + b / z,
    /// orthographic d = a (z − b)), whether it is perspective, and the scene's radius, for the
    /// ambient occlusion's reach.
    pub depth_terms: [f32; 2],
    pub persp: bool,
    pub radius: f32,
}

fn rgba(c: Rgb) -> [f32; 4] {
    [f32::from(c.0) / 255.0, f32::from(c.1) / 255.0, f32::from(c.2) / 255.0, 1.0]
}

impl Uniforms {
    /// The uniforms for `packed` seen by `camera` in a target `size` pixels, with lines
    /// `line_width` pixels wide and the ground from `background.0` (top) to `background.1`.
    ///
    /// The projection is the camera's (`emcraft_render::view3d::Camera::project`): the
    /// same pixel for the same point, with depth fitted to the scene's box.
    pub fn new(packed: &Packed, camera: &Camera, size: [u32; 2], line_width: f64, background: (Rgb, Rgb)) -> Uniforms {
        let c = camera.sanitized();
        let b = c.basis();
        let (w, h) = (f64::from(size[0].max(1)), f64::from(size[1].max(1)));
        let half = (0.5 * c.fov).tan();
        let persp = c.projection == Projection::Perspective;
        let eye = sub(b.eye, packed.origin);
        // The view's depth z runs along `forward` from the eye; the scene lies within `radius`
        // of the origin, the lines up to `nudge` nearer.
        let nudge = c.distance * 4e-3;
        let along = -dot(eye, b.forward);
        let reach = 1.05 * packed.radius.max(c.distance * 1e-6) + 2.0 * nudge;
        let near_min = c.distance * 1e-3;
        let (zn, zf) = if persp { ((along - reach).max(near_min), (along + reach).max(near_min * 2.0)) } else { (along - reach, along + reach) };
        let zf = if zf - zn > 1e-12 * zf.abs().max(1.0) { zf } else { zn + 1e-9_f64.max(zn.abs() * 1e-6) };
        // Rows of the clip transform applied to view coordinates (x, y, z, 1).
        let (sx, sy) = if persp { (h / (w * half), 1.0 / half) } else { (h / (w * c.distance * half), 1.0 / (c.distance * half)) };
        let (rz, rw, depth_terms): ([f64; 4], [f64; 4], [f64; 2]) = if persp {
            let a = zf / (zf - zn);
            ([0.0, 0.0, a, -a * zn], [0.0, 0.0, 1.0, 0.0], [a, -a * zn])
        } else {
            let k = 1.0 / (zf - zn);
            ([0.0, 0.0, k, -k * zn], [0.0, 0.0, 0.0, 1.0], [k, zn])
        };
        let rows = [[sx, 0.0, 0.0, 0.0], [0.0, sy, 0.0, 0.0], rz, rw];
        // View coordinates from the origin-relative world: (right, up, forward) · (p − eye).
        let view = [
            [b.right[0], b.right[1], b.right[2], -dot(b.right, eye)],
            [b.up[0], b.up[1], b.up[2], -dot(b.up, eye)],
            [b.forward[0], b.forward[1], b.forward[2], -dot(b.forward, eye)],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let mut m = [[0f32; 4]; 4];
        for (col, out) in m.iter_mut().enumerate() {
            for (row, v) in out.iter_mut().enumerate() {
                let x = (0..4).map(|k| rows[row][k] * view[k][col]).sum::<f64>() as f32;
                *v = if x.is_finite() { x } else { 0.0 };
            }
        }
        let light = [
            -b.forward[0] + 0.35 * b.up[0] + 0.25 * b.right[0],
            -b.forward[1] + 0.35 * b.up[1] + 0.25 * b.right[1],
            -b.forward[2] + 0.35 * b.up[2] + 0.25 * b.right[2],
        ];
        let len = dot(light, light).sqrt();
        let light = if len > 0.0 { light.map(|v| v / len) } else { [0.0, 0.0, 1.0] };
        let near_w = if persp { zn } else { -1.0 };
        Uniforms {
            view_proj: m,
            forward_nudge: [b.forward[0] as f32, b.forward[1] as f32, b.forward[2] as f32, nudge as f32],
            light: [light[0] as f32, light[1] as f32, light[2] as f32, 0.0],
            viewport: [
                w as f32,
                h as f32,
                (0.5 * if line_width.is_finite() { line_width.clamp(0.5, 16.0) } else { 1.0 }) as f32,
                if near_w.is_finite() { near_w as f32 } else { 0.0 },
            ],
            top: rgba(background.0),
            bottom: rgba(background.1),
            depth_terms: depth_terms.map(|v| if v.is_finite() { v as f32 } else { 0.0 }),
            persp,
            radius: packed.radius as f32,
        }
    }

    /// The bytes the uniform buffer holds.
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(UNIFORM_BYTES);
        for v in
            self.view_proj.iter().flatten().chain(&self.forward_nudge).chain(&self.light).chain(&self.viewport).chain(&self.top).chain(&self.bottom)
        {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// Where the origin-relative point `p` lands: pixels (y down) and depth (0 near, 1 far);
    /// what the vertex shader computes, for checks.
    pub fn project(&self, p: [f32; 3]) -> Option<[f32; 3]> {
        let m = &self.view_proj;
        let clip: [f32; 4] = std::array::from_fn(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r]);
        if !(clip[3] > 0.0) {
            return None;
        }
        let (w, h) = (self.viewport[0], self.viewport[1]);
        Some([(clip[0] / clip[3] + 1.0) * 0.5 * w, (1.0 - clip[1] / clip[3]) * 0.5 * h, clip[2] / clip[3]])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_render::view3d::StandardView;

    fn scene() -> Scene {
        let mut s = Scene::default();
        s.cuboid([0.010, -0.002, 0.003], [0.030, 0.004, 0.020], Rgb(180, 120, 60), 255);
        s
    }

    /// The GPU's transform puts every corner on the camera's pixel, depth inside 0–1 and
    /// ordered as the CPU's depth key, in both projections and from several directions.
    #[test]
    fn the_projection_is_the_cameras() {
        let s = scene();
        let p = Packed::new(&s, 0);
        for projection in [Projection::Perspective, Projection::Orthographic] {
            for view in [StandardView::Iso, StandardView::Front, StandardView::Top, StandardView::Left] {
                let mut cam = Camera { projection, ..Camera::default() };
                cam.standard(view);
                cam.fit([0.010, -0.002, 0.003], [0.030, 0.004, 0.020], 1.5);
                cam.zoom(1.5);
                let u = Uniforms::new(&p, &cam, [600, 400], 1.0, (Rgb::WHITE, Rgb::WHITE));
                let mut keyed = Vec::new();
                for t in &s.tris {
                    for q in t.p {
                        let cpu = cam.project(q, 600.0, 400.0).unwrap();
                        let r = sub(q, p.origin);
                        let gpu = u.project([r[0] as f32, r[1] as f32, r[2] as f32]).unwrap();
                        assert!(
                            (cpu[0] - f64::from(gpu[0])).abs() < 1e-2 && (cpu[1] - f64::from(gpu[1])).abs() < 1e-2,
                            "{view:?} {projection:?}: {cpu:?} {gpu:?}"
                        );
                        assert!((0.0..=1.0).contains(&gpu[2]), "depth {}", gpu[2]);
                        keyed.push((cpu[2], gpu[2]));
                    }
                }
                // Larger CPU key is nearer; smaller GPU depth is nearer.
                for a in &keyed {
                    for b in &keyed {
                        if a.0 > b.0 + 1e-9 * a.0.abs().max(1.0) {
                            assert!(a.1 <= b.1 + 1e-6, "{view:?} {projection:?}: depth order");
                        }
                    }
                }
            }
        }
    }

    /// Coplanar opaque faces rank by their runs in the plane, the later higher; others rank 0.
    #[test]
    fn coplanar_faces_rank_by_order() {
        let mut s = Scene::default();
        let q = |z: f64| [[0.0, 0.0, z], [1.0, 0.0, z], [1.0, 1.0, z], [0.0, 1.0, z]];
        s.tagged(1, |s| s.quad(q(0.0), Rgb(10, 10, 10), 255));
        s.tagged(2, |s| s.quad(q(0.0), Rgb(20, 20, 20), 255));
        s.tagged(1, |s| s.quad(q(0.0), Rgb(10, 10, 10), 255));
        s.tagged(2, |s| s.quad(q(0.5), Rgb(20, 20, 20), 255));
        let p = Packed::new(&s, 0);
        let ranks: Vec<f32> = p.opaque.chunks(TRI_VERTEX).map(|v| f32::from_le_bytes([v[24], v[25], v[26], v[27]])).collect();
        assert_eq!(ranks, [[0.0; 6], [1.0; 6], [2.0; 6], [0.0; 6]].concat());
    }

    #[test]
    fn hostile_cameras_and_scenes_give_finite_uniforms() {
        let mut s = Scene::default();
        s.tri([0.0; 3], [1e300, 0.0, 0.0], [0.0, f64::INFINITY, 0.0], Rgb::WHITE, 255);
        s.line([0.0; 3], [f64::NAN, 0.0, 0.0], Rgb::WHITE, 255);
        let p = Packed::new(&s, 0);
        assert!(p.lines.is_empty());
        let cam = Camera { distance: f64::NAN, fov: -3.0, yaw: f64::INFINITY, ..Camera::default() };
        for size in [[0, 0], [1, 100_000], [4096, 4096]] {
            let u = Uniforms::new(&p, &cam, size, f64::NAN, (Rgb::WHITE, Rgb::WHITE));
            assert!(u.bytes().len() == UNIFORM_BYTES);
            assert!(u.view_proj.iter().flatten().chain(&u.viewport).all(|v| v.is_finite()), "{u:?}");
        }
        assert_eq!(Packed::new(&Scene::default(), 3).radius, 0.0);
    }
}

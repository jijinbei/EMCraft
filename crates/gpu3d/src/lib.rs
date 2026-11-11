//! The 3D view drawn by the GPU (wgpu) with a depth buffer, whatever window shows it: on the
//! window's own device ([`Renderer::prepare`] and [`Renderer::show`] in its render pass — egui's
//! paint callback, iced's shader widget), or on a device of the process's own read back into a
//! picture ([`headless`]: a window that opened with OpenGL, `ui.render`, the scenarios, the start
//! page's pictures). It is the only 3D renderer (docs/adr/0004-gpu-viewport.md); without any
//! adapter the view says so.
//!
//! - Triangles in a depth buffer, flat-shaded (ambient plus Lambert from a light above and right of
//!   the eye, both sides alike), multisampled (4× where the device takes it) and supersampled 2×
//!   at rest when the view is small enough.
//! - Lines as screen-space quads, depth-tested and moved towards the eye so edges lying on faces
//!   show.
//! - See-through faces by weighted blended order-independent transparency (McGuire and Bavoil,
//!   *Weighted Blended Order-Independent Transparency*, JCGT 2(2), 2013), with the stencil
//!   marking where they are nearest so the lines in front of them are drawn over them
//!   ([`renderer`]).
//! - Per pixel ([`gbuffer`]): edges, ambient occlusion, the outline of the selection and of what is
//!   under the pointer, and picking by reading the tags under a pixel back; the field's colour map
//!   from its phasors ([`field`]). Each is a switch (Settings ▸ 3D view ▸ Drawing).
//! - Sections clip the uncut scene in WGSL; CPU cuts remain for interaction and mesh slice lines.
//! - Port direction glyphs are generated as cylinders and cones in WGSL.
//!   Field-vector arrows use the same procedural geometry, clipped with the field.
//!
//! Nothing here panics: every device call that can fail runs under error scopes or the device's
//! error handler, which marks the window's view failed (it draws offscreen from then on).
//!
//! The renderer's design follows SolidCraft's viewport (MIT OR Apache-2.0, ATTRIBUTION.md):
//! the offscreen frame shown by a paint callback, OIT targets and composite; the shaders and
//! this code are written for EMCraft's scene.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod field;
mod gbuffer;
pub mod headless;
mod pack;
mod renderer;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use emcraft_render::view3d::Scene;
pub use wgpu;

pub use field::{FieldFrame, FieldGrid};
pub use pack::{Arrow, LINE, MAX_BYTES, Motion, Packed, TRI_VERTEX, UNIFORM_BYTES, Uniforms};
pub use renderer::{Caps, Renderer};

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Gpu3dError {
    #[error("no GPU adapter: {0}")]
    NoAdapter(String),
    #[error("the GPU device could not be opened: {0}")]
    Device(String),
    #[error("the 3D view's GPU pipelines could not be made: {0}")]
    Pipeline(String),
    #[error("the GPU could not draw the 3D view: {0}")]
    Frame(String),
}

/// A future that is ready at once (wgpu's native calls), or `None`.
pub fn ready<T>(f: impl std::future::Future<Output = T>) -> Option<T> {
    let mut f = std::pin::pin!(f);
    match f.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
        std::task::Poll::Ready(v) => Some(v),
        std::task::Poll::Pending => None,
    }
}

/// What the 3D view draws beyond the plain picture, each switched on its own (Settings ▸ 3D view ▸
/// Drawing).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Look {
    /// Edges where the surface creases or ends, found per pixel from depth and normals.
    pub edges: bool,
    /// Ambient occlusion: creases and corners darkened by what is near them.
    pub ao: bool,
    /// A ring round what is selected and what is under the pointer.
    pub outline: bool,
    /// The tags under each pixel kept for picking ([`pick`]).
    pub pick: bool,
}

/// Most selected parts the outline rings (of a larger box selection, the first).
pub const MAX_OUTLINED: usize = 8;

/// One frame of the view.
#[derive(Clone, Debug)]
pub struct Frame {
    pub packed: Arc<Packed>,
    pub uniforms: Uniforms,
    /// The offscreen frame's size (pixels): the view's times `supersample`.
    pub size: [u32; 2],
    /// The view's size in the window (pixels).
    pub out: [u32; 2],
    pub supersample: u32,
    pub look: Look,
    /// The tags outlined: the selection's (the first [`MAX_OUTLINED`]) and the one under the
    /// pointer (0 none).
    pub selected: Vec<u32>,
    pub hover: u32,
    /// The outlines' colours (0–1, with their opacity).
    pub outline: [f32; 4],
    pub hover_color: [f32; 4],
    /// The window's pixels per point (the occlusion's radius and the outline's width follow it).
    pub ppp: f32,
    /// The field's plane coloured by the GPU (the scene marks it, [`Packed::with_field`]).
    pub field: Option<FieldFrame>,
}

impl Frame {
    /// A frame of `packed` with `uniforms`, nothing beyond the plain picture drawn.
    pub fn plain(packed: Arc<Packed>, uniforms: Uniforms, size: [u32; 2], out: [u32; 2], supersample: u32) -> Frame {
        Frame {
            packed,
            uniforms,
            size,
            out,
            supersample,
            look: Look::default(),
            selected: Vec::new(),
            hover: 0,
            outline: [0.0; 4],
            hover_color: [0.0; 4],
            ppp: 1.0,
            field: None,
        }
    }

    /// Tells this frame from another: the scene, the camera and the size, which the per-pixel
    /// tags read back for picking ([`pick`]) were drawn for.
    pub fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.packed.id.hash(&mut h);
        self.uniforms.bytes().hash(&mut h);
        self.out.hash(&mut h);
        if let Some(f) = &self.field {
            let sc = &f.scale;
            (f.grid.id, f.phase.to_bits(), sc.peak.to_bits(), sc.signed, sc.db.map(f64::to_bits), f.contours).hash(&mut h);
        }
        h.finish()
    }

    /// The show pass's uniform (`show.wgsl`'s `Show`).
    pub fn show_bytes(&self, srgb: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(128);
        let f = |out: &mut Vec<u8>, v: f32| out.extend_from_slice(&v.to_le_bytes());
        let look = self.look;
        let flags = u32::from(look.edges) | (u32::from(look.ao) << 1) | (u32::from(look.outline) << 2);
        for v in [self.out[0] as f32, self.out[1] as f32, self.supersample.max(1) as f32, if srgb { 1.0 } else { 0.0 }] {
            f(&mut out, v);
        }
        let selected: Vec<u32> = self.selected.iter().copied().filter(|t| *t != 0).take(MAX_OUTLINED).collect();
        for v in [flags, selected.len() as u32, self.hover] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        f(&mut out, if self.uniforms.persp { 1.0 } else { 0.0 });
        for v in [self.uniforms.depth_terms[0], self.uniforms.depth_terms[1], 0.0, 0.0] {
            f(&mut out, v);
        }
        for v in self.outline.iter().chain(&self.hover_color) {
            f(&mut out, *v);
        }
        for v in [12.0 * self.ppp.max(1.0), 0.25 * self.uniforms.radius.max(1e-9), 1.6, 0.0] {
            f(&mut out, v);
        }
        for k in 0..MAX_OUTLINED {
            out.extend_from_slice(&selected.get(k).copied().unwrap_or(0).to_le_bytes());
        }
        out
    }
}

/// The scene packed for the GPU, made again when the scene changes.
#[derive(Default)]
pub struct Cache {
    /// The scene's generation and field plane it was packed for, and the packing.
    packed: Option<((u64, Option<(u32, usize, f64)>), Arc<Packed>)>,
}

/// A new scene's id, unique in the process: the renderers keep a scene's buffers by it.
pub fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Cache {
    /// `scene` packed, from the cache when `generation` (the scene's) is unchanged.
    pub fn packed(&mut self, scene: &Scene, generation: u64) -> Arc<Packed> {
        self.packed_with(scene, generation, None)
    }

    /// As [`Cache::packed`], the field's plane marked ([`Packed::with_field`]).
    pub fn packed_with(&mut self, scene: &Scene, generation: u64, field: Option<(u32, usize, f64)>) -> Arc<Packed> {
        self.packed_arrows(scene, generation, field, &[])
    }

    /// As [`Cache::packed_with`], with the field's arrows.
    pub fn packed_arrows(&mut self, scene: &Scene, generation: u64, field: Option<(u32, usize, f64)>, arrows: &[Arrow]) -> Arc<Packed> {
        match &self.packed {
            Some((k, p)) if *k == (generation, field) => p.clone(),
            _ => {
                let mut p = Packed::with_field(scene, next_id(), field);
                p.append_arrows(arrows);
                let p = Arc::new(p);
                self.packed = Some(((generation, field), p.clone()));
                p
            }
        }
    }
}

/// A picture of `scene` through `camera`, `size` pixels, on the ground `background` (top,
/// bottom), drawn offscreen on the process's own device and supersampled 2× (the start page's
/// and the template cards' pictures).
pub fn scene_picture(
    scene: &Scene,
    camera: &emcraft_render::view3d::Camera,
    size: [u32; 2],
    background: (emcraft_render::Rgb, emcraft_render::Rgb),
) -> Result<emcraft_render::Canvas, Gpu3dError> {
    let out = [size[0].clamp(1, 2048), size[1].clamp(1, 2048)];
    let packed = Arc::new(Packed::new(scene, next_id()));
    let big = [out[0] * 2, out[1] * 2];
    let uniforms = Uniforms::new(&packed, camera, big, 4.0, background);
    picture(0, &Frame { look: Look { edges: true, ..Look::default() }, ..Frame::plain(packed, uniforms, big, out, 2) })
}

/// A picture of `frame` drawn offscreen for view `client` (any id from [`next_id`]; 0 for the
/// one-off pictures) on the process's own device ([`headless::shared`]).
pub fn picture(client: u64, frame: &Frame) -> Result<emcraft_render::Canvas, Gpu3dError> {
    headless::shared()?.picture(client, frame)
}

#[cfg(test)]
mod tests;

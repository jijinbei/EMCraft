//! The 3D view drawn by the window's GPU (wgpu, through an `egui_wgpu` paint callback) with a
//! depth buffer: the same [`Scene`] the CPU renderer (`emcraft_render::view3d`) draws, shaded
//! and projected the same way, every frame, still or moving (docs/adr/0004-gpu-viewport.md).
//!
//! - Triangles in a depth buffer, multisampled (4× where the device takes it) and supersampled
//!   2× at rest when the view is small enough, as the CPU's picture.
//! - Lines as screen-space quads of the CPU's width, depth-tested and moved towards the eye as
//!   the CPU moves them, so edges lying on faces show.
//! - See-through faces by weighted blended order-independent transparency (McGuire and Bavoil,
//!   *Weighted Blended Order-Independent Transparency*, JCGT 2(2), 2013), with the stencil
//!   marking where they are nearest so the lines in front of them are drawn over them
//!   ([`renderer`]).
//! - The selection's and the hovered row's tints and the section view's cut and caps are in the
//!   scene itself (`scene3d`, `emcraft_render::view3d::section`), which picking reads too, so
//!   both renderers show them alike.
//!
//! The CPU renderer stays the reference: headless pictures (`ui.render`, the scenarios) and
//! windows without a wgpu device draw with it. A window has the GPU's view only when
//! [`install`] succeeded on its context ([`available`]); any GPU error afterwards marks it
//! failed and the view goes back to the CPU from the next frame. Nothing here panics: every
//! device call that can fail runs under error scopes or the device's error handler.
//!
//! The renderer's design follows SolidCraft's viewport (MIT OR Apache-2.0, ATTRIBUTION.md):
//! the offscreen frame shown by a paint callback, OIT targets and composite; the shaders and
//! this code are written for EMCraft's scene.

mod gbuffer;
mod pack;
mod renderer;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use eframe::{egui_wgpu, wgpu};
use emcraft_render::view3d::Scene;

pub use pack::{LINE, MAX_BYTES, Packed, TRI_VERTEX, UNIFORM_BYTES, Uniforms};
pub use renderer::Caps;
use renderer::Renderer;

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
pub(crate) fn ready<T>(f: impl std::future::Future<Output = T>) -> Option<T> {
    let mut f = std::pin::pin!(f);
    match f.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
        std::task::Poll::Ready(v) => Some(v),
        std::task::Poll::Pending => None,
    }
}

/// The window's GPU, as its 3D view uses it: kept in the window's context data by [`install`].
#[derive(Clone)]
pub struct Available {
    /// The adapter's name and backend.
    pub adapter: String,
    pub caps: Caps,
    failed: Arc<AtomicBool>,
    rs: egui_wgpu::RenderState,
}

impl std::fmt::Debug for Available {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Available").field("adapter", &self.adapter).field("caps", &self.caps).field("failed", &self.failed()).finish()
    }
}

/// The tags under window pixel `px` of the frame `key` the GPU drew last: the nearest tagged
/// triangle's and the nearest solid one's (opacity ≥ 200), 0 for none, as
/// `emcraft_render::view3d::hits` finds them. `None` where the GPU cannot tell (no GPU view, the
/// frame drawn last was another, picking off): the caller asks the CPU.
pub fn pick(ctx: &egui::Context, key: u64, px: [u32; 2]) -> Option<(u32, u32)> {
    let a = available(ctx)?;
    let r = a.rs.renderer.read();
    let i = r.callback_resources.get::<Installed>()?;
    i.renderer.tags_at(&a.rs.device, &a.rs.queue, key, px)
}

impl Available {
    /// Whether a GPU error has stopped it (the view draws on the CPU from then on).
    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
}

fn key() -> egui::Id {
    egui::Id::new("emcraft.gpu3d")
}

/// The window's GPU for the 3D view, if [`install`] made it and it has not failed.
pub fn available(ctx: &egui::Context) -> Option<Available> {
    ctx.data(|d| d.get_temp::<Available>(key())).filter(|a| !a.failed())
}

/// Makes the 3D view's pipelines on the window's device and records them in `ctx`. GPU errors
/// nothing else catches are reported (once each, the first few) and stop the GPU's view rather
/// than panicking, as wgpu's default handler would.
pub fn install(rs: &egui_wgpu::RenderState, ctx: &egui::Context) -> Result<Available, Gpu3dError> {
    let failed = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(AtomicU32::new(0));
    let flag = failed.clone();
    rs.device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
        flag.store(true, Ordering::Release);
        if seen.fetch_add(1, Ordering::Relaxed) < 4 {
            eprintln!("emcraft: GPU error, the 3D view draws on the CPU: {e}");
        }
    }));
    let caps = Caps::of(&rs.adapter, &rs.device);
    let r = Renderer::new(&rs.device, caps, rs.target_format)?;
    let info = rs.adapter.get_info();
    let a = Available { adapter: format!("{} ({:?})", info.name, info.backend), caps, failed: failed.clone(), rs: rs.clone() };
    rs.renderer.write().callback_resources.insert(Installed { renderer: r, failed });
    ctx.data_mut(|d| d.insert_temp(key(), a.clone()));
    Ok(a)
}

/// What the callback finds in egui's resources.
struct Installed {
    renderer: Renderer,
    failed: Arc<AtomicBool>,
}

/// What the GPU's view draws beyond the CPU's picture, each switched on its own (Settings ▸
/// Drawing): all off draws what the CPU draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
}

impl Frame {
    /// A frame of `packed` with `uniforms`, nothing beyond the CPU's picture drawn.
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
    packed: Option<Arc<Packed>>,
}

impl Cache {
    /// `scene` packed, from the cache when `id` (the scene's generation) is unchanged.
    pub fn packed(&mut self, scene: &Scene, id: u64) -> Arc<Packed> {
        match &self.packed {
            Some(p) if p.id == id => p.clone(),
            _ => {
                let p = Arc::new(Packed::new(scene, id));
                self.packed = Some(p.clone());
                p
            }
        }
    }
}

/// Paints `frame` into `rect` with the window's GPU.
pub fn paint(p: &egui::Painter, rect: egui::Rect, frame: Frame) {
    p.add(egui_wgpu::Callback::new_paint_callback(rect, Callback(frame)));
}

struct Callback(Frame);

impl egui_wgpu::CallbackTrait for Callback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(i) = resources.get_mut::<Installed>()
            && !i.failed.load(Ordering::Acquire)
            && let Err(e) = i.renderer.prepare(device, queue, encoder, &self.0)
        {
            eprintln!("emcraft: {e}; the 3D view draws on the CPU");
            i.failed.store(true, Ordering::Release);
        }
        Vec::new()
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, resources: &egui_wgpu::CallbackResources) {
        if let Some(i) = resources.get::<Installed>()
            && !i.failed.load(Ordering::Acquire)
        {
            i.renderer.show(pass);
        }
    }
}

/// A device of its own, without a window: the first adapter wgpu offers on `backends`.
pub fn headless(backends: wgpu::Backends) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), Gpu3dError> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends, ..wgpu::InstanceDescriptor::new_without_display_handle() });
    let adapter = ready(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .ok_or_else(|| Gpu3dError::NoAdapter("the request did not finish".into()))?
        .map_err(|e| Gpu3dError::NoAdapter(e.to_string()))?;
    let (device, queue) = ready(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("emcraft_view3d_headless"),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .ok_or_else(|| Gpu3dError::Device("the request did not finish".into()))?
    .map_err(|e| Gpu3dError::Device(e.to_string()))?;
    device.on_uncaptured_error(Arc::new(|e: wgpu::Error| eprintln!("emcraft: GPU error: {e}")));
    Ok((adapter, device, queue))
}

#[cfg(test)]
mod tests;

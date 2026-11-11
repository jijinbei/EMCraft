//! The 3D view on egui's window ([`emcraft_gpu3d`] draws it): the renderer made on the window's
//! device and kept in egui's paint-callback resources, each frame drawn by a paint callback, and
//! the tags under a pixel read back for picking — the offscreen device's where the window has none.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use eframe::egui_wgpu;

pub use emcraft_gpu3d::*;

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
pub fn pick(ctx: &egui::Context, client: u64, key: u64, px: [u32; 2]) -> Option<(u32, u32)> {
    match available(ctx) {
        Some(a) => {
            let r = a.rs.renderer.read();
            let i = r.callback_resources.get::<Installed>()?;
            i.renderer.tags_at(&a.rs.device, &a.rs.queue, key, px)
        }
        None => headless::shared().ok()?.tags_at(client, key, px),
    }
}

impl Available {
    /// Whether a GPU error has stopped it (the view draws offscreen from then on).
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
            eprintln!("emcraft: GPU error, the 3D view draws offscreen: {e}");
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
            eprintln!("emcraft: {e}; the 3D view draws offscreen");
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A context nothing was installed in (the headless harness, `ui.render`) has no window GPU.
    #[test]
    fn a_context_without_a_window_gpu_has_none() {
        assert!(available(&egui::Context::default()).is_none());
    }

    /// The window's GPU where it has one, else the offscreen device, else nothing (a message).
    #[test]
    fn the_drawer_falls_back_without_a_window_gpu() {
        use crate::view3d::Drawer;
        assert_eq!(Drawer::choose(true, true), Drawer::Window);
        assert_eq!(Drawer::choose(true, false), Drawer::Window);
        assert_eq!(Drawer::choose(false, true), Drawer::Offscreen);
        assert_eq!(Drawer::choose(false, false), Drawer::None);
    }
}

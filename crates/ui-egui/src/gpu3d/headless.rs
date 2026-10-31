//! Pictures of the 3D view without a window: the same renderer on a device of its own, one per
//! process, read back as RGBA. The window draws with it when it has no wgpu device of its own
//! (it opened with OpenGL), and so do the headless pictures (`ui.render`, the scenarios, the start
//! page's thumbnails). Any adapter does: a GPU, or a software one (Mesa's lavapipe or llvmpipe on
//! Linux, WARP on Windows), asked for when no other is found.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use eframe::wgpu;

use super::renderer::{Caps, FRAME, Renderer};
use super::{Frame, Gpu3dError, ready};

/// Most views drawing offscreen at once, each with its renderer (its targets and the tags of its
/// last picture); the one used longest ago gives way.
const MAX_CLIENTS: usize = 8;

/// The process's offscreen device and a renderer for each view drawing on it.
pub struct Offscreen {
    /// The adapter's name and backend.
    pub adapter: String,
    device: wgpu::Device,
    queue: wgpu::Queue,
    caps: Caps,
    /// (the view, when it last drew, its renderer): the list locked only to find a view's
    /// renderer, which is locked on its own while it draws, so views draw side by side.
    renderers: Mutex<Vec<(u64, u64, Arc<Mutex<Renderer>>)>>,
    failed: Arc<AtomicBool>,
}

/// A device of its own, without a window: the adapter wgpu offers on `backends` (`WGPU_BACKEND`
/// narrows them), a software one if asked (`fallback`) or if no other is found.
pub fn device(backends: wgpu::Backends, fallback: bool) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), Gpu3dError> {
    let backends = wgpu::Backends::from_env().map_or(backends, |b| b & backends);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends, ..wgpu::InstanceDescriptor::new_without_display_handle() });
    let request = |force_fallback_adapter| {
        ready(instance.request_adapter(&wgpu::RequestAdapterOptions { force_fallback_adapter, ..Default::default() }))
            .ok_or_else(|| Gpu3dError::NoAdapter("the request did not finish".into()))?
            .map_err(|e| Gpu3dError::NoAdapter(e.to_string()))
    };
    let adapter = match request(fallback) {
        Ok(a) => a,
        Err(e) if !fallback => request(true).map_err(|_| e)?,
        Err(e) => return Err(e),
    };
    let (device, queue) = ready(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("emcraft_view3d_offscreen"),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .ok_or_else(|| Gpu3dError::Device("the request did not finish".into()))?
    .map_err(|e| Gpu3dError::Device(e.to_string()))?;
    Ok((adapter, device, queue))
}

impl Offscreen {
    fn open() -> Result<Offscreen, Gpu3dError> {
        let fallback = std::env::var_os("EMCRAFT_GPU_FALLBACK").is_some();
        let (adapter, device, queue) = device(wgpu::Backends::all(), fallback)?;
        let failed = Arc::new(AtomicBool::new(false));
        let flag = failed.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            if !flag.swap(true, Ordering::AcqRel) {
                eprintln!("emcraft: GPU error in the offscreen 3D view: {e}");
            }
        }));
        let caps = Caps::of(&adapter, &device);
        // The pipelines are checked once here, so a device that cannot make them is no device.
        let first = Renderer::new(&device, caps, FRAME)?;
        let info = adapter.get_info();
        let adapter = format!("{} ({:?})", info.name, info.backend);
        Ok(Offscreen { adapter, device, queue, caps, renderers: Mutex::new(vec![(0, 0, Arc::new(Mutex::new(first)))]), failed })
    }

    /// Runs `with` on view `client`'s renderer, made when it has none.
    fn with<T>(&self, client: u64, make: bool, with: impl FnOnce(&mut Renderer) -> Option<T>) -> Option<T> {
        static CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let renderer = {
            let mut all = self.renderers.lock().ok()?;
            let now = CLOCK.fetch_add(1, Ordering::Relaxed);
            let i = match all.iter().position(|(c, _, _)| *c == client) {
                Some(i) => i,
                None if make => {
                    let r = Renderer::new(&self.device, self.caps, FRAME).ok()?;
                    if all.len() >= MAX_CLIENTS
                        && let Some(old) = all.iter().enumerate().min_by_key(|(_, (_, used, _))| *used).map(|(i, _)| i)
                    {
                        all.swap_remove(old);
                    }
                    all.push((client, now, Arc::new(Mutex::new(r))));
                    all.len() - 1
                }
                None => return None,
            };
            let entry = all.get_mut(i)?;
            entry.1 = now;
            entry.2.clone()
        };
        let mut r = renderer.lock().ok()?;
        with(&mut r)
    }

    /// `frame` drawn for view `client` and read back: RGBA rows of `frame.out`.
    pub fn picture(&self, client: u64, frame: &Frame) -> Result<emcraft_render::Canvas, Gpu3dError> {
        if self.failed.load(Ordering::Acquire) {
            return Err(Gpu3dError::Frame("an earlier GPU error stopped the offscreen view".into()));
        }
        let rgba = self
            .with(client, true, |r| Some(draw(&self.device, &self.queue, r, frame)))
            .ok_or_else(|| Gpu3dError::Frame("no renderer for the picture".into()))??;
        if self.failed.load(Ordering::Acquire) {
            return Err(Gpu3dError::Frame("the GPU refused the picture".into()));
        }
        Ok(emcraft_render::Canvas { w: frame.out[0] as usize, h: frame.out[1] as usize, rgba })
    }

    /// The tags under pixel `px` of view `client`'s picture drawn last, if it was `key`
    /// (`super::pick`).
    pub fn tags_at(&self, client: u64, key: u64, px: [u32; 2]) -> Option<(u32, u32)> {
        self.with(client, false, |r| r.tags_at(&self.device, &self.queue, key, px))
    }
}

/// The process's offscreen device, opened on first use.
pub fn shared() -> Result<&'static Offscreen, Gpu3dError> {
    static SHARED: OnceLock<Result<Offscreen, Gpu3dError>> = OnceLock::new();
    SHARED.get_or_init(Offscreen::open).as_ref().map_err(Clone::clone)
}

/// `frame` drawn by `r` on `device` and read back (RGBA rows of `frame.out`).
pub fn draw(device: &wgpu::Device, queue: &wgpu::Queue, r: &mut Renderer, frame: &Frame) -> Result<Vec<u8>, Gpu3dError> {
    let [w, h] = frame.out;
    let side = r.caps().max_side;
    if w == 0 || h == 0 || w > side || h > side {
        return Err(Gpu3dError::Frame(format!("a picture is 1 to {side} pixels a side, not {w} × {h}")));
    }
    let row = (4 * w).div_ceil(256) * 256;
    let made = super::renderer::scoped(device, || {
        let out = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("emcraft_view3d_picture"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FRAME,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("emcraft_view3d_picture"),
            size: u64::from(row) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        (out, read)
    })
    .map_err(Gpu3dError::Frame)?;
    let (out, read) = made;
    let view = out.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("emcraft_view3d_picture") });
    r.prepare(device, queue, &mut encoder, frame)?;
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("emcraft_view3d_picture"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        r.show(&mut pass);
    }
    encoder.copy_texture_to_buffer(
        out.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &read,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    let submission = queue.submit([encoder.finish()]);
    let slice = read.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r.is_ok());
    });
    device
        .poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: Some(std::time::Duration::from_secs(10)) })
        .map_err(|e| Gpu3dError::Frame(format!("waiting for the picture: {e}")))?;
    if !rx.try_recv().unwrap_or(false) {
        return Err(Gpu3dError::Frame("the picture could not be read back".into()));
    }
    let data = slice.get_mapped_range().map_err(|e| Gpu3dError::Frame(format!("reading the picture back: {e}")))?;
    let mut rgba = Vec::with_capacity(4 * (w * h) as usize);
    for y in 0..h as usize {
        let start = y * row as usize;
        rgba.extend_from_slice(data.get(start..start + 4 * w as usize).ok_or_else(|| Gpu3dError::Frame("a short read back".into()))?);
    }
    drop(data);
    read.unmap();
    Ok(rgba)
}

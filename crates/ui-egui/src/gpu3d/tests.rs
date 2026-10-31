use std::sync::Arc;

use eframe::wgpu;
use emcraft_render::Rgb;
use emcraft_render::view3d::{self, Camera, Projection, Scene, StandardView};

use super::renderer::{FRAME, Renderer};
use super::*;

/// Without any backend there is no adapter: an error, never a panic.
#[test]
fn no_adapter_is_an_error_not_a_panic() {
    match headless(wgpu::Backends::empty()) {
        Err(Gpu3dError::NoAdapter(_)) => {}
        Err(e) => panic!("unexpected: {e}"),
        Ok(_) => panic!("an adapter with no backend"),
    }
    // A context nothing was installed in (the headless harness, `ui.render`) has no GPU view.
    assert!(available(&egui::Context::default()).is_none());
}

fn scene() -> Scene {
    let mut s = Scene::default();
    s.cuboid([-0.010, -0.006, -0.004], [0.010, 0.006, 0.004], Rgb(200, 60, 40), 255);
    // A see-through pane in front of the left half, and one behind.
    s.quad([[-0.014, -0.008, 0.008], [0.0, -0.008, 0.008], [0.0, 0.008, 0.008], [-0.014, 0.008, 0.008]], Rgb(40, 160, 220), 110);
    s.quad([[0.0, -0.008, -0.008], [0.014, -0.008, -0.008], [0.014, 0.008, -0.008], [0.0, 0.008, -0.008]], Rgb(40, 200, 80), 110);
    // The box's front edges, lying on its faces, and a line in front of the pane.
    let (lo, hi) = ([-0.010, -0.006, 0.004], [0.010, 0.006, 0.004]);
    for (a, b) in
        [([lo[0], lo[1]], [hi[0], lo[1]]), ([hi[0], lo[1]], [hi[0], hi[1]]), ([hi[0], hi[1]], [lo[0], hi[1]]), ([lo[0], hi[1]], [lo[0], lo[1]])]
    {
        s.line([a[0], a[1], lo[2]], [b[0], b[1], lo[2]], Rgb(20, 20, 20), 255);
    }
    s.line([-0.012, 0.0, 0.010], [-0.002, 0.0, 0.010], Rgb(250, 250, 0), 255);
    s
}

/// The GPU's view only where the window has one; without, the CPU's picture at rest and the
/// painter while moving, as Quality.
#[test]
fn the_drawer_falls_back_without_a_gpu_view() {
    use crate::prefs::Drawing3d::{Cpu, Fast, Gpu};
    use crate::view3d::Drawer;
    let cases = [
        (Gpu, true, false, Drawer::Gpu),
        (Gpu, true, true, Drawer::Gpu),
        (Gpu, false, false, Drawer::Cpu),
        (Gpu, false, true, Drawer::Painter),
        (Cpu, true, false, Drawer::Cpu),
        (Cpu, true, true, Drawer::Painter),
        (Fast, true, false, Drawer::Painter),
        (Fast, false, false, Drawer::Painter),
    ];
    for (pref, gpu, moving, want) in cases {
        assert_eq!(Drawer::choose(pref, gpu, moving), want, "{pref:?} gpu {gpu} moving {moving}");
    }
}

/// The frame drawn by the GPU and read back as RGBA rows.
fn gpu_picture(device: &wgpu::Device, queue: &wgpu::Queue, r: &mut Renderer, frame: &Frame) -> Vec<u8> {
    let [w, h] = frame.out;
    let out = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FRAME,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = out.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    r.prepare(device, queue, &mut encoder, frame).unwrap();
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
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
    let row = (4 * w).div_ceil(256) * 256;
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
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
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: None }).unwrap();
    let data = slice.get_mapped_range().unwrap();
    (0..h as usize).flat_map(|y| data[y * row as usize..y * row as usize + 4 * w as usize].to_vec()).collect()
}

/// A headless device, or `None` (printed) on a machine without an adapter; EMCRAFT_REQUIRE_GPU
/// makes that a failure.
fn device() -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    match headless(wgpu::Backends::all()) {
        Ok(d) => Some(d),
        Err(e) if std::env::var_os("EMCRAFT_REQUIRE_GPU").is_none() => {
            eprintln!("skipping: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

/// `s` drawn by both renderers through `cam`, at one and two samples a side: the share of pixels
/// that differ by more than 24 in a channel (asserted under `share`) and the mean difference
/// (under `mean`).
fn compare(device: &wgpu::Device, queue: &wgpu::Queue, r: &mut Renderer, s: &Scene, id: u64, cam: &Camera, name: &str, limits: (f64, f64)) {
    let packed = Arc::new(Packed::new(s, id));
    let (w, h) = (240u32, 160u32);
    let ground = (Rgb(235, 238, 242), Rgb(200, 205, 214));
    for supersample in [1, 2] {
        let uniforms = Uniforms::new(&packed, cam, [w * supersample, h * supersample], f64::from(supersample), ground);
        let frame = Frame { packed: packed.clone(), uniforms, size: [w * supersample, h * supersample], out: [w, h], supersample };
        let gpu = gpu_picture(device, queue, r, &frame);
        let opt = view3d::Options { width: w as usize, height: h as usize, supersample: supersample as usize, background: ground, line_width: 1.0 };
        let cpu = view3d::render(s, cam, &opt).rgba;
        assert_eq!(gpu.len(), cpu.len());
        let differ = gpu.chunks(4).zip(cpu.chunks(4)).filter(|(g, c)| (0..3).any(|i| g[i].abs_diff(c[i]) > 24)).count();
        let mean = gpu.chunks(4).zip(cpu.chunks(4)).map(|(g, c)| (0..3).map(|i| f64::from(g[i].abs_diff(c[i]))).sum::<f64>()).sum::<f64>()
            / f64::from(3 * w * h);
        eprintln!("{name} ×{supersample}: {differ} of {} pixels differ, mean {mean:.2}", w * h);
        if std::env::var_os("EMCRAFT_GPU_SHOTS").is_some() {
            let dir = std::env::temp_dir();
            let png = |rgba: &[u8]| emcraft_render::Canvas { w: w as usize, h: h as usize, rgba: rgba.to_vec() }.png().unwrap_or_default();
            let _ = std::fs::write(dir.join(format!("gpu3d-{name}-{supersample}-gpu.png")), png(&gpu));
            let _ = std::fs::write(dir.join(format!("gpu3d-{name}-{supersample}-cpu.png")), png(&cpu));
        }
        assert!((differ as f64) < limits.0 * f64::from(w * h), "{name} ×{supersample}: {differ} pixels differ");
        assert!(mean < limits.1, "{name} ×{supersample}: mean difference {mean}");
    }
}

/// The GPU draws what the CPU renderer draws: the same scene from several views, with opaque and
/// see-through faces and edges, differs only along edges (multisampled there) and where
/// see-through faces overlap (averaged rather than sorted). Skips on a machine without an
/// adapter (set EMCRAFT_REQUIRE_GPU to fail instead).
#[test]
fn the_gpu_draws_what_the_cpu_draws() {
    let Some((adapter, device, queue)) = device() else { return };
    eprintln!("adapter: {:?}", adapter.get_info());
    let mut r = Renderer::new(&device, Caps::of(&adapter, &device), FRAME).unwrap();
    let s = scene();
    for (k, (view, projection)) in
        [(StandardView::Front, Projection::Perspective), (StandardView::Iso, Projection::Perspective), (StandardView::Iso, Projection::Orthographic)]
            .into_iter()
            .enumerate()
    {
        let mut cam = Camera { projection, ..Camera::default() };
        cam.standard(view);
        cam.fit([-0.014, -0.008, -0.008], [0.014, 0.008, 0.010], 1.5);
        compare(&device, &queue, &mut r, &s, 1, &cam, &format!("view{k}"), (0.03, 2.0));
    }
}

/// A section view of bodies inside a region (the high-pass filter's sections in its guide): the
/// caps lie in one plane over each other, and the later is drawn, as on the CPU, not whichever
/// the depth buffer's rounding picks.
#[test]
fn coplanar_section_caps_are_drawn_as_on_the_cpu() {
    let Some((adapter, device, queue)) = device() else { return };
    let mut r = Renderer::new(&device, Caps::of(&adapter, &device), FRAME).unwrap();
    let mut s = Scene::default();
    s.tagged(1, |s| s.cuboid([-0.012, -0.005, -0.025], [0.012, 0.005, 0.025], Rgb(175, 200, 232), 255));
    s.tagged(2, |s| s.cuboid([-0.012, -0.005, -0.025], [0.012, 0.005, -0.010], Rgb(214, 150, 60), 255));
    s.tagged(3, |s| s.cuboid([-0.012, -0.005, 0.010], [0.012, 0.005, 0.025], Rgb(214, 150, 60), 255));
    let cut = view3d::section::section(&s, &view3d::Section::across(1, 0.0, view3d::Keep::Low)).unwrap();
    let mut cam = Camera::default();
    cam.standard(StandardView::Iso);
    cam.fit([-0.012, -0.005, -0.025], [0.012, 0.005, 0.025], 1.5);
    // Without the ranks, about 2.4 % of the pixels differ (the region's cap through the others').
    compare(&device, &queue, &mut r, &cut, 2, &cam, "section", (0.015, 0.8));
}

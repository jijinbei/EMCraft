use std::sync::Arc;

use eframe::wgpu;
use emcraft_render::Rgb;
use emcraft_render::view3d::{self, Camera, Projection, Scene, StandardView};

use super::renderer::{FRAME, Renderer};
use super::*;

/// Every shader parses and validates (naga, as wgpu compiles them), with the entry points the
/// pipelines name, on any machine: no GPU needed.
#[test]
fn every_shader_validates() {
    use super::renderer::{COMPOSITE_WGSL, SCENE_WGSL, SHOW_WGSL};
    let shaders = [
        ("scene.wgsl", SCENE_WGSL, &["vs_ground", "fs_ground", "vs_tri", "fs_opaque", "fs_blend", "fs_oit", "fs_none", "vs_line", "fs_line"][..]),
        ("composite.wgsl", COMPOSITE_WGSL, &["vs", "fs_composite"][..]),
        ("show.wgsl", SHOW_WGSL, &["vs", "fs_show"][..]),
    ];
    for (name, src, entries) in shaders {
        let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
        let have: Vec<&str> = module.entry_points.iter().map(|e| e.name.as_str()).collect();
        for e in entries {
            assert!(have.contains(e), "{name}: no entry point {e} (has {have:?})");
        }
    }
    // The shaders sit beside this module; a new one must join the list above.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/gpu3d");
    let mut files: Vec<String> =
        std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|n| n.ends_with(".wgsl")).collect();
    files.sort();
    assert_eq!(files, ["composite.wgsl", "full.wgsl", "scene.wgsl", "show.wgsl"], "a WGSL file not validated here");
}

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
        let frame = Frame::plain(packed.clone(), uniforms, [w * supersample, h * supersample], [w, h], supersample);
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

/// A box seen from the iso view, a second box through it and a see-through pane in front.
fn tagged_scene() -> (Scene, Camera) {
    let mut s = Scene::default();
    s.tagged(1 << 16, |s| s.cuboid([-0.010, -0.006, -0.004], [0.010, 0.006, 0.004], Rgb(200, 60, 40), 255));
    s.tagged((2 << 16) | 3, |s| s.cuboid([0.002, -0.009, -0.002], [0.006, 0.009, 0.002], Rgb(60, 160, 60), 255));
    s.tagged((2 << 16) | 4, |s| {
        s.quad([[-0.014, -0.008, 0.008], [0.0, -0.008, 0.008], [0.0, 0.008, 0.008], [-0.014, 0.008, 0.008]], Rgb(40, 160, 220), 110)
    });
    let mut cam = Camera::default();
    cam.standard(StandardView::Iso);
    cam.fit([-0.014, -0.009, -0.004], [0.014, 0.009, 0.008], 1.5);
    (s, cam)
}

/// What the GPU finds under a pixel is what the CPU's rays find: the nearest tagged triangle and
/// the nearest solid one, untagged ones hiding nothing; they differ only on triangles' edges.
#[test]
fn gpu_picking_agrees_with_the_cpus_rays() {
    let Some((adapter, device, queue)) = device() else { return };
    let mut r = Renderer::new(&device, Caps::of(&adapter, &device), FRAME).unwrap();
    let (mut s, cam) = tagged_scene();
    // An untagged sheet in front of everything: the CPU's rays go through it.
    s.quad([[-0.02, -0.02, 0.012], [0.02, -0.02, 0.012], [0.02, 0.02, 0.012], [-0.02, 0.02, 0.012]], Rgb(250, 250, 250), 255);
    let packed = Arc::new(Packed::new(&s, 5));
    let (w, h) = (180u32, 120u32);
    let uniforms = Uniforms::new(&packed, &cam, [w, h], 1.0, (Rgb::WHITE, Rgb::WHITE));
    let frame = Frame { look: Look { pick: true, ..Look::default() }, ..Frame::plain(packed, uniforms, [w, h], [w, h], 1) };
    let _ = gpu_picture(&device, &queue, &mut r, &frame);
    let (mut same, mut differ, mut hit) = (0, 0, 0);
    for y in (1..h).step_by(3) {
        for x in (1..w).step_by(3) {
            let gpu = r.tags_at(&device, &queue, frame.key(), [x, y]).unwrap();
            let cpu = view3d::hits(&s, &cam, f64::from(x) + 0.5, f64::from(y) + 0.5, f64::from(w), f64::from(h))
                .map_or((0, 0), |(any, solid)| (any.tag, solid.map_or(0, |h| h.tag)));
            if gpu == cpu {
                same += 1;
            } else {
                differ += 1;
            }
            if cpu.0 != 0 {
                hit += 1;
            }
        }
    }
    eprintln!("picking: {same} agree, {differ} differ, {hit} on something");
    assert!(hit > 200, "the scene fills the view");
    assert!(differ * 100 <= (same + differ) * 2, "{differ} of {} differ", same + differ);
    // Another frame's key reads nothing: the caller asks the CPU.
    assert!(r.tags_at(&device, &queue, frame.key() ^ 1, [10, 10]).is_none());
}

/// Each addition changes the picture only where it should: edges darken the box's outline, the
/// occlusion darkens and never lightens, the outline rings the selected body in its colour.
#[test]
fn the_gpu_additions_draw_where_they_should() {
    let Some((adapter, device, queue)) = device() else { return };
    let mut r = Renderer::new(&device, Caps::of(&adapter, &device), FRAME).unwrap();
    let (s, cam) = tagged_scene();
    let packed = Arc::new(Packed::new(&s, 6));
    let (w, h) = (240u32, 160u32);
    let uniforms = Uniforms::new(&packed, &cam, [w, h], 1.0, (Rgb::WHITE, Rgb::WHITE));
    let plain = Frame::plain(packed, uniforms, [w, h], [w, h], 1);
    let base = gpu_picture(&device, &queue, &mut r, &plain);
    let with = |look: Look, selected: u32| Frame { look, selected: vec![selected], outline: [1.0, 0.0, 1.0, 1.0], ..plain.clone() };
    let lum = |p: &[u8]| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]);
    let diff = |a: &[u8], b: &[u8]| a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x[..3] != y[..3]).count();
    let edges = gpu_picture(&device, &queue, &mut r, &with(Look { edges: true, ..Look::default() }, 0));
    let darker = edges.chunks(4).zip(base.chunks(4)).filter(|(e, b)| lum(e) + 30 < lum(b)).count();
    let lighter = edges.chunks(4).zip(base.chunks(4)).filter(|(e, b)| lum(e) > lum(b) + 3).count();
    eprintln!("edges: {darker} darker, {lighter} lighter");
    assert!(darker > 200 && darker < (w * h) as usize / 5 && lighter == 0);
    let ao = gpu_picture(&device, &queue, &mut r, &with(Look { ao: true, ..Look::default() }, 0));
    let lighter = ao.chunks(4).zip(base.chunks(4)).filter(|(e, b)| lum(e) > lum(b) + 3).count();
    eprintln!("occlusion: {} changed, {lighter} lighter", diff(&ao, &base));
    assert!(diff(&ao, &base) > 50);
    assert_eq!(lighter, 0);
    let ring = gpu_picture(&device, &queue, &mut r, &with(Look { outline: true, ..Look::default() }, (2 << 16) | 3));
    let magenta = ring.chunks(4).filter(|p| p[0] > 240 && p[1] < 20 && p[2] > 240).count();
    eprintln!("outline: {magenta} magenta");
    assert!(magenta > 100, "{magenta}");
    let none = gpu_picture(&device, &queue, &mut r, &with(Look { outline: true, ..Look::default() }, 0));
    assert_eq!(diff(&none, &base), 0, "no selection, no outline");
    if std::env::var_os("EMCRAFT_GPU_SHOTS").is_some() {
        for (name, rgba) in [("base", &base), ("edges", &edges), ("ao", &ao), ("ring", &ring)] {
            let png = emcraft_render::Canvas { w: w as usize, h: h as usize, rgba: rgba.clone() }.png().unwrap_or_default();
            let _ = std::fs::write(std::env::temp_dir().join(format!("gpu3d-look-{name}.png")), png);
        }
    }
}

/// The field plane coloured by the GPU from its phasors is the CPU's cell by cell colouring
/// (scene3d::field: each cell the mean of its corners, cells with a corner outside left out),
/// at several phases, for a component and for a length.
#[test]
fn the_gpu_field_is_the_cpus_cells() {
    let Some((adapter, device, queue)) = device() else { return };
    let mut r = Renderer::new(&device, Caps::of(&adapter, &device), FRAME).unwrap();
    let (nx, ny) = (37usize, 23usize);
    // A standing wave across, travelling along, with a hole (outside the mesh).
    let phasors: Vec<Option<([f64; 3], [f64; 3])>> = (0..nx * ny)
        .map(|k| {
            let (i, j) = ((k % nx) as f64 / (nx - 1) as f64, (k / nx) as f64 / (ny - 1) as f64);
            if (i - 0.7).abs() < 0.08 && (j - 0.4).abs() < 0.15 {
                return None;
            }
            let a = (std::f64::consts::PI * j).sin() * 120.0;
            let th = 2.0 * std::f64::consts::PI * i * 1.5;
            Some(([0.3 * a, a * th.cos(), 0.0], [0.0, a * th.sin(), 0.2 * a]))
        })
        .collect();
    let (c, u, v) = ([-0.02, 0.0, -0.01], [0.02, 0.0, -0.01], [-0.02, 0.0, 0.01]);
    for axis in [1u32, 3] {
        let grid = Arc::new(FieldGrid::new(axis as u64 + 40, nx, ny, axis, &phasors, c, u, v));
        for phase in [0.0, 1.1, 2.5] {
            let peak = grid.peak(phase);
            let (s_, c_) = phase.sin_cos();
            let value = |i: usize, j: usize| grid.value(j * nx + i, c_, s_);
            let color =
                |x: f64| if axis < 3 { emcraft_render::colormap::diverging(x / peak) } else { emcraft_render::colormap::sequential(x / peak) };
            let at = |i: usize, j: usize| {
                let (a, b) = (i as f64 / (nx - 1) as f64, j as f64 / (ny - 1) as f64);
                [c[0] + (u[0] - c[0]) * a + (v[0] - c[0]) * b, c[1], c[2] + (u[2] - c[2]) * a + (v[2] - c[2]) * b]
            };
            let mut cpu = Scene::default();
            let step = grid.step;
            let mut j = 0;
            while j + 1 < ny {
                let j1 = (j + step).min(ny - 1);
                let mut i = 0;
                while i + 1 < nx {
                    let i1 = (i + step).min(nx - 1);
                    let corners = [value(i, j), value(i1, j), value(i1, j1), value(i, j1)];
                    if corners.iter().all(|x| x.is_finite()) {
                        let mean = corners.iter().sum::<f64>() / 4.0;
                        cpu.tagged(9 << 16, |s| s.quad([at(i, j), at(i1, j), at(i1, j1), at(i, j1)], color(mean), 255));
                    }
                    i = i1;
                }
                j = j1;
            }
            let mut gpu = Scene::default();
            let far = [u[0] + v[0] - c[0], 0.0, u[2] + v[2] - c[2]];
            gpu.tagged(9 << 16, |s| s.quad([c, u, far, v], Rgb(128, 128, 128), 255));
            let mut cam = Camera::default();
            cam.standard(StandardView::Top);
            cam.fit([-0.02, 0.0, -0.01], [0.02, 0.0, 0.01], 1.5);
            let (w, h) = (240u32, 160u32);
            let ground = (Rgb(235, 238, 242), Rgb(200, 205, 214));
            let packed = Arc::new(Packed::with_field(&gpu, 100 + axis as u64, Some((9 << 16, 1, 0.0))));
            let uniforms = Uniforms::new(&packed, &cam, [w, h], 1.0, ground);
            let frame = Frame { field: Some(FieldFrame { grid: grid.clone(), phase, peak }), ..Frame::plain(packed, uniforms, [w, h], [w, h], 1) };
            let g = gpu_picture(&device, &queue, &mut r, &frame);
            let opt = view3d::Options { width: w as usize, height: h as usize, supersample: 1, background: ground, line_width: 1.0 };
            let cpu = view3d::render(&cpu, &cam, &opt).rgba;
            let differ = g.chunks(4).zip(cpu.chunks(4)).filter(|(a, b)| (0..3).any(|k| a[k].abs_diff(b[k]) > 6)).count();
            eprintln!("field axis {axis} phase {phase}: {differ} of {} differ", w * h);
            if std::env::var_os("EMCRAFT_GPU_SHOTS").is_some() {
                let png = |rgba: &[u8]| emcraft_render::Canvas { w: w as usize, h: h as usize, rgba: rgba.to_vec() }.png().unwrap_or_default();
                let _ = std::fs::write(std::env::temp_dir().join(format!("gpu3d-field-{axis}-gpu.png")), png(&g));
                let _ = std::fs::write(std::env::temp_dir().join(format!("gpu3d-field-{axis}-cpu.png")), png(&cpu));
            }
            assert!(differ * 100 < (w * h) as usize * 2, "axis {axis} phase {phase}: {differ} differ");
        }
    }
}

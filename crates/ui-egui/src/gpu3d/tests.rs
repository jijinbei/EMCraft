//! The GPU renderer against known answers: faces at known places and colours, read back pixel by
//! pixel and compared with the shading formula, the order of their depths, the transparency's
//! blend, the colour map of known values and rays met analytically. They need an adapter: a GPU,
//! or a software one (Mesa's lavapipe or llvmpipe, WARP; `EMCRAFT_GPU_FALLBACK=1` asks for it).
//! Without any they skip, unless `EMCRAFT_REQUIRE_GPU` is set.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use eframe::wgpu;
use emcraft_render::Rgb;
use emcraft_render::view3d::{Camera, Projection, Scene, StandardView};

use super::renderer::{FRAME, Renderer};
use super::*;

/// Every shader parses and validates (naga, as wgpu compiles them), with the entry points the
/// pipelines name, on any machine: no GPU needed.
#[test]
fn every_shader_validates() {
    use super::renderer::{COMPOSITE_WGSL, SCENE_WGSL, SHOW_WGSL};
    let shaders = [
        (
            "scene.wgsl",
            SCENE_WGSL,
            &[
                "vs_ground",
                "fs_ground",
                "vs_tri",
                "fs_opaque",
                "fs_blend",
                "fs_oit",
                "fs_none",
                "vs_line",
                "fs_line",
                "vs_g",
                "fs_g_normals",
                "fs_g_any",
                "fs_g_solid",
            ][..],
        ),
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
    match headless::device(wgpu::Backends::empty(), false) {
        Err(Gpu3dError::NoAdapter(_)) => {}
        Err(e) => panic!("unexpected: {e}"),
        Ok(_) => panic!("an adapter with no backend"),
    }
    // A context nothing was installed in (the headless harness, `ui.render`) has no window GPU.
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

/// A renderer on a device of the test's own.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    r: Renderer,
}

fn gpu() -> Option<Gpu> {
    let fallback = std::env::var_os("EMCRAFT_GPU_FALLBACK").is_some();
    match headless::device(wgpu::Backends::all(), fallback) {
        Ok((adapter, device, queue)) => {
            eprintln!("adapter: {} ({:?})", adapter.get_info().name, adapter.get_info().backend);
            let r = Renderer::new(&device, Caps::of(&adapter, &device), FRAME).unwrap();
            Some(Gpu { device, queue, r })
        }
        Err(e) if std::env::var_os("EMCRAFT_REQUIRE_GPU").is_none() => {
            eprintln!("skipping: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

/// A picture read back, with the camera it was drawn through.
struct Shot {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
    cam: Camera,
    /// The frame's key ([`Frame::key`]).
    key: u64,
}

impl Shot {
    fn px(&self, x: u32, y: u32) -> [f64; 3] {
        let i = 4 * (y * self.w + x) as usize;
        [f64::from(self.rgba[i]), f64::from(self.rgba[i + 1]), f64::from(self.rgba[i + 2])]
    }

    /// The pixel world point `p` lands on.
    fn at(&self, p: [f64; 3]) -> [f64; 3] {
        let q = self.cam.project(p, f64::from(self.w), f64::from(self.h)).unwrap();
        self.px(q[0].floor() as u32, q[1].floor() as u32)
    }

    fn png(&self, name: &str) {
        if std::env::var_os("EMCRAFT_GPU_SHOTS").is_some() {
            let png = emcraft_render::Canvas { w: self.w as usize, h: self.h as usize, rgba: self.rgba.clone() }.png().unwrap_or_default();
            let _ = std::fs::write(std::env::temp_dir().join(format!("gpu3d-{name}.png")), png);
        }
    }
}

static IDS: AtomicU64 = AtomicU64::new(1);

impl Gpu {
    /// `s` drawn through `cam` on a black ground, `edit` changing the frame first.
    fn shot(&mut self, s: &Scene, cam: Camera, size: [u32; 2], field: Option<(u32, usize, f64)>, edit: impl FnOnce(&mut Frame)) -> Shot {
        let packed = Arc::new(Packed::with_field(s, IDS.fetch_add(1, Ordering::Relaxed), field));
        let uniforms = Uniforms::new(&packed, &cam, size, 1.0, (Rgb(0, 0, 0), Rgb(0, 0, 0)));
        let mut frame = Frame::plain(packed, uniforms, size, size, 1);
        edit(&mut frame);
        let rgba = headless::draw(&self.device, &self.queue, &mut self.r, &frame).unwrap();
        Shot { w: size[0], h: size[1], rgba, cam, key: frame.key() }
    }
}

/// The shading formula: the colour times 0.42 + 0.58 |n · l|, the light above and right of the
/// eye (both sides lit alike).
fn lit(c: Rgb, n: [f64; 3], cam: &Camera) -> [f64; 3] {
    let b = cam.basis();
    let l: [f64; 3] = std::array::from_fn(|k| -b.forward[k] + 0.35 * b.up[k] + 0.25 * b.right[k]);
    let len = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
    let k = 0.42 + 0.58 * ((n[0] * l[0] + n[1] * l[1] + n[2] * l[2]) / len).abs();
    [f64::from(c.0) * k, f64::from(c.1) * k, f64::from(c.2) * k]
}

fn mix(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    std::array::from_fn(|k| a[k] * (1.0 - t) + b[k] * t)
}

#[track_caller]
fn near(got: [f64; 3], want: [f64; 3], tol: f64, what: &str) {
    assert!((0..3).all(|k| (got[k] - want[k]).abs() <= tol), "{what}: got {got:?}, want {want:?} (±{tol})");
}

/// A rectangle across z at `z`: x in `x`, y in `y`.
fn rect(s: &mut Scene, x: (f64, f64), y: (f64, f64), z: f64, c: Rgb, alpha: u8) {
    s.quad([[x.0, y.0, z], [x.1, y.0, z], [x.1, y.1, z], [x.0, y.1, z]], c, alpha);
}

fn front(projection: Projection) -> Camera {
    let mut cam = Camera { projection, ..Camera::default() };
    cam.standard(StandardView::Front);
    cam.fit([-1.0, -1.0, -0.5], [1.0, 1.0, 0.5], 1.5);
    cam
}

const Z: [f64; 3] = [0.0, 0.0, 1.0];

/// Nearer faces hide farther ones whatever their order in the scene, each shaded by the formula;
/// the ground runs from its top colour to its bottom one.
#[test]
fn faces_are_drawn_nearest_first_and_shaded() {
    let Some(mut g) = gpu() else { return };
    let mut s = Scene::default();
    let (red, green, blue) = (Rgb(220, 40, 30), Rgb(30, 200, 60), Rgb(40, 60, 210));
    rect(&mut s, (-0.5, 0.5), (-0.6, 0.6), 0.5, green, 255);
    rect(&mut s, (-1.0, 0.0), (-0.8, 0.8), 0.0, red, 255);
    rect(&mut s, (-1.2, 1.2), (-1.0, 1.0), -0.5, blue, 255);
    for projection in [Projection::Orthographic, Projection::Perspective] {
        let cam = front(projection);
        let shot = g.shot(&s, cam, [240, 160], None, |_| {});
        shot.png(&format!("depth-{projection:?}"));
        near(shot.at([-0.75, 0.0, 0.0]), lit(red, Z, &cam), 2.0, "red over blue");
        near(shot.at([-0.25, 0.0, 0.5]), lit(green, Z, &cam), 2.0, "green over red");
        near(shot.at([0.25, 0.0, 0.5]), lit(green, Z, &cam), 2.0, "green over blue");
        near(shot.at([0.9, 0.0, -0.5]), lit(blue, Z, &cam), 2.0, "blue alone");
    }
    // The ground: top colour on the first row, bottom colour on the last.
    let packed = Arc::new(Packed::new(&Scene::default(), 999));
    let uniforms = Uniforms::new(&packed, &front(Projection::Perspective), [64, 32], 1.0, (Rgb(10, 20, 30), Rgb(200, 100, 50)));
    let rgba = headless::draw(&g.device, &g.queue, &mut g.r, &Frame::plain(packed, uniforms, [64, 32], [64, 32], 1)).unwrap();
    let at = |y: usize| [f64::from(rgba[4 * 64 * y]), f64::from(rgba[4 * 64 * y + 1]), f64::from(rgba[4 * 64 * y + 2])];
    near(at(0), [10.0, 20.0, 30.0], 1.0, "ground top");
    near(at(31), [200.0, 100.0, 50.0], 1.0, "ground bottom");
}

/// One see-through layer blends exactly by its opacity; two layers of one colour cover
/// 1 − (1 − a)² of what is behind, whatever their order (weighted blended OIT).
#[test]
fn see_through_faces_blend_by_their_opacity() {
    let Some(mut g) = gpu() else { return };
    let (white, red, green) = (Rgb(200, 200, 200), Rgb(230, 30, 30), Rgb(30, 220, 60));
    let (a1, a2) = (128u8, 100u8);
    let mut s = Scene::default();
    rect(&mut s, (-1.0, 1.0), (-1.0, 1.0), 0.0, white, 255);
    rect(&mut s, (-1.0, 0.0), (-1.0, 1.0), 0.5, red, a1);
    rect(&mut s, (0.0, 1.0), (-1.0, 1.0), 0.6, green, a2);
    rect(&mut s, (0.0, 1.0), (-1.0, 1.0), 0.3, green, a2);
    let cam = front(Projection::Orthographic);
    let shot = g.shot(&s, cam, [240, 160], None, |_| {});
    shot.png("oit");
    let a = f64::from(a1) / 255.0;
    near(shot.at([-0.5, 0.0, 0.5]), mix(lit(white, Z, &cam), lit(red, Z, &cam), a), 3.0, "one layer");
    let b = f64::from(a2) / 255.0;
    let cover = 1.0 - (1.0 - b) * (1.0 - b);
    near(shot.at([0.5, 0.0, 0.6]), mix(lit(white, Z, &cam), lit(green, Z, &cam), cover), 3.0, "two layers");
}

/// A line in front of a see-through face is drawn over it at full strength; one behind shows
/// through it, blended by the face's opacity.
#[test]
fn lines_in_front_of_glass_stay_whole() {
    let Some(mut g) = gpu() else { return };
    let (grey, glass, yellow, cyan) = (Rgb(90, 90, 90), Rgb(40, 120, 220), Rgb(250, 230, 20), Rgb(20, 230, 230));
    let alpha = 110u8;
    let mut s = Scene::default();
    rect(&mut s, (-1.0, 1.0), (-1.0, 1.0), 0.0, grey, 255);
    rect(&mut s, (-1.0, 1.0), (-1.0, 1.0), 0.5, glass, alpha);
    s.line([-0.8, 0.3, 0.7], [0.8, 0.3, 0.7], yellow, 255);
    s.line([-0.8, -0.3, 0.2], [0.8, -0.3, 0.2], cyan, 255);
    let cam = front(Projection::Orthographic);
    let packed = Arc::new(Packed::new(&s, IDS.fetch_add(1, Ordering::Relaxed)));
    let uniforms = Uniforms::new(&packed, &cam, [240, 160], 5.0, (Rgb(0, 0, 0), Rgb(0, 0, 0)));
    let rgba = headless::draw(&g.device, &g.queue, &mut g.r, &Frame::plain(packed, uniforms, [240, 160], [240, 160], 1)).unwrap();
    let shot = Shot { w: 240, h: 160, rgba, cam, key: 0 };
    shot.png("lines");
    let a = f64::from(alpha) / 255.0;
    near(shot.at([0.0, 0.3, 0.7]), [250.0, 230.0, 20.0], 3.0, "line in front");
    near(shot.at([0.0, -0.3, 0.2]), mix([20.0, 230.0, 230.0], lit(glass, Z, &cam), a), 3.0, "line behind");
}

/// A section view of bodies inside a region: the caps lie in one plane over each other, and the
/// later (the inner body's) is drawn, not whichever the depth buffer's rounding picks.
#[test]
fn coplanar_caps_show_the_later_body() {
    let Some(mut g) = gpu() else { return };
    let (region, inner) = (Rgb(175, 200, 232), Rgb(214, 150, 60));
    let mut s = Scene::default();
    s.tagged(1, |s| s.cuboid([-0.012, -0.005, -0.025], [0.012, 0.005, 0.025], region, 255));
    s.tagged(2, |s| s.cuboid([-0.012, -0.005, -0.025], [0.012, 0.005, -0.010], inner, 255));
    s.tagged(3, |s| s.cuboid([-0.012, -0.005, 0.010], [0.012, 0.005, 0.025], inner, 255));
    let cut =
        emcraft_render::view3d::section::section(&s, &emcraft_render::view3d::Section::across(1, 0.0, emcraft_render::view3d::Keep::Low)).unwrap();
    let mut cam = Camera::default();
    cam.standard(StandardView::Top);
    cam.fit([-0.012, -0.005, -0.025], [0.012, 0.005, 0.025], 1.5);
    let shot = g.shot(&cut, cam, [240, 160], None, |_| {});
    shot.png("caps");
    let y = [0.0, 1.0, 0.0];
    for z in [-0.020, -0.015, 0.012, 0.022] {
        for x in [-0.008, 0.0, 0.008] {
            near(shot.at([x, 0.0, z]), lit(inner, y, &cam), 2.0, "the inner body's cap");
        }
    }
    near(shot.at([0.0, 0.0, 0.0]), lit(region, y, &cam), 2.0, "the region's cap");
}

/// The tags under a pixel are those of the triangles its ray meets first, worked out
/// analytically: the nearest tagged one and the nearest solid (opacity ≥ 200) one; untagged
/// faces hide nothing.
#[test]
fn picking_finds_what_the_ray_meets() {
    let Some(mut g) = gpu() else { return };
    // (x, y, z, tag, alpha)
    let rects: [((f64, f64), (f64, f64), f64, u32, u8); 5] = [
        ((-1.0, 1.0), (-1.0, 1.0), -0.4, 1 << 16, 255),
        ((-0.6, 0.2), (-0.5, 0.7), 0.0, (2 << 16) | 3, 255),
        ((0.0, 0.9), (-0.8, 0.1), 0.2, (2 << 16) | 4, 255),
        ((-0.9, 0.0), (-0.9, 0.0), 0.4, (2 << 16) | 5, 100),
        ((-1.5, 1.5), (-1.5, 1.5), 0.45, 0, 255),
    ];
    let mut s = Scene::default();
    for (x, y, z, tag, alpha) in rects {
        s.tagged(tag, |s| rect(s, x, y, z, Rgb(120, 120, 120), alpha));
    }
    let cam = front(Projection::Orthographic);
    let (w, h) = (180u32, 120u32);
    let shot = g.shot(&s, cam, [w, h], None, |f| f.look.pick = true);
    let key = shot.key;
    let px = cam.pixel_size(f64::from(h));
    let (mut checked, mut hits) = (0, 0);
    for y in (1..h).step_by(3) {
        for x in (1..w).step_by(3) {
            let (o, d) = cam.ray(f64::from(x) + 0.5, f64::from(y) + 0.5, f64::from(w), f64::from(h)).unwrap();
            let (mut any, mut solid): (Option<(f64, u32)>, Option<(f64, u32)>) = (None, None);
            let mut edge = false;
            for (rx, ry, z, tag, alpha) in rects {
                let t = (z - o[2]) / d[2];
                let (px_, py_) = (o[0] + d[0] * t, o[1] + d[1] * t);
                let margin = (px_ - rx.0).abs().min((px_ - rx.1).abs()).min((py_ - ry.0).abs()).min((py_ - ry.1).abs());
                edge |= margin < 2.0 * px;
                if tag == 0 || !(t > 0.0 && px_ > rx.0 && px_ < rx.1 && py_ > ry.0 && py_ < ry.1) {
                    continue;
                }
                if any.is_none_or(|(b, _)| t < b) {
                    any = Some((t, tag));
                }
                if alpha >= 200 && solid.is_none_or(|(b, _)| t < b) {
                    solid = Some((t, tag));
                }
            }
            if edge {
                continue;
            }
            let want = (any.map_or(0, |a| a.1), solid.map_or(0, |a| a.1));
            assert_eq!(g.r.tags_at(&g.device, &g.queue, key, [x, y]), Some(want), "pixel {x}, {y}");
            checked += 1;
            hits += usize::from(want.0 != 0);
        }
    }
    assert!(checked > 1000 && hits > 200, "{checked} checked, {hits} on something");
    // Another frame's key reads nothing: the caller asks the CPU's rays.
    assert!(g.r.tags_at(&g.device, &g.queue, key ^ 1, [10, 10]).is_none());
}

/// The field's plane coloured from known values: each cell the colour map of the mean of its
/// corners over the largest value, at phase 0 from the real parts and at a quarter period from
/// the imaginary ones; a cell with a corner outside the mesh left out.
#[test]
fn the_field_is_coloured_by_its_values() {
    let Some(mut g) = gpu() else { return };
    let (nx, ny) = (5usize, 3usize);
    for (quarter, axis) in [(false, 0u32), (true, 0), (false, 3)] {
        let phasors: Vec<Option<([f64; 3], [f64; 3])>> = (0..nx * ny)
            .map(|k| {
                let i = (k % nx) as f64;
                let v = 10.0 * i;
                if k == nx * ny - 1 {
                    return None;
                }
                // Re(F e^{jφ}) = re cos φ − im sin φ: at φ = π/2 the value is −im.
                Some(if quarter { ([0.0; 3], [-v, 0.0, 0.0]) } else { ([v, 0.0, 0.0], [0.0; 3]) })
            })
            .collect();
        let (c, u, v) = ([-1.0, 0.0, -0.6], [1.0, 0.0, -0.6], [-1.0, 0.0, 0.6]);
        let grid = Arc::new(FieldGrid::new(50 + u64::from(axis) + u64::from(quarter), nx, ny, axis, &phasors, c, u, v));
        let phase = if quarter { std::f64::consts::FRAC_PI_2 } else { 0.0 };
        let peak = grid.peak(phase);
        assert!((peak - 40.0).abs() < 1e-9);
        let tag = 9 << 16;
        let mut s = Scene::default();
        s.tagged(tag, |s| s.quad([c, u, [1.0, 0.0, 0.6], v], Rgb(128, 128, 128), 255));
        let mut cam = Camera { projection: Projection::Orthographic, ..Camera::default() };
        cam.standard(StandardView::Top);
        cam.fit([-1.0, 0.0, -0.6], [1.0, 0.0, 0.6], 1.5);
        let shot = g.shot(&s, cam, [240, 160], Some((tag, 1, 0.0)), |f| f.field = Some(FieldFrame { grid: grid.clone(), phase, peak }));
        shot.png(&format!("field-{axis}-{quarter}"));
        let at = |i: f64, j: f64| [c[0] + (u[0] - c[0]) * i / 4.0, 0.0, c[2] + (v[2] - c[2]) * j / 2.0];
        for i in 0..4 {
            let mean = 10.0 * i as f64 + 5.0;
            let map = if axis < 3 { emcraft_render::colormap::diverging(mean / peak) } else { emcraft_render::colormap::sequential(mean / peak) };
            near(shot.at(at(i as f64 + 0.5, 0.5)), lit(map, [0.0, 1.0, 0.0], &cam), 3.0, &format!("cell {i}, 0"));
        }
        // The cell with the corner outside: the ground.
        near(shot.at(at(3.5, 1.5)), [0.0, 0.0, 0.0], 1.0, "the cell left out");
    }
}

/// Multisampling smooths a slanted edge; the edges, the occlusion and the outline change the
/// picture only where they should.
#[test]
fn smoothing_and_the_additions() {
    let Some(mut g) = gpu() else { return };
    let mut s = Scene::default();
    s.tagged(1 << 16, |s| s.cuboid([-0.010, -0.006, -0.004], [0.010, 0.006, 0.004], Rgb(200, 60, 40), 255));
    s.tagged((2 << 16) | 3, |s| s.cuboid([0.002, -0.009, -0.002], [0.006, 0.009, 0.002], Rgb(60, 160, 60), 255));
    let mut cam = Camera::default();
    cam.standard(StandardView::Iso);
    cam.fit([-0.010, -0.009, -0.004], [0.010, 0.009, 0.004], 1.5);
    let size = [240, 160];
    let base = g.shot(&s, cam, size, None, |_| {});
    // Between the ground (black) and the faces, some pixels on the slanted silhouette are mixed.
    let mixed = base.rgba.chunks(4).filter(|p| p[0] > 8 && p[0] < 150 && p[1] < 50).count();
    assert!(mixed > 30, "{mixed} smoothed pixels");
    let lum = |p: &[u8]| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]);
    let compare = |a: &Shot| {
        let darker = a.rgba.chunks(4).zip(base.rgba.chunks(4)).filter(|(e, b)| lum(e) + 30 < lum(b)).count();
        let lighter = a.rgba.chunks(4).zip(base.rgba.chunks(4)).filter(|(e, b)| lum(e) > lum(b) + 3).count();
        (darker, lighter)
    };
    let edges = g.shot(&s, cam, size, None, |f| f.look.edges = true);
    let (darker, lighter) = compare(&edges);
    assert!(darker > 200 && darker < 240 * 160 / 5 && lighter == 0, "edges: {darker} darker, {lighter} lighter");
    let ao = g.shot(&s, cam, size, None, |f| f.look.ao = true);
    let (_, lighter) = compare(&ao);
    assert_eq!(lighter, 0, "occlusion only darkens");
    let ring = g.shot(&s, cam, size, None, |f| {
        f.look.outline = true;
        f.selected = vec![(2 << 16) | 3];
        f.outline = [1.0, 0.0, 1.0, 1.0];
    });
    let magenta = ring.rgba.chunks(4).filter(|p| p[0] > 240 && p[1] < 20 && p[2] > 240).count();
    assert!(magenta > 100, "{magenta} outline pixels");
    let none = g.shot(&s, cam, size, None, |f| f.look.outline = true);
    assert_eq!(none.rgba, base.rgba, "no selection, no outline");
    edges.png("edges");
    ring.png("ring");
}

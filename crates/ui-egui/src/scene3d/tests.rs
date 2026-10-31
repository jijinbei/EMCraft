//! The 3D scene's tests: every overlay of the samples builds, faces are told apart, ports
//! land where they belong.

use super::style::{sc, wall_style};
use super::surface::{Kind, Surface};
use super::*;
use serde_json::json;

#[test]
fn every_overlay_of_every_sample_builds() {
    for name in ["highpass", "coaxial-line", "dipole", "loaded-sphere"] {
        let mut s = Session::new();
        let script: serde_json::Value = serde_json::from_str(emcraft_engine::templates::find(name).unwrap().script).unwrap();
        // Without the solves: geometry, then the mesh.
        let steps: Vec<serde_json::Value> =
            script.as_array().unwrap().iter().filter(|c| !c["command"].as_str().unwrap().starts_with("solve.")).cloned().collect();
        let unmeshed: Vec<serde_json::Value> = steps.iter().filter(|c| c["command"] != json!("mesh.generate")).cloned().collect();
        // Meshing the open and curved samples takes seconds: their meshes are drawn in the
        // timing report and the scenarios.
        let scripts = if name == "highpass" { vec![unmeshed, steps] } else { vec![unmeshed] };
        for script in scripts {
            s.run_script(&json!(script)).unwrap();
            for o in [Overlay::Plain, Overlay::Materials, Overlay::Ports, Overlay::Boundaries, Overlay::Mesh] {
                let b = build(&s, o);
                assert!(!b.scene.tris.is_empty(), "{name} {o:?}");
                assert!(b.bounds.is_some());
            }
            s = Session::new();
        }
    }
}

#[test]
fn air_and_walls_let_what_is_inside_show() {
    let kinds = |s: &Session| {
        let mesh = s.model().mesh.as_ref().unwrap();
        let surface = Surface::of(s.model(), mesh);
        let count = |k: Kind| (0..mesh.boundary.len()).filter(|&i| surface.kind(i) == k).count();
        (count(Kind::Air), count(Kind::Wall), count(Kind::Conductor))
    };
    // An open guide: the air box around it, the guide's metal inside.
    let open = meshed("open-waveguide", Some(12.0));
    let (air, wall, metal) = kinds(&open);
    assert!(air > 0 && wall == 0 && metal > 0, "{air} {wall} {metal}");
    // A coaxial line: its outer wall (curved), its inner conductor apart from it (the ends'
    // ports join them). 4 mm cells, for speed: the conductors' faces are there whatever the size.
    let (air, wall, metal) = kinds(&meshed("coaxial-line", Some(4.0)));
    assert!(air == 0 && wall > 0 && metal > 0, "{air} {wall} {metal}");
    // Air is all but invisible; the conductors inside stay solid.
    let m = open.model();
    assert!(wall_style(m, Overlay::Plain, false, 0, Kind::Air).1 < 40);
    assert_eq!(wall_style(m, Overlay::Plain, false, 0, Kind::Conductor).1, 255);
    assert!(wall_style(m, Overlay::Plain, false, 0, Kind::Wall).1 < 255);
}

#[test]
fn a_port_on_a_face_inside_leaves_the_end_alone() {
    // The open guide's feed lies on a face inside the box: no end of the box is a port.
    let s = meshed("open-waveguide", Some(12.0));
    let b = build(&s, Overlay::Ports);
    assert!(b.scene.tris.iter().filter(|t| t.color == sc().port).count() <= 4, "the box's end is drawn as a port");
}

fn meshed(name: &str, size: Option<f64>) -> Session {
    let script: serde_json::Value = serde_json::from_str(emcraft_engine::templates::find(name).unwrap().script).unwrap();
    let steps: Vec<serde_json::Value> = script
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| !c["command"].as_str().unwrap().starts_with("solve."))
        .map(|c| match (size, c["command"].as_str()) {
            (Some(h), Some("mesh.generate")) => json!({"command": "mesh.generate", "params": {"size": h}}),
            _ => c.clone(),
        })
        .collect();
    let mut s = Session::new();
    s.run_script(&json!(steps)).unwrap();
    s
}

/// Milliseconds per picture of the samples' meshes at the window's 3D view size (about
/// 1180 × 620): `cargo test --release -p emcraft-ui-egui render_timings -- --ignored --nocapture`.
#[test]
#[ignore = "timing report: run by hand"]
fn render_timings() {
    use emcraft_render::view3d::{Camera, Options, render};
    for (name, size) in [("highpass", None), ("coaxial-line", None), ("coaxial-line", Some(0.45)), ("coaxial-line", Some(0.25))] {
        let s = meshed(name, size);
        let faces = s.model().mesh.as_ref().map_or(0, |m| m.boundary.len());
        for o in [Overlay::Plain, Overlay::Mesh, Overlay::Materials] {
            let b = build(&s, o);
            let (lo, hi) = b.bounds.unwrap();
            let mut c = Camera::default();
            c.fit(lo, hi, 1180.0 / 620.0);
            for (ss, w, h) in [(1, 1180, 620), (2, 1180, 620), (1, 590, 310)] {
                let opt = Options { width: w, height: h, supersample: ss, ..Default::default() };
                let t = std::time::Instant::now();
                let n = 5;
                for k in 0..n {
                    c.orbit(3.0 * k as f64, 0.0);
                    let _ = render(&b.scene, &c, &opt);
                }
                println!(
                    "{name} ({faces} boundary faces) {o:?}: {} triangles, {} lines, {w}x{h} x{ss}: {:.1} ms",
                    b.scene.tris.len(),
                    b.scene.lines.len(),
                    t.elapsed().as_secs_f64() * 1e3 / n as f64
                );
            }
        }
    }
}

/// A meshed guide cut along its length fills the whole cut, each material in its colour, the
/// same cut twice the same; a plane outside the mesh cuts nothing.
#[test]
fn a_mesh_cut_fills_the_section_by_material() {
    let mut s = Session::new();
    let script: serde_json::Value = serde_json::from_str(emcraft_engine::templates::find("highpass").unwrap().script).unwrap();
    let steps: Vec<serde_json::Value> =
        script.as_array().unwrap().iter().filter(|c| !c["command"].as_str().unwrap().starts_with("solve.")).cloned().collect();
    s.run_script(&json!(steps)).unwrap();
    let m = s.model();
    let (lo, hi) = m.bounds().map(|b| (p3(b.min), p3(b.max))).unwrap();
    let at = 0.5 * (lo[0] + hi[0]);
    let cut = mesh_slice(m, 0, at, true);
    let area = |t: &emcraft_render::view3d::Tri| {
        let (u, v) = ([0, 1, 2].map(|k| t.p[1][k] - t.p[0][k]), [0, 1, 2].map(|k| t.p[2][k] - t.p[0][k]));
        0.5 * ((u[1] * v[2] - u[2] * v[1]).powi(2) + (u[2] * v[0] - u[0] * v[2]).powi(2) + (u[0] * v[1] - u[1] * v[0]).powi(2)).sqrt()
    };
    // The metal backing: two triangles behind the cut, as large as the mesh's box there.
    let metal = super::style::sc().metal;
    let (backing, caps): (Vec<&emcraft_render::view3d::Tri>, Vec<_>) = cut.tris.iter().partition(|t| t.p.iter().all(|p| p[0] < at));
    assert_eq!(backing.len(), 2);
    assert!(backing.iter().all(|t| t.color == metal));
    let total: f64 = caps.iter().map(|t| area(t)).sum();
    let want = (hi[1] - lo[1]) * (hi[2] - lo[2]);
    assert!((total - want).abs() < 1e-9 * want.max(1e-12) + 1e-12, "{total} vs {want}");
    assert!(caps.iter().all(|t| t.p.iter().all(|p| p[0] == at)));
    // Facing the removed (high x) side.
    assert!(cut.tris.iter().all(|t| {
        let (u, v) = ([0, 1, 2].map(|k| t.p[1][k] - t.p[0][k]), [0, 1, 2].map(|k| t.p[2][k] - t.p[0][k]));
        u[1] * v[2] - u[2] * v[1] >= 0.0
    }));
    let colours: std::collections::BTreeSet<_> = caps.iter().map(|t| (t.color.0, t.color.1, t.color.2)).collect();
    assert_eq!(colours.len(), 2, "vacuum and ptfe-like");
    assert_eq!(mesh_slice(m, 0, at, true).tris, cut.tris);
    assert!(mesh_slice(m, 0, hi[0] + 1.0, true).tris.is_empty());
}

#[test]
fn walls_with_a_thickness_are_drawn_round_the_inside_before_and_after_meshing() {
    let mut s = Session::new();
    s.run_script(&json!([
        {"command": "geometry.waveguide", "params": {"width": 22.86, "height": 10.16, "sections": [{"length": 20, "material": "vacuum"}, {"length": 2, "material": "vacuum", "aperture": {"start": 6, "width": 10.86}}, {"length": 20, "material": "vacuum"}]}},
        {"command": "model.port.create", "params": {"name": "1", "end": "start", "modes": 2}},
        {"command": "model.port.create", "params": {"name": "2", "end": "end", "modes": 2}}
    ]))
    .unwrap();
    let flush = build(&s, Overlay::Plain).scene.tris.len();
    s.execute("model.walls", &json!({"thickness": 2})).unwrap();
    let thick = build(&s, Overlay::Plain).scene;
    assert!(thick.tris.len() > flush, "{} against {flush}", thick.tris.len());
    // The shell reaches 2 mm outside the guide, and no further than the ends.
    let reaches = |scene: &emcraft_render::view3d::Scene| {
        let lo = scene.tris.iter().flat_map(|t| t.p).fold([f64::INFINITY; 3], |m, p| [m[0].min(p[0]), m[1].min(p[1]), m[2].min(p[2])]);
        assert!((lo[0] + 2e-3).abs() < 1e-9 && (lo[1] + 2e-3).abs() < 1e-9 && lo[2].abs() < 1e-9, "{lo:?}");
    };
    reaches(&thick);
    s.execute("mesh.generate", &json!({"size": 6})).unwrap();
    reaches(&build(&s, Overlay::Plain).scene);
}

/// Pixels across a `w` × `h` view, and the camera fitted to `b` from a few directions.
fn views(b: &Built, w: f64, h: f64) -> Vec<emcraft_render::view3d::Camera> {
    use emcraft_render::view3d::{Camera, Projection, StandardView};
    let (lo, hi) = b.bounds.or_else(|| b.scene.bounds()).unwrap();
    [StandardView::Iso, StandardView::Front, StandardView::Top, StandardView::Right]
        .into_iter()
        .flat_map(|v| [Projection::Perspective, Projection::Orthographic].map(|p| (v, p)))
        .map(|(v, p)| {
            let mut c = Camera { projection: p, ..Camera::default() };
            c.standard(v);
            c.fit(lo, hi, w / h);
            c
        })
        .collect()
}

/// A sample's scene picked through its index answers every pixel as the scan does, and the
/// face under it is the same face: the tools snap and measure as before.
#[test]
fn picking_through_the_index_answers_as_the_scan_does() {
    use emcraft_render::view3d::hits;
    let s = meshed("highpass", None);
    let (w, h) = (300.0, 200.0);
    for o in [Overlay::Plain, Overlay::Ports, Overlay::Mesh] {
        let b = build(&s, o);
        b.cache.fill_now(&b.scene);
        let index = b.cache.index(&b.scene).unwrap();
        let mut picked = 0;
        for c in views(&b, w, h) {
            for k in 0..300 {
                let (x, y) = ((k * 37 % 300) as f64 + 0.25, (k * 23 % 200) as f64 * 0.5 + 50.0);
                let want = hits(&b.scene, &c, x, y, w, h);
                assert_eq!(b.hits(&c, x, y, w, h), want, "{o:?} ({x}, {y})");
                if let Some((near, _)) = want {
                    picked += 1;
                    if k % 10 == 0 {
                        assert_eq!(faces::Face::of_indexed(&b.scene, Some(index), near.index), faces::Face::of(&b.scene, near.index));
                    }
                }
            }
        }
        assert!(picked > 500, "{o:?}: {picked} pixels picked");
    }
}

/// A large scene is drawn from a coarser copy while the view moves (each part's colour and tag
/// kept), and picked from the full scene; a clone starts with nothing made.
#[test]
fn a_large_scene_moves_coarser_and_is_picked_in_full() {
    use emcraft_render::view3d::{Camera, StandardView, hits};
    let mut b = Built::default();
    // A finely divided block: 6 × 2 × 120² triangles.
    let n = 120;
    for k in 0..3 {
        let (i, j) = ((k + 1) % 3, (k + 2) % 3);
        for side in [0.0, 0.01] {
            b.scene.tagged(Pick::Solid.tag(), |s| {
                for a in 0..n {
                    for c in 0..n {
                        let at = |u: usize, v: usize| {
                            let mut p = [side; 3];
                            (p[i], p[j]) = (u as f64 * 0.01 / n as f64, v as f64 * 0.01 / n as f64);
                            p
                        };
                        s.quad([at(a, c), at(a + 1, c), at(a + 1, c + 1), at(a, c + 1)], Rgb(10, 20, 30), 255);
                    }
                }
            });
        }
    }
    assert!(b.scene.tris.len() >= cache::COARSE_FROM);
    b.cache.fill_now(&b.scene);
    let moving = b.moving();
    assert!(moving.tris.len() <= cache::COARSE_TRIANGLES * 5 / 4, "{} triangles while moving", moving.tris.len());
    assert!(moving.tris.iter().all(|t| t.tag == Pick::Solid.tag() && t.color == Rgb(10, 20, 30)));
    // Still the block: its box, to rounding.
    let ((lo, hi), (flo, fhi)) = (moving.bounds().unwrap(), b.scene.bounds().unwrap());
    assert!((0..3).all(|k| (lo[k] - flo[k]).abs() < 1e-12 && (hi[k] - fhi[k]).abs() < 1e-12), "{lo:?} {hi:?}");
    let mut c = Camera::default();
    c.standard(StandardView::Iso);
    c.fit([0.0; 3], [0.01; 3], 1.5);
    for (x, y) in [(150.0, 100.0), (120.0, 80.0), (190.0, 130.0)] {
        let got = b.hits(&c, x, y, 300.0, 200.0);
        assert!(got.is_some());
        assert_eq!(got, hits(&b.scene, &c, x, y, 300.0, 200.0));
    }
    let copy = b.clone();
    assert!(format!("{:?}", copy.cache).contains("index: false"));
}

/// Milliseconds per pick, face and moving picture on the magic tee's fine mesh, scanned and
/// through the index: `cargo test --release -p emcraft-ui-egui pick_timings -- --ignored
/// --nocapture`.
#[test]
#[ignore = "timing report: run by hand"]
fn pick_timings() {
    use emcraft_render::view3d::{flatten, hits};
    let size: f64 = std::env::var("EMCRAFT_PICK_MESH").ok().and_then(|v| v.parse().ok()).unwrap_or(1.2);
    let t = std::time::Instant::now();
    let s = meshed("magic-tee", Some(size));
    println!("magic tee meshed at {size} mm in {:.1} s", t.elapsed().as_secs_f64());
    for o in [Overlay::Plain, Overlay::Mesh] {
        let b = build(&s, o);
        let (w, h) = (1180.0, 620.0);
        let c = views(&b, w, h)[0];
        let t = std::time::Instant::now();
        b.cache.fill_now(&b.scene);
        let filled = t.elapsed().as_secs_f64() * 1e3;
        let t = std::time::Instant::now();
        let index = emcraft_render::view3d::SceneIndex::new(&b.scene);
        let built = t.elapsed().as_secs_f64() * 1e3;
        let pixels: Vec<(f64, f64)> = (0..400).map(|k| ((k * 37 % 1180) as f64, (k * 23 % 620) as f64)).collect();
        let time = |f: &dyn Fn(f64, f64)| {
            let t = std::time::Instant::now();
            pixels.iter().for_each(|&(x, y)| f(x, y));
            t.elapsed().as_secs_f64() * 1e3 / pixels.len() as f64
        };
        let scan = time(&|x, y| {
            std::hint::black_box(hits(&b.scene, &c, x, y, w, h));
        });
        let indexed = time(&|x, y| {
            std::hint::black_box(b.hits(&c, x, y, w, h));
        });
        let under: Vec<usize> = pixels.iter().filter_map(|&(x, y)| b.hits(&c, x, y, w, h)).map(|(n, _)| n.index).take(40).collect();
        let face = |idx: Option<&emcraft_render::view3d::SceneIndex>| {
            let t = std::time::Instant::now();
            under.iter().for_each(|&i| {
                std::hint::black_box(faces::Face::of_indexed(&b.scene, idx, i));
            });
            t.elapsed().as_secs_f64() * 1e3 / under.len().max(1) as f64
        };
        let (face_scan, face_indexed) = (face(None), face(Some(&index)));
        let draw = |scene: &Scene| {
            let t = std::time::Instant::now();
            std::hint::black_box(flatten(scene, &c, w, h));
            t.elapsed().as_secs_f64() * 1e3
        };
        let moving = b.moving();
        println!(
            "{o:?}: {} triangles ({} lines); index {built:.1} ms (with the coarser copy {filled:.0} ms); pick {scan:.3} ms scanned, {indexed:.4} ms indexed; face {face_scan:.2} ms scanned, {face_indexed:.2} ms indexed; moving picture {:.1} ms full, {:.1} ms from {} triangles",
            b.scene.tris.len(),
            b.scene.lines.len(),
            draw(&b.scene),
            draw(moving),
            moving.tris.len()
        );
    }
}

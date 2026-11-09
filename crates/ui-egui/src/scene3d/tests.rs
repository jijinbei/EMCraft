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

/// A cut shows the mesh throughout the domain, with neither filled caps nor a metal
/// backing. Geometry views keep only the outline and material boundaries.
#[test]
fn a_mesh_cut_shows_interior_edges_without_filled_faces() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    let m = s.model();
    let (lo, hi) = m.bounds().map(|b| (p3(b.min), p3(b.max))).unwrap();
    for axis in 0..3 {
        let at = 0.47 * lo[axis] + 0.53 * hi[axis];
        let cut = mesh_slice(m, axis, at);
        let outline = section_outline(m, axis, at);
        assert!(cut.tris.is_empty());
        assert!(outline.tris.is_empty());
        assert!(!outline.lines.is_empty());
        assert!(cut.lines.len() > outline.lines.len());
        assert!(cut.lines.iter().all(|l| [l.a, l.b].iter().all(|p| p[axis] == at)));
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        assert!(cut.lines.iter().any(|l| [l.a, l.b].iter().all(|p| p[u] > lo[u] && p[u] < hi[u] && p[v] > lo[v] && p[v] < hi[v])));
        assert!(mesh_slice(m, axis, hi[axis] + 1.0).lines.is_empty());
    }
    assert!(mesh_slice(m, 3, 0.0).lines.is_empty());
    assert!(mesh_slice(m, 0, f64::NAN).lines.is_empty());
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
                let want = hits(&b.scene, &c, emcraft_render::view3d::Pixel::new(x, y, w, h));
                assert_eq!(b.hits(&c, emcraft_render::view3d::Pixel::new(x, y, w, h)), want, "{o:?} ({x}, {y})");
                if let Some(emcraft_render::view3d::Hits { nearest: near, .. }) = want {
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
        let got = b.hits(&c, emcraft_render::view3d::Pixel::new(x, y, 300.0, 200.0));
        assert!(got.is_some());
        assert_eq!(got, hits(&b.scene, &c, emcraft_render::view3d::Pixel::new(x, y, 300.0, 200.0)));
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
    use emcraft_render::view3d::hits;
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
            std::hint::black_box(hits(&b.scene, &c, emcraft_render::view3d::Pixel::new(x, y, w, h)));
        });
        let indexed = time(&|x, y| {
            std::hint::black_box(b.hits(&c, emcraft_render::view3d::Pixel::new(x, y, w, h)));
        });
        let under: Vec<usize> = pixels
            .iter()
            .filter_map(|&(x, y)| b.hits(&c, emcraft_render::view3d::Pixel::new(x, y, w, h)))
            .map(|h| h.nearest.index)
            .take(40)
            .collect();
        let face = |idx: Option<&emcraft_render::view3d::SceneIndex>| {
            let t = std::time::Instant::now();
            under.iter().for_each(|&i| {
                std::hint::black_box(faces::Face::of_indexed(&b.scene, idx, i));
            });
            t.elapsed().as_secs_f64() * 1e3 / under.len().max(1) as f64
        };
        let (face_scan, face_indexed) = (face(None), face(Some(&index)));
        // Packing a scene for the GPU (the CPU's share of a new picture).
        let draw = |scene: &Scene| {
            let t = std::time::Instant::now();
            std::hint::black_box(crate::gpu3d::Packed::new(scene, 0));
            t.elapsed().as_secs_f64() * 1e3
        };
        let moving = b.moving();
        println!(
            "{o:?}: {} triangles ({} lines); index {built:.1} ms (with the coarser copy {filled:.0} ms); pick {scan:.3} ms scanned, {indexed:.4} ms indexed; face {face_scan:.2} ms scanned, {face_indexed:.2} ms indexed; packed for the GPU {:.1} ms full, {:.1} ms from {} triangles",
            b.scene.tris.len(),
            b.scene.lines.len(),
            draw(&b.scene),
            draw(moving),
            moving.tris.len()
        );
    }
}

#[test]
fn mesh_port_rims_are_transparent_and_directions_are_world_space() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    let built = build(&s, Overlay::Mesh);
    let rims: Vec<_> = built.scene.tris.iter().filter(|t| t.tag == Pick::Guide.tag()).collect();
    assert!(!rims.is_empty());
    assert!(rims.iter().all(|t| t.alpha <= 35), "the mouth must not hide the volume mesh");
    assert_eq!(built.arrows.len(), 3);
    for a in &built.arrows[..2] {
        assert!(a.start[2] < 0.0 && a.end[2] < 0.0, "glyphs must not intersect the start port's face");
    }
    assert!(built.arrows[2].start[2] > s.model().waveguide().unwrap().length());

    assert!(built.arrows[0].end[2] > built.arrows[0].start[2], "incident goes into the start port");
    assert!(built.arrows[1].end[2] < built.arrows[1].start[2], "reflected goes out of the start port");
    assert!(built.arrows[2].end[2] > built.arrows[2].start[2], "transmitted goes out of the end port");
}

#[test]
fn material_selection_glows_and_visibility_preserves_walls_and_ports() {
    let mut s = meshed("highpass", None);
    let baseline = build(&s, Overlay::Materials);
    let selected = build_with(&s, Overlay::Materials, &[], &[Pick::Material(0)]);
    assert!(selected.scene.tris.iter().any(|t| t.tag == Pick::Material(0).tag() && t.alpha >= 170));
    s.execute("view.visibility", &json!({"kind":"material","name":"vacuum","visible":false})).unwrap();
    let hidden = build_with(&s, Overlay::Materials, &[], &[Pick::Material(0)]);
    assert!(!hidden.scene.tris.iter().any(|t| t.tag == Pick::Material(0).tag()));
    assert_eq!(baseline.scene.tris.len(), hidden.scene.tris.len());
    let before = hidden.scene.tris.len();
    s.execute("view.visibility", &json!({"kind":"material","name":"ptfe-like","visible":false})).unwrap();
    let hidden = build(&s, Overlay::Materials);
    assert!(hidden.scene.tris.len() < before);
    let material_colour = material_rgb(s.model(), 1);
    assert!(
        hidden
            .scene
            .tris
            .iter()
            .filter(|t| matches!(Pick::from_tag(t.tag), Some(Pick::Port(_))))
            .all(|t| t.color != material_colour && t.alpha <= 25)
    );
    assert!(hidden.scene.tris.iter().any(|t| matches!(Pick::from_tag(t.tag), Some(Pick::Port(_)))));
    assert!(hidden.scene.tris.iter().any(|t| t.tag == Pick::Guide.tag()));
}

#[test]
fn dielectric_is_a_closed_block_and_geometry_visibility_hides_its_shell() {
    let mut s = meshed("highpass", None);
    let b = build(&s, Overlay::Materials);
    let mut axes = [false; 3];
    for t in b.scene.tris.iter().filter(|t| t.tag == Pick::Material(1).tag()) {
        for (axis, found) in axes.iter_mut().enumerate() {
            if t.p.iter().all(|p| (p[axis] - t.p[0][axis]).abs() < 1e-12) {
                *found = true;
            }
        }
    }
    assert_eq!(axes, [true; 3], "material must have side faces as well as interface caps");
    s.execute("view.visibility", &json!({"kind":"geometry","name":"guide","visible":false})).unwrap();
    let b = build(&s, Overlay::Materials);
    assert!(b.scene.tris.iter().all(|t| matches!(Pick::from_tag(t.tag), Some(Pick::Port(_)))));
    assert!(s.model().mesh.is_some());
    s.execute("view.visibility", &json!({"kind":"geometry","name":"guide","visible":true})).unwrap();
    assert!(build(&s, Overlay::Materials).scene.tris.iter().any(|t| t.tag == Pick::Material(1).tag()));
}

#[test]
fn a_board_shows_its_copper_by_role_with_its_heights_stretched() {
    let mut s = Session::new();
    s.execute("pcb.sample", &json!({"split": true})).unwrap();
    s.execute("view.pcb", &json!({"net": "DATA", "stage": "net"})).unwrap();
    let b = build(&s, Overlay::Board);
    let board = s.model().pcb().unwrap();
    let nets = board::nets(board);
    let tagged = |net: &str| {
        let tag = Pick::Net(nets.iter().position(|n| n == net).unwrap()).tag();
        b.scene.tris.iter().filter(move |t| t.tag == tag)
    };
    let t = crate::theme::Tokens::get();
    // The route in the warning colour, its reference in the success colour, both there.
    assert!(tagged("DATA").count() > 0 && tagged("DATA").all(|t2| t2.color == crate::theme::rgb(t.warning)));
    assert!(tagged("GND").count() > 0 && tagged("GND").all(|t2| t2.color == crate::theme::rgb(t.success)));
    assert!(tagged("CLK").all(|t2| t2.color == sc().strip));
    // The route's centre lines over it; the board's 0.18 mm stretched to show (the legend says).
    assert_eq!(b.scene.lines.iter().filter(|l| l.color == crate::theme::rgb(t.warning)).count(), 2);
    let (lo, hi) = b.bounds.unwrap();
    assert!(hi[1] - lo[1] > 0.05 * (hi[0] - lo[0]), "{lo:?} {hi:?}");
    assert!(b.legend.iter().any(|(text, _)| text.starts_with("heights")));
}

/// With a symmetry plane only the side below it is meshed, but the model is drawn whole: the
/// meshed half and its mirror image, framed as a whole, each port's arrows on the middle of its
/// whole face. The Mesh step tells the halves apart: the image faint, without the mesh's edges.
#[test]
fn a_halved_model_is_drawn_whole_and_its_image_faint_in_the_mesh_step() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "td-e-bend"})).unwrap();
    let half = s.model().domain.symmetry[0].at;
    let built = build(&s, Overlay::Plain);
    let (_, hi) = built.bounds.unwrap();
    assert!((hi[2] - 2.0 * half).abs() < 1e-12, "{hi:?}");
    let top = built.scene.tris.iter().flat_map(|t| t.p).map(|p| p[2]).fold(f64::MIN, f64::max);
    assert!(top > 2.0 * half - 1e-9, "the image across the plane is drawn: {top}");
    assert_eq!(built.arrows.len(), 3);
    for a in &built.arrows {
        assert!((a.start[2] - half).abs() < 1e-12 && (a.end[2] - half).abs() < 1e-12, "{a:?}");
    }
    let mesh = build(&s, Overlay::Mesh);
    let above = |p: [f64; 3]| p[2] > half + 1e-9;
    assert!(mesh.scene.tris.iter().filter(|t| t.p.iter().all(|p| above(*p))).all(|t| t.alpha <= 255 / 3));
    assert!(!mesh.scene.lines.iter().any(|l| above(l.a) && above(l.b)), "no mesh edges on the image");
}

/// A symmetry plane shows before the model is meshed: a faint sheet in its wall's colour across
/// the model's box, framed.
#[test]
fn a_symmetry_plane_is_drawn_faintly_before_the_mesh() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "cavity"})).unwrap();
    s.execute("model.symmetry", &json!({"axis": "x", "kind": "magnetic"})).unwrap();
    assert!(s.model().mesh.is_none());
    let built = build(&s, Overlay::Plain);
    let colour = super::style::symmetry_rgb(emcraft_model::SymmetryKind::Magnetic);
    let sheet: Vec<_> = built.scene.tris.iter().filter(|t| t.color == colour).collect();
    assert_eq!(sheet.len(), 2);
    assert!(sheet.iter().all(|t| t.alpha < 128 && t.p.iter().all(|p| (p[0] - 0.01143).abs() < 1e-12)));
    assert!(built.scene.lines.iter().filter(|l| l.color == colour).count() >= 4);
}

/// A sector is drawn whole: its n − 1 turns beside it, faint and without edges in the Mesh step;
/// before the mesh, its two cut half-planes faintly.
#[test]
fn a_sector_is_drawn_with_its_turns() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 15})).unwrap();
    s.execute("model.rotation", &json!({"n": 4})).unwrap();
    let colour = super::style::sc().periodic;
    let cuts = build(&s, Overlay::Boundaries).scene.tris.iter().filter(|t| t.color == colour && t.alpha < 128).count();
    assert_eq!(cuts, 4);
    s.execute("mesh.generate", &json!({"size": 5})).unwrap();
    let whole = build(&s, Overlay::Plain);
    let mesh = build(&s, Overlay::Mesh);
    // Every corner of the whole is within the radius, and the turns reach every quadrant.
    let quadrants: std::collections::BTreeSet<(bool, bool)> = whole.scene.tris.iter().flat_map(|t| t.p).map(|p| (p[0] > 1e-4, p[1] > 1e-4)).collect();
    assert_eq!(quadrants.len(), 4);
    let faint = mesh.scene.tris.iter().filter(|t| t.wire.is_none()).count();
    assert!(faint * 4 >= mesh.scene.tris.len() * 3 - 4, "{faint} of {}", mesh.scene.tris.len());
}

/// Regression: an imported cavity's metal (a block with a channel through it) showed round its
/// air before meshing and vanished once meshed: the walls are drawn in both.
#[test]
fn an_imported_cavitys_walls_show_before_and_after_meshing() {
    let path = format!("{}/../../tests/oracle/imported-iris-cavity/block.3mf", env!("CARGO_MANIFEST_DIR"));
    let mut s = Session::new();
    s.execute("geometry.import", &serde_json::json!({"path": path})).unwrap();
    // The walls' triangles as drawn alone, then found in the whole scene.
    let mut alone = Built::default();
    super::geometry::part_walls(s.model(), &mut alone);
    assert!(!alone.scene.tris.is_empty());
    let shown = |s: &Session| {
        let scene = build(s, Overlay::Plain).scene;
        alone.scene.tris.iter().all(|w| scene.tris.iter().any(|t| t.p == w.p && t.alpha == w.alpha))
    };
    assert!(shown(&s));
    s.execute("mesh.generate", &serde_json::json!({"size": 6})).unwrap();
    assert!(shown(&s));
}

/// An imported part's metal carved by its air (the horn: a solid block, its flare's air cut out)
/// is drawn before meshing as the mesh will have it: no metal across the mouth's opening
/// (x 20–60 mm, y 20–50 mm on the plane z = 55 mm), where the block alone would close it.
#[test]
fn a_carved_part_is_drawn_open_before_meshing() {
    let mut s = Session::new();
    let horn =
        serde_json::json!({"path": "template:horn.solidcraft", "materials": {"inside": "vacuum"}, "open": true, "thickness": 10, "margin": 18});
    s.execute("geometry.import", &horn).unwrap();
    let b = super::build_with(&s, Overlay::Plain, &[], &[]);
    let walls = super::Pick::Feature(0).tag();
    let across = b.scene.tris.iter().filter(|t| t.tag == walls).filter(|t| {
        let c = [0, 1, 2].map(|k| (t.p[0][k] + t.p[1][k] + t.p[2][k]) / 3.0);
        (c[2] - 0.055).abs() < 1e-6 && c[0] > 0.021 && c[0] < 0.059 && c[1] > 0.021 && c[1] < 0.049
    });
    assert_eq!(across.count(), 0, "metal across the mouth");
    assert!(b.scene.tris.iter().any(|t| t.tag == walls), "the walls drawn");
}

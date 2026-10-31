//! Building the model: materials, solids, imported parts, bodies and meshes.

use serde_json::json;

use super::scratch;
use crate::*;

#[test]
fn dispersive_materials_are_read_in_display_units_and_checked() {
    let mut s = Session::new();
    let r = s.execute(
        "model.material.create",
        &json!({"name": "d", "eps_r": 2, "debye": [{"delta_eps": 0.6, "frequency": 20}],
                "lorentz": [{"delta_eps": 0.4, "frequency": 16, "damping": 1.5}], "drude": [{"plasma_frequency": 3, "damping": 0.5}]}),
    );
    r.unwrap();
    let (_, m) = s.model.material("d").unwrap();
    let tau = 1.0 / (2.0 * std::f64::consts::PI * 20e9);
    assert_eq!(m.dispersion.len(), 3);
    assert!(matches!(m.dispersion[0], emcraft_model::Pole::Debye { delta_eps, tau: t } if delta_eps == 0.6 && (t - tau).abs() < 1e-24));
    assert_eq!(m.dispersion[1], emcraft_model::Pole::Lorentz { delta_eps: 0.4, frequency: 16e9, damping: 1.5e9 });
    assert_eq!(m.dispersion[2], emcraft_model::Pole::Drude { plasma_frequency: 3e9, damping: 0.5e9 });
    // In MHz.
    s.execute("units.set", &json!({"frequency": "MHz"})).unwrap();
    s.execute("model.material.create", &json!({"name": "m", "eps_r": 1, "lorentz": [{"delta_eps": 1, "frequency": 500, "damping": 10}]})).unwrap();
    assert_eq!(s.model.material("m").unwrap().1.dispersion[0], emcraft_model::Pole::Lorentz { delta_eps: 1.0, frequency: 5e8, damping: 1e7 });
    // Hostile poles: refused, nothing created, nothing panics.
    let many: Vec<_> = (0..emcraft_model::MAX_POLES + 1).map(|_| json!({"delta_eps": 1, "frequency": 1})).collect();
    for bad in [
        json!({"debye": [{"delta_eps": -1, "frequency": 1}]}),
        json!({"debye": [{"delta_eps": 1}]}),
        json!({"debye": [{"delta_eps": 1, "frequency": 0}]}),
        json!({"debye": [{"delta_eps": 1, "frequency": 1e308}]}),
        json!({"debye": "x"}),
        json!({"debye": [1, 2]}),
        json!({"lorentz": [{"delta_eps": 1, "frequency": 1, "damping": -1}]}),
        json!({"lorentz": [{"delta_eps": "a", "frequency": 1, "damping": 1}]}),
        json!({"drude": [{"plasma_frequency": 1, "damping": 0}]}),
        json!({"drude": [{"plasma_frequency": f64::MAX, "damping": 1}]}),
        json!({"debye": many}),
        json!({"debye": many[..9], "lorentz": (0..9).map(|_| json!({"delta_eps": 1, "frequency": 1, "damping": 1})).collect::<Vec<_>>()}),
    ] {
        let mut p = bad.clone();
        p["name"] = json!("bad");
        p["eps_r"] = json!(2);
        let r = s.execute("model.material.create", &p);
        assert!(matches!(r, Err(EngineError::Params(_) | EngineError::Failed(_))), "{bad}: {r:?}");
        assert!(s.model.material("bad").is_err());
    }
}

#[test]
fn solids_mesh_unstructured_and_their_modes_meet_the_bessel_zeros() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 30})).unwrap();
    let m = s.execute("mesh.generate", &json!({"size": 4})).unwrap();
    assert_eq!(m["unstructured"], true);
    // The mode nearest 10 GHz: TE111 at 10.1.
    let r = s.execute("solve.eigenmodes", &json!({"near": 10, "count": 1, "order": 2})).unwrap();
    let f = r["modes"][0]["frequency"].as_f64().unwrap();
    assert_eq!(r["exact"][0]["mode"], "TE111");
    let exact = r["exact"][0]["frequency"].as_f64().unwrap();
    assert!((f - exact).abs() < 0.02 * exact, "{f} {exact}");
    // The mode's picture, on the cross-section through the axis's middle.
    let path = scratch("cyl.png");
    s.execute("view.field", &json!({"path": path, "mode": 1, "plane": "cross"})).unwrap();
    assert!(std::fs::read(&path).unwrap().len() > 1000);
    assert!(s.execute("geometry.solid", &json!({"shape": "torus"})).is_err());
    // A dielectric rod along the axis's middle third.
    s.execute("model.material.create", &json!({"name": "ptfe", "eps_r": 2.1})).unwrap();
    s.execute("geometry.inclusion", &json!({"shape": "cylinder", "radius": 3, "length": 10, "at": [0, 0, 10], "material": "ptfe"})).unwrap();
    // 6 mm: the rod's curved faces set the mesh still, in half the time of 4 mm.
    let m = s.execute("mesh.generate", &json!({"size": 6})).unwrap();
    assert!(m["tetrahedra"].as_u64().unwrap() > 0);
    assert!(s.model.mesh.as_ref().unwrap().tet_material.contains(&1));
    assert!(s.execute("geometry.inclusion", &json!({"shape": "sphere", "radius": 20})).is_err());
    assert!(s.execute("geometry.solid", &json!({"shape": "box", "size": [1, -1, 1]})).is_err());
    // A box between two ports is compared with the empty guide.
    let mut b = Session::new();
    b.execute("geometry.solid", &json!({"shape": "box", "size": [22.86, 10.16, 20]})).unwrap();
    b.execute("mesh.generate", &json!({"size": 4})).unwrap();
    b.execute("model.port.create", &json!({"name": "1", "end": "start"})).unwrap();
    b.execute("model.port.create", &json!({"name": "2", "end": "end"})).unwrap();
    b.execute("solve.frequency", &json!({"frequencies": [10], "order": 2})).unwrap();
    let c = b.execute("results.compare", &json!({})).unwrap();
    assert!(c["worst"].as_f64().unwrap() < 2e-3, "{c}");
}

/// A box [0, x] × [0, y] × [0, z] (mm) as a closed surface, two outward triangles per face.
fn box_surface(x: f64, y: f64, z: f64) -> emcraft_io::Surface {
    let mut s = emcraft_io::Surface::default();
    for i in 0..8 {
        s.positions.push(emcraft_model::Point3::new(
            if i & 1 == 0 { 0.0 } else { x },
            if i & 2 == 0 { 0.0 } else { y },
            if i & 4 == 0 { 0.0 } else { z },
        ));
    }
    for q in [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]] {
        s.triangles.push([q[0], q[1], q[2]]);
        s.triangles.push([q[0], q[2], q[3]]);
    }
    s.tags = vec![0; 12];
    s
}

#[test]
fn imported_surfaces_mesh_and_carry_ports() {
    // WR-90, 30 mm long, as binary STL in millimetres (exact in f32: 22.86 is not, so 22.5).
    let path = scratch("guide.stl");
    std::fs::write(&path, emcraft_io::stl::write_binary(&box_surface(22.5, 10.0, 30.0), 1.0)).unwrap();
    let mut s = Session::new();
    let r = s.execute("geometry.import", &json!({"path": path})).unwrap();
    assert_eq!(r["faces"], 6);
    assert!((r["volume"].as_f64().unwrap() - 22.5e-3 * 10e-3 * 30e-3).abs() < 1e-15);
    s.execute("mesh.generate", &json!({"size": 3})).unwrap();
    s.execute("model.port.create", &json!({"name": "1", "end": "start"})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end"})).unwrap();
    let r = s.execute("solve.frequency", &json!({"frequencies": [10], "order": 2})).unwrap();
    let s21 = &r["points"][0]["s21"];
    let (re, im) = (s21["re"].as_f64().unwrap(), s21["im"].as_f64().unwrap());
    let exact = emcraft_theory::Te10Line { a: 0.0225, layers: vec![emcraft_theory::Layer::lossless(0.03, 1.0, 1.0)] }.solve(10e9).unwrap().s21;
    assert!((re - exact.re).hypot(im - exact.im) < 1e-3, "{re} {im} {exact}");
    // The same as ASCII, in centimetres.
    std::fs::write(&path, emcraft_io::stl::write_ascii(&box_surface(2.25, 1.0, 3.0), 1.0)).unwrap();
    let r = s.execute("geometry.import", &json!({"path": path, "unit": 10})).unwrap();
    assert!((r["volume"].as_f64().unwrap() - 6.75e-6).abs() < 1e-15);
    // An open surface, a missing file and an unknown format are refused.
    let mut open = box_surface(1.0, 1.0, 1.0);
    open.triangles.pop();
    open.tags.pop();
    std::fs::write(&path, emcraft_io::stl::write_binary(&open, 1.0)).unwrap();
    let e = s.execute("geometry.import", &json!({"path": path})).unwrap_err();
    assert!(e.to_string().contains("closed"), "{e}");
    assert!(s.execute("geometry.import", &json!({"path": scratch("nothing-here.stl")})).is_err());
    assert!(s.execute("geometry.import", &json!({"path": scratch("guide.obj")})).is_err());
    // The model was left as it was by the refusals.
    assert!(matches!(s.model.solid.as_ref().unwrap().shape, emcraft_model::Shape::Surface { .. }));
}

/// A closed cylinder along z, radius r, length l (millimetres), `n` facets round; one tag.
fn can_surface(r: f64, l: f64, n: u32) -> emcraft_io::Surface {
    let mut s = emcraft_io::Surface::default();
    for z in [0.0, l] {
        for i in 0..n {
            let t = 2.0 * std::f64::consts::PI * f64::from(i) / f64::from(n);
            s.positions.push(emcraft_model::Point3::new(r * t.cos(), r * t.sin(), z));
        }
    }
    s.positions.push(emcraft_model::Point3::new(0.0, 0.0, 0.0));
    s.positions.push(emcraft_model::Point3::new(0.0, 0.0, l));
    for i in 0..n {
        let j = (i + 1) % n;
        s.triangles.extend([[i, j, n + j], [i, n + j, n + i], [2 * n, j, i], [2 * n + 1, n + i, n + j]]);
    }
    s.tags = vec![0; s.triangles.len()];
    s
}

#[test]
fn an_imported_ring_is_fitted_with_its_torus() {
    // A torus (radii 10 and 4 mm, 40 × 16 facets) read from STL: one face, a torus; the mesh's
    // edges on it bent onto it.
    let (big, small, n, m) = (10.0, 4.0, 40u32, 16u32);
    let mut ring = emcraft_io::Surface::default();
    let tau = 2.0 * std::f64::consts::PI;
    for i in 0..n {
        let (sp, cp) = (tau * f64::from(i) / f64::from(n)).sin_cos();
        for j in 0..m {
            let (st, ct) = (tau * f64::from(j) / f64::from(m)).sin_cos();
            ring.positions.push(emcraft_model::Point3::new(cp * (big + small * ct), sp * (big + small * ct), small * st));
        }
    }
    let id = |i: u32, j: u32| (i % n) * m + j % m;
    for i in 0..n {
        for j in 0..m {
            ring.triangles.extend([[id(i, j), id(i + 1, j), id(i + 1, j + 1)], [id(i, j), id(i + 1, j + 1), id(i, j + 1)]]);
        }
    }
    ring.tags = vec![0; ring.triangles.len()];
    let path = scratch("ring.stl");
    std::fs::write(&path, emcraft_io::stl::write_binary(&ring, 1.0)).unwrap();
    let mut s = Session::new();
    let r = s.execute("geometry.import", &json!({"path": path, "angle": 60})).unwrap();
    assert_eq!(r["curved"], json!({"cylinder": 0, "sphere": 0, "cone": 0, "torus": 1}), "{r}");
    let bent = s.execute("mesh.generate", &json!({"size": 3})).unwrap()["curved_edges"].as_u64().unwrap();
    assert!(bent > 100, "{bent}");
}

#[test]
fn imported_curved_faces_get_their_exact_surfaces() {
    // A can read from STL (single precision): its side recovered as a cylinder, its caps as
    // planes, and the mesh's edges on the side bent onto it.
    let path = scratch("can.stl");
    std::fs::write(&path, emcraft_io::stl::write_binary(&can_surface(10.0, 20.0, 26), 1.0)).unwrap();
    let mut s = Session::new();
    let r = s.execute("geometry.import", &json!({"path": path})).unwrap();
    assert_eq!((r["faces"].as_u64(), r["planes"].as_u64()), (Some(3), Some(2)), "{r}");
    assert_eq!(r["curved"], json!({"cylinder": 1, "sphere": 0, "cone": 0, "torus": 0}));
    let bent = s.execute("mesh.generate", &json!({"size": 4})).unwrap()["curved_edges"].as_u64().unwrap();
    assert!(bent > 50, "{bent}");
    // Kept faceted on request: nothing bent.
    let r = s.execute("geometry.import", &json!({"path": path, "fit_surfaces": false})).unwrap();
    assert_eq!((r["planes"].as_u64(), r["curved"]["cylinder"].as_u64()), (Some(0), Some(0)));
    assert_eq!(s.execute("mesh.generate", &json!({"size": 4})).unwrap()["curved_edges"], 0);
    assert!(s.execute("geometry.import", &json!({"path": path, "fit_surfaces": "yes"})).is_err());
}

#[test]
fn meshes_follow_the_wavelength_in_each_material() {
    // A ceramic sphere (εr = 4) in a vacuum sphere: at 12 GHz λ is 25 mm outside, 12.5 mm inside.
    let mut s = Session::new();
    s.execute("model.material.create", &json!({"name": "ceramic", "eps_r": 4})).unwrap();
    s.execute("geometry.solid", &json!({"shape": "sphere", "radius": 10})).unwrap();
    s.execute("geometry.inclusion", &json!({"shape": "sphere", "radius": 5, "at": [0, 0, 0], "material": "ceramic"})).unwrap();
    let r = s.execute("mesh.generate", &json!({"per_wavelength": 8, "frequency": 12})).unwrap();
    let lambda = emcraft_model::units::C0 / 12e9 * 1e3;
    assert!((r["sizes"]["vacuum"].as_f64().unwrap() - lambda / 8.0).abs() < 1e-9);
    assert!((r["sizes"]["ceramic"].as_f64().unwrap() - lambda / 16.0).abs() < 1e-9);
    // The ceramic's tetrahedra are about half the size: an eighth of the volume, less near the
    // interface, whose triangles the vacuum's tetrahedra share (3.9 measured).
    let mesh = s.model.mesh.as_ref().unwrap();
    let v = emcraft_mesh::volumes(mesh);
    let mean = |m: u32| {
        let (sum, n) = v.iter().zip(&mesh.tet_material).filter(|(_, k)| **k == m).fold((0.0, 0), |(s, n), (v, _)| (s + v, n + 1));
        sum / n as f64
    };
    let ratio = mean(0) / mean(1);
    assert!(ratio > 2.5 && ratio < 16.0, "{ratio}");
    // A cap on the size; and a structured guide takes its smallest material's size.
    let r = s.execute("mesh.generate", &json!({"per_wavelength": 8, "frequency": 12, "size": 2})).unwrap();
    assert_eq!(r["sizes"]["vacuum"], 2.0);
    let mut w = Session::new();
    w.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    let r = w.execute("mesh.generate", &json!({"per_wavelength": 10, "frequency": 9})).unwrap();
    let h = emcraft_model::units::C0 / 9e9 / 2f64.sqrt() / 10.0 * 1e3;
    assert!((r["size"].as_f64().unwrap() - h).abs() < 1e-9, "{}", r["size"]);
    assert!(w.execute("mesh.generate", &json!({"per_wavelength": 10})).is_err());
    assert!(w.execute("mesh.generate", &json!({"per_wavelength": 0, "frequency": 9})).is_err());
    assert!(w.execute("mesh.generate", &json!({})).is_err());
}

/// A closed box surface [x0, x1] × … (metres), outward triangles, one face tag per side.
fn named_box(name: &str, lo: [f64; 3], hi: [f64; 3]) -> emcraft_io::Surface {
    let c = |i: usize| {
        emcraft_model::Point3::new(
            if i & 1 == 1 { hi[0] } else { lo[0] },
            if i & 2 == 2 { hi[1] } else { lo[1] },
            if i & 4 == 4 { hi[2] } else { lo[2] },
        )
    };
    let quads: [([u32; 4], u32); 6] =
        [([0, 2, 3, 1], 0), ([4, 5, 7, 6], 1), ([0, 1, 5, 4], 2), ([2, 6, 7, 3], 3), ([0, 4, 6, 2], 4), ([1, 3, 7, 5], 5)];
    let mut triangles = Vec::new();
    let mut tags = Vec::new();
    for (q, t) in quads {
        triangles.push([q[0], q[1], q[2]]);
        triangles.push([q[0], q[2], q[3]]);
        tags.extend([t, t]);
    }
    emcraft_io::Surface { name: name.into(), positions: (0..8).map(c).collect(), triangles, tags }
}

#[test]
fn bodies_are_placed_by_what_they_are_made_of() {
    let mut s = Session::new();
    s.execute("model.material.create", &json!({"name": "ceramic", "eps_r": 9.8})).unwrap();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [40, 40, 40]})).unwrap();
    // A metal shell, the air inside it given back, a ceramic puck apart.
    s.execute("geometry.body", &json!({"material": "metal", "shape": "box", "size": [20, 12, 16], "at": [10, 14, 0]})).unwrap();
    s.execute("geometry.body", &json!({"material": "vacuum", "shape": "box", "size": [16, 8, 14], "at": [12, 16, 0]})).unwrap();
    s.execute("geometry.body", &json!({"material": "ceramic", "shape": "cylinder", "radius": 3, "length": 4, "at": [20, 5, 30]})).unwrap();
    let names = |s: &Session| bodies::bodies(&s.model).into_iter().map(|b| (b.name, b.material)).collect::<Vec<_>>();
    assert_eq!(names(&s), [("body 1".to_owned(), "metal".to_owned()), ("body 2".into(), "vacuum".into()), ("body 3".into(), "ceramic".into())]);
    // The puck made metal: placed again, last.
    s.execute("geometry.body.material", &json!({"body": "body 3", "material": "metal"})).unwrap();
    assert_eq!(s.model.features.len(), 3);
    assert!(s.model.inclusions.is_empty());
    for bad in [json!({"body": "nothing", "material": "metal"}), json!({"body": "body 1", "material": "unobtainium"})] {
        assert!(s.execute("geometry.body.material", &bad).is_err(), "{bad}");
    }
    assert!(s.execute("geometry.body", &json!({"material": "ceramic", "shape": "box", "size": [2, 2, 2], "at": [1, 1, 1], "axis": "x"})).is_err());
}

#[test]
fn a_part_of_several_bodies_lies_in_air_each_metal_unless_named() {
    let path = scratch("bodies.3mf");
    let post = named_box("Post", [0.0, 0.0, 0.0], [0.002, 0.002, 0.01]);
    let puck = named_box("Puck", [0.006, 0.0, 0.002], [0.01, 0.004, 0.006]);
    std::fs::write(&path, emcraft_io::threemf::write(&[post, puck]).unwrap()).unwrap();
    let mut s = Session::new();
    s.execute("model.material.create", &json!({"name": "ceramic", "eps_r": 9.8})).unwrap();
    let r = s.execute("geometry.import", &json!({"path": path, "margin": 5, "materials": {"Puck": "ceramic"}})).unwrap();
    assert_eq!(r["region"], "box");
    assert_eq!(
        r["bodies"],
        json!([{"body": "Post", "material": "metal", "as": "metal"}, {"body": "Puck", "material": "ceramic", "as": "dielectric"}])
    );
    // The box: the parts' extent and 5 mm around.
    let Some(emcraft_model::Solid { shape: emcraft_model::Shape::Box { size }, .. }) = &s.model.solid else { panic!("{:?}", s.model.solid) };
    assert!((size.x - 0.02).abs() < 1e-12 && (size.z - 0.02).abs() < 1e-12, "{size:?}");
    // It meshes: the post a hole, the puck its own region.
    let m = s.execute("mesh.generate", &json!({"size": 2})).unwrap();
    assert!(m["tetrahedra"].as_u64().unwrap() > 100, "{m}");
    let mats: std::collections::BTreeSet<u32> = s.model.mesh.as_ref().unwrap().tet_material.iter().copied().collect();
    assert_eq!(mats.len(), 2);
    // A body named as the region; a name that is none.
    s.execute("geometry.import", &json!({"path": path, "region": "Post"})).unwrap_err();
    assert!(s.execute("geometry.import", &json!({"path": path, "materials": {"Lid": "metal"}})).is_err());
}

/// Regression: a metal body of an imported part touching the region's side wall and both broad
/// walls meshed "successfully" into flat tetrahedra, and the solve failed with "degenerate
/// tetrahedron (volume -0e0)". It is now subtracted like the same box as a native feature, and
/// the S-parameters agree.
#[test]
fn an_imported_body_touching_three_walls_is_subtracted() {
    let path = scratch("touch-xy.3mf");
    let guide = named_box("Guide", [0.0; 3], [0.02286, 0.01016, 0.032]);
    let plate = named_box("B0", [0.0, 0.0, 0.015], [0.00543, 0.01016, 0.017]);
    std::fs::write(&path, emcraft_io::threemf::write(&[guide, plate]).unwrap()).unwrap();
    let s11 = |first: serde_json::Value| {
        let mut s = Session::new();
        s.execute(first["command"].as_str().unwrap(), &first["params"]).unwrap();
        if s.model.solid.as_ref().is_some_and(|x| matches!(x.shape, emcraft_model::Shape::Box { .. })) {
            let feature = json!({"combine": "subtract", "shape": "box", "size": [5.43, 10.16, 2], "at": [0, 0, 15]});
            s.execute("geometry.feature", &feature).unwrap();
        }
        s.execute("mesh.generate", &json!({"size": 3})).unwrap();
        s.execute("model.port.create", &json!({"name": "1", "end": "start"})).unwrap();
        s.execute("model.port.create", &json!({"name": "2", "end": "end"})).unwrap();
        let r = s.execute("solve.frequency", &json!({"frequencies": [10], "order": 1})).unwrap();
        r["points"][0]["s11"]["mag"].as_f64().unwrap()
    };
    let imported = s11(json!({"command": "geometry.import", "params": {"path": path, "region": "Guide"}}));
    let native = s11(json!({"command": "geometry.solid", "params": {"shape": "box", "size": [22.86, 10.16, 32]}}));
    assert!((imported - native).abs() < 0.01, "{imported} {native}");
}

/// SolidCraft's waveguide block (tests/oracle/imported-iris-cavity): a 32.86 × 20.16 × 32 mm
/// block, the WR-90 channel through it along z, two iris plates joined to its side walls.
fn block_3mf() -> String {
    format!("{}/../../tests/oracle/imported-iris-cavity/block.3mf", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn a_part_drawn_as_its_metal_gives_the_air_it_encloses() {
    let mut s = Session::new();
    let r = s.execute("geometry.import", &json!({"path": block_3mf(), "region": "cavity"})).unwrap();
    assert_eq!(r["region"], "cavity");
    assert_eq!(r["bodies"], json!([{"body": "Block", "material": "metal", "as": "walls"}]));
    // The channel less the plates, mm³; nothing outside the block.
    let exact = 22.86 * 10.16 * 32.0 - 2.0 * 5.43 * 10.16 * 2.0;
    assert!((r["pieces"]["kept"][0].as_f64().unwrap() - exact).abs() < 1e-6 * exact, "{r}");
    assert_eq!(r["pieces"]["dropped"], 0);
    assert!(s.model.features.is_empty() && s.model.inclusions.is_empty());
    // The ports are the channel's ends: whole rectangles, the TE modes in closed form.
    s.execute("model.port.create", &json!({"name": "1", "end": "start"})).unwrap();
    let Ok(emcraft_model::PortShape::Rect { x0, y0, a, b }) = s.model.port_shape(emcraft_model::End::Start) else { panic!() };
    assert!((x0 - 0.005).abs() < 1e-8 && (y0 - 0.005).abs() < 1e-8 && (a - 0.02286).abs() < 1e-8 && (b - 0.01016).abs() < 1e-8);
    let m = s.execute("mesh.generate", &json!({"size": 4})).unwrap();
    assert!(m["tetrahedra"].as_u64().unwrap() > 100, "{m}");
    // `keep` only with a cavity; a cavity takes no margin, and needs metal.
    let bad = [
        json!({"path": block_3mf(), "region": "box", "keep": "largest"}),
        json!({"path": block_3mf(), "region": "cavity", "margin": 1}),
        json!({"path": block_3mf(), "region": "cavity", "keep": "most"}),
        json!({"path": block_3mf(), "region": "cavity", "materials": {"Block": "vacuum"}}),
    ];
    for p in bad {
        let r = s.execute("geometry.import", &p);
        assert!(r.is_err() && !matches!(r, Err(EngineError::Panicked(_))), "{p}: {r:?}");
    }
    s.execute("geometry.import", &json!({"path": block_3mf(), "region": "cavity", "keep": "largest"})).unwrap();
}

/// A ridge guide drawn as five metal bodies (walls, floor, roof and a ridge on the floor): the
/// cavity's ends are no rectangles, so the ports' modes come from the numerical cross-section
/// solver, and the guide passes power through without loss.
#[test]
fn a_cavity_with_a_ridge_takes_numerical_port_modes() {
    let path = scratch("ridge.3mf");
    let (a, b, l) = (0.02286, 0.01016, 0.02);
    let parts = [
        named_box("Left", [-0.002, -0.002, 0.0], [0.0, b + 0.002, l]),
        named_box("Right", [a, -0.002, 0.0], [a + 0.002, b + 0.002, l]),
        named_box("Floor", [0.0, -0.002, 0.0], [a, 0.0, l]),
        named_box("Roof", [0.0, b, 0.0], [a, b + 0.002, l]),
        named_box("Ridge", [0.009, 0.0, 0.0], [0.01386, 0.004, l]),
    ];
    std::fs::write(&path, emcraft_io::threemf::write(&parts).unwrap()).unwrap();
    let mut s = Session::new();
    let r = s.execute("geometry.import", &json!({"path": path, "region": "cavity"})).unwrap();
    let exact = (22.86 * 10.16 - 4.86 * 4.0) * 20.0;
    assert!((r["pieces"]["kept"][0].as_f64().unwrap() - exact).abs() < 1e-6 * exact, "{r}");
    s.execute("model.port.create", &json!({"name": "1", "end": "start", "modes": 1})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end", "modes": 1})).unwrap();
    assert!(s.model.port_shape(emcraft_model::End::Start).is_err(), "no closed form for a ridge");
    s.execute("mesh.generate", &json!({"size": 4})).unwrap();
    let r = s.execute("solve.frequency", &json!({"frequencies": [8], "order": 1})).unwrap();
    let p = &r["points"][0];
    let power = p["s11"]["mag"].as_f64().unwrap().powi(2) + p["s21"]["mag"].as_f64().unwrap().powi(2);
    assert!((power - 1.0).abs() < 1e-6, "{p}");
}

/// Regression: the block in a box of air a micrometre beyond it left a sliver of air the mesher
/// spent minutes on (or gave up on); a gap under a tenth of the element size is now refused at
/// once, naming it.
#[test]
fn a_box_of_air_too_thin_to_mesh_is_refused_at_once() {
    let mut s = Session::new();
    s.execute("geometry.import", &json!({"path": block_3mf(), "region": "box", "margin": 0.001})).unwrap();
    let start = std::time::Instant::now();
    let e = s.execute("mesh.generate", &json!({"size": 3})).unwrap_err().to_string();
    assert!(e.contains("gap narrower than 0.300 mm"), "{e}");
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
}

#[test]
fn an_open_part_gets_air_and_a_layer_a_quarter_wavelength_each() {
    let path = scratch("open.3mf");
    let post = named_box("Post", [0.0, 0.0, 0.0], [0.002, 0.002, 0.01]);
    let puck = named_box("Puck", [0.006, 0.0, 0.002], [0.01, 0.004, 0.006]);
    std::fs::write(&path, emcraft_io::threemf::write(&[post, puck]).unwrap()).unwrap();
    let mut s = Session::new();
    // Without the band the defaults are unknown.
    assert!(s.execute("geometry.import", &json!({"path": path, "open": true})).is_err());
    s.execute("project.frequency", &json!({"start": 5, "stop": 10})).unwrap();
    let r = s.execute("geometry.import", &json!({"path": path, "open": true})).unwrap();
    // λ at 5 GHz is 59.96 mm: 15 mm of air, 15 mm of layer, each side.
    let t = r["open"].as_f64().unwrap();
    assert!((t - 14.99).abs() < 0.01, "{r}");
    let Some(emcraft_model::Solid { shape: emcraft_model::Shape::Box { size }, .. }) = &s.model.solid else { panic!() };
    assert!((size.x - (0.01 + 4.0 * 0.01499)).abs() < 1e-5, "{size:?}");
    assert_eq!(s.model.open.map(|o| o.shape), Some(emcraft_model::OpenShape::Box));
    // Given sizes; air must lie before the layer; a body as the region is no open problem.
    let r = s.execute("geometry.import", &json!({"path": path, "open": true, "margin": 20, "thickness": 8})).unwrap();
    assert_eq!(r["open"], json!(8.0));
    assert!(s.execute("geometry.import", &json!({"path": path, "open": true, "margin": 8, "thickness": 8})).is_err());
    assert!(s.execute("geometry.import", &json!({"path": path, "open": true, "region": "Post"})).is_err());
}

#[test]
fn coupled_strips_keep_apart_and_hostile_ones_are_refused() {
    // A parallel-coupled section: two strips side by side 0.3 mm apart over part of their length,
    // each ending inside the box; a T where a stub meets a line edge to edge.
    let mut s = Session::new();
    s.execute("model.material.create", &json!({"name": "ptfe", "eps_r": 2.2})).unwrap();
    let guide = |strips: serde_json::Value| json!({"width": 12, "height": 2, "sections": [{"length": 16, "material": "ptfe"}], "strips": strips});
    let a = json!({"y": 1, "x0": 4, "width": 1.5, "z0": 0, "z1": 12});
    s.execute("geometry.waveguide", &guide(json!([a, {"y": 1, "x0": 5.8, "width": 1.5, "z0": 4, "z1": 16}]))).unwrap();
    s.execute("geometry.waveguide", &guide(json!([a, {"y": 1, "x0": 5.5, "width": 3, "z0": 5, "z1": 6}]))).unwrap();
    let before = s.model.clone();
    // Overlapping, the same strip twice, a gap no mesh resolves across x and along z, NaN by an
    // expression, an infinite width: refused, the model as it was, nothing panics.
    for bad in [
        json!([a, {"y": 1, "x0": 5, "width": 1.5, "z0": 4, "z1": 16}]),
        json!([a, a]),
        json!([a, {"y": 1, "x0": 5.500001, "width": 1.5, "z0": 4, "z1": 16}]),
        json!([a, {"y": 1, "x0": 4, "width": 1.5, "z0": 12.0000001, "z1": 16}]),
        json!([a, {"y": 1, "x0": "=0/0", "width": 1.5, "z0": 4, "z1": 16}]),
        json!([a, {"y": 1, "x0": 6, "width": f64::INFINITY, "z0": 4, "z1": 16}]),
    ] {
        let r = s.execute("geometry.waveguide", &guide(bad.clone()));
        assert!(r.is_err() && !matches!(r, Err(EngineError::Panicked(_))), "{bad}: {r:?}");
        assert_eq!(s.model, before);
    }
}

#[test]
fn measure_reads_and_answers_in_display_units_and_changes_nothing() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [10, 6, 20]})).unwrap();
    let before = s.history().len();
    // The box's faces x = 0 and x = 10, picked at two corners: 10 apart across, the corners
    // further.
    let r = s.execute("geometry.measure", &json!({"from": [0, 0, 0], "to": [10, 6, 20], "from_normal": [-1, 0, 0], "to_normal": [1, 0, 0]})).unwrap();
    assert!((r["distance"].as_f64().unwrap() - 536f64.sqrt()).abs() < 1e-12, "{r}");
    assert_eq!(
        (r["delta"].clone(), r["gap"].as_f64(), r["angle"].as_f64(), r["unit"].as_str()),
        (json!([10.0, 6.0, 20.0]), Some(10.0), Some(0.0), Some("mm"))
    );
    // Two points: no gap, no angle.
    let r = s.execute("geometry.measure", &json!({"from": [0, 0, 0], "to": [0, 3, 4]})).unwrap();
    assert_eq!((r["distance"].as_f64(), r["gap"].clone(), r["angle"].clone()), (Some(5.0), json!(null), json!(null)));
    // In metres the same points read in metres.
    s.execute("units.set", &json!({"length": "m"})).unwrap();
    let r = s.execute("geometry.measure", &json!({"from": [0, 0, 0], "to": [0.5, 0, 0], "to_along": [0, 1, 0]})).unwrap();
    assert_eq!((r["distance"].as_f64(), r["gap"].as_f64(), r["unit"].as_str()), (Some(0.5), Some(0.5), Some("m")));
    assert_eq!(s.history().len(), before + 1, "only units.set is recorded");
    // Hostile picks: refused.
    for bad in [
        json!({"from": [0, 0, 0]}),
        json!({"from": [0, 0], "to": [0, 0, 0]}),
        json!({"from": [0, 0, "x"], "to": [0, 0, 0]}),
        json!({"from": [0, 0, 0], "to": [1, 0, 0], "to_normal": [0, 0, 0]}),
        json!({"from": [0, 0, 0], "to": [1, 0, 0], "from_normal": [0, 0, 1], "from_along": [1, 0, 0]}),
        json!({"from": [0, 0, 0], "to": [1, 0, 0], "from_along": ["a", 0, 0]}),
    ] {
        assert!(s.execute("geometry.measure", &bad).is_err(), "{bad}");
    }
}

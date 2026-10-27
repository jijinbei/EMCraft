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
    let m = s.execute("mesh.generate", &json!({"size": 4})).unwrap();
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

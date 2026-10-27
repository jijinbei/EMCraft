use serde_json::json;

use super::*;

fn scratch(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("emcraft-engine-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir.join(name).to_string_lossy().into_owned()
}

/// The sample with a coarse mesh and two frequencies, for speed.
fn small() -> Session {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    s.execute("mesh.generate", &json!({"size": 5})).unwrap();
    s.execute("solve.frequency", &json!({"frequencies": [5.5, 8.0], "order": 2})).unwrap();
    s
}

#[test]
fn the_sample_solves_and_matches_the_theory() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    s.execute("mesh.generate", &json!({"size": 4})).unwrap();
    let r = s.execute("solve.frequency", &json!({"frequencies": [6.0, 8.0]})).unwrap();
    assert_eq!(r["order"], 2);
    assert_eq!(r["points"].as_array().unwrap().len(), 2);
    let c = s.execute("results.compare", &json!({})).unwrap();
    assert!(c["worst"].as_f64().unwrap() < 4e-3, "{c}");
    // Frequencies are read and written in GHz.
    assert_eq!(c["points"][1]["frequency"], 8.0);
}

#[test]
fn display_units_convert_at_the_boundary() {
    let mut s = Session::new();
    s.execute("units.set", &json!({"length": "m", "frequency": "MHz"})).unwrap();
    s.execute("geometry.waveguide", &json!({"width": 0.02286, "height": 0.01016, "sections": [{"length": 0.03, "material": "vacuum"}]})).unwrap();
    let w = s.model.waveguide.as_ref().unwrap();
    assert!((w.width - 0.02286).abs() < 1e-15);
    s.execute("mesh.generate", &json!({"size": 0.005})).unwrap();
    s.execute("model.port.create", &json!({"name": "a", "end": "start"})).unwrap();
    s.execute("model.port.create", &json!({"name": "b", "end": "end"})).unwrap();
    let r = s.execute("solve.frequency", &json!({"frequencies": [9000], "order": 1})).unwrap();
    assert_eq!(r["frequency_unit"], "MHz");
    assert_eq!(r["points"][0]["frequency"], 9000.0);
    assert!((s.results.sweep.as_ref().unwrap().results[0].frequency - 9e9).abs() < 1e-3);
}

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
fn a_dispersive_slab_solves_against_its_exact_cascade() {
    // The comparison evaluates ε(ω) at each frequency.
    let mut s = Session::new();
    s.execute("model.material.create", &json!({"name": "d", "eps_r": 2, "debye": [{"delta_eps": 0.6, "frequency": 20}]})).unwrap();
    s.execute(
        "geometry.waveguide",
        &json!({"width": 22.86, "height": 10.16, "sections": [{"length": 10, "material": "vacuum"}, {"length": 20, "material": "d"}, {"length": 10, "material": "vacuum"}]}),
    )
    .unwrap();
    s.execute("mesh.generate", &json!({"size": 3.5})).unwrap();
    s.execute("model.port.create", &json!({"name": "1", "end": "start", "modes": 2})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end", "modes": 2})).unwrap();
    s.execute("solve.frequency", &json!({"frequencies": [8, 12], "order": 2})).unwrap();
    let c = s.execute("results.compare", &json!({})).unwrap();
    assert!(c["worst"].as_f64().unwrap() < 1e-2, "{c}");
}

#[test]
fn a_failed_command_leaves_the_session_as_it_was() {
    let mut s = small();
    let before = s.model.clone();
    // An unknown material fails after the command started changing things.
    let e = s.execute("geometry.waveguide", &json!({"width": 10, "height": 5, "sections": [{"length": 3, "material": "nothing"}]}));
    assert!(e.is_err());
    assert_eq!(s.model, before);
    assert!(s.results.sweep.is_some());
    assert!(matches!(s.execute("mesh.generate", &json!({"size": 1e-5})), Err(EngineError::Failed(_))));
    assert!(s.model.mesh.is_some() && s.results.sweep.is_some());
}

#[test]
fn hostile_parameters_are_refused_and_nothing_panics() {
    let garbage = [
        json!(null),
        json!([]),
        json!("x"),
        json!({"name": 1, "eps_r": "a", "width": -1, "height": f64::MAX, "sections": [{}], "size": 0, "end": "middle", "modes": 1e9,
               "frequencies": [-1, "a"], "start": 1e308, "stop": -5, "points": 1e12, "order": 7, "frequency": 1e300, "at": [[1, 2]],
               "path": "", "plane": "q", "component": "zz", "phase": 1e308, "width_px": 3, "excitation": "nope", "solve": "yes"}),
        json!({"frequency": 8, "at": [[1e300, 0, 0]], "path": "/nonexistent-dir/x.png", "width": 1_000_000, "height": 0}),
    ];
    let solved = small();
    for c in COMMANDS {
        for g in &garbage {
            let mut s = solved.clone();
            let r = s.execute(c.id, g);
            assert!(!matches!(r, Err(EngineError::Panicked(_))), "{} panicked on {g}", c.id);
        }
    }
    let mut s = Session::new();
    assert!(matches!(s.execute("no.such", &json!({})), Err(EngineError::UnknownCommand(_))));
    assert!(matches!(s.execute("mesh.generate", &json!({"size": 1, "bogus": 2})), Err(EngineError::Params(_))));
    assert!(matches!(s.execute("file.sample", &json!({"name": "nothing"})), Err(EngineError::Params(_))));
}

#[test]
fn pictures_are_written() {
    let mut s = small();
    for plane in ["h", "e", "cross"] {
        let path = scratch(&format!("{plane}.png"));
        let r = s.execute("view.field", &json!({"path": path, "frequency": 8, "plane": plane, "width": 500})).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']), "{r}");
    }
    let path = scratch("s.png");
    s.execute("view.sparameters", &json!({"path": path, "width": 600, "height": 400})).unwrap();
    assert!(std::fs::read(&path).unwrap().len() > 1000);
}

#[test]
fn field_views_show_the_te10_pattern() {
    let s = small();
    // Cross-section: E arrows point along ±y only; H arrows exist.
    let v = view::FieldView::new(8.0e9, view::Plane::Cross);
    let plot = view::field_plot(&s, &v).unwrap();
    let e = plot.arrows.iter().find(|a| a.label.starts_with('E')).unwrap();
    let (mx, my) = e.arrows.iter().fold((0.0f64, 0.0f64), |m, (_, v)| (m.0.max(v[0].abs()), m.1.max(v[1].abs())));
    assert!(mx < 0.1 * my, "{mx} {my}");
    assert!(plot.arrows.iter().any(|a| a.label.starts_with('H')));
    // H's arrows sit between E's, never on them.
    let h = plot.arrows.iter().find(|a| a.label.starts_with('H')).unwrap();
    assert!(h.arrows.iter().all(|(p, _)| e.arrows.iter().all(|(q, _)| (p[0] - q[0]).abs() + (p[1] - q[1]).abs() > 1e-6)));
    // A probe at the guide's middle reads TE10's E: along y, largest there, none at the walls.
    let samples = view::sample_plane(&s, &v).unwrap();
    let (x, y) = samples.axes();
    let mid = samples.probe([(x.min + x.max) / 2.0, (y.min + y.max) / 2.0]).unwrap();
    assert!(mid.e.y.norm() > 10.0 * (mid.e.x.norm() + mid.e.z.norm()), "{mid:?}");
    let wall = samples.probe([x.min, (y.min + y.max) / 2.0]).unwrap();
    assert!(wall.e.y.norm() < 0.05 * mid.e.y.norm(), "{wall:?}");
    let (_, amplitude) = mid.value(view::Component::Ey, 0.0);
    assert!((amplitude - mid.e.y.norm()).abs() < 1e-12);
    assert!(samples.probe([x.max + 1.0, y.min]).is_none() && samples.probe([f64::NAN, 0.0]).is_none());
    // H-plane: E has no in-plane part, so only H arrows; E_y largest across the middle (x = a/2).
    let plot = view::field_plot(&s, &view::FieldView::new(8.0e9, view::Plane::H)).unwrap();
    assert!(plot.arrows.iter().all(|a| a.label.starts_with('H')));
    let sc = plot.scalar.as_ref().unwrap();
    let col = sc.nx / 8; // in the first fill section
    let amp = |j: usize| sc.values[j * sc.nx + col].abs();
    let mid = amp(sc.ny / 2);
    assert!(mid > 5.0 * amp(1) && mid > 5.0 * amp(sc.ny - 2), "{mid}");
    // Two section boundaries drawn.
    assert_eq!(plot.lines.len(), 2);
}

#[test]
fn an_iris_is_compared_with_mode_matching() {
    let mut s = Session::new();
    s.execute(
        "geometry.waveguide",
        &json!({"width": 22.86, "height": 10.16, "sections": [{"length": 12, "material": "vacuum"}, {"length": 2, "material": "vacuum", "width": 12}, {"length": 12, "material": "vacuum"}]}),
    )
    .unwrap();
    s.execute("mesh.generate", &json!({"size": 5, "size_y": 10.16})).unwrap();
    s.execute("model.port.create", &json!({"name": "1", "end": "start"})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end"})).unwrap();
    s.execute("solve.frequency", &json!({"frequencies": [10], "order": 2})).unwrap();
    let c = s.execute("results.compare", &json!({"modes": 60})).unwrap();
    assert_eq!(c["reference"], "mode matching");
    assert!(c["worst"].as_f64().unwrap() < 5e-2, "{c}");
    // Exact fields exist only for uniform guides.
    assert!(matches!(s.execute("results.field_error", &json!({"frequency": 10})), Err(EngineError::Failed(_))));
    // The iris' metal is drawn grey on the H-plane.
    let plot = view::field_plot(&s, &view::FieldView::new(10.0e9, view::Plane::H)).unwrap();
    assert!(plot.scalar.as_ref().unwrap().values.iter().any(|v| v.is_nan()));
    // The band-pass sample opens (its sweep is heavy: the oracle runs it).
    let mut b = Session::new();
    let r = b.execute("file.sample", &json!({"name": "iris-bandpass"})).unwrap();
    assert_eq!(r["waveguide"]["sections"].as_array().unwrap().len(), 9);
}

#[test]
fn cavity_eigenmodes_and_their_pictures() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "cavity"})).unwrap();
    s.execute("mesh.generate", &json!({"size": 5})).unwrap();
    // The two modes nearest 9 GHz: TE101 (8.24) and TE102 (11.95).
    let r = s.execute("solve.eigenmodes", &json!({"near": 9, "count": 2})).unwrap();
    let modes = r["modes"].as_array().unwrap();
    assert_eq!(modes.len(), 2);
    let (f, exact) = (modes[0]["frequency"].as_f64().unwrap(), r["exact"][0]["frequency"].as_f64().unwrap());
    assert_eq!(r["exact"][0]["mode"], "TE101");
    assert!((f - exact).abs() < 1e-3 * exact, "{f} {exact}");
    assert!(modes[0]["q_walls"].as_f64().unwrap() > 5000.0);
    let path = scratch("mode1.png");
    s.execute("view.field", &json!({"path": path, "mode": 1, "plane": "cross"})).unwrap();
    assert!(std::fs::read(&path).unwrap().starts_with(&[0x89, b'P', b'N', b'G']));
    assert!(s.execute("view.field", &json!({"path": path, "mode": 9})).is_err());
    assert!(s.execute("solve.eigenmodes", &json!({"near": 12, "count": 0})).is_err());
}

#[test]
fn time_domain_results_join_the_chart() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    let r = s.execute("solve.time_domain", &json!({"frequencies": [6, 8], "cell": 2})).unwrap();
    assert_eq!(r["points"].as_array().unwrap().len(), 2);
    assert!(r["converged"].as_bool().unwrap());
    let s21 = r["points"][1]["s21"]["mag"].as_f64().unwrap();
    assert!((s21 - 0.8149).abs() < 0.05, "{s21}");
    // Without a frequency-domain sweep the chart shows the time-domain curves alone.
    let chart = view::sparameter_chart(&s).unwrap();
    assert!(chart.series.iter().any(|c| c.label.contains("time domain")));
    assert!(s.execute("solve.time_domain", &json!({"frequencies": [6], "cell": -1})).is_err());
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

#[test]
fn features_page_lists_every_command() {
    let md = features_markdown();
    for c in COMMANDS {
        assert!(md.contains(&format!("`{}`", c.id)));
    }
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

#[test]
fn open_problems_radiate_and_scatter() {
    let mut s = Session::new();
    // A layer needs a box, and room inside it.
    s.execute("geometry.solid", &json!({"shape": "sphere", "radius": 10})).unwrap();
    assert!(s.execute("model.open", &json!({"thickness": 3})).is_err());
    s.execute("geometry.solid", &json!({"shape": "box", "size": [40, 40, 40]})).unwrap();
    assert!(s.execute("model.open", &json!({"thickness": 25})).is_err());
    assert!(s.execute("model.open", &json!({"thickness": 10, "reflection": 2})).is_err());
    assert!(s.execute("model.open", &json!({"thickness": 10, "reflection": 0})).is_err());
    s.execute("model.open", &json!({"thickness": 10})).unwrap();
    s.execute("mesh.generate", &json!({"per_wavelength": 4, "frequency": 10})).unwrap();
    assert!(s.execute("solve.radiation", &json!({"frequency": 10})).is_err(), "no sources");
    assert!(s.execute("results.far_field", &json!({})).is_err(), "nothing solved");
    assert!(s.execute("source.dipole", &json!({"at": [50, 20, 20], "length": 1})).is_err(), "outside");
    assert!(s.execute("source.dipole", &json!({"at": [20, 20, 20], "length": 1, "direction": [0, 0, 0]})).is_err());
    s.execute("source.dipole", &json!({"at": [20.1, 20.2, 19.9], "length": 1})).unwrap();
    let r = s.execute("solve.radiation", &json!({"frequency": 10})).unwrap();
    // Coarse (λ/4, a third of a wavelength of layer): the plumbing, not the accuracy.
    let (p, exact) = (r["radiated_power"].as_f64().unwrap(), r["exact_radiated_power"].as_f64().unwrap());
    assert!((p / exact - 1.0).abs() < 0.2, "{p} {exact}");
    let ff = s.execute("results.far_field", &json!({"theta": [90, 0.0], "phi": 30})).unwrap();
    let d = ff["points"][0]["directivity"].as_f64().unwrap();
    assert!((d / 1.5 - 1.0).abs() < 0.2, "{d}");
    assert!(ff["points"][1]["directivity"].as_f64().unwrap() < 0.1, "the axis is a null");
    let path = scratch("pattern.png");
    s.execute("view.pattern", &json!({"path": path, "points": 37})).unwrap();
    assert!(std::fs::read(&path).unwrap().len() > 1000);
    // A plane wave on a conducting sphere: the Mie series' cross-section comes with it.
    s.execute("source.clear", &json!({})).unwrap();
    assert!(s.execute("source.plane_wave", &json!({"direction": [0, 0, 1], "polarization": [0, 0, 1]})).is_err(), "not transverse");
    s.execute("geometry.inclusion", &json!({"shape": "sphere", "radius": 3, "at": [20, 20, 20]})).unwrap();
    s.execute("model.open", &json!({"thickness": 10})).unwrap();
    s.execute("mesh.generate", &json!({"per_wavelength": 4, "frequency": 10})).unwrap();
    s.execute("source.plane_wave", &json!({})).unwrap();
    let r = s.execute("solve.radiation", &json!({"frequency": 10})).unwrap();
    let (sigma, exact) = (r["scattering_cross_section"].as_f64().unwrap(), r["exact_scattering_cross_section"].as_f64().unwrap());
    assert!((sigma / exact - 1.0).abs() < 0.3, "{sigma} {exact}");
    // Ports and an open boundary do not mix.
    let mut g = Session::new();
    g.execute("geometry.solid", &json!({"shape": "box", "size": [22.86, 10.16, 30]})).unwrap();
    g.execute("mesh.generate", &json!({"size": 5})).unwrap();
    g.execute("model.port.create", &json!({"name": "1", "end": "start"})).unwrap();
    assert!(g.execute("model.open", &json!({"thickness": 2})).is_err());
    s.execute("source.clear", &json!({})).unwrap();
    assert!(s.execute("model.port.create", &json!({"name": "1", "end": "start"})).is_err());
}

#[test]
fn every_sample_is_a_script_of_known_commands() {
    for smp in crate::sample::SAMPLES {
        let script: serde_json::Value = serde_json::from_str(smp.script).unwrap_or_else(|e| panic!("{}: {e}", smp.name));
        for step in script.as_array().unwrap() {
            let id = step["command"].as_str().unwrap();
            assert!(crate::find_command(id).is_some(), "{}: {id}", smp.name);
        }
    }
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn every_sample_opens() {
    // Without their solve steps: the geometry, mesh, ports and sources of each.
    for smp in crate::sample::SAMPLES {
        let mut s = Session::new();
        s.execute("file.sample", &json!({"name": smp.name})).unwrap_or_else(|e| panic!("{}: {e}", smp.name));
        assert!(s.model.mesh.is_some(), "{}", smp.name);
    }
}

#[test]
fn a_lumped_gap_radiates_as_a_hertzian_dipole() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [40, 40, 40]})).unwrap();
    s.execute("model.open", &json!({"thickness": 10})).unwrap();
    s.execute("mesh.generate", &json!({"per_wavelength": 4, "frequency": 10})).unwrap();
    // Refused: outside, no gap, no impedance, a duplicate name.
    assert!(s.execute("model.port.lumped", &json!({"name": "x", "from": [20, 20, 50], "to": [20, 20, 51]})).is_err());
    assert!(s.execute("model.port.lumped", &json!({"name": "x", "from": [20, 20, 20], "to": [20, 20, 20]})).is_err());
    assert!(s.execute("model.port.lumped", &json!({"name": "x", "from": [20, 20, 19.5], "to": [20, 20, 20.5], "impedance": -50})).is_err());
    // A current source: 1 A through a 1 mm gap.
    s.execute("model.port.lumped", &json!({"name": "gap", "from": [20.1, 20.2, 19.4], "to": [20.1, 20.2, 20.4], "impedance": 1e9, "voltage": 1e9}))
        .unwrap();
    assert!(s.execute("model.port.lumped", &json!({"name": "gap", "from": [21, 20, 19], "to": [21, 20, 20]})).is_err());
    let r = s.execute("solve.radiation", &json!({"frequency": 10})).unwrap();
    // Re Z = 80π²(l/λ)², the power balance, coarse as the mesh is.
    let r11 = r["ports"][0]["impedance"]["re"].as_f64().unwrap();
    assert!((r11 / 0.8785 - 1.0).abs() < 0.1, "{r11}");
    assert!((r["radiation_efficiency"].as_f64().unwrap() - 1.0).abs() < 0.03);
    let p = &r["ports"][0];
    let (zr, zi) = (p["impedance"]["re"].as_f64().unwrap(), p["impedance"]["im"].as_f64().unwrap());
    let expect = (((zr - 1e9).powi(2) + zi * zi) / ((zr + 1e9).powi(2) + zi * zi)).sqrt();
    assert!((p["s11"]["mag"].as_f64().unwrap() - expect).abs() < 1e-9);
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn lumped_ports_feed_antennas() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [60, 60, 60]})).unwrap();
    s.execute("geometry.inclusion", &json!({"shape": "cylinder", "radius": 0.5, "length": 7, "at": [30, 30, 22.5]})).unwrap();
    s.execute("geometry.inclusion", &json!({"shape": "cylinder", "radius": 0.5, "length": 7, "at": [30, 30, 30.5]})).unwrap();
    s.execute("model.open", &json!({"thickness": 15})).unwrap();
    s.execute("mesh.generate", &json!({"per_wavelength": 3, "frequency": 10})).unwrap();
    s.execute("model.port.lumped", &json!({"name": "feed", "from": [30, 30, 29.5], "to": [30, 30, 30.5], "radius": 0.5})).unwrap();
    let r = s.execute("solve.radiation", &json!({"frequency": 10})).unwrap();
    // Coarse (λ/3): the plumbing. The power balance holds whatever the mesh.
    let eff = r["radiation_efficiency"].as_f64().unwrap();
    assert!((eff - 1.0).abs() < 0.03, "{eff}");
    let p = &r["ports"][0];
    let z = (p["impedance"]["re"].as_f64().unwrap(), p["impedance"]["im"].as_f64().unwrap());
    assert!(z.0 > 50.0 && z.0 < 200.0, "{z:?}");
    let s11 = p["s11"]["mag"].as_f64().unwrap();
    let (zr, zi) = z;
    let expect = (((zr - 50.0).powi(2) + zi * zi) / ((zr + 50.0).powi(2) + zi * zi)).sqrt();
    assert!((s11 - expect).abs() < 1e-12);
    assert!(p["induced_emf_impedance"]["re"].as_f64().is_some());
}

#[test]
fn ports_on_faces_go_with_open_boundaries() {
    let mut s = Session::new();
    // A face port needs a solid; with an open box ports must be on faces.
    s.execute("geometry.waveguide", &json!({"width": 22.86, "height": 10.16, "sections": [{"length": 10, "material": "vacuum"}]})).unwrap();
    assert!(s.execute("model.port.create", &json!({"name": "1", "end": "start", "face": [5, 0, 0, 22.86, 10.16]})).is_err());
    s.execute("geometry.solid", &json!({"shape": "box", "size": [60, 60, 60]})).unwrap();
    s.execute("model.open", &json!({"thickness": 10})).unwrap();
    assert!(s.execute("model.port.create", &json!({"name": "1", "end": "start"})).is_err());
    assert!(s.execute("model.port.create", &json!({"name": "1", "end": "start", "face": [5, 0, 0, -1, 10]})).is_err());
    assert!(s.execute("model.port.create", &json!({"name": "1", "end": "start", "face": [5, 0, 0]})).is_err());
    // A face where the mesh has no boundary: refused when solving, nothing panics.
    s.execute("model.port.create", &json!({"name": "1", "end": "start", "face": [30, 20, 20, 20, 10]})).unwrap();
    s.execute("mesh.generate", &json!({"per_wavelength": 3, "frequency": 10})).unwrap();
    let e = s.execute("solve.radiation", &json!({"frequency": 10})).unwrap_err();
    assert!(e.to_string().contains("no boundary face"), "{e}");
    // Eigenmodes of an open model are refused.
    assert!(s.execute("solve.eigenmodes", &json!({"near": 10})).is_err());
}

#[test]
fn the_time_domain_solves_open_problems() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [40, 40, 40]})).unwrap();
    s.execute("model.open", &json!({"thickness": 10})).unwrap();
    s.execute("model.port.lumped", &json!({"name": "gap", "from": [20, 20, 19], "to": [20, 20, 21], "impedance": 1e9, "voltage": 1e9})).unwrap();
    let r = s.execute("solve.time_domain", &json!({"frequencies": [10], "cell": 2})).unwrap();
    let p = &r["points"][0];
    // 80π²(l/λ)² for 2 mm: 3.51 Ω, coarse grid.
    let r11 = p["ports"][0]["impedance"]["re"].as_f64().unwrap();
    assert!((r11 / 3.514 - 1.0).abs() < 0.15, "{r11}");
    assert!((p["radiation_efficiency"].as_f64().unwrap() - 1.0).abs() < 0.05);
    let ff = s.execute("results.far_field", &json!({"theta": [90], "solver": "time_domain"})).unwrap();
    assert!((ff["points"][0]["directivity"].as_f64().unwrap() / 1.5 - 1.0).abs() < 0.1);
    assert!(s.execute("results.far_field", &json!({"solver": "fem"})).is_err(), "no FEM solve yet");
    assert!(s.execute("results.far_field", &json!({"solver": "magic"})).is_err());
    let path = scratch("td-pattern.png");
    s.execute("view.pattern", &json!({"path": path, "points": 19})).unwrap();
}

#[test]
fn circular_and_coaxial_guides_have_exact_references() {
    // An empty cylinder: TE11 (cut-off 8.79 GHz for r = 10 mm), S21 = e^{−jβL}, no S11; with a
    // coaxial conductor subtracted through it, TEM at k√εr; anything else has none.
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 30})).unwrap();
    let r = crate::view::reference(&s).unwrap();
    let (s11, s21) = r.s(12e9, 1).unwrap();
    let kc = 1.841_183_781_340_659 / 0.01;
    let k = 2.0 * std::f64::consts::PI * 12e9 / emcraft_model::units::C0;
    assert!(s11.norm() == 0.0 && (s21 - emcraft_solver::Complex64::from_polar(1.0, -(k * k - kc * kc).sqrt() * 0.03)).norm() < 1e-12);
    assert!(r.s(8e9, 1).is_err(), "below cut-off");
    s.execute("geometry.feature", &json!({"combine": "subtract", "shape": "cylinder", "radius": 3, "length": 30, "at": [0, 0, 0]})).unwrap();
    let (_, t) = crate::view::reference(&s).unwrap().s(3e9, 1).unwrap();
    assert!((t - emcraft_solver::Complex64::from_polar(1.0, -2.0 * std::f64::consts::PI * 3e9 / emcraft_model::units::C0 * 0.03)).norm() < 1e-12);
    s.execute("geometry.feature", &json!({"combine": "subtract", "shape": "cylinder", "radius": 1, "length": 30, "at": [5, 0, 0]})).unwrap();
    assert!(crate::view::reference(&s).is_err());
}

#[test]
fn planar_circuits_are_layered_guides_with_strips() {
    // A microstrip in a box: its ports' modes come from the cross-section solver; hostile
    // layers and strips are refused, and nothing panics.
    let mut s = Session::new();
    s.execute("model.material.create", &json!({"name": "fr4", "eps_r": 4.4})).unwrap();
    let guide = |layers: Value, strips: Value| json!({"width": 8, "height": 4, "sections": [{"length": 1, "material": "vacuum"}], "layers": layers, "strips": strips});
    s.execute("geometry.waveguide", &guide(json!([{"thickness": 0.8, "material": "fr4"}]), json!([{"y": 0.8, "x0": 3.25, "width": 1.5}]))).unwrap();
    s.execute("mesh.generate", &json!({"size": 0.8})).unwrap();
    s.execute("model.port.create", &json!({"name": "1", "end": "start", "modes": 2})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end", "modes": 2})).unwrap();
    let r = s.execute("solve.port_modes", &json!({"frequency": 2})).unwrap();
    let m = &r["ports"][0]["modes"][0];
    // Between the air's 1 and the substrate's 4.4, nearer the substrate for a wide strip.
    let eps = m["eps_eff"].as_f64().unwrap();
    assert!(m["propagating"].as_bool().unwrap() && eps > 2.7 && eps < 4.4 && !m["closed_form"].as_bool().unwrap(), "{r}");
    assert!(!r["ports"][0]["modes"][1]["propagating"].as_bool().unwrap());
    let sw = s.execute("solve.frequency", &json!({"frequencies": [2], "order": 1})).unwrap();
    assert!(sw["points"][0]["s21"]["mag"].as_f64().unwrap() > 0.99, "{sw}");
    // The time domain does not take strips.
    assert!(s.execute("solve.time_domain", &json!({"frequencies": [2]})).is_err());
    for (layers, strips) in [
        (json!([{"thickness": 5, "material": "fr4"}]), json!([])),
        (json!([{"thickness": -1, "material": "fr4"}]), json!([])),
        (json!([{"thickness": 1, "material": "nothing"}]), json!([])),
        (json!([]), json!([{"y": 4, "x0": 1, "width": 1}])),
        (json!([]), json!([{"y": 1, "x0": 7.5, "width": 1}])),
        (json!([]), json!([{"y": 1, "x0": 1, "width": 1, "z0": 0.5, "z1": 0.2}])),
        (json!([]), json!([{"y": f64::MAX, "x0": 1, "width": 1}])),
        (json!("layers"), json!([])),
    ] {
        assert!(s.execute("geometry.waveguide", &guide(layers.clone(), strips.clone())).is_err(), "{layers} {strips}");
    }
    assert!(s.execute("solve.port_modes", &json!({"frequency": -1})).is_err());
}

#[test]
fn the_workflow_status_guides_a_job_and_undo_saves_and_exports_work() {
    let mut s = Session::new();
    let status = |s: &mut Session| -> Vec<(String, String)> {
        let r = s.execute("project.status", &json!({})).unwrap();
        r["steps"].as_array().unwrap().iter().map(|x| (x["step"].as_str().unwrap().to_owned(), x["status"].as_str().unwrap().to_owned())).collect()
    };
    let of = |v: &[(String, String)], step: &str| v.iter().find(|(n, _)| n == step).map(|x| x.1.clone()).unwrap();
    // An empty session: geometry, excitation and mesh missing; no band yet.
    let st = status(&mut s);
    assert_eq!(of(&st, "geometry"), "missing");
    assert_eq!(of(&st, "intent"), "warning");
    assert_eq!(of(&st, "run"), "missing");
    // The band below WR-90's cut-off is flagged at the port.
    s.execute("project.frequency", &json!({"start": 5, "stop": 12})).unwrap();
    s.execute("geometry.waveguide", &json!({"width": 22.86, "height": 10.16, "sections": [{"length": 20, "material": "vacuum"}]})).unwrap();
    s.execute("model.port.create", &json!({"name": "1", "end": "start"})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end"})).unwrap();
    let st = status(&mut s);
    assert_eq!(of(&st, "excitation"), "warning", "{st:?}");
    assert_eq!(of(&st, "mesh"), "missing");
    s.execute("project.frequency", &json!({"start": 8, "stop": 12})).unwrap();
    s.execute("mesh.generate", &json!({"per_wavelength": 6})).unwrap();
    let st = status(&mut s);
    assert_eq!(of(&st, "excitation"), "done", "{st:?}");
    assert_eq!(of(&st, "mesh"), "done", "{st:?}");
    let est = s.execute("project.estimate", &json!({"order": 1})).unwrap();
    assert!(est["unknowns"].as_f64().unwrap() > 100.0 && est["memory_mb"].as_f64().is_some());
    // A sweep over the band by default; then results are done, and stale after a change.
    s.execute("solve.frequency", &json!({"points": 3, "order": 1})).unwrap();
    assert_eq!(of(&status(&mut s), "results"), "done");
    let check = s.execute("results.check", &json!({})).unwrap();
    assert!(check["passive"].as_bool().unwrap() && check["reciprocity_error"].as_f64().unwrap() < 1e-9, "{check}");
    let ts = scratch("guide.s2p");
    s.execute("results.touchstone", &json!({"path": ts})).unwrap();
    let text = std::fs::read_to_string(&ts).unwrap();
    assert!(text.contains("# GHz S RI R 50") && text.lines().filter(|l| !l.starts_with(['!', '#'])).count() == 3, "{text}");
    s.execute("units.set", &json!({"length": "m"})).unwrap();
    assert_eq!(of(&status(&mut s), "results"), "stale");
    // Undo, redo; the history rebuilds the model.
    let n = s.history.len();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.history.len(), n - 1);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.history.len(), n);
    let path = scratch("job.emcraft.json");
    s.execute("file.save", &json!({"path": path})).unwrap();
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": path})).unwrap();
    assert_eq!(t.model.band, s.model.band);
    assert_eq!(t.model.mesh.as_ref().map(|m| m.tets.len()), s.model.mesh.as_ref().map(|m| m.tets.len()));
    // Back to the empty session before the open.
    t.execute("edit.undo", &json!({})).unwrap();
    assert!(t.model.waveguide.is_none());
    // Hostile input is refused.
    assert!(s.execute("project.frequency", &json!({"start": 9, "stop": 2})).is_err());
    assert!(s.execute("project.frequency", &json!({"start": -1, "stop": 2})).is_err());
    assert!(s.execute("file.open", &json!({"path": ts})).is_err());
    assert!(s.execute("results.touchstone", &json!({"path": ts, "format": "db"})).is_err());
    assert!(Session::new().execute("edit.undo", &json!({})).is_err());
}

#[test]
fn every_choice_a_command_lists_is_one_its_parameter_reads() {
    use serde::de::DeserializeOwned;
    fn reads<T: DeserializeOwned>(id: &str, name: &str) -> usize {
        let spec = find_command(id).and_then(|c| c.params.iter().find(|p| p.name == name)).unwrap();
        assert!(!spec.choices.is_empty(), "{id} {name}");
        for c in spec.choices {
            assert!(serde_json::from_value::<T>(json!(c)).is_ok(), "{id} {name}: {c}");
        }
        spec.choices.len()
    }
    reads::<emcraft_model::units::Length>("units.set", "length");
    reads::<emcraft_model::units::Frequency>("units.set", "frequency");
    for id in ["geometry.solid", "geometry.inclusion", "geometry.feature"] {
        reads::<crate::geometry::Primitive>(id, "shape");
    }
    reads::<emcraft_model::Combine>("geometry.feature", "combine");
    reads::<emcraft_model::Axis>("geometry.feature", "axis");
    reads::<emcraft_model::End>("model.port.create", "end");
    reads::<emcraft_model::ModeSolver>("model.port.create", "mode_solver");
    reads::<crate::solve::Strategy>("solve.frequency", "sweep");
    reads::<crate::view::Plane>("view.field", "plane");
    reads::<crate::open::FarSolver>("results.far_field", "solver");
    reads::<crate::open::FarSolver>("view.pattern", "solver");
    reads::<crate::project::Format>("results.touchstone", "format");
    let component = find_command("view.field").and_then(|c| c.params.iter().find(|p| p.name == "component")).unwrap();
    let names: Vec<&str> = crate::view::Component::ALL.iter().map(|c| c.name()).collect();
    assert_eq!(component.choices, names.as_slice());
    assert!(component.choices.iter().all(|c| c.parse::<crate::view::Component>().is_ok()));
}

#[test]
fn projects_save_as_emcraft_files_and_know_when_they_changed() {
    let dir = std::env::temp_dir().join(format!("emcraft-project-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut s = Session::new();
    assert!(!s.is_dirty() && s.name() == "Untitled");
    // No file yet: a path is needed.
    assert!(matches!(s.execute("file.save", &json!({})), Err(EngineError::Params(_))));
    s.execute("model.material.create", &json!({"name": "ptfe", "eps_r": 2.1})).unwrap();
    assert!(s.is_dirty());
    let base = dir.join("filter").to_string_lossy().into_owned();
    let r = s.execute("file.save", &json!({"path": base})).unwrap();
    let path = format!("{base}.emcraft");
    assert_eq!(r["path"], json!(path));
    assert!(!s.is_dirty() && s.name() == "filter" && std::path::Path::new(&path).exists());
    // Changed, then saved again to its own file; undo back to the saved state is clean.
    s.execute("project.frequency", &json!({"start": 5, "stop": 9})).unwrap();
    assert!(s.is_dirty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(!s.is_dirty());
    s.execute("project.frequency", &json!({"start": 5, "stop": 9})).unwrap();
    s.execute("file.save", &json!({})).unwrap();
    assert!(!s.is_dirty());
    // Open: the same model, clean, with its path; new: empty and untitled, units kept.
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": path})).unwrap();
    assert!(!t.is_dirty() && t.path.as_deref() == Some(path.as_str()) && t.model.band.is_some());
    t.execute("units.set", &json!({"frequency": "MHz"})).unwrap();
    t.execute("file.new", &json!({})).unwrap();
    assert!(t.path.is_none() && !t.is_dirty() && t.model.band.is_none());
    assert_eq!(t.display.frequency, emcraft_model::units::Frequency::Megahertz);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn symmetry_planes_halve_the_model_and_keep_its_s_parameters() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    s.execute("mesh.generate", &json!({"size": 4})).unwrap();
    let whole = s.execute("solve.frequency", &json!({"frequencies": [6.0, 8.0]})).unwrap();
    let r = s.execute("model.symmetry", &json!({"axis": "x", "kind": "magnetic"})).unwrap();
    assert_eq!(r["planes"][0]["at"], 11.43);
    assert!(s.model.mesh.is_none() && s.results.sweep.is_none(), "the mesh and results go");
    s.execute("model.symmetry", &json!({"axis": "y", "kind": "magnetic"})).unwrap();
    let r = s.execute("model.symmetry", &json!({"axis": "y", "kind": "electric", "at": 5.08})).unwrap();
    assert_eq!(r["fraction"], 0.25, "{r}");
    assert_eq!(r["planes"][1]["kind"], "electric", "a new plane replaces the old");
    s.execute("mesh.generate", &json!({"size": 4})).unwrap();
    let quarter = s.execute("solve.frequency", &json!({"frequencies": [6.0, 8.0]})).unwrap();
    let (nw, nq) = (whole["unknowns"].as_u64().unwrap(), quarter["unknowns"].as_u64().unwrap());
    assert!(nq * 3 < nw, "{nq} of {nw}");
    let c = s.execute("results.compare", &json!({})).unwrap();
    assert!(c["worst"].as_f64().unwrap() < 4e-3, "{c}");
    // The field view shows the whole guide: images beyond the planes.
    let v = view::FieldView::new(8.0e9, view::Plane::Cross);
    let samples = view::sample_plane(&s, &v).unwrap();
    let (x, y) = samples.axes();
    let left = samples.probe([x.min + 0.3 * (x.max - x.min), y.min + 0.4 * (y.max - y.min)]).unwrap();
    let right = samples.probe([x.min + 0.7 * (x.max - x.min), y.min + 0.6 * (y.max - y.min)]).unwrap();
    assert!((left.e.y - right.e.y).norm() < 2e-2 * left.e.y.norm(), "{left:?} {right:?}");
    // The time domain refuses; clearing one plane, then all.
    assert!(s.execute("solve.time_domain", &json!({"frequencies": [8.0]})).is_err());
    let r = s.execute("model.symmetry", &json!({"clear": true, "axis": "y"})).unwrap();
    assert_eq!(r["planes"].as_array().unwrap().len(), 1);
    let r = s.execute("model.symmetry", &json!({"clear": true})).unwrap();
    assert_eq!(r["planes"].as_array().unwrap().len(), 0);
}

#[test]
fn hostile_symmetry_planes_are_refused() {
    let mut s = Session::new();
    assert!(s.execute("model.symmetry", &json!({"axis": "x", "kind": "magnetic"})).is_err(), "no geometry");
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    for bad in [
        json!({"axis": "z", "kind": "magnetic"}),
        json!({"axis": "x", "kind": "nothing"}),
        json!({"axis": "x"}),
        json!({"kind": "electric"}),
        json!({"axis": "x", "kind": "electric", "at": 5}),
        json!({"axis": "x", "kind": "electric", "at": 1e308}),
        json!({"axis": "x", "kind": "electric", "at": "middle"}),
        json!({"axis": 1, "kind": "electric"}),
        json!({"clear": "yes"}),
        json!({"clear": true, "kind": "electric"}),
    ] {
        let before = s.model.clone();
        let r = s.execute("model.symmetry", &bad);
        assert!(matches!(r, Err(EngineError::Params(_)) | Err(EngineError::Failed(_))), "{bad}: {r:?}");
        assert_eq!(s.model, before);
    }
    // A solid is refused; an asymmetric guide too, across x only.
    s.execute("geometry.solid", &json!({"shape": "box", "size": [20, 10, 30]})).unwrap();
    assert!(s.execute("model.symmetry", &json!({"axis": "x", "kind": "magnetic"})).is_err());
    let step = json!({"width": 20, "height": 10, "sections": [{"length": 5, "material": "vacuum"}, {"length": 2, "material": "vacuum", "width": 8, "offset": 0}]});
    s.execute("geometry.waveguide", &step).unwrap();
    assert!(s.execute("model.symmetry", &json!({"axis": "x", "kind": "magnetic"})).is_err());
    assert!(s.execute("model.symmetry", &json!({"axis": "y", "kind": "electric"})).is_ok(), "symmetric across y");
}

/// The inductive post through WR-90, its ports made, not meshed.
fn post_unmeshed() -> Session {
    let mut s = Session::new();
    for (c, p) in [
        ("geometry.solid", json!({"shape": "box", "size": [22.86, 10.16, 30]})),
        ("geometry.feature", json!({"combine": "subtract", "shape": "cylinder", "radius": 1.5, "length": 10.16, "at": [11.43, 0, 15], "axis": "y"})),
        ("model.port.create", json!({"name": "1", "end": "start", "modes": 4})),
        ("model.port.create", json!({"name": "2", "end": "end", "modes": 4})),
    ] {
        s.execute(c, &p).unwrap();
    }
    s
}

#[test]
fn adaptive_passes_refine_a_solid_deterministically() {
    let run = || {
        let mut s = post_unmeshed();
        let r = s.execute("mesh.adapt", &json!({"size": 8, "frequency": 10, "passes": 2, "order": 1, "tolerance": 1e-9})).unwrap();
        (r, s.model.mesh.clone().unwrap())
    };
    let (r, mesh) = run();
    let passes = r["passes"].as_array().unwrap();
    assert_eq!(passes.len(), 2, "{r}");
    assert_eq!(r["refinement"], "adaptive");
    assert_eq!(r["converged"], false);
    assert_eq!(r["stopped"], "passes");
    assert!(passes[0]["delta_s"].is_null() && passes[1]["delta_s"].as_f64().unwrap() > 0.0, "{r}");
    assert!(passes[0]["marked"].as_u64().unwrap() > 0);
    // Remeshed (the marked parts finer; the solver's tests check where).
    assert_ne!(passes[1]["tetrahedra"], passes[0]["tetrahedra"], "{r}");
    assert_eq!(r["tetrahedra"], passes[1]["tetrahedra"]);
    assert_eq!(r["s11"], passes[1]["s11"]);
    // The same input, the same mesh, bit for bit.
    let (r2, mesh2) = run();
    assert_eq!(mesh.tets, mesh2.tets);
    assert!(
        mesh.nodes
            .iter()
            .zip(&mesh2.nodes)
            .all(|(a, b)| a.x.to_bits() == b.x.to_bits() && a.y.to_bits() == b.y.to_bits() && a.z.to_bits() == b.z.to_bits())
    );
    assert_eq!(r["s11"], r2["s11"]);
}

#[test]
fn adaptive_passes_refine_a_waveguide_uniformly_until_s_settles() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    let r = s.execute("mesh.adapt", &json!({"size": 8, "frequency": 8, "passes": 4, "order": 2, "tolerance": 0.05})).unwrap();
    assert_eq!(r["refinement"], "uniform");
    let passes = r["passes"].as_array().unwrap();
    assert!(r["converged"] == true && passes.len() >= 2 && passes.len() <= 4, "{r}");
    assert!(passes.windows(2).all(|w| w[1]["unknowns"].as_u64() > w[0]["unknowns"].as_u64()), "{r}");
    assert!(passes.last().unwrap()["delta_s"].as_f64().unwrap() < 0.05);
    // The final mesh stays and solves.
    assert_eq!(s.model.mesh.as_ref().map(|m| m.tets.len() as u64), r["tetrahedra"].as_u64());
    assert!(s.execute("solve.frequency", &json!({"frequencies": [8], "order": 2})).is_ok());
}

#[test]
fn hostile_adaptive_requests_are_refused() {
    let mut s = post_unmeshed();
    for p in [
        json!({}),
        json!({"size": 0}),
        json!({"size": 8, "passes": 0}),
        json!({"size": 8, "passes": 13}),
        json!({"size": 8, "passes": 1e12}),
        json!({"size": 8, "tolerance": -1}),
        json!({"size": 8, "tolerance": 2}),
        json!({"size": 8, "tolerance": 0}),
        json!({"size": 8, "fraction": 0}),
        json!({"size": 8, "fraction": 1.5}),
        json!({"size": 8, "frequency": -1}),
        json!({"size": 8, "frequency": 1e300}),
        json!({"size": 8, "order": 3}),
        json!({"size": 1e-9, "frequency": 10}),
        json!({"size": 8, "frequency": 10, "bogus": 1}),
    ] {
        let r = s.execute("mesh.adapt", &p);
        assert!(r.is_err() && !matches!(r, Err(EngineError::Panicked(_))), "{p}: {r:?}");
        assert!(s.model.mesh.is_none(), "a refused request leaves the session as it was");
    }
    // No ports; no geometry.
    let mut bare = Session::new();
    bare.execute("geometry.solid", &json!({"shape": "box", "size": [22.86, 10.16, 30]})).unwrap();
    assert!(bare.execute("mesh.adapt", &json!({"size": 8, "frequency": 10})).is_err());
    assert!(Session::new().execute("mesh.adapt", &json!({"size": 8, "frequency": 10})).is_err());
}

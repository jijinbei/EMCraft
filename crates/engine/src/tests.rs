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
    assert!((s.sweep.as_ref().unwrap().results[0].frequency - 9e9).abs() < 1e-3);
}

#[test]
fn a_failed_command_leaves_the_session_as_it_was() {
    let mut s = small();
    let before = s.model.clone();
    // An unknown material fails after the command started changing things.
    let e = s.execute("geometry.waveguide", &json!({"width": 10, "height": 5, "sections": [{"length": 3, "material": "nothing"}]}));
    assert!(e.is_err());
    assert_eq!(s.model, before);
    assert!(s.sweep.is_some());
    assert!(matches!(s.execute("mesh.generate", &json!({"size": 1e-5})), Err(EngineError::Failed(_))));
    assert!(s.model.mesh.is_some() && s.sweep.is_some());
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

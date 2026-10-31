//! Adaptive meshing.

use serde_json::json;

use crate::*;

/// An inductive post through WR-90, its ports made, not meshed. A post of 3 mm radius, not the
/// template's 1.5: its curved wall sets the mesh, and the thicker post meshes three times as
/// fast.
fn post_unmeshed() -> Session {
    let mut s = Session::new();
    for (c, p) in [
        ("geometry.solid", json!({"shape": "box", "size": [22.86, 10.16, 30]})),
        ("geometry.feature", json!({"combine": "subtract", "shape": "cylinder", "radius": 3, "length": 10.16, "at": [11.43, 0, 15], "axis": "y"})),
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
    // An empty guide, not the post: the refusals read the parameters, and the one refused only
    // after meshing (a frequency that overflows in hertz) meshes it in a moment.
    let mut s = Session::new();
    for (c, p) in [
        ("geometry.solid", json!({"shape": "box", "size": [22.86, 10.16, 30]})),
        ("model.port.create", json!({"name": "1", "end": "start", "modes": 4})),
        ("model.port.create", json!({"name": "2", "end": "end", "modes": 4})),
    ] {
        s.execute(c, &p).unwrap();
    }
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

/// A current element in a 40 mm box lined with a 10 mm layer, not meshed.
fn dipole_unmeshed() -> Session {
    let mut s = Session::new();
    for (c, p) in [
        ("geometry.solid", json!({"shape": "box", "size": [40, 40, 40]})),
        ("model.open", json!({"thickness": 10})),
        ("source.dipole", json!({"at": [20.1, 20.2, 19.9], "length": 1})),
    ] {
        s.execute(c, &p).unwrap();
    }
    s
}

/// The tetrahedra of a mesh of the 40 mm box whose centroids lie in its 10 mm layer.
fn in_layer(mesh: &emcraft_model::Mesh) -> usize {
    (0..mesh.tets.len())
        .filter(|&t| {
            let c = mesh.centroid(t).unwrap();
            (0..3).any(|k| c[k] < 0.01 || c[k] > 0.03)
        })
        .count()
}

#[test]
fn adaptive_passes_refine_an_open_problem_on_its_far_field() {
    let run = || {
        let mut s = dipole_unmeshed();
        let r = s.execute("mesh.adapt", &json!({"per_wavelength": 4, "frequency": 10, "passes": 2, "order": 1, "tolerance": 1e-9})).unwrap();
        (r, s)
    };
    let (r, mut s) = run();
    assert_eq!(r["refinement"], "adaptive");
    assert_eq!(r["quantity"], "far_field");
    let passes = r["passes"].as_array().unwrap();
    assert_eq!(passes.len(), 2, "{r}");
    assert!(passes[0]["delta_far_field"].is_null() && passes[1]["delta_far_field"].as_f64().unwrap() > 0.0, "{r}");
    assert!(passes[0]["delta_s"].is_null() && passes[1]["delta_s"].is_null(), "no ports, no S");
    assert!(passes[0]["marked"].as_u64().unwrap() > 0);
    assert!(passes[1]["tetrahedra"].as_u64() > passes[0]["tetrahedra"].as_u64(), "{r}");
    assert!(passes.iter().all(|p| p["power"].as_f64().is_some_and(|w| w > 0.0)));
    assert_eq!(r["delta_far_field"], passes[1]["delta_far_field"]);
    // The layer keeps its mesh: the inside is refined, the layer only by the mesher's grading
    // next to refined elements.
    let mesh = s.model.mesh.clone().unwrap();
    let mut first = dipole_unmeshed();
    first.execute("mesh.generate", &json!({"per_wavelength": 4, "frequency": 10})).unwrap();
    let start = first.model.mesh.clone().unwrap();
    let (layer_now, layer_start) = (in_layer(&mesh), in_layer(&start));
    assert!(mesh.tets.len() - layer_now > start.tets.len() - layer_start, "the inside refined");
    assert!((layer_now as f64) < 1.5 * layer_start as f64, "{layer_start} → {layer_now}");
    // The final mesh stays and solves.
    assert!(s.execute("solve.radiation", &json!({"frequency": 10})).is_ok());
    // The same input, the same mesh, bit for bit.
    let (r2, s2) = run();
    assert_eq!(mesh.tets, s2.model.mesh.unwrap().tets);
    assert_eq!(r["delta_far_field"], r2["delta_far_field"]);
}

#[test]
fn hostile_open_adaptive_requests_are_refused() {
    let mut s = dipole_unmeshed();
    for p in [
        json!({}),
        json!({"per_wavelength": 0, "frequency": 10}),
        json!({"per_wavelength": 1e6, "frequency": 10}),
        json!({"per_wavelength": 4, "frequency": f64::MAX}),
        json!({"per_wavelength": 4, "frequency": 10, "passes": 13}),
        json!({"per_wavelength": 4, "frequency": 10, "tolerance": 0}),
        json!({"per_wavelength": 4, "frequency": 10, "fraction": -1}),
        json!({"per_wavelength": 4, "frequency": 10, "order": 7}),
        json!({"size": 1e-9, "frequency": 10}),
        json!({"size": f64::NAN, "frequency": 10}),
        json!({"per_wavelength": 4, "frequency": 10, "curved": "yes"}),
    ] {
        let r = s.execute("mesh.adapt", &p);
        assert!(r.is_err() && !matches!(r, Err(EngineError::Panicked(_))), "{p}: {r:?}");
        assert!(s.model.mesh.is_none(), "a refused request leaves the session as it was");
    }
    // An open problem with nothing driving it.
    s.execute("source.clear", &json!({})).unwrap();
    let r = s.execute("mesh.adapt", &json!({"per_wavelength": 4, "frequency": 10}));
    assert!(r.as_ref().is_err_and(|e| e.to_string().contains("source")), "{r:?}");
}

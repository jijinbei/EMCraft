//! The solvers against their references: driven sweeps, eigenmodes, the time domain and its
//! devices, symmetry planes.

use serde_json::{Value, json};

use super::scratch;
use crate::*;

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
fn ports_in_a_magnetically_lossy_filling_solve_against_the_exact_line() {
    let mut s = Session::new();
    // Hostile loss tangents are refused, and nothing is created.
    for bad in [json!(-0.1), json!(f64::MAX), json!(1e10), json!("x")] {
        let r = s.execute("model.material.create", &json!({"name": "bad", "eps_r": 2, "magnetic_loss_tangent": bad}));
        assert!(r.is_err(), "{bad}");
        let r = s.execute("model.material.create", &json!({"name": "bad", "eps_r": 2, "loss_tangent": bad}));
        assert!(r.is_err(), "{bad}");
    }
    assert!(s.model.material("bad").is_err());
    s.execute("model.material.create", &json!({"name": "f", "eps_r": 1.5, "mu_r": 1.6, "loss_tangent": 0.01, "magnetic_loss_tangent": 0.03}))
        .unwrap();
    let summary = s.summary();
    assert!(summary["materials"].as_array().unwrap().iter().any(|m| m["magnetic_loss_tangent"] == 0.03), "{summary}");
    s.execute("geometry.waveguide", &json!({"width": 22.86, "height": 10.16, "sections": [{"length": 30, "material": "f"}]})).unwrap();
    s.execute("mesh.generate", &json!({"size": 3.5})).unwrap();
    s.execute("model.port.create", &json!({"name": "1", "end": "start", "modes": 2})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end", "modes": 2})).unwrap();
    // The ports' modes attenuate.
    let m = s.execute("solve.port_modes", &json!({"frequency": 10})).unwrap();
    let first = &m["ports"][0]["modes"][0];
    assert!(first["propagating"].as_bool().unwrap() && first["attenuation_db"].as_f64().unwrap() > 0.0, "{m}");
    s.execute("solve.frequency", &json!({"frequencies": [8, 12], "order": 2})).unwrap();
    let c = s.execute("results.compare", &json!({})).unwrap();
    assert_eq!(c["reference"], "exact");
    assert!(c["worst"].as_f64().unwrap() < 1e-2, "{c}");
    // The time domain refuses it.
    assert!(s.execute("solve.time_domain", &json!({"frequencies": [10]})).is_err());
    // A closed cavity of it: the materials' Q split between the two loss tangents.
    let r = s.execute("solve.eigenmodes", &json!({"near": 5, "count": 1})).unwrap();
    let mode = &r["modes"][0];
    assert!(mode["q_magnetic"].as_f64().is_some() && mode["q_dielectric"].as_f64().is_some(), "{r}");
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
fn the_device_is_a_setting_of_the_machine() {
    let mut s = Session::new();
    let r = s.execute("solve.device", &json!({"device": "cubecl-cpu", "precision": "double"})).unwrap();
    assert!(r["factorization"].as_str().unwrap().contains("complex double"), "{r}");
    // Kept across a new project; the time domain takes it as its default.
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    assert!(s.compute.accelerator().is_some());
    assert_eq!(s.compute.precision, emcraft_gpu::Precision::Double);
    for bad in [json!({"device": "tpu"}), json!({"device": "cpu", "precision": "half"}), json!({"device": "cubecl-cpu", "min_front": 0})] {
        assert!(s.execute("solve.device", &bad).is_err(), "{bad}");
    }
    // A device chosen again keeps the earlier `min_front`.
    s.execute("solve.device", &json!({"device": "cubecl-cpu", "min_front": 300})).unwrap();
    s.execute("solve.device", &json!({"device": "cubecl-cpu", "precision": "single"})).unwrap();
    assert!(s.compute.accelerator().is_some_and(|a| !a.takes(299, 1) && a.takes(300, 1)));
    s.execute("solve.device", &json!({"device": "cpu"})).unwrap();
    assert!(s.compute.accelerator().is_none());
}

#[test]
fn the_factorization_hands_its_large_fronts_to_the_device() {
    // Every front to the GPU's kernels, here on CubeCL's CPU runtime; CUDA is checked by
    // emcraft-gpu's ignored tests.
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    let sweep = json!({"frequencies": [7.5], "order": 1});
    let cpu = s.execute("solve.frequency", &sweep).unwrap();
    let r = s.execute("solve.device", &json!({"device": "cubecl-cpu", "min_front": 1})).unwrap();
    assert!(r["factorization"].as_str().unwrap().contains("complex double"), "{r}");
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    let on = s.execute("solve.frequency", &sweep).unwrap();
    let s21 = |r: &Value| (r["points"][0]["s21"]["re"].as_f64().unwrap(), r["points"][0]["s21"]["im"].as_f64().unwrap());
    let (a, b) = (s21(&cpu), s21(&on));
    assert!((a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9, "{a:?} {b:?}");
    assert!(s.compute.fronts_taken() > 0);
}

#[test]
fn the_time_domain_runs_where_the_machine_can() {
    let mut s = Session::new();
    let r = s.execute("solve.devices", &json!({})).unwrap();
    let devices = r["devices"].as_array().unwrap();
    let names: Vec<&str> = devices.iter().map(|d| d["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["cpu", "cuda", "cubecl-cpu"]);
    assert_eq!(devices[0]["precisions"], json!(["double"]));
    assert_eq!(devices[2]["precisions"], json!(["single", "double"]));
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    // The threads run in double; unknown devices and precisions are refused.
    for bad in [json!({"device": "tpu"}), json!({"precision": "half"}), json!({"device": "cpu", "precision": "single"})] {
        let mut p = json!({"frequencies": [6, 8], "cell": 3});
        p.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        assert!(s.execute("solve.time_domain", &p).is_err(), "{p}");
    }
    let r = s.execute("solve.time_domain", &json!({"frequencies": [6, 8], "cell": 3, "device": "cpu", "precision": "double"})).unwrap();
    assert_eq!((r["device"].as_str(), r["precision"].as_str()), (Some("cpu"), Some("double")));
    // An open model on the GPU's kernels (run on the CPU here) matches the threads.
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [40, 40, 40]})).unwrap();
    s.execute("model.open", &json!({"thickness": 10})).unwrap();
    s.execute("model.port.lumped", &json!({"name": "gap", "from": [20, 20, 19], "to": [20, 20, 21], "impedance": 50})).unwrap();
    let threads = s.execute("solve.time_domain", &json!({"frequencies": [10], "cell": 2})).unwrap();
    let kernels = s.execute("solve.time_domain", &json!({"frequencies": [10], "cell": 2, "device": "cubecl-cpu", "precision": "double"})).unwrap();
    let z = |r: &Value| r["points"][0]["ports"][0]["impedance"]["re"].as_f64().unwrap();
    assert!((z(&kernels) / z(&threads) - 1.0).abs() < 1e-9, "{} {}", z(&kernels), z(&threads));
    assert_eq!(kernels["device"], "cubecl-cpu");
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
fn stubs_and_vias_mesh_and_solve_and_hostile_vias_are_refused() {
    // A stripline with a short stub off it, ended by a via through the whole height: the via's
    // cells are left out of the mesh, its faces walls; the stub's open end and the T mesh and
    // solve. Below the stub's quarter-wave frequency a shorted stub is a shunt inductor, so the
    // line reflects much more than without it.
    let mut s = Session::new();
    s.execute("model.material.create", &json!({"name": "ptfe", "eps_r": 2.2})).unwrap();
    let guide = |vias: Value| {
        json!({"width": 12, "height": 2, "sections": [{"length": 6, "material": "ptfe"}],
            "strips": [{"y": 1, "x0": 2, "width": 2}, {"y": 1, "x0": 4, "width": 4, "z0": 2, "z1": 4}], "vias": vias})
    };
    s.execute("geometry.waveguide", &guide(json!([{"x0": 8, "z0": 1, "wx": 1, "wz": 4}]))).unwrap();
    assert_eq!(s.model.waveguide.as_ref().unwrap().vias[0].y1, 0.002, "y1 is the height when left out");
    s.execute("model.symmetry", &json!({"axis": "y", "kind": "magnetic"})).unwrap();
    s.execute("mesh.generate", &json!({"size": 1, "strip_edge": 0.25})).unwrap();
    s.execute("model.port.create", &json!({"name": "1", "end": "start", "modes": 1})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end", "modes": 1})).unwrap();
    let r = s.execute("solve.frequency", &json!({"frequencies": [1], "order": 1})).unwrap();
    let (s11, s21) = (r["points"][0]["s11"]["mag"].as_f64().unwrap(), r["points"][0]["s21"]["mag"].as_f64().unwrap());
    assert!((s11 * s11 + s21 * s21 - 1.0).abs() < 1e-6, "{r}");
    assert!(s11 > 0.5, "a shorted stub 5 mm long at 1 GHz: {r}");
    // The time domain does not take vias.
    assert!(s.execute("solve.time_domain", &json!({"frequencies": [2]})).is_err());
    for vias in [
        json!([{"x0": 8, "z0": 1, "wx": 0, "wz": 4}]),
        json!([{"x0": 8, "z0": 1, "wx": -1, "wz": 4}]),
        json!([{"x0": 11.5, "z0": 1, "wx": 1, "wz": 4}]),
        json!([{"x0": 8, "z0": 5.5, "wx": 1, "wz": 1}]),
        json!([{"x0": 8, "z0": 1, "wx": 1, "wz": 1, "y0": 1.5, "y1": 1}]),
        json!([{"x0": 8, "z0": 1, "wx": 1, "wz": 1, "y1": 3}]),
        json!([{"x0": 8, "z0": 1, "wx": 1, "wz": 1, "y0": -1}]),
        json!([{"x0": f64::MAX, "z0": 1, "wx": 1, "wz": 1}]),
        json!([{"x0": 8, "z0": 1, "wx": 1}]),
        json!([{"x0": 0, "z0": 0, "wx": 12, "wz": 1}]),
        json!("vias"),
    ] {
        assert!(s.execute("geometry.waveguide", &guide(vias.clone())).is_err(), "{vias}");
    }
    // A via off the y plane has no mirror image there: the symmetry is refused at meshing.
    s.execute("geometry.waveguide", &guide(json!([{"x0": 8, "z0": 1, "wx": 1, "wz": 4, "y1": 1}]))).unwrap();
    assert!(s.execute("mesh.generate", &json!({"size": 1})).is_err());
}

#[test]
fn the_cut_offs_beside_the_band_are_the_ports_and_the_sections() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    let c = crate::workflow::cutoffs(&s);
    // WR-90 (a = 22.86 mm, b = 10.16 mm): c/2a = 6.5571 GHz in vacuum, divided by √2 in the
    // ports' εr = 2 filling; TE20 and TE01 from c/a and c/2b.
    let te10 = 299_792_458.0 / (2.0 * 22.86e-3);
    let near = |f: f64, label: &str| c.iter().any(|x| (x.frequency - f).abs() < 1e-6 * f && x.label == label);
    assert!(near(te10 / 2f64.sqrt(), "TE10 port 1, 2"), "{c:?}");
    assert!(near(2.0 * te10 / 2f64.sqrt(), "TE20 port 1, 2"), "{c:?}");
    assert!(near(299_792_458.0 / (2.0 * 10.16e-3) / 2f64.sqrt(), "TE01 port 1, 2"), "{c:?}");
    assert!(near(te10, "TE10 vacuum"), "{c:?}");
    assert!(c.windows(2).all(|w| w[0].frequency <= w[1].frequency));
    assert_eq!(c.iter().filter(|x| x.first).count(), 1);
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

/// Strips through a short stripline box 10 × 2 × 3 mm, meshed coarsely, ports at both ends
/// with `port`'s settings; the half below the strips' plane when `symmetric`.
fn coupled_strips(port: Value, strips: Value, symmetric: bool) -> Session {
    let mut s = Session::new();
    s.execute("model.material.create", &json!({"name": "ptfe", "eps_r": 2.2})).unwrap();
    s.execute("geometry.waveguide", &json!({"width": 10, "height": 2, "sections": [{"length": 3, "material": "ptfe"}], "strips": strips})).unwrap();
    if symmetric {
        s.execute("model.symmetry", &json!({"axis": "y", "kind": "magnetic"})).unwrap();
    }
    s.execute("mesh.generate", &json!({"size": 1, "strip_edge": 0.25})).unwrap();
    for (name, end) in [("1", "start"), ("2", "end")] {
        let mut p = json!({"name": name, "end": end, "modes": 2});
        if let (Some(o), Some(extra)) = (p.as_object_mut(), port.as_object()) {
            o.extend(extra.clone());
        }
        s.execute("model.port.create", &p).unwrap();
    }
    s
}

fn two_strips() -> Value {
    json!([{"y": 1, "x0": 3, "width": 1.5}, {"y": 1, "x0": 5, "width": 1.5}])
}

#[test]
fn a_port_across_two_strips_is_a_terminal_per_strip() {
    let mut s = coupled_strips(json!({}), two_strips(), true);
    let r = s.execute("solve.frequency", &json!({"frequencies": [2, 3], "order": 1})).unwrap();
    assert_eq!(r["ports"], json!(["1:1", "1:2", "2:1", "2:2"]), "{r}");
    assert_eq!(r["references"], json!([50.0, 50.0, 50.0, 50.0]));
    assert!(r["points"][0]["s44"]["mag"].is_number() && r["points"][0].get("s55").is_none());
    // Lossless and reciprocal; the through wave far larger than the isolated one.
    let check = s.execute("results.check", &json!({})).unwrap();
    assert!(check["unitarity_error"].as_f64().unwrap() < 1e-8 && check["reciprocity_error"].as_f64().unwrap() < 1e-9, "{check}");
    let mag = |r: &Value, k: &str| r["points"][1][k]["mag"].as_f64().unwrap();
    assert!(mag(&r, "s31") > 0.9 && mag(&r, "s21") > 0.01 && mag(&r, "s41") < mag(&r, "s21"), "{r}");
    // The terminals name excitations, the TDR's port and the Touchstone file's ports.
    let at = json!([[5, 0.5, 1.5]]);
    let probe = |s: &mut Session, excitation: &str| s.execute("results.probe", &json!({"frequency": 3, "at": at, "excitation": excitation}));
    let e1 = probe(&mut s, "1:1").unwrap();
    assert_ne!(e1["points"], probe(&mut s, "1:2").unwrap()["points"]);
    assert_eq!(probe(&mut s, "1").unwrap()["points"], e1["points"], "a port's name is its first terminal");
    for bad in ["1:3", "3:1", "1:", ":1"] {
        assert!(probe(&mut s, bad).is_err(), "{bad}");
    }
    // (Two frequencies are too few for a transform: refused for that, not for the name.)
    let tdr = s.execute("results.tdr", &json!({"port": "2:2"})).unwrap_err().to_string();
    assert!(tdr.contains("at least 4 frequencies"), "{tdr}");
    assert!(s.execute("results.tdr", &json!({"port": "2:3"})).unwrap_err().to_string().contains("no port or terminal"));
    let ts = scratch("coupled.s4p");
    assert_eq!(s.execute("results.touchstone", &json!({"path": ts})).unwrap()["ports"], 4);
    let text = std::fs::read_to_string(&ts).unwrap();
    assert!(text.contains("! ports: 1:1, 1:2, 2:1, 2:2") && text.contains("R 50"), "{text}");
    // The whole model agrees with the half below the strips' plane.
    let mut whole = coupled_strips(json!({}), two_strips(), false);
    let w = whole.execute("solve.frequency", &json!({"frequencies": [2, 3], "order": 1})).unwrap();
    for k in ["s11", "s21", "s31", "s41"] {
        assert!((mag(&w, k) - mag(&r, k)).abs() < 0.02, "{k}: whole {} half {}", w["points"][1][k], r["points"][1][k]);
    }
    // Read in its first mode: one entry per port, as before, and no terminals to name.
    let mut modal = coupled_strips(json!({"terminals": "modal"}), two_strips(), true);
    let m = modal.execute("solve.frequency", &json!({"frequencies": [3], "order": 1})).unwrap();
    assert_eq!(m["ports"], json!(["1", "2"]));
    assert!(probe(&mut modal, "1:1").is_err());
}

#[test]
fn hostile_terminal_ports_are_refused() {
    let mut s = Session::new();
    s.execute("model.material.create", &json!({"name": "ptfe", "eps_r": 2.2})).unwrap();
    s.execute("geometry.waveguide", &json!({"width": 10, "height": 2, "sections": [{"length": 3, "material": "ptfe"}]})).unwrap();
    for bad in [
        json!({"name": "a:b", "end": "start"}),
        json!({"name": "1", "end": "start", "terminals": "each"}),
        json!({"name": "1", "end": "start", "terminals": 2}),
        json!({"name": "1", "end": "start", "impedance": -50}),
        json!({"name": "1", "end": "start", "impedance": 0}),
        json!({"name": "1", "end": "start", "impedance": "NaN"}),
    ] {
        assert!(s.execute("model.port.create", &bad).is_err(), "{bad}");
    }
    // Nine strips: more conductors than a terminal port takes.
    let nine: Vec<Value> = (0..9).map(|k| json!({"y": 1, "x0": 0.5 + f64::from(k), "width": 0.5})).collect();
    let mut many = coupled_strips(json!({}), json!(nine), true);
    let e = many.execute("solve.frequency", &json!({"frequencies": [2], "order": 1})).unwrap_err().to_string();
    assert!(e.contains("at most 8"), "{e}");
    // Four strips under a plane between them: two strips in the half, each beside the plane.
    let four = json!([{"y": 1, "x0": 1, "width": 1}, {"y": 1, "x0": 3, "width": 1}, {"y": 1, "x0": 6, "width": 1}, {"y": 1, "x0": 8, "width": 1}]);
    let mut beside = coupled_strips(json!({}), four, false);
    beside.execute("model.symmetry", &json!({"axis": "x", "kind": "magnetic"})).unwrap();
    beside.execute("mesh.generate", &json!({"size": 1, "strip_edge": 0.25})).unwrap();
    let e = beside.execute("solve.frequency", &json!({"frequencies": [2], "order": 1})).unwrap_err().to_string();
    assert!(e.contains("symmetry plane beside a conductor"), "{e}");
    // A huge reference impedance is a number like any other: refused or solved, never a panic.
    let mut huge = coupled_strips(json!({"impedance": 1e300}), two_strips(), true);
    let r = huge.execute("solve.frequency", &json!({"frequencies": [2], "order": 1}));
    assert!(!matches!(r, Err(EngineError::Panicked(_))), "{r:?}");
}

#[test]
fn coupled_microstrips_convert_with_modes_of_distinct_beta() {
    // Over a substrate the even and odd modes travel apart (ε_eff differ): each is de-embedded
    // and converted by its own currents, and the lossless line's terminal S stays unitary and
    // symmetric.
    let mut s = Session::new();
    s.execute("model.material.create", &json!({"name": "fr4", "eps_r": 4.4})).unwrap();
    let strips = json!([{"y": 0.8, "x0": 4.3, "width": 1.5}, {"y": 0.8, "x0": 6.2, "width": 1.5}]);
    let guide = json!({"width": 12, "height": 5, "sections": [{"length": 4, "material": "vacuum"}], "layers": [{"thickness": 0.8, "material": "fr4"}], "strips": strips});
    s.execute("geometry.waveguide", &guide).unwrap();
    s.execute("mesh.generate", &json!({"size": 1.5, "strip_edge": 0.3})).unwrap();
    s.execute("model.port.create", &json!({"name": "1", "end": "start", "modes": 2, "deembed": 0.5})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end", "modes": 2})).unwrap();
    let modes = s.execute("solve.port_modes", &json!({"frequency": 5})).unwrap();
    let eps = |k: usize| modes["ports"][0]["modes"][k]["eps_eff"].as_f64().unwrap();
    assert!((eps(0) - eps(1)).abs() > 0.1, "{modes}");
    let r = s.execute("solve.frequency", &json!({"frequencies": [5], "order": 1})).unwrap();
    assert_eq!(r["ports"], json!(["1:1", "1:2", "2:1", "2:2"]));
    let check = s.execute("results.check", &json!({})).unwrap();
    assert!(check["unitarity_error"].as_f64().unwrap() < 1e-8 && check["reciprocity_error"].as_f64().unwrap() < 1e-9, "{check}");
}

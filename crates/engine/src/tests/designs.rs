//! Templates built from SolidCraft designs: the design compiled into the templates, opened by
//! `template:<file>`, the template's parameters passed to the design's.

use serde_json::json;

use crate::{EngineError, Session};

/// Template `name`'s model with `values`, built without meshing or solving.
fn built(name: &str, values: serde_json::Value) -> Session {
    let steps: Vec<crate::Step> = crate::template_steps(name, &values)
        .unwrap()
        .into_iter()
        .filter(|s| !s.command.starts_with("mesh.") && !s.command.starts_with("solve."))
        .collect();
    let mut s = Session::new();
    s.run_steps(&steps).unwrap();
    s
}

fn bounds(s: &Session) -> Vec<f64> {
    let [lo, hi] = s.model.bounds().unwrap().corners();
    (0..3).map(|d| (hi[d] - lo[d]) * 1e3).collect()
}

#[test]
fn a_template_opens_its_design_and_passes_its_parameters() {
    let mut s = built("taper", json!({"l": 30}));
    let size = bounds(&s);
    assert!((size[0] - 22.86).abs() < 1e-6 && (size[1] - 10.16).abs() < 1e-6 && (size[2] - 30.0).abs() < 1e-6, "{size:?}");
    // A parameter changed later rebuilds the model from the design at its new value.
    s.execute("project.variable", &json!({"name": "l", "value": 45})).unwrap();
    approx::assert_abs_diff_eq!(bounds(&s)[2], 45.0, epsilon = 1e-6);
    assert_eq!(s.model.ports.len(), 2);
}

#[test]
fn a_design_no_template_has_is_refused() {
    let mut s = Session::new();
    let e = s.execute("geometry.import", &json!({"path": "template:nothing.solidcraft"})).unwrap_err();
    assert!(matches!(&e, EngineError::Params(m) if m.contains("taper.solidcraft")), "{e}");
    // An expression of the project's variables needs them: an unknown one is refused.
    let e = s.execute("geometry.import", &json!({"path": "template:taper.solidcraft", "parameters": {"l": "=nothing"}})).unwrap_err();
    assert!(matches!(&e, EngineError::Params(_)), "{e}");
}

#[test]
fn the_horn_template_builds_its_walls_and_air() {
    let s = built("horn", json!(null));
    let bodies = crate::bodies::bodies(&s.model);
    assert!(
        bodies.iter().any(|b| b.name == "walls") && bodies.iter().any(|b| b.name == "inside"),
        "{:?}",
        bodies.iter().map(|b| &b.name).collect::<Vec<_>>()
    );
    assert!(s.model.domain.open.is_some());
}

/// The horn meshes at any density: its air body once went straight on for its last millimetre,
/// so the crease where it turned lay exactly in the walls' front face, and the Boolean (which
/// needs the surfaces in general position) left a vertex on an edge at 6, 7, 8 and 10 elements
/// per wavelength. It flares on past the mouth now.
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_horn_meshes_finer() {
    for n in [7, 10] {
        let mut s = built("horn", json!(null));
        s.execute("mesh.generate", &json!({"per_wavelength": n, "frequency": 10})).unwrap_or_else(|e| panic!("{n} per wavelength: {e}"));
    }
}

/// The horn as it was first drawn (`data/horn-crease.solidcraft`): its air goes straight on for
/// the last millimetre past the mouth, so the crease where it turns lies exactly in the walls'
/// front face. The Boolean once left a vertex on an edge there from 6 elements per wavelength
/// up; the crease is moved off the face to the side the union merges (`emcraft_mesh::flush::
/// creases`), and it meshes at every density.
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn a_crease_lying_in_another_bodys_face_meshes() {
    let path = format!("{}/src/tests/data/horn-crease.solidcraft", env!("CARGO_MANIFEST_DIR"));
    for n in [6, 8] {
        let mut s = Session::new();
        s.execute("geometry.import", &json!({"path": path, "materials": {"inside": "vacuum"}, "open": true, "thickness": 6, "margin": 14})).unwrap();
        s.execute("mesh.generate", &json!({"per_wavelength": n, "frequency": 10})).unwrap_or_else(|e| panic!("{n} per wavelength: {e}"));
    }
}

/// The corrugated guide's waving wall is a NURBS patch in the design: its tetrahedra's edges on
/// it are bent onto it (quadratic geometry), and its vertices sit on it, not on the
/// tessellation's chords — every curved edge's midpoint on the narrow wall's sine
/// y = b + h sin(2π(z − l)/p) to a micrometre (the chords stray up to 0.1 mm).
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_corrugated_wall_bends_the_elements_onto_its_sine() {
    let mut s = built("corrugated-guide", json!(null));
    s.execute("mesh.generate", &json!({"size": 5})).unwrap();
    let mesh = s.model.mesh.as_ref().unwrap();
    let wave = |z: f64| 10.16e-3 + 2e-3 * (2.0 * std::f64::consts::PI * (z - 10e-3) / 10e-3).sin();
    let on_wave: Vec<_> = mesh.curved.iter().filter(|(_, p)| p.z > 10.2e-3 && p.z < 49.8e-3 && p.y > 8.0e-3).collect();
    assert!(on_wave.len() > 40, "{}", on_wave.len());
    let worst = on_wave.iter().map(|(_, p)| (p.y - wave(p.z)).abs()).fold(0.0, f64::max);
    assert!(worst < 1e-6, "{worst}");
}

/// A design placed in a box of air moves with its exact surfaces: the heart's wire (one swept
/// NURBS face) bends its elements onto the wire where the box put it, not where the design drew
/// it (they were left behind, and no edge was bent).
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn a_design_in_a_box_bends_its_elements_onto_its_moved_surfaces() {
    let mut s = Session::new();
    let import = json!({"path": "template:heart-wire.solidcraft", "open": true, "thickness": 15, "margin": 30, "tolerance": 0.4});
    s.execute("geometry.import", &import).unwrap();
    s.execute("mesh.generate", &json!({"size": 20})).unwrap();
    let mesh = s.model.mesh.as_ref().unwrap();
    assert!(mesh.curved.len() > 1000, "{}", mesh.curved.len());
    // Each bent midpoint on the wire: within its radius (0.5 mm) of the swept curve's
    // tessellated surface, which lies in the box (all of it beyond the 30 mm margin).
    assert!(mesh.curved.iter().all(|(_, p)| p.x > 0.0299 && p.y > 0.0299 && p.z > 0.0299), "a midpoint left outside the box");
}

/// `fit_surfaces: false` keeps a design's faces faceted, as it does an STL's: the corrugated
/// wall's elements stay straight.
#[test]
fn a_design_imported_without_fitting_stays_faceted() {
    let mut steps: Vec<crate::Step> = crate::template_steps("corrugated-guide", &json!(null)).unwrap();
    steps.retain(|s| !s.command.starts_with("mesh.") && !s.command.starts_with("solve."));
    for step in steps.iter_mut().filter(|s| s.command == "geometry.import") {
        step.params["fit_surfaces"] = json!(false);
    }
    let mut s = Session::new();
    s.run_steps(&steps).unwrap();
    s.execute("mesh.generate", &json!({"size": 5})).unwrap();
    assert!(s.model.mesh.as_ref().unwrap().curved.is_empty());
}

/// The corrugated guide's wave is an equation curve of its parameters, the number of periods
/// `n` a plain number (no unit): changed, the guide is longer by the periods added, its wall's
/// points placed again from the formula.
#[test]
fn the_corrugated_guide_follows_its_parameters() {
    let length = |s: &Session| s.model.domain.solid.as_ref().map(|r| r.shape.bounds()).map(|b| b.max.z - b.min.z).unwrap();
    let four = built("corrugated-guide", json!(null));
    let six = built("corrugated-guide", json!({"n": 6}));
    // 2 l + n p: 60 mm, then 80 mm.
    approx::assert_abs_diff_eq!(length(&four), 0.060, epsilon = 1e-9);
    approx::assert_abs_diff_eq!(length(&six), 0.080, epsilon = 1e-9);
    // The narrow wall reaches b + h at the crests.
    let height = |s: &Session| s.model.domain.solid.as_ref().map(|r| r.shape.bounds()).map(|b| b.max.y - b.min.y).unwrap();
    approx::assert_abs_diff_eq!(height(&four), 0.01016 + 0.002, epsilon = 1e-6);
}

#[test]
fn the_termination_template_holds_its_absorber() {
    let s = built("termination", json!(null));
    assert_eq!(s.model.inclusions().len(), 1);
    assert_eq!(s.model.ports.len(), 1);
}

/// The matched load's |S11| over WR-90's band. A design target, not a reference: no closed form
/// gives a tapered lossy wedge's reflection; the bound says the template does what it is for
/// (0.057 at 8.2 GHz falling to 0.012 at 12.4 GHz when this was written).
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_termination_is_matched_over_the_band() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "termination"})).unwrap();
    let r = s.execute("solve.frequency", &json!({"start": 8.2, "stop": 12.4, "points": 5, "order": 2})).unwrap();
    let points = r["points"].as_array().unwrap();
    assert_eq!(points.len(), 5);
    let worst = points.iter().map(|p| p["s11"]["mag"].as_f64().unwrap()).fold(0.0, f64::max);
    assert!(worst < 0.07, "|S11| up to {worst}");
}

/// `geometry.import.inspect` says what a part holds without importing it: a design's bodies,
/// size and parameters, a 3MF package's bodies; bad paths refused, the model untouched.
#[test]
fn a_part_is_inspected_without_importing_it() {
    let mut s = Session::new();
    let r = s.execute("geometry.import.inspect", &json!({"path": "template:taper.solidcraft"})).unwrap();
    assert_eq!(r["format"], "solidcraft");
    assert_eq!(r["units"], "file");
    assert!(r["parameters"].as_array().is_some_and(|p| p.iter().any(|q| q["name"] == "l")), "{r}");
    assert!(!r["bodies"].as_array().unwrap().is_empty());
    assert!((r["size"][0].as_f64().unwrap() - 22.86).abs() < 1e-6, "{r}");
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/post-and-puck.3mf");
    let r = s.execute("geometry.import.inspect", &json!({"path": fixture})).unwrap();
    assert_eq!(r["format"], "3mf");
    let names: Vec<&str> = r["bodies"].as_array().unwrap().iter().filter_map(|b| b["name"].as_str()).collect();
    assert_eq!(names, ["Post", "Puck"]);
    assert_eq!(r["parameters"], json!([]));
    for bad in [json!({}), json!({"path": "/no/such/part.3mf"}), json!({"path": "part.obj"}), json!({"path": fixture, "unit": -1})] {
        assert!(s.execute("geometry.import.inspect", &bad).is_err(), "{bad}");
    }
    assert!(s.model.domain.solid.is_none());
}

/// A sample whose solves are its own (a periodic cell's Bloch waves, which no kind of
/// analysis' settings make) opens at once, unsolved: the solve recorded as its run, the pulse
/// that reads it kept for its view, no settings to confirm; Run runs the recorded solve.
#[test]
fn a_bloch_sample_opens_unsolved_and_run_runs_its_own_solve() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "pulse-compressor"})).unwrap();
    assert!(s.results.bloch.is_none(), "opening must not solve");
    assert_eq!(crate::job::own_runs(&s), ["solve.revolution.bloch"]);
    let kept = s.views.pulse.clone().unwrap();
    assert!(kept["frequency"].is_number() && kept["length"].is_number(), "{kept}");
    let status = s.execute("project.status", &json!({})).unwrap();
    assert!(!status.to_string().contains("settings are not confirmed"), "{status}");
    // Every solve declares itself one, so a sample never solves while it opens.
    for c in crate::COMMANDS.iter().filter(|c| c.id.starts_with("solve.")) {
        let quick = ["solve.port_modes", "solve.device", "solve.devices", "solve.run", "solve.rerun"];
        assert!(quick.contains(&c.id) || c.effect == crate::Effect::Solve, "{} is not declared a solve", c.id);
    }
}

/// The pulse compressor, run: its cell's Bloch wave over the band below the stop band (TE11
/// cut-off ×1.35 up to the Bragg frequency less 0.1 h), and the 20 ns pulse it sends through
/// 3 m, the chirp matched to the delay, compressed well over ×4 and to under 2 ns; the pulse
/// kept with the project for its view.
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_pulse_compressor_runs_its_bloch_wave_and_compresses_its_pulse() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "pulse-compressor"})).unwrap();
    let ran = s.execute("solve.run", &json!({})).unwrap();
    assert_eq!(ran["run"]["command"], "solve.revolution.bloch");
    let b = s.results.bloch.as_ref().unwrap();
    assert!((b.period - 0.016).abs() < 1e-9 && b.waves.len() == 1 && b.waves[0].gamma.len() == 121);
    let kept = s.views.pulse.clone().unwrap();
    let pulse = s.execute("results.pulse", &kept).unwrap();
    assert!(pulse["compression"].as_f64().unwrap() > 4.0 && pulse["output_width"].as_f64().unwrap() < 2.0, "{}", pulse["compression"]);
    assert_eq!(pulse["chirp"], "matched");
}

/// The helical compressor, run: its cell's Bloch waves in 3D, the third (TE11 of one rotation coupled
/// to TE21 near its cut-off) dispersing strongly; the kept pulse comes out short and clean.
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_helical_compressor_runs_its_3d_bloch_waves_and_compresses_its_pulse() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "helical-compressor"})).unwrap();
    s.execute("solve.run", &json!({})).unwrap();
    let b = s.results.bloch.as_ref().unwrap();
    assert!((b.period - 0.036).abs() < 1e-9 && b.waves.len() == 3 && b.waves[2].gamma.len() == 9);
    let kept = s.views.pulse.clone().unwrap();
    let pulse = s.execute("results.pulse", &kept).unwrap();
    assert!(pulse["compression"].as_f64().unwrap() > 6.0 && pulse["output_width"].as_f64().unwrap() < 1.2, "{}", pulse["compression"]);
}

/// A design of the user's, saved again (in SolidCraft) and read again: the taper's file
/// imported, its length changed in the file, `geometry.reload` brings the new length; set from
/// EMCraft (`geometry.design`) it changes where it is imported; a parameter the design lacks, a
/// design not imported, an empty value are refused, the model as it was. Analytic evidence: the
/// taper's z extent is its parameter `l` (mm).
#[test]
fn a_design_saved_again_or_set_here_is_read_again() {
    let bytes = emcraft_templates::design("template:taper.solidcraft").unwrap();
    let path = super::scratch("reload-taper.solidcraft");
    let write_l = |l: &str| {
        let mut d: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        for p in d["params"].as_array_mut().unwrap() {
            if p["name"] == "l" {
                p["expr"] = json!(l);
            }
        }
        std::fs::write(&path, serde_json::to_vec(&d).unwrap()).unwrap();
    };
    write_l("30");
    let mut s = Session::new();
    // Its air the region, as the taper template has it.
    s.execute("geometry.import", &json!({"path": path, "materials": {"Body1": "interior"}})).unwrap();
    approx::assert_abs_diff_eq!(bounds(&s)[2], 30.0, epsilon = 1e-6);
    let found = crate::geometry::designs(&s);
    assert_eq!(found.len(), 1);
    assert!(found[0].parameters.as_ref().is_ok_and(|p| p.iter().any(|q| q.name == "l")));
    let steps = s.history().len();
    write_l("42");
    s.execute("geometry.reload", &json!({})).unwrap();
    approx::assert_abs_diff_eq!(bounds(&s)[2], 42.0, epsilon = 1e-6);
    assert_eq!(s.history().len(), steps, "the history as it was");
    s.execute("geometry.design", &json!({"design": path, "name": "l", "value": "25"})).unwrap();
    approx::assert_abs_diff_eq!(bounds(&s)[2], 25.0, epsilon = 1e-6);
    assert_eq!(crate::geometry::designs(&s)[0].set.get("l"), Some(&json!(25.0)));
    for (params, what) in [
        (json!({"design": path, "name": "nothing", "value": "3"}), "name"),
        (json!({"design": "other.solidcraft", "name": "l", "value": "3"}), "design"),
        (json!({"design": path, "name": "l", "value": " "}), "value"),
    ] {
        let e = s.execute("geometry.design", &params).unwrap_err();
        assert!(matches!(&e, EngineError::Params(m) if m.contains(what)), "{e}");
    }
    approx::assert_abs_diff_eq!(bounds(&s)[2], 25.0, epsilon = 1e-6);
}

/// A port on a design's face by its name follows the face (ADR 0010): the taper's far end face,
/// found by where it is, takes a port by its name; the design's length changed (in the file,
/// read again), the port is on the new end; a face that is not there is refused naming the
/// port. Analytic evidence: the taper's far end lies at z = `l`.
#[test]
fn a_port_on_a_named_face_follows_it() {
    let bytes = emcraft_templates::design("template:taper.solidcraft").unwrap();
    let path = super::scratch("named-face-taper.solidcraft");
    let write_l = |l: &str| {
        let mut d: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        for p in d["params"].as_array_mut().unwrap() {
            if p["name"] == "l" {
                p["expr"] = json!(l);
            }
        }
        std::fs::write(&path, serde_json::to_vec(&d).unwrap()).unwrap();
    };
    write_l("30");
    let mut s = Session::new();
    s.execute("geometry.import", &json!({"path": path, "materials": {"Body1": "interior"}})).unwrap();
    assert!(!s.model.faces.is_empty(), "the design's face names kept");
    let b = s.model.bounds().unwrap();
    let centre = emcraft_model::Point3::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0, b.max.z);
    let far = emcraft_model::faces::name_at(&s.model, emcraft_model::Axis::Z, centre, 1e-9).expect("the far end's face has a name");
    s.execute("model.port.create", &json!({"name": "2", "face_name": far})).unwrap();
    let at = |s: &Session| s.model.ports[0].face.map(|f| (f.axis, f.at * 1e3));
    assert_eq!(s.model.ports[0].end, emcraft_model::End::End, "the guide lies before the far end");
    let (axis, z) = at(&s).unwrap();
    assert_eq!(axis, emcraft_model::Axis::Z);
    approx::assert_abs_diff_eq!(z, 30.0, epsilon = 1e-6);
    write_l("44");
    s.execute("geometry.reload", &json!({})).unwrap();
    approx::assert_abs_diff_eq!(at(&s).unwrap().1, 44.0, epsilon = 1e-6);
    let e = s.execute("model.port.create", &json!({"name": "3", "face_name": "no such face"})).unwrap_err();
    assert!(matches!(&e, EngineError::Params(m) if m.contains("port `3`") && m.contains("no face")), "{e}");
    let e = s.execute("model.port.create", &json!({"name": "4", "face_name": far, "face": [0, 0, 0, 1, 1]})).unwrap_err();
    assert!(matches!(&e, EngineError::Params(m) if m.contains("not both")), "{e}");
}

/// An open template's box set by its corner (`box`): the design's sphere left where it is, at
/// the middle of the 50 mm box; a box with a margin too, or one the bodies stick out of, refused.
#[test]
fn a_design_lies_in_the_box_its_corner_gives() {
    let s = built("sphere-scattering", json!(null));
    let [lo, hi] = s.model.bounds().unwrap().corners();
    assert!((0..3).all(|d| lo[d].abs() < 1e-12 && (hi[d] - 0.05).abs() < 1e-12), "{lo:?} {hi:?}");
    assert!(s.model.domain.open.is_some());
    let mut s = Session::new();
    let at = |extra: serde_json::Value| {
        let mut p = json!({"path": "template:sphere-scattering.solidcraft", "open": true, "thickness": 12});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    for bad in [json!({"box": [50, 50, 50], "margin": 20}), json!({"box": [50, 50, 28]}), json!({"box": [50, 50]})] {
        let e = s.execute("geometry.import", &at(bad.clone())).unwrap_err();
        assert!(matches!(&e, EngineError::Params(_)), "{bad}: {e}");
    }
}

//! The templates built from SolidCraft designs beside their script twins
//! (docs/adr/0010-geometry-from-solidcraft.md, H5): each pilot template opens its design
//! (`template:<name>.solidcraft`), and its twin (`crates/templates/twins/<name>.json`) writes the
//! same model with EMCraft's own shape commands. Solved alike, the two agree with each other —
//! which tests the import, its exact surfaces and the mesh built on them — and with the closed
//! form where there is one, evaluated here independently of the finite elements.

use std::f64::consts::PI;

use emcraft_model::units::C0;
use serde_json::{Value, json};

use crate::{Session, Step};

/// Each pilot template's twin: the same model, its shapes EMCraft's own.
const TWINS: &[(&str, &str)] = &[
    ("h-bend", include_str!("../../../templates/twins/h-bend.json")),
    ("cylinder-cavity", include_str!("../../../templates/twins/cylinder-cavity.json")),
    ("coaxial-line", include_str!("../../../templates/twins/coaxial-line.json")),
    ("inductive-post", include_str!("../../../templates/twins/inductive-post.json")),
    ("metal-sphere-scattering", include_str!("../../../templates/twins/metal-sphere-scattering.json")),
    ("cross-guide-coupler", include_str!("../../../templates/twins/cross-guide-coupler.json")),
];

/// `steps` run up to their solve, then `command` with `params`: the solve's reply.
fn solved(steps: Vec<Step>, command: &str, params: &Value) -> Value {
    let built: Vec<Step> = steps.into_iter().filter(|st| !st.command.starts_with("solve.") && st.command != "project.setup").collect();
    let mut s = Session::new();
    s.run_steps(&built).unwrap();
    s.execute(command, params).unwrap()
}

/// The twin of template `name`, its variables set to `values` (an object of numbers, or null).
fn twin_steps(name: &str, values: &Value) -> Vec<Step> {
    let script = TWINS.iter().find(|(n, _)| *n == name).unwrap().1;
    let mut steps: Vec<Step> = serde_json::from_str(script).unwrap();
    for (key, v) in values.as_object().into_iter().flatten() {
        steps.iter_mut().filter(|st| st.defines(key)).for_each(|st| st.params["value"] = v.clone());
    }
    steps
}

/// Template `name` (its design) and its twin, their variables set to `values`, each solved by
/// `command` with `params`.
fn both(name: &str, values: Value, command: &str, params: Value) -> (Value, Value) {
    let template = crate::template_steps(name, &values).unwrap();
    (solved(template, command, &params), solved(twin_steps(name, &values), command, &params))
}

/// The number at `pointer` in `v`.
fn at(v: &Value, pointer: &str) -> f64 {
    v.pointer(pointer).and_then(Value::as_f64).unwrap_or_else(|| panic!("{pointer}: {v}"))
}

/// The difference of two phases (degrees), wrapped to ±180.
fn phase_gap(a: f64, b: f64) -> f64 {
    (a - b + 540.0).rem_euclid(360.0) - 180.0
}

#[test]
fn every_twin_is_a_template_built_from_a_design() {
    for (name, _) in TWINS {
        let script = emcraft_templates::find(name).unwrap().script;
        assert!(script.contains("\"geometry.import\"") && script.contains("template:"), "{name}");
        for shape in ["geometry.solid", "geometry.body", "geometry.feature", "geometry.inclusion"] {
            assert!(!script.contains(shape), "{name} still makes a shape: {shape}");
        }
    }
}

/// A cylinder of radius r = 10 mm and length l = 30 mm: TE111 at (c/2π)√((p′₁₁/r)² + (π/l)²),
/// p′₁₁ = 1.841184 (twice: its two polarizations), TM010 at (c/2π) p₀₁/r, p₀₁ = 2.404826
/// (D. M. Pozar, Microwave Engineering, 4th ed., 2012, §6.4).
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_cylinder_cavitys_design_rings_as_its_twin_and_the_closed_form() {
    let (design, twin) = both("cylinder-cavity", json!(null), "solve.eigenmodes", json!({"near": 11, "count": 3, "order": 2}));
    let (r, l): (f64, f64) = (0.010, 0.030);
    let te111 = C0 / (2.0 * PI) * ((1.841184 / r).powi(2) + (PI / l).powi(2)).sqrt() / 1e9;
    let tm010 = C0 / (2.0 * PI) * 2.404826 / r / 1e9;
    for (k, exact) in [te111, te111, tm010].into_iter().enumerate() {
        let (d, t) = (at(&design, &format!("/modes/{k}/frequency")), at(&twin, &format!("/modes/{k}/frequency")));
        assert!((d - t).abs() < 2e-5 * exact, "mode {k}: design {d}, twin {t}");
        assert!((d - exact).abs() < 1e-3 * exact, "mode {k}: design {d}, closed form {exact}");
    }
}

/// A matched line of PTFE (εr = 2.1, 50 Ω-ish 3.5/1 mm) 20 mm long: S21 = e^{−jβl}, β = 2πf√εr/c.
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_coaxial_lines_design_passes_as_its_twin_and_the_closed_form() {
    let (design, twin) = both("coaxial-line", json!(null), "solve.frequency", json!({"frequencies": [2, 6], "order": 2}));
    for (k, f) in [2e9, 6e9].into_iter().enumerate() {
        let exact = -360.0 * f * 0.020 * 2.1f64.sqrt() / C0;
        let (d, t) = (at(&design, &format!("/points/{k}/s21/deg")), at(&twin, &format!("/points/{k}/s21/deg")));
        assert!(phase_gap(d, t).abs() < 0.05, "{f}: design {d}°, twin {t}°");
        assert!(phase_gap(d, exact).abs() < 0.5, "{f}: design {d}°, closed form {exact}°");
        assert!((at(&design, &format!("/points/{k}/s21/mag")) - 1.0).abs() < 2e-3, "{design}");
    }
}

/// A post of radius 1.5 mm across WR-90 at its middle, 30 mm between the ports, at 10 GHz: S from
/// the integral equation on the post (emcraft_theory::post, Nyström on 64 nodes; N. Marcuvitz,
/// Waveguide Handbook, McGraw-Hill 1951, §5.11).
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_inductive_posts_design_reflects_as_its_twin_and_the_closed_form() {
    let (design, twin) = both("inductive-post", json!(null), "solve.frequency", json!({"frequencies": [10], "order": 2}));
    let post = emcraft_theory::post::InductivePost { width: 0.02286, length: 0.030, x: 0.01143, z: 0.015, radius: 0.0015 };
    let exact = post.solve(10e9, 64).unwrap().s11.norm();
    let (d, t) = (at(&design, "/points/0/s11/mag"), at(&twin, "/points/0/s11/mag"));
    assert!((d - t).abs() < 2e-3, "design {d}, twin {t}");
    assert!((d - exact).abs() < 1e-2, "design {d}, closed form {exact}");
}

/// The mitred H-plane bend has no closed form: its design and its twin agree (with its symmetry
/// plane: the design's two ends triangulated differently are still mirror images).
#[test]
fn the_h_bends_design_turns_as_its_twin() {
    let (design, twin) = both("h-bend", json!(null), "solve.frequency", json!({"frequencies": [9, 11], "order": 2}));
    for k in 0..2 {
        for s in ["s11", "s21"] {
            let p = format!("/points/{k}/{s}/mag");
            assert!((at(&design, &p) - at(&twin, &p)).abs() < 3e-3, "{p}: design {design}, twin {twin}");
        }
    }
}

/// The cross-guide coupler's two holes: its design and its twin agree on the coupling into each
/// port (Bethe's small-hole theory is too rough to check them by).
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_cross_guide_couplers_design_couples_as_its_twin() {
    let (design, twin) = both("cross-guide-coupler", json!(null), "solve.frequency", json!({"frequencies": [10], "order": 2}));
    for s in ["s11", "s21", "s31", "s41"] {
        let p = format!("/points/0/{s}/mag");
        assert!((at(&design, &p) - at(&twin, &p)).abs() < 3e-3, "{p}: design {design}, twin {twin}");
    }
}

/// A perfectly conducting sphere of radius 5 mm at 10 GHz (ka ≈ 1.05): its scattering cross
/// section by the Mie series (emcraft_theory::radiation::mie_efficiencies; C. F. Bohren,
/// D. R. Huffman, Absorption and Scattering of Light by Small Particles, Wiley 1983, ch. 4).
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_metal_spheres_design_scatters_as_its_twin_and_the_mie_series() {
    let (design, twin) = both("metal-sphere-scattering", json!(null), "solve.radiation", json!({"frequency": 10}));
    let a = 0.005;
    let x = 2.0 * PI * 10e9 / C0 * a;
    let exact = emcraft_theory::radiation::mie_efficiencies(x, None).0 * PI * a * a * 1e6;
    // The cross-sections in mm².
    let (d, t) = (at(&design, "/scattering_cross_section"), at(&twin, "/scattering_cross_section"));
    assert!((d - t).abs() < 0.03 * exact, "design {d}, twin {t}");
    assert!((d - exact).abs() < 0.1 * exact, "design {d}, Mie {exact}");
}

/// The bend's square corner (m = 0: the design's mitre shrunk to nothing, the polygon's short
/// side gone) beside its twin and the field-superposition solution of the square corner
/// (emcraft_theory::bend::right_angle, 60 modes; A. Sieverding, F. Arndt, IEEE Trans. MTT
/// 40(2):353–362, 1992), |S11| and |S21| at 8 GHz.
#[test]
fn the_bends_square_corner_turns_as_its_twin_and_the_closed_form() {
    let (design, twin) = both("h-bend", json!({"m": 0}), "solve.frequency", json!({"frequencies": [8], "order": 2}));
    let (s11, s21) = emcraft_theory::bend::right_angle(emcraft_theory::bend::Corner::HPlane, 0.02286, 0.01016, 8e9, 60).unwrap();
    for (s, exact) in [("s11", s11.norm()), ("s21", s21.norm())] {
        let p = format!("/points/0/{s}/mag");
        let (d, t) = (at(&design, &p), at(&twin, &p));
        assert!((d - t).abs() < 2e-3, "{s}: design {d}, twin {t}");
        assert!((d - exact).abs() < 3e-3, "{s}: design {d}, closed form {exact}");
    }
}

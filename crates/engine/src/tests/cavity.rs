//! An eigenmode as an accelerating cavity (`results.cavity`): its voltage on the axis, transit
//! time, R/Q and shunt impedance.

use serde_json::json;

use crate::*;

/// A copper pillbox, half a wavelength long, on a coarse mesh: T and R/Q against the closed
/// forms (Wangler, *RF Linear Accelerators*, §2.3) — T = sin x / x with x = ωL/(2c), and
/// R/Q = 2 L T²/(ω ε0 π R² J1²(χ01)), 196.0 Ω — the voltage at a lower speed felt less, and
/// hostile parameters refused.
#[test]
fn a_pillbox_accelerates_as_its_closed_form_says() {
    let mut s = Session::new();
    assert!(s.execute("results.cavity", &json!({})).is_err(), "nothing solved");
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 13.06})).unwrap();
    s.execute("model.walls", &json!({"conductivity": 5.8e7})).unwrap();
    s.execute("mesh.generate", &json!({"size": 3})).unwrap();
    s.execute("solve.eigenmodes", &json!({"near": 11.4, "count": 2, "order": 2})).unwrap();
    // Two asked for, three kept: TE111's pair is not split.
    assert_eq!(s.results().eigen.as_ref().map(|e| e.modes.len()), Some(3));
    let r = s.execute("results.cavity", &json!({})).unwrap();
    // TE111 has no E_z on the axis: no transit time to give.
    let te = s.execute("results.cavity", &json!({"mode": 2})).unwrap();
    assert!(te["transit"].is_null() && te["r_over_q"].as_f64().unwrap() < 1e-3, "{te}");
    let (t, rq) = (r["transit"].as_f64().unwrap(), r["r_over_q"].as_f64().unwrap());
    approx::assert_abs_diff_eq!(t, 0.636_800, epsilon = 5e-4);
    approx::assert_abs_diff_eq!(rq, 195.973, epsilon = 1.0);
    assert!(r["shunt"].as_f64().unwrap() > 1.5e6, "{r}");
    // A slower particle (β = 0.5) crosses in a whole period: the voltage nearly cancels.
    let slow = s.execute("results.cavity", &json!({"beta": 0.5})).unwrap();
    assert!(slow["transit"].as_f64().unwrap() < 0.1, "{slow}");
    // The same line given: the same figures.
    let given = s.execute("results.cavity", &json!({"from": [0, 0, 0], "to": [0, 0, 13.06]})).unwrap();
    approx::assert_abs_diff_eq!(given["r_over_q"].as_f64().unwrap(), rq, epsilon = 1e-9 * rq);
    for bad in [
        json!({"mode": 0}),
        json!({"mode": 4}),
        json!({"beta": 0}),
        json!({"beta": 1.5}),
        json!({"beta": "fast"}),
        json!({"points": 0}),
        json!({"points": 1e9}),
        json!({"from": [0, 0, 0]}),
        json!({"from": [0, 0, 0], "to": [0, 0, 0]}),
        json!({"from": [0, 0, "x"], "to": [0, 0, 1]}),
    ] {
        assert!(s.execute("results.cavity", &bad).is_err(), "{bad}");
    }
}

/// A cell with beam pipes (3 mm radius, 10 mm each side): no closed form, so the 3D eigenmode
/// of the revolved profile against the body-of-revolution solver on the same profile — two
/// discretizations, 3D tetrahedra and the meridian plane's triangles — within 2.5e-3 (1.6e-3
/// at these sizes; both converge on about 11.615 GHz). R/Q falls below the pillbox's.
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn a_beam_pipe_cavity_agrees_with_its_body_of_revolution() {
    let profile = json!([[0, 0], [3, 0], [3, 10], [10, 10], [10, 23.06], [3, 23.06], [3, 33.06], [0, 33.06]]);
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "revolved", "points": profile, "sides": 128})).unwrap();
    s.execute("mesh.generate", &json!({"size": 1.5})).unwrap();
    s.execute("solve.eigenmodes", &json!({"near": 11.3, "count": 1, "order": 2})).unwrap();
    let r = s.execute("results.cavity", &json!({})).unwrap();
    let three_d = r["frequency"].as_f64().unwrap();
    let bor = s.execute("solve.revolution", &json!({"points": profile, "frequency": 11.3, "m": 0, "count": 2, "size": 0.2})).unwrap();
    let revolved = bor["modes"][0]["frequency"].as_f64().unwrap();
    assert!((three_d - revolved).abs() < 2.5e-3 * revolved, "3D {three_d} GHz, revolution {revolved} GHz");
    let rq = r["r_over_q"].as_f64().unwrap();
    assert!(rq > 120.0 && rq < 160.0, "{r}");
}

/// Electrons tracked through a pillbox's mode (its energies against qV₀T cos φ are the oracle's
/// `pillbox-tracking`): refused before anything is solved and for hostile parameters; an
/// electron bent off its line by a static field across it is lost on the cylinder's wall, where
/// it met it said; the last run's parameters kept for the window to draw. Evidence: 1 MeV
/// electrons (γ = 2.96) in 0.5 T turn on a radius γmv/(eB) ≈ 9.4 mm, so one starting 8 mm off
/// the axis of a 10 mm pillbox reaches its wall (r = 10 mm) before its far end.
#[test]
fn electrons_are_tracked_through_a_mode_and_lost_on_its_walls() {
    let mut s = Session::new();
    assert!(s.execute("results.track", &json!({})).is_err(), "nothing solved");
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 13.06})).unwrap();
    s.execute("mesh.generate", &json!({"size": 4})).unwrap();
    s.execute("solve.eigenmodes", &json!({"lowest": true, "count": 1, "order": 1})).unwrap();
    let bent = json!({"energy": 1e6, "voltage": 1e3, "phase": 0, "offsets": [8], "magnetic": [0.0, 0.5, 0.0]});
    let r = s.execute("results.track", &bent).unwrap();
    let p = &r["particles"][0];
    assert_eq!(p["end"], "lost", "{p}");
    let at = p["lost_at"].as_array().unwrap();
    let radius = (at[0].as_f64().unwrap() - 0.0).hypot(at[1].as_f64().unwrap());
    assert!((radius - 10.0).abs() < 0.6, "on the wall: r = {radius} mm");
    assert_eq!(s.views().track.as_ref(), Some(&bent), "kept for the window");
    let straight = s.execute("results.track", &json!({"energy": 1e6, "voltage": 1e3, "phases": 4})).unwrap();
    assert!(straight["particles"].as_array().unwrap().iter().all(|p| p["end"] == "through"), "{straight}");
    for bad in [
        json!({"mode": 0}),
        json!({"mode": 2}),
        json!({"voltage": 0}),
        json!({"voltage": -1}),
        json!({"voltage": 1e300}),
        json!({"energy": 0}),
        json!({"energy": "fast"}),
        json!({"energy": 1e20}),
        json!({"phases": 0}),
        json!({"phases": 361}),
        json!({"phase": "inf"}),
        json!({"offsets": []}),
        json!({"offsets": [1, "x"]}),
        json!({"phases": 360, "offsets": [0, 1, 2]}),
        json!({"magnetic": [0, 0]}),
        json!({"magnetic": [0, 0, 1e9]}),
        json!({"pusher": "leapfrog"}),
        json!({"steps": 1}),
        json!({"points": 1}),
        json!({"from": [0, 0, 0]}),
        json!({"from": [0, 0, 0], "to": [0, 0, 0]}),
        json!({"energy": 1, "steps": 100_000}),
    ] {
        assert!(s.execute("results.track", &bad).is_err(), "{bad}");
    }
}

/// The electron pillbox's template keeps its electrons for after its solve: opened without
/// solving, its mode is not there to track through, so `results.track`'s parameters wait in
/// the views (the window sends them once the mode is solved) and nothing is tracked yet.
#[test]
fn a_templates_electrons_wait_for_its_solve() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "electron-pillbox"})).unwrap();
    assert!(s.results().eigen.is_none());
    let kept = s.views().track.as_ref().unwrap_or_else(|| panic!("the electrons kept"));
    assert_eq!(kept["phases"], 12);
    assert!(s.views().tracked.is_none());
}

//! Pictures: fields, animations, port modes, scales and time-domain reflectometry.

use serde_json::json;

use super::{scratch, small};
use crate::*;

#[test]
fn pictures_are_written() {
    let mut s = small();
    for plane in ["h", "e", "cross"] {
        let path = scratch(&format!("{plane}.png"));
        let r = s.execute("view.field", &json!({"path": path, "frequency": 8, "plane": plane, "width": 500})).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']), "{r}");
    }
    // The arrows drawn are `e` and `h`; anything else is refused, not passed over.
    let path = scratch("arrows.png");
    s.execute("view.field", &json!({"path": path, "frequency": 8, "arrows": ["e", "h"], "width": 300})).unwrap();
    for bad in [json!(["e", "b"]), json!([1])] {
        let r = s.execute("view.field", &json!({"path": path, "frequency": 8, "arrows": bad, "width": 300}));
        assert!(matches!(&r, Err(EngineError::Params(m)) if m.starts_with("`arrows` holds `e` or `h`")), "{r:?}");
    }
    let path = scratch("s.png");
    s.execute("view.sparameters", &json!({"path": path, "width": 600, "height": 400})).unwrap();
    assert!(std::fs::read(&path).unwrap().len() > 1000);
}

/// `view.animation` writes one period as a looping APNG; frames and period are bounded.
#[test]
fn an_animation_is_written() {
    let mut s = small();
    let path = scratch("anim.png");
    let r = s.execute("view.animation", &json!({"path": path, "frequency": 8, "plane": "cross", "frames": 6, "period": 1.2, "width": 300})).unwrap();
    assert_eq!((r["frames"].as_u64(), r["delay_ms"].as_u64()), (Some(6), Some(200)));
    let bytes = std::fs::read(&path).unwrap();
    // The animation control chunk announces six frames, looping forever.
    let actl = bytes.windows(4).position(|w| w == b"acTL").unwrap();
    assert_eq!(&bytes[actl + 4..actl + 12], &[0, 0, 0, 6, 0, 0, 0, 0]);
    for bad in [json!({"frames": 1}), json!({"frames": 10_000}), json!({"period": -1}), json!({"period": 1e9}), json!({"width": 1_000_000})] {
        let mut p = json!({"path": path, "frequency": 8});
        p.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        assert!(matches!(s.execute("view.animation", &p), Err(EngineError::Params(_))), "{p}");
    }
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
fn a_port_mode_is_drawn_across_its_face() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    let path = scratch("te11.png");
    let r = s.execute("view.port_mode", &json!({"path": path, "port": "1", "mode": 4, "frequency": 8})).unwrap();
    assert!(r["mode"].as_str().unwrap().contains("TE11"), "{r}");
    assert!(std::fs::metadata(&path).unwrap().len() > 1000);
    // The pattern itself: TE10's E is along y, largest in the middle, zero at the side walls.
    let pat = emcraft_solver::driven::port_mode_pattern(&s.model, emcraft_fem::Order::Two, 8e9, 0, 0, 41).unwrap();
    let at = |i: usize, j: usize| pat.e[j * pat.nx + i];
    let (mid, wall) = (at(pat.nx / 2, pat.ny / 2), at(0, pat.ny / 2));
    assert!(mid[0].abs() < 1e-9 * mid[1].abs() && mid[1].abs() > 0.0 && wall[1].abs() < 1e-9 * mid[1].abs(), "{mid:?} {wall:?}");
    for bad in [json!({"path": path, "port": "9", "frequency": 8}), json!({"path": path, "port": "1", "mode": 9, "frequency": 8})] {
        assert!(s.execute("view.port_mode", &bad).is_err(), "{bad}");
    }
}

#[test]
fn a_source_spike_does_not_set_a_pictures_scale() {
    // A plane of values about 1 with a few near a source a thousand times larger: the scale is
    // the bulk's; without a spike, the largest.
    let mut v: Vec<f64> = (0..200).map(|i| 1.0 + (i % 7) as f64 * 0.01).collect();
    v.extend([1000.0, 800.0, 500.0]);
    assert!(view::robust_peak(v) < 1.1);
    let plain: Vec<f64> = (0..200).map(|i| (i as f64 * 0.1).sin().abs()).collect();
    let max = plain.iter().copied().fold(0.0, f64::max);
    assert_eq!(view::robust_peak(plain), max);
}

#[test]
fn a_guides_tdr_is_band_pass_and_low_pass_needs_a_harmonic_sweep() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    assert!(s.execute("results.tdr", &json!({})).is_err());
    s.execute("solve.frequency", &json!({"start": 6.6, "stop": 9, "points": 13, "order": 1})).unwrap();
    let r = s.execute("results.tdr", &json!({})).unwrap();
    assert_eq!(r["mode"], "bandpass");
    // The guide's reflections lie within the first nanosecond of a 5 ns period.
    let peak = r["points"].as_array().unwrap().iter().max_by(|a, b| a["value"].as_f64().unwrap().total_cmp(&b["value"].as_f64().unwrap())).unwrap();
    assert!(peak["t"].as_f64().unwrap() < 1.0, "{peak}");
    assert!(s.execute("results.tdr", &json!({"mode": "lowpass"})).is_err());
}

#[test]
#[ignore = "heavy: a minute (cargo test --release -p emcraft-engine -- --ignored)"]
fn a_strip_narrowing_steps_the_tdr_by_its_reflection() {
    // A stripline 3 mm wide then 1 mm wide: low-pass TDR from a harmonic sweep steps at the
    // round trip to the step (2 × 20 mm × √2.2 / c = 0.198 ns) to ρ = (Z2 − Z1)/(Z2 + Z1), the
    // two ends' line impedances by their cross-sections.
    let mut s = Session::new();
    s.execute("model.material.create", &json!({"name": "ptfe", "eps_r": 2.2})).unwrap();
    let strips = json!([{"y": 1, "x0": 2.5, "width": 3, "z0": 0, "z1": 20}, {"y": 1, "x0": 3.5, "width": 1, "z0": 20, "z1": 40}]);
    s.execute("geometry.waveguide", &json!({"width": 8, "height": 2, "sections": [{"length": 40, "material": "ptfe"}], "strips": strips})).unwrap();
    s.execute("mesh.generate", &json!({"size": 1, "strip_edge": 0.25})).unwrap();
    s.execute("model.port.create", &json!({"name": "1", "end": "start", "modes": 1})).unwrap();
    s.execute("model.port.create", &json!({"name": "2", "end": "end", "modes": 1})).unwrap();
    let modes = s.execute("solve.port_modes", &json!({"frequency": 2, "order": 1})).unwrap();
    let z = |k: usize| modes["ports"][k]["modes"][0]["impedance"].as_f64().unwrap();
    let rho = (z(1) - z(0)) / (z(1) + z(0));
    s.execute("solve.frequency", &json!({"start": 0.4, "stop": 10, "points": 25, "order": 1})).unwrap();
    let r = s.execute("results.tdr", &json!({})).unwrap();
    assert_eq!(r["mode"], "lowpass");
    let at = |t: f64| {
        let p = r["points"]
            .as_array()
            .unwrap()
            .iter()
            .min_by(|a, b| (a["t"].as_f64().unwrap() - t).abs().total_cmp(&(b["t"].as_f64().unwrap() - t).abs()))
            .unwrap();
        p["value"].as_f64().unwrap()
    };
    assert!(at(0.05).abs() < 0.02, "{}", at(0.05));
    // Measured 0.339 against 0.346 at 0.5 mm; coarser here.
    assert!((at(1.0) - rho).abs() < 0.03, "{} {rho}", at(1.0));
    assert!(s.execute("results.tdr", &json!({"port": "9"})).is_err());
}

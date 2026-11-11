//! Measured S-parameters, network matrices and mixed mode through their commands.

use std::fmt::Write as _;

use emcraft_solver::Complex64 as C;
use serde_json::{Value, json};

use super::scratch;
use crate::Session;

/// The highpass sample solved (two waveguide ports, no reference impedance).
fn highpass() -> Session {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass", "solve": true})).unwrap();
    s
}

/// A reply's [re, im] pair.
fn c(v: &Value) -> C {
    C::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap())
}

#[test]
fn a_sweep_written_and_read_back_matches_itself_and_stays_with_the_project() {
    let mut s = highpass();
    let ts = scratch("measured-highpass.s2p");
    s.execute("results.touchstone", &json!({"path": ts, "format": "ma"})).unwrap();
    let r = s.execute("results.import", &json!({"path": ts})).unwrap();
    assert_eq!((r["name"].as_str(), r["ports"].as_u64()), (Some("measured-highpass"), Some(2)), "{r}");
    // Its own S back: every entry within the file's 12 digits.
    let worst = r["compared"]["worst"]["max_difference"].as_f64().unwrap();
    assert!(worst < 1e-9, "{r}");
    assert_eq!(r["compared"]["entries"].as_array().unwrap().len(), 4);
    // Drawn over the chart, behind the solved points, and on the Smith chart.
    let chart = crate::view::sparameter_chart(&s).unwrap();
    assert!(chart.series.iter().any(|c| c.key == "s21.m1" && c.label.contains("measured-highpass")));
    let smith = crate::view::sparameter_chart_with(&s, crate::view::SDisplay::Smith, &[], false).unwrap();
    assert!(smith.series.iter().any(|c| c.key == "s11.m1"));
    // Kept with the project: saved and opened again.
    let path = scratch("measured.emcraft");
    s.execute("file.save", &json!({"path": path})).unwrap();
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": path})).unwrap();
    assert_eq!(t.views.measured, s.views.measured);
    // Compared again on the opened project once solved; and let go.
    assert!(s.execute("results.measured", &json!({})).unwrap()["compared"]["worst"]["max_difference"].as_f64().unwrap() < 1e-9);
    s.execute("results.import.delete", &json!({"name": "measured-highpass"})).unwrap();
    assert!(s.views.measured.is_empty());
    assert!(s.execute("results.import.delete", &json!({"name": "measured-highpass"})).is_err());
}

#[test]
fn a_measurement_alone_draws_its_chart() {
    // Nothing solved: the chart is the measurement's, over its own band.
    let mut s = Session::new();
    let ts = scratch("pad.s2p");
    std::fs::write(&ts, "# MHz S DB R 50\n1000 -30 0 -3 -90 -3 -90 -30 0\n3000 -25 0 -3 -270 -3 -270 -25 0\n").unwrap();
    s.execute("results.import", &json!({"path": ts, "name": "pad"})).unwrap();
    let chart = crate::view::sparameter_chart(&s).unwrap();
    assert_eq!((chart.x.min, chart.x.max), (1.0, 3.0));
    let s21 = chart.series.iter().find(|c| c.key == "s21.m1").unwrap();
    assert!(s21.points.iter().all(|p| (p[1] + 3.0).abs() < 1e-9), "{s21:?}");
    // No sweep to compare with, nor to read a network from but the measurement itself.
    assert!(s.execute("results.measured", &json!({})).is_err());
    let z = s.execute("results.network", &json!({"data": "pad", "kind": "s"})).unwrap();
    assert_eq!(z["frequencies"].as_array().unwrap().len(), 2);
}

#[test]
fn the_input_impedance_is_z_loaded_by_the_matched_port() {
    // Z on 50 Ω references: with port 2 matched, Z_in = Z11 − Z12 Z21 / (Z22 + 50) (Pozar
    // §4.3) — the same impedance the reflection gives, 50 (1 + S11)/(1 − S11).
    let mut s = highpass();
    let r = s.execute("results.network", &json!({"kind": "z", "impedance": 50})).unwrap();
    assert_eq!(r["normalized"], json!(false), "{r}");
    let n = r["frequencies"].as_array().unwrap().len();
    assert!(n > 3);
    for k in 0..n {
        let m = &r["matrices"][k];
        let (z11, z12, z21, z22) = (c(&m[0][0]), c(&m[0][1]), c(&m[1][0]), c(&m[1][1]));
        let zin = z11 - z12 * z21 / (z22 + C::new(50.0, 0.0));
        let given = c(&r["input_impedance"][k][0]);
        assert!((zin - given).norm() < 1e-6 * given.norm().max(1.0), "{zin} {given}");
    }
    // Waveguide ports without `impedance`: normalized.
    let unit = s.execute("results.network", &json!({"kind": "y", "frequency": 7})).unwrap();
    assert_eq!((unit["normalized"].as_bool(), unit["unit"].as_str()), (Some(true), Some("normalized")));
    assert_eq!(unit["matrices"].as_array().unwrap().len(), 1);
    // ABCD of the two-port: AD − BC = 1 (reciprocal).
    let a = s.execute("results.network", &json!({"kind": "abcd", "impedance": 50})).unwrap();
    for m in a["matrices"].as_array().unwrap() {
        let det = c(&m[0][0]) * c(&m[1][1]) - c(&m[0][1]) * c(&m[1][0]);
        assert!((det - C::new(1.0, 0.0)).norm() < 1e-6, "{det}");
    }
}

#[test]
fn mixed_mode_of_two_unequal_lines_converts_by_their_difference() {
    // Two thru lines 1→3 and 2→4, transmissions t and u (a 4-port file): pairs (1, 2), (3, 4)
    // give Sdd21 = Scc21 = (t + u)/2 and Scd21 = (t − u)/2 (Bockelman & Eisenstadt 1995).
    let (t, u) = (C::from_polar(0.9, -1.0), C::from_polar(0.8, -1.2));
    let mut text = String::from("# GHz S RI R 50\n");
    for f in [1.0, 2.0] {
        let _ = write!(text, "{f}");
        for p in 0..4 {
            for q in 0..4 {
                let v = match (p, q) {
                    (2, 0) | (0, 2) => t,
                    (3, 1) | (1, 3) => u,
                    _ => C::new(0.0, 0.0),
                };
                let _ = write!(text, " {} {}", v.re, v.im);
            }
            text.push('\n');
        }
    }
    let ts = scratch("lines.s4p");
    std::fs::write(&ts, text).unwrap();
    let mut s = Session::new();
    s.execute("results.import", &json!({"path": ts})).unwrap();
    let r = s.execute("results.mixed_mode", &json!({"data": "lines", "frequency": 1})).unwrap();
    assert_eq!(r["waves"], json!(["d1", "d2", "c1", "c2"]));
    let m = &r["matrices"][0];
    assert!((c(&m[1][0]) - (t + u) / 2.0).norm() < 1e-12);
    assert!((c(&m[3][2]) - (t + u) / 2.0).norm() < 1e-12);
    assert!((c(&m[3][0]) - (t - u) / 2.0).norm() < 1e-12);
    // Paired otherwise (1 with 3): each pair is one line's two ends, no transmission between pairs.
    let other = s.execute("results.mixed_mode", &json!({"data": "lines", "pairs": [[1, 3], [2, 4]], "frequency": 1})).unwrap();
    assert!(c(&other["matrices"][0][1][0]).norm() < 1e-12);
    // The chart in mixed mode: the first pair's column, Sdd21 among it.
    let chart = crate::view::sparameter_chart_with(&s, crate::view::SDisplay::Db, &[], true).unwrap();
    let sdd21 = chart.series.iter().find(|c| c.key == "sdd21.m1").unwrap();
    let want = emcraft_model::units::decibels(((t + u) / 2.0).norm());
    assert!(sdd21.points.iter().all(|p| (p[1] - want).abs() < 1e-9));
    // Hostile pairs and kinds are refused.
    for bad in [
        json!({"pairs": [[1, 1]]}),
        json!({"pairs": [[1, 2], [2, 3]]}),
        json!({"pairs": [[0, 1]]}),
        json!({"pairs": [[1, 9]]}),
        json!({"pairs": "1,2"}),
    ] {
        let mut p = bad.clone();
        p["data"] = json!("lines");
        assert!(s.execute("results.mixed_mode", &p).is_err(), "{bad}");
    }
    assert!(s.execute("results.network", &json!({"data": "lines", "kind": "abcd"})).is_err());
    assert!(s.execute("results.network", &json!({"data": "nothing"})).is_err());
}

#[test]
fn hostile_imports_are_refused_and_nothing_is_kept() {
    let mut s = Session::new();
    let bad = scratch("bad.s2p");
    for text in ["# GHz S RI\n1 0 0 1\n", "# GHz Q RI\n1 0 0 0 0 0 0 0 0\n", "# GHz S RI\n1 nan 0 0 0 0 0 0 0\n"] {
        std::fs::write(&bad, text).unwrap();
        assert!(s.execute("results.import", &json!({"path": bad})).is_err(), "{text}");
    }
    assert!(s.execute("results.import", &json!({"path": scratch("missing.s2p")})).is_err());
    let unknown = scratch("data.txt");
    std::fs::write(&unknown, "# GHz S RI\n1 0 0\n").unwrap();
    assert!(s.execute("results.import", &json!({"path": unknown})).is_err());
    std::fs::write(&bad, "# GHz S RI\n1 0 0 0 0 0 0 0 0\n").unwrap();
    assert!(s.execute("results.import", &json!({"path": bad, "name": ""})).is_err());
    assert!(s.views.measured.is_empty());
    // At most eight; a ninth refused, one of the same name replaced.
    for k in 0..8 {
        s.execute("results.import", &json!({"path": bad, "name": format!("m{k}")})).unwrap();
    }
    assert!(s.execute("results.import", &json!({"path": bad, "name": "m9"})).is_err());
    s.execute("results.import", &json!({"path": bad, "name": "m3"})).unwrap();
    assert_eq!(s.views.measured.len(), 8);
}

#[test]
fn material_noise_and_rough_walls_refuse_hostile_parameters() {
    let mut s = highpass();
    for (command, params) in [
        ("results.material", json!({})),
        ("results.material", json!({"length": 0})),
        ("results.material", json!({"length": 5, "before": -1})),
        ("results.material", json!({"length": 5, "cutoff": -1})),
        ("results.material", json!({"length": 5, "branch": 100000})),
        ("results.material", json!({"length": 5, "input": 2, "output": 2})),
        ("results.material", json!({"length": 5, "output": 9})),
        ("results.noise", json!({"temperature": 0})),
        ("results.noise", json!({"temperature": 1e9})),
        ("results.noise", json!({"data": "no such"})),
        ("model.walls", json!({"roughness": 0.001})),
        ("model.walls", json!({"conductivity": 5.8e7, "roughness": 1000})),
        ("model.walls", json!({"conductivity": 5.8e7, "roughness": -1})),
    ] {
        assert!(s.execute(command, &params).is_err(), "{command} {params}");
    }
    // The highpass filter's iris: every point reads, or says why; the passive filter's figure ≥ 0 dB.
    let m = s.execute("results.material", &json!({"length": 5})).unwrap();
    assert!(m["points"].as_array().unwrap().iter().all(|p| p.get("eps_r").is_some() || p["why"].is_string()), "{m}");
    let n = s.execute("results.noise", &json!({})).unwrap();
    assert!(n["points"].as_array().unwrap().iter().all(|p| p["nf_db"].as_f64().is_none_or(|f| f >= -1e-9)), "{n}");
}

#[test]
fn power_handling_and_temperature_refuse_hostile_parameters() {
    let mut s = highpass();
    for (command, params) in [
        ("results.breakdown", json!({"breakdown": 0})),
        ("results.breakdown", json!({"breakdown": 1e9})),
        ("results.breakdown", json!({"excitation": "nope"})),
        ("results.breakdown", json!({"frequency": 123.456})),
        ("results.thermal", json!({"power": 0})),
        ("results.thermal", json!({"power": 1e12})),
        ("results.thermal", json!({"wall_temperature": -300})),
        // The highpass sweep solved several frequencies: one must be named.
        ("results.thermal", json!({})),
    ] {
        assert!(s.execute(command, &params).is_err(), "{command} {params}");
    }
    // Its filling has no thermal conductivity: named in the refusal.
    let f = s.results.sweep().unwrap().results[0].frequency / 1e9;
    let e = s.execute("results.thermal", &json!({"frequency": f})).unwrap_err().to_string();
    assert!(e.contains("thermal conductivity"), "{e}");
    let b = s.execute("results.breakdown", &json!({})).unwrap();
    assert!(b["points"].as_array().unwrap().iter().all(|p| p["max_power"].as_f64().is_some_and(|w| w > 0.0)), "{b}");
}

/// A 20 dB amplifier at 1–1.2 GHz, matched, with noise data: F_min 0.6 dB from a 50 Ω source
/// (Γ_opt = 0), R_n = 0.2 × 50 Ω.
const LNA: &str = "# GHz S MA R 50\n1 0 0 10 0 0.01 0 0 0\n1.2 0 0 10 0 0.01 0 0 0\n1 0.6 0 0 0.2\n1.2 0.6 0 0 0.2\n";

/// A matched 3 dB pad, no noise data.
const PAD: &str = "# GHz S MA R 50\n1 0 0 0.7071067811865476 0 0.7071067811865476 0 0 0\n1.2 0 0 0.7071067811865476 0 0.7071067811865476 0 0 0\n";

#[test]
fn an_amplifiers_noise_data_gives_its_figure_and_a_receiver_chain_follows_friis() {
    let mut s = Session::new();
    for (name, text) in [("lna", LNA), ("pad", PAD)] {
        let path = scratch(&format!("{name}.s2p"));
        std::fs::write(&path, text).unwrap();
        let r = s.execute("results.import", &json!({"path": path})).unwrap();
        assert_eq!(r["noise_frequencies"], if name == "lna" { 2 } else { 0 });
    }
    // The amplifier alone: from its best source, F = F_min.
    let n = s.execute("results.noise", &json!({"data": "lna"})).unwrap();
    assert_eq!(n["from"], "noise data");
    for p in n["points"].as_array().unwrap() {
        approx::assert_abs_diff_eq!(p["nf_db"].as_f64().unwrap(), 0.6, epsilon = 1e-12);
        approx::assert_abs_diff_eq!(p["gain_db"].as_f64().unwrap(), 20.0, epsilon = 1e-12);
    }
    // The pad (passive, at T₀) then the amplifier: F = L·F₂ (Friis; Pozar §10.3): 3.01 + 0.6 dB.
    let blocks = json!([{"type": "data", "data": "pad"}, {"type": "data", "data": "lna"}]);
    let r = s.execute("circuit.cascade", &json!({"blocks": blocks, "name": "rx"})).unwrap();
    let expect = 10.0 * (2.0 * 10f64.powf(0.06)).log10();
    approx::assert_abs_diff_eq!(r["noise"]["nf_db_least"].as_f64().unwrap(), expect, epsilon = 1e-9);
    approx::assert_abs_diff_eq!(r["noise"]["nf_db_most"].as_f64().unwrap(), expect, epsilon = 1e-9);
    let n = s.execute("results.noise", &json!({"data": "rx"})).unwrap();
    approx::assert_abs_diff_eq!(n["points"][0]["nf_db"].as_f64().unwrap(), expect, epsilon = 1e-9);
    // The amplifier first: F = F₁ + (L − 1)/G₁, nearly F₁.
    let r = s.execute("circuit.cascade", &json!({"blocks": [{"type": "data", "data": "lna"}, {"type": "data", "data": "pad"}]})).unwrap();
    let f1 = 10f64.powf(0.06);
    approx::assert_abs_diff_eq!(r["noise"]["nf_db_least"].as_f64().unwrap(), 10.0 * (f1 + 1.0 / 100.0).log10(), epsilon = 1e-9);
    // A 5 Ω resistor in series before it: its own thermal noise, F = 1 + 5/50 more at the input.
    let r = s.execute("circuit.cascade", &json!({"blocks": [{"type": "series", "resistance": 5}, {"type": "data", "data": "lna"}]})).unwrap();
    assert!(r["noise"]["nf_db_least"].as_f64().unwrap() > 0.6 + 0.3, "{r}");
    // No noise: an amplifier flipped (its data is its port 1's), or a gain without noise data.
    let r = s.execute("circuit.cascade", &json!({"blocks": [{"type": "data", "data": "lna", "flip": true}]})).unwrap();
    assert!(r["noise"].is_null() && r["noise_why"].as_str().unwrap().contains("flipped"), "{r}");
    let gain = scratch("gain.s2p");
    std::fs::write(&gain, "# GHz S MA R 50\n1 0 0 10 0 0.01 0 0 0\n1.2 0 0 10 0 0.01 0 0 0\n").unwrap();
    s.execute("results.import", &json!({"path": gain})).unwrap();
    let r = s.execute("circuit.cascade", &json!({"blocks": [{"type": "data", "data": "gain"}]})).unwrap();
    assert!(r["noise_why"].as_str().unwrap().contains("gains"), "{r}");
    assert!(s.execute("circuit.cascade", &json!({"blocks": blocks, "temperature": 0})).is_err());
    // Saved and opened: the noise data stays with the measurement.
    let path = scratch("rx.emcraft");
    s.execute("file.save", &json!({"path": path})).unwrap();
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": path})).unwrap();
    let n = t.execute("results.noise", &json!({"data": "lna"})).unwrap();
    assert_eq!(n["from"], "noise data");
}

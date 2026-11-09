//! Coupling matrices, equivalent circuits and cascades through their commands.

use serde_json::{Value, json};

use crate::Session;

/// A reply's matrix entry.
fn m(r: &Value, i: usize, j: usize) -> f64 {
    r["matrix"][i][j].as_f64().unwrap()
}

#[test]
fn a_synthesized_filter_is_extracted_again_from_its_own_response() {
    // Fourth order, 22 dB, a pair of zeros at ω = ±1.8: synthesized, its band-pass response
    // kept, then the matrix extracted from that response (no loading) — the same matrix.
    let mut s = Session::new();
    let made = s.execute("filter.synthesize", &json!({"order": 4, "return_loss": 22, "zeros": [-1.8, 1.8], "center": 10, "bandwidth": 0.4})).unwrap();
    assert_eq!(made["names"], json!(["S", "1", "2", "3", "4", "L"]));
    assert_eq!(made["kept"], json!("prototype"));
    // Symmetric: M_S1 = M_4L, M12 = M34, a cross coupling M14 for the pair of zeros.
    assert!((m(&made, 0, 1) - m(&made, 4, 5)).abs() < 1e-9 && (m(&made, 1, 2) - m(&made, 3, 4)).abs() < 1e-9);
    assert!(m(&made, 1, 4).abs() > 0.05, "{made}");
    // Drawn over the chart.
    assert!(crate::view::sparameter_chart(&s).unwrap().series.iter().any(|c| c.key == "s21.m1"));
    let back = s
        .execute("results.coupling", &json!({"data": "prototype", "order": 4, "zeros": 2, "center": 10, "bandwidth": 0.4, "method": "cauchy_plain"}))
        .unwrap();
    for i in 0..6 {
        for j in 0..6 {
            assert!((m(&back, i, j).abs() - m(&made, i, j).abs()).abs() < 1e-4, "M[{i}][{j}]: {} vs {}", m(&back, i, j), m(&made, i, j));
        }
    }
    assert!(back["rms_s21"].as_f64().unwrap() < 1e-6, "{back}");
    // Hostile requests.
    for bad in [
        json!({"order": 0}),
        json!({"order": 17}),
        json!({"order": 3, "zeros": [0.5]}),
        json!({"order": 3, "zeros": [2, 3, 4, 5]}),
        json!({"order": 3, "return_loss": -2}),
        json!({"order": 3, "center": 10}),
        json!({"order": 3, "center": 10, "bandwidth": 30}),
    ] {
        assert!(s.execute("filter.synthesize", &bad).is_err(), "{bad}");
    }
    assert!(s.execute("results.coupling", &json!({"data": "prototype", "order": 4, "zeros": 5, "center": 10, "bandwidth": 0.4})).is_err());
    assert!(s.execute("results.coupling", &json!({"data": "prototype", "order": 4, "center": 10, "bandwidth": 0.0001})).is_err());
}

#[test]
fn an_l_section_cascaded_fits_back_to_its_own_l_and_c() {
    // 2 nH in series, then 1 pF shunt, on 50 Ω from 0.1 to 3 GHz: a T whose port-1 arm is
    // the inductor, whose port-2 arm is nothing and whose shunt branch is the capacitor.
    let mut s = Session::new();
    let r = s
        .execute(
            "circuit.cascade",
            &json!({"blocks": [{"type": "series", "inductance": 2}, {"type": "shunt", "capacitance": 1}], "start": 0.1, "stop": 3, "points": 30}),
        )
        .unwrap();
    assert_eq!(r["blocks"], json!(2));
    let e = s.execute("results.equivalent", &json!({"data": "cascade", "topology": "t"})).unwrap();
    let b = &e["branches"];
    assert!((b[0]["inductance_nh"].as_f64().unwrap() - 2.0).abs() < 1e-6, "{e}");
    assert!((b[2]["capacitance_pf"].as_f64().unwrap() - 1.0).abs() < 1e-6, "{e}");
    assert!(b[1]["inductance_nh"].is_null() && b[1]["capacitance_pf"].is_null(), "{e}");
    assert!(b.as_array().unwrap().iter().all(|x| x["relative_error"].as_f64().unwrap() < 1e-6));
    // The one-port view: port 1's input with port 2 matched is not one R, L, C — the fit says how far.
    let one = s.execute("results.equivalent", &json!({"data": "cascade", "topology": "series"})).unwrap();
    assert!(one["branches"][0]["relative_error"].as_f64().unwrap() > 1e-3);
}

#[test]
fn two_quarter_wave_lines_turn_the_phase_by_a_half_and_a_sweep_chains_with_them() {
    // Two quarter-wave lines chained by ABCD give S21 = -1 (Pozar, Microwave Engineering, 4th ed.,
    // §4.4, Tables 4.1-4.2).
    let mut s = Session::new();
    // 75 mm in vacuum is a quarter wave at 1 GHz: two of them, S21 = −1 there.
    // A band of one frequency is refused.
    assert!(s.execute("circuit.cascade", &json!({"blocks": [{"type": "line", "length": 75}], "start": 1, "stop": 1})).is_err());
    s.execute("circuit.cascade", &json!({"blocks": [{"type": "line", "length": 74.9481145}, {"type": "line", "length": 74.9481145}], "start": 0.5, "stop": 1, "points": 2, "name": "lines"}))
        .unwrap();
    let r = s.execute("results.network", &json!({"data": "lines", "kind": "s", "frequency": 1})).unwrap();
    let s21 = &r["matrices"][0][1][0];
    assert!((s21[0].as_f64().unwrap() + 1.0).abs() < 1e-9 && s21[1].as_f64().unwrap().abs() < 1e-9, "{r}");
    // The highpass sweep between two 50 Ω-normalized lines: its band, its points.
    s.execute("file.sample", &json!({"name": "highpass", "solve": true})).unwrap();
    let n = s.results.sweep.as_ref().unwrap().results.len();
    let c = s
        .execute("circuit.cascade", &json!({"blocks": [{"type": "line", "length": 10}, {"type": "data", "data": "sweep", "flip": true}, {"type": "shunt", "resistance": 1000}]}))
        .unwrap();
    assert_eq!(c["frequencies"].as_u64(), Some(n as u64));
    // Hostile blocks.
    for bad in [
        json!({"blocks": []}),
        json!({"blocks": [{"type": "valve"}]}),
        json!({"blocks": [{"type": "series"}]}),
        json!({"blocks": [{"type": "series", "resistance": -1}]}),
        json!({"blocks": [{"type": "line"}]}),
        json!({"blocks": [{"type": "data", "data": "nothing"}]}),
        json!({"blocks": [{"type": "series", "resistance": 1}], "start": 2, "stop": 1}),
        json!({"blocks": (0..33).map(|_| json!({"type": "series", "resistance": 1})).collect::<Vec<_>>(), "start": 1, "stop": 2}),
    ] {
        assert!(s.execute("circuit.cascade", &bad).is_err(), "{bad}");
    }
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_iris_filters_sweep_extracts_to_a_symmetric_three_resonator_matrix() {
    // The iris-bandpass template: four irises, three half-wave cavities, a symmetric filter.
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "iris-bandpass", "solve": true})).unwrap();
    // The passband about 9.69 to 10.27 GHz (its ripple's −21 dB), the guide's cut-off 6.557 GHz;
    // fitted from 9.4 to 10.6 GHz; its reflection zeros near 9.76, 9.96 and 10.20 GHz (ω ≈ −0.76, −0.03, 0.81). Iris
    // couplings that change with frequency are not quite three ideal resonators': within 0.15.
    let r =
        s.execute("results.coupling", &json!({"order": 3, "center": 9.975, "bandwidth": 0.58, "cutoff": 6.557, "start": 9.4, "stop": 10.6})).unwrap();
    eprintln!("{r}");
    assert!(r["rms_s21"].as_f64().unwrap() < 0.03, "{r}");
    let zeros: Vec<f64> = r["reflection_zeros"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    for (got, want) in zeros.iter().zip([-0.76, -0.03, 0.81]) {
        assert!((got - want).abs() < 0.15, "{zeros:?}");
    }
    // Symmetric: the input and output couplings, the two inter-resonator couplings.
    assert!((m(&r, 0, 1).abs() - m(&r, 3, 4).abs()).abs() < 0.02 * m(&r, 0, 1).abs(), "{r}");
    assert!((m(&r, 1, 2).abs() - m(&r, 2, 3).abs()).abs() < 0.02 * m(&r, 1, 2).abs(), "{r}");
}

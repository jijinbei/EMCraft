//! A board's region through the finite elements: `pcb.region`, the planar mesh, ports on the
//! box's sides.

use serde_json::{Value, json};

use crate::Session;

/// A microstrip `w` wide over `h` of εr `er` (mm), `l` long along x, its ground the floor: the
/// board loaded, the region a box `width` wide and `height` tall with ports at both ends.
fn microstrip(w: f64, h: f64, er: f64, l: f64, width: f64, height: f64) -> Session {
    let mut s = Session::new();
    let board = json!({
        "name": "microstrip",
        "layers": [{"name": "F.Cu", "z": 0, "eps_below": er}, {"name": "B.Cu", "z": h, "eps_below": 1}],
        "segments": [{"id": "s", "net": "S", "layer": "F.Cu", "start": [-1, 0], "end": [l + 1.0, 0], "width": w}],
    });
    s.execute("pcb.load", &json!({"data": board})).unwrap();
    let r = s.execute("pcb.region", &json!({"lo": [0, -width / 2.0], "hi": [l, width / 2.0], "above": height - h, "below": 0})).unwrap();
    assert_eq!(r["hi"], json!([l, width / 2.0]));
    for (name, end, at) in [("1", "start", 0.0), ("2", "end", l)] {
        s.execute("model.port.create", &json!({"name": name, "end": end, "axis": "x", "face": [at, 0, 0, height, width], "modes": 1})).unwrap();
    }
    s
}

fn first_mode(r: &Value, key: &str) -> f64 {
    r["ports"][0]["modes"][0][key].as_f64().unwrap()
}

#[test]
fn a_microstrip_region_meshes_and_its_port_mode_is_the_lines() {
    // Alumina, W/h = 0.945 (the oracle's microstrip-eps-eff): Hammerstad–Jensen's ε_eff 6.5485,
    // Z0 50.66 Ω; a coarse mesh within 2 %.
    let mut s = microstrip(0.600075, 0.635, 9.8, 4.0, 14.4018, 24.0);
    let m = s.execute("mesh.generate", &json!({"size": 2.0})).unwrap();
    assert!(m["copper_faces"].as_u64().unwrap() > 0, "{m}");
    let r = s.execute("solve.port_modes", &json!({"frequency": 1})).unwrap();
    let (eps, z0) = (first_mode(&r, "eps_eff"), first_mode(&r, "impedance"));
    assert!((eps / 6.548452971078884 - 1.0).abs() < 0.02, "{eps}");
    assert!((z0 / 50.66067851980078 - 1.0).abs() < 0.03, "{z0}");
}

#[test]
fn a_region_needs_a_board_and_sane_corners() {
    let mut s = Session::new();
    assert!(s.execute("pcb.region", &json!({})).is_err());
    let mut s = microstrip(0.6, 0.635, 9.8, 4.0, 14.4, 24.0);
    for bad in [json!({"lo": [0, 0], "hi": [0, 1]}), json!({"lo": [0]}), json!({"above": -1}), json!({"net": "nope"})] {
        assert!(s.execute("pcb.region", &bad).is_err(), "{bad}");
    }
}

#[test]
fn a_port_at_a_trace_end_finds_its_reference_and_lumped_ports_give_s() {
    let mut s = Session::new();
    s.execute("pcb.sample", &json!({"split": false, "neighbour": false, "length": 6})).unwrap();
    assert!(s.execute("pcb.port", &json!({"name": "1", "segment": "signal:0"})).is_err(), "no region yet");
    s.execute("pcb.region", &json!({"net": "DATA", "margin": 1, "above": 1, "below": 0.5})).unwrap();
    // The ground pour under the trace's start is its reference, not the floor.
    let r = s.execute("pcb.port", &json!({"name": "1", "segment": "signal:0", "end": "start"})).unwrap();
    assert_eq!(r["reference"], "B.Cu");
    s.execute("pcb.port", &json!({"name": "2", "segment": "signal:1", "end": "end"})).unwrap();
    for bad in [
        json!({"name": "3", "segment": "nope"}),
        json!({"name": "3", "segment": "signal:0", "end": "middle"}),
        json!({"name": "1", "segment": "signal:0"}),
    ] {
        assert!(s.execute("pcb.port", &bad).is_err(), "{bad}");
    }
    s.execute("mesh.generate", &json!({"size": 3, "strip_edge": 0.1})).unwrap();
    let f = s.execute("solve.frequency", &json!({"frequencies": [1], "order": 1})).unwrap();
    let p = &f["points"][0];
    // Lossless and reciprocal: power kept, S21 = S12.
    let (s11, s21, s12) = (p["s11"]["mag"].as_f64().unwrap(), p["s21"]["mag"].as_f64().unwrap(), p["s12"]["mag"].as_f64().unwrap());
    assert!((s11 * s11 + s21 * s21 - 1.0).abs() < 1e-6, "{s11} {s21}");
    assert!((s21 - s12).abs() < 1e-9 && s21 > 0.9, "{s21} {s12}");
}

/// S of the model's lumped ports at 1 GHz on a coarse first-order mesh, as (|S|, phase °).
fn lumped_s(s: &mut Session, size: f64) -> Value {
    s.execute("mesh.generate", &json!({"size": size, "strip_edge": 0.2})).unwrap();
    let f = s.execute("solve.frequency", &json!({"frequencies": [1], "order": 1})).unwrap();
    f["points"][0].clone()
}

fn mag(p: &Value, key: &str) -> f64 {
    p[key]["mag"].as_f64().unwrap()
}

#[test]
fn a_bent_route_passes_its_power_round_the_corner() {
    // An L of two 0.6 mm traces, 4 mm each, over 0.635 mm of alumina; ports at its free ends.
    let mut s = Session::new();
    let board = json!({
        "name": "bend",
        "layers": [{"name": "F.Cu", "z": 0, "eps_below": 9.8}, {"name": "B.Cu", "z": 0.635, "eps_below": 1}],
        "segments": [
            {"id": "a", "net": "S", "layer": "F.Cu", "start": [0, 0], "end": [4, 0], "width": 0.6},
            {"id": "b", "net": "S", "layer": "F.Cu", "start": [4, 0], "end": [4, 4], "width": 0.6},
        ],
    });
    s.execute("pcb.load", &json!({"data": board})).unwrap();
    s.execute("pcb.region", &json!({"net": "S", "margin": 2, "above": 3, "below": 0})).unwrap();
    s.execute("pcb.port", &json!({"name": "1", "segment": "a", "end": "start"})).unwrap();
    s.execute("pcb.port", &json!({"name": "2", "segment": "b", "end": "end"})).unwrap();
    let p = lumped_s(&mut s, 1.5);
    let (s11, s21, s12) = (mag(&p, "s11"), mag(&p, "s21"), mag(&p, "s12"));
    assert!((s11 * s11 + s21 * s21 - 1.0).abs() < 1e-6 && (s21 - s12).abs() < 1e-9, "{s11} {s21} {s12}");
    assert!(s21 > 0.95, "{s21}");
}

#[test]
fn coupled_lines_couple_less_than_they_carry_and_keep_the_power() {
    // Two parallel 0.6 mm traces 0.3 mm apart, ports at all four ends: 1 → 2 through, 3 the
    // near end of the other trace, 4 its far end.
    let mut s = Session::new();
    let board = json!({
        "name": "coupled",
        "layers": [{"name": "F.Cu", "z": 0, "eps_below": 9.8}, {"name": "B.Cu", "z": 0.635, "eps_below": 1}],
        "segments": [
            {"id": "a", "net": "A", "layer": "F.Cu", "start": [0, 0], "end": [6, 0], "width": 0.6},
            {"id": "b", "net": "B", "layer": "F.Cu", "start": [0, 0.9], "end": [6, 0.9], "width": 0.6},
        ],
    });
    s.execute("pcb.load", &json!({"data": board})).unwrap();
    s.execute("pcb.region", &json!({"margin": 2, "above": 3, "below": 0})).unwrap();
    for (name, id, end) in [("1", "a", "start"), ("2", "a", "end"), ("3", "b", "start"), ("4", "b", "end")] {
        s.execute("pcb.port", &json!({"name": name, "segment": id, "end": end})).unwrap();
    }
    let p = lumped_s(&mut s, 1.5);
    let power: f64 = ["s11", "s21", "s31", "s41"].iter().map(|k| mag(&p, k).powi(2)).sum();
    assert!((power - 1.0).abs() < 1e-6, "{power}");
    assert!((mag(&p, "s31") - mag(&p, "s13")).abs() < 1e-9);
    // Through far more than either coupled end; the near end couples, if weakly.
    assert!(mag(&p, "s21") > 0.9 && mag(&p, "s31") < 0.3 && mag(&p, "s31") > 1e-3, "{p}");
}

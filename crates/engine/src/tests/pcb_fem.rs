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
        s.execute("model.port.create", &json!({"name": name, "end": end, "axis": "x", "face": [at, 0, 0, width, height], "modes": 1})).unwrap();
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

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
    approx::assert_abs_diff_eq!(mag(&p, "s31"), mag(&p, "s13"), epsilon = 1e-9);
    // Through far more than either coupled end; the near end couples, if weakly.
    assert!(mag(&p, "s21") > 0.9 && mag(&p, "s31") < 0.3 && mag(&p, "s31") > 1e-3, "{p}");
}

#[test]
fn a_via_is_a_conducting_barrel_through_its_layers() {
    let mut s = Session::new();
    let board = json!({
        "name": "via",
        "layers": [{"name": "F.Cu", "z": 0, "eps_below": 4.4}, {"name": "In1", "z": 0.5, "eps_below": 4.4}, {"name": "B.Cu", "z": 1.0, "eps_below": 1}],
        "segments": [{"id": "s", "net": "S", "layer": "F.Cu", "start": [0, 0], "end": [3, 0], "width": 0.5}],
        "vias": [{"net": "S", "at": [3, 0], "layers": ["F.Cu", "In1"], "diameter": 0.8, "drill": 0.4}],
    });
    s.execute("pcb.load", &json!({"data": board})).unwrap();
    s.execute("pcb.region", &json!({"margin": 1, "above": 1, "below": 1})).unwrap();
    s.execute("mesh.generate", &json!({"size": 1, "strip_edge": 0.25})).unwrap();
    let mesh = s.model().mesh.clone().unwrap();
    emcraft_mesh::check_tetrahedra(&mesh).unwrap();
    let volume: f64 = (0..mesh.tets.len())
        .map(|t| {
            let c = mesh.corners(t).unwrap();
            emcraft_model::geom::triple(c[1] - c[0], c[2] - c[0], c[3] - c[0]) / 6.0
        })
        .sum();
    let b = s.model().bounds().unwrap();
    let full = (b.max.x - b.min.x) * (b.max.y - b.min.y) * (b.max.z - b.min.z);
    // The barrel (a polygon inscribed in its 0.4 mm circle) through the 0.5 mm between its layers.
    let barrel = std::f64::consts::PI * 0.2e-3 * 0.2e-3 * 0.5e-3;
    assert!((full - volume) > 0.9 * barrel && (full - volume) < barrel, "{} {barrel}", full - volume);
    // Conforming: the faces held by one tetrahedron are the walls (the box's and the barrel's).
    let topo = emcraft_mesh::Topology::new(&mesh);
    let outer = topo.face_tets.iter().filter(|o| o[1] == emcraft_mesh::NO_TET).count();
    let inner = mesh
        .boundary
        .iter()
        .filter(|f| {
            topo.face_tets.iter().zip(&topo.faces).any(|(o, n)| {
                o[1] != emcraft_mesh::NO_TET && {
                    let mut a = *n;
                    let mut b2 = f.nodes;
                    a.sort_unstable();
                    b2.sort_unstable();
                    a == b2
                }
            })
        })
        .count();
    assert_eq!(outer + inner, mesh.boundary.len());
}

#[test]
fn a_line_run_into_the_open_boundary_is_matched() {
    // A microstrip over a ground pour, floating in air in an open box: the line runs into the
    // layer on the box's far side, which takes its wave; its other end a 50 Ω port.
    let board = json!({
        "name": "open",
        "layers": [{"name": "F.Cu", "z": 0, "eps_below": 9.8}, {"name": "B.Cu", "z": 0.635, "eps_below": 1}],
        "segments": [{"id": "s", "net": "S", "layer": "F.Cu", "start": [3, 0], "end": [40, 0], "width": 0.6}],
        "copper": [{"net": "GND", "layer": "B.Cu", "polygon": [[-10, -10], [50, -10], [50, 10], [-10, 10]]}],
    });
    let reflection = |open: bool| {
        let mut s = Session::new();
        s.execute("pcb.load", &json!({"data": board})).unwrap();
        s.execute("pcb.region", &json!({"lo": [0, -5], "hi": [16, 5], "above": 5, "below": 3})).unwrap();
        if open {
            s.execute("model.open", &json!({"thickness": 2})).unwrap();
        }
        s.execute("pcb.port", &json!({"name": "1", "segment": "s", "end": "start"})).unwrap();
        mag(&lumped_s(&mut s, 1.0), "s11")
    };
    let (closed, open) = (reflection(false), reflection(true));
    // Shorted on the box's wall it reflects all; into the layer, little (0.045 measured: the
    // line's and the feed's own mismatch, and the layer two cells deep).
    assert!(closed > 0.9 && open < 0.07, "{closed} {open}");
}

#[test]
fn the_four_layer_example_board_meshes_and_its_route_crosses_layers() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "pcb-board"})).unwrap();
    let board = s.model().pcb().cloned().unwrap();
    assert_eq!(board.layers.len(), 4);
    // The route's two free ends are its pads; its via joins the layers.
    let ends = crate::pcb_fem::route_ends(&board, "DATA");
    assert_eq!(ends, vec![("data:0".to_string(), emcraft_model::End::Start), ("data:3".to_string(), emcraft_model::End::End)]);
    s.execute("pcb.region", &json!({"net": "DATA", "margin": 1.5})).unwrap();
    s.execute("pcb.port", &json!({"name": "1", "segment": "data:0", "end": "start"})).unwrap();
    let r = s.execute("pcb.port", &json!({"name": "2", "segment": "data:3", "end": "end"})).unwrap();
    // The bottom trace's reference is the power plane above it.
    assert_eq!(r["reference"], "PWR");
    let m = s.execute("mesh.generate", &json!({"size": 3, "strip_edge": 0.3})).unwrap();
    assert!(m["copper_faces"].as_u64().unwrap() > 0);
    emcraft_mesh::check_tetrahedra(s.model().mesh.as_ref().unwrap()).unwrap();
}

/// Lumped ports in a board's shielded region are what an S-parameter job's box is for: no
/// warning to open it; an antenna's (radiation) still gets one.
#[test]
fn lumped_ports_in_a_shielded_board_are_no_warning_for_s_parameters() {
    let mut s = Session::new();
    s.execute("pcb.sample", &json!({"split": false, "neighbour": false, "length": 6})).unwrap();
    s.execute("pcb.region", &json!({"net": "DATA"})).unwrap();
    s.execute("pcb.port", &json!({"name": "1", "segment": "signal:0", "end": "start"})).unwrap();
    let boundaries = |s: &mut Session| {
        let st = s.execute("project.status", &json!({})).unwrap();
        st["steps"].as_array().unwrap().iter().find(|x| x["step"] == "boundaries").cloned().unwrap()
    };
    s.execute("project.analysis", &json!({"kind": "frequency"})).unwrap();
    assert_eq!(boundaries(&mut s)["status"], "done");
    s.execute("project.analysis", &json!({"kind": "radiation"})).unwrap();
    assert!(boundaries(&mut s)["messages"].as_array().unwrap().iter().any(|m| m["command"] == "model.open"));
}

/// The example board's planes cover their layers but for the slot and the clearance round the
/// via: their outlines cut in to the clearance and back out the same way (a closing edge from the
/// clearance to the corner took a triangle out of each plane, even–odd).
#[test]
fn the_example_boards_planes_have_no_hole_but_their_clearances() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "pcb-board"})).unwrap();
    let board = s.model().pcb().cloned().unwrap();
    // Even–odd: a point is copper when a ray from it crosses the outline an odd number of times.
    let inside = |poly: &[[f64; 2]], [x, y]: [f64; 2]| {
        let mut odd = false;
        for (i, a) in poly.iter().enumerate() {
            let b = poly[(i + 1) % poly.len()];
            if (a[1] > y) != (b[1] > y) && x < a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                odd = !odd;
            }
        }
        odd
    };
    let mm = |x: f64, y: f64| [x * 1e-3, y * 1e-3];
    for (net, at, clear) in [("GND", mm(5.0, 3.0), mm(22.0, 9.0)), ("1V8", mm(20.0, 8.0), mm(22.0, 9.0))] {
        let covers = |p| board.copper.iter().filter(|c| c.net == net && c.layer != "TOP").any(|c| inside(&c.polygon, p));
        assert!(covers(at), "{net} has a hole at {at:?}");
        assert!(!covers(clear), "{net} covers its via's clearance");
    }
}

#[test]
fn a_via_through_another_nets_plane_without_clearance_is_refused() {
    let mut s = Session::new();
    let board = json!({
        "name": "short",
        "layers": [{"name": "F.Cu", "z": 0, "eps_below": 4.4}, {"name": "In1", "z": 0.5, "eps_below": 4.4}, {"name": "B.Cu", "z": 1.0, "eps_below": 1}],
        "segments": [{"id": "s", "net": "S", "layer": "F.Cu", "start": [0, 0], "end": [3, 0], "width": 0.5}],
        "vias": [{"net": "S", "at": [3, 0], "layers": ["F.Cu", "B.Cu"], "diameter": 0.8, "drill": 0.4}],
        "copper": [{"net": "GND", "layer": "In1", "polygon": [[-2, -2], [6, -2], [6, 2], [-2, 2]]}],
    });
    s.execute("pcb.load", &json!({"data": board})).unwrap();
    s.execute("pcb.region", &json!({"margin": 1})).unwrap();
    let e = s.execute("mesh.generate", &json!({"size": 1, "strip_edge": 0.25})).unwrap_err().to_string();
    assert!(e.contains("GND") && e.contains("In1") && e.contains("clearance"), "{e}");
}

#[test]
fn lumped_ports_sweep_fast_as_they_solve_in_full() {
    let mut s = Session::new();
    s.execute("pcb.sample", &json!({"split": false, "neighbour": false, "length": 6})).unwrap();
    s.execute("pcb.region", &json!({"net": "DATA", "margin": 1, "above": 1, "below": 0.5})).unwrap();
    s.execute("pcb.port", &json!({"name": "1", "segment": "signal:0", "end": "start"})).unwrap();
    s.execute("pcb.port", &json!({"name": "2", "segment": "signal:1", "end": "end"})).unwrap();
    s.execute("mesh.generate", &json!({"size": 3, "strip_edge": 0.1})).unwrap();
    let sweep = |strategy: &str| {
        let mut s = s.clone();
        s.execute("solve.frequency", &json!({"start": 1, "stop": 9, "points": 9, "order": 1, "sweep": strategy})).unwrap()
    };
    let (fast, full) = (sweep("fast"), sweep("full"));
    for (a, b) in fast["points"].as_array().unwrap().iter().zip(full["points"].as_array().unwrap()) {
        for k in ["s11", "s21"] {
            let d = (a[k]["re"].as_f64().unwrap() - b[k]["re"].as_f64().unwrap()).hypot(a[k]["im"].as_f64().unwrap() - b[k]["im"].as_f64().unwrap());
            assert!(d < 1e-6, "{k}: {d}");
        }
    }
    assert!(fast["solved_in_full"].as_u64().unwrap() < 9, "{}", fast["solved_in_full"]);
}

#[test]
fn second_order_solves_iteratively_as_they_factor() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    let solve = |solver: &str| {
        let mut s = s.clone();
        s.execute("solve.frequency", &json!({"frequencies": [8, 9], "order": 2, "solver": solver, "sweep": "full"})).unwrap()
    };
    let (direct, iterative) = (solve("direct"), solve("iterative"));
    for (a, b) in direct["points"].as_array().unwrap().iter().zip(iterative["points"].as_array().unwrap()) {
        for k in ["s11", "s21"] {
            let d = (a[k]["re"].as_f64().unwrap() - b[k]["re"].as_f64().unwrap()).hypot(a[k]["im"].as_f64().unwrap() - b[k]["im"].as_f64().unwrap());
            assert!(d < 1e-6, "{k}: {d}");
        }
    }
    assert!(s.clone().execute("solve.frequency", &json!({"frequencies": [8], "order": 1, "solver": "iterative"})).is_err());
}

#[test]
fn a_board_is_edited_by_commands_and_undone() {
    let mut s = Session::new();
    s.execute("pcb.sample", &json!({"split": false, "neighbour": false})).unwrap();
    let count = |s: &Session| {
        let b = s.model().pcb().unwrap();
        (b.segments.len(), b.vias.len(), b.copper.len())
    };
    let before = count(&s);
    let r = s.execute("pcb.segment", &json!({"net": "DATA", "layer": "F.Cu", "start": [45, 10], "end": [45, 15], "width": 0.3})).unwrap();
    let id = r["id"].as_str().unwrap().to_owned();
    s.execute("pcb.segment", &json!({"id": id, "width": 0.5})).unwrap();
    approx::assert_abs_diff_eq!(s.model().pcb().unwrap().segments.last().unwrap().width, 0.5e-3, epsilon = 1e-15);
    s.execute("pcb.via", &json!({"net": "DATA", "at": [45, 15], "layers": ["F.Cu", "B.Cu"], "diameter": 0.6, "drill": 0.3})).unwrap();
    s.execute("pcb.pour", &json!({"net": "PWR", "layer": "F.Cu", "polygon": [[60, 0], [70, 0], [70, 5]]})).unwrap();
    assert_eq!(count(&s), (before.0 + 1, before.1 + 1, before.2 + 1));
    // Refused: an unknown layer, a drill wider than its pads, missing fields, out of range.
    for (c, p) in [
        ("pcb.segment", json!({"net": "X", "layer": "nope", "start": [0, 0], "end": [1, 0], "width": 0.2})),
        ("pcb.segment", json!({"net": "X", "start": [0, 0]})),
        ("pcb.via", json!({"net": "X", "at": [1, 1], "layers": ["F.Cu", "B.Cu"], "diameter": 0.3, "drill": 0.5})),
        ("pcb.via.delete", json!({"index": 9})),
        ("pcb.segment.delete", json!({"id": "nope"})),
    ] {
        assert!(s.execute(c, &p).is_err(), "{c} {p}");
    }
    s.execute("pcb.pour.delete", &json!({"index": before.2 + 1})).unwrap();
    s.execute("pcb.via.delete", &json!({"index": 1})).unwrap();
    s.execute("pcb.segment.delete", &json!({"id": id})).unwrap();
    assert_eq!(count(&s), before);
    for _ in 0..3 {
        s.execute("edit.undo", &json!({})).unwrap();
    }
    assert_eq!(count(&s), (before.0 + 1, before.1 + 1, before.2 + 1));
}

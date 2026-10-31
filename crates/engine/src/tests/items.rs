//! Editing and deleting the model's items, and the values that reproduce them.

use serde_json::{Value, json};

use crate::*;

/// A box of air with a metal post, a dielectric sphere, a dipole, a plane wave and a lumped
/// port: every kind of item an open problem holds.
fn open_box() -> Session {
    let mut s = Session::new();
    for (c, p) in [
        ("model.material.create", json!({"name": "ceramic", "eps_r": 4, "debye": [{"delta_eps": 2, "frequency": 3}]})),
        ("model.material.create", json!({"name": "glass", "eps_r": 6})),
        ("geometry.solid", json!({"shape": "box", "size": [40, 40, 40]})),
        ("geometry.body", json!({"material": "metal", "shape": "cylinder", "radius": 1.5, "length": 40, "at": [5, 0, 5], "axis": "y"})),
        ("geometry.body", json!({"material": "ceramic", "shape": "sphere", "radius": 3, "at": [20, 20, 20]})),
        ("source.dipole", json!({"at": [30, 30, 30], "length": 1})),
        ("source.plane_wave", json!({"direction": [0, 0, 1], "polarization": [1, 0, 0]})),
        ("model.port.lumped", json!({"name": "L1", "from": [30, 10, 19.5], "to": [30, 10, 20.5], "phase": 30})),
    ] {
        s.execute(c, &p).unwrap_or_else(|e| panic!("{c}: {e}"));
    }
    s
}

/// The error text of a refused command, which must leave the model as it was.
fn refused(s: &mut Session, command: &str, params: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(command, &params).expect_err(command);
    assert!(!matches!(e, EngineError::Panicked(_)), "{command}: {e}");
    assert_eq!(s.model, before, "{command} {params}: refused, so nothing changed");
    e.to_string()
}

#[test]
fn materials_are_edited_renamed_and_deleted_unless_in_use() {
    let mut s = super::small();
    // In use: refused with its users.
    let e = refused(&mut s, "model.material.delete", json!({"name": "ptfe-like"}));
    assert!(e.contains("section 1, section 3"), "{e}");
    let e = refused(&mut s, "model.material.delete", json!({"name": "teflon"}));
    assert!(e.contains("names no material: `teflon` (there are vacuum, ptfe-like)"), "{e}");
    assert!(refused(&mut s, "model.material.delete", json!({"name": "vacuum"})).contains("built in"));
    // Renamed: the sections follow, the results stay.
    let n = s.history.len();
    let r = s.execute("model.material.edit", &json!({"name": "ptfe-like", "rename": "ptfe"})).unwrap();
    assert_eq!(r["results_dropped"], false);
    assert!(s.results.sweep.is_some());
    assert_eq!(s.model.waveguide.as_ref().unwrap().sections[2].material, "ptfe");
    // Its permittivity: the results go, the mesh stays.
    s.execute("model.material.edit", &json!({"name": "ptfe", "eps_r": 2.1})).unwrap();
    assert!(s.results.sweep.is_none() && s.model.mesh.is_some());
    assert_eq!(s.model.material("ptfe").unwrap().1.eps_r, 2.1);
    // Hostile edits are refused whole.
    for bad in [
        json!({"name": "ptfe", "eps_r": -1}),
        json!({"name": "ptfe", "eps_r": "=nope"}),
        json!({"name": "ptfe", "rename": "vacuum"}),
        json!({"name": "ptfe", "rename": " "}),
        json!({"name": "ptfe", "loss_tangent": 1e300}),
        json!({"name": "ptfe", "debye": [{"delta_eps": 1}]}),
        json!({"name": "vacuum", "rename": "air"}),
        json!({"name": 3}),
    ] {
        refused(&mut s, "model.material.edit", bad);
    }
    // An unused one goes; undo brings everything back in turn.
    s.execute("model.material.create", &json!({"name": "spare", "eps_r": 3})).unwrap();
    s.execute("model.material.delete", &json!({"name": "spare"})).unwrap();
    assert!(s.model.material("spare").is_err() && s.model.mesh.is_some());
    assert_eq!(s.history.len(), n + 4);
    for _ in 0..4 {
        s.execute("edit.undo", &json!({})).unwrap();
    }
    assert!(s.model.material("ptfe-like").is_ok());
}

#[test]
fn ports_are_edited_in_place_and_deleted() {
    let mut s = super::small();
    s.execute("model.port.edit", &json!({"name": "1", "rename": "in", "modes": 2, "impedance": 50})).unwrap();
    assert!(s.results.sweep.is_none(), "the sweep was of the old ports");
    let p = &s.model.ports[0];
    assert_eq!((p.name.as_str(), p.modes, p.reference), ("in", 2, Some(50.0)));
    // `null` clears the reference impedance; the other settings stay.
    s.execute("model.port.edit", &json!({"name": "in", "impedance": null})).unwrap();
    assert_eq!((s.model.ports[0].reference, s.model.ports[0].modes), (None, 2));
    assert!(refused(&mut s, "model.port.edit", json!({"name": "in", "end": "end"})).contains("already has a port"));
    assert!(refused(&mut s, "model.port.edit", json!({"name": "in", "rename": "2"})).contains("already exists"));
    assert!(refused(&mut s, "model.port.edit", json!({"name": "3"})).contains("there are in, 2"));
    for bad in [
        json!({"name": "in", "modes": 0}),
        json!({"name": "in", "modes": 1e9}),
        json!({"name": "in", "rename": "a:b"}),
        json!({"name": "in", "deembed": "x"}),
        json!({"name": "in", "face": [1, 2]}),
        json!({"name": "in", "impedance": -50}),
    ] {
        refused(&mut s, "model.port.edit", bad);
    }
    s.execute("model.port.delete", &json!({"name": "in"})).unwrap();
    assert_eq!(s.model.ports.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["2"]);
    // Now the start is free.
    s.execute("model.port.edit", &json!({"name": "2", "end": "start"})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.model.ports.len(), 2);
}

#[test]
fn lumped_ports_sources_and_bodies_are_edited_and_deleted() {
    let mut s = open_box();
    // A lumped port: its name points the other commands' way and back.
    assert!(refused(&mut s, "model.port.edit", json!({"name": "L1"})).contains("model.port.lumped.edit"));
    s.execute("model.port.lumped.edit", &json!({"name": "L1", "rename": "feed", "impedance": 75})).unwrap();
    let l = &s.model.lumped[0];
    assert_eq!((l.name.as_str(), l.impedance), ("feed", 75.0));
    assert!((l.voltage[1] - 0.5).abs() < 1e-12, "the phase stays");
    assert!(refused(&mut s, "model.port.lumped.edit", json!({"name": "feed", "to": [99, 0, 0]})).contains("inside the geometry"));
    refused(&mut s, "model.port.lumped.edit", json!({"name": "feed", "from": [30, 10, 20.5]}));
    // Sources by number.
    s.execute("source.edit", &json!({"index": 1, "at": [25, 25, 25], "current": 2})).unwrap();
    assert!(matches!(s.model.sources[0], emcraft_model::Source::Dipole { current, .. } if current == 2.0));
    assert!(refused(&mut s, "source.edit", json!({"index": 2, "at": [1, 1, 1]})).contains("plane wave"));
    assert!(refused(&mut s, "source.edit", json!({"index": 1, "polarization": [1, 0, 0]})).contains("dipole"));
    refused(&mut s, "source.edit", json!({"index": 2, "polarization": [0, 0, 1]}));
    refused(&mut s, "source.edit", json!({"index": 1, "at": [99, 0, 0]}));
    for i in [json!(0), json!(3), json!(-1), json!(1.5), json!("x")] {
        refused(&mut s, "source.delete", json!({"index": i}));
    }
    s.execute("source.delete", &json!({"index": 1})).unwrap();
    assert!(matches!(s.model.sources[..], [emcraft_model::Source::PlaneWave { .. }]));
    // Bodies by name: the post's radius, then the sphere a dielectric of glass, in place.
    s.execute("geometry.body.edit", &json!({"body": "body 1", "radius": 1})).unwrap();
    assert!(matches!(s.model.features[0].shape, emcraft_model::Shape::Cylinder { radius, length } if radius == 0.001 && length == 0.04));
    s.execute("geometry.body.edit", &json!({"body": "body 2", "material": "glass", "at": [21, 20, 20]})).unwrap();
    assert_eq!(s.model.inclusions[0].material.as_deref(), Some("glass"));
    assert!(s.model.mesh.is_none());
    // Metal now: a subtracted feature, placed again last.
    let r = s.execute("geometry.body.edit", &json!({"body": "body 2", "material": "metal"})).unwrap();
    assert_eq!(r["placed_again"], true);
    assert_eq!((s.model.features.len(), s.model.inclusions.len()), (2, 0));
    assert!(refused(&mut s, "geometry.body.edit", json!({"body": "body 9"})).contains("bodies: body 1, body 2"));
    refused(&mut s, "geometry.body.edit", json!({"body": "body 1", "at": [500, 0, 0]}));
    refused(&mut s, "geometry.body.edit", json!({"body": "body 1", "shape": "box"}));
    refused(&mut s, "geometry.body.edit", json!({"body": "body 1", "radius": -1}));
    refused(&mut s, "geometry.body.edit", json!({"body": "body 1", "material": "nothing"}));
    s.execute("geometry.body.delete", &json!({"body": "body 1"})).unwrap();
    assert_eq!(crate::bodies::bodies(&s.model).iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["body 1"]);
    // The glass is used by nothing now; the ceramic by nothing either.
    s.execute("model.material.delete", &json!({"name": "ceramic"})).unwrap();
    s.execute("model.port.lumped.delete", &json!({"name": "feed"})).unwrap();
    assert!(s.model.lumped.is_empty());
    assert!(refused(&mut s, "model.port.lumped.delete", json!({"name": "feed"})).contains("there is none"));
    // Every change is undone in turn, back to the box as built.
    let built = open_box();
    while s.history.len() > built.history.len() {
        s.execute("edit.undo", &json!({})).unwrap();
    }
    assert_eq!(s.model, built.model);
}

/// A material in use by a body lists it.
#[test]
fn a_material_used_by_a_body_names_it() {
    let mut s = open_box();
    let e = refused(&mut s, "model.material.delete", json!({"name": "ceramic"}));
    assert!(e.contains("used by body 2"), "{e}");
}

/// The values project.values gives each command, and running the command with them leaves the
/// model as it is.
#[test]
fn values_reproduce_the_model() {
    let unchanged = |s: &mut Session, command: &str, select: Value, drops_mesh: bool| -> Value {
        let mut ask = json!({"command": command});
        if let (Some(a), Some(b)) = (ask.as_object_mut(), select.as_object()) {
            a.extend(b.clone());
        }
        let v = s.execute("project.values", &ask).unwrap();
        let mut before = s.model.clone();
        s.execute(command, &v).unwrap_or_else(|e| panic!("{command} {v}: {e}"));
        let mut after = s.model.clone();
        if drops_mesh {
            before.mesh = None;
            after.mesh = None;
        }
        assert_eq!(after, before, "{command} {v}");
        v
    };
    // A waveguide with everything a guide may have set.
    let mut s = super::small();
    s.execute("model.walls", &json!({"conductivity": 5.8e7, "thickness": 1.27})).unwrap();
    s.execute("project.frequency", &json!({"start": 6.5, "stop": 9})).unwrap();
    s.execute("model.symmetry", &json!({"axis": "y", "kind": "electric"})).unwrap();
    s.execute("mesh.generate", &json!({"size": 5})).unwrap();
    s.execute("model.port.edit", &json!({"name": "1", "impedance": 50, "deembed": 1.5})).unwrap();
    assert_eq!(unchanged(&mut s, "model.walls", json!({}), false), json!({"conductivity": 5.8e7, "thickness": 1.27}));
    assert_eq!(unchanged(&mut s, "project.frequency", json!({}), false), json!({"start": 6.5, "stop": 9.0}));
    assert_eq!(unchanged(&mut s, "model.symmetry", json!({"axis": "y"}), true)["kind"], "electric");
    assert_eq!(s.execute("project.values", &json!({"command": "model.symmetry", "axis": "x"})).unwrap(), json!({}));
    unchanged(&mut s, "geometry.waveguide", json!({}), true);
    s.execute("mesh.generate", &json!({"size": 5})).unwrap();
    assert_eq!(unchanged(&mut s, "mesh.generate", json!({}), false), json!({"size": 5}));
    assert_eq!(unchanged(&mut s, "model.port.edit", json!({"name": "1"}), false)["deembed"], 1.5);
    unchanged(&mut s, "model.material.edit", json!({"name": "ptfe-like"}), false);
    unchanged(&mut s, "units.set", json!({}), false);
    // In other units: the mesh's size reads in them.
    s.execute("units.set", &json!({"length": "m"})).unwrap();
    assert_eq!(s.execute("project.values", &json!({"command": "mesh.generate"})).unwrap(), json!({"size": 0.005}));
    // The solves: their last run's, or nothing.
    let run = s.execute("project.values", &json!({"command": "solve.frequency"})).unwrap();
    assert_eq!(run, json!({"frequencies": [5.5, 8.0], "order": 2}));
    assert_eq!(s.execute("project.values", &json!({"command": "solve.eigenmodes"})).unwrap(), json!({}));
    assert_eq!(s.execute("project.values", &json!({"command": "model.open"})).unwrap(), json!({}));
    assert_eq!(s.execute("project.values", &json!({"command": "results.sparameters"})).unwrap(), json!({}));
    // A periodic cell.
    let mut c = Session::new();
    c.execute("geometry.waveguide", &json!({"width": 10, "height": 10, "sections": [{"length": 5, "material": "vacuum"}]})).unwrap();
    c.execute("model.periodic", &json!({"theta": 30, "phi": 45, "polarization": "tm"})).unwrap();
    assert_eq!(unchanged(&mut c, "model.periodic", json!({}), false)["polarization"], "tm");
    // An open box: the layer, the solid, the bodies, the sources, the lumped port.
    let mut s = open_box();
    s.execute("model.open", &json!({"thickness": 8, "reflection": 1e-4})).unwrap();
    assert_eq!(unchanged(&mut s, "model.open", json!({}), false), json!({"thickness": 8.0, "reflection": 1e-4, "shape": "box"}));
    for body in ["body 1", "body 2"] {
        unchanged(&mut s, "geometry.body.edit", json!({"body": body}), false);
    }
    let ceramic = unchanged(&mut s, "model.material.edit", json!({"name": "ceramic"}), false);
    assert!((ceramic["debye"][0]["frequency"].as_f64().unwrap() - 3.0).abs() < 1e-12, "{ceramic}");
    for index in [1, 2] {
        unchanged(&mut s, "source.edit", json!({"index": index}), false);
    }
    let lumped = unchanged(&mut s, "model.port.lumped.edit", json!({"name": "L1"}), false);
    assert!((lumped["phase"].as_f64().unwrap() - 30.0).abs() < 1e-12, "{lumped}");
    let mut solid = Session::new();
    solid.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 10, "length": 30})).unwrap();
    assert_eq!(
        unchanged(&mut solid, "geometry.solid", json!({}), true),
        json!({"shape": "cylinder", "radius": 10.0, "length": 30.0, "material": "vacuum"})
    );
    // What does not exist is refused, naming what does.
    assert!(s.execute("project.values", &json!({"command": "model.port.lumped.edit", "name": "L9"})).is_err());
    assert!(s.execute("project.values", &json!({"command": "source.edit", "index": 3})).is_err());
    assert!(s.execute("project.values", &json!({"command": "no.such"})).is_err());
    assert!(s.execute("project.values", &json!({"command": "model.material.edit"})).is_err());
}

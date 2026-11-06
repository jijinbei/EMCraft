//! Symmetry planes in solids: a template solved on the half below a plane through every port's
//! middle gives the whole model's S, within what the two meshes (not mirror images of each
//! other) differ by, on about half the unknowns. Second-order elements: on first-order ones at
//! these coarse sizes the two meshes' errors alone differ by a few hundredths.

use serde_json::{Value, json};

use super::s;

/// The template `name` at `size` (mm) solved at `frequencies` (GHz), whole and on the half below
/// the plane across `axis` closed by a `kind` wall: the two replies.
fn whole_and_half(name: &str, size: f64, frequencies: &[f64], axis: &str, kind: &str) -> (Value, Value) {
    let solve = json!({"frequencies": frequencies, "order": 2});
    let mut whole = super::template_meshed(name, size);
    whole.execute("model.symmetry", &json!({"clear": true})).unwrap();
    whole.execute("mesh.generate", &json!({"size": size})).unwrap();
    let w = whole.execute("solve.frequency", &solve).unwrap();
    let mut half = super::template_meshed(name, size);
    half.execute("model.symmetry", &json!({"axis": axis, "kind": kind})).unwrap();
    half.execute("mesh.generate", &json!({"size": size})).unwrap();
    let h = half.execute("solve.frequency", &solve).unwrap();
    (w, h)
}

/// The two replies' S agree entry by entry within `tol`, the half on fewer than `share` of the
/// whole's unknowns.
fn agree(name: &str, (w, h): (Value, Value), entries: &[&str], tol: f64, share: f64) {
    for i in 0..w["points"].as_array().unwrap().len() {
        for pq in entries {
            let (a, b) = (s(&w, i, pq), s(&h, i, pq));
            assert!((a - b).norm() < tol, "{name} {pq} at point {i}: whole {a}, half {b}");
        }
    }
    let unknowns = |r: &Value| r["unknowns"].as_f64().unwrap();
    assert!(unknowns(&h) < share * unknowns(&w), "{name}: half {} of whole {} unknowns", unknowns(&h), unknowns(&w));
}

#[test]
fn an_h_plane_tee_solves_on_its_lower_half() {
    // About its mid-height, an electric wall: TE10's E is normal to the plane.
    let r = whole_and_half("h-tee", 6.0, &[9.0, 11.0], "z", "electric");
    agree("h-tee", r, &["s11", "s21", "s31", "s33"], 0.01, 0.6);
}

#[test]
fn an_e_plane_tee_solves_on_its_half_across_the_broad_wall() {
    // About the middle of the broad wall (the prism's length), a magnetic wall: TE10's E is
    // tangential to the plane and even about it.
    let (w, h) = whole_and_half("e-tee", 4.0, &[9.0, 11.0], "z", "magnetic");
    agree("e-tee", (w, h), &["s11", "s21", "s31", "s33"], 0.01, 0.6);
}

#[test]
fn a_port_the_plane_does_not_cut_through_its_middle_is_refused() {
    let mut s = super::template_meshed("h-tee", 8.0);
    // The tee mirrors about its side arm's axis, but its through arm's ports lie on faces
    // parallel to that plane: a mirrored pair, not ports the plane cuts.
    let e = s.execute("model.symmetry", &json!({"axis": "x", "kind": "magnetic"})).unwrap_err();
    assert!(e.to_string().contains("port"), "{e}");
}

/// A face port's sides come either way round: an E-plane bend's faces give the narrow side
/// first, and its dominant mode is still across the broad one — no "below cut-off" note at the
/// X band, its first cut-off TE10 at c/2a, and the X band its ports' band.
#[test]
fn a_face_port_given_narrow_side_first_is_read_by_its_broad_side() {
    let mut s = crate::Session::new();
    s.execute("file.sample", &json!({"name": "td-e-bend"})).unwrap();
    let steps = crate::workflow::steps(&s);
    let notes: Vec<&str> = steps.iter().flat_map(|st| st.messages.iter().map(|m| m.text.as_str())).collect();
    assert!(!notes.iter().any(|t| t.contains("cut-off")), "{notes:?}");
    let first = crate::workflow::cutoffs(&s).into_iter().find(|c| c.first).unwrap();
    assert!((first.frequency - emcraft_model::units::C0 / (2.0 * 0.02286)).abs() < 1e3 && first.label.starts_with("TE10"), "{first:?}");
    let bands = crate::guides::bands(&s);
    assert_eq!(bands.first().map(|b| b.name.as_str()), Some("WR-90"), "{bands:?}");
}

/// The planes are found, not typed: a guide's ports ask for a magnetic wall across the broad
/// side and an electric one across the narrow side; an E-plane bend's for the magnetic wall
/// its template sets by hand; a cavity without ports needs `kind`; a bend is not symmetric
/// across its arms.
#[test]
fn symmetry_planes_are_found_with_the_walls_the_ports_ask_for() {
    let planes = |s: &crate::Session| s.model().domain.symmetry.iter().map(|q| (q.axis.id(), q.kind.id())).collect::<Vec<_>>();
    let mut s = crate::Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    s.execute("model.symmetry.find", &json!({})).unwrap();
    assert_eq!(planes(&s), [("x", "magnetic"), ("y", "electric")]);

    let mut s = crate::Session::new();
    s.execute("file.sample", &json!({"name": "td-e-bend"})).unwrap();
    let by_hand = planes(&s);
    s.execute("model.symmetry", &json!({"clear": true})).unwrap();
    let r = s.execute("model.symmetry.find", &json!({})).unwrap();
    assert_eq!(planes(&s), by_hand, "{r}");
    assert!((r["planes"][0]["at"].as_f64().unwrap() - 11.43).abs() < 1e-9, "{r}");

    let mut s = crate::Session::new();
    s.execute("file.sample", &json!({"name": "cavity"})).unwrap();
    s.execute("model.symmetry", &json!({"clear": true})).unwrap();
    let e = s.execute("model.symmetry.find", &json!({})).unwrap_err();
    assert!(e.to_string().contains("kind"), "{e}");
    assert!(s.model().domain.symmetry.is_empty());
    s.execute("model.symmetry.find", &json!({"kind": "magnetic"})).unwrap();
    assert_eq!(planes(&s), [("x", "magnetic"), ("y", "magnetic")]);
}

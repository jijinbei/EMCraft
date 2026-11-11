//! What every S-parameter template obeys whatever its mesh: S passive and reciprocal, unitary
//! when nothing in the model absorbs, and the same bits from the same input.
//!
//! These are identities of the discrete system (docs/verification.md, theory), so they are
//! checked here once per template rather than in each model's own test, on coarse first-order
//! meshes: each model's accuracy is the oracle's and the solver tests'.

use serde_json::{Value, json};

use crate::Session;

/// How much coarser than its own a template's mesh is taken.
const COARSER: f64 = 2.5;

/// `v` (a number or an `=expression` of the template's variables) times `k`.
fn scaled(v: &Value, k: f64) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().unwrap() * k),
        Value::String(e) if e.starts_with('=') => json!(format!("=({})*{k}", &e[1..])),
        other => other.clone(),
    }
}

/// The template's script, its steps as `(command, params)`.
fn script(name: &str) -> Vec<(String, Value)> {
    let script: Vec<Value> = serde_json::from_str(emcraft_templates::find(name).unwrap().script).unwrap();
    script.into_iter().map(|step| (step["command"].as_str().unwrap().to_owned(), step["params"].clone())).collect()
}

/// Whether the template's analysis is a sweep of S-parameters.
fn sweeps(name: &str) -> bool {
    script(name).iter().any(|(c, _)| c == "solve.frequency")
}

/// The template `name` built by its script, its mesh `COARSER` times coarser, and swept at its
/// band's ends at first order: the session and the sweep's reply.
fn swept(name: &str) -> (Session, Value) {
    let mut s = Session::new();
    let mut reply = Value::Null;
    for (command, mut params) in script(name) {
        if command == "mesh.generate" {
            // The element size only: the grading's fractions stay.
            params["size"] = scaled(&params["size"], COARSER);
        }
        if command == "solve.frequency" {
            params["points"] = json!(2);
            params["order"] = json!(1);
        }
        let r = s.execute(&command, &params).unwrap_or_else(|e| panic!("{name}: {command}: {e}"));
        if command == "solve.frequency" {
            reply = r;
        }
    }
    (s, reply)
}

/// Whether anything in the model absorbs: a lossy or dispersive material, lossy walls.
fn lossy(s: &Session) -> bool {
    let m = &s.model;
    m.domain.wall_conductivity.is_some()
        || m.materials.iter().any(|x| x.loss_tangent.get() != 0.0 || x.magnetic_loss_tangent.get() != 0.0 || !x.dispersion.is_empty())
}

/// Templates whose S, read on their first port modes, leaks to modes the ports carry but S does
/// not hold: passive and reciprocal, unitary only to the mesh.
const LEAKY: &[(&str, &str)] = &[("circular-guide", "the unstructured mesh couples TE11 into its cross-polarised twin (4.5e-3 on 7.5 mm elements)")];

/// The checks on the template `name`.
fn invariants(name: &str) {
    let (mut s, _) = swept(name);
    assert!(s.model.domain.open.is_none(), "{name}: a sweep of S with an open boundary");
    let c = s.execute("results.check", &json!({})).unwrap();
    let (gain, asym, unitary) =
        (c["largest_singular_value"].as_f64().unwrap(), c["reciprocity_error"].as_f64().unwrap(), c["unitarity_error"].as_f64().unwrap());
    // A magnetized ferrite's point is its non-reciprocity: S ≠ Sᵀ, still passive.
    let gyrotropic = s.model.materials.iter().any(|m| m.ferrite.is_some());
    assert!(c["passive"] == true && (asym < 1e-9) != gyrotropic, "{name}: {c}");
    // A resistor (a Wilkinson divider's) absorbs only some excitations: passive, not unitary.
    let resistive = s.model.elements.iter().any(|e| e.resistance > 0.0);
    if lossy(&s) {
        assert!(gain < 1.0 - 1e-3, "{name} absorbs: {c}");
    } else if resistive {
        assert!(unitary > 1e-3, "{name}'s resistor absorbs: {c}");
    } else if !LEAKY.iter().any(|(n, _)| *n == name) {
        assert!(unitary < 1e-9, "{name} is lossless: {c}");
    }
}

/// A test per template. The heavy ones are the slowest to mesh (their curved bodies and posts
/// keep their elements small however coarse the size, 3 to 45 s each); the gate keeps a
/// template of each kind of mesh and port: structured guides, prisms, a curved solid, strips
/// with terminal ports, an imported design (the one lossy template, the termination, is heavy).
macro_rules! templates {
    (gate: $($test:ident: $name:literal),* ; heavy: $($slow:ident: $heavy:literal ($why:literal)),* $(,)?) => {
        $(#[test] fn $test() { invariants($name) })*
        $(
            #[doc = $why]
            #[test]
            #[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
            fn $slow() {
                invariants($heavy)
            }
        )*
        const LISTED: &[&str] = &[$($name),*, $($heavy),*];
    };
}

templates! {
    gate:
    highpass: "highpass",
    iris_bandpass: "iris-bandpass",
    straight: "straight",
    h_bend: "h-bend",
    e_bend: "e-bend",
    h_tee: "h-tee",
    e_tee: "e-tee",
    circular_guide: "circular-guide",
    coupled_filter: "coupled-filter",
    taper: "taper",
    pcb_stub_filter: "pcb-stub-filter",
    pcb_lowpass: "pcb-lowpass",
    adsb_bandpass: "adsb-bandpass",
    adsb_receiver: "adsb-receiver",
    pcb_quarter_wave: "pcb-quarter-wave",
    pcb_branch_line: "pcb-branch-line",
    pcb_coupled_line: "pcb-coupled-line",
    pcb_bend: "pcb-bend",
    pcb_wilkinson: "pcb-wilkinson",
    pcb_ring: "pcb-ring",
    material_sample: "material-sample";
    heavy:
    corrugated_guide: "corrugated-guide" ("25 s: a sweep of 22 frequencies on 130 000 unknowns"),
    h_bend_radius: "h-bend-radius" ("4 s: its arcs"),
    e_bend_radius: "e-bend-radius" ("3 s: its arcs"),
    cross_guide_coupler: "cross-guide-coupler" ("4 s: its holes"),
    inductive_post: "inductive-post" ("3.5 s: its post"),
    magic_tee: "magic-tee" ("13 s: its posts"),
    coaxial_line: "coaxial-line" ("8 s: its inner conductor"),
    termination: "termination" ("43 s: its wedge"),
    ferrite_phase_shifter: "ferrite-phase-shifter" ("15 s: its slab, solved by LU a frequency at a time"),
}

#[test]
fn every_sweep_template_is_checked() {
    for t in emcraft_templates::TEMPLATES {
        assert_eq!(sweeps(t.name), LISTED.contains(&t.name), "{}: list it in templates!", t.name);
    }
}

/// The sweep's S entries and backward errors, point by point, without its timings.
fn entries(r: &Value) -> Vec<Value> {
    let kept =
        |k: &str| k == "frequency" || k == "backward_error" || (k.len() > 2 && k.starts_with('s') && k[1..].bytes().all(|b| b.is_ascii_digit()));
    let points = r["points"].as_array().unwrap();
    points
        .iter()
        .map(|p| Value::Object(p.as_object().unwrap().iter().filter(|(k, _)| kept(k)).map(|(k, v)| (k.clone(), v.clone())).collect()))
        .collect()
}

#[test]
fn templates_built_again_are_the_same_bit_for_bit() {
    // A structured guide, an unstructured prism, a curved solid, an imported design and lumped
    // ports on microstrips: built again from nothing, the same mesh and the same S. (`mesh.adapt`'s passes have
    // their own test, `adapt.rs`.)
    for name in ["highpass", "h-tee", "circular-guide", "taper", "pcb-coupled-line"] {
        let ((a, ra), (b, rb)) = (swept(name), swept(name));
        assert_eq!(a.model.mesh, b.model.mesh, "{name}");
        let e = entries(&ra);
        assert!(e.iter().all(|p| p.as_object().is_some_and(|o| o.len() > 2)), "{name}: {ra}");
        assert_eq!(e, entries(&rb), "{name}");
    }
}

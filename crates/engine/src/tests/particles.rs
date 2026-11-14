//! Particle-in-cell runs by command (docs/adr/0012-particles.md, stage B): the sources set,
//! checked and undone; `solve.particles` against a cold plasma's ω_p and a planar diode's
//! space-charge-limited current; a run saved and opened again.

use serde_json::{Value, json};

use crate::Session;
use crate::tests::scratch;

fn run(s: &mut Session, steps: &[(&str, Value)]) {
    for (c, p) in steps {
        s.execute(c, p).unwrap_or_else(|e| panic!("{c}: {e}"));
    }
}

/// A cold plasma of 10¹⁶ m⁻³ along a periodic box of 32 cells, its velocities a wave of one
/// wavelength, a probe a quarter of the way along.
fn plasma() -> Session {
    let mut s = Session::new();
    run(
        &mut s,
        &[
            ("geometry.solid", json!({"shape": "box", "size": [160, 5, 5]})),
            ("project.analysis", json!({"kind": "particles"})),
            ("particles.walls", json!({"x": "periodic", "y": "periodic", "z": "periodic"})),
            ("particles.plasma", json!({"density": 1e16, "per_cell": [16, 1, 1], "wave": 1e4})),
            ("particles.probe", json!({"at": [40, 0, 0]})),
            ("project.setup", json!({"time": 12, "cell": 5})),
        ],
    );
    s
}

#[test]
fn a_cold_plasma_rings_at_the_plasma_frequency() {
    let mut s = plasma();
    let steps = crate::workflow::steps(&s);
    assert!(steps[1..4].iter().all(|st| st.status == crate::workflow::Status::Done), "{steps:?}");
    let r = s.execute("solve.run", &json!({})).unwrap();
    // ω_p/2π = √(n e²/ε₀ m)/2π = 0.8979 GHz; the grid's shape and the leapfrog put it 0.23 % low.
    let f = r["probes"][0]["frequency"].as_f64().unwrap();
    assert!((f / 0.897_866 - 1.0 + 0.0023).abs() < 3e-4, "{f} GHz");
    assert_eq!((r["cells"].clone(), r["particles"].as_u64()), (json!([32, 1, 1]), Some(512)));
    let kept = s.results.particles.clone().unwrap();
    assert!(kept.history.len() > 100 && kept.snapshot.len() == 512 && kept.probes[0].e.len() == kept.steps);
}

#[test]
fn a_planar_diode_draws_the_space_charge_limited_current() {
    let mut s = Session::new();
    run(
        &mut s,
        &[
            ("geometry.solid", json!({"shape": "box", "size": [0.25, 0.25, 10]})),
            ("project.analysis", json!({"kind": "particles"})),
            ("particles.walls", json!({"x": "periodic", "y": "periodic"})),
            ("particles.voltage", json!({"axis": "z", "volts": 1e5})),
            ("particles.emitter", json!({"face": "z-"})),
        ],
    );
    let r = s.execute("solve.particles", &json!({"time": 1, "cell": 0.25})).unwrap();
    // Jory & Trivelpiece's current at 100 kV across 1 cm, 7.23e5 A/m², 5.9 % high at 40 cells
    // (emcraft_fdtd::pic's convergence test); electrons arriving: a negative current.
    let want = emcraft_theory::plasma::child_langmuir_relativistic(1e5, 1e-2, 1.602_176_634e-19, 9.109_383_701_5e-31).unwrap();
    let j = -r["walls"]["z+"]["density"].as_f64().unwrap();
    assert!((j / want - 1.059).abs() < 0.01, "{j} A/m²");
    assert!(r["walls"]["z-"]["current"].as_f64() == Some(0.0) && r["walls"].get("x+").is_none(), "{r}");
    let e = r["fastest_ev"].as_f64().unwrap();
    assert!(e < 1e5 && e > 0.9e5, "the fastest electrons near the anode: {e} eV");
}

#[test]
fn sources_are_checked_kept_and_undone() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "box", "size": [10, 10, 10]})).unwrap();
    for (c, p) in [
        ("particles.walls", json!({"x": "sideways"})),
        ("particles.emitter", json!({"face": "w+"})),
        ("particles.emitter", json!({"face": "z-", "per_node": 0})),
        ("particles.emitter", json!({"face": "z-", "current": -1})),
        ("particles.plasma", json!({"density": -1})),
        ("particles.plasma", json!({"density": 1e16, "per_cell": [0, 1, 1]})),
        ("particles.plasma", json!({"density": 1e16, "per_cell": [64, 64, 64]})),
        ("particles.plasma", json!({"density": 1e16, "per_cell": "four"})),
        ("particles.plasma", json!({"density": 1e16, "drift": [1e300, 0, 0]})),
        ("particles.voltage", json!({"axis": "w", "volts": 1})),
        ("particles.magnetic", json!({"b": [f64::MAX, 0, 0]})),
        ("particles.probe", json!({"at": [0, 0]})),
    ] {
        assert!(s.execute(c, &p).is_err(), "{c} {p}");
    }
    assert!(s.model.particles.is_empty());
    // Nothing to run yet; then an emitter on a wall that is periodic is refused at the run.
    assert!(s.execute("solve.particles", &json!({"time": 1, "cell": 1})).unwrap_err().to_string().contains("nothing puts particles"));
    s.execute("particles.walls", &json!({"z": "periodic"})).unwrap();
    s.execute("particles.emitter", &json!({"face": "z-"})).unwrap();
    let e = s.execute("solve.particles", &json!({"time": 1, "cell": 1})).unwrap_err().to_string();
    assert!(e.contains("z-"), "{e}");
    // A voltage across walls with others closed is refused, said how to fix.
    s.execute("particles.walls", &json!({"z": "conductor"})).unwrap();
    s.execute("particles.voltage", &json!({"axis": "z", "volts": 10})).unwrap();
    let e = s.execute("solve.particles", &json!({"time": 1, "cell": 1})).unwrap_err().to_string();
    assert!(e.contains("periodic"), "{e}");
    // Too many steps, a hostile cell.
    assert!(s.execute("solve.particles", &json!({"time": 1e12, "cell": 1})).is_err());
    assert!(s.execute("solve.particles", &json!({"time": 1, "cell": 1e-9})).is_err());
    assert!(s.execute("solve.particles", &json!({"time": f64::NAN, "cell": 1})).is_err());
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.model.particles.voltage.is_none());
    s.execute("particles.clear", &json!({})).unwrap();
    assert!(s.model.particles.emitters.is_empty() && s.model.particles.walls[2] == emcraft_model::particles::Wall::Conductor);
}

#[test]
fn a_run_saved_opens_with_its_sources_and_result() {
    let mut s = plasma();
    s.execute("solve.particles", &json!({"time": 1})).unwrap();
    let path = scratch("plasma.emcraft");
    s.execute("file.save", &json!({"path": path})).unwrap();
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": path})).unwrap();
    assert_eq!(t.model.particles, s.model.particles);
    assert_eq!(t.model.analysis(), emcraft_model::Analysis::Particles);
    assert_eq!(t.results.particles, s.results.particles, "read back, not solved again");
    // A change to the sources drops the run.
    t.execute("particles.magnetic", &json!({"b": [0, 0, 0.1]})).unwrap();
    assert!(t.results.particles.is_none());
    let mut bad = (*s.results.particles.clone().unwrap()).clone();
    bad.dt = f64::NAN;
    assert!(bad.check().is_err());
}

#[test]
fn a_bunch_through_a_closed_pillbox_leaves_its_loss_factor() {
    // The perfectly conducting pillbox's outside as metal cells, a 1 nC bunch (σ_z = 2 mm, 10 GeV)
    // through it on the axis: q² Σ k_n e^{−(ω_n σ_t)²} left in it (P. B. Wilson, SLAC-PUB-4547,
    // 1989, §3; emcraft_theory::wake).
    let mut s = Session::new();
    run(
        &mut s,
        &[
            ("geometry.solid", json!({"shape": "cylinder", "radius": 10, "length": 13.06})),
            ("project.analysis", json!({"kind": "particles"})),
            ("particles.bunch", json!({"face": "z-", "charge": 1, "length": 2, "energy": 1e10})),
        ],
    );
    let r = s.execute("solve.particles", &json!({"time": 0.12, "cell": 0.5})).unwrap();
    let want = emcraft_theory::wake::Pillbox::new(0.01, 0.01306).loss_factor(1.0, 2e-3 / 299_792_458.0).unwrap() * 1e-12;
    let k = r["loss_factor_v_per_pc"].as_f64().unwrap();
    assert!((k / want - 1.0).abs() < 4e-3, "{k} V/pC against {want}");
    assert_eq!((r["particles"].as_u64(), r["walls"]["z+"]["current"].as_f64().map(f64::is_sign_negative)), (Some(0), Some(true)));
    // Hostile bunches are refused.
    for p in [
        json!({"face": "z-", "charge": -1, "length": 2}),
        json!({"face": "z-", "charge": 1, "length": 0}),
        json!({"face": "q", "charge": 1, "length": 1}),
    ] {
        assert!(s.execute("particles.bunch", &p).is_err(), "{p}");
    }
}

#[test]
fn particle_runs_take_lossless_dielectrics_only() {
    // A lossy filling is refused rather than run as if lossless.
    let mut s = Session::new();
    run(
        &mut s,
        &[
            ("model.material.create", json!({"name": "lossy", "eps_r": 2.25, "loss_tangent": 0.01})),
            ("geometry.solid", json!({"shape": "cylinder", "radius": 10, "length": 13.06, "material": "lossy"})),
            ("particles.bunch", json!({"face": "z-", "charge": 0.001, "length": 2})),
        ],
    );
    let e = s.execute("solve.particles", &json!({"time": 0.1, "cell": 1})).unwrap_err().to_string();
    assert!(e.contains("lossy") && e.contains("not supported yet"), "{e}");
}

#[test]
fn beams_are_checked_and_reply_their_gamma() {
    let mut s = Session::new();
    s.execute("geometry.solid", &json!({"shape": "cylinder", "radius": 5, "length": 10})).unwrap();
    for p in [
        json!({"face": "z-", "current": -1, "energy": 1e5}),
        json!({"face": "z-", "current": 1, "energy": 0}),
        json!({"face": "z-", "current": 1, "energy": 1e5, "radius": 1, "inner": 2}),
        json!({"face": "z-", "current": 1, "energy": 1e5, "rise": -5}),
    ] {
        assert!(s.execute("particles.beam", &p).is_err(), "{p}");
    }
    s.execute("particles.beam", &json!({"face": "z-", "current": 10, "energy": 1e5, "radius": 2})).unwrap();
    let r = s.execute("solve.particles", &json!({"time": 0.1, "cell": 1})).unwrap();
    // 100 keV: γ = 1.196 at the start, a little less where the beam has got to.
    let g = r["mid_gamma"].as_f64().unwrap();
    assert!(g > 1.18 && g < 1.1958, "{g}");
}

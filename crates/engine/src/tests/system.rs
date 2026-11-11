//! A system's chain: its stages edited by command, `solve.system`'s budget against the closed
//! forms of a cascade — Friis' noise factor, the gains in dB added, kT₀B, the cascade's intercept
//! point (Pozar §10.1, §10.3).

use serde_json::{Value, json};

use crate::Session;
use crate::tests::scratch;

/// kT₀ over 1 MHz in dBm (Boltzmann's constant times 290 K times 1 MHz).
fn kt0b_dbm() -> f64 {
    10.0 * (1.380_649e-23 * 290.0 * 1e6f64).log10() + 30.0
}

fn db(x: f64) -> f64 {
    10.0 * x.log10()
}

fn ratio(db: f64) -> f64 {
    10f64.powf(db / 10.0)
}

/// An LNA (20 dB, 1 dB, OIP3 30 dBm, P1dB 20 dBm), a 3 dB pad and a second stage (10 dB, 6 dB,
/// OIP3 20 dBm): its source −80 dBm from 290 K, the receiver 1 MHz needing 10 dB.
fn receiver() -> Session {
    let mut s = Session::new();
    for (kind, params) in [
        ("amplifier", json!({"name": "LNA", "gain": 20, "noise_figure": 1, "oip3": 30, "p1db": 20})),
        ("attenuator", json!({"name": "pad", "loss": 3})),
        ("amplifier", json!({"name": "IF", "gain": 10, "noise_figure": 6, "oip3": 20})),
    ] {
        let mut p = params;
        p["type"] = json!(kind);
        s.execute("system.stage.add", &p).unwrap();
    }
    s.execute("system.source", &json!({"power": -80})).unwrap();
    s.execute("system.receiver", &json!({"bandwidth": 0.001, "snr": 10})).unwrap();
    s
}

fn row(r: &Value, k: usize, key: &str) -> f64 {
    r["budget"]["stages"][k][key].as_f64().unwrap_or_else(|| panic!("{key} of stage {k}: {r}"))
}

#[test]
fn a_chains_budget_follows_friis_and_the_cascades_intercept_point() {
    let mut s = receiver();
    assert_eq!(s.model.analysis(), emcraft_model::Analysis::System, "a chain and no geometry");
    let r = s.execute("solve.system", &json!({"start": 1, "stop": 1.2, "points": 3})).unwrap();
    // Gains add in dB: 20, 17, 27.
    for (k, g) in [20.0, 17.0, 27.0].into_iter().enumerate() {
        approx::assert_abs_diff_eq!(row(&r, k, "gain_db"), g, epsilon = 1e-9);
    }
    approx::assert_abs_diff_eq!(row(&r, 1, "stage_gain_db"), -3.0, epsilon = 1e-9);
    // Friis: F = F₁ + (L − 1)/G₁ + (F₃ − 1)L/G₁, the pad at T₀ a noise factor of its loss.
    let (f1, l, f3, g1) = (ratio(1.0), ratio(3.0), ratio(6.0), ratio(20.0));
    let figures = [f1, f1 + (l - 1.0) / g1, f1 + (l - 1.0) / g1 + (f3 - 1.0) * l / g1];
    for (k, f) in figures.into_iter().enumerate() {
        approx::assert_abs_diff_eq!(row(&r, k, "nf_db"), db(f), epsilon = 1e-9);
        approx::assert_abs_diff_eq!(row(&r, k, "noise_temperature"), 290.0 * (f - 1.0), epsilon = 1e-6);
        // From a 290 K source the output noise is kT₀B F G, the signal −80 dBm + G.
        let g = [20.0, 17.0, 27.0][k];
        approx::assert_abs_diff_eq!(row(&r, k, "signal_dbm"), -80.0 + g, epsilon = 1e-9);
        approx::assert_abs_diff_eq!(row(&r, k, "noise_dbm"), kt0b_dbm() + db(f) + g, epsilon = 1e-9);
        approx::assert_abs_diff_eq!(row(&r, k, "snr_db"), -80.0 - kt0b_dbm() - db(f), epsilon = 1e-9);
    }
    // The sensitivity: kT₀B F + SNR.
    approx::assert_abs_diff_eq!(r["budget"]["sensitivity_dbm"].as_f64().unwrap(), kt0b_dbm() + db(figures[2]) + 10.0, epsilon = 1e-9);
    // IIP3: each amplifier's OIP3 − its gain (10 dBm both), joined as 1/IIP3 = Σ G_before/IIP3_k;
    // the pad has none. Output-referred: IIP3 + G.
    let iip3 = db(1.0 / (1.0 / 10.0 + ratio(17.0) / 10.0));
    approx::assert_abs_diff_eq!(row(&r, 0, "iip3_dbm"), 10.0, epsilon = 1e-9);
    approx::assert_abs_diff_eq!(row(&r, 1, "iip3_dbm"), 10.0, epsilon = 1e-9);
    approx::assert_abs_diff_eq!(row(&r, 2, "iip3_dbm"), iip3, epsilon = 1e-9);
    approx::assert_abs_diff_eq!(row(&r, 2, "oip3_dbm"), iip3 + 27.0, epsilon = 1e-9);
    // P1dB: the LNA's 20 dBm out is 20 − (20 − 1) = 1 dBm in; out of the chain, IP1dB + G − 1.
    approx::assert_abs_diff_eq!(row(&r, 2, "ip1db_dbm"), 1.0, epsilon = 1e-9);
    approx::assert_abs_diff_eq!(row(&r, 2, "op1db_dbm"), 1.0 + 27.0 - 1.0, epsilon = 1e-9);
    approx::assert_abs_diff_eq!(r["nf_db_least"].as_f64().unwrap(), db(figures[2]), epsilon = 1e-9);
    // The result is held and goes stale when the chain changes.
    assert!(s.results.system.is_some());
    s.execute("system.stage.edit", &json!({"stage": "pad", "loss": 6})).unwrap();
    assert!(s.results.is_stale(crate::ResultKind::System));
}

#[test]
fn stages_are_edited_moved_and_deleted_by_name_or_place_and_undone() {
    let mut s = receiver();
    let names = |s: &Session| s.model.system.stages.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
    s.execute("system.stage.move", &json!({"stage": "pad", "position": 1})).unwrap();
    assert_eq!(names(&s), ["pad", "LNA", "IF"]);
    s.execute("system.stage.edit", &json!({"stage": 2, "rename": "LNA2", "noise_figure": 0.5})).unwrap();
    assert_eq!(names(&s), ["pad", "LNA2", "IF"]);
    // The settings not given are kept.
    let r = s.execute("system.stage.edit", &json!({"stage": "LNA2"})).unwrap();
    assert_eq!(r["stages"][1]["gain"], 20.0);
    assert_eq!(r["stages"][1]["noise_figure"], 0.5);
    // Another kind: its settings given anew.
    s.execute("system.stage.edit", &json!({"stage": "pad", "type": "line", "length": 100, "attenuation": 0.01})).unwrap();
    assert_eq!(s.model.system.stages[0].part.kind(), "line");
    s.execute("system.stage.delete", &json!({"stage": "IF"})).unwrap();
    assert_eq!(names(&s), ["pad", "LNA2"]);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(names(&s), ["pad", "LNA2", "IF"]);
    let unnamed = s.execute("system.stage.add", &json!({"type": "attenuator", "loss": 1, "position": 1})).unwrap();
    assert_eq!(unnamed["stages"][0]["name"], "attenuator 1");
    for (command, bad) in [
        ("system.stage.add", json!({"type": "mixer"})),
        ("system.stage.add", json!({"type": "amplifier"})),
        ("system.stage.add", json!({"type": "amplifier", "gain": 1e9})),
        ("system.stage.add", json!({"type": "amplifier", "gain": 10, "name": "IF"})),
        ("system.stage.add", json!({"type": "attenuator", "loss": -1})),
        ("system.stage.add", json!({"type": "line", "length": -1})),
        ("system.stage.edit", json!({"stage": "nope", "gain": 1})),
        ("system.stage.edit", json!({"stage": 9, "gain": 1})),
        ("system.stage.edit", json!({"stage": "IF", "noise_figure": -2})),
        ("system.stage.move", json!({"stage": "IF", "position": 0})),
        ("system.stage.delete", json!({})),
        ("system.source", json!({"temperature": -1})),
        ("system.source", json!({"power": 1e9})),
        ("system.receiver", json!({"bandwidth": -1})),
        ("system.receiver", json!({"impedance": 0})),
    ] {
        let before = s.model.system.clone();
        assert!(s.execute(command, &bad).is_err(), "{command} {bad}");
        assert_eq!(s.model.system, before, "{command} {bad}");
    }
}

#[test]
fn a_chain_reads_its_band_from_the_setup_its_antennas_impedance_and_its_data_stages() {
    let mut s = Session::new();
    assert!(s.execute("solve.system", &json!({"start": 1, "stop": 2})).is_err(), "no stages");
    // A 100 Ω antenna (|Γs|² = 1/9) on an amplifier of 1 dB: F = F_min + (F_min − 1)/8; its
    // available gain from it, matched and unilateral, |S21|²(1 − |Γs|²).
    let ant = scratch("ant.s1p");
    std::fs::write(&ant, "# GHz S RI R 50\n1 0.3333333333333333 0\n1.2 0.3333333333333333 0\n").unwrap();
    s.execute("results.import", &json!({"path": ant, "name": "ant"})).unwrap();
    s.execute("system.stage.add", &json!({"type": "amplifier", "gain": 20, "noise_figure": 1})).unwrap();
    s.execute("system.source", &json!({"antenna": "ant"})).unwrap();
    assert!(s.execute("solve.system", &json!({})).is_err(), "no band");
    s.execute("project.setup", &json!({"kind": "system", "start": 1, "stop": 1.4, "points": 5, "frequency": 1.1})).unwrap();
    let r = s.execute("solve.run", &json!({})).unwrap();
    assert_eq!(r["run"]["command"], "solve.system");
    let f = ratio(1.0);
    approx::assert_abs_diff_eq!(row(&r, 0, "nf_db"), db(f + (f - 1.0) / 8.0), epsilon = 1e-9);
    approx::assert_abs_diff_eq!(row(&r, 0, "gain_db"), 20.0 + db(8.0 / 9.0), epsilon = 1e-9);
    assert_eq!(r["budget"]["frequency"], 1.1);
    // Past the antenna's band (1.2 GHz) its impedance is not known: said, not guessed.
    let system = s.results.system.clone().unwrap();
    let past = system.nearest(1.4e9).unwrap();
    assert!(past.rows[0].nf_db.is_none() && past.why.as_deref().is_some_and(|w| w.contains("antenna")), "{past:?}");
    // A data stage: a matched 3 dB pad's S, no noise data, its noise a passive network's.
    let pad = scratch("pad.s2p");
    std::fs::write(
        &pad,
        "# GHz S MA R 50\n1 0 0 0.7071067811865476 0 0.7071067811865476 0 0 0\n1.4 0 0 0.7071067811865476 0 0.7071067811865476 0 0 0\n",
    )
    .unwrap();
    s.execute("results.import", &json!({"path": pad, "name": "pad"})).unwrap();
    s.execute("system.source", &json!({"antenna": ""})).unwrap();
    s.execute("system.stage.add", &json!({"type": "data", "data": "pad", "position": 1})).unwrap();
    let r = s.execute("solve.system", &json!({"start": 1, "stop": 1.4, "points": 2})).unwrap();
    approx::assert_abs_diff_eq!(row(&r, 1, "nf_db"), db(2.0 * f), epsilon = 1e-9);
    // A data stage naming no measurement: refused, its stage named.
    s.execute("system.stage.add", &json!({"type": "data", "data": "gone"})).unwrap();
    let e = s.execute("solve.system", &json!({"start": 1, "stop": 1.4})).unwrap_err().to_string();
    assert!(e.contains("data 2") && e.contains("gone"), "{e}");
}

#[test]
fn a_system_saved_opens_with_its_chain_and_budget_and_a_hostile_one_is_left_out() {
    let mut s = receiver();
    s.execute("solve.system", &json!({"start": 1, "stop": 1.2, "points": 3, "frequency": 1.1})).unwrap();
    let path = scratch("rx-system.emcraft");
    s.execute("file.save", &json!({"path": path})).unwrap();
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": path})).unwrap();
    assert_eq!(t.model.system, s.model.system);
    assert_eq!(t.model.analysis(), emcraft_model::Analysis::System);
    assert_eq!(t.results.system, s.results.system, "read back, not solved again");
    // A budget whose rows do not match its stages is refused on its own.
    let mut bad = (*s.results.system.clone().unwrap()).clone();
    bad.points[0].rows.pop();
    assert!(bad.check().is_err());
    let mut bad = (*s.results.system.clone().unwrap()).clone();
    bad.at = -1.0;
    assert!(bad.check().is_err());
}

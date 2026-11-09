//! Results in the project file: a project opens with its results as they were — the sweep and
//! its fields, the eigenmodes, the time domain, the dispersion, the study's table, the open
//! problems — without solving again, bit for bit; a result that no longer fits is left out.

use serde_json::json;

use super::{project_json, scratch, write_project};
use crate::*;

/// `s` saved (with `fields`, when they fit) and opened in a fresh session.
fn reopened(s: &mut Session, name: &str, fields: bool) -> (Session, serde_json::Value) {
    let path = scratch(&format!("saved-{name}.emcraft"));
    let r = s.execute("file.save", &json!({"path": path, "fields": fields})).unwrap();
    assert!(r["fields"] == fields || r["fields"] == false, "{name}");
    if r["fields"] == false {
        // What is compared is what was written: the results without their fields.
        strip_fields(&mut s.results);
    }
    let mut t = Session::new();
    let opened = t.execute("file.open", &json!({"path": path})).unwrap();
    // A solved template's file with its fields is large: gone once read.
    let _ = std::fs::remove_file(&path);
    (t, opened)
}

/// The results without their field vectors (as a file written without them reads back).
fn strip_fields(r: &mut Results) {
    if let Some(sw) = r.sweep.as_mut() {
        let sw = std::sync::Arc::make_mut(sw);
        sw.results.iter_mut().for_each(|p| p.fields.clear());
    }
    if let Some(e) = r.eigen.as_mut() {
        std::sync::Arc::make_mut(e).modes.iter_mut().for_each(|m| m.vector.clear());
    }
}

/// Whether two sessions hold the same results, number for number (NaNs alike).
fn same_results(a: &Results, b: &Results) {
    let sweep =
        |r: &Results| r.sweep.as_ref().map(|sw| (sw.order, sw.deembed.clone(), sw.results.clone(), sw.driven.dofs.unknowns, sw.driven.iterative));
    assert_eq!(sweep(a), sweep(b));
    let eigen = |r: &Results| r.eigen.as_ref().map(|e| (e.modes.clone(), e.iterations, e.driven.dofs.unknowns));
    assert_eq!(eigen(a), eigen(b));
    // An open problem's field compared where both have it (a file may leave it out).
    let open = |r: &Results| {
        r.radiation.as_ref().map(|o| {
            (
                emcraft_solver::radiation::OpenResult { field: Vec::new(), ..o.result.clone() },
                o.huygens.clone(),
                o.power.to_bits(),
                o.driven.dofs.unknowns,
            )
        })
    };
    assert_eq!(open(a), open(b));
    if let (Some(x), Some(y)) = (&a.radiation, &b.radiation)
        && !y.result.field.is_empty()
    {
        assert_eq!(x.result.field, y.result.field);
    }
    let debug = |x: &dyn std::fmt::Debug| format!("{x:?}");
    assert_eq!(debug(&a.time_domain), debug(&b.time_domain));
    assert_eq!(debug(&a.td_radiation), debug(&b.td_radiation));
    assert_eq!(debug(&a.dispersion), debug(&b.dispersion));
    assert_eq!(debug(&a.study), debug(&b.study));
}

#[test]
fn a_solved_sweep_opens_with_its_fields_and_needs_no_solve() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass", "solve": true})).unwrap();
    assert!(s.results.sweep.as_ref().is_some_and(|sw| sw.results.iter().all(|r| !r.fields.is_empty())));
    let (mut t, opened) = reopened(&mut s, "sweep", true);
    assert_eq!((opened["results"].as_bool(), opened["left_out"].as_array().map(Vec::len)), (Some(true), Some(0)));
    same_results(&s.results, &t.results);
    // Current, the kept solve it is in memory (restoring it solves nothing), clean.
    assert!(!t.results().any_stale());
    assert!(!t.is_dirty() && t.archive().last().is_some_and(super::super::archive::Archived::in_memory));
    // The field at a point reads as before.
    let probe = json!({"frequency": 8, "at": [[11.43, 5.08, 50.0]]});
    assert_eq!(t.execute("results.probe", &probe).unwrap(), s.execute("results.probe", &probe).unwrap());
}

#[test]
fn without_fields_the_s_parameters_open_and_the_fields_ask_for_a_solve() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass", "solve": true})).unwrap();
    let (mut t, _) = reopened(&mut s, "no-fields", false);
    let (a, b) = (s.results.sweep.as_ref().unwrap(), t.results.sweep.as_ref().unwrap());
    assert!(a.results.iter().zip(&b.results).all(|(x, y)| x.s == y.s && y.fields.is_empty()));
    assert!(t.execute("results.probe", &json!({"frequency": 8, "at": [[11.43, 5.08, 50.0]]})).is_err());
    assert!(t.execute("results.sparameters", &json!({})).is_ok());
}

#[test]
fn eigenmodes_open_with_their_vectors() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "cavity", "solve": true})).unwrap();
    assert!(s.results.eigen.is_some());
    let (t, _) = reopened(&mut s, "eigen", true);
    same_results(&s.results, &t.results);
}

/// Results that need no system (the time domain, the open time domain, a dispersion, a study's
/// table with its NaNs and predictions) read back exactly.
#[test]
fn tables_read_back_exactly() {
    // No closed form: the theory's types are only data here.
    use emcraft_solver::Complex64 as C;
    let mut s = Session::new();
    let c = |re: f64, im: f64| C::new(re, im);
    s.results.time_domain = Some(std::sync::Arc::new(emcraft_fdtd::TdResult {
        frequencies: vec![1e9, 2e9],
        ports: 3,
        inputs: vec![0, 2],
        s: (0..12).map(|i| c(0.1 * i as f64, -0.05 * i as f64)).collect(),
        cells: 1000,
        dt: 1e-12,
        steps: 500,
        reference_steps: 400,
        converged: true,
        loss_reference: 1.5e9,
    }));
    s.results.dispersion = Some(std::sync::Arc::new(emcraft_solver::bloch::Dispersion {
        axis: 2,
        period: 0.01,
        points: vec![emcraft_solver::bloch::BlochPoint { psi: 0.5, beta: 50.0, frequencies: vec![1e9, 3e9], iterations: 7 }],
        unknowns: 321,
    }));
    s.results.td_radiation = Some(std::sync::Arc::new(emcraft_fdtd::open::OpenRun {
        frequencies: vec![1e9],
        ports: vec![emcraft_fdtd::open::PortSpectrum {
            name: "L1".into(),
            voltage: vec![c(1.0, 0.5)],
            current: vec![c(0.02, 0.0)],
            impedance: vec![c(50.0, 25.0)],
            power: vec![0.01],
        }],
        radiated: vec![0.009],
        huygens: vec![emcraft_theory::radiation::Huygens {
            frequency: 1e9,
            centre: emcraft_model::Point3::new(0.0, 0.0, 0.0),
            samples: vec![emcraft_theory::radiation::HuygensSample {
                point: emcraft_model::Point3::new(0.1, 0.0, 0.0),
                weight: 0.01,
                normal: emcraft_model::Vec3::new(1.0, 0.0, 0.0),
                e: emcraft_model::CVec3 { x: c(0.0, 0.0), y: c(1.0, 0.0), z: c(0.0, -1.0) },
                h: emcraft_model::CVec3 { x: c(0.0, 0.0), y: c(0.0, 0.01), z: c(0.002, 0.0) },
            }],
        }],
        cells: 64,
        steps: 99,
        dt: 2e-12,
        converged: false,
    }));
    let row = |v: f64| crate::study::Row {
        values: vec![v, 2.0],
        frequencies: vec![7e9, 8e9],
        s: vec![("s11".into(), vec![c(0.1, 0.0), c(f64::NAN, 0.0)]), ("s21".into(), vec![c(0.9, 0.0), c(0.8, 0.0)])],
        modes: vec![],
        scalars: vec![("power".into(), f64::NEG_INFINITY)],
        objective: Some(-20.0),
        error: None,
        prediction: Some(crate::study::Predicted {
            db: vec![("s11".into(), vec![-20.0, -30.0], vec![0.5, 0.6])],
            modes_sigma: vec![],
            scalars_sigma: vec![("power".into(), 0.1)],
        }),
    };
    s.results.study = Some(std::sync::Arc::new(crate::study::Study {
        kind: crate::study::Kind::Sweep,
        variables: vec!["a".into(), "b".into()],
        grid: vec![vec![1.0, 3.0], vec![2.0]],
        solve: "solve.frequency".into(),
        frequency_unit: "GHz".into(),
        goal: None,
        rows: vec![row(1.0), crate::study::Row { error: Some("did not mesh".into()), ..row(3.0) }],
        best: Some(1),
        converged: true,
        surrogate: Some(crate::study::Surrogate { solves: 2, lengths: vec![0.3, 0.5], at_best: Some((-21.0, 0.4)) }),
    }));
    let (t, opened) = reopened(&mut s, "tables", true);
    assert_eq!(opened["left_out"].as_array().map(Vec::len), Some(0), "{opened}");
    same_results(&s.results, &t.results);
}

/// A result whose numbers no longer fit (fewer than its shape says, or a system that does not
/// match) is left out with why; the project opens.
#[test]
fn results_that_do_not_fit_are_left_out_with_why() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass", "solve": true})).unwrap();
    let good = scratch("saved-fit.emcraft");
    s.execute("file.save", &json!({"path": good})).unwrap();
    let doc = project_json(&good);
    let path = scratch("saved-unfit.emcraft");
    for (at, value, why) in [
        ("/results/sweep/entries", json!(3), "entries of S"),
        ("/results/sweep/points/0/impedances", json!([true, true, true]), "impedances"),
        ("/results/sweep/numbers", json!("results/eigen/numbers.f64"), "numbers.f64"),
        ("/results/sweep/order", json!(5), "order"),
    ] {
        let mut d = doc.clone();
        *d.pointer_mut(at).unwrap() = value;
        write_project(&path, &d, Some(&good));
        let mut t = Session::new();
        let r = t.execute("file.open", &json!({"path": path})).unwrap();
        let left: Vec<crate::store::FileError> = serde_json::from_value(r["left_out"].clone()).unwrap();
        assert!(left.len() == 1 && left[0].part == "the sweep" && left[0].why.contains(why) && t.results.sweep.is_none(), "{at}: {left:?}");
        assert!(!t.results().any_stale());
    }
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn every_solved_template_opens_with_its_results() {
    // The whole board's sweep takes minutes: the region-sized boards stand for it.
    for t in emcraft_templates::TEMPLATES.iter().filter(|t| t.name != "pcb-board") {
        let mut s = Session::new();
        s.execute("file.sample", &json!({"name": t.name, "solve": true})).unwrap();
        let (u, opened) = reopened(&mut s, t.name, true);
        assert_eq!(opened["left_out"].as_array().map(Vec::len), Some(0), "{}: {opened}", t.name);
        same_results(&s.results, &u.results);
        assert!(u.results().stale_kinds().eq(s.results().stale_kinds()), "{}", t.name);
    }
}

/// Property tests: results that need no system, random in their shapes and values, written to a
/// file and read back the same — the write and the read of each kind cannot drift apart.
mod properties {
    use super::*;
    use emcraft_solver::Complex64 as C;
    use proptest::prelude::*;
    use proptest::test_runner::{Config, RngSeed};

    fn x() -> impl Strategy<Value = f64> {
        prop_oneof![-1e3..1e3f64, Just(0.0), Just(f64::NAN), Just(f64::INFINITY)]
    }
    fn c() -> impl Strategy<Value = C> {
        (-1.0..1.0f64, -1.0..1.0f64).prop_map(|(re, im)| C::new(re, im))
    }
    fn frequencies(n: usize) -> Vec<f64> {
        (1..=n).map(|k| 1e9 * k as f64).collect()
    }

    fn time_domain() -> impl Strategy<Value = emcraft_fdtd::TdResult> {
        (1usize..6).prop_flat_map(|n| {
            (prop::collection::vec(c(), n), prop::collection::vec(c(), n), 0usize..1000, any::<bool>()).prop_map(
                move |(s11, s21, steps, converged)| emcraft_fdtd::TdResult {
                    frequencies: frequencies(n),
                    ports: 2,
                    inputs: vec![0],
                    s: s11.into_iter().chain(s21).collect(),
                    cells: steps * 3,
                    dt: 1e-12,
                    steps,
                    reference_steps: steps / 2,
                    converged,
                    loss_reference: 1e9,
                },
            )
        })
    }

    fn dispersion() -> impl Strategy<Value = emcraft_solver::bloch::Dispersion> {
        prop::collection::vec((x(), x(), prop::collection::vec(1e8..1e11f64, 0..4), 0usize..50), 0..5).prop_map(|points| {
            emcraft_solver::bloch::Dispersion {
                axis: 2,
                period: 0.01,
                points: points
                    .into_iter()
                    .map(|(psi, beta, frequencies, iterations)| emcraft_solver::bloch::BlochPoint { psi, beta, frequencies, iterations })
                    .collect(),
                unknowns: 99,
            }
        })
    }

    fn row(variables: usize) -> impl Strategy<Value = crate::study::Row> {
        (1usize..4).prop_flat_map(move |f| {
            (
                prop::collection::vec(x(), variables),
                prop::collection::vec((Just("s11".to_owned()), prop::collection::vec(c(), f)), 0..3),
                prop::collection::vec(x(), 0..3),
                prop::collection::vec((Just("power".to_owned()), x()), 0..2),
                prop::option::of(x()),
                prop::option::of(Just("did not mesh".to_owned())),
                prop::option::of((prop::collection::vec(x(), f), prop::collection::vec(x(), f), prop::collection::vec(x(), 0..2))),
            )
                .prop_map(move |(values, s, modes, scalars, objective, error, predicted)| crate::study::Row {
                    values,
                    frequencies: frequencies(f),
                    s,
                    modes,
                    scalars,
                    objective,
                    error,
                    prediction: predicted.map(|(db, sigma, modes_sigma)| crate::study::Predicted {
                        db: vec![("s11".into(), db, sigma)],
                        modes_sigma,
                        scalars_sigma: vec![("power".into(), 0.5)],
                    }),
                })
        })
    }

    fn study() -> impl Strategy<Value = crate::study::Study> {
        (1usize..3).prop_flat_map(|v| {
            (prop::collection::vec(row(v), 1..5), any::<bool>(), prop::option::of(prop::collection::vec(0.0..1.0f64, v))).prop_map(
                move |(rows, optimize, lengths)| {
                    let best = Some(rows.len() - 1);
                    crate::study::Study {
                        kind: if optimize { crate::study::Kind::Optimize } else { crate::study::Kind::Sweep },
                        variables: (0..v).map(|k| format!("v{k}")).collect(),
                        grid: Vec::new(),
                        solve: "solve.frequency".into(),
                        frequency_unit: "GHz".into(),
                        goal: optimize.then(|| "min |S11|".into()),
                        rows,
                        best,
                        converged: optimize,
                        surrogate: lengths.map(|lengths| crate::study::Surrogate { solves: 3, lengths, at_best: Some((-1.0, 0.1)) }),
                    }
                },
            )
        })
    }

    proptest! {
        #![proptest_config(Config { cases: 32, rng_seed: RngSeed::Fixed(0x454d_4372), failure_persistence: None, ..Config::default() })]

        #[test]
        fn tables_round_trip(td in time_domain(), d in dispersion(), st in study()) {
            let mut s = Session::new();
            s.results.time_domain = Some(std::sync::Arc::new(td));
            s.results.dispersion = Some(std::sync::Arc::new(d));
            s.results.study = Some(std::sync::Arc::new(st));
            let (t, opened) = reopened(&mut s, "round-trip", true);
            prop_assert_eq!(opened["left_out"].as_array().map(Vec::len), Some(0), "{}", opened);
            same_results(&s.results, &t.results);
        }
    }
}

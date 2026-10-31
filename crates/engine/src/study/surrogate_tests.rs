//! The surrogate studies through the session: a sweep that solves a budget of points and
//! predicts the rest, and the Bayesian optimizer, on tiny models.

use serde_json::{Value, json};

use super::tests::quarter_wave;
use crate::{EngineError, Session};

/// The high-pass filter with its middle section's length a variable `l`, meshed coarsely.
fn filter() -> Session {
    let mut s = Session::new();
    s.run_script(&json!([
        {"command": "project.variable", "params": {"name": "l", "value": 20}},
        {"command": "model.material.create", "params": {"name": "ptfe-like", "eps_r": 2.0}},
        {"command": "geometry.waveguide", "params": {"width": 22.86, "height": 10.16, "sections": [
            {"length": 15, "material": "ptfe-like"}, {"length": "=l", "material": "vacuum"}, {"length": 15, "material": "ptfe-like"}]}},
        {"command": "mesh.generate", "params": {"size": 8}},
        {"command": "model.port.create", "params": {"name": "1", "end": "start", "modes": 1}},
        {"command": "model.port.create", "params": {"name": "2", "end": "end", "modes": 1}}
    ]))
    .unwrap();
    s
}

fn db(pt: &Value, name: &str) -> f64 {
    pt[name]["db"].as_f64().unwrap_or_else(|| 20.0 * pt[name]["mag"].as_f64().unwrap_or(f64::NAN).log10())
}

#[test]
fn a_surrogate_sweep_solves_its_budget_and_predicts_the_rest() {
    let mut s = filter();
    let solve = json!({"command": "solve.frequency", "params": {"frequencies": [7, 9], "order": 1}});
    let sweep = |budget: Option<usize>| {
        let mut p = json!({"variable": "l", "start": 14, "stop": 26, "points": 7, "solve": solve, "outputs": ["s21", "s11"]});
        if let Some(b) = budget {
            p["surrogate"] = json!({"budget": b});
        }
        p
    };
    let full = s.execute("study.sweep", &sweep(None)).unwrap();
    let r = s.execute("study.sweep", &sweep(Some(4))).unwrap();
    assert!(full.get("surrogate").is_none());
    assert_eq!(r["surrogate"]["solved"], 4);
    assert_eq!(r["surrogate"]["predicted"], 3);
    assert_eq!(r["surrogate"]["length_scales"].as_array().map(Vec::len), Some(1));
    let rows = r["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 7);
    for (row, exact) in rows.iter().zip(full["rows"].as_array().unwrap()) {
        if row.get("predicted").is_none() {
            // A solved row is as the full sweep's.
            assert_eq!(row, exact);
            continue;
        }
        assert_eq!(row["predicted"], true);
        assert_eq!(row["value"], exact["value"]);
        for (pt, e) in row["points"].as_array().unwrap().iter().zip(exact["points"].as_array().unwrap()) {
            for name in ["s11", "s21"] {
                let (got, want, sigma) = (db(pt, name), db(e, name), pt[name]["sigma_db"].as_f64().unwrap());
                assert!(sigma.is_finite() && sigma > 0.0, "{pt}");
                assert!((got - want).abs() < 4.0 * sigma + 0.5, "{name} at l = {}: {got} ± {sigma} against {want}", row["value"]);
            }
        }
    }
    // The same sweep, the same choice and the same numbers.
    assert_eq!(s.execute("study.sweep", &sweep(Some(4))).unwrap(), r);
    // Charted: the predicted values dashed over their bands.
    let study = s.results.study.clone().unwrap();
    assert_eq!(study.rows.iter().filter(|r| r.is_predicted()).count(), 3);
    let c = super::chart(&study, Some("s21"), &super::Slice::default()).unwrap();
    use emcraft_render::chart::Style;
    assert_eq!(c.series.iter().filter(|x| x.style == Style::Dashed).count(), 3);
    assert_eq!(c.series.iter().filter(|x| x.style == Style::Band).count(), 3);
    assert!(c.series.iter().any(|x| x.label.ends_with("(predicted)")));
    // Estimated as its budget of solves.
    let e = s.execute("project.estimate", &json!({"command": "study.sweep", "params": sweep(Some(4))})).unwrap();
    assert!(e["what"].as_str().is_some_and(|w| w.starts_with("4 points")), "{e}");
    // A budget of every point solves them all.
    let all = s.execute("study.sweep", &sweep(Some(7))).unwrap();
    assert_eq!((all["surrogate"]["solved"].clone(), all["surrogate"]["predicted"].clone()), (json!(7), json!(0)));
    assert_eq!(all["rows"], full["rows"]);
}

#[test]
fn a_surrogate_grid_at_one_frequency_is_charted_with_hollow_points() {
    let mut s = filter();
    s.execute("project.variable", &json!({"name": "er", "value": 2})).unwrap();
    s.execute("model.material.edit", &json!({"name": "ptfe-like", "eps_r": "=er"})).unwrap();
    let p = json!({"variables": [{"name": "l", "values": [14, 18, 22, 26]}, {"name": "er", "values": [1.8, 2.2]}],
                   "solve": {"command": "solve.frequency", "params": {"frequencies": [8], "order": 1}}, "outputs": ["s21"],
                   "surrogate": {"budget": 5}});
    let r = s.execute("study.sweep", &p).unwrap();
    assert_eq!((r["surrogate"]["solved"].clone(), r["surrogate"]["predicted"].clone()), (json!(5), json!(3)));
    assert_eq!(r["surrogate"]["length_scales"].as_array().map(Vec::len), Some(2));
    let study = s.results.study.clone().unwrap();
    let c = super::chart(&study, Some("s21"), &super::Slice::default()).unwrap();
    use emcraft_render::chart::Style;
    assert!(c.series.iter().any(|x| x.style == Style::Hollow) && c.series.iter().any(|x| x.style == Style::Band));
}

#[test]
fn hostile_surrogates_are_refused() {
    let mut s = filter();
    let solve = json!({"command": "solve.frequency", "params": {"frequencies": [8], "order": 1}});
    for bad in [
        json!({"variable": "l", "values": [10, 20, 30], "solve": solve, "surrogate": {"budget": 1}}),
        json!({"variable": "l", "values": [10, 20, 30], "solve": solve, "surrogate": {"budget": 4}}),
        json!({"variable": "l", "values": [10, 20, 30], "solve": solve, "surrogate": {"budget": 0}}),
        json!({"variable": "l", "values": [10, 20, 30], "solve": solve, "surrogate": {"budget": -2}}),
        json!({"variable": "l", "values": [10, 20, 30], "solve": solve, "surrogate": {"budget": 2.5}}),
        json!({"variable": "l", "values": [10, 20, 30], "solve": solve, "surrogate": {"budget": "=0/0"}}),
        json!({"variable": "l", "values": [10, 20, 30], "solve": solve, "surrogate": {"budget": 1e300}}),
        json!({"variable": "l", "values": [10, 20, 30], "solve": solve, "surrogate": {}}),
        json!({"variable": "l", "values": [10, 20, 30], "solve": solve, "surrogate": 3}),
        json!({"variable": "l", "values": [10, 20], "solve": solve, "surrogate": {"budget": 2}}),
        json!({"variable": "l", "values": [10, 20, 10], "solve": solve, "surrogate": {"budget": 2}}),
    ] {
        let r = s.execute("study.sweep", &bad);
        assert!(matches!(r, Err(EngineError::Params(_))), "{bad}: {r:?}");
    }
    let bounds = json!([{"name": "l", "min": 10, "max": 30}]);
    for bad in [json!("genetic"), json!(3), json!("")] {
        let r = s.execute("study.optimize", &json!({"variables": bounds, "goal": {"type": "min_max_db"}, "solve": solve, "method": bad}));
        assert!(matches!(r, Err(EngineError::Params(_))), "{bad}: {r:?}");
    }
    assert!(s.results.study.is_none());
}

/// The quarter-wave match (see `tests::quarter_wave`) found by `method`: the reply.
fn quarter_wave_by(method: &str, size: f64, order: u8, tolerance: f64) -> (Value, Session) {
    let (er, _) = quarter_wave();
    let mut s = Session::new();
    s.run_script(&json!([
        {"command": "project.variable", "params": {"name": "l", "value": 8}},
        {"command": "model.material.create", "params": {"name": "match", "eps_r": er}},
        {"command": "model.material.create", "params": {"name": "load", "eps_r": 2.2}},
        {"command": "geometry.waveguide", "params": {"width": 22.86, "height": 10.16, "sections": [
            {"length": 8, "material": "vacuum"}, {"length": "=l", "material": "match"}, {"length": 8, "material": "load"}]}},
        {"command": "mesh.generate", "params": {"size": size}},
        {"command": "model.port.create", "params": {"name": "1", "end": "start", "modes": 1}},
        {"command": "model.port.create", "params": {"name": "2", "end": "end", "modes": 1}}
    ]))
    .unwrap();
    let r = s
        .execute(
            "study.optimize",
            &json!({"variables": [{"name": "l", "min": 4, "max": 18}], "goal": {"type": "min_max_db", "parameter": "s11"},
                    "solve": {"command": "solve.frequency", "params": {"frequencies": [8], "order": order}}, "tolerance": tolerance, "method": method}),
        )
        .unwrap();
    (r, s)
}

#[test]
fn the_bayesian_optimizer_finds_the_quarter_wave_match_in_no_more_solves() {
    let (_, quarter) = quarter_wave();
    let (r, mut s) = quarter_wave_by("bayesian", 5.0, 1, 1e-3);
    let (simplex, _) = quarter_wave_by("simplex", 5.0, 1, 1e-3);
    let (n, m) = (r["evaluations"].as_u64().unwrap(), simplex["evaluations"].as_u64().unwrap());
    eprintln!("quarter wave: Bayesian {n} solves, Nelder–Mead {m}; best {} against {}", r["best"]["values"][0], simplex["best"]["values"][0]);
    assert_eq!((r["method"].clone(), simplex["method"].clone()), (json!("bayesian"), json!("simplex")));
    assert!(simplex.get("surrogate").is_none());
    let best = r["best"]["values"][0].as_f64().unwrap();
    assert!((best - quarter).abs() < 0.03 * quarter, "{best} mm against λg/4 = {quarter} mm: {r}");
    assert!(r["best"]["objective"].as_f64().unwrap() < -20.0, "{r}");
    assert!(n <= m, "Bayesian {n} against Nelder–Mead {m}");
    // Every row a solve; the surrogate's own guess at the best point beside it.
    assert!(r["rows"].as_array().unwrap().iter().all(|row| row.get("predicted").is_none() && row["objective"].is_number()));
    let predicted = &r["best"]["predicted"];
    assert!(predicted["objective"].is_number() && predicted["sigma"].as_f64().is_some_and(|s| s >= 0.0), "{r}");
    // The model left at the optimum, as one step undo takes back.
    assert_eq!(s.variables["l"], best);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.variables["l"], 8.0);
    // Deterministic.
    assert_eq!(quarter_wave_by("bayesian", 5.0, 1, 1e-3).0, r);
}

/// The oracle case `study-optimize-quarter-wave` (second order on 3 mm) by both methods. The
/// goal, |S11| in dB, is a sharp notch that a stationary surrogate smooths over: the Bayesian
/// search lands within about 1 % of λg/4 (measured 11.238 mm in 10 solves at tolerance 1e-3, the
/// same in 15 at 1e-5) where the simplex, which needs no model, reaches 0.002 mm in 14.
#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn the_bayesian_optimizer_comes_near_the_simplex_on_the_oracle_case() {
    let (_, quarter) = quarter_wave();
    let (bayes, _) = quarter_wave_by("bayesian", 3.0, 2, 1e-3);
    let (simplex, _) = quarter_wave_by("simplex", 3.0, 2, 1e-3);
    let length = |r: &Value| r["best"]["values"][0].as_f64().unwrap_or(f64::NAN);
    let (n, m) = (bayes["evaluations"].as_u64().unwrap_or(0), simplex["evaluations"].as_u64().unwrap_or(0));
    eprintln!("oracle quarter wave: Bayesian {n} solves ({} mm), Nelder–Mead {m} ({} mm), λg/4 = {quarter} mm", length(&bayes), length(&simplex));
    assert!((length(&bayes) - quarter).abs() < 0.015 * quarter, "{bayes}");
    assert!(bayes["best"]["objective"].as_f64().unwrap_or(0.0) < -40.0, "{bayes}");
    assert!((length(&simplex) - quarter).abs() < 0.03, "{simplex}");
    assert!(n <= m + 5, "Bayesian {n} against Nelder–Mead {m}");
}

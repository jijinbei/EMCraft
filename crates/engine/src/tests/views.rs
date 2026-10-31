//! How results are looked at (chart styles, probes), more frequencies in a sweep, reference
//! planes moved without a solve, and the run's estimate.

use serde_json::json;

use super::scratch;
use crate::views::ChartId;
use crate::*;

/// The high-pass sample swept from 6 to 8 GHz at order 1.
fn swept(points: usize) -> Session {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    s.execute("solve.frequency", &json!({"start": 6, "stop": 8, "points": points, "order": 1})).unwrap();
    s
}

#[test]
fn chart_styles_apply_to_pngs_are_saved_and_refuse_bad_input() {
    let mut s = swept(3);
    let r = s.execute("view.chart.style", &json!({"chart": "sparameters"})).unwrap();
    let keys: Vec<&str> = r["curves"].as_array().unwrap().iter().filter_map(|c| c["key"].as_str()).collect();
    // The data only: the closed form is the guide's to add, not drawn unasked.
    assert!(keys.contains(&"s21.fem") && !keys.contains(&"s11.ref"), "{keys:?}");
    let optional: Vec<String> = views::optional_curves(&s, ChartId::SParameters).into_iter().map(|(k, _)| k).collect();
    assert_eq!(optional, ["s21.ref", "s11.ref"]);
    assert!(s.execute("view.chart.style", &json!({"chart": "sparameters", "added": ["nothing"]})).is_err());
    s.execute("view.chart.style", &json!({"chart": "sparameters", "added": ["s21.ref", "s11.ref"]})).unwrap();
    let with: Vec<String> = view::sparameter_chart(&s).unwrap().series.iter().map(|c| c.key.clone()).collect();
    assert!(with.iter().any(|k| k == "s11.ref"), "{with:?}");
    s.execute("view.chart.style", &json!({"chart": "sparameters", "reset": true})).unwrap();
    assert!(s.views().is_empty(), "asking changes nothing");
    s.mark_saved();
    let plain = scratch("style-plain.png");
    s.execute("view.sparameters", &json!({"path": plain})).unwrap();
    // A frequency axis is fixed in display units and kept in hertz.
    let set = json!({"chart": "sparameters", "x_min": 6.5, "x_max": 7.5, "y_min": -40, "title": "Mine", "hidden": ["s11.ref"],
                     "curve": "s21.fem", "colour": "#008000", "width": 6, "grid": false, "legend": "top-right"});
    let r = s.execute("view.chart.style", &set).unwrap();
    assert_eq!(r["style"]["x"]["min"], json!(6.5));
    assert_eq!(s.views().charts["sparameters"].x.min, Some(6.5e9));
    assert!(s.is_dirty(), "a style is the project's");
    let chart = views::style_chart(&s, ChartId::SParameters, view::sparameter_chart(&s).unwrap());
    assert_eq!((chart.x.min, chart.x.max, chart.y.min, chart.title.as_str()), (6.5, 7.5, -40.0, "Mine"));
    assert!(chart.series.iter().all(|c| c.key != "s11.ref"));
    assert!(!chart.look.grid);
    let styled = scratch("style-mine.png");
    s.execute("view.sparameters", &json!({"path": styled})).unwrap();
    assert_ne!(std::fs::read(&plain).unwrap(), std::fs::read(&styled).unwrap(), "the PNG follows the style");
    // In megahertz the fixed range reads in megahertz.
    s.execute("units.set", &json!({"frequency": "MHz"})).unwrap();
    assert_eq!(s.execute("view.chart.style", &json!({"chart": "sparameters"})).unwrap()["style"]["x"]["min"], json!(6500.0));
    s.execute("units.set", &json!({"frequency": "GHz"})).unwrap();
    // Hostile or wrong input is refused and the style stays.
    let before = s.views().clone();
    for bad in [
        json!({"chart": "nope"}),
        json!({"chart": "sparameters", "x_min": 9}),
        json!({"chart": "sparameters", "y_scale": "log"}),
        json!({"chart": "sparameters", "curve": "no.such", "colour": "#000000"}),
        json!({"chart": "sparameters", "colour": "#000000"}),
        json!({"chart": "sparameters", "curve": "s21.fem", "colour": "red"}),
        json!({"chart": "sparameters", "curve": "s21.fem", "width": 1e9}),
        json!({"chart": "sparameters", "x_max": "inf"}),
        json!({"chart": "sparameters", "title": "x".repeat(500)}),
        json!({"chart": "sparameters", "hidden": [1, 2]}),
        json!({"chart": "sparameters", "legend": "middle"}),
    ] {
        assert!(s.execute("view.chart.style", &bad).is_err(), "{bad}");
        assert_eq!(s.views(), &before);
    }
    // Saved with the project (version 4: its solves are kept too), opened again, clean; null clears, reset forgets.
    let dir = std::env::temp_dir().join(format!("emcraft-views-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("styled.emcraft").to_string_lossy().into_owned();
    s.execute("file.save", &json!({"path": path})).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(doc["version"], json!(4));
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": path})).unwrap();
    assert_eq!(t.views(), s.views());
    assert!(!t.is_dirty());
    t.execute("view.chart.style", &json!({"chart": "sparameters", "x_min": null, "x_max": null})).unwrap();
    assert_eq!(t.views().charts["sparameters"].x.min, None);
    t.execute("view.chart.style", &json!({"chart": "sparameters", "reset": true})).unwrap();
    assert!(t.views().charts.is_empty());
    // Undoing the open brings the views back with the project before it.
    t.execute("edit.undo", &json!({})).unwrap();
    assert!(t.views().is_empty());
    // A file's views are hostile too.
    for views in [
        json!({"charts": {"nope": {}}}),
        json!({"charts": {"tdr": {"x": {"min": "big"}}}}),
        json!({"charts": {"tdr": {"curves": {"a": {"colour": "blue"}}}}}),
        json!({"probes": [{"name": "a", "at": [0, 0, 0]}, {"name": "a", "at": [0, 0, 0]}]}),
        json!({"probes": [{"name": "", "at": [0, 0, 0]}]}),
    ] {
        let bad = dir.join("bad.emcraft");
        std::fs::write(&bad, json!({"format": "emcraft-project", "version": 3, "history": [], "views": views}).to_string()).unwrap();
        assert!(t.execute("file.open", &json!({"path": bad.to_string_lossy()})).is_err(), "{views}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn log_axes_and_other_charts_take_styles() {
    let mut s = swept(5);
    s.execute("view.chart.style", &json!({"chart": "sparameters", "x_scale": "log", "y_scale": "log"})).unwrap();
    // |S| in dB is negative: a log y axis takes the positive points' range, none here, so it
    // stays linear; the frequency axis is positive.
    let chart = views::style_chart(&s, ChartId::SParameters, view::sparameter_chart_as(&s, view::SDisplay::Magnitude).unwrap());
    assert!(chart.x.is_log() && chart.y.is_log());
    let png = scratch("style-log.png");
    s.execute("view.sparameters", &json!({"path": png, "display": "magnitude"})).unwrap();
    s.execute("view.chart.style", &json!({"chart": "tdr", "legend": "none", "y_label": "rho"})).unwrap();
    s.execute("view.tdr", &json!({"path": scratch("style-tdr.png")})).unwrap();
    assert!(s.execute("view.sparameters", &json!({"path": png, "display": "sideways"})).is_err());
}

#[test]
fn probes_are_named_moved_read_and_saved() {
    let mut s = swept(2);
    assert!(s.execute("view.probe.add", &json!({"at": [500, 0, 0]})).is_err(), "outside the model");
    let r = s.execute("view.probe.add", &json!({"at": [11.43, 5, 10]})).unwrap();
    assert_eq!(r["name"], "P1");
    s.execute("view.probe.add", &json!({"at": [5, 5, 30], "name": "near wall"})).unwrap();
    assert!(s.execute("view.probe.add", &json!({"at": [5, 5, 30], "name": "near wall"})).is_err(), "names are unique");
    assert_eq!(s.execute("view.probe.add", &json!({"at": [1, 1, 1]})).unwrap()["name"], "P2");
    s.execute("view.probe.delete", &json!({"name": "P2"})).unwrap();
    assert!(s.execute("view.probe.delete", &json!({"name": "P2"})).is_err());
    s.execute("view.probe.move", &json!({"name": "near wall", "at": [1, 5, 30], "rename": "wall"})).unwrap();
    assert_eq!(s.views().probes[1].name, "wall");
    assert!(s.is_dirty());
    let v = s.execute("results.probes", &json!({"frequency": 8})).unwrap();
    let rows = v["probes"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    // TE10: strongest across the middle, weak near the side wall.
    let (mid, wall) = (rows[0]["e_magnitude"].as_f64().unwrap(), rows[1]["e_magnitude"].as_f64().unwrap());
    assert!(mid > 2.0 * wall && wall > 0.0, "{mid} {wall}");
    // The same values as results.probe at those points.
    let one = s.execute("results.probe", &json!({"frequency": 8, "at": [[11.43, 5, 10]]})).unwrap();
    assert_eq!(one["points"][0]["e"], rows[0]["e"]);
    assert!(s.execute("results.probes", &json!({"mode": 1})).is_err(), "no eigenmodes");
}

#[test]
fn the_driven_port_is_the_default_excitation_and_saved() {
    let mut s = swept(2);
    let r = s.execute("view.driven", &json!({})).unwrap();
    assert_eq!(r["driven"], "1");
    assert_eq!(r["entries"], json!([{"name": "1", "role": "input"}, {"name": "2", "role": "output"}]));
    assert!(s.views().is_empty(), "the first is driven by default");
    let at = json!({"frequency": 8, "at": [[11.43, 5, 10]]});
    let first = s.execute("results.probe", &at).unwrap();
    let steps = s.history().len();
    let r = s.execute("view.driven", &json!({"port": "2"})).unwrap();
    assert_eq!((r["driven"].as_str(), r["entries"][0]["role"].as_str()), (Some("2"), Some("output")));
    assert_eq!((s.driven_entry(), s.driven_port()), (1, Some(1)));
    assert_eq!((s.views().driven.as_deref(), s.history().len()), (Some("2"), steps), "a view setting, kept out of the model's history");
    // The default excitation follows it; naming one still overrides.
    let second = s.execute("results.probe", &at).unwrap();
    let named = s.execute("results.probe", &json!({"frequency": 8, "at": [[11.43, 5, 10]], "excitation": "2"})).unwrap();
    assert_eq!(second["points"], named["points"]);
    assert_ne!(second["points"], first["points"]);
    assert!(s.execute("view.driven", &json!({"port": "9"})).is_err());
    assert!(s.execute("view.driven", &json!({"port": 2})).is_err());
    assert_eq!(s.driven_entry(), 1, "a refusal changes nothing");
    // Saved with the project and opened again.
    let dir = std::env::temp_dir().join(format!("emcraft-driven-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("driven.emcraft").to_string_lossy().into_owned();
    s.execute("file.save", &json!({"path": path})).unwrap();
    let mut t = Session::new();
    t.execute("file.open", &json!({"path": path})).unwrap();
    assert_eq!(t.views().driven.as_deref(), Some("2"));
    // A file's driven port is checked; one naming no port drives the first.
    let bad = dir.join("bad.emcraft");
    std::fs::write(&bad, json!({"format": "emcraft-project", "version": 3, "history": [], "views": {"driven": ""}}).to_string()).unwrap();
    assert!(t.execute("file.open", &json!({"path": bad.to_string_lossy()})).is_err());
    t.execute("view.driven", &json!({})).unwrap();
    assert_eq!(t.views().driven, None);
    let mut u = Session::new();
    u.views.driven = Some("gone".into());
    assert_eq!((u.driven_entry(), u.driven_port()), (0, None));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sweep_is_refined_and_recorded_as_one() {
    let mut s = swept(3);
    assert!(s.execute("solve.refine", &json!({"frequencies": []})).is_err());
    let r = s.execute("solve.refine", &json!({"start": 6.5, "stop": 7.5, "points": 3})).unwrap();
    // 7 GHz was solved already: two added.
    assert_eq!(r["added"], json!([6.5, 7.5]));
    let freqs: Vec<f64> = s.results().sweep.as_ref().unwrap().results.iter().map(|r| r.frequency / 1e9).collect();
    assert_eq!(freqs, vec![6.0, 6.5, 7.0, 7.5, 8.0]);
    let run = s.runs().last().unwrap();
    assert_eq!((run.command.as_str(), &run.params["frequencies"]), ("solve.frequency", &json!([6.0, 6.5, 7.0, 7.5, 8.0])));
    // A full sweep at those frequencies gives the same S.
    let merged = s.results().sweep.clone().unwrap();
    s.execute("solve.rerun", &json!({})).unwrap();
    let again = s.results().sweep.clone().unwrap();
    for (a, b) in merged.results.iter().zip(&again.results) {
        assert!((a.s[(1, 0)] - b.s[(1, 0)]).norm() < 1e-6, "{} GHz", a.frequency / 1e9);
    }
    // Not after the model changed.
    s.execute("project.frequency", &json!({"start": 5, "stop": 9})).unwrap();
    assert!(s.execute("solve.refine", &json!({"frequencies": [6.25]})).is_err());
    assert!(Session::new().execute("solve.refine", &json!({"frequencies": [6.25]})).is_err(), "no sweep");
}

#[test]
fn a_reference_plane_moves_without_a_solve() {
    let mut s = swept(3);
    let r = s.execute("model.port.deembed", &json!({"name": "1", "deembed": 5})).unwrap();
    assert_eq!(r["rerun"], json!(false));
    assert_eq!(s.solved_revision(), Some(s.revision()), "the results stay current");
    let moved = s.results().sweep.clone().unwrap();
    // The same as solving with the plane there.
    let mut t = Session::new();
    t.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    t.execute("model.port.edit", &json!({"name": "1", "deembed": 5})).unwrap();
    t.execute("solve.frequency", &json!({"start": 6, "stop": 8, "points": 3, "order": 1})).unwrap();
    let solved = t.results().sweep.clone().unwrap();
    for (a, b) in moved.results.iter().zip(&solved.results) {
        for (p, q) in [(0, 0), (1, 0), (1, 1)] {
            assert!((a.s[(p, q)] - b.s[(p, q)]).norm() < 1e-9, "S{}{} at {}", p + 1, q + 1, a.frequency);
        }
    }
    // |S| stays, the phase moves; more frequencies follow the moved plane.
    s.execute("solve.refine", &json!({"frequencies": [7.5]})).unwrap();
    t.execute("solve.refine", &json!({"frequencies": [7.5]})).unwrap();
    let (a, b) = (
        s.results().sweep.as_ref().unwrap().at(7.5e9, false).unwrap().s[(0, 0)],
        t.results().sweep.as_ref().unwrap().at(7.5e9, false).unwrap().s[(0, 0)],
    );
    assert!((a - b).norm() < 1e-9);
    // Undo puts the plane back; the results then are stale.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.model().ports[0].deembed, 0.0);
    assert!(s.execute("model.port.deembed", &json!({"name": "9", "deembed": 1})).is_err());
    assert!(s.execute("model.port.deembed", &json!({"name": "1", "deembed": "x"})).is_err());
}

#[test]
fn the_run_is_estimated_from_its_settings() {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    s.execute("mesh.generate", &json!({"size": 4})).unwrap();
    let est = |s: &mut Session, command: &str, params: serde_json::Value| {
        s.execute("project.estimate", &json!({"command": command, "params": params})).unwrap()
    };
    let sweep = est(&mut s, "solve.frequency", json!({"points": 11, "order": 1}));
    assert_eq!((sweep["basis"].as_str(), sweep["solves"].as_f64()), (Some("reference"), Some(5.0)));
    let full = est(&mut s, "solve.frequency", json!({"points": 11, "order": 1, "sweep": "full"}));
    assert!(full["seconds"].as_f64() > sweep["seconds"].as_f64());
    let order2 = est(&mut s, "solve.frequency", json!({"points": 11}));
    assert!(order2["unknowns"].as_f64() > sweep["unknowns"].as_f64());
    let eigen = est(&mut s, "solve.eigenmodes", json!({"near": 7, "count": 10, "order": 1}));
    assert!(eigen["seconds"].as_f64().unwrap() > sweep["seconds_per_frequency"].as_f64().unwrap());
    let adapt = est(&mut s, "mesh.adapt", json!({"passes": 4, "order": 1}));
    assert_eq!(adapt["solves"], json!(4.0));
    let td = est(&mut s, "solve.time_domain", json!({"cell": 1, "start": 6, "stop": 8}));
    assert!(td["seconds"].as_f64().unwrap() > 0.0 && td.get("unknowns").is_none());
    let study =
        est(&mut s, "study.sweep", json!({"values": [1, 2, 3], "solve": {"command": "solve.frequency", "params": {"points": 11, "order": 1}}}));
    assert!((study["seconds"].as_f64().unwrap() / sweep["seconds"].as_f64().unwrap() - 3.0).abs() < 1e-9);
    // The latest run by default; after a sweep, this machine's numbers.
    s.execute("solve.frequency", &json!({"start": 6, "stop": 8, "points": 3, "order": 1})).unwrap();
    let latest = s.execute("project.estimate", &json!({})).unwrap();
    assert_eq!((latest["command"].as_str(), latest["basis"].as_str()), (Some("solve.frequency"), Some("measured")));
    assert_eq!(s.execute("project.estimate", &json!({})).unwrap(), latest, "deterministic");
    for bad in [
        json!({"command": "file.open"}),
        json!({"command": "solve.frequency", "params": 3}),
        json!({"command": "solve.frequency", "params": {"points": -1}}),
    ] {
        assert!(s.execute("project.estimate", &bad).is_err(), "{bad}");
    }
}

#[test]
fn each_point_says_whether_it_was_solved_in_full() {
    use emcraft_render::chart::Style;
    let flags = |r: &serde_json::Value| r["points"].as_array().unwrap().iter().map(|p| p["full"].as_bool().unwrap()).collect::<Vec<bool>>();
    // A fast sweep: the band's ends in full, most of the rest in the basis.
    let mut s = swept(9);
    let fast = flags(&s.execute("results.sparameters", &json!({})).unwrap());
    let counted = fast.iter().filter(|f| **f).count();
    assert!(fast[0] && fast[8] && counted < 9, "{fast:?}");
    // Its curves are lines, the full solves marked on them and named once in the legend.
    let chart = view::sparameter_chart(&s).unwrap();
    let s21: Vec<_> = chart.series.iter().filter(|c| c.key == "s21.fem").collect();
    assert_eq!(s21.iter().map(|c| c.style).collect::<Vec<_>>(), [Style::Line, Style::Markers]);
    assert_eq!((s21[0].points.len(), s21[1].points.len(), s21[1].label.as_str()), (9, counted, ""));
    assert_eq!(chart.series.iter().filter(|c| c.key == view::FULL_SOLVES).count(), 1);
    // A full sweep: every point in full, every point marked as before, no legend entry.
    let r = s.execute("solve.frequency", &json!({"start": 6, "stop": 8, "points": 9, "order": 1, "sweep": "full"})).unwrap();
    assert_eq!((flags(&r), r["solved_in_full"].as_u64()), (vec![true; 9], Some(9)));
    let chart = view::sparameter_chart(&s).unwrap();
    assert!(chart.series.iter().all(|c| c.key != view::FULL_SOLVES && (c.key != "s21.fem" || c.style == Style::Joined)));
    // Refined: the point added says how it was solved, the others keep theirs.
    let mut s = swept(9);
    let r = s.execute("solve.refine", &json!({"frequencies": [6.1]})).unwrap();
    assert_eq!(r["solved_in_full"], json!(1));
    let mut after = fast.clone();
    after.insert(1, true);
    assert_eq!(flags(&r), after);
}

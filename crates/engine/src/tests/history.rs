//! The analysis history in the project file: saved and read back as it was, restored by
//! solving again, older files without it, hostile ones refused.

use serde_json::{Value, json};

use super::scratch;
use crate::*;

/// The highpass sample solved twice (the gap 20 mm, then 26 mm) on a coarse sweep, the first
/// pinned and the second labelled.
fn solved_twice() -> Session {
    let mut s = Session::new();
    s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
    let solve = json!({"start": 6.6, "stop": 9, "points": 3, "order": 1});
    s.execute("solve.frequency", &solve).unwrap();
    s.execute("project.variable", &json!({"name": "l_gap", "value": 26})).unwrap();
    s.execute("solve.frequency", &solve).unwrap();
    s.execute("results.history.pin", &json!({"id": 1})).unwrap();
    s.execute("results.history.label", &json!({"id": 2, "label": "  wider gap "})).unwrap();
    s
}

/// `results.history`'s rows without `current` (nothing is current in a project just opened:
/// its results are not loaded until a solve is restored).
fn rows(s: &mut Session) -> Vec<Value> {
    let h = s.execute("results.history", &json!({})).unwrap();
    h["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let mut r = r.clone();
            r.as_object_mut().unwrap().remove("current");
            r
        })
        .collect()
}

#[test]
fn the_analysis_history_survives_saving_and_reopening() {
    let mut s = solved_twice();
    let before = rows(&mut s);
    assert_eq!((before[0]["pinned"].as_bool(), before[1]["label"].as_str()), (Some(true), Some("wider gap")));
    let path = scratch("history.emcraft");
    let r = s.execute("file.save", &json!({"path": path})).unwrap();
    assert_eq!(r["kept"], 2);
    assert!(!s.is_dirty());
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(doc["version"], 4);
    let h = &doc["analysis_history"];
    // Two models, each history written once; the S-parameters, not the fields.
    assert_eq!((h["next"].as_u64(), h["histories"].as_array().map(Vec::len)), (Some(2), Some(2)));
    let sweep = &h["solves"][0]["results"]["sweep"];
    assert_eq!(sweep["frequencies"].as_array().map(Vec::len), Some(3));
    assert_eq!(sweep["s"][0].as_array().map(Vec::len), Some(8));

    // Opened: the same rows, number for number; clean; nothing current.
    let mut t = Session::new();
    let r = t.execute("file.open", &json!({"path": path})).unwrap();
    assert_eq!((r["kept"].as_u64(), r["dirty"].as_bool()), (Some(2), Some(false)));
    assert_eq!(rows(&mut t), before);
    assert!(t.archive().iter().all(|a| !a.in_memory()));
    // Compared before anything is solved: the kept sweeps alone.
    let chart = crate::view::sparameter_chart_compared(&t, crate::view::SDisplay::Db, &[1, 2]).unwrap();
    assert_eq!(chart.series.len(), 4);
    assert!(chart.x.min.is_finite() && chart.x.min < chart.x.max);
    assert!(crate::view::sparameter_chart_compared(&t, crate::view::SDisplay::Db, &[]).is_err());

    // Restored: rebuilt from its history and solved again, the same results bit for bit.
    let r = t.execute("results.history.restore", &json!({"id": 1})).unwrap();
    assert_eq!((r["solved_again"].as_bool(), r["same_as_saved"].as_bool()), (Some(true), Some(true)));
    assert_eq!(t.variables().get("l_gap"), Some(&20.0));
    assert!(t.results().sweep.is_some() && t.solved_revision() == Some(t.revision()));
    assert_eq!(rows(&mut t), before);
    assert!(t.archive()[0].in_memory());
    // Only the model: nothing solved, nothing current.
    let r = t.execute("results.history.restore", &json!({"id": 2, "solve": false})).unwrap();
    assert!(r.get("solved_again").is_none());
    assert_eq!(t.variables().get("l_gap"), Some(&26.0));
    assert!(t.results().sweep.is_none() && !t.archive()[1].in_memory());
    // Undo goes back to the first.
    t.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(t.variables().get("l_gap"), Some(&20.0));

    // A new solve keeps the next id; it is unsaved.
    let mut u = Session::new();
    u.execute("file.open", &json!({"path": path})).unwrap();
    u.execute("solve.frequency", &json!({"frequencies": [8], "order": 1})).unwrap();
    assert_eq!(u.archive().last().map(|a| a.id), Some(3));
    assert!(u.is_dirty());
}

/// Saving marks nothing: the history's pins, labels and deletions are unsaved changes like
/// any other; a copy leaves the saved state alone and recovering it keeps the history.
#[test]
fn the_history_is_part_of_the_unsaved_state() {
    let mut s = solved_twice();
    assert!(s.is_dirty());
    let (own, copy) = (scratch("history-own.emcraft"), scratch("history-copy.emcraft"));
    s.execute("file.save", &json!({"path": own})).unwrap();
    assert!(!s.is_dirty());
    // The same solve again is the one kept: nothing new, nothing unsaved.
    s.execute("solve.frequency", &json!({"start": 6.6, "stop": 9, "points": 3, "order": 1})).unwrap();
    assert_eq!((s.archive().len(), s.archive_current, s.is_dirty()), (2, Some(2), false));
    s.execute("results.history.pin", &json!({"id": 2})).unwrap();
    assert!(s.is_dirty());
    s.execute("results.history.pin", &json!({"id": 2, "pinned": false})).unwrap();
    assert!(!s.is_dirty());
    s.execute("results.history.label", &json!({"id": 1, "label": "first"})).unwrap();
    assert!(s.is_dirty());
    s.execute("results.history.label", &json!({"id": 1, "label": ""})).unwrap();
    assert!(!s.is_dirty());
    s.execute("results.history.delete", &json!({"id": 1})).unwrap();
    assert!(s.is_dirty());
    // An autosave: the copy holds the history as it is now; the session stays unsaved.
    s.execute("file.save", &json!({"path": copy, "copy": true})).unwrap();
    assert!(s.is_dirty());
    let mut t = Session::new();
    let r = t.execute("file.open", &json!({"path": copy, "original": own})).unwrap();
    assert_eq!((r["kept"].as_u64(), r["dirty"].as_bool()), (Some(1), Some(true)));
    assert_eq!(t.path(), Some(own.as_str()));
    // Undo of the open: the session before it, its own history.
    let mut u = solved_twice();
    u.execute("file.open", &json!({"path": copy})).unwrap();
    assert_eq!(u.archive().len(), 1);
    u.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(u.archive().len(), 2);
    assert!(u.archive().iter().all(|a| a.in_memory()));
    // Labels and pins are checked.
    assert!(u.execute("results.history.label", &json!({"id": 1, "label": "x".repeat(81)})).is_err());
    assert!(u.execute("results.history.label", &json!({"id": 1, "label": "a\u{7}"})).is_err());
    assert!(u.execute("results.history.label", &json!({"id": 9, "label": "a"})).is_err());
}

#[test]
fn files_without_a_history_open_as_before_and_newer_ones_are_refused() {
    let path = scratch("history-old.emcraft");
    let mut s = Session::new();
    // A project with nothing kept is written in the version it was before.
    s.execute("project.frequency", &json!({"start": 5, "stop": 9})).unwrap();
    s.execute("file.save", &json!({"path": path})).unwrap();
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(doc["version"], 2);
    assert!(doc.get("analysis_history").is_none());
    // A version 3 file opens with an empty history, whatever it holds besides.
    let mut doc = doc;
    doc["version"] = json!(3);
    doc["analysis_history"] = json!({"next": 1, "histories": [], "solves": "not read"});
    std::fs::write(&path, doc.to_string()).unwrap();
    s.execute("file.open", &json!({"path": path})).unwrap();
    assert!(s.archive().is_empty() && !s.is_dirty());
    doc["version"] = json!(5);
    std::fs::write(&path, doc.to_string()).unwrap();
    assert!(matches!(s.execute("file.open", &json!({"path": path})), Err(EngineError::Failed(m)) if m.contains("newer")));
}

/// A saved history's text, each value of it spoiled in turn: refused with a reason, or read
/// when the change is harmless, and nothing panics.
#[test]
fn hostile_histories_in_a_file_are_refused_and_nothing_panics() {
    let mut s = solved_twice();
    let good = scratch("history-good.emcraft");
    s.execute("file.save", &json!({"path": good})).unwrap();
    let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(&good).unwrap()).unwrap();
    // The project's own model left out: opening replays nothing, so each try is quick.
    doc["history"] = json!([]);
    let path = scratch("history-hostile.emcraft");
    let mut t = Session::new();
    let open = |t: &mut Session, d: &Value| {
        std::fs::write(&path, d.to_string()).unwrap();
        t.execute("file.open", &json!({"path": path}))
    };
    let solve = |d: &mut Value| d["analysis_history"]["solves"][0].clone();
    let one = solve(&mut doc.clone());
    let many = |n: usize, pinned: bool| {
        let list: Vec<Value> = (1..=n)
            .map(|i| {
                let mut x = one.clone();
                x["id"] = json!(i);
                x["pinned"] = json!(pinned);
                x
            })
            .collect();
        json!(list)
    };
    let cases: Vec<(&str, Value, &str)> = vec![
        ("/analysis_history/solves", many(archive::MAX_KEPT + archive::MAX_PINNED + 1, false), "at most"),
        ("/analysis_history/solves", many(archive::MAX_PINNED + 1, true), "at most"),
        ("/analysis_history/solves/1/id", json!(1), "ascend"),
        ("/analysis_history/solves/0/id", json!(0), "ascend"),
        ("/analysis_history/next", json!(1), "ascend"),
        ("/analysis_history/solves/0/command", json!("file.save"), "not a solve command"),
        ("/analysis_history/solves/0/params", json!({"path": "/tmp/x"}), "unknown parameter"),
        ("/analysis_history/solves/0/params", json!([1]), "JSON object"),
        ("/analysis_history/solves/0/history", json!(7), "names no history"),
        ("/analysis_history/histories/0/0", json!({"command": "file.save", "params": {"path": "/tmp/x"}}), "a command a history holds"),
        ("/analysis_history/histories/0/0", json!({"command": "file.open", "params": {"path": "/tmp/x"}}), "a command a history holds"),
        ("/analysis_history/solves/0/variables", json!({"1x": 2.0}), "valid name"),
        ("/analysis_history/solves/1/label", json!("x".repeat(500)), "label"),
        ("/analysis_history/solves/0/results/sweep/s/0", json!([1.0, 2.0]), "numbers, not"),
        ("/analysis_history/solves/0/results/sweep/s", json!([]), "S matrices"),
        ("/analysis_history/solves/0/results/sweep/frequencies/0", json!(-6.6e9), "positive"),
        ("/analysis_history/solves/0/results/sweep/frequencies/0", json!(9.9e9), "ascending"),
        ("/analysis_history/solves/0/results/sweep/order", json!(7), "order"),
        ("/analysis_history/solves/0/results/sweep/entries", json!([]), "entries"),
        ("/analysis_history/solves/0/results/sweep/entries/0/reference", json!(-50.0), "reference"),
        ("/analysis_history/solves/0/results/eigenmodes", json!([{"frequency": 0.0}]), "eigenmode"),
        ("/analysis_history/solves/0/results/time_domain", json!({"frequencies": [1e9], "s11": [1], "s21": [1, 0]}), "time domain"),
        ("/analysis_history/solves/0/results/sweep/frequencies", json!(vec![1.0; 3000]), "frequencies"),
        ("/analysis_history/solves/0/units", json!({"length": "furlong", "frequency": "GHz"}), "not a project file"),
        ("/analysis_history/solves", json!("x"), "not a project file"),
    ];
    for (at, value, why) in cases {
        let mut d = doc.clone();
        put(&mut d, at, value);
        let r = open(&mut t, &d);
        assert!(matches!(&r, Err(EngineError::Failed(m)) if m.contains(why)), "{at} ({why}): {r:?}");
    }
    // Every leaf of the history replaced in turn by hostile values: refused or read, never a
    // panic, and what is read stays within the caps.
    let mut leaves = Vec::new();
    collect_leaves(&doc["analysis_history"], "/analysis_history".to_owned(), &mut leaves);
    let hostile = [json!(null), json!(-1), json!(1e308), json!(u64::MAX), json!("=1/0"), json!([[[]]]), json!({"a": {}}), json!(f64::MIN_POSITIVE)];
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    for (i, at) in leaves.iter().enumerate() {
        // A spread of the leaves (every one in the first solve's identity), each with a value
        // chosen by a fixed sequence: deterministic, and fast.
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        if i % 7 != 0 && !at.contains("/solves/0/") {
            continue;
        }
        let mut d = doc.clone();
        if let Some(v) = d.pointer_mut(at) {
            *v = hostile[(seed % hostile.len() as u64) as usize].clone();
        }
        match open(&mut t, &d) {
            Ok(_) => assert!(t.archive().len() <= archive::MAX_KEPT + archive::MAX_PINNED),
            Err(e) => assert!(!matches!(e, EngineError::Panicked(_)), "{at}: {e}"),
        }
    }
}

/// `value` at pointer `at` of `doc`, the last key added to its object when it is not there.
fn put(doc: &mut Value, at: &str, value: Value) {
    let (parent, key) = at.rsplit_once('/').unwrap();
    match doc.pointer_mut(parent).unwrap_or_else(|| panic!("{at}")) {
        Value::Object(o) => {
            o.insert(key.to_owned(), value);
        }
        Value::Array(a) => a[key.parse::<usize>().unwrap()] = value,
        _ => panic!("{at}"),
    }
}

fn collect_leaves(v: &Value, at: String, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => o.iter().for_each(|(k, x)| collect_leaves(x, format!("{at}/{k}"), out)),
        Value::Array(a) => a.iter().enumerate().for_each(|(k, x)| collect_leaves(x, format!("{at}/{k}"), out)),
        _ => out.push(at),
    }
}

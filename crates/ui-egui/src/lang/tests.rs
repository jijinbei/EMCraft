//! The translations stay whole: every command and sample translated, the placeholders kept,
//! and a tour of the window in Japanese that finds no English left and no translation unused.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::*;
use crate::scenario::Harness;

fn japanese() -> &'static Language {
    find("ja").unwrap_or_else(|| panic!("no Japanese: {:?}", problems()))
}

#[test]
fn the_built_in_languages_load() {
    assert_eq!(problems(), &[] as &[String]);
    assert_eq!(ids(), ["en", "ja"]);
    assert_eq!(japanese().name, "日本語");
}

#[test]
fn every_command_and_sample_is_translated_and_nothing_else() {
    let ja = japanese();
    for c in emcraft_engine::COMMANDS {
        let t = ja.commands.get(c.id).unwrap_or_else(|| panic!("{}: no Japanese", c.id));
        assert!(!t.label.is_empty() && !t.doc.is_empty(), "{}: label and doc", c.id);
        for p in c.params {
            assert!(t.params.get(p.name).is_some_and(|d| !d.is_empty()), "{} › {}: no Japanese", c.id, p.name);
        }
        for name in t.params.keys() {
            assert!(c.params.iter().any(|p| p.name == name), "{} has no parameter `{name}`", c.id);
        }
    }
    for id in ja.commands.keys() {
        assert!(emcraft_engine::find_command(id).is_some(), "no command `{id}`");
    }
    for s in emcraft_engine::templates::TEMPLATES {
        assert!(ja.samples.get(s.name).is_some_and(|l| !l.is_empty()), "sample {}: no Japanese", s.name);
    }
    for name in ja.samples.keys() {
        assert!(emcraft_engine::templates::find(name).is_some(), "no sample `{name}`");
    }
}

#[test]
fn translations_keep_their_placeholders() {
    fn sorted(t: &str) -> BTreeSet<&str> {
        placeholders(t).into_iter().collect()
    }
    for (english, text) in &japanese().strings {
        assert_eq!(sorted(english), sorted(text), "`{english}` → `{text}`");
        assert!(!text.trim().is_empty(), "`{english}`: empty");
    }
}

#[test]
fn placeholders_are_filled_once_and_braces_kept() {
    assert_eq!(fill("{a} and {b}", &[("a", &1), ("b", &"{a}")]), "1 and {a}");
    assert_eq!(fill("{x} {unknown} {", &[("x", &2.5)]), "2.5 {unknown} {");
    assert_eq!(placeholders("Run, accepting {n} warning(s) {not a name}"), ["n"]);
    assert!(is_text("{command} done.") && !is_text("{command}…") && !is_text("× 3"));
}

#[test]
fn the_language_is_the_threads_own() {
    assert!(set("ja"));
    assert_eq!((current(), tr("Settings")), ("ja", "設定"));
    std::thread::spawn(|| assert_eq!((current(), tr("Settings")), ("en", "Settings"))).join().unwrap();
    assert_eq!(command_label("mesh.generate"), japanese().commands["mesh.generate"].label);
    assert!(!set("klingon"));
    assert_eq!(Some(command_label("mesh.generate")), emcraft_engine::find_command("mesh.generate").map(|c| c.label));
    assert_eq!(current(), "en");
    assert_eq!(param_doc("no.such", "x"), "");
}

/// The models the tour draws, each with results of a kind (coarse, to keep it fast): the
/// commands run after each one's script (one by one: the views keep their replies), and the
/// steps visited (all when none are named).
type Model = (&'static str, &'static str, &'static [&'static str]);

const MODELS: &[Model] = &[
    // Nothing yet: every view says what it needs.
    ("[]", "[]", &[]),
    // A guide with a port and no band: the steps' errors that ask for one; a failed solve.
    (
        r#"[{"command": "geometry.waveguide", "params": {"width": 22.86, "height": 10.16, "sections": [{"length": 10, "material": "vacuum", "width": 20}]}},
            {"command": "model.symmetry", "params": {"axis": "y", "kind": "electric"}},
            {"command": "mesh.generate", "params": {"size": 8}},
            {"command": "model.port.create", "params": {"name": "1", "end": "start", "modes": 1}}]"#,
        r#"[{"command": "solve.frequency", "params": {"points": 3}}]"#,
        &[],
    ),
    // A layered guide with strips and a via (not meshed).
    (
        r#"[{"command": "model.material.create", "params": {"name": "fill", "eps_r": 2.2}},
            {"command": "geometry.waveguide", "params": {"width": 20, "height": 2, "sections": [{"length": 30, "material": "fill"}],
              "layers": [{"thickness": 0.5, "material": "fill"}],
              "strips": [{"y": 1, "x0": 3, "width": 2, "z0": 0, "z1": 30}], "vias": [{"x0": 14, "z0": 14, "wx": 1, "wz": 2, "y0": 0, "y1": 1}]}}]"#,
        "[]",
        &["geometry"],
    ),
    // A filter: S-parameters, fields, port modes, the comparison with theory, TDR, studies.
    (
        r#"[{"command": "project.variable", "params": {"name": "l", "value": 20}},
            {"command": "model.material.create", "params": {"name": "ptfe-like", "eps_r": 2.0}},
            {"command": "geometry.waveguide", "params": {"width": 22.86, "height": 10.16, "sections": [
              {"length": 15, "material": "ptfe-like"}, {"length": "=l", "material": "vacuum"}, {"length": 15, "material": "ptfe-like"}]}},
            {"command": "project.frequency", "params": {"start": 7, "stop": 9}},
            {"command": "model.symmetry", "params": {"axis": "x", "kind": "magnetic"}},
            {"command": "mesh.generate", "params": {"size": 8}},
            {"command": "model.port.create", "params": {"name": "1", "end": "start", "modes": 1}},
            {"command": "model.port.create", "params": {"name": "2", "end": "end", "modes": 2}},
            {"command": "solve.frequency", "params": {"start": 7, "stop": 9, "points": 3, "order": 1}}]"#,
        r#"[{"command": "solve.port_modes", "params": {"frequency": 9, "order": 1}},
            {"command": "view.probe.add", "params": {"at": [5, 5, 10]}},
            {"command": "results.compare", "params": {}},
            {"command": "results.check", "params": {}}]"#,
        &[],
    ),
    // The filter optimized.
    (
        "[]",
        r#"[{"command": "study.optimize", "params": {"variables": [{"name": "l", "min": 18, "max": 22}], "goal": {"type": "min_max_db", "parameter": "s11"},
              "solve": {"command": "solve.frequency", "params": {"frequencies": [8], "order": 1}}, "max_evaluations": 2}}]"#,
        &["run", "results"],
    ),
    // The filter swept: one variable (a point that fails), then on a grid.
    (
        "[]",
        r#"[{"command": "project.variable", "params": {"name": "w", "value": 20}},
            {"command": "study.sweep", "params": {"variable": "l", "values": [-5, 20], "solve": {"command": "solve.frequency", "params": {"frequencies": [8], "order": 1}}}},
            {"command": "study.sweep", "params": {"variables": [{"name": "l", "values": [18, 22]}, {"name": "w", "values": [20]}],
              "solve": {"command": "solve.frequency", "params": {"frequencies": [8], "order": 1}}}}]"#,
        &["run", "results"],
    ),
    // A cavity with an electric wall: eigenmodes.
    (
        r#"[{"command": "geometry.waveguide", "params": {"width": 22.86, "height": 10.16, "sections": [{"length": 30, "material": "vacuum"}]}},
            {"command": "model.walls", "params": {"conductivity": 5.8e7}},
            {"command": "mesh.generate", "params": {"size": 8}},
            {"command": "solve.eigenmodes", "params": {"near": 12, "count": 2, "order": 1}}]"#,
        "[]",
        &["boundaries", "setup", "run", "results"],
    ),
    // An open box with a dipole: radiation and the far field.
    (
        r#"[{"command": "geometry.solid", "params": {"shape": "box", "size": [60, 60, 60]}},
            {"command": "model.open", "params": {"thickness": 15}},
            {"command": "mesh.generate", "params": {"size": 15}},
            {"command": "source.dipole", "params": {"at": [30.1, 30.2, 29.9], "direction": [0, 0, 1], "length": 1, "current": 1}},
            {"command": "model.port.lumped", "params": {"name": "L1", "from": [30, 30, 25], "to": [30, 30, 35], "impedance": 50}},
            {"command": "source.plane_wave", "params": {"direction": [0, 0, 1], "polarization": [1, 0, 0]}},
            {"command": "project.frequency", "params": {"start": 9, "stop": 11}},
            {"command": "solve.radiation", "params": {"frequency": 10, "order": 1}}]"#,
        "[]",
        &["intent", "excitation", "boundaries", "run"],
    ),
    // A sphere in a spherical layer (not meshed).
    (
        r#"[{"command": "geometry.solid", "params": {"shape": "sphere", "radius": 30}}, {"command": "model.open", "params": {"thickness": 10, "shape": "sphere"}}]"#,
        "[]",
        &["boundaries", "run"],
    ),
    // A periodic guide: the dispersion diagram.
    (
        r#"[{"command": "geometry.waveguide", "params": {"width": 22.86, "height": 10.16, "sections": [{"length": 10, "material": "vacuum"}]}},
            {"command": "mesh.generate", "params": {"size": 5}},
            {"command": "model.periodic", "params": {"x": false, "y": false, "z": true}}, {"command": "project.frequency", "params": {"start": 5, "stop": 6}},
            {"command": "solve.dispersion", "params": {"from": 0, "to": 180, "points": 3, "bands": 1, "near": 15, "order": 1}}]"#,
        "[]",
        &["boundaries", "run"],
    ),
];

/// `request` over the control channel, which must succeed.
fn call(h: &mut Harness, method: &str, params: Value) -> Value {
    let r = h.call(method, params.clone());
    assert_eq!(r["ok"], json!(true), "{method} {params}: {r}");
    r["result"].clone()
}

/// A scenario's step; what it does not find on screen is left out.
fn step(h: &mut Harness, s: Value) {
    let _ = h.step(&s);
    h.frames(2);
}

/// The app performs `actions` as a frame asks for them.
fn apply<const N: usize>(h: &mut Harness, actions: [crate::Action; N]) {
    let mut out = crate::Actions::default();
    out.extend(actions);
    h.app.apply_all(out);
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

/// Every screen of the window, in Japanese, with what was looked up while it was drawn.
fn tour() -> Record {
    use crate::files::{FileAction, Prompt, Purpose, Then};
    use crate::steps::{Solver, Step};
    use crate::{Action, View};
    let mut h = Harness::new();
    call(&mut h, "ui.set", json!({"language": "ja"}));
    record(true);
    // The 3D view drawn by egui's meshes, not the CPU's pictures: the words are the same.
    h.app.prefs.gpu3d = true;
    h.app.info_with("Open a sample or a project (File) or build a model from the ribbon.", &[]);
    // The start page, empty and with a project missing from the recent list, the settings
    // (Files with no recent project too), the prompts.
    h.app.home.open = true;
    call(&mut h, "ui.set", json!({"settings": "files"}));
    h.frames(2);
    call(&mut h, "ui.set", json!({"settings": "none"}));
    h.app.prefs.recent = vec!["/no/such/project.emcraft".into()];
    step(&mut h, json!({"click": "home:recent:0", "button": "right"}));
    step(&mut h, json!({"key": "Escape"}));
    for s in crate::settings::Section::ALL {
        call(&mut h, "ui.set", json!({"settings": s.id()}));
        h.frames(2);
    }
    call(&mut h, "ui.set", json!({"settings": "none"}));
    for purpose in [Purpose::Open, Purpose::SaveAs] {
        h.app.files.prompt = Some(Prompt { purpose, text: String::new() });
        h.frames(2);
    }
    h.app.files.prompt = None;
    h.app.files.confirm = Some(Then::New);
    h.frames(2);
    h.app.files.confirm = None;
    // Autosaved copies to recover, of every age, of a saved project and of one never saved.
    let now = std::time::SystemTime::now();
    h.app.autosave.found = [0, 300, 7200, 200_000]
        .into_iter()
        .enumerate()
        .map(|(i, secs)| crate::autosave::Found {
            file: format!("/no/such/{i}.emcraft").into(),
            name: format!("p{i}"),
            original: (i % 2 == 0).then(|| "/no/such/p.emcraft".to_owned()),
            modified: now.checked_sub(std::time::Duration::from_secs(secs)),
        })
        .collect();
    h.frames(2);
    h.app.autosave.found.clear();
    h.app.home.open = false;
    // Each model: its views, its steps (each solver in Setup), the menus, the search.
    for (i, &(script, then, steps)) in MODELS.iter().enumerate() {
        if script.len() > 2 {
            call(&mut h, "engine.execute", json!({"command": "file.new"}));
            call(&mut h, "engine.script", json!({"commands": json(script)}));
        }
        for c in json(then).as_array().into_iter().flatten() {
            let _ = h.call("engine.execute", json!({"command": c["command"], "params": c["params"]}));
        }
        let _ = crate::status::text("file.sample", &json!({"name": "highpass"}), &json!({}), &h.app.session);
        h.app.workflow.fields.solver = None;
        let views: Vec<View> = View::ALL.into_iter().filter(|v| i == 0 || v.available(&h.app)).collect();
        for v in views {
            call(&mut h, "ui.set", json!({"view": v.id(), "step": "none"}));
            h.frames(3);
            // A chart's style card, and a band of the S-parameters dragged to refine.
            if crate::scenario::handle_at("chart:style").is_some() {
                step(&mut h, json!({"click": "chart:style"}));
                step(&mut h, json!({"click": "chart:style"}));
            }
            if crate::scenario::handle_at("chart:sparameters").is_some() {
                step(
                    &mut h,
                    json!({"drag": [{"handle": "chart:sparameters", "offset": [-60, 0]}, {"handle": "chart:sparameters", "offset": [40, 0]}]}),
                );
            }
            for at in ["picture:field", "chart:sparameters"] {
                if crate::scenario::handle_at(at).is_some() {
                    step(&mut h, json!({"click": at}));
                    step(&mut h, json!({"move": {"handle": at, "offset": [20, 10]}}));
                    step(&mut h, json!({"click": "field:keep"}));
                    step(&mut h, json!({"click": "field:plane:e"}));
                    step(&mut h, json!({"click": "field:keep"}));
                    step(&mut h, json!({"click": "field:phase:play"}));
                }
            }
        }
        if i == 3 {
            // A job running: its progress, a dialog that waits, Run's panel, a refused request.
            call(
                &mut h,
                "engine.execute",
                json!({"command": "solve.frequency", "params": {"start": 7, "stop": 9, "points": 101, "order": 2}, "wait": false}),
            );
            call(&mut h, "ui.start", json!({"command": "mesh.generate"}));
            call(&mut h, "ui.set", json!({"step": "run"}));
            // Until the job says how far it is (bounded).
            for _ in 0..500 {
                h.frames(1);
                if h.app.job.as_ref().is_none_or(|j| j.snapshot().total > 0) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            h.frames(1);
            assert!(h.app.busy(), "the job ended before the tour saw it run");
            // Closing while it runs.
            h.app.files.confirm = Some(Then::Quit);
            h.frames(2);
            h.app.files.confirm = None;
            apply(&mut h, [Action::File(FileAction::Request(Then::New))]);
            call(&mut h, "ui.cancel", json!({}));
            call(&mut h, "ui.set", json!({"step": "none"}));
        }
        call(&mut h, "ui.set", json!({"view": "model"}));
        let picked = crate::browser::tree(&h.app)
            .into_iter()
            .find_map(|(s, rows)| rows.into_iter().find(|r| r.pick.is_some()).map(|r| (format!("{s}/{}", r.text), r.pick)));
        if let Some((row, pick)) = picked {
            (h.app.browser.selected, h.app.view3d.selected) = (Some(row), pick);
        }
        // A row's right-click menu: Edit, Delete, Show in 3D.
        let menu = crate::browser::tree(&h.app)
            .into_iter()
            .flat_map(|(_, rows)| rows)
            .find(|r| r.item.is_some() && r.pick.is_some())
            .map(|r| r.text.trim_start().to_owned());
        if let Some(text) = menu {
            step(&mut h, json!({"click": format!("row:{text}"), "button": "right"}));
            step(&mut h, json!({"key": "Escape"}));
        }
        for s in Step::ALL.into_iter().filter(|s| steps.is_empty() || steps.contains(&s.id())) {
            call(&mut h, "ui.set", json!({"step": s.id()}));
            h.frames(2);
        }
        // Results of a model changed after their run: out of date in their view and the browser.
        if h.app.session.results().any() {
            let _ = h.call("engine.execute", json!({"command": "project.frequency", "params": {"start": 7, "stop": 9.5}}));
            if let Some(v) = View::ALL.into_iter().find(|v| v.is_result() && v.available(&h.app)) {
                call(&mut h, "ui.set", json!({"view": v.id()}));
                h.frames(2);
            }
        }
        // The Results step's 3D view, its field playing and paused, orthographic.
        call(&mut h, "ui.set", json!({"view": "model"}));
        h.frames(2);
        h.app.field.player.playing = false;
        call(&mut h, "ui.set", json!({"camera": {"projection": "orthographic"}}));
        h.frames(2);
        // Cut open (when there is a model to cut).
        if h.call("ui.set", json!({"section": {"axis": "x"}}))["ok"] == json!(true) {
            h.frames(2);
            call(&mut h, "ui.set", json!({"section": null}));
        }
        call(&mut h, "ui.set", json!({"step": "setup"}));
        for s in Solver::ALL {
            h.app.workflow.fields.solver = Some(s);
            h.frames(2);
        }
        h.app.workflow.fields.solver = None;
        for m in ["menu:file", "menu:edit"] {
            step(&mut h, json!({"click": m}));
            step(&mut h, json!({"key": "Escape"}));
        }
        call(&mut h, "ui.set", json!({"step": "none", "camera": {"projection": "perspective"}}));
    }
    // The 3D view's tools: no port on a curved face, nor on a guide's side wall without a solid,
    // nor across a guide inside it; a measurement between two faces, with their gap.
    call(&mut h, "engine.execute", json!({"command": "file.new"}));
    call(&mut h, "engine.execute", json!({"command": "geometry.solid", "params": {"shape": "cylinder", "radius": 10, "length": 30}}));
    call(&mut h, "ui.set", json!({"view": "model", "tool": "port"}));
    call(&mut h, "ui.set", json!({"step": "none", "standard_view": "top"}));
    step(&mut h, json!({"move": "view3d"}));
    let guide = json!({"width": 20, "height": 10, "sections": [{"length": 10, "material": "vacuum"}, {"length": 10, "material": "vacuum"}]});
    call(&mut h, "engine.execute", json!({"command": "geometry.waveguide", "params": guide}));
    call(&mut h, "ui.set", json!({"view": "model", "tool": "port"}));
    call(&mut h, "ui.set", json!({"step": "none", "standard_view": "top"}));
    step(&mut h, json!({"move": "view3d"}));
    call(&mut h, "ui.set", json!({"standard_view": "front", "section": {"axis": "z", "at": 5}}));
    step(&mut h, json!({"move": {"handle": "view3d", "offset": [1, 0]}}));
    call(&mut h, "ui.set", json!({"section": null, "tool": "measure"}));
    let face = |z: f64| crate::tools3d::Snap { at: [0.0, 0.0, z], kind: crate::tools3d::SnapKind::Face, dir: Some([0.0, 0.0, 1.0]) };
    apply(&mut h, [Action::Measure([face(0.0), face(0.01)])]);
    h.frames(2);
    call(&mut h, "ui.set", json!({"tool": "none"}));
    // The command search.
    for text in ["zzzz", "mesh.generate {}", "mesh"] {
        h.app.search.text = text.into();
        h.app.search.open = true;
        h.app.search.field = Some(egui::Rect::from_min_size(egui::pos2(900.0, 40.0), egui::vec2(290.0, 22.0)));
        h.frames(2);
    }
    h.app.search.open = false;
    // Every command's dialog, docked and floating, some refused, a job running under one.
    for c in emcraft_engine::COMMANDS.iter().filter(|c| !c.params.is_empty()) {
        call(&mut h, "ui.start", json!({"command": c.id}));
        h.frames(2);
    }
    call(&mut h, "ui.set", json!({"dock": true}));
    let sections = json!([{"length": "", "material": "vacuum"}, {"length": "x", "material": "vacuum"}]);
    call(&mut h, "ui.start", json!({"command": "geometry.waveguide", "params": {"width": "wide", "height": "", "sections": sections}}));
    let _ = h.call("ui.dialog", json!({"ok": true}));
    h.frames(2);
    call(&mut h, "ui.set", json!({"dock": false}));
    for params in [
        json!({"command": "results.probe", "params": {"at": "{not json"}}),
        json!({"command": "solve.eigenmodes", "params": {"count": "1.5", "near": "1e999", "order": "=w"}}),
        json!({"command": "solve.eigenmodes", "params": {"near": format!("={}", "a".repeat(emcraft_engine::expr::MAX_LEN + 2))}}),
        json!({"command": "geometry.waveguide", "params": {"sections": [{"length": "1", "material": "vacuum", "width": "x"}]}}),
    ] {
        call(&mut h, "ui.start", params);
        let _ = h.call("ui.dialog", json!({"ok": true}));
    }
    h.frames(2);
    // The steps' fields refused, and Run refused.
    call(&mut h, "engine.execute", json!({"command": "file.new"}));
    call(&mut h, "engine.script", json!({"commands": json(MODELS[1].0)}));
    h.app.workflow.step = Some(Step::Intent);
    h.frames(1);
    let f = &mut h.app.workflow.fields;
    (f.start, f.stop, f.points, f.order, f.solver, f.frequency) =
        ("x".into(), "0".into(), "many".into(), "7".into(), Some(Solver::Radiation), String::new());
    apply(&mut h, [Action::ApplyBand, Action::StartRun]);
    h.app.workflow.step = Some(Step::Run);
    h.frames(2);
    let f = &mut h.app.workflow.fields;
    (f.start, f.stop, f.touchstone, f.solver) = ("9".into(), "8".into(), " ".into(), Some(Solver::Frequency));
    apply(&mut h, [Action::ApplyBand, Action::ExportTouchstone]);
    h.app.workflow.step = Some(Step::Setup);
    h.frames(2);
    // The status line of every command, from an empty reply and the replies that change it.
    let session = h.app.session.clone();
    let none = json!({});
    for c in emcraft_engine::COMMANDS {
        let _ = crate::status::text(c.id, &none, &none, &session);
    }
    for (id, params, reply) in [
        ("file.open", json!({"path": "a.emcraft"}), json!({})),
        ("file.open", json!({"path": "a.emcraft", "original": "b.emcraft"}), json!({"name": "b"})),
        ("solve.time_domain", json!({}), json!({"device": "cuda", "precision": "single"})),
        ("edit.redo", json!({}), json!({})),
        ("results.check", json!({}), json!({"passive": true})),
        ("mesh.adapt", json!({}), json!({"quantity": "far_field", "converged": true})),
    ] {
        let _ = crate::status::text(id, &params, &reply, &session);
    }
    // An autosave that cannot be written.
    h.app.autosave.dir = Some("/dev/null/autosave".into());
    let _ = h.call("engine.execute", json!({"command": "model.material.create", "params": {"name": "tour", "eps_r": 3}}));
    assert!(h.app.autosave_now().is_err());
    h.app.autosave.dir = None;
    h.app.export = Some("history.json".into());
    h.frames(2);
    let written = std::env::temp_dir().join(format!("emcraft-tour-{}.json", std::process::id()));
    apply(&mut h, [Action::ExportHistory(written.display().to_string()), Action::ExportHistory("/no/such/dir/history.json".into())]);
    let _ = std::fs::remove_file(written);
    record(false)
}

/// Texts a tour of the window cannot reach, with why.
const UNREACHED: &[(&str, &str)] = &[
    ("Time-domain radiation", "a time-domain solve with sources takes too long for the gate"),
    ("Time domain: {n} frequencies", "a time-domain solve takes too long for the gate"),
    ("modes ({unit})", "a study of eigenmodes takes too long for the gate"),
    ("Entries of S: {entries}", "needs a terminal port (scenario terminal_ports)"),
    ("Port {name} (on a face)", "needs a port on an imported face (scenario import_cavity)"),
    ("Port {name} (on a face): {n} mode{impedance}", "needs a port on an imported face (scenario import_cavity)"),
    ("Port {name} (on a face): {n} modes{impedance}", "needs a port on an imported face (scenario import_cavity)"),
    ("Region: {name} (imported), {material}", "needs an imported file (scenario import_bodies)"),
    ("NOT passive", "needs a sweep that is not passive"),
    ("Back to the port's face", "a hint on the selected port's reference plane (scenario view3d_port_tool)"),
    ("Parameters", "needs a template's card (scenario template_dialog)"),
    ("More commands", "needs a window too narrow for the ribbon's buttons (scenario small_window)"),
    ("More tabs", "needs a window too narrow for the ribbon's tabs (scenario small_window)"),
    ("Switching the device…", "shown while `solve.device` runs, a moment"),
    ("The next solve runs on {device} in {precision} precision.", "needs a device other than the CPU"),
    ("The next solve runs on the CPU, its factorization's large fronts on {device} (complex double).", "needs a device other than the CPU"),
    ("{name} cannot run here", "depends on the machine's devices"),
    ("{name} cannot run here: {problem}", "depends on the machine's devices"),
    ("an NVIDIA GPU", "depends on the machine's devices"),
    ("this device does not run it", "needs a device other than the CPU"),
    ("Running with {n} accepted warning.", "starts a full solve"),
    ("Running with {n} accepted warnings.", "starts a full solve"),
    ("starting", "only until a job says how much work it has: a moment, timing-dependent"),
    ("Script", "a script's progress: it ends before a frame shows it"),
    ("goal", "an optimization of a number the solve gives (maximize, minimize): the tour's goal is in dB"),
    ("{command} · {what} · {secs} s", "a job whose run cannot be estimated (opening a file): it ends before a frame shows it"),
    ("terminal ports: solved again", "needs a sweep with terminal ports (scenario terminal_ports)"),
];

/// The texts that are looked up by name rather than written where they are shown: the views',
/// tabs', steps' and sections' labels, the ribbon's panels and short labels, the built-in
/// themes.
fn named() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut add = |s: &str| {
        out.insert(s.to_owned());
    };
    crate::View::ALL.iter().for_each(|v| add(v.label()));
    crate::ribbon::RibbonTab::ALL.iter().for_each(|v| add(v.label()));
    crate::settings::Section::ALL.iter().for_each(|v| add(v.label()));
    crate::steps::Step::ALL.iter().for_each(|v| add(v.label()));
    crate::steps::Status::ALL.iter().for_each(|v| add(v.label()));
    crate::steps::Solver::ALL.iter().for_each(|v| add(v.label()));
    crate::tools3d::Tool::ALL.iter().for_each(|v| add(v.label()));
    for (_, panels) in crate::ribbon::TABS {
        panels.iter().for_each(|(p, _)| add(p));
    }
    crate::commands::UI_COMMANDS.iter().filter_map(|c| c.label).for_each(&mut add);
    for b in emcraft_engine::theme::Base::ALL {
        let mut name = b.id().to_owned();
        if let Some(first) = name.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        add(&name);
    }
    out
}

/// Drawn in Japanese, nothing on screen is left in English, and every translation is used.
#[test]
fn a_tour_of_the_window_in_japanese_finds_every_text_translated_and_none_unused() {
    let r = tour();
    let missed: Vec<&String> = r.missed.iter().collect();
    assert!(
        missed.is_empty(),
        "{} texts have no Japanese:\n{}",
        missed.len(),
        missed.iter().map(|m| format!("{m:?}")).collect::<Vec<_>>().join("\n")
    );
    let named = named();
    for n in &named {
        assert!(japanese().strings.contains_key(n), "no Japanese for the label `{n}`");
    }
    let unused: Vec<&String> =
        japanese().strings.keys().filter(|k| !r.asked.contains(*k) && !named.contains(*k) && !UNREACHED.iter().any(|(u, _)| u == k)).collect();
    assert!(
        unused.is_empty(),
        "{} translations are never asked for (stale, or add them to UNREACHED with why):\n{}",
        unused.len(),
        unused.iter().map(|m| format!("{m:?}")).collect::<Vec<_>>().join("\n")
    );
}

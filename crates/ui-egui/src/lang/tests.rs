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

/// The Japanese style (docs/languages.md): a half-width space between Latin letters or digits
/// and Japanese (Sパラメータ the one exception, code in backticks left alone), ranges with 〜
/// (U+301C), never ～ (U+FF5E), and command texts in the plain form, never です・ます.
#[test]
fn the_japanese_keeps_its_style() {
    // Kana and kanji; the middle dot ・ joins words without spaces.
    fn japanese_char(c: char) -> bool {
        c != '・' && matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}')
    }
    fn unspaced(text: &str) -> Option<String> {
        let text = text.replace("Sパラメータ", "S パラメータ");
        let prose = text.split('`').step_by(2).collect::<String>();
        let chars: Vec<char> = prose.chars().collect();
        chars.windows(2).find_map(|w| {
            let crossing = (w[0].is_ascii_alphanumeric() && japanese_char(w[1])) || (japanese_char(w[0]) && w[1].is_ascii_alphanumeric());
            crossing.then(|| w.iter().collect())
        })
    }
    let ja = japanese();
    let mut texts: Vec<(String, &str, bool)> = ja.strings.iter().map(|(k, v)| (k.clone(), v.as_str(), false)).collect();
    texts.extend(ja.samples.iter().map(|(k, v)| (format!("sample {k}"), v.as_str(), false)));
    for (id, c) in &ja.commands {
        texts.push((format!("{id} label"), &c.label, true));
        texts.push((format!("{id} doc"), &c.doc, true));
        texts.extend(c.params.iter().map(|(p, v)| (format!("{id}.{p}"), v.as_str(), true)));
    }
    let mut problems: Vec<String> = Vec::new();
    for (key, text, command) in &texts {
        if let Some(pair) = unspaced(text) {
            problems.push(format!("{key}: no space in {pair:?}"));
        }
        if text.contains('～') {
            problems.push(format!("{key}: ～ (U+FF5E) for a range, write 〜"));
        }
        if *command && (text.contains("です") || text.contains("ます")) {
            problems.push(format!("{key}: です・ます in a command's text, write the plain form"));
        }
    }
    problems.sort();
    assert!(problems.is_empty(), "{} Japanese texts out of style:\n{}", problems.len(), problems.join("\n"));
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
    // A guide with a port and no band, chosen a resonator: the steps' errors that ask for one,
    // its port a mismatch; a failed solve.
    (
        r#"[{"command": "geometry.waveguide", "params": {"width": 22.86, "height": 10.16, "sections": [{"length": 10, "material": "vacuum", "width": 20}]}},
            {"command": "model.symmetry", "params": {"axis": "y", "kind": "electric"}},
            {"command": "mesh.generate", "params": {"size": 8}},
            {"command": "model.port.create", "params": {"name": "1", "end": "start", "modes": 1}},
            {"command": "project.analysis", "params": {"kind": "eigenmodes"}}]"#,
        r#"[{"command": "solve.frequency", "params": {"points": 3}}]"#,
        &[],
    ),
    // A cavity of revolution: its rotation found (not set), then set and drawn with its turns.
    (r#"[{"command": "geometry.solid", "params": {"shape": "cylinder", "radius": 10, "length": 15}}]"#, "[]", &["boundaries"]),
    (
        r#"[{"command": "geometry.solid", "params": {"shape": "cylinder", "radius": 10, "length": 15}},
            {"command": "model.rotation.find", "params": {}}]"#,
        "[]",
        &["boundaries"],
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
            {"command": "project.setup", "params": {"kind": "frequency", "start": 7, "stop": 9}},
            {"command": "model.symmetry", "params": {"axis": "x", "kind": "magnetic"}},
            {"command": "mesh.generate", "params": {"size": 8}},
            {"command": "model.port.create", "params": {"name": "1", "end": "start", "modes": 1}},
            {"command": "model.port.create", "params": {"name": "2", "end": "end", "modes": 2}},
            {"command": "solve.frequency", "params": {"start": 7, "stop": 9, "points": 9, "order": 1}}]"#,
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
    // The filter optimized by the Bayesian search, then swept with a surrogate.
    (
        "[]",
        r#"[{"command": "study.optimize", "params": {"variables": [{"name": "l", "min": 18, "max": 22}], "goal": {"type": "min_max_db", "parameter": "s11"},
              "solve": {"command": "solve.frequency", "params": {"frequencies": [8], "order": 1}}, "max_evaluations": 3, "method": "bayesian"}}]"#,
        &["results"],
    ),
    (
        "[]",
        r#"[{"command": "study.sweep", "params": {"variable": "l", "values": [16, 18, 20, 22], "outputs": ["s21"], "surrogate": {"budget": 3},
              "solve": {"command": "solve.frequency", "params": {"frequencies": [7, 8], "order": 1}}}}]"#,
        &["results"],
    ),
    // A cavity with an electric wall: eigenmodes.
    (
        r#"[{"command": "geometry.waveguide", "params": {"width": 22.86, "height": 10.16, "sections": [{"length": 30, "material": "vacuum"}]}},
            {"command": "model.walls", "params": {"conductivity": 5.8e7, "thickness": 1.27}},
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
            {"command": "project.setup", "params": {"kind": "frequency", "start": 9, "stop": 11}},
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
            {"command": "model.periodic", "params": {"x": false, "y": false, "z": true}}, {"command": "project.setup", "params": {"kind": "frequency", "start": 5, "stop": 6}},
            {"command": "solve.dispersion", "params": {"from": 0, "to": 180, "points": 3, "bands": 1, "near": 15, "order": 1}}]"#,
        "[]",
        &["boundaries", "run"],
    ),
    // A body placed with no region: the region automatic round it (ADR 0005).
    (r#"[{"command": "geometry.body", "params": {"shape": "sphere", "radius": 5, "material": "metal"}}]"#, "[]", &["geometry"]),
    // The same pinned: a box off the origin.
    (
        r#"[{"command": "geometry.body", "params": {"shape": "sphere", "radius": 5, "material": "metal"}}, {"command": "model.region", "params": {"mode": "pinned"}}]"#,
        "[]",
        &["geometry"],
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
    // Keep this language coverage tour independent of the machine's preferred accelerator.
    call(&mut h, "engine.execute", json!({"command":"solve.device","params":{"device":"cpu","precision":"double"}}));
    record(true);
    h.app.info_with("Open a sample or a project (File) or build a model from the ribbon.", &[]);
    // The start page, empty and with a project missing from the recent list, the settings
    // (Files with no recent project too), the prompts.
    h.app.page = crate::home::Page::Home;
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
    for purpose in [Purpose::Open, Purpose::SaveAs, Purpose::Import] {
        h.app.ask = Some(crate::files::Ask::Path(Prompt { purpose, text: String::new() }));
        h.frames(2);
    }
    h.app.ask = None;
    h.app.ask = Some(crate::files::Ask::Save(Then::New));
    h.frames(2);
    h.app.ask = None;
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
    h.app.page = crate::home::Page::Workspace;
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
        h.app.workflow.fields = crate::steps::Fields::default();
        let views: Vec<View> = View::ALL.into_iter().filter(|v| i == 0 || v.available(&h.app)).collect();
        for v in views {
            if v == View::Field {
                h.app.field.beside_3d = true;
            }
            call(&mut h, "ui.set", json!({"view": v.id(), "step": "none"}));
            h.frames(3);
            // Also tour the optional flat plot and its probing controls.
            if v == View::Field {
                h.app.field.beside_3d = false;
                h.frames(2);
            }
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
                    // One frame playing says so; more only draw the field again at each phase.
                    h.app.field.player.playing = false;
                }
            }
        }
        if i == 3 {
            // A job running: its progress, a dialog that waits, Run's panel, a refused request.
            // The field's animation stopped first: each of its frames is a new 3D picture, which
            // a short job could outrun.
            call(&mut h, "ui.set", json!({"animate": false}));
            h.frames(2);
            // Keep the worker alive until the UI has inspected it, independently of
            // solver speed and the time spent drawing on a software graphics adapter.
            let (release, held) = std::sync::mpsc::channel();
            h.app.job = Some(crate::worker::Job::spawn(
                &h.app.session,
                "solve.frequency",
                json!({"start": 7, "stop": 9, "points": 101, "order": 2}),
                None,
                move |_| {
                    emcraft_engine::progress::report(1, 101, "Frequency sweep")?;
                    let _ = held.recv_timeout(std::time::Duration::from_secs(30));
                    emcraft_engine::progress::report(2, 101, "Frequency sweep")?;
                    Ok(json!({}))
                },
            ));
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
            // Exercise the remaining-time wording without depending on rendering speed.
            if let Some(job) = h.app.job.as_mut() {
                job.started = job.started.checked_sub(std::time::Duration::from_secs(2)).unwrap_or(job.started);
            }
            h.frames(1);
            assert!(h.app.busy(), "the job ended before the tour saw it run");
            // Closing while it runs.
            h.app.ask = Some(crate::files::Ask::Save(Then::Quit));
            h.frames(2);
            h.app.ask = None;
            apply(&mut h, [Action::File(FileAction::Request(Then::New))]);
            call(&mut h, "ui.cancel", json!({}));
            let _ = release.send(());
            call(&mut h, "ui.set", json!({"step": "none"}));
        }
        call(&mut h, "ui.set", json!({"view": "model"}));
        let picked = crate::browser::tree(&h.app)
            .into_iter()
            .find_map(|(s, rows)| rows.into_iter().find(|r| r.pick.is_some()).map(|r| (format!("{s}/{}", r.text), r.pick)));
        // Several parts selected: the card says how many.
        let picks: Vec<_> = crate::browser::tree(&h.app).into_iter().flat_map(|(_, rows)| rows).filter_map(|r| r.pick).take(2).collect();
        if picks.len() == 2 {
            h.app.view3d.selected = picks;
            h.frames(2);
        }
        if let Some((row, pick)) = picked {
            (h.app.browser.selected, h.app.view3d.selected) = (Some(row), pick.into_iter().collect());
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
            let _ = h.call("engine.execute", json!({"command": "project.setup", "params": {"kind": "frequency", "start": 7, "stop": 9.5}}));
            if let Some(v) = View::ALL.into_iter().find(|v| v.is_result() && v.available(&h.app)) {
                call(&mut h, "ui.set", json!({"view": v.id()}));
                h.frames(2);
            }
        }
        // The Results step's 3D view, its field playing and paused, orthographic.
        call(&mut h, "ui.set", json!({"view": "model"}));
        h.app.field.player.playing = h.app.session.results().any();
        h.frames(1);
        h.app.field.player.playing = false;
        call(&mut h, "ui.set", json!({"camera": {"projection": "orthographic"}}));
        h.frames(2);
        // Cut open (when there is a model to cut).
        if h.call("ui.set", json!({"section": {"axis": "x"}}))["ok"] == json!(true) {
            h.frames(2);
            // Its dimensions off, then on again.
            h.app.view3d.section_dims = false;
            h.frames(2);
            h.app.view3d.section_dims = true;
            call(&mut h, "ui.set", json!({"section": null}));
        }
        call(&mut h, "ui.set", json!({"step": "setup"}));
        for s in Solver::ALL {
            call(&mut h, "engine.execute", json!({"command":"project.analysis","params":{"kind":s.id()}}));
            // A frequency for the kinds that work at one.
            h.app.workflow.fields.frequency = "9".into();
            h.frames(2);
        }
        h.app.workflow.fields = crate::steps::Fields::default();
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
    // PCB diagnostics use the same command path as the workspace, in both valid and blocked cases.
    for split in [true, false] {
        call(&mut h, "engine.execute", json!({"command":"pcb.sample","params":{"split":split}}));
        call(&mut h, "engine.execute", json!({"command":"view.pcb","params":{"stage":"net"}}));
        h.frames(2);
        call(&mut h, "engine.execute", json!({"command":"pcb.inspect","params":{"net":"DATA"}}));
        h.frames(3);
    }
    // The kinds of analysis, listed by the step bar's switch.
    call(&mut h, "ui.click", json!({"handle":"step:intent"}));
    h.frames(2);
    call(&mut h, "ui.click", json!({"handle":"step:intent"}));
    call(&mut h, "ui.set", json!({"view":"pcb"}));
    for stage in emcraft_engine::pcb::Stage::ALL {
        call(&mut h, "engine.execute", json!({"command":"view.pcb","params":{"stage":stage.id()}}));
        h.frames(2);
    }
    h.app.views.pcb.baseline = h.app.replies.pcb.clone();
    if let Some(r) = &mut h.app.replies.pcb {
        r.findings = emcraft_engine::pcb::Code::ALL
            .into_iter()
            .map(|code| emcraft_engine::pcb::FindingRow { code, method: "geometry_screening".into(), segment: None, detail: String::new() })
            .collect();
    }
    h.frames(2);
    // Conditions typed but not confirmed: Set conditions asks for its button and the diagnosis
    // waits; then a report of other conditions says so.
    h.app.views.pcb.reference = "75".into();
    for stage in ["settings", "inspect"] {
        call(&mut h, "engine.execute", json!({"command":"view.pcb","params":{"stage":stage}}));
        h.frames(2);
    }
    call(&mut h, "engine.execute", json!({"command":"pcb.conditions","params":{"reference":75}}));
    call(&mut h, "engine.execute", json!({"command":"view.pcb","params":{"stage":"results"}}));
    h.frames(2);
    call(&mut h, "engine.execute", json!({"command":"file.sample","params":{"name":"pcb-parallel"}}));
    call(&mut h, "engine.execute", json!({"command":"view.pcb","params":{"net":"DATA","victim":"CLK","stage":"net"}}));
    h.frames(2);
    call(&mut h, "engine.execute", json!({"command":"pcb.inspect","params":{"net":"DATA"}}));
    call(&mut h, "engine.execute", json!({"command":"view.pcb","params":{"stage":"results"}}));
    h.frames(2);
    // A saved split still has copper elsewhere on the layer, so the local return can be absent.
    call(&mut h, "engine.execute", json!({"command":"pcb.sample","params":{"split":true}}));
    call(&mut h, "engine.execute", json!({"command":"pcb.inspect","params":{"net":"DATA"}}));
    call(&mut h, "engine.execute", json!({"command":"view.pcb","params":{"victim":"CLK","mode":"return","stage":"results"}}));
    h.frames(2);
    let saved = h.app.replies.pcb.clone();
    if let Some(r) = &mut h.app.replies.pcb {
        r.coupling_pairs.clear();
    }
    h.frames(2);
    h.app.replies.pcb = saved;
    let mut no_ground = h.app.session.model().pcb.clone().unwrap();
    no_ground.copper.clear();
    no_ground.map_lengths(|v| h.app.session.display().length.from_si(v));
    call(&mut h, "engine.execute", json!({"command":"pcb.load","params":{"data":no_ground}}));
    call(&mut h, "engine.execute", json!({"command":"pcb.inspect","params":{"net":"DATA"}}));
    call(&mut h, "engine.execute", json!({"command":"view.pcb","params":{"victim":"CLK"}}));
    h.frames(3);
    call(&mut h, "engine.execute", json!({"command":"pcb.sample","params":{"split":false}}));
    // A native copy without a neighbour reaches the clean-screening wording too.
    let mut board = h.app.session.model().pcb.clone().unwrap();
    board.segments.retain(|s| s.net == "DATA");
    board.map_lengths(|v| h.app.session.display().length.from_si(v));
    call(&mut h, "engine.execute", json!({"command":"pcb.load","params":{"data":board}}));
    call(&mut h, "engine.execute", json!({"command":"pcb.inspect","params":{"net":"DATA"}}));
    h.frames(3);
    h.app.views.pcb.reference = "invalid".into();
    h.frames(2);
    h.app.replies.pcb = None;
    h.app.views.pcb.attempted = None;
    h.frames(2);
    h.app.views.pcb.reference = "50".into();
    // Every command's dialog, docked and floating, some refused, a job running under one.
    for c in emcraft_engine::COMMANDS.iter().filter(|c| !c.params.is_empty()) {
        call(&mut h, "ui.start", json!({"command": c.id}));
        h.frames(2);
        if let Some(d) = h.app.dialog.as_mut() {
            d.help = true;
            d.options = true;
        }
        h.frames(2);
    }
    // The import dialog on a design (its parameters), a 3MF package (its bodies) and an STL file
    // (its units), its options and help unfolded, the line of each kind of region.
    let stl = std::env::temp_dir().join(format!("emcraft-tour-{}.stl", std::process::id()));
    let facet = |a: &str, b: &str, c: &str| format!("facet normal 0 0 0\nouter loop\nvertex {a}\nvertex {b}\nvertex {c}\nendloop\nendfacet\n");
    let (o, x, y, z) = ("0 0 0", "1 0 0", "0 1 0", "0 0 1");
    let text = format!("solid t\n{}{}{}{}endsolid t\n", facet(o, y, x), facet(o, x, z), facet(o, z, y), facet(x, y, z));
    std::fs::write(&stl, text).unwrap_or_else(|e| panic!("{}: {e}", stl.display()));
    call(&mut h, "ui.start", json!({"command": "geometry.import", "params": {"path": "../../tests/oracle/design-post/post.solidcraft"}}));
    h.frames(2);
    if let Some(d) = h.app.dialog.as_mut() {
        (d.options, d.help) = (true, true);
    }
    for set in [
        json!({"region": "box"}),
        json!({"region": "cavity"}),
        json!({"region": "guide"}),
        json!({"region": "", "open": true}),
        json!({"open": false, "path": "tests/fixtures/post-and-puck.3mf"}),
        json!({"path": stl.display().to_string()}),
    ] {
        call(&mut h, "ui.dialog", json!({"set": set}));
        h.frames(2);
    }
    let _ = std::fs::remove_file(&stl);
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
    h.app.place = h.app.place.with_step(Some(Step::Setup));
    h.frames(1);
    call(&mut h, "engine.execute", json!({"command":"project.analysis","params":{"kind":"radiation"}}));
    let f = &mut h.app.workflow.fields;
    (f.start, f.stop, f.points, f.order, f.frequency) = ("x".into(), "0".into(), "many".into(), "7".into(), String::new());
    apply(&mut h, [Action::ConfirmSetup, Action::StartRun]);
    h.app.place = h.app.place.with_step(Some(Step::Run));
    h.frames(2);
    call(&mut h, "engine.execute", json!({"command":"project.analysis","params":{"kind":"frequency"}}));
    let f = &mut h.app.workflow.fields;
    (f.start, f.stop) = ("9".into(), "8".into());
    apply(&mut h, [Action::ConfirmSetup]);
    h.app.workflow.fields.touchstone = " ".into();
    apply(&mut h, [Action::ExportTouchstone]);
    h.app.place = h.app.place.with_step(Some(Step::Setup));
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
    h.app.ask = Some(crate::files::Ask::Export("history.json".into()));
    h.frames(2);
    let written = std::env::temp_dir().join(format!("emcraft-tour-{}.json", std::process::id()));
    apply(&mut h, [Action::ExportHistory(written.display().to_string()), Action::ExportHistory("/no/such/dir/history.json".into())]);
    let _ = std::fs::remove_file(written);
    record(false)
}

/// Texts a tour of the window cannot reach, with why.
const UNREACHED: &[(&str, &str)] = &[
    ("Click to add segments; right click or Esc ends.", "step 1 while drawing (scenario pcb_edit)"),
    ("Route segments", "step 1 while drawing (scenario pcb_edit)"),
    ("The route needs exactly two free ends for its ports.", "step 4 for a branching route; the samples' routes run end to end"),
    ("Show object", "tooltip on the eye of a hidden material (scenario material_visibility)"),
    ("Drop to import", "shown while a file is dragged over the window, which the headless harness does not do"),
    ("Drop to open", "shown while a file is dragged over the window, which the headless harness does not do"),
    (
        "No GPU: the 3D view needs a graphics adapter (or a software one, such as Mesa's llvmpipe).",
        "shown only on a machine without any wgpu adapter",
    ),
    ("Time-domain radiation", "a time-domain solve with sources takes too long for the gate"),
    ("Time domain: {n} frequencies", "a time-domain solve takes too long for the gate"),
    ("modes ({unit})", "a study of eigenmodes takes too long for the gate"),
    ("predicted", "a surrogate sweep of eigenmodes takes too long for the gate"),
    (" · Bayesian", "a Bayesian optimization with fewer than two points solved (unit tests of study::bayes)"),
    ("Entries of S: {entries}", "needs a terminal port (scenario terminal_ports)"),
    ("Port {name} (on a face)", "needs a port on an imported face (scenario import_cavity)"),
    ("Port {name} (on a face): {n} mode{impedance}", "needs a port on an imported face (scenario import_cavity)"),
    ("Port {name} (on a face): {n} modes{impedance}", "needs a port on an imported face (scenario import_cavity)"),
    ("Region: {name} (imported), {material}", "needs an imported file (scenario import_bodies)"),
    ("{name}: metal walls (imported)", "needs an imported file (scenario import_cavity)"),
    ("{name}: the region (imported)", "needs an imported file (scenario import_cavity)"),
    ("Ports go on the channel's openings, not on its walls", "a port-tool click on an imported cavity's wall (unit test tools3d::tests)"),
    ("NOT passive", "needs a sweep that is not passive"),
    ("Back to the port's face", "a hint on the selected port's reference plane (scenario view3d_port_tool)"),
    ("GPU {device}: {n} fronts", "needs a GPU device (the CUDA runtime and an NVIDIA GPU)"),
    ("GPU {device}", "needs a GPU device"),
    ("CPU kernels ({device})", "needs the cubecl-cpu device (scenario time_domain_device)"),
    ("The time domain's steps on the GPU", "needs a GPU device"),
    ("CPU (no front large enough for {device})", "needs a GPU device"),
    ("The factorization on the CPU, its large fronts on the GPU", "needs a GPU device"),
    ("Blank", "needs a purpose chosen on the start page (scenario purpose_first)"),
    ("An empty project for this analysis", "needs an analysis chosen on the start page (scenario purpose_first)"),
    ("as it was", "needs two solves kept (scenario run_history)"),
    ("Click: draw it over the chart · right-click: restore, pin, delete", "needs two solves kept (scenario run_history)"),
    ("|S| (linear)", "in the S-parameter display menu, open (scenario sparameter_display)"),
    ("phase (°)", "in the S-parameter display menu, open (scenario sparameter_display)"),
    ("History", "needs two solves kept (scenario run_history)"),
    ("Pin", "needs two solves kept (scenario run_history)"),
    ("Unpin", "needs two solves kept (scenario run_history)"),
    ("Restore", "needs two solves kept (scenario run_history)"),
    ("{n} frequencies", "needs two solves kept (scenario run_history)"),
    ("deepest |S11| {db} dB at {f} {unit}", "needs two solves kept (scenario run_history)"),
    ("solved in {seconds} s on {device}", "needs two solves kept (scenario run_history)"),
    ("{unknowns} unknowns, {tets} tetrahedra", "needs two solves kept (scenario run_history)"),
    ("Saved: restore solves it again", "a tip of a solve read back from a project file (scenario saved_history)"),
    (
        "Not read back (solve again): {what}",
        "a project file whose results no longer fit its model (engine test results_that_do_not_fit_are_left_out_with_why)",
    ),
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
    ("fast: {n} in full, residual {r}", "a fast sweep after its first pass in the basis, timing-dependent (scenario time_left)"),
    ("goal", "an optimization of a number the solve gives (maximize, minimize): the tour's goal is in dB"),
    ("{command} · {what} · {secs} s", "a job whose run cannot be estimated (opening a file): it ends before a frame shows it"),
    ("terminal ports: solved again", "needs a sweep with terminal ports (scenario terminal_ports)"),
    ("{view} — drag to orbit, Shift snaps", "the view cube's tip, shown while the pointer rests on a face"),
    ("View from {side} ({view})", "the axes navigator's tip (Settings ▸ 3D view ▸ Navigator: Axes; scenario navigation)"),
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
    // The mouse mappings' help (shown for the chosen one), the orbit centres, the navigators and
    // the view cube's faces (those turned away are not drawn).
    for p in crate::navigation::NAV_PRESETS {
        add(p.help);
    }
    add("Touchpad");
    crate::navigation::OrbitAround::ALL.iter().for_each(|v| add(v.label()));
    crate::navigation::Navigator::ALL.iter().for_each(|v| add(v.label()));
    crate::view_cube::FACE_NAMES.iter().for_each(|n| add(n));
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

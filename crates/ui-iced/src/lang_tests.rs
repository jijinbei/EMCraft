//! A tour of the iced window in Japanese: every scenario played step by step and every page,
//! step, card and setting drawn (headless, on the CPU), with what was looked up recorded — no
//! text drawn is left in English, and no translation is one that nothing asks for.

use std::collections::BTreeSet;

use emcraft_ui_core::lang::{self, Record};
use serde_json::{Value, json};

use crate::app::{App, Message};

/// Draws the window as it is (its texts looked up as a frame looks them up).
fn draw(app: &mut App) {
    let _ = crate::headless::render(app, (1400.0, 900.0));
}

/// Every page, step, setting and card of `app`'s project, drawn.
fn visit(app: &mut App) {
    for v in crate::run::PAGES {
        app.update(Message::Page(v));
        draw(app);
    }
    for s in emcraft_engine::workflow::Step::ALL {
        app.update(Message::Step(Some(s)));
        draw(app);
    }
    app.update(Message::Step(None));
    for open in [true, false] {
        app.update(Message::Log(open));
        app.update(Message::Settings(open));
        draw(app);
    }
}

/// The tour: every scenario's steps (what fails in Japanese is left), each scenario's window
/// visited at its end; the start page with each purpose.
fn tour() -> Record {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios");
    let mut files: Vec<_> = std::fs::read_dir(&dir).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    files.sort();
    lang::set("ja");
    lang::record(true);
    for f in &files {
        let steps: Vec<Value> = std::fs::read_to_string(f).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let mut app = App::new(None);
        // Each step's window built (its texts looked up), drawn where it ends.
        for s in &steps {
            let _ = crate::scenario::run(&mut app, std::slice::from_ref(s));
            drop(app.view());
        }
        draw(&mut app);
        visit(&mut app);
    }
    let mut app = App::new(None);
    app.update(Message::Home(true));
    for k in emcraft_ui_core::steps::Solver::ALL {
        app.update(Message::Purpose(Some(k)));
        draw(&mut app);
    }
    // Every command's dialog, on a sample's project (its fields' names).
    let mut app = App::new(Some("highpass"));
    for c in emcraft_engine::COMMANDS {
        app.update(Message::Open(c.id, json!({})));
        drop(app.view());
        app.update(Message::Dialog(crate::dialog::Edit::Cancel));
    }
    // Every command's message, from an empty reply.
    for c in emcraft_engine::COMMANDS {
        let _ = emcraft_ui_core::status::text(c.id, &json!({}), &json!({}), &app.session);
    }
    let r = lang::record(false);
    lang::set("en");
    r
}

/// The texts looked up by name rather than written where they are shown: the views', tabs',
/// steps' and sections' labels, the ribbon's panels, the navigation's words, the view cube's
/// faces, the window's own commands, the values' words, the built-in themes.
fn named() -> BTreeSet<String> {
    use emcraft_ui_core::*;
    let mut out = BTreeSet::new();
    let mut add = |s: &str| {
        out.insert(s.to_owned());
    };
    view::View::ALL.iter().for_each(|v| add(v.label()));
    ribbon::RibbonTab::ALL.iter().for_each(|v| add(v.label()));
    page::Section::ALL.iter().for_each(|v| add(v.label()));
    steps::Step::ALL.iter().for_each(|v| add(v.label()));
    steps::Status::ALL.iter().for_each(|v| add(v.label()));
    steps::Solver::ALL.iter().for_each(|v| add(v.label()));
    tools::Tool::ALL.iter().for_each(|v| add(v.label()));
    for p in navigation::NAV_PRESETS {
        add(p.help.en());
    }
    add("Touchpad");
    navigation::OrbitAround::ALL.iter().for_each(|v| add(v.label()));
    navigation::Navigator::ALL.iter().for_each(|v| add(v.label()));
    ["RIGHT", "LEFT", "TOP", "BOTTOM", "FRONT", "BACK"].iter().for_each(|n| add(n));
    for (_, panels) in ribbon::TABS {
        panels.iter().for_each(|(p, _)| add(p));
    }
    commands::UI_COMMANDS.iter().filter_map(|c| c.label).for_each(&mut add);
    param_labels::VALUE_LABELS.iter().for_each(|(_, l)| add(l));
    for b in emcraft_engine::theme::Base::ALL {
        let mut name = b.id().to_owned();
        if let Some(first) = name.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        add(&name);
    }
    out
}

/// Every text written as `t!("…")` (the windows' and the shared crate's) and `text!("…")` (the
/// engine's steps' checks): what the compiler checked is translated.
fn in_source() -> BTreeSet<String> {
    fn literals(text: &str, out: &mut BTreeSet<String>) {
        let mut rest = text;
        while let Some(i) = rest.find("t!(") {
            let after = rest[i + 3..].trim_start();
            let Some(body) = after.strip_prefix('"') else {
                rest = &rest[i + 3..];
                continue;
            };
            let mut lit = String::new();
            let mut chars = body.char_indices();
            let mut end = body.len();
            while let Some((k, c)) = chars.next() {
                match c {
                    '\\' => {
                        if let Some((_, n)) = chars.next() {
                            lit.push(if n == 'n' { '\n' } else { n });
                        }
                    }
                    '"' => {
                        end = k;
                        break;
                    }
                    c => lit.push(c),
                }
            }
            rest = &body[end..];
            out.insert(lit);
        }
    }
    fn walk(dir: &std::path::Path, out: &mut BTreeSet<String>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                literals(&std::fs::read_to_string(&p).unwrap_or_default(), out);
            }
        }
    }
    let mut out = BTreeSet::new();
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for dir in ["ui-iced/src", "ui-core/src", "engine/src"] {
        walk(&crates.join(dir), &mut out);
    }
    out
}

/// Texts the tour cannot reach, with why.
const UNREACHED: &[(&str, &str)] = &[];

/// Drawn in Japanese, nothing on screen is left in English, and every translation is used.
#[test]
fn a_tour_of_the_window_in_japanese_finds_every_text_translated_and_none_unused() {
    let r = tour();
    let ja = lang::find("ja").unwrap_or_else(|| panic!("no Japanese"));
    let missed: Vec<&String> = r.missed.iter().collect();
    assert!(
        missed.is_empty(),
        "{} texts have no Japanese:\n{}",
        missed.len(),
        missed.iter().map(|m| format!("{m:?}")).collect::<Vec<_>>().join("\n")
    );
    let named = named();
    for n in &named {
        assert!(ja.strings.contains_key(n), "no Japanese for the label `{n}`");
    }
    let source = in_source();
    let unused: Vec<&String> = ja
        .strings
        .keys()
        .filter(|k| !r.asked.contains(*k) && !named.contains(*k) && !source.contains(*k) && !UNREACHED.iter().any(|(u, _)| u == k))
        .collect();
    assert!(
        unused.is_empty(),
        "{} translations are never asked for:\n{}",
        unused.len(),
        unused.iter().map(|m| format!("{m:?}")).collect::<Vec<_>>().join("\n")
    );
}

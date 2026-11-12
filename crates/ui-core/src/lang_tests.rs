//! The translations stay whole, whatever window shows them: every command and sample
//! translated, the placeholders kept, the Japanese in its style, the language the thread's own.
//! (A tour of the window in Japanese, finding no English left and no translation unused, is the
//! window's: `emcraft-ui-iced`'s `lang_tests`.)

use std::collections::BTreeSet;

use crate::lang::*;

fn japanese() -> &'static Language {
    find("ja").unwrap_or_else(|| panic!("no Japanese: {:?}", problems()))
}

#[test]
fn the_built_in_languages_load() {
    assert_eq!(problems(), &[] as &[String]);
    assert_eq!(ids(), ["en", "ja"]);
    assert_eq!(japanese().name, "Japanese");
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
    // Every template parameter's label translated, and every translation one's.
    let labels: BTreeSet<String> = emcraft_engine::templates::TEMPLATES
        .iter()
        .flat_map(|t| emcraft_engine::template_parameters(t.name).unwrap_or_default())
        .filter_map(|p| p.label)
        .collect();
    for l in &labels {
        assert!(ja.parameters.get(l).is_some_and(|x| !x.is_empty()), "parameter label `{l}`: no Japanese");
    }
    for l in ja.parameters.keys() {
        assert!(labels.contains(l), "parameter label `{l}`: no template has it");
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
    texts.extend(ja.parameters.iter().map(|(k, v)| (format!("parameter {k}"), v.as_str(), false)));
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
fn the_language_is_the_threads_own() {
    assert!(set("ja"));
    assert_eq!((current(), tr(t!("Settings"))), ("ja", "設定"));
    std::thread::spawn(|| assert_eq!((current(), tr(t!("Settings"))), ("en", "Settings"))).join().unwrap();
    assert_eq!(command_label("mesh.generate"), japanese().commands["mesh.generate"].label);
    assert!(!set("klingon"));
    assert_eq!(Some(command_label("mesh.generate")), emcraft_engine::find_command("mesh.generate").map(|c| c.label));
    assert_eq!(current(), "en");
    assert_eq!(param_doc("no.such", "x"), "");
}

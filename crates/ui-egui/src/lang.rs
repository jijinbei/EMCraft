//! The window's languages (docs/languages.md): English, the source text and the default, and
//! the translations compiled in (`crates/ui-egui/locales/<id>.json`, data in the spirit of the
//! built-in themes). Text is looked up by its English, as gettext does, where it leaves for the
//! screen: the window's text helpers ([`crate::UiExt::text`], `act`'s hint, the `ui_kit`
//! controls) take a [`Shown`] — a `&str` is English to look up, a `String` (or [`Raw`]) is data
//! shown as it is. [`tr`] gives the current language's text for an English one, the English
//! itself when there is none; [`trf`] fills named placeholders (`{n}`) after the lookup, so a
//! translation may put them in its own order; [`shown`] reads a `named_enum!`'s label. The
//! engine's commands (their labels, descriptions and parameters' descriptions) and the samples
//! are translated by id ([`command_label`], [`param_doc`], [`sample_label`]); the engine's table
//! itself stays English.
//!
//! What stays English whatever the language: the engine's messages and errors, the text drawn
//! inside pictures (charts and field plots, set in DejaVu Sans, which has no Japanese), the
//! control channel's names (handles are made from the English) and `emcraft-cli`.
//!
//! The current language belongs to the thread drawing the frame ([`set`], [`current`]), as the
//! theme does, so tests drawing apps on threads of their own keep their own.
//!
//! ```
//! use emcraft_ui_egui::lang::{self, tr, trf};
//!
//! assert_eq!(lang::current(), "en");
//! assert_eq!(tr("Settings"), "Settings");
//! assert!(lang::set("ja"));
//! assert_eq!(tr("Settings"), "設定");
//! assert_eq!(trf("{n} projects", &[("n", &3)]), "3 件のプロジェクト");
//! // Text without a translation reads as its English; unknown languages are refused.
//! assert_eq!(tr("no such text"), "no such text");
//! assert!(!lang::set("xx"));
//! assert_eq!(lang::current(), "en");
//! ```

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::fmt::{Display, Write};
use std::sync::OnceLock;

use serde::Deserialize;

/// The source language, and the default.
pub const ENGLISH: &str = "en";

/// The translations compiled in: each language's id and file.
const BUILT_IN: &[(&str, &str)] = &[("ja", include_str!("../locales/ja.json"))];

/// Longest language id read from preferences or requests.
pub const MAX_ID: usize = 16;

/// A translation file: its language's name in that language, its strings by their English,
/// the commands' texts by command id, the samples' descriptions by sample name.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    name: String,
    #[serde(default)]
    strings: HashMap<String, String>,
    #[serde(default)]
    commands: HashMap<String, CommandText>,
    #[serde(default)]
    samples: HashMap<String, String>,
}

/// A command's texts in a language.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandText {
    pub label: String,
    #[serde(default)]
    pub doc: String,
    /// Each parameter's description, by the parameter's name.
    #[serde(default)]
    pub params: HashMap<String, String>,
}

/// A language the window can be shown in.
#[derive(Debug, Default)]
pub struct Language {
    pub id: &'static str,
    /// Its name in itself ("English", "日本語").
    pub name: String,
    pub strings: HashMap<String, String>,
    pub commands: HashMap<String, CommandText>,
    pub samples: HashMap<String, String>,
}

/// The languages there are, English first, and why a built-in one was left out.
struct Catalog {
    list: Vec<Language>,
    problems: Vec<String>,
}

fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut list = vec![Language { id: ENGLISH, name: "English".into(), ..Language::default() }];
        let mut problems = Vec::new();
        for &(id, text) in BUILT_IN {
            match serde_json::from_str::<File>(text) {
                Ok(f) => list.push(Language { id, name: f.name, strings: f.strings, commands: f.commands, samples: f.samples }),
                Err(e) => problems.push(format!("language `{id}` left out: {e}")),
            }
        }
        Catalog { list, problems }
    })
}

thread_local! {
    /// The index of the thread's language in the catalogue (0: English).
    static CURRENT: Cell<usize> = const { Cell::new(0) };
    /// What the lookups were asked, while a test records them.
    static RECORD: RefCell<Option<Record>> = const { RefCell::new(None) };
}

/// The texts looked up while recording ([`record`]): every English asked, and those the
/// current language had no text for (text with letters outside its placeholders only).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub asked: BTreeSet<String>,
    pub missed: BTreeSet<String>,
}

/// Starts recording the thread's lookups (`true`), or stops and gives what was recorded.
pub fn record(on: bool) -> Record {
    RECORD.with_borrow_mut(|r| {
        let done = r.take().unwrap_or_default();
        if on {
            *r = Some(Record::default());
        }
        done
    })
}

/// Whether `english` is text to translate: letters outside its placeholders.
fn is_text(english: &str) -> bool {
    let mut depth = 0;
    english.chars().any(|c| {
        match c {
            '{' => depth += 1,
            '}' => depth = (depth - 1).max(0),
            _ => {}
        }
        depth == 0 && c.is_alphabetic()
    })
}

/// The languages there are, English first.
pub fn languages() -> &'static [Language] {
    &catalog().list
}

/// The ids of the languages there are, English first.
pub fn ids() -> Vec<&'static str> {
    languages().iter().map(|l| l.id).collect()
}

/// Why a built-in language was left out (none, unless its file is broken).
pub fn problems() -> &'static [String] {
    &catalog().problems
}

/// The language `id`.
pub fn find(id: &str) -> Option<&'static Language> {
    languages().iter().find(|l| l.id == id)
}

/// Makes `id` the thread's language; an unknown one is refused (`false`) and English used.
pub fn set(id: &str) -> bool {
    let i = languages().iter().position(|l| l.id == id);
    CURRENT.with(|c| c.set(i.unwrap_or(0)));
    i.is_some()
}

/// The thread's language's id.
pub fn current() -> &'static str {
    languages().get(CURRENT.with(Cell::get)).map_or(ENGLISH, |l| l.id)
}

/// The thread's language, unless it is English.
fn translation() -> Option<&'static Language> {
    match CURRENT.with(Cell::get) {
        0 => None,
        i => languages().get(i),
    }
}

/// The current language's text for `english`, or `english` when it has none.
pub fn tr(english: &str) -> &str {
    let lang = translation();
    let found = lang.and_then(|l| l.strings.get(english));
    RECORD.with_borrow_mut(|r| {
        if let Some(r) = r.as_mut()
            && !english.is_empty()
        {
            r.asked.insert(english.to_owned());
            if lang.is_some() && found.is_none() && is_text(english) {
                r.missed.insert(english.to_owned());
            }
        }
    });
    found.map_or(english, String::as_str)
}

/// A `named_enum!`'s label in the current language.
pub fn shown<T: emcraft_macros::Named>(x: T) -> &'static str {
    tr(x.label())
}

/// Text on its way to the screen: English to look up (`&str`), or data shown as it is (a
/// `String`, a `&String`, [`Raw`]).
pub trait Shown {
    /// The text the current language shows.
    fn shown(&self) -> &str;
}

impl Shown for &str {
    fn shown(&self) -> &str {
        tr(self)
    }
}

impl Shown for String {
    fn shown(&self) -> &str {
        self
    }
}

impl Shown for &String {
    fn shown(&self) -> &str {
        self
    }
}

/// Text that is data (a name, a number, an engine's message): shown as it is.
#[derive(Clone, Copy, Debug)]
pub struct Raw<'a>(pub &'a str);

impl Shown for Raw<'_> {
    fn shown(&self) -> &str {
        self.0
    }
}

/// The values of a text's placeholders, by name, for [`trf`], [`both`] and
/// `EmcraftApp::info_with`: `args!(n = count, path = p)`.
macro_rules! args {
    ($($name:ident = $value:expr),* $(,)?) => {
        &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),*]
    };
}
pub(crate) use args;

/// [`tr`] with its placeholders filled: each `{name}` by the value given for `name`.
pub fn trf(english: &str, args: &[(&str, &dyn Display)]) -> String {
    fill(tr(english), args)
}

/// [`trf`] of `one` when `n` is 1, else of `many` (English's singular and plural; a language
/// without plurals translates both alike). Both are looked up whichever is shown, so the checks
/// ask for a translation of each.
///
/// ```
/// use emcraft_ui_egui::lang::plural;
///
/// assert_eq!(plural(1, "{n} source", "{n} sources", &[("n", &1)]), "1 source");
/// assert_eq!(plural(3, "{n} source", "{n} sources", &[("n", &3)]), "3 sources");
/// ```
pub fn plural(n: usize, one: &str, many: &str, args: &[(&str, &dyn Display)]) -> String {
    trf(pick(n, one, many), args)
}

/// The English of `one` when `n` is 1, else of `many`, both looked up (see [`plural`]): for a
/// text built by hand ([`both`]).
pub fn pick<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    let _ = (tr(one), tr(many));
    if n == 1 { one } else { many }
}

/// A text in English and as the current language shows it, placeholders filled in both: the
/// English names what is shown (a browser row's handle), the other is drawn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Both {
    pub en: String,
    pub shown: String,
}

/// [`trf`], with the English filled too.
pub fn both(english: &str, args: &[(&str, &dyn Display)]) -> Both {
    let en = fill(english, args);
    let shown = match translation() {
        Some(_) => trf(english, args),
        None => en.clone(),
    };
    Both { en, shown }
}

/// `text` with each `{name}` that `args` gives replaced by its value; other braces kept. One
/// pass: a value's own braces are not read again.
pub fn fill(text: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let value = after.find('}').and_then(|close| {
            let name = &after[..close];
            args.iter().find(|(n, _)| *n == name).map(|(_, v)| (close, v))
        });
        match value {
            Some((close, v)) => {
                let _ = write!(out, "{v}");
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The placeholders of `text`: each `{name}` (letters, digits and `_`), in order.
pub fn placeholders(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if close > 0 && after[..close].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
                out.push(&after[..close]);
                rest = &after[close + 1..];
            }
            _ => rest = after,
        }
    }
    out
}

/// The current language's texts of command `id`.
fn command(id: &str) -> Option<&'static CommandText> {
    translation().and_then(|l| l.commands.get(id))
}

/// A command's label in the current language (the engine's, or the id for an unknown one).
pub fn command_label(id: &str) -> &str {
    match command(id) {
        Some(c) if !c.label.is_empty() => &c.label,
        _ => emcraft_engine::find_command(id).map_or(id, |c| c.label),
    }
}

/// A command's description in the current language (the engine's; empty for an unknown one).
pub fn command_doc(id: &str) -> &'static str {
    match command(id) {
        Some(c) if !c.doc.is_empty() => &c.doc,
        _ => emcraft_engine::find_command(id).map_or("", |c| c.doc),
    }
}

/// A command's parameter's description in the current language (the engine's; empty for an
/// unknown one).
pub fn param_doc(id: &str, name: &str) -> &'static str {
    match command(id).and_then(|c| c.params.get(name)) {
        Some(d) if !d.is_empty() => d,
        _ => emcraft_engine::find_command(id).and_then(|c| c.params.iter().find(|p| p.name == name)).map_or("", |p| p.doc),
    }
}

/// A sample's description in the current language (the engine's; empty for an unknown one).
pub fn sample_label(name: &str) -> &'static str {
    match translation().and_then(|l| l.samples.get(name)) {
        Some(s) if !s.is_empty() => s,
        _ => emcraft_engine::templates::find(name).map_or("", |s| s.label),
    }
}

/// `text` cut to [`MAX_ID`] characters, for an id read from outside.
pub fn clip_id(text: &str) -> String {
    text.chars().take(MAX_ID).collect()
}

#[cfg(test)]
mod tests;

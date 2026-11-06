//! The window's fonts: egui's own, then a font for what they lack in the window's language —
//! the Japanese translation's kana and kanji — found among the system's fonts ([`system`]) or
//! in a font file the user chose (Settings ▸ Language). Nothing is embedded: where no font
//! shows the language, the window stays in English and says so, with what to install, until
//! one does ([`State::Missing`]).
//!
//! The tests draw with a subset of Noto Sans JP (`crates/ui-egui/fonts/`, SIL OFL 1.1, made by
//! `cargo xtask font-subset`) and never the machine's fonts, so the headless scenarios and
//! `ui.render`'s pictures are the same on every machine; it is not in the app.

pub mod system;

use std::path::PathBuf;
use std::sync::Arc;

/// The two families a language's font serves.
const FAMILIES: [egui::FontFamily; 2] = [egui::FontFamily::Proportional, egui::FontFamily::Monospace];

/// Noto Sans JP Regular, subset (`cargo xtask font-subset`): the tests' font.
#[cfg(test)]
pub(crate) static NOTO_SANS_JP_OTF: &[u8] = include_bytes!("../fonts/NotoSansJP-Regular-subset.otf");

/// Where the window's language stands for a font.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// English, or a language egui's fonts show.
    NotNeeded,
    /// Shown with this family.
    Found(String),
    /// No font shows it: this many of its characters are missing in the best one looked at.
    Missing(usize),
}

/// The window's fonts: the fonts there are, the language they were chosen for and how.
#[derive(Default)]
pub struct Fonts {
    library: Option<system::Library>,
    /// The font file the library was loaded with.
    file: Option<PathBuf>,
    /// The language the fonts were chosen for, how, and egui's definitions with them.
    chosen: Option<(String, State, Arc<egui::FontDefinitions>)>,
    /// The language whose missing font was reported.
    reported: Option<String>,
}

impl Fonts {
    /// The language's standing, once its fonts were chosen.
    pub fn state(&self, language: &str) -> Option<&State> {
        self.chosen.as_ref().filter(|(l, _, _)| l == language).map(|(_, s, _)| s)
    }

    /// Whether the window can be drawn in `language`: English, or its font found. The tests'
    /// font shows every language.
    pub fn shows(&mut self, language: &str) -> bool {
        if language == crate::lang::ENGLISH || cfg!(test) {
            return true;
        }
        self.choose(language);
        !matches!(self.state(language), Some(State::Missing(_)))
    }

    /// The font file the user chose (none: the system's fonts only); the fonts are looked for
    /// again.
    pub fn set_file(&mut self, file: Option<PathBuf>) {
        if self.file != file {
            self.file = file;
            self.library = None;
            self.chosen = None;
        }
    }

    /// Looks for the fonts again (fonts installed meanwhile).
    pub fn search_again(&mut self) {
        self.library = None;
        self.chosen = None;
        self.reported = None;
    }

    /// Whether `language`'s missing font is still to be reported (once).
    pub fn report(&mut self, language: &str) -> bool {
        let first = self.reported.as_deref() != Some(language);
        self.reported = Some(language.to_owned());
        first
    }

    /// Whether the chosen font file could be read.
    pub fn file_loaded(&self) -> Option<bool> {
        self.file.as_ref().map(|_| self.library.as_ref().is_some_and(system::Library::chosen_loaded))
    }

    /// Chooses the fonts for `language` (the system's fonts loaded the first time).
    fn choose(&mut self, language: &str) {
        if self.chosen.as_ref().is_some_and(|(l, _, _)| l == language) {
            return;
        }
        let needed = crate::lang::script_chars(language);
        let (state, defs) = if cfg!(test) {
            (State::Found("Noto Sans JP (tests)".into()), tests_fonts())
        } else if needed.is_empty() {
            (State::NotNeeded, egui::FontDefinitions::default())
        } else {
            let file = self.file.clone();
            let library = self.library.get_or_insert_with(|| system::Library::system(file.as_deref()));
            match library.find(&needed) {
                Ok(found) => (State::Found(found.family.clone()), with_installed(found)),
                Err(missing) => (State::Missing(missing), egui::FontDefinitions::default()),
            }
        };
        self.chosen = Some((language.to_owned(), state, Arc::new(defs)));
    }

    /// Gives `ctx` the fonts of `language` (chosen first if need be); cheap when they are set.
    pub fn install(&mut self, ctx: &egui::Context, language: &str) {
        self.choose(language);
        let Some((_, _, defs)) = &self.chosen else { return };
        let key = egui::Id::new("emcraft_fonts");
        let set = ctx.data(|d| d.get_temp::<usize>(key));
        let mine = Arc::as_ptr(defs) as usize;
        if set != Some(mine) {
            ctx.set_fonts((**defs).clone());
            ctx.data_mut(|d| d.insert_temp(key, mine));
        }
    }

    /// The family in use for `language`, for `ui.inspect`.
    pub fn in_use(&self, language: &str) -> String {
        match self.state(language) {
            Some(State::Found(f)) => f.clone(),
            Some(State::Missing(n)) => format!("missing ({n} characters)"),
            Some(State::NotNeeded) | None => "none needed".into(),
        }
    }
}

/// egui's fonts, then the face found, in both families.
fn with_installed(found: system::Found) -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    let key = format!("{} (installed)", found.family);
    let data = egui::FontData { font: std::borrow::Cow::Owned(found.bytes), index: found.index, tweak: Default::default() };
    defs.font_data.insert(key.clone(), Arc::new(data));
    for family in FAMILIES {
        defs.families.entry(family).or_default().push(key.clone());
    }
    defs
}

/// egui's fonts and the tests' Japanese font.
#[cfg(test)]
fn tests_fonts() -> egui::FontDefinitions {
    with_installed(system::Found { family: "Noto Sans JP".into(), index: 0, bytes: NOTO_SANS_JP_OTF.to_vec() })
}

#[cfg(not(test))]
fn tests_fonts() -> egui::FontDefinitions {
    egui::FontDefinitions::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character of the Japanese translation that Noto Sans JP has is in the tests'
    /// subset.
    #[test]
    fn the_subset_holds_every_character_of_the_translation() {
        let face = ttf_parser::Face::parse(NOTO_SANS_JP_OTF, 0).unwrap();
        let full_bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/fonts/full/NotoSansJP-Regular.otf")).unwrap();
        let full = ttf_parser::Face::parse(&full_bytes, 0).unwrap();
        let has = |f: &ttf_parser::Face, c: char| f.glyph_index(c).is_some_and(|g| g.0 != 0);
        let ja = include_str!("../../../locales/ja.json");
        let mut missing: Vec<char> = ja.chars().filter(|c| !c.is_ascii() && has(&full, *c) && !has(&face, *c)).collect();
        missing.sort_unstable();
        missing.dedup();
        assert!(missing.is_empty(), "not in the subset (run cargo xtask font-subset): {}", missing.iter().collect::<String>());
    }

    /// A face found goes after egui's own fonts, in both families.
    #[test]
    fn a_found_font_comes_after_eguis_own() {
        let defs = with_installed(system::Found { family: "Some Gothic".into(), index: 0, bytes: NOTO_SANS_JP_OTF.to_vec() });
        for family in FAMILIES {
            let list = &defs.families[&family];
            let n = list.len();
            assert_eq!(list[n - 1], "Some Gothic (installed)");
            assert_eq!(list[..n - 1], egui::FontDefinitions::default().families[&family][..]);
        }
        let mut f = Fonts::default();
        assert!(f.shows("ja") && f.shows("en"));
    }
}

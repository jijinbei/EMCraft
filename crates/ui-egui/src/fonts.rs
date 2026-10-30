//! The window's fonts: egui's own, then a Japanese font for what they lack — the Japanese
//! translation, and Japanese file and project names in any language — as a fallback in both
//! the proportional and the monospace family.
//!
//! The desktop window looks for a Japanese font installed on the system ([`system`]) on a
//! thread at start ([`Japanese::look_for_installed`]) and uses it once found, before the
//! embedded one. The embedded font is a subset of Noto Sans JP (`crates/ui-egui/fonts/`, SIL OFL
//! 1.1, made by `cargo xtask font-subset`): kana, JIS X 0208's symbols and first-level kanji, and
//! every character of the Japanese translation. It is installed in every context the app draws
//! in; the headless harness and `ui.render` use it alone, so their pictures are the same on
//! every machine.

pub mod system;

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};

/// The embedded fallback's name among the context's fonts.
pub const NOTO_SANS_JP: &str = "Noto Sans JP";

/// What `ui.inspect` reports when the embedded font is the Japanese one in use.
pub const EMBEDDED: &str = "embedded subset";

/// Noto Sans JP Regular, subset (`cargo xtask font-subset`).
pub(crate) static NOTO_SANS_JP_OTF: &[u8] = include_bytes!("../fonts/NotoSansJP-Regular-subset.otf");

/// The two families the fallbacks serve.
const FAMILIES: [egui::FontFamily; 2] = [egui::FontFamily::Proportional, egui::FontFamily::Monospace];

/// Adds the embedded fallback to `ctx`'s fonts (from its next pass); a context that has it is
/// left as it is, so this is cheap to call every frame.
pub fn install(ctx: &egui::Context) {
    let families = FAMILIES.into_iter().map(|family| InsertFontFamily { family, priority: FontPriority::Lowest }).collect();
    ctx.add_font(FontInsert::new(NOTO_SANS_JP, egui::FontData::from_static(NOTO_SANS_JP_OTF), families));
}

/// A window's Japanese font: the embedded one, and the installed one once found.
#[derive(Default)]
pub struct Japanese {
    /// The installed family in use.
    installed: Option<String>,
    /// The search under way.
    search: Option<Receiver<Option<system::Found>>>,
}

impl Japanese {
    /// Starts looking for an installed Japanese font on a thread of its own; `ctx` is asked to
    /// draw again when the search ends.
    pub fn look_for_installed(&mut self, ctx: &egui::Context) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let started = std::thread::Builder::new().name("emcraft-fonts".into()).spawn(move || {
            let _ = tx.send(system::find());
            ctx.request_repaint();
        });
        if started.is_ok() {
            self.search = Some(rx);
        }
    }

    /// Each frame: the embedded fallback, and the installed font once the search found one.
    pub fn install(&mut self, ctx: &egui::Context) {
        install(ctx);
        let Some(rx) = &self.search else { return };
        match rx.try_recv() {
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) | Ok(None) => self.search = None,
            Ok(Some(found)) => {
                self.search = None;
                ctx.set_fonts(with_installed(found.family.clone(), found.bytes, found.index));
                self.installed = Some(found.family);
            }
        }
    }

    /// The Japanese font in use: the installed family's name, or [`EMBEDDED`].
    pub fn in_use(&self) -> &str {
        self.installed.as_deref().unwrap_or(EMBEDDED)
    }
}

/// egui's fonts, then the installed `family` (face `index` of `bytes`), then the embedded one.
fn with_installed(family: String, bytes: Vec<u8>, index: u32) -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    // Its own key, so a system font of the embedded one's name does not replace it.
    let key = format!("{family} (installed)");
    let data = egui::FontData { font: std::borrow::Cow::Owned(bytes), index, tweak: Default::default() };
    defs.font_data.insert(key.clone(), Arc::new(data));
    defs.font_data.insert(NOTO_SANS_JP.to_owned(), Arc::new(egui::FontData::from_static(NOTO_SANS_JP_OTF)));
    for family in FAMILIES {
        let list = defs.families.entry(family).or_default();
        list.push(key.clone());
        list.push(NOTO_SANS_JP.to_owned());
    }
    defs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character of the Japanese translation that Noto Sans JP has is in the embedded
    /// subset.
    #[test]
    fn the_subset_holds_every_character_of_the_translation() {
        let face = ttf_parser::Face::parse(NOTO_SANS_JP_OTF, 0).unwrap();
        let full_bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/fonts/full/NotoSansJP-Regular.otf")).unwrap();
        let full = ttf_parser::Face::parse(&full_bytes, 0).unwrap();
        let has = |f: &ttf_parser::Face, c: char| f.glyph_index(c).is_some_and(|g| g.0 != 0);
        let ja = include_str!("../locales/ja.json");
        let mut missing: Vec<char> = ja.chars().filter(|c| !c.is_ascii() && has(&full, *c) && !has(&face, *c)).collect();
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "not in the subset (run cargo xtask font-subset): {}", missing.iter().collect::<String>());
        // Kana and a first-level kanji beyond the translation.
        for c in ['ぁ', 'ゖ', 'ァ', 'ヶ', 'ｱ', '亜', '腕', '・', '〜', '～'] {
            assert!(face.glyph_index(c).is_some_and(|g| g.0 != 0), "{c}");
        }
    }

    /// An installed font goes after egui's own and before the embedded one, in both families.
    #[test]
    fn an_installed_font_comes_before_the_embedded_one() {
        let defs = with_installed("Some Gothic".into(), NOTO_SANS_JP_OTF.to_vec(), 0);
        for family in FAMILIES {
            let list = &defs.families[&family];
            let n = list.len();
            assert_eq!(&list[n - 2..], ["Some Gothic (installed)", NOTO_SANS_JP]);
            assert_eq!(list[..n - 2], egui::FontDefinitions::default().families[&family][..]);
        }
        assert_eq!(Japanese::default().in_use(), EMBEDDED);
    }
}

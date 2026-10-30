//! The window's fonts: egui's own, then Noto Sans JP (`crates/ui-egui/fonts/`, SIL OFL 1.1) for
//! what they lack — the Japanese translation, and Japanese file and project names in any
//! language. It is a fallback in both the proportional and the monospace family, installed in
//! every context the app draws in (the desktop window, the headless harness, `ui.render`).

use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};

/// The fallback's name among the context's fonts.
pub const NOTO_SANS_JP: &str = "Noto Sans JP";

/// Noto Sans JP Regular (the noto-cjk SubsetOTF JP build, unmodified).
static NOTO_SANS_JP_OTF: &[u8] = include_bytes!("../fonts/NotoSansJP-Regular.otf");

/// Adds the fallback to `ctx`'s fonts (from its next pass); a context that has it is left as
/// it is, so this is cheap to call every frame.
pub fn install(ctx: &egui::Context) {
    let families = [egui::FontFamily::Proportional, egui::FontFamily::Monospace]
        .into_iter()
        .map(|family| InsertFontFamily { family, priority: FontPriority::Lowest })
        .collect();
    ctx.add_font(FontInsert::new(NOTO_SANS_JP, egui::FontData::from_static(NOTO_SANS_JP_OTF), families));
}

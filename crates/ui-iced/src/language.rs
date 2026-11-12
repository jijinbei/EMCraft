//! The window's language and the font that shows it, as the egui window's
//! ([`emcraft_ui_core::fonts`]): the preferred language shown when a font has its characters —
//! the system's, or a font file chosen in Settings — else the window in English, said once, with
//! what to install. The face found also sets the pictures' text (charts and labels drawn on the
//! CPU: [`emcraft_render::text::set_fallback`]) and, read from a chosen file, is given to iced.

use emcraft_ui_core::fonts::{self, Found, State};
use emcraft_ui_core::lang::{self, args, trf};

use crate::app::App;

/// The font for the preferred language (`font_file` first): how it stands and the face found,
/// the pictures' text set with it and the window's language set to the one shown.
pub fn chosen(language: &str, font_file: Option<&str>) -> (State, Option<Found>) {
    let (state, found) = fonts::choose(language, font_file.map(std::path::Path::new));
    if let Some(f) = &found {
        emcraft_render::text::set_fallback(Some((f.bytes.clone(), f.index)));
    }
    lang::set(if matches!(state, State::Missing(_)) { lang::ENGLISH } else { language });
    (state, found)
}

impl App {
    /// The window with its language's font standing `state`: a missing one said (once a
    /// language).
    pub fn with_font(mut self, state: State) -> App {
        self.said_font(&state);
        self.font = state;
        self
    }

    /// The font looked for again (the language or the font file changed, or fonts were
    /// installed): the face to give iced, when one was read from a chosen file.
    pub(crate) fn use_language(&mut self) -> Option<Vec<u8>> {
        let (state, found) = chosen(self.prefs.language(), self.prefs.font_file.as_deref());
        self.said_font(&state);
        self.font = state;
        found.filter(|_| self.prefs.font_file.is_some()).map(|f| f.bytes)
    }

    /// A missing font said, once for each language.
    fn said_font(&mut self, state: &State) {
        let wanted = self.prefs.language().to_owned();
        let State::Missing(n) = state else { return };
        if self.font_said.as_deref() == Some(wanted.as_str()) {
            return;
        }
        let name = lang::find(&wanted).map_or_else(|| wanted.clone(), |l| l.name.clone());
        self.fail(trf(
            t!("No font shows {language} ({n} characters missing): the window stays in English. Install one (Linux: fonts-noto-cjk) or choose a font file in Settings ▸ Language."),
            args!(language = name, n = n),
        ));
        self.font_said = Some(wanted);
    }
}

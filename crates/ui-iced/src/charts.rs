//! The charts the window shows as pictures (the engine draws them): each kept with what it was
//! drawn from and drawn again only when that changes, so iced keeps one picture on the GPU
//! instead of a new one every frame (a large picture is uploaded over several frames).

use emcraft_engine::Session;
use emcraft_engine::view::SystemChart;
use emcraft_render::chart::Chart;
use iced::widget::image::Handle;

use crate::look::Look;

/// The S-parameters chart's size in pixels, and the system chart's (each under the 2 MiB iced
/// uploads at once).
const SPARAMETERS: [usize; 2] = [1100, 470];
const SYSTEM: [usize; 2] = [1100, 320];

/// What a chart was drawn from: the window's generation of the project, the theme, the language and which chart.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    generation: u64,
    theme: String,
    language: &'static str,
    which: Option<SystemChart>,
}

/// A chart as drawn, or why it could not be.
#[derive(Clone, Debug)]
pub struct Picture {
    key: Key,
    pub shown: Result<Handle, String>,
}

/// The window's charts.
#[derive(Clone, Debug, Default)]
pub struct Charts {
    pub sparameters: Option<Picture>,
    pub system: Option<Picture>,
}

/// `chart` drawn `size` pixels big in `look`'s palette, its words in the window's language.
fn drawn(chart: emcraft_engine::Result<Chart>, size: [usize; 2], look: &Look) -> Result<Handle, String> {
    let mut c = chart.map_err(|e| e.to_string())?;
    emcraft_ui_core::chart_words::translate_chart(&mut c);
    let canvas = emcraft_render::chart::draw(&c, size[0], size[1], &look.theme.palette);
    Ok(Handle::from_rgba(canvas.w as u32, canvas.h as u32, canvas.rgba))
}

/// `old` kept when it was drawn from `key`, else drawn again by `draw`.
fn kept(old: Option<Picture>, key: Key, draw: impl FnOnce() -> Result<Handle, String>) -> Picture {
    match old {
        Some(p) if p.key == key => p,
        _ => Picture { shown: draw(), key },
    }
}

impl Charts {
    /// The charts of `session` at `generation` ([`crate::app::App`]'s) in `look` (the system's
    /// `which`), drawn again where what they show changed.
    pub fn refreshed(self, session: &Session, generation: u64, look: &Look, which: SystemChart) -> Charts {
        let key = |which| Key { generation, theme: look.theme.name.clone(), language: emcraft_ui_core::lang::current(), which };
        let sparameters = kept(self.sparameters, key(None), || drawn(emcraft_engine::view::sparameter_chart(session), SPARAMETERS, look));
        let system = session
            .results()
            .system
            .as_ref()
            .map(|_| kept(self.system, key(Some(which)), || drawn(emcraft_engine::view::system_chart(session, which), SYSTEM, look)));
        Charts { sparameters: Some(sparameters), system }
    }
}

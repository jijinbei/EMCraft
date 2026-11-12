//! The charts the window shows: each the engine's chart (its words in the window's language),
//! kept with what it was made from and made again only when that changes; the GPU draws it as
//! vectors at the widget's size ([`crate::chart`]).

use emcraft_engine::Session;
use emcraft_engine::view::SystemChart;
use emcraft_engine::views::ChartId;
use emcraft_render::chart::Chart;
use emcraft_ui_core::view::View;

use crate::look::Look;

/// What a chart was drawn from: the window's generation of the project, the theme, the language and which chart.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    generation: u64,
    theme: String,
    language: &'static str,
    which: Option<SystemChart>,
    page: Option<View>,
}

/// A chart as made, or why it could not be.
#[derive(Clone, Debug)]
pub struct Picture {
    key: Key,
    pub shown: Result<Chart, String>,
}

/// The window's charts: the result page's shown (the engine's chart that page is) and the
/// system's.
#[derive(Clone, Debug, Default)]
pub struct Charts {
    pub page: Option<Picture>,
    pub system: Option<Picture>,
}

/// `chart` (the engine's `id`) in the style the project keeps for it (`view.chart_style`), its
/// words in the window's language.
fn translated(session: &Session, id: ChartId, chart: emcraft_engine::Result<Chart>) -> Result<Chart, String> {
    let mut c = emcraft_engine::views::style_chart(session, id, chart.map_err(|e| e.to_string())?);
    emcraft_ui_core::chart_words::translate_chart(&mut c);
    Ok(c)
}

/// `old` kept when it was drawn from `key`, else drawn again by `draw`.
fn kept(old: Option<Picture>, key: Key, draw: impl FnOnce() -> Result<Chart, String>) -> Picture {
    match old {
        Some(p) if p.key == key => p,
        _ => Picture { shown: draw(), key },
    }
}

impl Charts {
    /// The charts of `session` at `generation` ([`crate::app::App`]'s) in `look` for result page
    /// `page` (the system's `which`), made again where what they show changed; only the page shown
    /// is made.
    pub fn refreshed(self, session: &Session, generation: u64, look: &Look, which: SystemChart, page: View) -> Charts {
        let key = |which, page| Key { generation, theme: look.theme.name.clone(), language: emcraft_ui_core::lang::current(), which, page };
        let shown =
            page.chart().map(|id| kept(self.page, key(None, Some(page)), || translated(session, id, emcraft_engine::views::chart_of(session, id))));
        let system = session.results().system.as_ref().map(|_| {
            kept(self.system, key(Some(which), None), || translated(session, ChartId::System, emcraft_engine::view::system_chart(session, which)))
        });
        Charts { page: shown, system }
    }
}

//! A table of text ([`emcraft_ui_core::results::Table`]): its caption, its headers, its rows
//! with their numbers right-aligned under them in the monospace face, a column of marks when
//! its rows are marked (◆ a row solved in full), every other row shaded.

use emcraft_ui_core::results::Table;
use iced::widget::{Column, container, row, scrollable, text};
use iced::{Element, Font, Widget, alignment};

use crate::app::Message;
use crate::look::{self, Tokens};

/// A character's width as a share of the monospace face's size.
const CHAR_W: f32 = 0.62;
/// Most rows drawn (a long sweep's table scrolls past them in the window's own scroll).
const MOST_ROWS: usize = 2000;

/// How many ems `s` takes: a wide (CJK) character a whole one, any other [`CHAR_W`].
fn ems(s: &str) -> f32 {
    s.chars().map(|c| if c >= '\u{2E80}' { 1.0 } else { CHAR_W }).sum()
}

/// `table` in the window's tokens.
pub fn view<'a>(t: Tokens, table: &Table) -> Element<'a, Message> {
    let size = t.font_small;
    let widths: Vec<f32> = (0..table.headers.len())
        .map(|c| {
            let cells = std::iter::once(&table.headers[c]).chain(table.rows.iter().filter_map(|r| r.get(c)));
            cells.map(|s| ems(s)).fold(0.0, f32::max) * size + 14.0
        })
        .collect();
    let mark = |m: Option<bool>| table.marks.as_ref().map(|_| text(if m == Some(true) { "◆" } else { "" }).size(size).width(14).boxed());
    let line = |cells: &[String], head: bool, mark: Option<Element<'a, Message>>| {
        let cells = cells.iter().zip(&widths).map(|(c, w)| {
            text(c.clone())
                .size(size)
                .font(Font::MONOSPACE)
                .color(if head { t.text_dim } else { t.text })
                .width(*w)
                .align_x(alignment::Horizontal::Right)
                .boxed()
        });
        row(mark.into_iter().chain(cells)).boxed()
    };
    let head = line(&table.headers, true, mark(None));
    let rows = table.rows.iter().take(MOST_ROWS).enumerate().map(|(i, r)| {
        let m = table.marks.as_ref().and_then(|m| m.get(i).copied());
        container(line(r, false, mark(m))).style(look::stripe(t, i % 2 == 1)).boxed()
    });
    let caption = table.caption.as_ref().map(|c| text(c.clone()).size(size).color(t.text_dim).boxed());
    let body = Column::with_children(caption.into_iter().chain(std::iter::once(head)).chain(rows)).spacing(t.space_2);
    scrollable(body).direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::default())).boxed()
}

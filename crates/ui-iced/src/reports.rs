//! The crash reports' card over the iced window ([`crate::record`]): each report's kind, when,
//! how often and its message, with Copy (the text of an issue about it, for the person to read
//! and paste: nothing is sent) and Delete; where they are kept; Close.

use emcraft_ui_core::crash::{Kind, Report};
use emcraft_ui_core::lang::{args, tr, trf};
use iced::widget::{Column, button, center, column, container, opaque, row, scrollable, text};
use iced::{Alignment, Color, Element, Fill, Widget};

use crate::app::{App, Message};
use crate::look;
use crate::record::ReportAct;

fn send(a: ReportAct) -> Message {
    Message::Reports(a)
}

/// Days since 1970.
fn today() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400)
}

/// When a report last happened, in words.
fn when(r: &Report) -> String {
    match today().saturating_sub(r.last / 86_400) {
        0 => tr(t!("today")).to_owned(),
        n => trf(t!("{n} days ago"), args!(n = n)),
    }
}

/// The card, while it is open.
pub fn view(app: &App) -> Option<Element<'_, Message>> {
    let reports = app.crashes.shown.as_ref()?;
    let t = app.look.tokens;
    let rows = reports.iter().enumerate().map(|(i, r)| {
        let kind = tr(match r.kind {
            Kind::Panic => t!("Crash"),
            Kind::UncleanExit => t!("Closed unexpectedly"),
            Kind::Report => t!("Report"),
        });
        let times = if r.count > 1 { format!("  ·  ×{}", r.count) } else { String::new() };
        let head =
            row![text(kind).size(t.font_body), text(format!("{}{times}  ·  EMCraft {}", when(r), r.emcraft)).size(t.font_small).color(t.text_dim)]
                .spacing(10)
                .align_y(Alignment::Center);
        let first = emcraft_engine::text::clip(r.message.lines().next().unwrap_or_default(), 160);
        let buttons = row![
            button(text(tr(t!("Copy"))).size(t.font_button)).style(look::button(t, false)).on_press(send(ReportAct::Copy(i))),
            button(text(tr(t!("Delete"))).size(t.font_button)).style(look::quiet(t)).on_press(send(ReportAct::Delete(r.id.clone())))
        ]
        .spacing(8);
        column![head, text(first).size(t.font_small), buttons].spacing(4).boxed()
    });
    let none = reports.is_empty().then(|| text(tr(t!("No reports."))).size(t.font_small).boxed());
    // More than a few scroll.
    let height = if reports.len() > 3 { iced::Length::Fixed(320.0) } else { iced::Length::Shrink };
    let list = scrollable(Column::with_children(none.into_iter().chain(rows)).spacing(12).width(Fill)).height(height);
    let folder = app.crashes.dir.as_ref().map(|d| text(d.display().to_string()).size(t.font_small).color(t.text_dim).boxed());
    let body = Column::with_children(
        [
            text(tr(t!("Crash reports"))).size(t.font_heading).boxed(),
            text(tr(t!("Kept on this computer only. Copy one to report it; nothing is sent."))).size(t.font_small).color(t.text_dim).boxed(),
            list.boxed(),
        ]
        .into_iter()
        .chain(folder)
        .chain(std::iter::once(
            button(text(tr(t!("Close"))).size(t.font_button)).style(look::button(t, true)).on_press(send(ReportAct::Show(false))).boxed(),
        )),
    )
    .spacing(10)
    .width(520);
    let card = container(body).padding(16).style(look::ground(t));
    let veil = container(center(card))
        .width(Fill)
        .height(Fill)
        .style(|_: &iced::Theme| container::Style { background: Some(Color { a: 0.35, ..Color::BLACK }.into()), ..container::Style::default() });
    Some(opaque(veil).boxed())
}

/// The card's handles: `reports:close`, and each report's `reports:copy:<i>` and
/// `reports:delete:<i>`.
pub fn handles(app: &App) -> Vec<(String, Message)> {
    let Some(reports) = app.crashes.shown.as_ref() else { return Vec::new() };
    let each = reports.iter().enumerate().flat_map(|(i, r)| {
        [(format!("reports:copy:{i}"), send(ReportAct::Copy(i))), (format!("reports:delete:{i}"), send(ReportAct::Delete(r.id.clone())))]
    });
    std::iter::once(("reports:close".to_owned(), send(ReportAct::Show(false)))).chain(each).collect()
}

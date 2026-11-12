//! The message area at the window's foot, as the egui window's: the running job's row (Cancel,
//! where it runs, what it does and how far it is), the last message on a line (an error in its
//! colour, a message repeated counted), and the earlier ones a click away (▴ with how many there
//! are opens the newest of them, scrolled to the end; ▾ folds them).

use iced::widget::{Column, button, container, progress_bar, row, scrollable, text};
use iced::{Alignment, Element, Fill, Widget};

use crate::app::{App, Message};
use crate::look;

/// Earlier messages shown when open, and the area's height then (points).
const SHOWN: usize = 40;
const OPEN_HEIGHT: f32 = 96.0;

/// The area.
pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let line = |l: &emcraft_ui_core::messages::Line| text(l.shown()).size(t.font_body).color(if l.error { t.error } else { t.text }).boxed();
    let job = app.job.as_ref().map(|job| {
        let cancel = button(text(crate::run::cancel_label()).size(t.font_button)).style(look::button(t, false)).on_press(Message::Cancel);
        let place = job.place().map(|w| {
            let on = w.gpu() && (w.stepped || w.fronts > 0);
            let chip = text(w.text()).size(t.font_small).color(if on { t.on_accent } else { t.text_dim });
            container(chip).padding([2, 6]).style(look::chip(t, on)).boxed()
        });
        let bar = job.snapshot().fraction().map(|f| progress_bar(0.0..=1.0, f as f32).length(180).girth(8).boxed());
        let parts = [cancel.boxed()].into_iter().chain(place).chain(std::iter::once(text(job.text()).size(t.font_body).boxed())).chain(bar);
        row(parts).spacing(10).align_y(Alignment::Center).boxed()
    });
    let n = app.log.lines().len();
    let toggle = (n > 0).then(|| {
        let mark = if app.log_open { "▾" } else { "▴" };
        button(text(format!("{mark} {n}")).size(t.font_small).color(t.text_dim)).style(look::quiet(t)).on_press(Message::Log(!app.log_open)).boxed()
    });
    let messages: Element<Message> = if app.log_open {
        let earlier = app.log.lines().iter().rev().take(SHOWN).rev().map(line);
        scrollable(Column::with_children(earlier).spacing(2).width(Fill)).anchor_bottom().height(OPEN_HEIGHT).width(Fill).boxed()
    } else {
        let last = app.log.last().map_or_else(|| text("").size(t.font_body).boxed(), line);
        container(last).width(Fill).clip(true).boxed()
    };
    let foot = row(std::iter::once(messages).chain(toggle)).spacing(8).align_y(if app.log_open { Alignment::End } else { Alignment::Center });
    let body = Column::with_children(job.into_iter().chain(std::iter::once(foot.boxed()))).spacing(6);
    container(body).padding([4, 12]).width(Fill).style(look::ground(t)).boxed()
}

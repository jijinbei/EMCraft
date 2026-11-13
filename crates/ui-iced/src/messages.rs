//! The message area at the window's foot: the running job's row (Cancel,
//! where it runs, what it does and how far it is), the last message on a line (an error in its
//! colour, a message repeated counted), and the earlier ones a click away (▴ with how many there
//! are opens the newest of them, scrolled to the end; ▾ folds them); and the toasts over the
//! window's corner, each gone after a few seconds or its ×.

use iced::Task;
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
            container(chip).padding([t.space_2, t.space_6]).style(look::chip(t, on)).boxed()
        });
        let bar = job.snapshot().fraction().map(|f| progress_bar(0.0..=1.0, f as f32).length(180).girth(8).boxed());
        let parts = [cancel.boxed()].into_iter().chain(place).chain(std::iter::once(text(job.text()).size(t.font_body).boxed())).chain(bar);
        row(parts).spacing(t.space_8).align_y(Alignment::Center).boxed()
    });
    let n = app.log.lines().len();
    let toggle = (n > 0).then(|| {
        let mark = if app.log_open { "▾" } else { "▴" };
        button(text(format!("{mark} {n}")).size(t.font_small).color(t.text_dim)).style(look::quiet(t)).on_press(Message::Log(!app.log_open)).boxed()
    });
    let messages: Element<Message> = if app.log_open {
        let earlier = app.log.lines().iter().rev().take(SHOWN).rev().map(line);
        scrollable(Column::with_children(earlier).spacing(t.space_2).width(Fill)).anchor_bottom().height(OPEN_HEIGHT).width(Fill).boxed()
    } else {
        let last = app.log.last().map_or_else(|| text("").size(t.font_body).boxed(), line);
        container(last).width(Fill).clip(true).boxed()
    };
    let foot = row(std::iter::once(messages).chain(toggle)).spacing(t.space_8).align_y(if app.log_open { Alignment::End } else { Alignment::Center });
    let body = Column::with_children(job.into_iter().chain(std::iter::once(foot.boxed()))).spacing(t.space_6);
    container(body).padding([t.space_4, t.space_12]).width(Fill).style(look::ground(t)).boxed()
}

/// The toasts, newest at the bottom, over the window's lower right corner above the message
/// area; none while there are none.
pub fn toasts(app: &App) -> Option<Element<'_, Message>> {
    let t = app.look.tokens;
    let shown = app.toasts.shown();
    if shown.is_empty() {
        return None;
    }
    let cards = shown.iter().map(|toast| {
        let said = text(toast.text.clone()).size(t.font_body).color(if toast.error { t.error } else { t.text }).width(Fill);
        let close = crate::parts::close(t, t!("Close"), Message::Toast(toast.id));
        let (mark, ink) = if toast.error { ("!", t.error) } else { ("✔", t.success) };
        let face = row![text(mark).size(t.font_body).color(ink), said, close].spacing(t.space_8).align_y(Alignment::Center);
        container(face).padding([t.space_8, t.space_12]).width(TOAST_W).style(look::toast(t, toast.error)).boxed()
    });
    let list = Column::with_children(cards).spacing(t.space_6);
    Some(
        container(list)
            .align_right(Fill)
            .align_bottom(Fill)
            .padding(iced::Padding { top: 0.0, right: t.space_16, bottom: TOAST_FOOT, left: 0.0 })
            .boxed(),
    )
}

/// A toast's width, and its room above the window's foot (points).
const TOAST_W: f32 = 340.0;
const TOAST_FOOT: f32 = 52.0;

/// Toast `id` taken away after its seconds (a pool thread waits them out).
pub fn expire(id: u64) -> Task<Message> {
    Task::perform(async { std::thread::sleep(std::time::Duration::from_secs(emcraft_ui_core::toast::SECONDS)) }, move |()| Message::Toast(id))
}

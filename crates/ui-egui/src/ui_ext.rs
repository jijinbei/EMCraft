//! What every control of the window does besides being drawn: it is published under its
//! [`Handle`] (for scenarios and the control channel), shows its hint on hover, and asks for
//! an [`Action`] when clicked. [`UiExt`] does the three in one call. Text leaves for the screen
//! through here too ([`UiExt::text`] in a [`Style`], hints): a `&str` is English, shown in the
//! current language ([`crate::lang`]); a `String` is data, shown as it is.

use egui::{Rect, Response, RichText, Sense, Ui, Widget};

use crate::handle::Handle;
use crate::lang::Shown;
use crate::theme::Tokens;
use crate::{Action, Actions, scenario};

/// How a line of text is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// egui's own label.
    Body,
    Dim,
    Small,
    SmallDim,
    Strong,
    /// A heading in the text's colour.
    Heading,
    /// A heading in the dim colour (a view with nothing to show).
    HeadingDim,
    /// A prompt's or a setting's title.
    Prompt,
    /// A page's title.
    Title,
}

impl Style {
    /// `text` set in this style.
    pub fn rich(self, text: impl Into<String>) -> RichText {
        let t = &Tokens::get();
        let r = RichText::new(text);
        match self {
            Style::Body => r,
            Style::Dim => r.color(t.text_dim),
            Style::Small => r.size(t.font_small).color(t.text),
            Style::SmallDim => r.size(t.font_small).color(t.text_dim),
            Style::Strong => r.strong(),
            Style::Heading => r.size(t.font_heading).strong().color(t.text),
            Style::HeadingDim => r.size(t.font_heading).color(t.text_dim),
            Style::Prompt => r.size(t.font_label).strong().color(t.form_ink),
            Style::Title => r.size(30.0).strong().color(t.text),
        }
    }
}

/// Controls that publish their handle and ask for an action.
pub trait UiExt {
    /// `widget` (greyed out unless `enabled`), published as `handle`, `hint` on hover (none
    /// when empty); `action()` is asked for when it is clicked.
    fn act(
        &mut self,
        enabled: bool,
        widget: impl Widget,
        handle: Handle,
        hint: impl Shown,
        action: impl FnOnce() -> Action,
        out: &mut Actions,
    ) -> Response;

    /// A button labelled `label` doing that.
    fn action_button(&mut self, handle: Handle, label: impl Shown, hint: impl Shown, action: impl FnOnce() -> Action, out: &mut Actions) -> Response {
        self.act(true, egui::Button::new(label.shown()), handle, hint, action, out)
    }

    /// `widget`, published as `handle` (a control whose change stays in the widget's state).
    fn named(&mut self, widget: impl Widget, handle: Handle) -> Response {
        self.named_enabled(true, widget, handle)
    }

    /// `widget`, greyed out unless `enabled`, published as `handle`.
    fn named_enabled(&mut self, enabled: bool, widget: impl Widget, handle: Handle) -> Response;

    /// The area `rect` sensing `sense`, published as `handle`: a control the caller draws.
    fn hit(&mut self, rect: Rect, handle: Handle, sense: Sense) -> Response;

    /// A line of `text` in `style`.
    fn text(&mut self, style: Style, text: impl Shown) -> Response;
}

impl UiExt for Ui {
    fn act(
        &mut self,
        enabled: bool,
        widget: impl Widget,
        handle: Handle,
        hint: impl Shown,
        action: impl FnOnce() -> Action,
        out: &mut Actions,
    ) -> Response {
        let r = self.add_enabled(enabled, widget);
        scenario::publish_target(handle, r.rect);
        let r = r.hint(hint);
        if r.clicked() {
            out.push(action());
        }
        r
    }

    fn named_enabled(&mut self, enabled: bool, widget: impl Widget, handle: Handle) -> Response {
        let r = self.add_enabled(enabled, widget);
        scenario::publish_target(handle, r.rect);
        r
    }

    fn hit(&mut self, rect: Rect, handle: Handle, sense: Sense) -> Response {
        let r = self.interact(rect, self.id().with(handle), sense);
        scenario::publish_target(handle, rect);
        r
    }

    fn text(&mut self, style: Style, text: impl Shown) -> Response {
        self.label(style.rich(text.shown()))
    }
}

/// A response's hover text in the current language.
pub trait Hint {
    /// `text` on hover (none when empty).
    fn hint(self, text: impl Shown) -> Response;
    /// `text` on hover while disabled.
    fn hint_disabled(self, text: impl Shown) -> Response;
}

impl Hint for Response {
    fn hint(self, text: impl Shown) -> Response {
        match text.shown() {
            "" => self,
            s => self.on_hover_text(s),
        }
    }

    fn hint_disabled(self, text: impl Shown) -> Response {
        self.on_disabled_hover_text(text.shown())
    }
}

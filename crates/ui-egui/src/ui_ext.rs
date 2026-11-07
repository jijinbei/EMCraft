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
    /// `widget` (greyed out unless `enabled`, or while a job runs when `action` waits for it),
    /// published as `handle`; `action` is asked for when it is clicked. Its hover text is the response's `hint`.
    fn act(&mut self, enabled: bool, widget: impl Widget, handle: Handle, action: Action, out: &mut Actions) -> Response;

    /// A button labelled `label` doing that.
    fn action_button(&mut self, handle: Handle, label: impl Shown, hint: impl Shown, action: Action, out: &mut Actions) -> Response {
        self.act(true, egui::Button::new(label.shown()), handle, action, out).hint(hint)
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
    fn act(&mut self, enabled: bool, widget: impl Widget, handle: Handle, action: Action, out: &mut Actions) -> Response {
        let waits = !out.allows(&action);
        let r = self.add_enabled(enabled && !waits, widget);
        scenario::publish_target(handle, r.rect);
        let r = if waits { r.hint_disabled(t!("a job is running")) } else { r };
        if r.clicked() {
            out.push(action);
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

/// The most characters a hover text shows (its first sentence, cut there).
pub const BRIEF_CHARS: usize = 110;

/// A hover text in brief: its first sentence (to `. `, `。` or a line's end), or its head before
/// a colon when that says enough (20 characters or more: what follows is the detail), its code
/// marks (backticks) dropped, at most [`BRIEF_CHARS`] characters, cut at a word with `…` when
/// longer. The whole text stays the
/// command's documentation, its dialog's help and the search's.
///
/// ```
/// use emcraft_ui_egui::ui_ext::brief;
/// assert_eq!(brief("Rectangular waveguide. Sections along z, each with its material."), "Rectangular waveguide.");
/// assert_eq!(brief("矩形導波管。区間ごとに材料を持つ。"), "矩形導波管。");
/// assert!(brief(&"word ".repeat(60)).ends_with('…') && brief(&"word ".repeat(60)).chars().count() <= 111);
/// assert_eq!(brief("v1.5 mm. Next."), "v1.5 mm.");
/// assert_eq!(brief("A rectangular waveguide along z: `width` (x) × `height` (y)."), "A rectangular waveguide along z");
/// assert_eq!(brief("Sets `order`."), "Sets order.");
/// ```
pub fn brief(text: &str) -> String {
    let text = text.trim();
    let mut end = text.len();
    let mut prev: Option<char> = None;
    for (i, c) in text.char_indices() {
        if c == '\n' || c == '。' {
            end = if c == '。' { i + c.len_utf8() } else { i };
            break;
        }
        if c == ' ' && prev == Some('.') {
            end = i;
            break;
        }
        if (c == ' ' && prev == Some(':') || c == '：') && text[..i].chars().count() > 20 {
            end = if c == '：' { i } else { i - 1 };
            break;
        }
        prev = Some(c);
    }
    let first = text[..end].replace('`', "");
    let first = first.as_str();
    if first.chars().count() <= BRIEF_CHARS {
        return first.to_owned();
    }
    let cut: String = first.chars().take(BRIEF_CHARS).collect();
    let at = cut.rfind([' ', '、', ',', ';']).filter(|i| *i > BRIEF_CHARS / 2).unwrap_or(cut.len());
    format!("{}…", cut[..at].trim_end_matches([',', ';', ' ', '、']))
}

impl Hint for Response {
    fn hint(self, text: impl Shown) -> Response {
        match brief(text.shown()).as_str() {
            "" => self,
            // Its symbols set as mathematics (`εr` with a lowered r).
            s => crate::math::tip(self, s),
        }
    }

    fn hint_disabled(self, text: impl Shown) -> Response {
        self.on_disabled_hover_text(text.shown())
    }
}

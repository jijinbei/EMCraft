//! What every control of the window does besides being drawn: it is published under its
//! [`Handle`] (for scenarios and the control channel), shows its hint on hover, and asks for
//! an [`Action`] when clicked. [`UiExt`] does the three in one call.

use egui::{Rect, Response, Sense, Ui, Widget, WidgetText};

use crate::handle::Handle;
use crate::{Action, Actions, scenario};

/// Controls that publish their handle and ask for an action.
pub trait UiExt {
    /// `widget` (greyed out unless `enabled`), published as `handle`, `hint` on hover (none
    /// when empty); `action()` is asked for when it is clicked.
    fn act(&mut self, enabled: bool, widget: impl Widget, handle: Handle, hint: &str, action: impl FnOnce() -> Action, out: &mut Actions)
    -> Response;

    /// A button labelled `label` doing that.
    fn action_button(
        &mut self,
        handle: Handle,
        label: impl Into<WidgetText>,
        hint: &str,
        action: impl FnOnce() -> Action,
        out: &mut Actions,
    ) -> Response {
        self.act(true, egui::Button::new(label), handle, hint, action, out)
    }

    /// `widget`, published as `handle` (a control whose change stays in the widget's state).
    fn named(&mut self, widget: impl Widget, handle: Handle) -> Response {
        self.named_enabled(true, widget, handle)
    }

    /// `widget`, greyed out unless `enabled`, published as `handle`.
    fn named_enabled(&mut self, enabled: bool, widget: impl Widget, handle: Handle) -> Response;

    /// The area `rect` sensing `sense`, published as `handle`: a control the caller draws.
    fn hit(&mut self, rect: Rect, handle: Handle, sense: Sense) -> Response;
}

impl UiExt for Ui {
    fn act(
        &mut self,
        enabled: bool,
        widget: impl Widget,
        handle: Handle,
        hint: &str,
        action: impl FnOnce() -> Action,
        out: &mut Actions,
    ) -> Response {
        let r = self.add_enabled(enabled, widget);
        scenario::publish_target(handle, r.rect);
        let r = if hint.is_empty() { r } else { r.on_hover_text(hint) };
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
}

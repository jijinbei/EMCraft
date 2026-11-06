//! The window's layers, top first: what is open over what. Escape closes the top one, one at a
//! time — a question before work is lost, a template's card, the search, the dialog, the
//! settings, the route being drawn, the selection box, the 3D view's tool, the selection — and
//! nothing else does: every layer's way out is written once ([`close`]), and which one Escape
//! reaches follows from the window's state ([`stack`]).
//!
//! A menu or a drop-down open takes Escape itself (egui closes it); a text field outside the
//! top layer gives up the keyboard and closes nothing.

use crate::files::{Answer, Ask, FileAction};
use crate::{Action, EmcraftApp};

emcraft_macros::named_enum! {
    /// A layer of the window, top first.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Layer {
        /// A question before work is lost, or a prompt for a path or a file's name.
        Ask = "ask",
        /// A template's card.
        Template = "template",
        /// The command search, open.
        Search = "search",
        /// The open dialog (floating or docked).
        Dialog = "dialog",
        /// The settings page, shown.
        Settings = "settings",
        /// A route being drawn on the board (the pen).
        Pen = "pen",
        /// The 3D view's selection box, being dragged.
        Box = "box",
        /// The 3D view's tool, with what it picked.
        Tool = "tool",
        /// What is selected (in the 3D view and the browser).
        Selection = "selection",
    }
}

/// The layers open now, top first.
pub fn stack(app: &EmcraftApp) -> Vec<Layer> {
    let open = |l: Layer| match l {
        Layer::Ask => app.ask.is_some(),
        Layer::Template => app.template.is_some(),
        Layer::Search => app.search.open,
        Layer::Dialog => app.dialog.is_some(),
        Layer::Settings => app.page.settings_shown().is_some(),
        Layer::Pen => app.views.pcb.drawing.is_some(),
        Layer::Box => app.view3d.boxsel.is_some(),
        Layer::Tool => app.view3d.tools.tool.is_some() || app.view3d.tools.measured.is_some(),
        Layer::Selection => !app.view3d.selected.is_empty() || app.browser.selected.is_some(),
    };
    Layer::ALL.into_iter().filter(|l| open(*l)).collect()
}

/// The layer Escape closes now: the top one, unless a menu or a drop-down is open (it takes
/// Escape) or a text field the top layer does not own has the keyboard (it lets go of it).
pub fn escape_target(app: &EmcraftApp, ctx: &egui::Context) -> Option<Layer> {
    if egui::Popup::is_any_open(ctx) {
        return None;
    }
    let top = stack(app).into_iter().next()?;
    // Who had the keyboard: egui lets go of it on Escape before the frame, so the end of the
    // last frame says.
    let focus = app.focus_last;
    let owned = match top {
        // Their own fields: Escape is theirs while one has the keyboard.
        Layer::Ask | Layer::Search => true,
        Layer::Dialog => focus.is_none_or(|f| app.dialog.as_ref().is_some_and(|d| d.widgets.iter().any(|(id, _)| *id == f))),
        _ => focus.is_none() && !ctx.egui_wants_keyboard_input(),
    };
    owned.then_some(top)
}

/// Takes the frame's Escape for the layer it closes, if any (before anything is drawn, so no
/// widget sees it too).
pub fn escape(app: &EmcraftApp, ctx: &egui::Context) -> Option<Action> {
    let layer = escape_target(app, ctx)?;
    ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)).then_some(Action::Close(layer))
}

/// Closes `layer`: each layer's way out, written once.
pub(crate) fn close(app: &mut EmcraftApp, layer: Layer) -> Option<Action> {
    match layer {
        Layer::Ask => match app.ask.take() {
            Some(Ask::Path(p)) => return Some(Action::File(FileAction::Chosen(p.purpose, None))),
            Some(Ask::Save(then)) => {
                app.ask = Some(Ask::Save(then));
                return Some(Action::File(FileAction::Confirm(Answer::Cancel)));
            }
            Some(Ask::Export(_)) | None => {}
        },
        Layer::Template => app.template = None,
        Layer::Search => {
            app.search.open = false;
            app.search.text.clear();
        }
        Layer::Dialog => return Some(Action::DialogCancel),
        Layer::Settings => app.page.close_settings(),
        // A line begun goes first, then the pen.
        Layer::Pen => {
            let pen = &mut app.views.pcb.drawing;
            match pen.as_mut() {
                Some(p) if p.from.is_some() => p.from = None,
                _ => *pen = None,
            }
        }
        Layer::Box => app.view3d.cancel_box(),
        // What the tool picked goes first, then the tool.
        Layer::Tool => app.view3d.tools.escape(),
        Layer::Selection => {
            app.view3d.selected.clear();
            app.browser.selected = None;
        }
    }
    None
}

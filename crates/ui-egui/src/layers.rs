//! Escape on the window: the frame's key, given to the top layer ([`crate::ui_state`] decides
//! which) before anything is drawn, so no widget sees it too. The 3D view's gestures (the pen,
//! the selection box, what the tool picked) are drafts of the view: whether they are open and
//! how they let go are here.

use crate::ui_state::{Keyboard, Layer};
use crate::{Action, EmcraftApp};

/// The 3D view's layers open, top first.
fn view_layers(app: &EmcraftApp) -> Vec<Layer> {
    let open = [(Layer::Box, app.view3d.boxsel.is_some()), (Layer::Tool, app.view3d.tools.measured.is_some())];
    open.into_iter().filter_map(|(l, on)| on.then_some(l)).collect()
}

/// The layers open now, top first.
pub fn stack(app: &EmcraftApp) -> Vec<Layer> {
    app.ui_state.layers(&view_layers(app))
}

/// The layer Escape closes now: none while a menu or a drop-down is open (it takes Escape).
pub fn escape_target(app: &EmcraftApp, ctx: &egui::Context) -> Option<Layer> {
    if egui::Popup::is_any_open(ctx) {
        return None;
    }
    // Who had the keyboard: egui lets go of it on Escape before the frame, so the end of the
    // last frame says.
    let keyboard = match app.focus_last {
        None if !ctx.egui_wants_keyboard_input() => Keyboard::Free,
        Some(f) if app.ui_state.dialog().is_some_and(|d| d.widgets.iter().any(|(id, _)| *id == f)) => Keyboard::Dialog,
        _ => Keyboard::Elsewhere,
    };
    app.ui_state.escape_target(&view_layers(app), keyboard)
}

/// Takes the frame's Escape for the layer it closes, if any.
pub fn escape(app: &EmcraftApp, ctx: &egui::Context) -> Option<Action> {
    let layer = escape_target(app, ctx)?;
    ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)).then_some(Action::Close(layer))
}

/// Closes one of the 3D view's layers (the window's are [`crate::ui_state::UiState::close`]).
pub(crate) fn close(app: &mut EmcraftApp, layer: Layer) -> Option<Action> {
    match layer {
        // A line begun goes first, then the pen.
        Layer::Pen => {
            if app.views.pcb.pen.from.take().is_none() {
                app.ui_state.set_drawing(false);
            }
        }
        Layer::Box => app.view3d.cancel_box(),
        // What the tool picked goes first, then the tool.
        Layer::Tool => {
            if app.view3d.tools.escape() {
                app.ui_state.set_tool(None);
            }
        }
        Layer::Ask | Layer::Template | Layer::Search | Layer::Dialog | Layer::Settings | Layer::Selection => return app.ui_state.close(layer),
    }
    None
}

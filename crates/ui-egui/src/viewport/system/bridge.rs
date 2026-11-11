//! What an egui drag reports to the shared canvas' gestures, and where the canvas is kept between
//! frames (its geometry converted by [`crate::geom`]).

pub(super) use crate::geom::{point, pos, rect, vector};
use emcraft_ui_core::system::gesture::{Canvas, Input};

/// What a dragged response reports this frame: `start` when its drag begins, the move, where
/// it is let go.
pub(super) fn drag_inputs(resp: &egui::Response, start: Input) -> Vec<Input> {
    let began = resp.drag_started().then_some(start);
    let moved = resp.dragged().then(|| Input::Drag(vector(resp.drag_delta())));
    let ended = resp.drag_stopped().then(|| resp.interact_pointer_pos().map(|p| Input::Release(point(p)))).flatten();
    [began, moved, ended].into_iter().flatten().collect()
}

/// The canvas as the last frame left it (kept in the context's memory, as egui keeps a
/// widget's state).
pub(super) fn load(ctx: &egui::Context) -> Canvas {
    ctx.data_mut(|d| d.get_temp::<Canvas>(egui::Id::new("system_canvas"))).unwrap_or_default()
}

/// The canvas kept for the next frame.
pub(super) fn store(ctx: &egui::Context, canvas: Canvas) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("system_canvas"), canvas));
}

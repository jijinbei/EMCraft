//! The shared canvas' geometry on egui's screen, and what an egui drag reports to its gestures.

use emcraft_ui_core::geom;
use emcraft_ui_core::system::gesture::{Canvas, Input};

/// A shared point as egui's.
pub(super) fn pos(p: geom::Point) -> egui::Pos2 {
    egui::pos2(p.x, p.y)
}

/// A shared rectangle as egui's.
pub(super) fn rect(r: geom::Rect) -> egui::Rect {
    egui::Rect::from_min_size(pos(r.min), egui::vec2(r.size.x, r.size.y))
}

/// An egui point as the shared one.
pub(super) fn point(p: egui::Pos2) -> geom::Point {
    geom::Point::new(p.x, p.y)
}

/// An egui offset as the shared one.
pub(super) fn vector(v: egui::Vec2) -> geom::Vector {
    geom::Vector::new(v.x, v.y)
}

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

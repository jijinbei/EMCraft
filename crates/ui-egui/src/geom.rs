//! The shared geometry ([`emcraft_ui_core::geom`]) on egui's screen and back: the one place the
//! window converts between them.

use emcraft_ui_core::geom::{Point, Rect, Vector};

/// A shared point as egui's.
pub(crate) fn pos(p: Point) -> egui::Pos2 {
    egui::pos2(p.x, p.y)
}

/// A shared rectangle as egui's.
pub(crate) fn rect(r: Rect) -> egui::Rect {
    egui::Rect::from_min_size(pos(r.min), egui::vec2(r.size.x, r.size.y))
}

/// An egui point as the shared one.
pub(crate) fn point(p: egui::Pos2) -> Point {
    Point::new(p.x, p.y)
}

/// An egui offset as the shared one.
pub(crate) fn vector(v: egui::Vec2) -> Vector {
    Vector::new(v.x, v.y)
}

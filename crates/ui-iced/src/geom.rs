//! The shared geometry ([`emcraft_ui_core::geom`]) on iced's screen and back: the one place the
//! window converts between them.

use emcraft_ui_core::geom::{Point, Vector};

/// A shared point as iced's.
pub fn point(p: Point) -> iced::Point {
    iced::Point::new(p.x, p.y)
}

/// An iced point as the shared one.
pub fn shared(p: iced::Point) -> Point {
    Point::new(p.x, p.y)
}

/// A shared offset as iced's.
pub fn vector(v: Vector) -> iced::Vector {
    iced::Vector::new(v.x, v.y)
}

/// An iced size as the shared offset across it.
pub fn size(s: iced::Size) -> Vector {
    Vector::new(s.width, s.height)
}

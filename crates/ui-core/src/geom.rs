//! Points, offsets and rectangles on screen (points, y down), as the shared rules lay things
//! out; each window converts them to its toolkit's own (egui's `Pos2`, iced's `Point`).
//!
//! ```
//! use emcraft_ui_core::geom::{Point, Rect, Vector};
//! let r = Rect::from_min_size(Point::new(10.0, 20.0), Vector::new(100.0, 50.0));
//! assert_eq!(r.right_top() + Vector::new(0.0, 5.0), Point::new(110.0, 25.0));
//! assert_eq!(Point::new(3.0, 4.0).distance(Point::new(0.0, 0.0)), 5.0);
//! ```

use std::ops::{Add, AddAssign, Sub};

/// A point.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// An offset between points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector {
    pub x: f32,
    pub y: f32,
}

/// A rectangle by its top left corner and its size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub min: Point,
    pub size: Vector,
}

impl Point {
    pub const ORIGIN: Point = Point { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    pub fn distance(self, other: Point) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }

    /// The point a share `t` of the way to `other`.
    pub fn lerp(self, other: Point, t: f32) -> Point {
        Point::new(self.x + (other.x - self.x) * t, self.y + (other.y - self.y) * t)
    }
}

impl Vector {
    pub const ZERO: Vector = Vector { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Vector {
        Vector { x, y }
    }
}

impl Add<Vector> for Point {
    type Output = Point;
    fn add(self, v: Vector) -> Point {
        Point::new(self.x + v.x, self.y + v.y)
    }
}

impl Sub<Vector> for Point {
    type Output = Point;
    fn sub(self, v: Vector) -> Point {
        Point::new(self.x - v.x, self.y - v.y)
    }
}

impl Sub for Point {
    type Output = Vector;
    fn sub(self, p: Point) -> Vector {
        Vector::new(self.x - p.x, self.y - p.y)
    }
}

impl Add for Vector {
    type Output = Vector;
    fn add(self, v: Vector) -> Vector {
        Vector::new(self.x + v.x, self.y + v.y)
    }
}

impl AddAssign for Vector {
    fn add_assign(&mut self, v: Vector) {
        *self = *self + v;
    }
}

impl Rect {
    pub const fn from_min_size(min: Point, size: Vector) -> Rect {
        Rect { min, size }
    }

    pub fn left(&self) -> f32 {
        self.min.x
    }

    pub fn top(&self) -> f32 {
        self.min.y
    }

    pub fn right(&self) -> f32 {
        self.min.x + self.size.x
    }

    pub fn bottom(&self) -> f32 {
        self.min.y + self.size.y
    }

    pub fn width(&self) -> f32 {
        self.size.x
    }

    pub fn left_top(&self) -> Point {
        self.min
    }

    pub fn right_top(&self) -> Point {
        Point::new(self.right(), self.top())
    }

    pub fn right_bottom(&self) -> Point {
        Point::new(self.right(), self.bottom())
    }
}

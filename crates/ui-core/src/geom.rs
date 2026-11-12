//! Points, offsets and rectangles on screen (points, y down), as the shared rules lay things
//! out; the window converts them to its toolkit's own (iced's `Point`).
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

    /// Whether both coordinates are finite.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
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

    /// The rectangle with corners `a` and `b`, either way round.
    pub fn from_two_pos(a: Point, b: Point) -> Rect {
        let min = Point::new(a.x.min(b.x), a.y.min(b.y));
        Rect { min, size: Vector::new((a.x - b.x).abs(), (a.y - b.y).abs()) }
    }

    /// Whether `p` lies inside or on it.
    pub fn contains(&self, p: Point) -> bool {
        (self.left()..=self.right()).contains(&p.x) && (self.top()..=self.bottom()).contains(&p.y)
    }

    pub fn center(&self) -> Point {
        Point::new(self.min.x + self.size.x / 2.0, self.min.y + self.size.y / 2.0)
    }

    pub fn left_bottom(&self) -> Point {
        Point::new(self.left(), self.bottom())
    }

    /// Whether its corner and size are finite.
    pub fn is_finite(&self) -> bool {
        self.min.is_finite() && self.size.x.is_finite() && self.size.y.is_finite()
    }

    /// It moved by `v`.
    pub fn translate(&self, v: Vector) -> Rect {
        Rect { min: self.min + v, size: self.size }
    }

    pub fn height(&self) -> f32 {
        self.size.y
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

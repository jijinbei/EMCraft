//! Points and vectors in space (m), as their own types rather than bare `[f64; 3]`: a
//! [`Point3`] is a place, a [`Vec3`] a displacement or a direction. Their differences and sums
//! follow affine rules — point − point = vector, point + vector = point, vectors add and scale
//! — so the arithmetic of geometry reads as it is written on paper and a place cannot be added
//! to a place by mistake. Components are indexed by axis (0 = x, 1 = y, 2 = z) where an
//! algorithm runs over the axes.
//!
//! Both serialise as `[x, y, z]`, the form scripts and files use.

use std::ops::{Add, AddAssign, Div, Index, IndexMut, Mul, Neg, Sub, SubAssign};

use num_complex::Complex64 as C;
use serde::{Deserialize, Serialize};

/// A point in space (m).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "[f64; 3]", into = "[f64; 3]")]
pub struct Point3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// A displacement or a direction (m, or dimensionless for unit vectors).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "[f64; 3]", into = "[f64; 3]")]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

macro_rules! common {
    ($t:ident) => {
        impl $t {
            pub const fn new(x: f64, y: f64, z: f64) -> $t {
                $t { x, y, z }
            }

            /// The components as an array (for interfaces that need one: I/O, predicates).
            pub const fn to_array(self) -> [f64; 3] {
                [self.x, self.y, self.z]
            }

            /// Each component through `f`.
            pub fn map(self, f: impl Fn(f64) -> f64) -> $t {
                $t::new(f(self.x), f(self.y), f(self.z))
            }

            /// Componentwise with another's.
            pub fn zip_map(self, o: $t, f: impl Fn(f64, f64) -> f64) -> $t {
                $t::new(f(self.x, o.x), f(self.y, o.y), f(self.z, o.z))
            }

            /// The same with component `axis` set to `v`.
            pub fn with(mut self, axis: usize, v: f64) -> $t {
                self[axis] = v;
                self
            }

            pub fn is_finite(self) -> bool {
                self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
            }

            /// The components, x, y, z.
            pub fn iter(self) -> impl Iterator<Item = f64> {
                [self.x, self.y, self.z].into_iter()
            }

            /// Componentwise minimum and maximum.
            pub fn min(self, o: $t) -> $t {
                self.zip_map(o, f64::min)
            }

            pub fn max(self, o: $t) -> $t {
                self.zip_map(o, f64::max)
            }
        }

        impl From<[f64; 3]> for $t {
            fn from(a: [f64; 3]) -> $t {
                $t::new(a[0], a[1], a[2])
            }
        }

        impl From<$t> for [f64; 3] {
            fn from(p: $t) -> [f64; 3] {
                p.to_array()
            }
        }

        impl Index<usize> for $t {
            type Output = f64;
            /// Component `axis` (0, 1, 2); any other index reads z.
            fn index(&self, axis: usize) -> &f64 {
                match axis {
                    0 => &self.x,
                    1 => &self.y,
                    _ => &self.z,
                }
            }
        }

        impl IndexMut<usize> for $t {
            fn index_mut(&mut self, axis: usize) -> &mut f64 {
                match axis {
                    0 => &mut self.x,
                    1 => &mut self.y,
                    _ => &mut self.z,
                }
            }
        }
    };
}

common!(Point3);
common!(Vec3);

impl Point3 {
    pub const ORIGIN: Point3 = Point3::new(0.0, 0.0, 0.0);

    /// The displacement from the origin.
    pub fn to_vec(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }

    /// The point a fraction `t` of the way to `o`.
    pub fn lerp(self, o: Point3, t: f64) -> Point3 {
        self + (o - self) * t
    }

    /// (a + b)/2: the same whichever end comes first.
    pub fn midpoint(self, o: Point3) -> Point3 {
        self.zip_map(o, |a, b| (a + b) / 2.0)
    }

    pub fn distance(self, o: Point3) -> f64 {
        (o - self).norm()
    }

    /// The centroid of points (`None` for none).
    pub fn centroid(points: impl IntoIterator<Item = Point3>) -> Option<Point3> {
        let (sum, n) = points.into_iter().fold((Vec3::ZERO, 0usize), |(s, n), p| (s + p.to_vec(), n + 1));
        (n > 0).then(|| Point3::ORIGIN + sum / n as f64)
    }
}

/// An axis-aligned box: the points from `min` to `max` in each coordinate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: Point3,
    pub max: Point3,
}

impl Aabb {
    pub const fn new(min: Point3, max: Point3) -> Aabb {
        Aabb { min, max }
    }

    /// The box of one point.
    pub const fn point(p: Point3) -> Aabb {
        Aabb { min: p, max: p }
    }

    /// The smallest box holding the points (`None` for none).
    pub fn around(points: impl IntoIterator<Item = Point3>) -> Option<Aabb> {
        points.into_iter().map(Aabb::point).reduce(Aabb::union)
    }

    /// The smallest box holding both.
    pub fn union(self, o: Aabb) -> Aabb {
        Aabb { min: self.min.min(o.min), max: self.max.max(o.max) }
    }

    /// The box grown by `margin` on every side.
    pub fn grown(self, margin: f64) -> Aabb {
        Aabb { min: self.min.map(|v| v - margin), max: self.max.map(|v| v + margin) }
    }

    pub fn size(self) -> Vec3 {
        self.max - self.min
    }

    pub fn center(self) -> Point3 {
        self.min.midpoint(self.max)
    }

    /// Whether `p` lies inside or on the box.
    pub fn contains(self, p: Point3) -> bool {
        (0..3).all(|i| self.min[i] <= p[i] && p[i] <= self.max[i])
    }

    /// Whether `o` lies inside or on the box, within `tol`.
    pub fn encloses(self, o: Aabb, tol: f64) -> bool {
        (0..3).all(|i| o.min[i] >= self.min[i] - tol && o.max[i] <= self.max[i] + tol)
    }

    /// Whether the boxes share any point.
    pub fn overlaps(self, o: Aabb) -> bool {
        (0..3).all(|i| self.min[i] <= o.max[i] && o.min[i] <= self.max[i])
    }

    /// Whether `p` lies strictly inside the box, off its faces.
    pub fn holds_inside(self, p: Point3) -> bool {
        (0..3).all(|i| self.min[i] < p[i] && p[i] < self.max[i])
    }

    /// Whether `o` lies strictly inside the box, off its faces.
    pub fn encloses_inside(self, o: Aabb) -> bool {
        (0..3).all(|i| self.min[i] < o.min[i] && o.max[i] < self.max[i])
    }

    /// Whether the boxes' interiors meet (more than a face in common).
    pub fn overlaps_inside(self, o: Aabb) -> bool {
        (0..3).all(|i| self.min[i] < o.max[i] && o.min[i] < self.max[i])
    }

    /// The two opposite corners, `min` first.
    pub fn corners(self) -> [Point3; 2] {
        [self.min, self.max]
    }
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);

    /// The unit vector along `axis`.
    pub fn unit(axis: usize) -> Vec3 {
        Vec3::ZERO.with(axis, 1.0)
    }

    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }

    pub fn norm_sqr(self) -> f64 {
        self.dot(self)
    }

    pub fn norm(self) -> f64 {
        self.norm_sqr().sqrt()
    }

    /// The unit vector along this one; `None` for a zero or non-finite one.
    pub fn normalized(self) -> Option<Vec3> {
        let n = self.norm();
        (n > 0.0 && n.is_finite()).then(|| self / n)
    }

    /// The point this displacement leads to from the origin.
    pub fn to_point(self) -> Point3 {
        Point3::new(self.x, self.y, self.z)
    }

    /// The part across the unit vector `n`: v − (v·n) n.
    pub fn tangential(self, n: Vec3) -> Vec3 {
        self - n * self.dot(n)
    }

    /// The spherical unit vectors (r̂, θ̂, φ̂) at polar angle θ and azimuth φ.
    pub fn spherical(theta: f64, phi: f64) -> [Vec3; 3] {
        let ((st, ct), (sp, cp)) = (theta.sin_cos(), phi.sin_cos());
        [Vec3::new(st * cp, st * sp, ct), Vec3::new(ct * cp, ct * sp, -st), Vec3::new(-sp, cp, 0.0)]
    }

    /// The axis of the largest component in magnitude.
    pub fn dominant_axis(self) -> usize {
        let a = self.map(f64::abs);
        if a.x >= a.y && a.x >= a.z {
            0
        } else if a.y >= a.z {
            1
        } else {
            2
        }
    }
}

/// The scalar triple product a · (b × c): six times the signed volume of the tetrahedron the
/// three edges span.
pub fn triple(a: Vec3, b: Vec3, c: Vec3) -> f64 {
    a.dot(b.cross(c))
}

impl Sub for Point3 {
    type Output = Vec3;
    fn sub(self, o: Point3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Add<Vec3> for Point3 {
    type Output = Point3;
    fn add(self, v: Vec3) -> Point3 {
        Point3::new(self.x + v.x, self.y + v.y, self.z + v.z)
    }
}

impl Sub<Vec3> for Point3 {
    type Output = Point3;
    fn sub(self, v: Vec3) -> Point3 {
        Point3::new(self.x - v.x, self.y - v.y, self.z - v.z)
    }
}

impl AddAssign<Vec3> for Point3 {
    fn add_assign(&mut self, v: Vec3) {
        *self = *self + v;
    }
}

impl SubAssign<Vec3> for Point3 {
    fn sub_assign(&mut self, v: Vec3) {
        *self = *self - v;
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        self.zip_map(o, |a, b| a + b)
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        self.zip_map(o, |a, b| a - b)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, o: Vec3) {
        *self = *self + o;
    }
}

impl SubAssign for Vec3 {
    fn sub_assign(&mut self, o: Vec3) {
        *self = *self - o;
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        self.map(|a| -a)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f64) -> Vec3 {
        self.map(|a| a * s)
    }
}

impl Mul<Vec3> for f64 {
    type Output = Vec3;
    fn mul(self, v: Vec3) -> Vec3 {
        v * self
    }
}

impl Div<f64> for Vec3 {
    type Output = Vec3;
    fn div(self, s: f64) -> Vec3 {
        self.map(|a| a / s)
    }
}

impl std::iter::Sum for Vec3 {
    fn sum<I: Iterator<Item = Vec3>>(it: I) -> Vec3 {
        it.fold(Vec3::ZERO, Add::add)
    }
}

/// A complex vector: the phasor of a time-harmonic field (E in V/m, H in A/m), e^{jωt}.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CVec3 {
    pub x: C,
    pub y: C,
    pub z: C,
}

impl CVec3 {
    pub const ZERO: CVec3 = CVec3 { x: C::new(0.0, 0.0), y: C::new(0.0, 0.0), z: C::new(0.0, 0.0) };

    pub const fn new(x: C, y: C, z: C) -> CVec3 {
        CVec3 { x, y, z }
    }

    /// A real vector as a phasor.
    pub fn from_real(v: Vec3) -> CVec3 {
        CVec3::new(C::new(v.x, 0.0), C::new(v.y, 0.0), C::new(v.z, 0.0))
    }

    pub fn map(self, f: impl Fn(C) -> C) -> CVec3 {
        CVec3::new(f(self.x), f(self.y), f(self.z))
    }

    /// Σ aᵢ bᵢ (no conjugation).
    pub fn dot(self, o: CVec3) -> C {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    /// Σ aᵢ vᵢ with a real vector.
    pub fn dot_real(self, v: Vec3) -> C {
        self.x * v.x + self.y * v.y + self.z * v.z
    }

    pub fn cross(self, o: CVec3) -> CVec3 {
        CVec3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }

    pub fn conj(self) -> CVec3 {
        self.map(|c| c.conj())
    }

    /// Σ |aᵢ|².
    pub fn norm_sqr(self) -> f64 {
        self.x.norm_sqr() + self.y.norm_sqr() + self.z.norm_sqr()
    }

    /// The real parts.
    pub fn re(self) -> Vec3 {
        Vec3::new(self.x.re, self.y.re, self.z.re)
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    pub fn to_array(self) -> [C; 3] {
        [self.x, self.y, self.z]
    }

    pub fn iter(self) -> impl Iterator<Item = C> {
        self.to_array().into_iter()
    }
}

impl Index<usize> for CVec3 {
    type Output = C;
    fn index(&self, axis: usize) -> &C {
        match axis {
            0 => &self.x,
            1 => &self.y,
            _ => &self.z,
        }
    }
}

impl IndexMut<usize> for CVec3 {
    fn index_mut(&mut self, axis: usize) -> &mut C {
        match axis {
            0 => &mut self.x,
            1 => &mut self.y,
            _ => &mut self.z,
        }
    }
}

impl Add for CVec3 {
    type Output = CVec3;
    fn add(self, o: CVec3) -> CVec3 {
        CVec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for CVec3 {
    type Output = CVec3;
    fn sub(self, o: CVec3) -> CVec3 {
        CVec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl AddAssign for CVec3 {
    fn add_assign(&mut self, o: CVec3) {
        *self = *self + o;
    }
}

impl Neg for CVec3 {
    type Output = CVec3;
    fn neg(self) -> CVec3 {
        self.map(|c| -c)
    }
}

impl Mul<C> for CVec3 {
    type Output = CVec3;
    fn mul(self, s: C) -> CVec3 {
        self.map(|c| c * s)
    }
}

impl Mul<f64> for CVec3 {
    type Output = CVec3;
    fn mul(self, s: f64) -> CVec3 {
        self.map(|c| c * s)
    }
}

impl Div<C> for CVec3 {
    type Output = CVec3;
    fn div(self, s: C) -> CVec3 {
        self.map(|c| c / s)
    }
}

impl std::iter::Sum for CVec3 {
    fn sum<I: Iterator<Item = CVec3>>(it: I) -> CVec3 {
        it.fold(CVec3::ZERO, Add::add)
    }
}

/// A real vector times a complex scalar.
impl Mul<C> for Vec3 {
    type Output = CVec3;
    fn mul(self, s: C) -> CVec3 {
        CVec3::new(s * self.x, s * self.y, s * self.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midpoints_do_not_depend_on_the_order() {
        // An edge's midpoint is shared by the faces on it, whichever way they walk it: the
        // same bits from either end (a + (b − a)/2 is not).
        let (a, b) = (Point3::new(0.1, -7.3e-3, 1.0 / 3.0), Point3::new(2.0f64.sqrt(), 0.7, -1e-9));
        assert_eq!(a.midpoint(b), b.midpoint(a));
        assert_eq!(a.midpoint(b), Point3::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0, (a.z + b.z) / 2.0));
    }

    #[test]
    fn affine_arithmetic() {
        let (a, b) = (Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 6.0, 3.0));
        let d = b - a;
        assert_eq!(d, Vec3::new(3.0, 4.0, 0.0));
        assert_eq!(d.norm(), 5.0);
        assert_eq!(a + d, b);
        assert_eq!(a.midpoint(b), Point3::new(2.5, 4.0, 3.0));
        assert_eq!(Vec3::unit(0).cross(Vec3::unit(1)), Vec3::unit(2));
        assert_eq!(triple(Vec3::unit(0), Vec3::unit(1), Vec3::unit(2)), 1.0);
        assert_eq!(Point3::centroid([a, b]), Some(a.midpoint(b)));
        assert_eq!(Point3::centroid([]), None);
        assert_eq!(Vec3::new(0.0, -3.0, 2.0).dominant_axis(), 1);
        assert_eq!(Vec3::ZERO.normalized(), None);
        assert_eq!(Vec3::new(1.0, 2.0, 3.0).tangential(Vec3::unit(2)), Vec3::new(1.0, 2.0, 0.0));
        let mut p = a;
        p[2] = 7.0;
        assert_eq!(p.z, 7.0);
        // JSON as arrays.
        assert_eq!(serde_json::to_string(&a).unwrap(), "[1.0,2.0,3.0]");
        assert_eq!(serde_json::from_str::<Vec3>("[3,4,0]").unwrap(), d);
        // Complex vectors: E × H* of a plane wave carries power along its direction.
        let (e, h) = (Vec3::unit(0) * C::new(0.0, 1.0), Vec3::unit(1) * C::new(0.0, 1.0));
        assert_eq!(e.cross(h.conj()).re(), Vec3::unit(2));
        assert_eq!(CVec3::from_real(d).dot_real(d), C::new(25.0, 0.0));
    }
}

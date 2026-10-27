//! Points and vectors in space, as their own types rather than bare `[f64; 3]` (floating
//! point: measures, placement and display, never meshing decisions, which use
//! [`crate::predicates`] on the coordinates).
//!
//! A [`Point3`] is a place, a [`Vec3`] a displacement or a direction; their arithmetic follows
//! affine rules — point − point = vector, point ± vector = point, vectors add and scale — so a
//! place cannot be added to a place by mistake. Components are indexed by axis (0 = x, 1 = y,
//! 2 = z) where an algorithm runs over the axes.
//!
//! **Every operation is the plain componentwise IEEE 754 arithmetic it reads as, in the order it
//! is written** (Rust contracts no multiply–add): `a.dot(b)` is `a.x * b.x + a.y * b.y + a.z *
//! b.z`, `v * s` multiplies each component by `s`, [`Point3::lerp`] is `a + (b − a) t` per
//! component, [`Point3::midpoint`] `0.5 (a + b)`. Meshes depend on these bits (AGENTS.md,
//! Determinism), so a formula keeps its operations and their order when written with these
//! types. Conversions to and from `[f64; 3]` ([`Point3::to_array`], `From`) move bits only.

use std::ops::{Add, AddAssign, Index, IndexMut, Mul, Neg, Sub, SubAssign};

/// A point in space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// A displacement or a direction.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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

            /// All three components `v`.
            pub const fn splat(v: f64) -> $t {
                $t { x: v, y: v, z: v }
            }

            /// The components as an array (for the predicates and other coordinate interfaces).
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

            /// Componentwise minimum.
            pub fn min(self, o: $t) -> $t {
                self.zip_map(o, f64::min)
            }

            /// Componentwise maximum.
            pub fn max(self, o: $t) -> $t {
                self.zip_map(o, f64::max)
            }

            pub fn is_finite(self) -> bool {
                self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
            }

            /// The components, x, y, z.
            pub fn iter(self) -> impl Iterator<Item = f64> {
                [self.x, self.y, self.z].into_iter()
            }
        }

        impl From<[f64; 3]> for $t {
            fn from([x, y, z]: [f64; 3]) -> $t {
                $t { x, y, z }
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

    /// The displacement from the origin (the same components: no arithmetic).
    pub const fn to_vec(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }

    /// The point a fraction `t` of the way to `o`: `self + (o − self) t` per component.
    pub fn lerp(self, o: Point3, t: f64) -> Point3 {
        self + (o - self) * t
    }

    /// `0.5 (self + o)` per component.
    pub fn midpoint(self, o: Point3) -> Point3 {
        self.zip_map(o, |a, b| 0.5 * (a + b))
    }

    /// `|self − o|`.
    pub fn distance(self, o: Point3) -> f64 {
        (self - o).norm()
    }

    /// The centroid of a triangle: `((a + b) + c) × (1/3)` per component.
    pub fn centroid3(a: Point3, b: Point3, c: Point3) -> Point3 {
        ((a.to_vec() + b.to_vec() + c.to_vec()) * (1.0 / 3.0)).to_point()
    }
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);

    /// The point at this displacement from the origin (the same components: no arithmetic).
    pub const fn to_point(self) -> Point3 {
        Point3::new(self.x, self.y, self.z)
    }

    /// `x x' + y y' + z z'`, in that order.
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }

    /// `√(v · v)`.
    pub fn norm(self) -> f64 {
        self.dot(self).sqrt()
    }

    /// The triple product `self · (b × c)`: the determinant with rows `self`, `b`, `c`.
    pub fn det(self, b: Vec3, c: Vec3) -> f64 {
        self.dot(b.cross(c))
    }

    /// The angle to `o` in degrees (0 when either is zero), the same on every platform.
    pub fn angle_deg(self, o: Vec3) -> f64 {
        atan2(self.cross(o).norm(), self.dot(o)).to_degrees()
    }
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
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
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
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

/// Each component times `s` (`v * s`, as written; multiplication commutes exactly).
impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Mul<Vec3> for f64 {
    type Output = Vec3;
    fn mul(self, v: Vec3) -> Vec3 {
        Vec3::new(self * v.x, self * v.y, self * v.z)
    }
}

/// `atan(t)` for `0 ≤ t ≤ 1` from `+ − × ÷ √` only: two half-angle reductions
/// (`atan t = 2 atan(t / (1 + √(1 + t²)))`) bring `t` below tan(π/16) ≈ 0.199, where the Taylor
/// series converges to double precision in 20 terms.
fn atan_unit(t: f64) -> f64 {
    let mut t = t;
    for _ in 0..2 {
        t /= 1.0 + (1.0 + t * t).sqrt();
    }
    let (t2, mut term, mut sum) = (t * t, t, 0.0);
    for k in 0..20 {
        let s = term / (2 * k + 1) as f64;
        sum += if k % 2 == 0 { s } else { -s };
        term *= t2;
    }
    4.0 * sum
}

/// The angle of `(x, y)` in radians, in `(−π, π]`, like [`f64::atan2`] but the same bit for bit
/// on every platform: the platform maths libraries' `atan2` differ in the last bits (macOS and
/// glibc), and a mesh or report must not (AGENTS.md, Determinism). 0 for `(0, 0)` and for
/// non-finite input.
pub(crate) fn atan2(y: f64, x: f64) -> f64 {
    use std::f64::consts::{FRAC_PI_2, PI};
    if !(x.is_finite() && y.is_finite()) || (x == 0.0 && y == 0.0) {
        return 0.0;
    }
    let (ax, ay) = (x.abs(), y.abs());
    let a = if ay <= ax { atan_unit(ay / ax) } else { FRAC_PI_2 - atan_unit(ax / ay) };
    let a = if x < 0.0 { PI - a } else { a };
    if y < 0.0 { -a } else { a }
}

/// The cube root, from `+ − × ÷` only (Newton's iteration from an exact power-of-two estimate),
/// the same on every platform (see [`atan2`]).
pub(crate) fn cbrt(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    let a = x.abs();
    // 2^(e/3) for a's binary exponent e: within a factor 2 of the root.
    let e = ((a.to_bits() >> 52) & 0x7ff) as i64 - 1023;
    let mut r = f64::from_bits((((e / 3) + 1023).clamp(1, 2046) as u64) << 52);
    for _ in 0..60 {
        let next = r - (r * r * r - a) / (3.0 * r * r);
        if next == r || !next.is_finite() {
            break;
        }
        r = next;
    }
    if x < 0.0 { -r } else { r }
}

/// Points from coordinate triples (tests).
#[cfg(test)]
pub(crate) fn pts<const N: usize>(a: [[f64; 3]; N]) -> [Point3; N] {
    a.map(Point3::from)
}

/// Points from a list of coordinate triples (tests).
#[cfg(test)]
pub(crate) fn points_of(a: &[[f64; 3]]) -> Vec<Point3> {
    a.iter().map(|&p| Point3::from(p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each operation is the componentwise formula it stands for, bit for bit (meshes depend on
    /// it: see the module docs).
    #[test]
    fn operations_are_their_formulas_bit_for_bit() {
        let mut s = 0x9e37_79b9_7f4a_7c15u64;
        let mut r = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            ((s >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * [1e-7, 1.0, 3e5][(s % 3) as usize]
        };
        let same = |x: [f64; 3], y: [f64; 3]| x.iter().zip(&y).all(|(a, b)| a.to_bits() == b.to_bits());
        for _ in 0..10_000 {
            let (a, b, c, t) = ([r(), r(), r()], [r(), r(), r()], [r(), r(), r()], r());
            let (p, q) = (Point3::from(a), Point3::from(b));
            let (u, v, w) = (Vec3::from(a), Vec3::from(b), Vec3::from(c));
            let dot = |x: [f64; 3], y: [f64; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
            let cross = |x: [f64; 3], y: [f64; 3]| [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]];
            assert!(same((p - q).to_array(), [a[0] - b[0], a[1] - b[1], a[2] - b[2]]));
            assert!(same((p + v).to_array(), [a[0] + b[0], a[1] + b[1], a[2] + b[2]]));
            assert!(same((u * t).to_array(), [a[0] * t, a[1] * t, a[2] * t]));
            assert!(same((t * u).to_array(), [a[0] * t, a[1] * t, a[2] * t]));
            assert!(same(p.lerp(q, t).to_array(), [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]));
            assert!(same(p.midpoint(q).to_array(), [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]), 0.5 * (a[2] + b[2])]));
            assert!(same(u.cross(v).to_array(), cross(a, b)));
            assert_eq!(u.dot(v).to_bits(), dot(a, b).to_bits());
            assert_eq!(u.norm().to_bits(), dot(a, a).sqrt().to_bits());
            assert_eq!(u.det(v, w).to_bits(), dot(a, cross(b, c)).to_bits());
            assert_eq!(p.distance(q).to_bits(), (p - q).norm().to_bits());
        }
    }

    /// The portable atan2 and cbrt agree with the platform's to a few ulps.
    #[test]
    fn portable_functions_match_the_platform() {
        let mut worst: f64 = 0.0;
        for i in 0..2000 {
            let a = -std::f64::consts::PI + i as f64 * (2.0 * std::f64::consts::PI / 1999.0);
            for r in [1e-300, 1e-3, 1.0, 7.5, 1e200] {
                let (y, x) = (r * a.sin(), r * a.cos());
                worst = worst.max((atan2(y, x) - y.atan2(x)).abs());
            }
        }
        assert!(worst < 4e-15, "{worst}");
        assert_eq!(atan2(0.0, 0.0), 0.0);
        assert_eq!(atan2(1.0, 0.0), std::f64::consts::FRAC_PI_2);
        assert_eq!(atan2(0.0, -1.0), std::f64::consts::PI);
        for x in [1e-300, 2.5e-7, 0.3, 1.0, 8.0, 27.0, 1234.5, 1e300, -64.0] {
            let rel: f64 = (cbrt(x) - x.cbrt()).abs() / x.cbrt().abs();
            assert!(rel < 4e-16, "{x}: {} vs {}", cbrt(x), x.cbrt());
        }
        assert_eq!(cbrt(0.0), 0.0);
    }
}

//! Integrals of the electric field along a straight line: ∫ E·t̂ w(s) ds from one point to
//! another, `w` a weight of the distance s along it — the voltage across a gap (w = 1), or a
//! cavity's accelerating voltage seen by a particle crossing it at speed βc
//! (w = e^{jωs/(βc)}; T. P. Wangler, *RF Linear Accelerators*, 2nd ed., Wiley 2008, §2.2).
//!
//! Composite Gauss–Legendre quadrature, three points in each of `n` equal intervals: it samples
//! no end point, so a line ending on a conductor (a pillbox's end walls, where E_z is largest)
//! reads the field inside, never at the wall where locating the point could miss it. Points
//! outside the mesh (inside a conductor) count as no field.

use emcraft_model::{Point3, Vec3};

use super::{Driven, Result, invalid};
use num_complex::Complex64 as C;

/// The most intervals a line is cut into.
pub const MAX_INTERVALS: usize = 100_000;

/// Three-point Gauss–Legendre nodes on [−1, 1] and their weights.
const GAUSS: [(f64, f64); 3] = [(-0.774_596_669_241_483_4, 5.0 / 9.0), (0.0, 8.0 / 9.0), (0.774_596_669_241_483_4, 5.0 / 9.0)];

impl Driven {
    /// ∫ E·t̂ w(s) ds (V for w = 1) from `from` to `to` (m) of the solution `x` at `frequency`,
    /// t̂ the line's direction and s the distance from `from`, in `n` intervals (1 to
    /// [`MAX_INTERVALS`]).
    pub fn line_integral(&self, x: &[C], frequency: f64, (from, to): (Point3, Point3), n: usize, weight: impl Fn(f64) -> C) -> Result<C> {
        let d: Vec3 = to - from;
        let length = d.norm();
        if !(length.is_finite() && length > 0.0) || !from.is_finite() || !to.is_finite() {
            return Err(invalid("line integral", "needs two distinct, finite points"));
        }
        if !(1..=MAX_INTERVALS).contains(&n) {
            return Err(invalid("line integral", format!("takes 1 to {MAX_INTERVALS} intervals, got {n}")));
        }
        let t = d * (1.0 / length);
        let h = length / n as f64;
        let mut total = C::new(0.0, 0.0);
        for k in 0..n {
            let mid = (k as f64 + 0.5) * h;
            for (xi, wi) in GAUSS {
                let s = mid + 0.5 * h * xi;
                let Some((e, _)) = self.fields(x, frequency, from + t * s) else { continue };
                let along = e.x * t.x + e.y * t.y + e.z * t.z;
                total += along * weight(s) * (0.5 * h * wi);
            }
        }
        Ok(total)
    }
}

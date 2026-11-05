//! Rotational symmetry: a model invariant under turns of 2π/n about an axis is meshed on one
//! sector of that angle and solved one azimuthal harmonic at a time, the sector's two cut faces
//! tied by e^{−j2πm/n} (the cyclic condition, the rotational analogue of Floquet's: J.-M. Jin,
//! *The Finite Element Method in Electromagnetics*, 3rd ed., Wiley 2014, ch. on periodic
//! structures). The axis is parallel to z through (`centre`); the sector runs from the ray at
//! φ₀ = [`Rotation::start`] to φ₀ + 2π/n.

use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid};
use crate::geom::Point3;

/// Where a sector starts, as a fraction of its angle ([`Rotation::start`]).
pub const SECTOR_OFFSET: f64 = 0.0937;

/// The fewest and most sectors.
pub const MIN_SECTORS: u32 = 3;
pub const MAX_SECTORS: u32 = 64;

/// A model's rotational symmetry ([`crate::Model::rotation`]).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rotation {
    /// The turns that map the model onto itself: 2π/n.
    pub n: u32,
    /// The harmonic solved: the fields of one sector are e^{−j2πm/n} times the previous one's.
    pub m: u32,
    /// The axis' place (m) in x and y; it runs along z.
    pub centre: [f64; 2],
}

impl Rotation {
    /// Checked: `n` from [`MIN_SECTORS`] to [`MAX_SECTORS`], `m` below `n`, the centre finite.
    ///
    /// ```
    /// use emcraft_model::Rotation;
    ///
    /// let r = Rotation::new(4, 1, [0.0, 0.0])?;
    /// assert_eq!(r.angle(), std::f64::consts::FRAC_PI_2);
    /// let p = r.turned(emcraft_model::Point3::new(1.0, 0.0, 2.0));
    /// assert!(p.x.abs() < 1e-15 && (p.y - 1.0).abs() < 1e-15 && p.z == 2.0);
    /// assert!(Rotation::new(2, 0, [0.0, 0.0]).is_err() && Rotation::new(4, 4, [0.0, 0.0]).is_err());
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    pub fn new(n: u32, m: u32, centre: [f64; 2]) -> Result<Rotation> {
        if !(MIN_SECTORS..=MAX_SECTORS).contains(&n) {
            return Err(invalid("rotation", format!("n must be {MIN_SECTORS} to {MAX_SECTORS}, got {n}")));
        }
        if m >= n {
            return Err(invalid("rotation", format!("the harmonic m must be below n = {n}, got {m}")));
        }
        if !centre.iter().all(|c| c.is_finite()) {
            return Err(invalid("rotation", "the axis' centre must be finite"));
        }
        Ok(Rotation { n, m, centre })
    }

    /// The sector's angle 2π/n.
    pub fn angle(&self) -> f64 {
        std::f64::consts::TAU / f64::from(self.n)
    }

    /// Where the sector starts (radians from +x): a fixed fraction of its angle off the planes
    /// at multiples of 90°, where a tessellation's seams lie, so the cut crosses its triangles
    /// cleanly rather than running along their edges (any start keeps the symmetry). The sector
    /// runs from here to here plus [`Rotation::angle`].
    pub fn start(&self) -> f64 {
        SECTOR_OFFSET * self.angle()
    }

    /// The phase from one sector to the next, 2πm/n.
    pub fn phase(&self) -> f64 {
        std::f64::consts::TAU * f64::from(self.m) / f64::from(self.n)
    }

    /// `p` turned about the axis by `angle` (radians).
    pub fn turned_by(&self, p: Point3, angle: f64) -> Point3 {
        let (s, c) = angle.sin_cos();
        let (x, y) = (p.x - self.centre[0], p.y - self.centre[1]);
        Point3::new(self.centre[0] + c * x - s * y, self.centre[1] + s * x + c * y, p.z)
    }

    /// `p` turned one sector on.
    pub fn turned(&self, p: Point3) -> Point3 {
        self.turned_by(p, self.angle())
    }
}

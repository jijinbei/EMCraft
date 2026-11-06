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

/// A model's rotational symmetry ([`crate::domain::Domain::rotation`]).
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

    /// Whether `p` lies within `tol` (m) on the half-plane from the axis out at `angle`
    /// (radians from x): a sector's cut faces are those at [`Rotation::start`] and one sector on.
    ///
    /// ```
    /// use emcraft_model::{Point3, Rotation};
    ///
    /// let r = Rotation::new(4, 0, [0.0, 0.0])?;
    /// let (s, c) = r.start().sin_cos();
    /// assert!(r.on_half_plane(Point3::new(2.0 * c, 2.0 * s, 1.0), r.start(), 1e-9));
    /// assert!(!r.on_half_plane(Point3::new(-2.0 * c, -2.0 * s, 1.0), r.start(), 1e-9)); // the plane's other half
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    pub fn on_half_plane(&self, p: Point3, angle: f64, tol: f64) -> bool {
        let (x, y) = (p.x - self.centre[0], p.y - self.centre[1]);
        let (s, c) = angle.sin_cos();
        (-x * s + y * c).abs() <= tol && x * c + y * s >= -tol
    }

    /// Whether a body (`shape` placed at `at`) lies inside the sector (`Some(true)`), outside it
    /// (`Some(false)`) or across its cut faces (`None`), by its box across z against the wedge
    /// (convex, its angle at most 2π/3): inside when the box's corners all are, outside when an
    /// axis separates the box from the wedge's triangle out past the box (the separating axis
    /// theorem: the box's sides and the triangle's), a millionth of the box's size to spare.
    ///
    /// ```
    /// use emcraft_model::{Metres, Point3, Rotation, Shape};
    ///
    /// let r = Rotation::new(4, 0, [0.0, 0.0])?; // the sector from 8.4° to 98.4°
    /// let post = Shape::Cylinder { radius: Metres::new(0.001)?, length: Metres::new(0.003)? };
    /// assert_eq!(r.sector_side(&post, Point3::new(0.0035, 0.0035, 0.0)), Some(true)); // at 45°
    /// assert_eq!(r.sector_side(&post, Point3::new(0.0, 0.005, 0.0)), None); // at 90°, across 98.4°
    /// assert_eq!(r.sector_side(&post, Point3::new(0.005, -0.001, 0.0)), Some(false)); // below 0°
    /// assert_eq!(r.sector_side(&post, Point3::new(0.0, 0.0, 0.0)), None); // on the axis
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    pub fn sector_side(&self, shape: &crate::Shape, at: Point3) -> Option<bool> {
        let b = shape.bounds();
        let (lo, hi) = (b.min.to_vec() + at.to_vec(), b.max.to_vec() + at.to_vec());
        let margin = 1e-6 * (hi - lo).norm().max(f64::MIN_POSITIVE);
        let c = self.centre;
        let corners = [[lo.x, lo.y], [hi.x, lo.y], [hi.x, hi.y], [lo.x, hi.y]];
        let (d0, d1) = ([self.start().cos(), self.start().sin()], [(self.start() + self.angle()).cos(), (self.start() + self.angle()).sin()]);
        let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
        let rel = |p: [f64; 2]| [p[0] - c[0], p[1] - c[1]];
        if corners.iter().all(|&p| cross(d0, rel(p)) > margin && cross(rel(p), d1) > margin) {
            return Some(true);
        }
        let reach = 2.0 * corners.iter().map(|&p| rel(p)[0].hypot(rel(p)[1])).fold(0.0, f64::max) + 1.0;
        let tri = [c, [c[0] + reach * d0[0], c[1] + reach * d0[1]], [c[0] + reach * d1[0], c[1] + reach * d1[1]]];
        let mut axes = vec![[1.0, 0.0], [0.0, 1.0]];
        for k in 0..3 {
            let (p, q) = (tri[k], tri[(k + 1) % 3]);
            axes.push([q[1] - p[1], p[0] - q[0]]);
        }
        let span = |pts: &[[f64; 2]], a: [f64; 2]| {
            let n = a[0].hypot(a[1]).max(f64::MIN_POSITIVE);
            pts.iter().map(|p| (p[0] * a[0] + p[1] * a[1]) / n).fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| (l.min(v), h.max(v)))
        };
        let separated = axes.iter().any(|&a| {
            let ((l1, h1), (l2, h2)) = (span(&corners, a), span(&tri, a));
            h1 + margin < l2 || h2 + margin < l1
        });
        separated.then_some(false)
    }

    /// `p` turned one sector on.
    pub fn turned(&self, p: Point3) -> Point3 {
        self.turned_by(p, self.angle())
    }
}

/// A rotational symmetry the model allows ([`crate::Model::find_rotation`]): its axis' centre
/// (m) and the most sectors it can be cut into (`None`: any number, the solid being round).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoundRotation {
    pub centre: [f64; 2],
    pub most: Option<u32>,
}

impl FoundRotation {
    /// Whether `n` sectors fit the symmetry: any for a round solid, a divisor of the most else.
    ///
    /// ```
    /// use emcraft_model::symmetry::rotation::FoundRotation;
    ///
    /// let hexagon = FoundRotation { centre: [0.0, 0.0], most: Some(6) };
    /// assert!(hexagon.allows(3) && hexagon.allows(6) && !hexagon.allows(4) && !hexagon.allows(2));
    /// assert_eq!(hexagon.group(), "C6");
    /// assert!(FoundRotation { centre: [0.0, 0.0], most: None }.allows(64));
    /// ```
    pub fn allows(&self, n: u32) -> bool {
        (MIN_SECTORS..=MAX_SECTORS).contains(&n) && self.most.is_none_or(|k| k % n == 0)
    }

    /// The rotation group's Schoenflies name about the axis: C∞ for a round solid, Cₙ else.
    pub fn group(&self) -> String {
        self.most.map_or_else(|| "C∞".to_owned(), |k| format!("C{k}"))
    }
}

impl crate::Model {
    /// The rotational symmetry about an axis along z the model allows as it is (what
    /// [`crate::Model::set_rotation`] would take): the axis of a cylinder or a sphere, a square
    /// box's middle, the centroid of an imported surface's vertices (a symmetric set's centroid
    /// lies on its axis), with the most sectors that turn it onto itself.
    pub fn find_rotation(&self) -> Option<FoundRotation> {
        let solid = self.domain.solid.as_ref()?;
        let fits = |n: u32, centre: [f64; 2]| self.rotation_issue_of(&Rotation { n, m: 0, centre }).is_none();
        match &solid.shape {
            // Round, and its features about the axis too: any number; else the most there are.
            crate::Shape::Cylinder { .. } | crate::Shape::Sphere { .. } if self.features.is_empty() => {
                let centre = [0.0, 0.0];
                fits(MIN_SECTORS + 1, centre).then_some(FoundRotation { centre, most: None })
            }
            crate::Shape::Cylinder { .. } | crate::Shape::Sphere { .. } => {
                let centre = [0.0, 0.0];
                let all = (MIN_SECTORS..=MAX_SECTORS).all(|n| fits(n, centre));
                if all {
                    return Some(FoundRotation { centre, most: None });
                }
                (MIN_SECTORS..=MAX_SECTORS).rev().find(|&n| fits(n, centre)).map(|n| FoundRotation { centre, most: Some(n) })
            }
            crate::Shape::Box { size } => {
                let centre = [0.5 * size.x, 0.5 * size.y];
                fits(4, centre).then_some(FoundRotation { centre, most: Some(4) })
            }
            crate::Shape::Surface { positions, .. } => {
                let b = solid.shape.bounds();
                let q = 1e-6 * (b.max - b.min).norm().max(f64::MIN_POSITIVE);
                // Each vertex once (a mesh repeats them per face), in a fixed order.
                let distinct: std::collections::BTreeMap<[i64; 3], crate::Point3> =
                    positions.iter().map(|p| ([(p.x / q).round() as i64, (p.y / q).round() as i64, (p.z / q).round() as i64], *p)).collect();
                let count = distinct.len().max(1) as f64;
                let (x, y) = distinct.values().fold((0.0, 0.0), |(x, y), p| (x + p.x, y + p.y));
                let centre = [x / count, y / count];
                (MIN_SECTORS..=MAX_SECTORS).rev().find(|&n| fits(n, centre)).map(|n| FoundRotation { centre, most: Some(n) })
            }
        }
    }

    /// Sets the rotational symmetry (none clears it) after checking the model allows it; the
    /// mesh is dropped (a sector's is not the whole's) unless only the harmonic changes.
    pub fn set_rotation(&mut self, rotation: Option<Rotation>) -> Result<()> {
        if let Some(why) = rotation.and_then(|r| self.rotation_issue_of(&r)) {
            return Err(invalid("rotation", why));
        }
        // Another harmonic keeps the sector and its mesh: only the cut faces' phase moves.
        let same_sector = |a: &Rotation, b: &Rotation| a.n == b.n && a.centre == b.centre;
        if !matches!((&self.domain.rotation, &rotation), (Some(a), Some(b)) if same_sector(a, b)) {
            self.mesh = None;
        }
        self.domain.rotation = rotation;
        Ok(())
    }

    /// Why the model's rotational symmetry cannot hold, if it cannot (the geometry may have
    /// changed since it was set).
    pub fn rotation_issue(&self) -> Option<String> {
        self.domain.rotation.as_ref().and_then(|r| self.rotation_issue_of(r))
    }

    /// Why no rotational symmetry is found: what [`Self::rotation_unsupported`] says, else what
    /// keeps a quarter turn about a round solid's or a box's own axis from holding.
    pub fn rotation_blocker(&self) -> Option<String> {
        if let Some(why) = self.rotation_unsupported() {
            return Some(why);
        }
        let centre = match &self.domain.solid.as_ref()?.shape {
            crate::Shape::Cylinder { .. } | crate::Shape::Sphere { .. } => [0.0, 0.0],
            crate::Shape::Box { size } => [0.5 * size.x, 0.5 * size.y],
            crate::Shape::Surface { .. } => return None,
        };
        self.rotation_issue_of(&Rotation { n: 4, m: 0, centre })
    }

    /// What keeps the model from any rotational symmetry whatever its shape: no solid, or
    /// what a sector cannot carry yet.
    pub fn rotation_unsupported(&self) -> Option<String> {
        if self.domain.solid.is_none() || self.waveguide.is_some() {
            return Some("needs a solid (geometry.solid): a waveguide has no rotational symmetry to cut".into());
        }
        if !self.domain.symmetry.is_empty() || self.domain.periodic.is_some() || self.domain.open.is_some() {
            return Some("not supported yet: with symmetry planes, periodic boundaries or an open boundary".into());
        }
        if !self.lumped.is_empty() || !self.sources.is_empty() {
            return Some("not supported yet: with lumped ports or sources".into());
        }
        None
    }

    /// Whether every feature turns onto a feature of its combination (itself, centred on the axis,
    /// or another, as posts round it do) and back: points sampled inside each (a grid over its
    /// box, cell centres) turned by 2π/n lie inside the other, and the other's turned back inside
    /// it. The features' order is kept by the turn, so their combination is too.
    fn features_turn(&self, r: &Rotation) -> std::result::Result<(), String> {
        const SIDE: usize = 9;
        let samples = |f: &crate::Feature| -> Vec<Point3> {
            let b = f.bounds();
            let at = |i: usize, k: usize| b.min[k] + (i as f64 + 0.5) / SIDE as f64 * (b.max[k] - b.min[k]);
            (0..SIDE * SIDE * SIDE)
                .map(|n| Point3::new(at(n % SIDE, 0), at(n / SIDE % SIDE, 1), at(n / (SIDE * SIDE), 2)))
                .filter(|p| f.contains(*p))
                .collect()
        };
        let inside: Vec<Vec<Point3>> = self.features.iter().map(samples).collect();
        for (i, f) in self.features.iter().enumerate() {
            let onto = self.features.iter().zip(&inside).any(|(g, theirs)| {
                g.combine == f.combine
                    && inside[i].iter().all(|p| g.contains(r.turned(*p)))
                    && theirs.iter().all(|q| f.contains(r.turned_by(*q, -r.angle())))
            });
            if !onto {
                return Err(format!(
                    "feature {} does not turn onto a feature of its kind by 2π/{}: the model is not symmetric under the turn",
                    i + 1,
                    r.n
                ));
            }
        }
        Ok(())
    }

    /// Whether every inclusion turns onto one of the same filling, as [`Self::features_turn`]
    /// checks features (one inside the sector is meshed as it is, one across its cut faces cut
    /// with it, one outside it is its images').
    fn inclusions_turn(&self, r: &Rotation) -> std::result::Result<(), String> {
        const SIDE: usize = 9;
        let samples = |inc: &crate::Inclusion| -> Vec<Point3> {
            let b = inc.bounds();
            let at = |i: usize, k: usize| b.min[k] + (i as f64 + 0.5) / SIDE as f64 * (b.max[k] - b.min[k]);
            (0..SIDE * SIDE * SIDE)
                .map(|n| Point3::new(at(n % SIDE, 0), at(n / SIDE % SIDE, 1), at(n / (SIDE * SIDE), 2)))
                .filter(|p| inc.contains(*p))
                .collect()
        };
        let inside: Vec<Vec<Point3>> = self.inclusions.iter().map(samples).collect();
        for (i, inc) in self.inclusions.iter().enumerate() {
            let onto = self.inclusions.iter().zip(&inside).any(|(o, theirs)| {
                o.material == inc.material
                    && inside[i].iter().all(|p| o.contains(r.turned(*p)))
                    && theirs.iter().all(|q| inc.contains(r.turned_by(*q, -r.angle())))
            });
            if !onto {
                return Err(format!(
                    "inclusion {} does not turn onto an inclusion of its filling by 2π/{}: the model is not symmetric under the turn",
                    i + 1,
                    r.n
                ));
            }
        }
        Ok(())
    }

    /// Whether the model is the same turned by 2π/n about the axis: its solid (a cylinder or a
    /// sphere about its own axis, a box square across z about its middle and n = 4, an imported
    /// surface whose vertices turn onto its vertices), and nothing the sector cannot carry yet.
    fn rotation_issue_of(&self, r: &Rotation) -> Option<String> {
        let issue = || -> std::result::Result<(), String> {
            if let Some(why) = self.rotation_unsupported() {
                return Err(why);
            }
            let solid = self.domain.solid.as_ref().ok_or("needs a solid")?;
            self.features_turn(r)?;
            self.inclusions_turn(r)?;
            let b = solid.shape.bounds();
            let scale = (b.max - b.min).norm().max(f64::MIN_POSITIVE);
            let tol = 1e-9 * scale;
            let on_axis = |x: f64, y: f64| (r.centre[0] - x).abs() <= tol && (r.centre[1] - y).abs() <= tol;
            match &solid.shape {
                crate::Shape::Cylinder { .. } | crate::Shape::Sphere { .. } if on_axis(0.0, 0.0) => Ok(()),
                crate::Shape::Cylinder { .. } | crate::Shape::Sphere { .. } => Err("the axis must be the body's own: x = 0, y = 0".into()),
                crate::Shape::Box { size } => {
                    let square = (size.x - size.y).abs() <= tol;
                    if !(square && r.n == 4) {
                        return Err("a box turns onto itself only by quarter turns about its middle, and only when square across z (n = 4)".into());
                    }
                    if !on_axis(0.5 * size.x, 0.5 * size.y) {
                        return Err(format!("the axis must be the box's middle: x = {} m, y = {} m", 0.5 * size.x, 0.5 * size.y));
                    }
                    Ok(())
                }
                crate::Shape::Surface { positions, .. } => {
                    let key = |p: crate::Point3| {
                        [(p.x / (1e-6 * scale)).round() as i64, (p.y / (1e-6 * scale)).round() as i64, (p.z / (1e-6 * scale)).round() as i64]
                    };
                    let keys: std::collections::BTreeSet<[i64; 3]> = positions.iter().map(|p| key(*p)).collect();
                    let near = |k: [i64; 3]| (-1..=1).any(|i| (-1..=1).any(|j| (-1..=1).any(|l| keys.contains(&[k[0] + i, k[1] + j, k[2] + l]))));
                    if positions.iter().all(|p| near(key(r.turned(*p)))) {
                        Ok(())
                    } else {
                        Err(format!("the imported surface does not turn onto itself by 2π/{} about the axis", r.n))
                    }
                }
            }
        };
        issue().err()
    }
}

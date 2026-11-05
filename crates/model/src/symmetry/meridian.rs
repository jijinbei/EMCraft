//! A body of revolution's meridian profile: the model's outline on the half-plane r ≥ 0 about
//! the z axis, meshed in triangles and solved one azimuthal order at a time ([`MeridianProfile`]).

use crate::error::{Result, invalid};

/// The most points a meridian profile's outline may have.
pub const MAX_PROFILE_POINTS: usize = 4096;

/// A body of revolution's meridian profile ([`crate::Model::meridian_profile`]): its outline on
/// the half-plane r ≥ 0 as points (r, z) in metres and segments between them (a hole's outline
/// among them; segments on r = 0 are the axis; with inclusions, the interfaces between
/// materials too), its filling's εr and μr, and for a profile of cells the material of each.
#[derive(Clone, Debug, PartialEq)]
pub struct MeridianProfile {
    pub points: Vec<[f64; 2]>,
    pub segments: Vec<[u32; 2]>,
    pub eps_r: f64,
    pub mu_r: f64,
    /// The grid's radii and ends and, per cell (r index fastest), its (εr, μr) or none outside.
    pub cells: Option<(Vec<f64>, Vec<f64>, Vec<Option<(f64, f64)>>)>,
}

impl MeridianProfile {
    /// The (εr, μr) at (r, z), `None` outside the body (in a conductor). A sphere's profile is
    /// one material inside its outline.
    pub fn material_at(&self, q: [f64; 2]) -> Option<(f64, f64)> {
        let Some((rs, zs, cells)) = &self.cells else { return Some((self.eps_r, self.mu_r)) };
        let find = |v: &[f64], x: f64| v.windows(2).position(|w| x >= w[0] && x <= w[1]);
        let (i, j) = (find(rs, q[0])?, find(zs, q[1])?);
        cells.get(j * (rs.len() - 1) + i).copied().flatten()
    }

    /// Whether the profile has regions of other materials (its triangles then take theirs).
    pub fn is_layered(&self) -> bool {
        self.cells.as_ref().is_some_and(|(_, _, c)| c.iter().flatten().any(|m| *m != (self.eps_r, self.mu_r)))
    }
}

impl crate::Model {
    /// The model as a body of revolution about the z axis: a cylinder's or a sphere's solid (a
    /// sphere's semicircle in chords of about `size`), and features that are cylinders along z
    /// centred on the axis, added, subtracted or intersected in order. The profile of a cylinder
    /// and its features is the union of the cells of the grid their radii and ends make, those
    /// whose middle the model holds; its outline the cells' sides between inside and out.
    ///
    /// ```
    /// use emcraft_model::{Metres, Model, Shape, Solid};
    ///
    /// let mut m = Model::new();
    /// let shape = Shape::Cylinder { radius: Metres::new(0.01)?, length: Metres::new(0.015)? };
    /// m.set_solid(Solid { shape, material: "vacuum".into() })?;
    /// let p = m.meridian_profile(0.001)?;
    /// assert_eq!(p.segments.len(), 4); // the rectangle 0 ≤ r ≤ 10 mm, 0 ≤ z ≤ 15 mm
    /// assert!(p.points.iter().all(|q| q[0] >= 0.0));
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    pub fn meridian_profile(&self, size: f64) -> Result<MeridianProfile> {
        let solid = self.solid.as_ref().ok_or_else(|| invalid("body of revolution", "needs a solid (geometry.solid)"))?;
        if self.waveguide.is_some() || self.open.is_some() {
            return Err(invalid("body of revolution", "not supported yet: a waveguide or an open boundary"));
        }
        let (_, mat) = self.material(&solid.material)?;
        let (eps_r, mu_r) = (mat.eps_r.get(), mat.mu_r.get());
        if !(size.is_finite() && size > 0.0) {
            return Err(invalid("body of revolution", "the size must be finite and > 0"));
        }
        let on_axis = |at: crate::Point3| at.x.abs() <= 1e-12 && at.y.abs() <= 1e-12;
        let mut radii = vec![0.0];
        let mut ends = Vec::new();
        match &solid.shape {
            crate::Shape::Sphere { radius } => {
                if !self.features.is_empty() || !self.inclusions.is_empty() {
                    return Err(invalid("body of revolution", "not supported yet: features or inclusions in a sphere"));
                }
                let r = radius.get();
                let n = ((std::f64::consts::PI * r / size).ceil() as usize).clamp(8, MAX_PROFILE_POINTS - 1);
                let points: Vec<[f64; 2]> =
                    (0..=n).map(|k| std::f64::consts::PI * k as f64 / n as f64).map(|t| [r * t.sin(), -r * t.cos()]).collect();
                let segments = (0..=n as u32).map(|k| [k, (k + 1) % (n as u32 + 1)]).collect();
                return Ok(MeridianProfile { points, segments, eps_r, mu_r, cells: None });
            }
            crate::Shape::Cylinder { radius, length } => {
                radii.push(radius.get());
                ends.extend([0.0, length.get()]);
            }
            _ => return Err(invalid("body of revolution", "needs a cylinder or a sphere about the z axis")),
        }
        for (i, f) in self.features.iter().enumerate() {
            let crate::Shape::Cylinder { radius, length } = f.shape else {
                return Err(invalid("body of revolution", format!("feature {} is not a cylinder about the axis", i + 1)));
            };
            if f.axis != crate::Axis::Z || !on_axis(f.at) {
                return Err(invalid("body of revolution", format!("feature {} is not a cylinder about the axis", i + 1)));
            }
            radii.push(radius.get());
            ends.extend([f.at.z, f.at.z + length.get()]);
        }
        // Inclusions: cylinders along z about the axis, dielectric or conducting.
        let mut fillings = Vec::with_capacity(self.inclusions.len());
        for (i, inc) in self.inclusions.iter().enumerate() {
            let crate::Shape::Cylinder { radius, length } = inc.shape else {
                return Err(invalid("body of revolution", format!("inclusion {} is not a cylinder about the axis", i + 1)));
            };
            if !on_axis(inc.at) {
                return Err(invalid("body of revolution", format!("inclusion {} is not a cylinder about the axis", i + 1)));
            }
            radii.push(radius.get());
            ends.extend([inc.at.z, inc.at.z + length.get()]);
            fillings.push(match &inc.material {
                Some(name) => {
                    let (_, mt) = self.material(name)?;
                    Some((mt.eps_r.get(), mt.mu_r.get()))
                }
                None => None,
            });
        }
        let sorted = |mut v: Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v.dedup_by(|a, b| (*a - *b).abs() <= 1e-12);
            v
        };
        let (rs, zs) = (sorted(radii), sorted(ends));
        if rs.len() * zs.len() > MAX_PROFILE_POINTS {
            return Err(invalid("body of revolution", "too many features"));
        }
        let inside = |r: f64, z: f64| {
            let p = crate::Point3::new(r, 0.0, z);
            self.features.iter().fold(solid.shape.contains(p), |v, f| match f.combine {
                crate::Combine::Add => v || f.contains(p),
                crate::Combine::Subtract => v && !f.contains(p),
                crate::Combine::Intersect => v && f.contains(p),
            })
        };
        let (nr, nz) = (rs.len() - 1, zs.len() - 1);
        // Each cell's filling: the body's, an inclusion's (the first holding it), or none.
        let filling = |r: f64, z: f64| -> Option<(f64, f64)> {
            if !inside(r, z) {
                return None;
            }
            let p = crate::Point3::new(r, 0.0, z);
            match self.inclusions.iter().position(|inc| inc.contains(p)) {
                Some(k) => fillings.get(k).copied().flatten(),
                None => Some((eps_r, mu_r)),
            }
        };
        let materials: Vec<Option<(f64, f64)>> =
            (0..nr * nz).map(|c| filling(0.5 * (rs[c % nr] + rs[c % nr + 1]), 0.5 * (zs[c / nr] + zs[c / nr + 1]))).collect();
        let cell = |i: usize, j: usize| if i < nr && j < nz { materials[j * nr + i] } else { None };
        let id = |i: usize, j: usize| (j * rs.len() + i) as u32;
        let mut segments = Vec::new();
        for j in 0..nz {
            for i in 0..=nr {
                // The side at r = rs[i] between cells i − 1 and i: an outline or an interface.
                let left = if i > 0 { cell(i - 1, j) } else { None };
                if left != cell(i, j) {
                    segments.push([id(i, j), id(i, j + 1)]);
                }
            }
        }
        for i in 0..nr {
            for j in 0..=nz {
                let below = if j > 0 { cell(i, j - 1) } else { None };
                if below != cell(i, j) {
                    segments.push([id(i, j), id(i + 1, j)]);
                }
            }
        }
        if segments.is_empty() {
            return Err(invalid("body of revolution", "the profile holds nothing"));
        }
        // Only the grid's points the outline uses, renumbered in order.
        let mut used: Vec<u32> = segments.iter().flatten().copied().collect();
        used.sort_unstable();
        used.dedup();
        let renumber: std::collections::BTreeMap<u32, u32> = used.iter().enumerate().map(|(k, &v)| (v, k as u32)).collect();
        let points = used.iter().map(|&v| [rs[v as usize % rs.len()], zs[v as usize / rs.len()]]).collect();
        let segments = segments.iter().map(|s| s.map(|v| renumber[&v])).collect();
        Ok(MeridianProfile { points, segments, eps_r, mu_r, cells: Some((rs, zs, materials)) })
    }
}

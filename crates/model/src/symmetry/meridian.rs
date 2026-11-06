//! A body of revolution's meridian profile: the model's outline on the half-plane r ≥ 0 about
//! the z axis, meshed in triangles and solved one azimuthal order at a time ([`MeridianProfile`]).

use crate::error::{Result, invalid};

/// The most points a meridian profile's outline may have.
pub const MAX_PROFILE_POINTS: usize = 4096;

/// A body of revolution's meridian profile ([`crate::Model::meridian_profile`]): its outline on
/// the half-plane r ≥ 0 as points (r, z) in metres and segments between them (a hole's outline
/// among them; segments on r = 0 are the axis; with inclusions, the interfaces between
/// materials too), its filling's εr and μr, and the largest εr μr in it (its shortest
/// wavelength). What fills a point is [`crate::Model::revolution_fill`]'s.
#[derive(Clone, Debug, PartialEq)]
pub struct MeridianProfile {
    pub points: Vec<[f64; 2]>,
    pub segments: Vec<[u32; 2]>,
    pub eps_r: f64,
    pub mu_r: f64,
    pub densest: f64,
}

/// The most triangles of an imported surface a profile takes the section of.
pub const MAX_SECTION_TRIANGLES: usize = 400_000;

/// A segment of the half-plane, (r, z) to (r, z).
type Segment = [[f64; 2]; 2];

/// An outline's segment and how far the body's true boundary bulges off it (a sphere's chord's
/// sagitta; nothing for a straight side or an imported surface's facet, its own boundary).
type Line = (Segment, f64);

impl crate::Model {
    /// What fills the point (r, z) of the half-plane φ = 0: (εr, μr), `None` outside the body,
    /// in a conductor or at r < 0.
    pub fn revolution_fill(&self, q: [f64; 2]) -> Option<(f64, f64)> {
        if q[0] < 0.0 {
            return None;
        }
        let i = self.material_at(crate::Point3::new(q[0], 0.0, q[1]))?;
        self.materials.get(i).map(|m| (m.eps_r.get(), m.mu_r.get()))
    }

    /// The model as a body of revolution about the z axis: its solid, features and inclusions
    /// each a body of revolution about it — a cylinder along z on the axis, a sphere centred on
    /// it (its semicircle in chords of about `size`) or an imported surface turned about it (its
    /// section by the half-plane φ = 0, its vertices within a hundredth of its extent of that
    /// section) — combined as the model combines them. The outline is the arrangement of their
    /// outlines and the axis, split where they cross; a piece is kept where what fills the model
    /// ([`crate::Model::revolution_fill`]) differs on its two sides.
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
    /// assert!(m.meridian_profile(f64::NAN).is_err());
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    pub fn meridian_profile(&self, size: f64) -> Result<MeridianProfile> {
        let solid = self.domain.solid.as_ref().ok_or_else(|| invalid("body of revolution", "needs a solid (geometry.solid)"))?;
        if self.waveguide.is_some() {
            return Err(invalid("body of revolution", "not supported yet: a waveguide"));
        }
        let (_, mat) = self.material(&solid.material)?;
        let (eps_r, mu_r) = (mat.eps_r.get(), mat.mu_r.get());
        if !(size.is_finite() && size > 0.0) {
            return Err(invalid("body of revolution", "the size must be finite and > 0"));
        }
        let mut lines: Vec<Line> = Vec::new();
        outline(&solid.shape, crate::Point3::ORIGIN, crate::Axis::Z, size, &mut lines)
            .map_err(|why| invalid("body of revolution", format!("the solid {why}")))?;
        for (i, f) in self.features().iter().enumerate() {
            outline(&f.shape, f.at, f.axis, size, &mut lines).map_err(|why| invalid("body of revolution", format!("feature {} {why}", i + 1)))?;
        }
        for (i, inc) in self.inclusions().iter().enumerate() {
            outline(&inc.shape, inc.at, crate::Axis::Z, size, &mut lines)
                .map_err(|why| invalid("body of revolution", format!("inclusion {} {why}", i + 1)))?;
        }
        if lines.len() >= MAX_PROFILE_POINTS {
            return Err(invalid("body of revolution", format!("its outline takes more than {MAX_PROFILE_POINTS} segments: a larger size")));
        }
        let [lo, hi] = lines.iter().flat_map(|l| l.0).fold([f64::INFINITY, f64::NEG_INFINITY], |b, q| [b[0].min(q[1]), b[1].max(q[1])]);
        let extent = lines.iter().flat_map(|l| l.0).fold(hi - lo, |e, q| e.max(q[0]));
        if !(extent.is_finite() && extent > 0.0) {
            return Err(invalid("body of revolution", "the profile holds nothing"));
        }
        lines.push(([[0.0, lo], [0.0, hi]], 0.0));
        // An open boundary's layer: its inner surface a line the triangles keep to.
        let layer = self.layer_lines(size);
        lines.extend(layer.iter().map(|s| (*s, 0.0)));
        let tol = 1e-9 * extent;
        let (points, pieces) = arrangement(&lines, tol);
        // A piece is kept where the model's filling differs across it, looked at past the
        // boundary's bulge off it.
        let mut segments = Vec::new();
        let mut densest = eps_r * mu_r;
        for ([a, b], bulge) in pieces {
            let (pa, pb) = (points[a as usize], points[b as usize]);
            let (dr, dz) = (pb[0] - pa[0], pb[1] - pa[1]);
            let len = dr.hypot(dz);
            let d = ((1e-4 * len).max(1e3 * tol) + 2.0 * bulge) / len;
            let mid = [0.5 * (pa[0] + pb[0]), 0.5 * (pa[1] + pb[1])];
            let (left, right) = (self.revolution_fill([mid[0] - dz * d, mid[1] + dr * d]), self.revolution_fill([mid[0] + dz * d, mid[1] - dr * d]));
            for (e, u) in [left, right].into_iter().flatten() {
                densest = densest.max(e * u);
            }
            let on_layer = layer.iter().any(|s| distance(mid, s) <= 1e3 * tol);
            if left != right || (on_layer && left.is_some() && right.is_some()) {
                segments.push([a, b]);
            }
        }
        if segments.is_empty() {
            return Err(invalid("body of revolution", "the profile holds nothing"));
        }
        // Only the points the outline uses, renumbered in order.
        let mut used: Vec<u32> = segments.iter().flatten().copied().collect();
        used.sort_unstable();
        used.dedup();
        let renumber: std::collections::BTreeMap<u32, u32> = used.iter().enumerate().map(|(k, &v)| (v, k as u32)).collect();
        let points = used.iter().map(|&v| points[v as usize]).collect();
        let segments = segments.iter().map(|s| s.map(|v| renumber[&v])).collect();
        Ok(MeridianProfile { points, segments, eps_r, mu_r, densest })
    }
}

impl crate::Model {
    /// The inner surface of the open boundary's layer on the half-plane (none when closed): a
    /// cylinder's three lines r = R − d, z = d and z = L − d, a sphere's inner semicircle.
    fn layer_lines(&self, size: f64) -> Vec<Segment> {
        let (Some(o), Some(solid)) = (&self.domain.open, &self.domain.solid) else { return Vec::new() };
        let d = o.thickness.get();
        match solid.shape {
            crate::Shape::Cylinder { radius, length } => {
                let (r, l) = (radius.get(), length.get());
                vec![[[r - d, 0.0], [r - d, l]], [[0.0, d], [r, d]], [[0.0, l - d], [r, l - d]]]
            }
            crate::Shape::Sphere { radius } => {
                let r = radius.get() - d;
                let n = ((std::f64::consts::PI * r / size).ceil() as usize).clamp(8, MAX_PROFILE_POINTS / 2);
                let q = |k: usize| {
                    let t = std::f64::consts::PI * k as f64 / n as f64;
                    [if k == 0 || k == n { 0.0 } else { r * t.sin() }, -r * t.cos()]
                };
                (0..n).map(|k| [q(k), q(k + 1)]).collect()
            }
            _ => Vec::new(),
        }
    }
}

/// A body's outline on the half-plane φ = 0, its frame placed at `at` with its own z along
/// `axis`, appended to `lines`; why it is not a body of revolution about the z axis.
fn outline(shape: &crate::Shape, at: crate::Point3, axis: crate::Axis, size: f64, lines: &mut Vec<Line>) -> std::result::Result<(), String> {
    let near_axis = |x: f64, y: f64| x.abs() <= 1e-12 && y.abs() <= 1e-12;
    match shape {
        crate::Shape::Cylinder { radius, length } => {
            if axis != crate::Axis::Z || !near_axis(at.x, at.y) {
                return Err("is a cylinder off the z axis".into());
            }
            let (r, z0, z1) = (radius.get(), at.z, at.z + length.get());
            lines.extend([[[0.0, z0], [r, z0]], [[r, z0], [r, z1]], [[r, z1], [0.0, z1]]].map(|s| (s, 0.0)));
        }
        crate::Shape::Sphere { radius } => {
            if !near_axis(at.x, at.y) {
                return Err("is a sphere off the z axis".into());
            }
            let r = radius.get();
            let n = ((std::f64::consts::PI * r / size).ceil() as usize).clamp(8, MAX_PROFILE_POINTS);
            let q = |k: usize| {
                let t = std::f64::consts::PI * k as f64 / n as f64;
                [r * t.sin(), at.z - r * t.cos()]
            };
            let sagitta = r * (1.0 - (std::f64::consts::PI / (2 * n) as f64).cos());
            lines.extend((0..n).map(|k| ([q(k), q(k + 1)], sagitta)));
        }
        crate::Shape::Box { .. } => return Err("is a box, not a body of revolution".into()),
        crate::Shape::Surface { positions, triangles, .. } => {
            if triangles.len() > MAX_SECTION_TRIANGLES {
                return Err(format!("has more than {MAX_SECTION_TRIANGLES} triangles"));
            }
            let place = |p: crate::Point3| at + axis.turn(p.to_vec());
            let points: Vec<crate::Point3> = positions.iter().copied().map(place).collect();
            let mut own = Vec::new();
            section(&points, triangles, &mut own);
            if own.is_empty() {
                return Err("does not meet the half-plane φ = 0".into());
            }
            // Turned about the axis: its sections by other half-planes near the one at φ = 0.
            let b = crate::geom::Aabb::around(points.iter().copied()).unwrap_or_default();
            let tol = 1e-2 * (0..3).map(|d| b.max[d] - b.min[d]).fold(0.0, f64::max);
            for phi in [1.0f64, 2.3, 4.1] {
                let (c, s) = (phi.cos(), phi.sin());
                let turned: Vec<crate::Point3> = points.iter().map(|p| crate::Point3::new(p.x * c + p.y * s, p.y * c - p.x * s, p.z)).collect();
                let mut other = Vec::new();
                section(&turned, triangles, &mut other);
                let off = other.iter().flatten().find(|q| own.iter().all(|s| distance(**q, s) > tol));
                if let Some(q) = off {
                    return Err(format!(
                        "is not turned about the z axis (at φ = {phi} rad its section reaches (r, z) = ({:.4}, {:.4}) m, off its section at φ = 0)",
                        q[0], q[1]
                    ));
                }
            }
            lines.extend(own.into_iter().map(|s| (s, 0.0)));
        }
    }
    Ok(())
}

/// The section of a closed surface by the half-plane y = 0, x ≥ 0: per triangle crossing the
/// plane, the segment between its two crossing edges (a vertex on the plane counted above it,
/// so neighbours agree), clipped to x ≥ 0, as (r, z) = (x, z).
fn section(points: &[crate::Point3], triangles: &[[u32; 3]], lines: &mut Vec<Segment>) {
    let above = |p: &crate::Point3| p.y >= 0.0;
    for t in triangles {
        let Some(v) = t.iter().map(|&i| points.get(i as usize)).collect::<Option<Vec<_>>>() else { continue };
        let mut cut = Vec::with_capacity(2);
        for k in 0..3 {
            let (a, b) = (v[k], v[(k + 1) % 3]);
            if above(a) != above(b) {
                let s = a.y / (a.y - b.y);
                cut.push([a.x + s * (b.x - a.x), a.z + s * (b.z - a.z)]);
            }
        }
        let [p, q] = match cut.as_slice() {
            [p, q] => [*p, *q],
            _ => continue,
        };
        // Clipped to x ≥ 0.
        let clipped = match (p[0] >= 0.0, q[0] >= 0.0) {
            (true, true) => [p, q],
            (false, false) => continue,
            (inside, _) => {
                let s = p[0] / (p[0] - q[0]);
                let on = [0.0, p[1] + s * (q[1] - p[1])];
                if inside { [p, on] } else { [on, q] }
            }
        };
        if clipped[0] != clipped[1] {
            lines.push(clipped);
        }
    }
}

/// The distance from `q` to the segment `s`.
fn distance(q: [f64; 2], s: &Segment) -> f64 {
    let (d, w) = ([s[1][0] - s[0][0], s[1][1] - s[0][1]], [q[0] - s[0][0], q[1] - s[0][1]]);
    let l = d[0] * d[0] + d[1] * d[1];
    let t = if l > 0.0 { ((w[0] * d[0] + w[1] * d[1]) / l).clamp(0.0, 1.0) } else { 0.0 };
    (w[0] - t * d[0]).hypot(w[1] - t * d[1])
}

/// Points within `tol` of one another taken as one, found through a grid of cells `tol` wide.
struct Points {
    tol: f64,
    cells: std::collections::BTreeMap<(i64, i64), Vec<u32>>,
    all: Vec<[f64; 2]>,
}

impl Points {
    /// The index of `q`, a point near it if there is one.
    fn at(&mut self, mut q: [f64; 2]) -> u32 {
        if q[0].abs() <= self.tol {
            q[0] = 0.0;
        }
        let key = |v: f64| (v / self.tol).floor() as i64;
        let (i, j) = (key(q[0]), key(q[1]));
        for di in -1..=1 {
            for dj in -1..=1 {
                for &k in self.cells.get(&(i + di, j + dj)).into_iter().flatten() {
                    let p = self.all[k as usize];
                    if (p[0] - q[0]).hypot(p[1] - q[1]) <= self.tol {
                        return k;
                    }
                }
            }
        }
        let k = self.all.len() as u32;
        self.all.push(q);
        self.cells.entry((i, j)).or_default().push(k);
        k
    }
}

/// The arrangement of `lines`: their points (those within `tol` merged) and the pieces they
/// split into where they cross one another or another's end lies on them, each piece once (with
/// the largest bulge of the lines it lies on).
fn arrangement(lines: &[Line], tol: f64) -> (Vec<[f64; 2]>, Vec<([u32; 2], f64)>) {
    let mut cuts: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; lines.len()];
    for (i, (a, _)) in lines.iter().enumerate() {
        let da = [a[1][0] - a[0][0], a[1][1] - a[0][1]];
        let la = da[0].hypot(da[1]);
        if la <= tol {
            continue;
        }
        for (j, (b, _)) in lines.iter().enumerate().skip(i + 1) {
            let db = [b[1][0] - b[0][0], b[1][1] - b[0][1]];
            // Boxes apart: nothing to split.
            if a[0][0].min(a[1][0]) > b[0][0].max(b[1][0]) + tol
                || b[0][0].min(b[1][0]) > a[0][0].max(a[1][0]) + tol
                || a[0][1].min(a[1][1]) > b[0][1].max(b[1][1]) + tol
                || b[0][1].min(b[1][1]) > a[0][1].max(a[1][1]) + tol
            {
                continue;
            }
            // Ends on the other.
            let along = |s: &Segment, d: [f64; 2], l: f64, q: [f64; 2]| ((q[0] - s[0][0]) * d[0] + (q[1] - s[0][1]) * d[1]) / (l * l);
            for q in *b {
                if distance(q, a) <= tol {
                    cuts[i].push(along(a, da, la, q));
                }
            }
            let lb = db[0].hypot(db[1]);
            if lb > tol {
                for q in *a {
                    if distance(q, b) <= tol {
                        cuts[j].push(along(b, db, lb, q));
                    }
                }
            }
            // A proper crossing.
            let den = da[0] * db[1] - da[1] * db[0];
            if den.abs() > 1e-12 * la * lb {
                let w = [b[0][0] - a[0][0], b[0][1] - a[0][1]];
                let (s, t) = ((w[0] * db[1] - w[1] * db[0]) / den, (w[0] * da[1] - w[1] * da[0]) / den);
                if (0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&t) {
                    cuts[i].push(s);
                    cuts[j].push(t);
                }
            }
        }
    }
    let mut points = Points { tol, cells: std::collections::BTreeMap::new(), all: Vec::new() };
    let mut pieces: std::collections::BTreeMap<[u32; 2], f64> = std::collections::BTreeMap::new();
    for ((a, bulge), mut ts) in lines.iter().zip(cuts) {
        ts.retain(|t| t.is_finite());
        ts.sort_by(f64::total_cmp);
        let ids: Vec<u32> = ts
            .iter()
            .map(|&t| points.at([a[0][0] + t.clamp(0.0, 1.0) * (a[1][0] - a[0][0]), a[0][1] + t.clamp(0.0, 1.0) * (a[1][1] - a[0][1])]))
            .collect();
        for w in ids.windows(2) {
            if w[0] != w[1] {
                let b = pieces.entry([w[0].min(w[1]), w[0].max(w[1])]).or_insert(0.0);
                *b = b.max(*bulge);
            }
        }
    }
    (points.all, pieces.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use crate::{Combine, Feature, Inclusion, Metres, Model, Point3, Shape, Solid};

    fn cylinder(r: f64, l: f64) -> Shape {
        Shape::Cylinder { radius: Metres::new(r).unwrap(), length: Metres::new(l).unwrap() }
    }

    fn sphere(r: f64) -> Shape {
        Shape::Sphere { radius: Metres::new(r).unwrap() }
    }

    fn model(shape: Shape) -> Model {
        let mut m = Model::new();
        m.set_solid(Solid { shape, material: "vacuum".into() }).unwrap();
        m
    }

    fn turned(profile: &[[f64; 2]], sides: usize) -> Shape {
        Shape::revolved("turned", profile, sides).unwrap()
    }

    /// A cylinder with a sphere taken out of its top: the axis, the bottom, the wall, the top's
    /// ring and the sphere's arc inside the cylinder (the arc's part above the top dropped).
    #[test]
    fn a_sphere_cut_out_of_a_cylinder_leaves_its_arc_in_the_outline() {
        let mut m = model(cylinder(0.01, 0.02));
        m.bodies_mut().push(crate::Body::from(Feature {
            combine: Combine::Subtract,
            shape: sphere(0.005),
            at: Point3::new(0.0, 0.0, 0.02),
            axis: crate::Axis::Z,
        }));
        let p = m.meridian_profile(0.0005).unwrap();
        assert!(p.points.iter().all(|q| q[0] >= 0.0 && q[1] >= -1e-12 && q[1] <= 0.02 + 1e-12));
        // Every point on the arc is 5 mm from the top's centre or on the cylinder's sides.
        let on = |q: &[f64; 2]| {
            ((q[0].hypot(q[1] - 0.02) - 0.005).abs() < 1e-9 && q[1] <= 0.02 + 1e-12)
                || q[0] == 0.0
                || (q[0] - 0.01).abs() < 1e-12
                || q[1].abs() < 1e-12
                || ((q[1] - 0.02).abs() < 1e-12 && q[0] >= 0.005 - 1e-9)
        };
        assert!(p.points.iter().all(on), "{:?}", p.points);
        // The axis runs from the bottom to the sphere's lowest point only.
        let axis_top = p.points.iter().filter(|q| q[0] == 0.0).map(|q| q[1]).fold(f64::NEG_INFINITY, f64::max);
        assert!((axis_top - 0.015).abs() < 1e-9, "{axis_top}");
    }

    /// An imported body turned about z gives the section of its facets; a cone's is its slant
    /// line and its ends.
    #[test]
    fn an_imported_body_of_revolution_gives_its_section() {
        let cone = [[0.0, 0.0], [0.004, 0.0], [0.008, 0.01], [0.0, 0.01]];
        let mut m = model(turned(&cone, 48));
        let p = m.meridian_profile(0.001).unwrap();
        assert_eq!(p.segments.len(), 4, "{:?}", p.points);
        let rim = p.points.iter().map(|q| q[0]).fold(0.0, f64::max);
        assert!((rim - 0.008).abs() < 1e-9, "{rim}");
        // A sphere inclusion inside it: an interface the model's filling changes across.
        m.add_material(crate::Material::new("ptfe", 2.1, 1.0).unwrap()).unwrap();
        m.bodies_mut().push(crate::Body::from(Inclusion { shape: sphere(0.002), at: Point3::new(0.0, 0.0, 0.005), material: Some("ptfe".into()) }));
        let p = m.meridian_profile(0.0005).unwrap();
        assert!(p.segments.len() > 8, "{:?} {:?}", p.points, p.segments);
        assert!((p.densest - 2.1).abs() < 1e-12);
        assert_eq!(m.revolution_fill([0.0005, 0.005]), Some((2.1, 1.0)));
        assert_eq!(m.revolution_fill([0.0035, 0.005]), Some((1.0, 1.0)));
        assert_eq!(m.revolution_fill([0.0075, 0.001]), None);
    }

    /// Bodies that are not turned about the z axis are refused, each named.
    #[test]
    fn bodies_not_turned_about_the_axis_are_refused() {
        let square = turned(&[[0.0, 0.0], [0.004, 0.0], [0.004, 0.01], [0.0, 0.01]], 4);
        let e = model(square).meridian_profile(0.001).unwrap_err().to_string();
        assert!(e.contains("the solid is not turned about the z axis"), "{e}");
        let mut m = model(cylinder(0.01, 0.02));
        m.bodies_mut().push(crate::Body::from(Feature {
            combine: Combine::Add,
            shape: sphere(0.002),
            at: Point3::new(0.005, 0.0, 0.02),
            axis: crate::Axis::Z,
        }));
        let e = m.meridian_profile(0.001).unwrap_err().to_string();
        assert!(e.contains("feature 1 is a sphere off the z axis"), "{e}");
        let e =
            model(Shape::Box { size: crate::shape::BoxSize::new(crate::Vec3::new(0.01, 0.01, 0.01)).unwrap() }).meridian_profile(0.001).unwrap_err();
        assert!(e.to_string().contains("a box"), "{e}");
    }
}

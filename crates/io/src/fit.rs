//! Exact surfaces recovered from an imported face's triangles: a plane, a cylinder, a sphere or a cone
//! fitted to its vertices by least squares, kept only when every vertex lies on it and every
//! triangle hugs it as a chord does; otherwise the face stays faceted.
//!
//! A CAD kernel's tessellation (SolidCraft's 3MF with face tags) puts every vertex on the true
//! surface, so a face's vertices determine it to rounding. The fits:
//! - **plane**: the centroid and the smallest principal axis of the vertices' scatter (total
//!   least squares; Eberly, "Least Squares Fitting of Data by Linear or Quadratic Structures",
//!   Geometric Tools, 2021, §3);
//! - **sphere**: the algebraic fit of |p|² + D·p + G = 0, linear in its unknowns (Kåsa, IEEE
//!   Trans. Instrum. Meas. 25(1), 1976; Pratt, Computer Graphics 21(4), 1987), refined by
//!   Levenberg–Marquardt on the geometric distances | |p − c| − R | (Lukács, Martin, Marshall,
//!   "Faithful least-squares fitting of spheres, cylinders, cones and tori for reliable
//!   segmentation", ECCV 1998);
//! - **cylinder**: the axis from the Gauss map — a cylinder's normals are perpendicular to its
//!   axis, so the axis is the least eigenvector of Σ A n nᵀ over the triangles' area-weighted
//!   normals —, the circle across it by the algebraic fit, then Levenberg–Marquardt on the
//!   distances to the axis (Lukács et al. 1998; Eberly 2021, §7).
//!
//! - **cone**: the axis from the Gauss map again — a cone's normals make one angle with its axis,
//!   so they lie on a plane across it —, the quadric (x − x₀)² + (y − y₀)² = k²(h − h₀)² across
//!   it by linear least squares, then Levenberg–Marquardt on ρ cos φ − h sin φ (Lukács et al.
//!   1998).
//!
//! A fit is accepted when every vertex lies within [`TOLERANCE`] of the body's size of it, and
//! every triangle's centroid within [`CHORD`] of its longest edge (a chord's sagitta is far less;
//! a wrong surface through the same vertices — the sphere through a cylinder's two rims — bulges
//! away from the triangles). Of the curved surfaces that pass, the one closer to the
//! centroids wins. Every loop is bounded, every result deterministic (fixed orders of
//! summation, no hashing), and data that is too little, degenerate or not finite gives no fit.

use std::collections::BTreeMap;

use emcraft_model::{FaceSurface, Point3, Vec3};

use crate::Surface;

/// A vertex may lie this far (relative to the body's size) off the fitted surface.
pub const TOLERANCE: f64 = 1e-6; // unit-factors: relative to the body's size
/// A triangle's centroid may lie this far (relative to its longest edge) off the surface.
const CHORD: f64 = 0.15; // unit-factors: relative to a triangle's longest edge
/// Fewest distinct vertices a curved fit takes.
const MIN_CURVED_POINTS: usize = 8;
/// Radii outside these multiples of the body's size are refused (nearly flat, or a point).
const RADIUS_RANGE: (f64, f64) = (1e-6, 1e3); // unit-factors: relative to the body's size
/// A cone's half-angle is at least this far from 0 and π/2 (radians).
const MIN_CONE_ANGLE: f64 = 1e-3; // unit-factors: an angle in radians
/// Levenberg–Marquardt steps at most.
const MAX_STEPS: usize = 200;
/// Jacobi sweeps at most for a 3 × 3 eigenproblem.
const MAX_SWEEPS: usize = 64;

/// Each face's exact surface (indexed by the face tags in `s.tags`), where one fits. `source`
/// gives each triangle's face before faces were split (`split_faces`): a face whose own vertices
/// fit nothing takes its source face's surface when that one fits (so a cylinder split into
/// patches is still a cylinder). Pass `s.tags` when there was no split.
pub fn fit_faces(s: &Surface, source: &[u32]) -> Vec<Option<FaceSurface>> {
    let faces = s.tags.iter().map(|t| *t as usize + 1).max().unwrap_or(0);
    let mut out = vec![None; faces];
    let Some(scale) = body_scale(&s.positions) else { return out };
    let groups = |tags: &[u32]| {
        let mut g: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for (t, &tag) in tags.iter().enumerate().take(s.triangles.len()) {
            g.entry(tag).or_default().push(t);
        }
        g
    };
    let by_source: BTreeMap<u32, Option<FaceSurface>> = if source.len() == s.triangles.len() {
        groups(source).into_iter().map(|(tag, tris)| (tag, fit(s, &tris, &scale))).collect()
    } else {
        BTreeMap::new()
    };
    for (tag, tris) in groups(&s.tags) {
        let inherited = tris.first().and_then(|&t| source.get(t)).and_then(|src| by_source.get(src).copied().flatten());
        let own = match inherited {
            Some(f) => Some(f),
            None => fit(s, &tris, &scale),
        };
        if let Some(slot) = out.get_mut(tag as usize) {
            *slot = own;
        }
    }
    out
}

/// A body's size (its largest extent) and how far a vertex may lie off a fitted surface.
struct Scale {
    size: f64,
    tol: f64,
}

/// The body's scale: the tolerance [`TOLERANCE`] of its size, plus the rounding of single
/// precision at its largest coordinate (STL stores floats); `None` for no or non-finite points.
fn body_scale(points: &[Point3]) -> Option<Scale> {
    let first = *points.first()?;
    let (mut lo, mut hi, mut far) = (first, first, 0.0f64);
    for p in points {
        lo = lo.min(*p);
        hi = hi.max(*p);
        far = far.max(p.x.abs()).max(p.y.abs()).max(p.z.abs());
    }
    let size = (hi - lo).iter().fold(0.0f64, f64::max);
    let tol = TOLERANCE * size + 1e-7 * far;
    (size > 0.0 && tol.is_finite()).then_some(Scale { size, tol })
}

/// The surface the triangles `tris` of `s` lie on, if one fits.
fn fit(s: &Surface, tris: &[usize], scale: &Scale) -> Option<FaceSurface> {
    let corner = |v: u32| s.positions.get(v as usize).copied();
    let mut vertices: Vec<u32> = tris.iter().filter_map(|&t| s.triangles.get(t)).flatten().copied().collect();
    vertices.sort_unstable();
    vertices.dedup();
    let points: Vec<Point3> = vertices.iter().filter_map(|&v| corner(v)).collect();
    // Each triangle: its corners, area-weighted normal (twice the area) and longest edge.
    let faces: Vec<([Point3; 3], Vec3)> = tris
        .iter()
        .filter_map(|&t| {
            let [a, b, c] = s.triangles.get(t)?.map(corner);
            let (a, b, c) = (a?, b?, c?);
            Some(([a, b, c], (b - a).cross(c - a)))
        })
        .collect();
    if points.len() < 3 || faces.len() != tris.len() || !points.iter().all(|p| p.is_finite()) {
        return None;
    }
    let tol = scale.tol;
    let fits = |f: &FaceSurface| points.iter().all(|p| f.distance(*p) <= tol);
    if let Some(plane) = plane(&points, &faces)
        && fits(&plane)
    {
        return Some(plane);
    }
    if points.len() < MIN_CURVED_POINTS {
        return None;
    }
    let radius_ok = |r: f64| r >= RADIUS_RANGE.0 * scale.size && r <= RADIUS_RANGE.1 * scale.size;
    // The largest distance of a triangle's centroid from the surface, as a fraction of its
    // longest edge; `None` when one exceeds the chord bound.
    let hug = |f: &FaceSurface| -> Option<f64> {
        let mut worst = 0.0f64;
        for ([a, b, c], _) in &faces {
            let g = Point3::centroid([*a, *b, *c])?;
            let edge = a.distance(*b).max(b.distance(*c)).max(c.distance(*a));
            let d = f.distance(g);
            if !(d <= CHORD * edge + tol) {
                return None;
            }
            worst = worst.max(d / edge.max(f64::MIN_POSITIVE));
        }
        Some(worst)
    };
    let mut best: Option<(f64, FaceSurface)> = None;
    for f in [cylinder(&points, &faces, scale), sphere(&points, scale), cone(&points, &faces, scale)].into_iter().flatten() {
        let shaped = match f {
            FaceSurface::Cylinder { radius, .. } | FaceSurface::Sphere { radius, .. } => radius_ok(radius),
            // Nearly a cylinder or nearly a plane: those fit better as such.
            FaceSurface::Cone { half_angle, .. } => half_angle > MIN_CONE_ANGLE && half_angle < std::f64::consts::FRAC_PI_2 - MIN_CONE_ANGLE,
            FaceSurface::Plane { .. } => false,
        };
        if !shaped || f.check().is_err() || !fits(&f) {
            continue;
        }
        if let Some(h) = hug(&f)
            && best.is_none_or(|(b, _)| h < b)
        {
            best = Some((h, f));
        }
    }
    best.map(|(_, f)| f)
}

/// The total least-squares plane of the points, its normal along the triangles' (outwards).
fn plane(points: &[Point3], faces: &[([Point3; 3], Vec3)]) -> Option<FaceSurface> {
    let c = Point3::centroid(points.iter().copied())?;
    let mut m = [[0.0; 3]; 3];
    for p in points {
        let d = *p - c;
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += d[i] * d[j];
            }
        }
    }
    let (values, vectors) = eigen3(m)?;
    // Collinear points (two vanishing spreads) span no plane.
    if !(values[1] > 1e-20 * values[2]) {
        return None;
    }
    let mut n = vectors[0];
    let area: Vec3 = faces.iter().map(|(_, a)| *a).sum();
    if n.dot(area) < 0.0 {
        n = -n;
    }
    Some(FaceSurface::Plane { normal: n.normalized()?, point: c })
}

/// The sphere through the points: the algebraic fit, refined geometrically.
fn sphere(points: &[Point3], scale: &Scale) -> Option<FaceSurface> {
    let c0 = Point3::centroid(points.iter().copied())?;
    // Centred and scaled coordinates condition the normal equations.
    let s = points.iter().map(|p| (*p - c0).norm()).fold(0.0f64, f64::max);
    if !(s > 0.0) {
        return None;
    }
    // |q|² + D·q + G = 0: rows [q, 1], right-hand side −|q|².
    let mut ata = [[0.0; 4]; 4];
    let mut atb = [0.0; 4];
    for p in points {
        let q = (*p - c0) / s;
        let row = [q.x, q.y, q.z, 1.0];
        let rhs = -q.norm_sqr();
        for i in 0..4 {
            for j in 0..4 {
                ata[i][j] += row[i] * row[j];
            }
            atb[i] += row[i] * rhs;
        }
    }
    let x = solve(ata, atb)?;
    let centre = Vec3::new(-0.5 * x[0], -0.5 * x[1], -0.5 * x[2]);
    let r2 = centre.norm_sqr() - x[3];
    if !(r2 > 0.0) {
        return None;
    }
    let mut c = c0 + centre * s;
    let mut r = r2.sqrt() * s;
    // Levenberg–Marquardt on |p − c| − R; parameters (c, R).
    let cost = |c: Point3, r: f64| points.iter().map(|p| ((*p - c).norm() - r).powi(2)).sum::<f64>();
    let mut current = cost(c, r);
    let mut lambda = 1e-3;
    for _ in 0..MAX_STEPS {
        if !(current > (1e-3 * scale.tol).powi(2)) {
            break;
        }
        let mut jtj = [[0.0; 4]; 4];
        let mut jtr = [0.0; 4];
        for p in points {
            let d = *p - c;
            let rho = d.norm();
            let Some(e) = d.normalized() else { continue };
            let row = [-e.x, -e.y, -e.z, -1.0];
            let res = rho - r;
            for i in 0..4 {
                for j in 0..4 {
                    jtj[i][j] += row[i] * row[j];
                }
                jtr[i] += row[i] * res;
            }
        }
        let Some((step, next)) = damped(
            jtj,
            jtr,
            &mut lambda,
            |x| {
                let (c2, r2) = (c + Vec3::new(x[0], x[1], x[2]), r + x[3]);
                (cost(c2, r2), (c2, r2))
            },
            current,
        ) else {
            break;
        };
        (c, r) = next.1;
        current = next.0;
        if step <= 1e-15 * scale.size {
            break;
        }
    }
    (r > 0.0 && c.is_finite()).then_some(FaceSurface::Sphere { center: c, radius: r })
}

/// The cylinder through the points: its axis from the triangles' normals, the circle across it,
/// refined geometrically.
fn cylinder(points: &[Point3], faces: &[([Point3; 3], Vec3)], scale: &Scale) -> Option<FaceSurface> {
    // Σ A n nᵀ = Σ (2A n)(2A n)ᵀ / (4A): the area-weighted second moment of the unit normals.
    let mut m = [[0.0; 3]; 3];
    for (_, a) in faces {
        let w = a.norm();
        if !(w > 0.0) {
            continue;
        }
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += a[i] * a[j] / w;
            }
        }
    }
    let (values, vectors) = eigen3(m)?;
    // Normals all alike (a flat patch) leave the axis undetermined.
    if !(values[1] > 1e-12 * values[2]) {
        return None;
    }
    let mut axis = vectors[0].normalized()?;
    let c0 = Point3::centroid(points.iter().copied())?;
    let (u, v) = basis(axis);
    // The circle across the axis: x² + y² + D x + E y + F = 0 on centred, scaled coordinates.
    let s = points.iter().map(|p| (*p - c0).norm()).fold(0.0f64, f64::max);
    if !(s > 0.0) {
        return None;
    }
    let mut ata = [[0.0; 3]; 3];
    let mut atb = [0.0; 3];
    for p in points {
        let d = (*p - c0) / s;
        let (x, y) = (d.dot(u), d.dot(v));
        let row = [x, y, 1.0];
        let rhs = -(x * x + y * y);
        for i in 0..3 {
            for j in 0..3 {
                ata[i][j] += row[i] * row[j];
            }
            atb[i] += row[i] * rhs;
        }
    }
    let x = solve(ata, atb)?;
    let (cx, cy) = (-0.5 * x[0], -0.5 * x[1]);
    let r2 = cx * cx + cy * cy - x[2];
    if !(r2 > 0.0) {
        return None;
    }
    let mut origin = c0 + (u * cx + v * cy) * s;
    let mut r = r2.sqrt() * s;
    let distance = |origin: Point3, axis: Vec3, p: Point3| {
        let w = p - origin;
        (w - axis * w.dot(axis)).norm()
    };
    let cost = |origin: Point3, axis: Vec3, r: f64| points.iter().map(|p| (distance(origin, axis, *p) - r).powi(2)).sum::<f64>();
    let mut current = cost(origin, axis, r);
    let mut lambda = 1e-3;
    // Levenberg–Marquardt on the distance to the axis minus R; parameters: the axis turned by
    // (α, β) towards u and v, the origin moved by (γ, δ) along them, and R.
    for _ in 0..MAX_STEPS {
        if !(current > (1e-3 * scale.tol).powi(2)) {
            break;
        }
        let (u, v) = basis(axis);
        let mut jtj = [[0.0; 5]; 5];
        let mut jtr = [0.0; 5];
        for p in points {
            let w = *p - origin;
            let along = w.dot(axis);
            let q = w - axis * along;
            let rho = q.norm();
            let Some(e) = q.normalized() else { continue };
            let (eu, ev) = (e.dot(u), e.dot(v));
            let row = [-along * eu, -along * ev, -eu, -ev, -1.0];
            let res = rho - r;
            for i in 0..5 {
                for j in 0..5 {
                    jtj[i][j] += row[i] * row[j];
                }
                jtr[i] += row[i] * res;
            }
        }
        let trial = |x: [f64; 5]| {
            let a = (axis + u * x[0] + v * x[1]).normalized().unwrap_or(axis);
            let o = origin + u * x[2] + v * x[3];
            // The origin kept on the plane through the centroid across the axis.
            let o = o - a * (o - c0).dot(a);
            let r2 = r + x[4];
            (cost(o, a, r2), (o, a, r2))
        };
        let Some((step, next)) = damped(jtj, jtr, &mut lambda, trial, current) else { break };
        (origin, axis, r) = next.1;
        current = next.0;
        if step <= 1e-15 * scale.size {
            break;
        }
    }
    // A canonical direction: the largest component positive.
    let k = axis.dominant_axis();
    if axis[k] < 0.0 {
        axis = -axis;
    }
    (r > 0.0 && origin.is_finite()).then_some(FaceSurface::Cylinder { origin, axis, radius: r })
}

/// The cone through the points: its axis from the triangles' normals (a cone's normals make one
/// angle with its axis, so they lie on a plane across it: the least eigenvector of their
/// scatter about their mean), the quadric (x − x₀)² + (y − y₀)² = k²(h − h₀)² across it by
/// linear least squares, refined geometrically.
fn cone(points: &[Point3], faces: &[([Point3; 3], Vec3)], scale: &Scale) -> Option<FaceSurface> {
    let mut total = 0.0;
    let mut mean = Vec3::ZERO;
    for (_, a) in faces {
        let w = a.norm();
        if w > 0.0 {
            mean += *a;
            total += w;
        }
    }
    if !(total > 0.0) {
        return None;
    }
    let mean = mean / total;
    let mut m = [[0.0; 3]; 3];
    for (_, a) in faces {
        let w = a.norm();
        if !(w > 0.0) {
            continue;
        }
        let n = *a / w - mean;
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += w * n[i] * n[j];
            }
        }
    }
    let (values, vectors) = eigen3(m)?;
    if !(values[1] > 1e-12 * values[2]) {
        return None;
    }
    let mut axis = vectors[0].normalized()?;
    let c0 = Point3::centroid(points.iter().copied())?;
    let (u, v) = basis(axis);
    let s = points.iter().map(|p| (*p - c0).norm()).fold(0.0f64, f64::max);
    if !(s > 0.0) {
        return None;
    }
    // The axis' foot (x₀, y₀) from x² + y² + D x + E y + F + G h = 0 (ρ² taken linear in h:
    // exact through two rings, a start otherwise), then ρ = k h + b by least squares.
    let local: Vec<[f64; 3]> = points
        .iter()
        .map(|p| {
            let d = (*p - c0) / s;
            [d.dot(u), d.dot(v), d.dot(axis)]
        })
        .collect();
    let mut ata = [[0.0; 4]; 4];
    let mut atb = [0.0; 4];
    for &[x, y, h] in &local {
        let row = [x, y, 1.0, h];
        let rhs = -(x * x + y * y);
        for i in 0..4 {
            for j in 0..4 {
                ata[i][j] += row[i] * row[j];
            }
            atb[i] += row[i] * rhs;
        }
    }
    let foot = solve(ata, atb)?;
    let (x0, y0) = (-0.5 * foot[0], -0.5 * foot[1]);
    let mut line = [[0.0; 2]; 2];
    let mut rhs = [0.0; 2];
    for &[x, y, h] in &local {
        let rho = ((x - x0).powi(2) + (y - y0).powi(2)).sqrt();
        let row = [h, 1.0];
        for i in 0..2 {
            for j in 0..2 {
                line[i][j] += row[i] * row[j];
            }
            rhs[i] += row[i] * rho;
        }
    }
    let [k, b] = solve(line, rhs)?;
    if !(k != 0.0) {
        return None;
    }
    let mut apex = c0 + (u * x0 + v * y0 + axis * (-b / k)) * s;
    // The axis points from the apex into the cone, towards the points.
    if points.iter().map(|p| (*p - apex).dot(axis)).sum::<f64>() < 0.0 {
        axis = -axis;
    }
    let mut angle = k.abs().atan();
    let residual = |apex: Point3, axis: Vec3, angle: f64, p: Point3| {
        let d = p - apex;
        let h = d.dot(axis);
        (d - axis * h).norm() * angle.cos() - h * angle.sin()
    };
    let cost = |apex: Point3, axis: Vec3, angle: f64| points.iter().map(|p| residual(apex, axis, angle, *p).powi(2)).sum::<f64>();
    let mut current = cost(apex, axis, angle);
    let mut lambda = 1e-3;
    // Levenberg–Marquardt on ρ cos φ − h sin φ; parameters: the apex moved (3), the axis turned
    // towards u and v (2), and the half-angle φ.
    for _ in 0..MAX_STEPS {
        if !(current > (1e-3 * scale.tol).powi(2)) {
            break;
        }
        let (u, v) = basis(axis);
        let (sn, cs) = angle.sin_cos();
        let mut jtj = [[0.0; 6]; 6];
        let mut jtr = [0.0; 6];
        for p in points {
            let d = *p - apex;
            let h = d.dot(axis);
            let q = d - axis * h;
            let rho = q.norm();
            let Some(e) = q.normalized() else { continue };
            let g = axis * sn - e * cs;
            let (eu, ev) = (e.dot(u), e.dot(v));
            let row = [g.x, g.y, g.z, -cs * h * eu - sn * d.dot(u), -cs * h * ev - sn * d.dot(v), -rho * sn - h * cs];
            let res = rho * cs - h * sn;
            for i in 0..6 {
                for j in 0..6 {
                    jtj[i][j] += row[i] * row[j];
                }
                jtr[i] += row[i] * res;
            }
        }
        let trial = |x: [f64; 6]| {
            let o = apex + Vec3::new(x[0], x[1], x[2]);
            let a = (axis + u * x[3] + v * x[4]).normalized().unwrap_or(axis);
            let phi = angle + x[5];
            (cost(o, a, phi), (o, a, phi))
        };
        let Some((step, next)) = damped(jtj, jtr, &mut lambda, trial, current) else { break };
        (apex, axis, angle) = next.1;
        current = next.0;
        if step <= 1e-15 * scale.size {
            break;
        }
    }
    (apex.is_finite() && angle.is_finite()).then_some(FaceSurface::Cone { apex, axis, half_angle: angle })
}

/// One Levenberg–Marquardt step: (JᵀJ + λ diag JᵀJ) x = −Jᵀr, λ raised until the cost falls
/// (at most a few times), lowered after. Returns the step's size and the accepted
/// (cost, parameters), or `None` when no step lowers the cost.
fn damped<const N: usize, T>(
    jtj: [[f64; N]; N],
    jtr: [f64; N],
    lambda: &mut f64,
    trial: impl Fn([f64; N]) -> (f64, T),
    current: f64,
) -> Option<(f64, (f64, T))> {
    for _ in 0..16 {
        let mut a = jtj;
        for i in 0..N {
            a[i][i] += *lambda * jtj[i][i].max(1e-300);
        }
        let x = solve(a, jtr.map(|v| -v))?;
        let (c, t) = trial(x);
        if c < current {
            *lambda = (*lambda * 0.1).max(1e-12);
            let size = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            return Some((size, (c, t)));
        }
        *lambda *= 10.0;
        if !(*lambda < 1e12) {
            return None;
        }
    }
    None
}

/// Two unit vectors completing `a` to a right-handed orthonormal basis.
fn basis(a: Vec3) -> (Vec3, Vec3) {
    let t = if a.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) };
    let u = a.cross(t).normalized().unwrap_or(Vec3::new(0.0, 0.0, 1.0));
    (u, a.cross(u))
}

/// Solves a small dense system by Gaussian elimination with partial pivoting; `None` when it
/// is singular or the result not finite.
fn solve<const N: usize>(mut a: [[f64; N]; N], mut b: [f64; N]) -> Option<[f64; N]> {
    let size = a.iter().flatten().fold(0.0f64, |m, v| m.max(v.abs()));
    if !(size > 0.0 && size.is_finite()) {
        return None;
    }
    for k in 0..N {
        let mut p = k;
        for i in k + 1..N {
            if a[i][k].abs() > a[p][k].abs() {
                p = i;
            }
        }
        if !(a[p][k].abs() > 1e-14 * size) {
            return None;
        }
        a.swap(k, p);
        b.swap(k, p);
        for i in k + 1..N {
            let f = a[i][k] / a[k][k];
            for j in k..N {
                a[i][j] -= f * a[k][j];
            }
            b[i] -= f * b[k];
        }
    }
    let mut x = [0.0; N];
    for k in (0..N).rev() {
        let mut s = b[k];
        for j in k + 1..N {
            s -= a[k][j] * x[j];
        }
        x[k] = s / a[k][k];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// The eigenvalues (ascending) and unit eigenvectors of a symmetric 3 × 3 matrix by cyclic
/// Jacobi rotations; `None` for a matrix that is not finite.
fn eigen3(mut m: [[f64; 3]; 3]) -> Option<([f64; 3], [Vec3; 3])> {
    if !m.iter().flatten().all(|v| v.is_finite()) {
        return None;
    }
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..MAX_SWEEPS {
        let off = m[0][1].abs() + m[0][2].abs() + m[1][2].abs();
        let diag = m[0][0].abs() + m[1][1].abs() + m[2][2].abs();
        if !(off > 1e-18 * diag) {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if m[p][q] == 0.0 {
                continue;
            }
            let theta = (m[q][q] - m[p][p]) / (2.0 * m[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for k in 0..3 {
                let (mkp, mkq) = (m[k][p], m[k][q]);
                m[k][p] = c * mkp - s * mkq;
                m[k][q] = s * mkp + c * mkq;
            }
            for k in 0..3 {
                let (mpk, mqk) = (m[p][k], m[q][k]);
                m[p][k] = c * mpk - s * mqk;
                m[q][k] = s * mpk + c * mqk;
            }
            for row in v.iter_mut() {
                let (vp, vq) = (row[p], row[q]);
                row[p] = c * vp - s * vq;
                row[q] = s * vp + c * vq;
            }
        }
    }
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| m[a][a].total_cmp(&m[b][b]));
    let values = order.map(|k| m[k][k]);
    let vectors = order.map(|k| Vec3::new(v[0][k], v[1][k], v[2][k]));
    Some((values, vectors))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed cylinder (axis through `o` along `a`, radius `r`, length `len`), its side
    /// tessellated with `n` facets round and `m` rings along, the caps as fans: tags side 0,
    /// caps 1 and 2.
    fn cylinder_surface(o: Point3, a: Vec3, r: f64, len: f64, n: usize, m: usize) -> Surface {
        frustum_surface(o, a, [r, r], len, n, m)
    }

    /// [`cylinder_surface`] with radius `r[0]` at its base and `r[1]` at its top.
    fn frustum_surface(o: Point3, a: Vec3, r: [f64; 2], len: f64, n: usize, m: usize) -> Surface {
        let (u, v) = basis(a);
        let mut s = Surface::default();
        for k in 0..=m {
            let f = k as f64 / m as f64;
            for i in 0..n {
                let t = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
                s.positions.push(o + a * (len * f) + (u * t.cos() + v * t.sin()) * (r[0] + (r[1] - r[0]) * f));
            }
        }
        let id = |k: usize, i: usize| (k * n + i % n) as u32;
        for k in 0..m {
            for i in 0..n {
                s.triangles.push([id(k, i), id(k, i + 1), id(k + 1, i + 1)]);
                s.triangles.push([id(k, i), id(k + 1, i + 1), id(k + 1, i)]);
                s.tags.extend([0, 0]);
            }
        }
        let (bottom, top) = (s.positions.len() as u32, s.positions.len() as u32 + 1);
        s.positions.push(o);
        s.positions.push(o + a * len);
        for i in 0..n {
            s.triangles.push([bottom, id(0, i + 1), id(0, i)]);
            s.triangles.push([top, id(m, i), id(m, i + 1)]);
            s.tags.extend([1, 2]);
        }
        s
    }

    /// A sphere: an icosahedron subdivided `levels` times, its vertices on the sphere; one tag.
    fn sphere_surface(c: Point3, r: f64, levels: usize) -> Surface {
        let g = (1.0 + 5f64.sqrt()) / 2.0;
        let mut pts: Vec<Vec3> = [
            (-1.0, g, 0.0),
            (1.0, g, 0.0),
            (-1.0, -g, 0.0),
            (1.0, -g, 0.0),
            (0.0, -1.0, g),
            (0.0, 1.0, g),
            (0.0, -1.0, -g),
            (0.0, 1.0, -g),
            (g, 0.0, -1.0),
            (g, 0.0, 1.0),
            (-g, 0.0, -1.0),
            (-g, 0.0, 1.0),
        ]
        .iter()
        .map(|&(x, y, z)| Vec3::new(x, y, z).normalized().unwrap())
        .collect();
        let mut tris: Vec<[u32; 3]> = vec![
            [0, 11, 5],
            [0, 5, 1],
            [0, 1, 7],
            [0, 7, 10],
            [0, 10, 11],
            [1, 5, 9],
            [5, 11, 4],
            [11, 10, 2],
            [10, 7, 6],
            [7, 1, 8],
            [3, 9, 4],
            [3, 4, 2],
            [3, 2, 6],
            [3, 6, 8],
            [3, 8, 9],
            [4, 9, 5],
            [2, 4, 11],
            [6, 2, 10],
            [8, 6, 7],
            [9, 8, 1],
        ];
        for _ in 0..levels {
            let mut mid = BTreeMap::new();
            let mut next = Vec::new();
            for t in &tris {
                let mut m = [0u32; 3];
                for e in 0..3 {
                    let (a, b) = (t[e].min(t[(e + 1) % 3]), t[e].max(t[(e + 1) % 3]));
                    m[e] = *mid.entry((a, b)).or_insert_with(|| {
                        pts.push((pts[a as usize] + pts[b as usize]).normalized().unwrap());
                        pts.len() as u32 - 1
                    });
                }
                next.extend([[t[0], m[0], m[2]], [t[1], m[1], m[0]], [t[2], m[2], m[1]], m]);
            }
            tris = next;
        }
        let tags = vec![0; tris.len()];
        Surface { name: "ball".into(), positions: pts.iter().map(|p| c + *p * r).collect(), triangles: tris, tags }
    }

    #[test]
    fn a_tilted_cylinder_is_recovered_to_rounding() {
        let a = Vec3::new(0.3, -0.5, 0.8).normalized().unwrap();
        let o = Point3::new(0.011, -0.004, 0.02);
        let s = cylinder_surface(o, a, 0.0015, 0.01, 26, 1);
        let fits = fit_faces(&s, &s.tags);
        let Some(FaceSurface::Cylinder { origin, axis, radius }) = fits[0] else { panic!("{:?}", fits[0]) };
        assert!((radius - 0.0015).abs() < 1e-14, "{radius}");
        assert!(axis.cross(a).norm() < 1e-12, "{axis:?}");
        assert!((origin - o).cross(a).norm() < 1e-14);
        // The caps are planes along the axis.
        for (f, sign) in [(fits[1], -1.0), (fits[2], 1.0)] {
            let Some(FaceSurface::Plane { normal, .. }) = f else { panic!("{f:?}") };
            assert!((normal - a * sign).norm() < 1e-12, "{normal:?}");
        }
        // Rings along it too, and a coarse polygon (8 facets) still a cylinder.
        let fine = cylinder_surface(o, a, 0.0015, 0.01, 40, 5);
        assert!(matches!(fit_faces(&fine, &fine.tags)[0], Some(FaceSurface::Cylinder { .. })));
        let coarse = cylinder_surface(o, a, 0.0015, 0.01, 8, 1);
        assert!(matches!(fit_faces(&coarse, &coarse.tags)[0], Some(FaceSurface::Cylinder { .. })));
        // The same answer bit for bit.
        assert_eq!(fit_faces(&s, &s.tags), fits);
    }

    #[test]
    fn a_short_cylinder_is_not_taken_for_the_sphere_through_its_rims() {
        // Two rims 0.2 r apart lie on a sphere as well; the triangles tell them apart.
        let s = cylinder_surface(Point3::ORIGIN, Vec3::new(0.0, 0.0, 1.0), 1.0, 0.2, 32, 1);
        assert!(matches!(fit_faces(&s, &s.tags)[0], Some(FaceSurface::Cylinder { .. })));
        let tall = cylinder_surface(Point3::ORIGIN, Vec3::new(0.0, 1.0, 0.0), 1.0, 7.0, 32, 1);
        assert!(matches!(fit_faces(&tall, &tall.tags)[0], Some(FaceSurface::Cylinder { .. })));
    }

    #[test]
    fn a_frustum_is_a_cone_not_the_sphere_through_its_rims() {
        let a = Vec3::new(0.0, 0.6, 0.8);
        for (r, len) in [([1.0, 0.5], 1.0), ([2.0, 2.2], 0.1), ([0.4, 1.6], 3.0)] {
            let s = frustum_surface(Point3::new(0.5, 0.0, 0.0), a, r, len, 32, 1);
            let fits = fit_faces(&s, &s.tags);
            let Some(FaceSurface::Cone { apex, axis, half_angle }) = fits[0] else { panic!("{r:?}: {:?}", fits[0]) };
            let slope = ((r[1] - r[0]) / len).abs();
            assert!((half_angle.tan() - slope).abs() < 1e-10, "{half_angle}");
            assert!(axis.cross(a).norm() < 1e-10);
            // The apex where the radius falls to nothing.
            let along = -r[0] / (r[1] - r[0]) * len;
            assert!((apex - (Point3::new(0.5, 0.0, 0.0) + a * along)).norm() < 1e-9, "{apex:?}");
        }
    }

    #[test]
    fn a_sphere_is_recovered_to_rounding() {
        let c = Point3::new(0.01, 0.02, -0.03);
        let s = sphere_surface(c, 0.004, 3);
        let fits = fit_faces(&s, &s.tags);
        let Some(FaceSurface::Sphere { center, radius }) = fits[0] else { panic!("{:?}", fits[0]) };
        assert!((radius - 0.004).abs() < 1e-15 && (center - c).norm() < 1e-15, "{center:?} {radius}");
    }

    #[test]
    fn a_patch_split_off_a_face_keeps_its_source_surface() {
        let mut s = cylinder_surface(Point3::ORIGIN, Vec3::new(0.0, 0.0, 1.0), 1.0, 1.0, 24, 2);
        let source = s.tags.clone();
        // Each side triangle its own face: too few points to fit, so they inherit the side's.
        let n = s.tags.len() as u32;
        s.tags = (0..n).collect();
        let fits = fit_faces(&s, &source);
        assert_eq!(fits.len(), n as usize);
        assert!(fits[..96].iter().all(|f| matches!(f, Some(FaceSurface::Cylinder { .. }))));
        assert!(fits[96..].iter().all(|f| matches!(f, Some(FaceSurface::Plane { .. }))));
    }

    #[test]
    fn surfaces_that_fit_nothing_stay_faceted() {
        // A cylinder whose vertices are moved off it by 1e-4 of its radius: neither fits.
        let mut s = cylinder_surface(Point3::ORIGIN, Vec3::new(0.0, 0.0, 1.0), 1.0, 1.0, 24, 3);
        for (i, p) in s.positions.iter_mut().enumerate() {
            let wobble = 1e-4 * (((i * 7919) % 13) as f64 / 13.0 - 0.5);
            p.x *= 1.0 + wobble;
        }
        assert_eq!(fit_faces(&s, &s.tags)[0], None);
        // An ellipsoid (a sphere stretched by 1 %).
        let mut e = sphere_surface(Point3::ORIGIN, 1.0, 2);
        e.positions.iter_mut().for_each(|p| p.z *= 1.01);
        assert_eq!(fit_faces(&e, &e.tags)[0], None);
        // A box's faces are planes.
        let mut b = crate::stl::tests::cube_surface(0.02, 0.01, 0.03);
        let source = b.tags.clone();
        crate::split_faces(&mut b, 30.0);
        assert!(fit_faces(&b, &source).iter().all(|f| matches!(f, Some(FaceSurface::Plane { .. }))));
    }

    #[test]
    fn degenerate_and_hostile_input_gives_no_fit_and_no_panic() {
        let tri = |positions: Vec<Point3>, triangles: Vec<[u32; 3]>| {
            let tags = vec![0; triangles.len()];
            Surface { name: String::new(), positions, triangles, tags }
        };
        // Nothing, collinear points, a single point, missing corners.
        assert!(fit_faces(&Surface::default(), &[]).is_empty());
        let line: Vec<Point3> = (0..10).map(|i| Point3::new(i as f64, 0.0, 0.0)).collect();
        assert_eq!(fit_faces(&tri(line, vec![[0, 1, 2], [3, 4, 5], [6, 7, 8]]), &[0, 0, 0]), vec![None]);
        let same = vec![Point3::new(1.0, 1.0, 1.0); 3];
        assert_eq!(fit_faces(&tri(same, vec![[0, 1, 2]]), &[0]), vec![None]);
        let few = vec![Point3::ORIGIN, Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)];
        assert_eq!(fit_faces(&tri(few.clone(), vec![[0, 1, 9]]), &[0]), vec![None]);
        // NaN and infinite coordinates.
        for bad in [f64::NAN, f64::INFINITY] {
            let mut s = sphere_surface(Point3::ORIGIN, 1.0, 1);
            s.positions[3].y = bad;
            assert_eq!(fit_faces(&s, &s.tags), vec![None]);
        }
        // A source of the wrong length is ignored.
        let s = sphere_surface(Point3::ORIGIN, 1.0, 1);
        assert!(matches!(fit_faces(&s, &[0])[0], Some(FaceSurface::Sphere { .. })));
        // A tiny sphere far from the origin is below the coordinates' rounding: not a sphere.
        let far = sphere_surface(Point3::new(1e3, 0.0, 0.0), 1e-7, 1);
        assert!(!matches!(fit_faces(&far, &far.tags)[0], Some(FaceSurface::Sphere { .. })));
    }

    #[test]
    fn small_eigenproblems_and_systems() {
        let (values, vectors) = eigen3([[2.0, 1.0, 0.0], [1.0, 2.0, 0.0], [0.0, 0.0, 5.0]]).unwrap();
        assert!((values[0] - 1.0).abs() < 1e-14 && (values[1] - 3.0).abs() < 1e-14 && (values[2] - 5.0).abs() < 1e-14);
        assert!(vectors[0].dot(Vec3::new(1.0, -1.0, 0.0)).abs() > 1.414);
        assert!(eigen3([[f64::NAN, 0.0, 0.0], [0.0; 3], [0.0; 3]]).is_none());
        assert_eq!(solve([[2.0, 0.0], [0.0, 4.0]], [2.0, 2.0]), Some([1.0, 0.5]));
        assert!(solve([[1.0, 1.0], [1.0, 1.0]], [1.0, 2.0]).is_none());
        assert!(solve([[0.0; 2]; 2], [1.0, 2.0]).is_none());
    }
}

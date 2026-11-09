//! A NURBS patch: a face's exact surface as a design gives it (SolidCraft's exchange snapshot),
//! evaluated with its first derivatives and inverted for the point nearest another — what bends
//! the curved elements onto a free-form face (an equation curve extruded, a lofted wall).
//!
//! Evaluation: the B-spline basis and its derivatives by the Cox–de Boor recurrence (L. Piegl,
//! W. Tiller, *The NURBS Book*, 2nd ed., Springer 1997, algorithms A2.1–A2.3), the rational
//! surface in homogeneous coordinates and its derivatives by the quotient rule (§4.5). The
//! nearest point (§6.1): the parameters of the nearest of a grid of samples taken once
//! ([`Projector`]), then Gauss–Newton steps on |S(u, v) − p|², the parameters kept in the patch's
//! domain, at most [`MAX_STEPS`] of them.

use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid};
use crate::geom::{Point3, Vec3};

/// The highest degree, most control points a direction and most samples a projector takes.
pub const MAX_DEGREE: usize = 10;
pub const MAX_POINTS: usize = 4096;
const MAX_SAMPLES: usize = 200_000;
/// The most Gauss–Newton steps a projection takes.
pub const MAX_STEPS: usize = 30;

/// A knot vector: as many knots as control points plus the degree plus one, non-decreasing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Knots {
    pub degree: usize,
    pub knots: Vec<f64>,
}

impl Knots {
    fn domain(&self) -> (f64, f64) {
        let k = &self.knots;
        (k.get(self.degree).copied().unwrap_or(0.0), k.get(k.len().saturating_sub(self.degree + 1)).copied().unwrap_or(1.0))
    }

    /// The knot span holding `t` (clamped to the domain): i with knots[i] ≤ t < knots[i + 1].
    fn span(&self, t: f64) -> usize {
        let n = self.knots.len() - self.degree - 1;
        let (lo, hi) = self.domain();
        if t >= hi {
            return n - 1;
        }
        if t <= lo {
            return self.degree;
        }
        // The last knot not past t, within [degree, n − 1].
        let i = self.knots.partition_point(|k| *k <= t).saturating_sub(1);
        i.clamp(self.degree, n - 1)
    }

    /// The degree + 1 basis functions not zero at `t` (from span − degree) and their first
    /// derivatives.
    fn basis(&self, span: usize, t: f64) -> (Vec<f64>, Vec<f64>) {
        let p = self.degree;
        let k = &self.knots;
        // ndu as in Piegl & Tiller A2.3, first derivatives only.
        let mut ndu = vec![vec![0.0; p + 1]; p + 1];
        let mut left = vec![0.0; p + 1];
        let mut right = vec![0.0; p + 1];
        ndu[0][0] = 1.0;
        for j in 1..=p {
            left[j] = t - k[span + 1 - j];
            right[j] = k[span + j] - t;
            let mut saved = 0.0;
            for r in 0..j {
                ndu[j][r] = right[r + 1] + left[j - r];
                let temp = if ndu[j][r] != 0.0 { ndu[r][j - 1] / ndu[j][r] } else { 0.0 };
                ndu[r][j] = saved + right[r + 1] * temp;
                saved = left[j - r] * temp;
            }
            ndu[j][j] = saved;
        }
        let n: Vec<f64> = (0..=p).map(|j| ndu[j][p]).collect();
        let mut d = vec![0.0; p + 1];
        if p > 0 {
            for (r, dr) in d.iter_mut().enumerate() {
                let a = if r >= 1 && ndu[p][r - 1] != 0.0 { ndu[r - 1][p - 1] / ndu[p][r - 1] } else { 0.0 };
                let b = if r < p && ndu[p][r] != 0.0 { ndu[r][p - 1] / ndu[p][r] } else { 0.0 };
                *dr = p as f64 * (a - b);
            }
        }
        (n, d)
    }

    fn check(&self, points: usize, what: &str) -> Result<()> {
        let ok = (1..=MAX_DEGREE).contains(&self.degree)
            && (self.degree + 1..=MAX_POINTS).contains(&points)
            && self.knots.len() == points + self.degree + 1
            && self.knots.iter().all(|k| k.is_finite())
            && self.knots.windows(2).all(|w| w[0] <= w[1])
            && {
                let (lo, hi) = self.domain();
                hi > lo
            };
        if ok { Ok(()) } else { Err(invalid("NURBS surface", format!("its {what} knots do not fit its degree and control points"))) }
    }
}

/// A NURBS patch: `net[i][j]` (control point in metres, weight) over the knots `u` (index i) and
/// `v` (index j).
///
/// ```
/// use emcraft_model::nurbs::{Knots, Nurbs};
/// use emcraft_model::Point3;
/// // A bilinear patch: the square [0, 1]² in the plane z = 0.5.
/// let k = Knots { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0] };
/// let net = vec![
///     vec![(Point3::new(0.0, 0.0, 0.5), 1.0), (Point3::new(0.0, 1.0, 0.5), 1.0)],
///     vec![(Point3::new(1.0, 0.0, 0.5), 1.0), (Point3::new(1.0, 1.0, 0.5), 1.0)],
/// ];
/// let s = Nurbs { u: k.clone(), v: k, net };
/// s.check()?;
/// assert!(s.point(0.25, 0.75).distance(Point3::new(0.25, 0.75, 0.5)) < 1e-15);
/// // The nearest point to one above it: straight down.
/// let q = s.projector().project(Point3::new(0.3, 0.6, 2.0)).unwrap_or(Point3::ORIGIN);
/// assert!(q.distance(Point3::new(0.3, 0.6, 0.5)) < 1e-12);
/// // A net that does not fit its knots is refused.
/// assert!(Nurbs { net: vec![], ..s }.check().is_err());
/// # Ok::<(), emcraft_model::ModelError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Nurbs {
    pub u: Knots,
    pub v: Knots,
    pub net: Vec<Vec<(Point3, f64)>>,
}

impl Nurbs {
    /// Knots that fit the net, finite points, positive weights.
    pub fn check(&self) -> Result<()> {
        let rows = self.net.len();
        let cols = self.net.first().map_or(0, Vec::len);
        self.u.check(rows, "u")?;
        self.v.check(cols, "v")?;
        if self.net.iter().any(|r| r.len() != cols) {
            return Err(invalid("NURBS surface", "its control net is not rectangular"));
        }
        if self.net.iter().flatten().any(|(p, w)| !p.is_finite() || !(w.is_finite() && *w > 0.0)) {
            return Err(invalid("NURBS surface", "a control point is not finite or its weight not positive"));
        }
        Ok(())
    }

    /// The patch's parameter domain: (u from, to), (v from, to).
    pub fn domain(&self) -> ((f64, f64), (f64, f64)) {
        (self.u.domain(), self.v.domain())
    }

    /// S(u, v) and its derivatives along u and v. The patch must have passed [`Nurbs::check`]
    /// (a malformed one gives NaN, never a panic).
    pub fn eval(&self, u: f64, v: f64) -> Evaluated {
        let nan = Evaluated { point: Point3::new(f64::NAN, f64::NAN, f64::NAN), along_u: Vec3::ZERO, along_v: Vec3::ZERO };
        let (rows, cols) = (self.net.len(), self.net.first().map_or(0, Vec::len));
        if rows <= self.u.degree
            || cols <= self.v.degree
            || self.u.knots.len() != rows + self.u.degree + 1
            || self.v.knots.len() != cols + self.v.degree + 1
        {
            return nan;
        }
        let (su, sv) = (self.u.span(u), self.v.span(v));
        let (nu, du) = self.u.basis(su, u.clamp(self.u.domain().0, self.u.domain().1));
        let (nv, dv) = self.v.basis(sv, v.clamp(self.v.domain().0, self.v.domain().1));
        // Homogeneous sums: A = Σ w P N, W = Σ w N, and their derivatives.
        let (mut a, mut au, mut av) = (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO);
        let (mut w, mut wu, mut wv) = (0.0, 0.0, 0.0);
        for (i, (ni, dni)) in nu.iter().zip(&du).enumerate() {
            let Some(row) = self.net.get(su - self.u.degree + i) else { return nan };
            for (j, (nj, dnj)) in nv.iter().zip(&dv).enumerate() {
                let Some(&(p, wt)) = row.get(sv - self.v.degree + j) else { return nan };
                let pw = p.to_vec() * wt;
                a += pw * (ni * nj);
                au += pw * (dni * nj);
                av += pw * (ni * dnj);
                w += wt * ni * nj;
                wu += wt * dni * nj;
                wv += wt * ni * dnj;
            }
        }
        let s = a * (1.0 / w);
        let su_ = (au - s * wu) * (1.0 / w);
        let sv_ = (av - s * wv) * (1.0 / w);
        Evaluated { point: Point3::ORIGIN + s, along_u: su_, along_v: sv_ }
    }

    /// S(u, v).
    pub fn point(&self, u: f64, v: f64) -> Point3 {
        self.eval(u, v).point
    }

    /// The patch with every control point mapped by `f` (an affine map: a move, a turn).
    pub fn mapped(&self, f: impl Fn(Point3) -> Point3) -> Nurbs {
        Nurbs { u: self.u.clone(), v: self.v.clone(), net: self.net.iter().map(|r| r.iter().map(|(p, w)| (f(*p), *w)).collect()).collect() }
    }

    /// The patch with its samples taken, for many projections.
    pub fn projector(&self) -> Projector {
        Projector::new(self.clone())
    }
}

/// A patch and a grid of its samples: the starting points of its projections.
#[derive(Clone, Debug, PartialEq)]
pub struct Projector {
    patch: Nurbs,
    /// (u, v, S(u, v)).
    samples: Vec<(f64, f64, Point3)>,
    /// The samples bucketed in a grid of cubes over their box: its corner, the cube's side and
    /// the cells along each axis, each cell's samples.
    grid: Grid,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Grid {
    lo: Point3,
    side: f64,
    n: [usize; 3],
    cells: Vec<Vec<u32>>,
}

impl Grid {
    /// About one sample a cell.
    fn new(points: &[Point3]) -> Grid {
        let finite: Vec<Point3> = points.iter().copied().filter(|p| p.is_finite()).collect();
        let Some(b) = crate::geom::Aabb::around(finite.iter().copied()) else { return Grid::default() };
        let size = b.max - b.min;
        let extent = size.x.max(size.y).max(size.z).max(f64::MIN_POSITIVE);
        // Cubes whose count is about the samples' (at most 64 a side per sample's share).
        let volume: f64 = [size.x, size.y, size.z].iter().map(|s| s.max(1e-6 * extent)).product();
        let side = (volume / finite.len().max(1) as f64).cbrt().max(1e-9 * extent);
        let n = [size.x, size.y, size.z].map(|s| ((s / side).floor() as usize + 1).min(256));
        let mut cells = vec![Vec::new(); n[0] * n[1] * n[2]];
        let g = Grid { lo: b.min, side, n, cells: Vec::new() };
        for (i, p) in points.iter().enumerate() {
            if let Some(c) = g.cell(*p).map(|c| g.index(c))
                && let Some(list) = cells.get_mut(c)
            {
                list.push(i as u32);
            }
        }
        Grid { cells, ..g }
    }

    fn cell(&self, p: Point3) -> Option<[usize; 3]> {
        if !p.is_finite() || self.cells.is_empty() && self.n == [0; 3] {
            return None;
        }
        let d = p - self.lo;
        let at = |x: f64, n: usize| ((x / self.side).floor().max(0.0) as usize).min(n.saturating_sub(1));
        Some([at(d.x, self.n[0]), at(d.y, self.n[1]), at(d.z, self.n[2])])
    }

    fn index(&self, c: [usize; 3]) -> usize {
        (c[0] * self.n[1] + c[1]) * self.n[2] + c[2]
    }
}

impl Projector {
    fn new(patch: Nurbs) -> Projector {
        // Four samples a knot span each way (distinct spans only), within the samples' cap.
        let along = |k: &Knots| {
            let (lo, hi) = k.domain();
            let mut ts: Vec<f64> = k.knots.iter().copied().filter(|t| *t >= lo && *t <= hi).collect();
            ts.dedup();
            let mut out = Vec::new();
            for w in ts.windows(2) {
                out.extend((0..4).map(|i| w[0] + (w[1] - w[0]) * i as f64 / 4.0));
            }
            out.push(hi);
            out
        };
        let (mut us, mut vs) = (along(&patch.u), along(&patch.v));
        while us.len() * vs.len() > MAX_SAMPLES {
            let thin = |x: &mut Vec<f64>| *x = x.iter().step_by(2).copied().collect();
            if us.len() >= vs.len() { thin(&mut us) } else { thin(&mut vs) }
        }
        let samples: Vec<(f64, f64, Point3)> =
            us.iter().flat_map(|&u| vs.iter().map(move |&v| (u, v))).map(|(u, v)| (u, v, patch.point(u, v))).collect();
        let grid = Grid::new(&samples.iter().map(|s| s.2).collect::<Vec<_>>());
        Projector { patch, samples, grid }
    }

    /// The patch.
    pub fn patch(&self) -> &Nurbs {
        &self.patch
    }

    /// The point of the patch nearest `p` (the parameters kept in its domain), `None` when
    /// nothing finite is found.
    pub fn project(&self, p: Point3) -> Option<Point3> {
        self.nearest(p).map(|n| n.point)
    }

    /// The parameters and point nearest `p`.
    pub fn nearest(&self, p: Point3) -> Option<Nearest> {
        let &(mut u, mut v, _) = self.nearest_sample(p)?;
        let ((u0, u1), (v0, v1)) = self.patch.domain();
        for _ in 0..MAX_STEPS {
            let Evaluated { point: s, along_u: su, along_v: sv } = self.patch.eval(u, v);
            let r = s - p;
            // Gauss–Newton: [su·su su·sv; sv·su sv·sv] δ = −[su·r; sv·r].
            let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
            let (f, g) = (-su.dot(r), -sv.dot(r));
            let det = a * c - b * b;
            if !(det.abs() > 1e-300) || !det.is_finite() {
                break;
            }
            let du = (f * c - b * g) / det;
            let dv = (a * g - b * f) / det;
            let (nu, nv) = ((u + du).clamp(u0, u1), (v + dv).clamp(v0, v1));
            let moved = (nu - u).abs() + (nv - v).abs();
            (u, v) = (nu, nv);
            if moved <= 1e-14 * ((u1 - u0) + (v1 - v0)) {
                break;
            }
        }
        let q = self.patch.point(u, v);
        q.is_finite().then_some(Nearest { u, v, point: q })
    }

    /// The sample nearest `p`: the grid's cells in growing shells about p's (clamped into the
    /// grid), until the nearest found is nearer than the next shell can hold.
    fn nearest_sample(&self, p: Point3) -> Option<&(f64, f64, Point3)> {
        let g = &self.grid;
        let Some(c) = g.cell(p) else {
            return self.samples.iter().filter(|s| s.2.is_finite()).min_by(|a, b| a.2.distance(p).total_cmp(&b.2.distance(p)));
        };
        // How far p lies outside the grid's box (its cell is clamped).
        let hi = g.lo + Vec3::new(g.n[0] as f64, g.n[1] as f64, g.n[2] as f64) * g.side;
        let outside =
            Vec3::new((g.lo.x - p.x).max(p.x - hi.x).max(0.0), (g.lo.y - p.y).max(p.y - hi.y).max(0.0), (g.lo.z - p.z).max(p.z - hi.z).max(0.0))
                .norm();
        let mut best: Option<(f64, usize)> = None;
        let reach = g.n.iter().copied().max().unwrap_or(0);
        for r in 0..=reach {
            if let Some((d, _)) = best
                && d <= outside + (r as f64 - 1.0).max(0.0) * g.side
            {
                break;
            }
            let lo = c.map(|x| x.saturating_sub(r));
            let hi = [0, 1, 2].map(|k| (c[k] + r).min(g.n[k].saturating_sub(1)));
            for i in lo[0]..=hi[0] {
                for j in lo[1]..=hi[1] {
                    for k in lo[2]..=hi[2] {
                        // The shell only: the inside was searched already.
                        let shell = [i, j, k].iter().zip(&c).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
                        if shell != r {
                            continue;
                        }
                        for &s in g.cells.get(g.index([i, j, k])).map_or(&[][..], Vec::as_slice) {
                            let Some(q) = self.samples.get(s as usize) else { continue };
                            let d = q.2.distance(p);
                            if d.is_finite() && best.is_none_or(|(b, _)| d < b) {
                                best = Some((d, s as usize));
                            }
                        }
                    }
                }
            }
        }
        best.and_then(|(_, i)| self.samples.get(i))
    }

    /// The unit normal ∂S/∂u × ∂S/∂v at the point nearest `p`.
    pub fn normal(&self, p: Point3) -> Option<Vec3> {
        let Nearest { u, v, .. } = self.nearest(p)?;
        let e = self.patch.eval(u, v);
        e.along_u.cross(e.along_v).normalized()
    }
}

/// A patch's point S(u, v) and its derivatives ∂S/∂u and ∂S/∂v there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Evaluated {
    pub point: Point3,
    pub along_u: Vec3,
    pub along_v: Vec3,
}

/// The parameters (u, v) of a patch's point nearest another, and that point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nearest {
    pub u: f64,
    pub v: f64,
    pub point: Point3,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quarter circle as a rational quadratic (weights 1, √½, 1), extruded 1 along z: every
    /// point on the unit cylinder, the derivative along the arc tangent to it, and a point
    /// off it projected radially.
    #[test]
    fn a_rational_arc_lies_on_its_circle() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let arc = [(Point3::new(1.0, 0.0, 0.0), 1.0), (Point3::new(1.0, 1.0, 0.0), w), (Point3::new(0.0, 1.0, 0.0), 1.0)];
        let net = arc.iter().map(|&(p, wt)| vec![(p, wt), (p + Vec3::new(0.0, 0.0, 1.0), wt)]).collect();
        let s =
            Nurbs { u: Knots { degree: 2, knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0] }, v: Knots { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0] }, net };
        s.check().unwrap_or_else(|e| panic!("{e}"));
        for k in 0..=10 {
            let Evaluated { point: p, along_u: du, along_v: dv } = s.eval(k as f64 / 10.0, 0.3);
            assert!((p.x.hypot(p.y) - 1.0).abs() < 1e-14 && (p.z - 0.3).abs() < 1e-14, "{p:?}");
            assert!((du.x * p.x + du.y * p.y).abs() < 1e-12 && (dv - Vec3::new(0.0, 0.0, 1.0)).norm() < 1e-12);
        }
        let pr = s.projector();
        let q = pr.project(Point3::new(1.5, 1.5, 0.7)).unwrap_or(Point3::ORIGIN);
        let c = std::f64::consts::FRAC_1_SQRT_2;
        assert!(q.distance(Point3::new(c, c, 0.7)) < 1e-10, "{q:?}");
        let n = pr.normal(Point3::new(1.5, 1.5, 0.7)).unwrap_or(Vec3::ZERO);
        assert!((n.dot(Vec3::new(c, c, 0.0)).abs() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn malformed_patches_are_refused_and_give_nan() {
        let k = Knots { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0] };
        let p = (Point3::ORIGIN, 1.0);
        let good = Nurbs { u: k.clone(), v: k.clone(), net: vec![vec![p, p], vec![p, p]] };
        assert!(good.check().is_ok());
        assert!(Nurbs { net: vec![vec![p, p], vec![p]], ..good.clone() }.check().is_err());
        assert!(Nurbs { net: vec![vec![p, (Point3::ORIGIN, 0.0)], vec![p, p]], ..good.clone() }.check().is_err());
        assert!(Nurbs { u: Knots { degree: 1, knots: vec![0.0, 1.0, 0.5, 1.0] }, ..good.clone() }.check().is_err());
        assert!(Nurbs { u: Knots { degree: 0, knots: vec![0.0, 1.0] }, ..good.clone() }.check().is_err());
        // Unchecked and broken: NaN, no panic.
        let broken = Nurbs { net: vec![vec![p]], ..good };
        assert!(!broken.point(0.5, 0.5).is_finite());
        assert!(broken.projector().project(Point3::ORIGIN).is_none());
    }
}

//! Section (cut-away) views: the scene cut by a plane, what lies on the far side removed and the
//! cut faces of closed bodies filled ("capped") in each body's own colour, so what is inside
//! shows — sections, dielectric bodies, conductors, ports — as in a CAD section view.
//!
//! An operation on a [`Scene`] that gives a [`Scene`]: the CPU renderer ([`super::render`],
//! depth-buffered) and the GPU path ([`super::flatten`], painter's order) draw the same plain
//! triangles and lines, and [`super::pick`] finds the caps as their bodies.
//!
//! 1. **Clipping.** Every triangle and line is clipped against the kept half space (Sutherland
//!    and Hodgman, "Reentrant polygon clipping", Comm. ACM 17(1), 1974): a triangle the plane
//!    crosses keeps a triangle or a quad (two triangles, in the triangle's winding). A point's
//!    side follows from its position alone, points within a tolerance (10⁻⁹ of the scene's size)
//!    of the plane counting as removed — the plane as if moved a hair towards the kept side, so a
//!    face lying on it goes and the cap takes its place. Where an edge crosses is computed from
//!    its kept end towards its removed end, so the two triangles sharing an edge cut it at the
//!    same point, bit for bit.
//! 2. **Bodies.** The triangles at least [`Section::cap_alpha`] opaque are grouped by (tag,
//!    colour, opacity): one body each, as the scene builders draw a section, a layer, a post or
//!    a material's interfaces. The pieces of the plane the clipping leaves on a body's triangles
//!    are the outline of its cut, on the plane's own coordinates. Where every end point of the
//!    outline (snapped to a grid of the tolerance) is shared by an even number of pieces the
//!    outline is closed and the body is capped; an open one (a strip, a lone face, a surface
//!    whose hole the plane crosses) is only clipped.
//! 3. **Caps.** The capped bodies' outlines are swept across the plane in slabs between
//!    successive end points and crossings — the slab method (Dobkin and Lipton,
//!    "Multidimensional searching problems", SIAM J. Comput. 5(2), 1976; Preparata and Shamos,
//!    *Computational Geometry: An Introduction*, Springer 1985, §2.2.2.1). In a slab the pieces
//!    are sorted across it, and the trapezoid between two successive ones lies inside a body
//!    when an odd number of that body's pieces lies below it (the even–odd rule; Haines, "Point
//!    in polygon strategies", *Graphics Gems IV*, 1994), so holes (a tube's ring) and nested
//!    bodies come out without building loops, whatever the pieces' order or direction. Where
//!    bodies overlap (a post inside a section, a layer across sections) the one with the
//!    smallest cut wins, ties to the later body in the scene: caps never overlap, so coplanar
//!    caps cannot fight over depth in either renderer. Trapezoids of one slab with the same
//!    winner are joined; each is drawn as one or two triangles in the winner's colour, opacity
//!    and tag, and [`Section::outline`] draws the bodies' outlines as lines.
//!
//! Work is bounded ([`MAX_CAP_BODIES`], [`MAX_CAP_SEGMENTS`], [`MAX_CAP_WORK`]): past a cap the
//! scene is still clipped, the bodies beyond it (or, past the sweep's budget, all of them) left
//! without caps. Deterministic: a fixed order throughout, ties broken by the scene's order.

use std::collections::{BTreeMap, BTreeSet};

use super::{Line, MAX_PRIMITIVES, Scene, Tri, cross, dot, normalized, sub};
use crate::Rgb;

/// Most bodies capped in one section.
pub const MAX_CAP_BODIES: usize = 4096;
/// Most pieces of outline swept into caps in one section (the bodies past it are not capped).
pub const MAX_CAP_SEGMENTS: usize = 400_000;
/// The sweep's budget, in pieces visited over all slabs (past it no body is capped).
pub const MAX_CAP_WORK: usize = 50_000_000;

/// The largest plane offset accepted (m).
const MAX_OFFSET: f64 = 1e12;

/// Which side of an axis-aligned cut stays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    /// The side of the smaller coordinates.
    Low,
    /// The side of the larger coordinates.
    High,
}

/// A cut: the plane n̂ · p = `offset` (n̂ the unit `normal`); the side the normal points to is
/// removed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    /// The plane's normal (any length but zero, finite), pointing to the side removed.
    pub normal: [f64; 3],
    /// Where the plane lies along the unit normal (world units, m).
    pub offset: f64,
    /// Bodies at least this opaque are capped; more see-through ones are only clipped.
    pub cap_alpha: u8,
    /// The colour of the caps' outlines; none draws no outlines.
    pub outline: Option<Rgb>,
}

impl Section {
    /// The opacity from which bodies are capped by default: the solid ones, as [`super::pick`]
    /// counts them.
    pub const CAP_ALPHA: u8 = 200;

    /// The plane n̂ · p = `offset`, the side `normal` points to removed; solid bodies capped, no
    /// outlines.
    pub fn new(normal: [f64; 3], offset: f64) -> Section {
        Section { normal, offset, cap_alpha: Section::CAP_ALPHA, outline: None }
    }

    /// A cut across axis `axis` (0 x, 1 y, 2 z) at coordinate `at` (m), keeping the `keep` side.
    /// An axis past 2 gives a plane [`section`] refuses.
    pub fn across(axis: usize, at: f64, keep: Keep) -> Section {
        let mut normal = [0.0; 3];
        if let Some(n) = normal.get_mut(axis) {
            *n = 1.0;
        }
        let s = Section::new(normal, at);
        if keep == Keep::High { s.flipped() } else { s }
    }

    /// The same plane, the other side kept.
    pub fn flipped(self) -> Section {
        Section { normal: self.normal.map(|v| -v), offset: -self.offset, ..self }
    }

    /// Whether `p` lies on the kept side (strictly): for what the scene does not hold, such as
    /// labels. `false` for a plane [`section`] refuses.
    pub fn keeps(&self, p: [f64; 3]) -> bool {
        Plane::new(self, 0.0).is_ok_and(|pl| pl.dist(p) < 0.0)
    }
}

/// Why a section was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionError {
    /// The normal is zero or not finite.
    Normal,
    /// The offset is not finite or beyond ±10¹² m.
    Offset,
}

impl std::fmt::Display for SectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SectionError::Normal => write!(f, "the section plane's normal must be finite and not zero"),
            SectionError::Offset => write!(f, "the section plane's offset must be finite and within ±1e12 m"),
        }
    }
}

impl std::error::Error for SectionError {}

/// The cut plane, checked, with its own coordinates.
struct Plane {
    /// The unit normal.
    n: [f64; 3],
    d: f64,
    /// For a plane across an axis: the axis and the normal's sign along it, so distances and
    /// the plane's coordinates are the points' own, exactly.
    axis: Option<(usize, f64)>,
    /// Unit vectors along the plane (n, e1, e2 right-handed).
    e1: [f64; 3],
    e2: [f64; 3],
    tol: f64,
}

impl Plane {
    fn new(s: &Section, tol: f64) -> Result<Plane, SectionError> {
        if !s.normal.iter().all(|v| v.is_finite()) {
            return Err(SectionError::Normal);
        }
        // Scaled first, so a tiny normal does not underflow.
        let big = s.normal.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        let n = normalized(s.normal.map(|v| v / big)).ok_or(SectionError::Normal)?;
        if !(s.offset.abs() <= MAX_OFFSET) {
            return Err(SectionError::Offset);
        }
        let along: Vec<usize> = (0..3).filter(|&k| s.normal[k] != 0.0).collect();
        let axis = match along[..] {
            [k] => Some((k, s.normal[k].signum())),
            _ => None,
        };
        let (n, e1, e2) = match axis {
            Some((k, sign)) => {
                let mut n = [0.0; 3];
                n[k] = sign;
                let (mut e1, mut e2) = ([0.0; 3], [0.0; 3]);
                e1[(k + 1) % 3] = 1.0;
                e2[(k + 2) % 3] = 1.0;
                (n, e1, e2)
            }
            None => {
                // Across the axis the normal leans on least (Hughes and Möller, "Building an
                // orthonormal basis from a unit vector", J. Graphics Tools 4(4), 1999).
                let k = (0..3).fold(0, |m, k| if n[k].abs() < n[m].abs() { k } else { m });
                let mut a = [0.0; 3];
                a[k] = 1.0;
                let e1 = normalized(cross(n, a)).ok_or(SectionError::Normal)?;
                (n, e1, cross(n, e1))
            }
        };
        Ok(Plane { n, d: s.offset, axis, e1, e2, tol })
    }

    /// The signed distance from the plane, positive on the removed side.
    fn dist(&self, p: [f64; 3]) -> f64 {
        match self.axis {
            Some((k, sign)) => sign * p[k] - self.d,
            None => dot(self.n, p) - self.d,
        }
    }

    /// Whether `p` stays (`false` for NaN).
    fn kept(&self, p: [f64; 3]) -> bool {
        self.dist(p) < -self.tol
    }

    /// Where the edge from the kept point `k` to the removed point `r` meets the plane.
    fn cut(&self, k: [f64; 3], r: [f64; 3]) -> [f64; 3] {
        let (sk, sr) = (self.dist(k), self.dist(r).max(0.0));
        let mut p = if sr == 0.0 {
            // On the plane (within the tolerance): the point itself, onto the plane.
            let s = self.dist(r);
            if self.axis.is_some() { r } else { [r[0] - s * self.n[0], r[1] - s * self.n[1], r[2] - s * self.n[2]] }
        } else {
            let t = sk / (sk - sr);
            [k[0] + t * (r[0] - k[0]), k[1] + t * (r[1] - k[1]), k[2] + t * (r[2] - k[2])]
        };
        if let Some((a, sign)) = self.axis {
            p[a] = sign * self.d;
        }
        p
    }

    /// Sutherland–Hodgman against the plane: the kept part of the triangle `p` whose corners
    /// are `kept` or not (a triangle or a quad: its corners and their count) and the points the
    /// plane cuts its edges at (and their count).
    fn clip(&self, p: [[f64; 3]; 3], kept: [bool; 3]) -> ([[f64; 3]; 4], usize, [[f64; 3]; 2], usize) {
        let (mut poly, mut n) = ([[0.0; 3]; 4], 0);
        let (mut cuts, mut m) = ([[0.0; 3]; 2], 0);
        for i in 0..3 {
            let j = (i + 1) % 3;
            let mut put = |q: [f64; 3]| {
                if let Some(slot) = poly.get_mut(n) {
                    *slot = q;
                    n += 1;
                }
            };
            if kept[i] {
                put(p[i]);
            }
            if kept[i] != kept[j] {
                let c = if kept[i] { self.cut(p[i], p[j]) } else { self.cut(p[j], p[i]) };
                put(c);
                if let Some(slot) = cuts.get_mut(m) {
                    *slot = c;
                    m += 1;
                }
            }
        }
        (poly, n, cuts, m)
    }

    /// `p`'s coordinates on the plane (`+ 0.0` makes -0 into 0, so equal points have equal
    /// bits).
    fn uv(&self, p: [f64; 3]) -> [f64; 2] {
        let q = match self.axis {
            Some((k, _)) => [p[(k + 1) % 3], p[(k + 2) % 3]],
            None => [dot(self.e1, p), dot(self.e2, p)],
        };
        q.map(|v| v + 0.0)
    }

    /// A point of the plane snapped to the tolerance's grid, as bits: points that differ by
    /// rounding alone match.
    fn key(&self, q: [f64; 2]) -> [u64; 2] {
        q.map(|v| {
            let s = (v / self.tol).round() * self.tol;
            (if self.tol > 0.0 && s.is_finite() { s } else { v } + 0.0).to_bits()
        })
    }

    /// The point of the plane at coordinates `q`.
    fn lift(&self, q: [f64; 2]) -> [f64; 3] {
        match self.axis {
            Some((k, sign)) => {
                let mut p = [0.0; 3];
                p[k] = sign * self.d;
                p[(k + 1) % 3] = q[0];
                p[(k + 2) % 3] = q[1];
                p
            }
            None => [0, 1, 2].map(|i| self.d * self.n[i] + q[0] * self.e1[i] + q[1] * self.e2[i]),
        }
    }
}

/// A piece of a cut's outline on the plane's coordinates.
type Piece = [[f64; 2]; 2];

/// A body: what its triangles share and its cut's outline.
struct Body {
    tag: u32,
    color: Rgb,
    alpha: u8,
    pieces: Vec<Piece>,
}

/// A point's bits, for matching points that are equal.
fn bits(p: [f64; 2]) -> [u64; 2] {
    p.map(f64::to_bits)
}

/// Whether an outline is closed: every end point shared by an even number of pieces.
fn closed(plane: &Plane, pieces: &[Piece]) -> bool {
    let mut ends: Vec<[u64; 2]> = pieces.iter().flat_map(|s| [plane.key(s[0]), plane.key(s[1])]).collect();
    ends.sort_unstable();
    !ends.is_empty() && ends.chunk_by(|a, b| a == b).all(|g| g.len() % 2 == 0)
}

/// The bounds of the work, [`MAX_CAP_BODIES`] and the others (smaller in tests).
#[derive(Clone, Copy)]
struct Limits {
    bodies: usize,
    segments: usize,
    work: usize,
}

const LIMITS: Limits = Limits { bodies: MAX_CAP_BODIES, segments: MAX_CAP_SEGMENTS, work: MAX_CAP_WORK };

/// `scene` cut by `s`: every triangle and line on the removed side gone, those across the plane
/// clipped to it, and the cut faces of the closed bodies at least [`Section::cap_alpha`] opaque
/// filled in their colour, opacity and tag (the module docs tell how). Refuses a plane with a zero
/// or non-finite normal or a non-finite offset; a plane beyond the scene on the removed side
/// leaves it as it was, one beyond it on the kept side leaves nothing.
///
/// ```
/// use emcraft_render::Rgb;
/// use emcraft_render::view3d::{Keep, Scene, Section, SectionError, section::section};
///
/// // A 20 × 10 × 30 mm box (world units are metres).
/// let mut scene = Scene::default();
/// scene.cuboid([0.0; 3], [0.02, 0.01, 0.03], Rgb::WHITE, 255);
/// // Cut across x at 5 mm, the low side kept: the box's quarter and its 10 × 30 mm cap.
/// let cut = section(&scene, &Section::across(0, 0.005, Keep::Low))?;
/// let area = |t: &emcraft_render::view3d::Tri| {
///     let (u, v) = ([0, 1, 2].map(|k| t.p[1][k] - t.p[0][k]), [0, 1, 2].map(|k| t.p[2][k] - t.p[0][k]));
///     0.5 * ((u[1] * v[2] - u[2] * v[1]).powi(2) + (u[2] * v[0] - u[0] * v[2]).powi(2) + (u[0] * v[1] - u[1] * v[0]).powi(2)).sqrt()
/// };
/// let cap: f64 = cut.tris.iter().filter(|t| t.p.iter().all(|p| p[0] == 0.005)).map(area).sum();
/// assert!((cap - 0.01 * 0.03).abs() < 1e-12);
/// assert!(cut.tris.iter().flat_map(|t| t.p).all(|p| p[0] <= 0.005));
/// // A plane that is no plane is refused.
/// assert_eq!(section(&scene, &Section::across(0, f64::NAN, Keep::Low)), Err(SectionError::Offset));
/// assert_eq!(section(&scene, &Section::new([0.0; 3], 0.0)), Err(SectionError::Normal));
/// # Ok::<(), SectionError>(())
/// ```
pub fn section(scene: &Scene, s: &Section) -> Result<Scene, SectionError> {
    section_with(scene, s, LIMITS)
}

fn section_with(scene: &Scene, s: &Section, lim: Limits) -> Result<Scene, SectionError> {
    let size = scene.bounds().map_or(0.0, |(lo, hi)| dot(sub(hi, lo), sub(hi, lo)).sqrt());
    let tol = if size.is_finite() { 1e-9 * size } else { 0.0 };
    let plane = Plane::new(s, tol)?;
    let mut out = Scene { tris: Vec::new(), lines: Vec::new(), tag: scene.tag };
    let mut index: BTreeMap<(u32, [u8; 4]), usize> = BTreeMap::new();
    let mut bodies: Vec<Body> = Vec::new();
    let finite = |p: &[f64; 3]| p.iter().all(|v| v.is_finite());
    for t in scene.tris.iter().take(MAX_PRIMITIVES).filter(|t| t.p.iter().all(finite)) {
        let kept = t.p.map(|p| plane.kept(p));
        match kept.iter().filter(|k| **k).count() {
            3 => {
                out.tris.push(*t);
                continue;
            }
            0 => continue,
            _ => {}
        }
        let (poly, n, cuts, m) = plane.clip(t.p, kept);
        let poly = &poly[..n];
        if !poly.iter().all(finite) {
            continue;
        }
        for k in 1..n.saturating_sub(1) {
            let p = [poly[0], poly[k], poly[k + 1]];
            if p[0] != p[1] && p[1] != p[2] && p[2] != p[0] {
                out.tris.push(Tri { p, ..*t });
            }
        }
        let piece = [plane.uv(cuts[0]), plane.uv(cuts[1])];
        if m == 2 && t.alpha > 0 && t.alpha >= s.cap_alpha && piece[0] != piece[1] && piece.iter().flatten().all(|v| v.is_finite()) {
            let key = (t.tag, [t.color.0, t.color.1, t.color.2, t.alpha]);
            let b = *index.entry(key).or_insert_with(|| {
                bodies.push(Body { tag: t.tag, color: t.color, alpha: t.alpha, pieces: Vec::new() });
                bodies.len() - 1
            });
            if let Some(body) = bodies.get_mut(b) {
                body.pieces.push(piece);
            }
        }
    }
    for l in scene.lines.iter().take(MAX_PRIMITIVES).filter(|l| finite(&l.a) && finite(&l.b)) {
        let line = match (plane.kept(l.a), plane.kept(l.b)) {
            (true, true) => *l,
            (false, false) => continue,
            (true, false) => Line { b: plane.cut(l.a, l.b), ..*l },
            (false, true) => Line { a: plane.cut(l.b, l.a), ..*l },
        };
        if finite(&line.a) && finite(&line.b) {
            out.lines.push(line);
        }
    }
    // The closed outlines, within the caps.
    let mut capped: Vec<&Body> = Vec::new();
    let mut total = 0;
    for b in &bodies {
        if capped.len() >= lim.bodies {
            break;
        }
        if total + b.pieces.len() <= lim.segments && closed(&plane, &b.pieces) {
            total += b.pieces.len();
            capped.push(b);
        }
    }
    let capped = if caps(&plane, &capped, lim.work, &mut out) { capped } else { Vec::new() };
    if let Some(color) = s.outline {
        outline(&plane, &capped, color, &mut out);
    }
    Ok(out)
}

/// The capped bodies' outlines as lines in `color`, each piece once.
fn outline(plane: &Plane, capped: &[&Body], color: Rgb, out: &mut Scene) {
    let mut seen: BTreeSet<[[u64; 2]; 2]> = BTreeSet::new();
    for b in capped {
        for p in &b.pieces {
            let k = if bits(p[0]) <= bits(p[1]) { [bits(p[0]), bits(p[1])] } else { [bits(p[1]), bits(p[0])] };
            if seen.insert(k) {
                out.lines.push(Line { a: plane.lift(p[0]), b: plane.lift(p[1]), color, alpha: 230 });
            }
        }
    }
}

/// A piece of outline that is not along the sweep's direction: x0 < x1.
#[derive(Clone, Copy)]
struct Seg {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    body: u32,
}

impl Seg {
    /// Its height at `x`, the end points' own at the ends.
    fn y(&self, x: f64) -> f64 {
        if x <= self.x0 {
            self.y0
        } else if x >= self.x1 {
            self.y1
        } else {
            self.y0 + (self.y1 - self.y0) * ((x - self.x0) / (self.x1 - self.x0))
        }
    }
}

/// The region between two pieces over xa–xb: their heights `lo` and `hi` at xa and at xb.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Trap {
    xa: f64,
    xb: f64,
    lo: [f64; 2],
    hi: [f64; 2],
}

impl Trap {
    fn area(&self) -> f64 {
        0.5 * (self.xb - self.xa) * ((self.hi[0] - self.lo[0]) + (self.hi[1] - self.lo[1]))
    }
}

/// The sweep went past its budget.
struct OverBudget;

/// Sweeps `segs` in slabs (the module docs), calling `visit` with every trapezoid inside at least
/// one body, bottom to top in each slab, the slabs left to right, and the bodies it lies in.
fn sweep(segs: &[Seg], budget: usize, mut visit: impl FnMut(Trap, &[u32])) -> Result<(), OverBudget> {
    let seg = |i: usize| segs.get(i).copied();
    let mut order: Vec<usize> = (0..segs.len()).collect();
    order.sort_by(|&a, &b| segs[a].x0.total_cmp(&segs[b].x0).then(a.cmp(&b)));
    let mut xs: Vec<f64> = segs.iter().flat_map(|s| [s.x0, s.x1]).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    let (mut active, mut next, mut work) = (Vec::<usize>::new(), 0, 0usize);
    let mut row: Vec<(f64, f64, f64, usize)> = Vec::new();
    let mut cuts: Vec<f64> = Vec::new();
    let mut inside: Vec<u32> = Vec::new();
    let mut spend = |w: usize| {
        work = work.saturating_add(w);
        if work > budget { Err(OverBudget) } else { Ok(()) }
    };
    for w in xs.windows(2) {
        let (xa, xb) = (w[0], w[1]);
        while let Some(s) = order.get(next).and_then(|&i| seg(i).filter(|s| s.x0 <= xa).map(|_| i)) {
            active.push(s);
            next += 1;
        }
        active.retain(|&i| seg(i).is_some_and(|s| s.x1 > xa));
        spend(active.len())?;
        // Pieces that cross inside the slab split it where they cross.
        row.clear();
        row.extend(active.iter().filter_map(|&i| seg(i).map(|s| (s.y(xa), s.y(xb), 0.0, i))));
        row.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.3.cmp(&b.3)));
        cuts.clear();
        cuts.push(xa);
        if row.windows(2).any(|p| p[0].1 > p[1].1) {
            spend(row.len().saturating_mul(row.len()))?;
            for (i, a) in row.iter().enumerate() {
                for b in row.iter().skip(i + 1).filter(|b| a.1 > b.1) {
                    let (fa, fb) = (a.0 - b.0, a.1 - b.1);
                    let x = xa + (xb - xa) * (-fa / (fb - fa));
                    if x > xa && x < xb {
                        cuts.push(x);
                    }
                }
            }
            cuts.sort_by(f64::total_cmp);
            cuts.dedup();
        }
        cuts.push(xb);
        for c in cuts.windows(2) {
            let (ca, cb) = (c[0], c[1]);
            if !(cb > ca) {
                continue;
            }
            let mid = 0.5 * (ca + cb);
            row.clear();
            row.extend(active.iter().filter_map(|&i| seg(i).map(|s| (s.y(ca), s.y(cb), s.y(mid), i))));
            row.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.total_cmp(&b.0)).then(a.1.total_cmp(&b.1)).then(a.3.cmp(&b.3)));
            inside.clear();
            for p in row.windows(2) {
                let Some(s) = seg(p[0].3) else { continue };
                match inside.iter().position(|&b| b == s.body) {
                    Some(k) => {
                        inside.remove(k);
                    }
                    None => inside.push(s.body),
                }
                let trap = Trap { xa: ca, xb: cb, lo: [p[0].0, p[0].1], hi: [p[1].0.max(p[0].0), p[1].1.max(p[0].1)] };
                if !inside.is_empty() && trap.area() > 0.0 {
                    visit(trap, &inside);
                }
            }
        }
    }
    Ok(())
}

/// The caps of the `bodies` (closed outlines) added to `out`; `false` (and nothing added) past
/// the sweep's budget.
fn caps(plane: &Plane, bodies: &[&Body], budget: usize, out: &mut Scene) -> bool {
    let mut segs: Vec<Seg> = Vec::new();
    for (i, b) in bodies.iter().enumerate() {
        for p in &b.pieces {
            let (a, z) = if p[0][0] <= p[1][0] { (p[0], p[1]) } else { (p[1], p[0]) };
            // Pieces along the sweep's direction bound no slab.
            if a[0] < z[0] {
                segs.push(Seg { x0: a[0], y0: a[1], x1: z[0], y1: z[1], body: i as u32 });
            }
        }
    }
    // The area of each body's cut, then the trapezoids in their winners' colours.
    let mut area = vec![0.0f64; bodies.len()];
    let first = sweep(&segs, budget, |t, inside| {
        for &b in inside {
            if let Some(a) = area.get_mut(b as usize) {
                *a += t.area();
            }
        }
    });
    if first.is_err() {
        return false;
    }
    let size = |b: u32| area.get(b as usize).copied().unwrap_or(f64::INFINITY);
    let mut tris: Vec<Tri> = Vec::new();
    let mut emit = |t: Trap, b: u32| {
        let Some(body) = bodies.get(b as usize) else { return };
        let [a, bb, c, d] = [[t.xa, t.lo[0]], [t.xb, t.lo[1]], [t.xb, t.hi[1]], [t.xa, t.hi[0]]].map(|q| plane.lift(q));
        let mut put = |p: [[f64; 3]; 3]| {
            if p.iter().flatten().all(|v| v.is_finite()) {
                tris.push(Tri { p, color: body.color, alpha: body.alpha, tag: body.tag });
            }
        };
        if t.hi[1] > t.lo[1] {
            put([a, bb, c]);
        }
        if t.hi[0] > t.lo[0] {
            put([a, c, d]);
        }
    };
    let mut pending: Option<(Trap, u32)> = None;
    let second = sweep(&segs, budget, |t, inside| {
        let Some(&win) = inside.iter().min_by(|&&a, &&b| size(a).total_cmp(&size(b)).then(b.cmp(&a))) else { return };
        if let Some((p, w)) = &mut pending
            && *w == win
            && p.xa == t.xa
            && p.xb == t.xb
            && p.hi == t.lo
        {
            p.hi = t.hi;
        } else if let Some((p, w)) = pending.replace((t, win)) {
            emit(p, w);
        }
    });
    if let Some((p, w)) = pending {
        emit(p, w);
    }
    if second.is_ok() {
        out.tris.extend(tris);
    }
    second.is_ok()
}

#[cfg(test)]
mod tests {
    use super::super::{Camera, Options, Projection, StandardView, flatten, pick, render};
    use super::*;

    fn area(t: &Tri) -> f64 {
        let c = cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0]));
        0.5 * dot(c, c).sqrt()
    }

    /// The triangles lying in the plane x_k = at (the caps, where no face of the scene lies there).
    fn on(scene: &Scene, k: usize, at: f64) -> Vec<Tri> {
        scene.tris.iter().filter(|t| t.p.iter().all(|p| p[k] == at)).copied().collect()
    }

    fn cap_area(scene: &Scene, k: usize, at: f64) -> f64 {
        on(scene, k, at).iter().map(area).sum()
    }

    /// The colours of the caps covering a point (on the plane x_k = at), moved a little off the
    /// round numbers so that it lies on no triangle's edge.
    fn cap_at(scene: &Scene, k: usize, at: f64, q: [f64; 3]) -> Vec<Rgb> {
        let (i, j) = ((k + 1) % 3, (k + 2) % 3);
        let mut q = q;
        (q[i], q[j]) = (q[i] + 0.0123, q[j] + 0.0371);
        on(scene, k, at)
            .iter()
            .filter(|t| {
                let e = |a: [f64; 3], b: [f64; 3]| (b[i] - a[i]) * (q[j] - a[j]) - (b[j] - a[j]) * (q[i] - a[i]);
                let w = [e(t.p[0], t.p[1]), e(t.p[1], t.p[2]), e(t.p[2], t.p[0])];
                w.iter().all(|v| *v >= 0.0) || w.iter().all(|v| *v <= 0.0)
            })
            .map(|t| t.color)
            .collect()
    }

    const A: Rgb = Rgb(200, 60, 40);
    const B: Rgb = Rgb(40, 90, 210);

    #[test]
    fn a_box_cut_in_half_is_capped_with_the_cut_s_area() {
        let mut s = Scene::default();
        s.cuboid([0.0; 3], [2.0, 1.0, 3.0], A, 255);
        s.line([0.0, 0.0, 0.0], [2.0, 0.0, 0.0], Rgb::BLACK, 255);
        s.line([1.5, 1.0, 0.0], [2.0, 1.0, 0.0], Rgb::BLACK, 255);
        let cut = section(&s, &Section::across(0, 1.0, Keep::Low)).unwrap();
        assert!((cap_area(&cut, 0, 1.0) - 3.0).abs() < 1e-12);
        assert!(cut.tris.iter().flat_map(|t| t.p).all(|p| p[0] <= 1.0));
        // The kept faces: the x = 0 face and four half faces, area 3 + 2·(1 + 3).
        let total: f64 = cut.tris.iter().map(area).sum();
        assert!((total - (3.0 + 3.0 + 8.0)).abs() < 1e-12, "{total}");
        // Lines: one shortened, one gone.
        assert_eq!(cut.lines.len(), 1);
        assert_eq!(cut.lines[0].b, [1.0, 0.0, 0.0]);
        // The other side, and an arbitrary normal along the same axis' diagonal.
        let high = section(&s, &Section::across(0, 1.0, Keep::High)).unwrap();
        assert!((cap_area(&high, 0, 1.0) - 3.0).abs() < 1e-12);
        assert!(high.tris.iter().flat_map(|t| t.p).all(|p| p[0] >= 1.0));
        let r = 0.5f64.sqrt();
        let diag = section(&s, &Section::new([1.0, 1.0, 0.0], r)).unwrap();
        let tilted: f64 = diag.tris.iter().filter(|t| t.p.iter().all(|p| (p[0] + p[1] - 1.0).abs() < 1e-12)).map(area).sum();
        assert!((tilted - 3.0 * 2f64.sqrt()).abs() < 1e-9, "{tilted}");
    }

    #[test]
    fn a_hollow_box_gives_a_ring_and_nested_boxes_do_not_overlap() {
        // One body of two surfaces: a cube with a cubic hole.
        let mut ring = Scene::default();
        ring.cuboid([0.0; 3], [3.0; 3], A, 255);
        ring.cuboid([1.0; 3], [2.0; 3], A, 255);
        let cut = section(&ring, &Section::across(2, 1.5, Keep::Low)).unwrap();
        assert!((cap_area(&cut, 2, 1.5) - 8.0).abs() < 1e-12);
        assert!(cap_at(&cut, 2, 1.5, [1.5, 1.5, 1.5]).is_empty());
        assert_eq!(cap_at(&cut, 2, 1.5, [0.5, 1.5, 1.5]), vec![A]);
        // Two bodies, one inside the other: the inner one's cap fills the outer one's hole.
        let mut nest = Scene::default();
        nest.tagged(1, |s| s.cuboid([0.0; 3], [3.0; 3], A, 255));
        nest.tagged(2, |s| s.cuboid([1.0; 3], [2.0; 3], B, 255));
        let cut = section(&nest, &Section::across(2, 1.5, Keep::Low)).unwrap();
        let caps = on(&cut, 2, 1.5);
        let of = |c: Rgb| caps.iter().filter(|t| t.color == c).map(area).sum::<f64>();
        assert!((of(A) - 8.0).abs() < 1e-12 && (of(B) - 1.0).abs() < 1e-12, "{} {}", of(A), of(B));
        assert!(caps.iter().all(|t| (t.color == A) == (t.tag == 1)));
        // No point of the plane is covered twice.
        for q in [[1.5, 1.5, 1.5], [0.5, 0.5, 1.5], [1.2, 2.7, 1.5], [2.5, 1.1, 1.5]] {
            assert_eq!(cap_at(&cut, 2, 1.5, q).len(), 1, "{q:?}");
        }
        // Order does not matter: the inner body wins drawn first too.
        let mut rev = Scene::default();
        rev.tagged(2, |s| s.cuboid([1.0; 3], [2.0; 3], B, 255));
        rev.tagged(1, |s| s.cuboid([0.0; 3], [3.0; 3], A, 255));
        let cut = section(&rev, &Section::across(2, 1.5, Keep::Low)).unwrap();
        assert_eq!(cap_at(&cut, 2, 1.5, [1.5, 1.5, 1.5]), vec![B]);
        // Overlapping, not nested: the smaller cut wins where they cross, nothing covered twice.
        let mut cross = Scene::default();
        cross.tagged(1, |s| s.cuboid([0.0, 1.0, 0.0], [4.0, 2.0, 1.0], A, 255));
        cross.tagged(2, |s| s.cuboid([1.0, 0.0, 0.0], [2.0, 3.0, 1.0], B, 255));
        let cut = section(&cross, &Section::across(2, 0.5, Keep::Low)).unwrap();
        assert!((cap_area(&cut, 2, 0.5) - 6.0).abs() < 1e-12, "{}", cap_area(&cut, 2, 0.5));
        assert_eq!(cap_at(&cut, 2, 0.5, [1.5, 1.5, 0.5]), vec![B]);
        assert_eq!(cap_at(&cut, 2, 0.5, [3.0, 1.5, 0.5]), vec![A]);
    }

    #[test]
    fn crossing_outlines_split_the_slabs() {
        // Two triangular prisms whose cuts cross inside one slab.
        let mut s = Scene::default();
        let prism = |s: &mut Scene, t: [[f64; 2]; 3], c: Rgb| {
            let p = |q: [f64; 2], z: f64| [q[0], q[1], z];
            s.tri(p(t[0], 0.0), p(t[1], 0.0), p(t[2], 0.0), c, 255);
            s.tri(p(t[0], 1.0), p(t[2], 1.0), p(t[1], 1.0), c, 255);
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                s.quad([p(a, 0.0), p(b, 0.0), p(b, 1.0), p(a, 1.0)], c, 255);
            }
        };
        s.tagged(1, |s| prism(s, [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0]], A));
        s.tagged(2, |s| prism(s, [[0.0, 3.0], [4.0, 1.0], [0.0, 1.0]], B));
        let cut = section(&s, &Section::across(2, 0.5, Keep::Low)).unwrap();
        // Each point once, where one covers it and where both do.
        for q in [[3.5, 1.0 + 0.01, 0.5], [1.0, 0.5, 0.5], [1.0, 2.0, 0.5], [3.9, 3.5, 0.5], [2.5, 1.5, 0.5]] {
            assert_eq!(cap_at(&cut, 2, 0.5, q).len(), 1, "{q:?}");
        }
        let covered = cap_area(&cut, 2, 0.5);
        // The union's area, sampled independently: below y = x, or between y = 1 and y = 3 − x/2.
        let n = 400;
        let mut inside = 0;
        for i in 0..n {
            for j in 0..n {
                let (x, y) = ((i as f64 + 0.5) * 4.0 / n as f64, (j as f64 + 0.5) * 4.0 / n as f64);
                let a = y < x;
                let b = y > 1.0 && y < 3.0 - 0.5 * x;
                inside += usize::from(a || b);
            }
        }
        let sampled = inside as f64 * 16.0 / (n * n) as f64;
        assert!((covered - sampled).abs() < 0.03, "{covered} {sampled}");
    }

    #[test]
    fn open_and_see_through_surfaces_are_clipped_without_caps() {
        let mut s = Scene::default();
        // A strip, a box open on top and a see-through box.
        s.tagged(1, |s| s.quad([[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 0.0, 1.0], [0.0, 0.0, 1.0]], A, 255));
        s.tagged(2, |s| {
            let mut b = Scene::default();
            b.cuboid([0.0, 2.0, 0.0], [2.0, 3.0, 1.0], A, 255);
            // The top (+y) face dropped.
            b.tris.retain(|t| !t.p.iter().all(|p| p[1] == 3.0));
            s.tris.extend(b.tris.into_iter().map(|t| Tri { tag: 2, ..t }));
        });
        s.tagged(3, |s| s.cuboid([0.0, 4.0, 0.0], [2.0, 5.0, 1.0], B, 120));
        let cut = section(&s, &Section::across(0, 1.0, Keep::Low)).unwrap();
        assert!(on(&cut, 0, 1.0).is_empty());
        assert!(cut.tris.iter().flat_map(|t| t.p).all(|p| p[0] <= 1.0));
        // A lower threshold caps the see-through box in its own opacity.
        let low = section(&s, &Section { cap_alpha: 100, ..Section::across(0, 1.0, Keep::Low) }).unwrap();
        let caps = on(&low, 0, 1.0);
        assert!(!caps.is_empty() && caps.iter().all(|t| t.alpha == 120 && t.tag == 3));
    }

    #[test]
    fn planes_beyond_the_scene_keep_it_or_empty_it_and_planes_on_faces_are_exact() {
        let mut s = Scene::default();
        s.cuboid([0.0; 3], [1.0; 3], A, 255);
        s.line([0.0; 3], [1.0; 3], Rgb::BLACK, 255);
        assert_eq!(section(&s, &Section::across(0, 10.0, Keep::Low)).unwrap(), s);
        let none = section(&s, &Section::across(0, -10.0, Keep::Low)).unwrap();
        assert!(none.tris.is_empty() && none.lines.is_empty());
        // On a face: the face goes and the cap takes its place, the same square.
        let flush = section(&s, &Section::across(0, 1.0, Keep::Low)).unwrap();
        assert!((cap_area(&flush, 0, 1.0) - 1.0).abs() < 1e-12);
        let total: f64 = flush.tris.iter().map(area).sum();
        assert!((total - 6.0).abs() < 1e-12);
        // On the far face: an infinitely thin slice, nothing.
        assert!(section(&s, &Section::across(0, 0.0, Keep::Low)).unwrap().tris.is_empty());
        // Through two opposite edges (a diagonal plane) and through a corner only.
        let r = 0.5f64.sqrt();
        let diag = section(&s, &Section::new([1.0, 1.0, 0.0], r)).unwrap();
        let tilted: f64 = diag.tris.iter().filter(|t| t.p.iter().all(|p| (p[0] + p[1] - 1.0).abs() < 1e-12)).map(area).sum();
        assert!((tilted - 2f64.sqrt()).abs() < 1e-9, "{tilted}");
        let corner = section(&s, &Section::new([1.0, 1.0, 1.0], 3f64.sqrt() / 3.0)).unwrap();
        // The corner tetrahedron's cut: an equilateral triangle of side √2.
        let tri: f64 = corner.tris.iter().filter(|t| t.p.iter().all(|p| (p[0] + p[1] + p[2] - 1.0).abs() < 1e-9)).map(area).sum();
        assert!((tri - 3f64.sqrt() / 2.0).abs() < 1e-9, "{tri}");
    }

    #[test]
    fn hostile_planes_are_refused_and_hostile_scenes_do_not_panic() {
        let mut s = Scene::default();
        s.cuboid([0.0; 3], [1.0; 3], A, 255);
        for bad in [
            Section::across(0, f64::NAN, Keep::Low),
            Section::across(0, f64::INFINITY, Keep::High),
            Section::across(0, 1e300, Keep::Low),
            Section::across(7, 0.5, Keep::Low),
            Section::new([0.0; 3], 0.5),
            Section::new([f64::NAN, 1.0, 0.0], 0.5),
            Section::new([f64::INFINITY, 0.0, 0.0], 0.5),
            Section::new([-0.0, 0.0, 0.0], 0.5),
        ] {
            assert!(section(&s, &bad).is_err(), "{bad:?}");
            assert!(!bad.keeps([0.0; 3]));
        }
        assert!(Section::across(1, 0.5, Keep::Low).keeps([9.0, 0.2, 9.0]));
        assert!(!Section::across(1, 0.5, Keep::High).keeps([9.0, 0.2, 9.0]));
        // Non-finite and huge points, degenerate triangles, a tiny normal along no axis.
        s.tri([f64::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], A, 255);
        s.tri([1e300, 0.0, 0.0], [-1e300, 0.0, 0.0], [0.0, 1e300, 0.0], A, 255);
        s.tri([0.5, 0.5, 0.5], [0.5, 0.5, 0.5], [0.6, 0.5, 0.5], A, 255);
        s.line([f64::INFINITY; 3], [0.0; 3], Rgb::BLACK, 255);
        let tiny = [Section::new([1e-300, 2e-300, 0.0], 0.0), Section::new([1e-320, 0.0, 0.0], 0.5)];
        for sec in [Section::across(0, 0.5, Keep::Low), Section::new([3.0, -1.0, 2.0], 0.1)].into_iter().chain(tiny) {
            let cut = section(&s, &sec).unwrap();
            assert!(cut.tris.iter().flat_map(|t| t.p).flatten().all(|v| v.is_finite()));
        }
        assert!(section(&Scene::default(), &Section::across(0, 0.0, Keep::Low)).unwrap().tris.is_empty());
    }

    #[test]
    fn past_its_caps_a_section_is_clipped_without_caps() {
        let mut s = Scene::default();
        for i in 0..5 {
            let x = 2.0 * i as f64;
            s.tagged(i + 1, |s| s.cuboid([x, 0.0, 0.0], [x + 1.0, 1.0, 1.0], A, 255));
        }
        let sec = Section::across(2, 0.5, Keep::Low);
        let full = section_with(&s, &sec, LIMITS).unwrap();
        assert!((cap_area(&full, 2, 0.5) - 5.0).abs() < 1e-12);
        let few = section_with(&s, &sec, Limits { bodies: 2, ..LIMITS }).unwrap();
        assert!((cap_area(&few, 2, 0.5) - 2.0).abs() < 1e-12);
        let short = section_with(&s, &sec, Limits { segments: 16, ..LIMITS }).unwrap();
        assert!((cap_area(&short, 2, 0.5) - 2.0).abs() < 1e-12);
        let outlined = Section { outline: Some(Rgb::BLACK), ..sec };
        let poor = section_with(&s, &outlined, Limits { work: 3, ..LIMITS }).unwrap();
        assert!(on(&poor, 2, 0.5).is_empty());
        assert!(poor.lines.is_empty(), "no outlines without their caps");
        assert_eq!(poor.tris.len(), full.tris.len() - on(&full, 2, 0.5).len());
    }

    /// A small model like the waveguide samples: three sections of different materials along
    /// z, a metal post across the middle one, ports on the ends and the boxes' edges.
    fn guide() -> Scene {
        let (a, b) = (0.02286, 0.01016);
        let mut s = Scene::default();
        let colours = [Rgb(120, 170, 220), Rgb(230, 170, 70), Rgb(140, 200, 120)];
        let lengths = [0.02, 0.015, 0.02];
        let mut z = 0.0;
        for (i, (c, l)) in colours.iter().zip(lengths).enumerate() {
            s.tagged(10 + i as u32, |s| s.cuboid([0.0, 0.0, z], [a, b, z + l], *c, 255));
            for (p, q) in [([0.0, 0.0], [a, 0.0]), ([a, 0.0], [a, b]), ([a, b], [0.0, b]), ([0.0, b], [0.0, 0.0])] {
                s.line([p[0], p[1], z], [q[0], q[1], z], Rgb(40, 40, 40), 200);
            }
            for (x, y) in [(0.0, 0.0), (a, 0.0), (a, b), (0.0, b)] {
                s.line([x, y, z], [x, y, z + l], Rgb(40, 40, 40), 200);
            }
            z += l;
        }
        // The post: a 48-sided cylinder along y through the middle section's centre.
        let (cx, cz, r) = (0.5 * a, 0.0275, 0.0015);
        s.tagged(20, |s| {
            let at = |k: usize, y: f64| {
                let t = std::f64::consts::TAU * k as f64 / 48.0;
                [cx + r * t.cos(), y, cz + r * t.sin()]
            };
            let metal = Rgb(110, 110, 120);
            for k in 0..48 {
                s.quad([at(k, 0.0), at(k + 1, 0.0), at(k + 1, b), at(k, b)], metal, 255);
                s.tri([cx, 0.0, cz], at(k + 1, 0.0), at(k, 0.0), metal, 255);
                s.tri([cx, b, cz], at(k, b), at(k + 1, b), metal, 255);
            }
        });
        for (i, z) in [0.0, z].into_iter().enumerate() {
            s.tagged(30 + i as u32, |s| s.quad([[0.0, 0.0, z], [a, 0.0, z], [a, b, z], [0.0, b, z]], Rgb(220, 60, 60), 220));
        }
        s
    }

    #[test]
    fn a_sectioned_guide_renders_its_insides_the_same_every_time() {
        let s = guide();
        let sec = Section { outline: Some(Rgb(30, 30, 30)), ..Section::across(0, 0.5 * 0.02286, Keep::Low) };
        let cut = section(&s, &sec).unwrap();
        assert_eq!(cut, section(&s, &sec).unwrap());
        // Every section capped with its own colour, the post's cut (a 3 × 10.16 mm rectangle at
        // the 48-gon's widest) over the middle one's.
        let caps = on(&cut, 0, 0.5 * 0.02286);
        let by_tag = |tag: u32| caps.iter().filter(|t| t.tag == tag).map(area).sum::<f64>();
        let post = by_tag(20);
        assert!((post - 0.003 * 0.01016).abs() < 1e-12, "{post}");
        assert!((by_tag(10) - 0.02 * 0.01016).abs() < 1e-12);
        assert!((by_tag(11) - 0.015 * 0.01016 + post).abs() < 1e-12);
        assert!((by_tag(12) - 0.02 * 0.01016).abs() < 1e-12);
        // Ports (open faces) are clipped, not capped.
        assert!(caps.iter().all(|t| t.tag < 30));
        assert!(cut.tris.iter().any(|t| t.tag == 30));
        let (lo, hi) = s.bounds().unwrap();
        let mut cam = Camera { projection: Projection::Perspective, ..Camera::default() };
        cam.standard(StandardView::Iso);
        cam.yaw = 1.1;
        cam.fit(lo, hi, 4.0 / 3.0);
        let opt = Options { width: 640, height: 480, supersample: 2, background: (Rgb(250, 250, 252), Rgb(225, 228, 235)), line_width: 1.2 };
        let pic = render(&cut, &cam, &opt);
        assert_eq!(pic, render(&cut, &cam, &opt));
        // The middle of the cut shows the post, picked as itself.
        let c = cam.project([0.5 * 0.02286, 0.005, 0.0275], 640.0, 480.0).unwrap();
        assert_eq!(pick(&cut, &cam, c[0], c[1], 640.0, 480.0), Some(20));
        let px = pic.pixel(c[0] as usize, c[1] as usize).unwrap();
        assert!(px.0.abs_diff(px.1) < 20 && px.1.abs_diff(px.2) < 30, "the post is grey: {px:?}");
        // The GPU path gets the same primitives.
        assert!(!flatten(&cut, &cam, 640.0, 480.0).is_empty());
        if let Some(dir) = std::env::var_os("EMCRAFT_RENDER_SHOTS") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("section-guide.png"), pic.png().unwrap()).unwrap();
            std::fs::write(dir.join("section-guide-whole.png"), render(&s, &cam, &opt).png().unwrap()).unwrap();
            let z = section(&s, &Section { outline: Some(Rgb(30, 30, 30)), ..Section::across(2, 0.0275, Keep::Low) }).unwrap();
            std::fs::write(dir.join("section-guide-z.png"), render(&z, &cam, &opt).png().unwrap()).unwrap();
        }
    }
}

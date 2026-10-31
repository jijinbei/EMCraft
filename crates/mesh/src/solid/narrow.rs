//! Narrow gaps of the region: two of its flat faces across one axis facing each other closer
//! than a tenth of the element size (a post half a millimetre from a wall, two plates nearly
//! touching). The mesher's own proximity sizing reads the gap only at the surface's vertices,
//! which lie an element apart on a coarse wall; here each pair of facing triangles adds a local
//! size as small as the gap over the box between them, graded away from it at the mesher's rate
//! (`emcraft_tetmesh::size::LocalSize`), so the gap is filled with elements about as wide as it
//! is, not flattened. What that costs is estimated first — each unit of gap area (the facing
//! triangles' overlap, clipped exactly) takes about 12/w² tetrahedra for a gap w wide (the
//! mesher makes 6.5/w² to 13/w² of plates and posts 0.05 mm off a wall at 4 mm) — and gaps that
//! would need more than the mesh's limit are refused, naming the narrowest, before any is made.
//! A gap narrower than the mesher's smallest size ([`MIN_SIZE`] of the element size) is left to
//! its proximity sizing, which fills it at that size; one under a tenth of that is refused, its
//! elements would be flat.

use emcraft_model::Point3;
use emcraft_tetmesh::size::{LocalSize, Region};

use super::mesher::mesher_point;
use super::tessellation::Tessellation;
use crate::flush::Triangles;
use crate::{MAX_TETS, MeshError, Result};

/// Gaps narrower than this fraction of the element size are refined.
pub(super) const BELOW: f64 = 0.1; // unit-factors: relative to the element size

/// The smallest local size, as a fraction of the element size: the mesher's own floor
/// (`QualityOptions::min_size`).
pub(super) const MIN_SIZE: f64 = 0.02; // unit-factors: relative to the element size

/// A gap narrower than this fraction of the smallest size is refused: its elements would be
/// flat.
const FLATTEST: f64 = 0.1; // unit-factors: relative to the smallest size

/// The most local sizes the gaps of one solid add.
pub(super) const MAX_GAP_SIZES: usize = 100_000;

/// Tetrahedra per unit area of a gap filled at size h, times h².
const TETS_PER_AREA: f64 = 12.0;

/// A gap for messages: its width and a point in it.
fn named(width: f64, at: Point3) -> String {
    format!("a gap {:.4} mm wide near ({:.3}, {:.3}, {:.3}) mm", width * 1e3, at.x * 1e3, at.y * 1e3, at.z * 1e3)
}

/// The local sizes that refine the tessellation's narrow gaps (width under [`BELOW`] of `size`,
/// over `flush`), and the tetrahedra they add, about. Refused when that is more than
/// [`MAX_TETS`], a gap is too narrow for any element, or the gaps need more than
/// [`MAX_GAP_SIZES`] local sizes.
pub(super) fn gap_sizes(t: &Tessellation, size: f64, flush: f64) -> Result<(Vec<LocalSize>, f64)> {
    let flats = crate::clip::flat(Triangles { positions: &t.positions, triangles: &t.triangles }, flush);
    // Per axis, the triangles facing down it (the low side of a gap: outward −) and up it, by plane.
    let mut up: [Vec<(f64, usize)>; 3] = Default::default();
    let mut down: [Vec<(f64, usize)>; 3] = Default::default();
    for &(axis, at, outward, i) in &flats {
        if outward > 0.0 { up[axis].push((at, i)) } else { down[axis].push((at, i)) }
    }
    for list in up.iter_mut().chain(down.iter_mut()) {
        list.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    }
    let (below, floor) = (BELOW * size, MIN_SIZE * size);
    let mut out = Vec::new();
    let mut tets = 0.0;
    // The narrowest gap, for the message.
    let mut worst: Option<(f64, Point3)> = None;
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for &(c1, i) in &down[axis] {
            let list = &up[axis];
            let from = list.partition_point(|o| o.0 <= c1 + flush);
            for &(c2, j) in list[from..].iter().take_while(|o| o.0 < c1 + below) {
                let (Some(a), Some(b)) = (corners(t, i), corners(t, j)) else { continue };
                let flat2 = |q: [Point3; 3]| q.map(|p| [p[u], p[v]]);
                let common = overlap(flat2(a), flat2(b));
                let area = polygon_area(&common);
                if !(area > flush * flush) {
                    continue;
                }
                let lo = common.iter().fold([f64::INFINITY; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
                let hi = common.iter().fold([f64::NEG_INFINITY; 2], |m, p| [m[0].max(p[0]), m[1].max(p[1])]);
                let mut p = Point3::ORIGIN;
                let mut q = Point3::ORIGIN;
                (p[axis], q[axis], p[u], q[u], p[v], q[v]) = (c1, c2, lo[0], hi[0], lo[1], hi[1]);
                let width = c2 - c1;
                if width < FLATTEST * floor {
                    return Err(MeshError::invalid(
                        "geometry",
                        format!(
                            "{} is narrower than a tenth of the smallest element ({:.4} mm, {MIN_SIZE} of the element size): its elements would be flat; widen the gap, let the faces touch, or use a smaller element size",
                            named(width, p.midpoint(q)),
                            floor * 1e3
                        ),
                    ));
                }
                // Under the smallest size the mesher's own proximity sizing (at the floor) is all
                // there is: elements as small as the floor all over such a gap would only be
                // flatter and slower (a 0.02 mm gap at 4 mm: 127 s instead of 13).
                if width < floor {
                    continue;
                }
                let h = width;
                tets += TETS_PER_AREA * area / (h * h);
                if worst.is_none_or(|w| width < w.0) {
                    worst = Some((width, p.midpoint(q)));
                }
                if out.len() >= MAX_GAP_SIZES {
                    return Err(MeshError::invalid(
                        "geometry",
                        format!(
                            "narrow gaps between its faces need more than {MAX_GAP_SIZES} local element sizes; widen them or let the faces touch"
                        ),
                    ));
                }
                out.push(LocalSize { region: Region::Box { lo: mesher_point(p), hi: mesher_point(q) }, size: h });
            }
        }
    }
    if tets > MAX_TETS as f64
        && let Some((width, at)) = worst
    {
        return Err(MeshError::invalid(
            "geometry",
            format!(
                "{} (and the region's other gaps under {:.3} mm, a tenth of the element size) would need about {:.2e} tetrahedra to fill with elements as small as the gaps; the limit is {MAX_TETS}: widen the gap or let the faces touch",
                named(width, at),
                below * 1e3,
                tets
            ),
        ));
    }
    Ok((out, tets))
}

fn corners(t: &Tessellation, i: usize) -> Option<[Point3; 3]> {
    crate::shared::corners(&t.positions, t.triangles.get(i)?)
}

/// The overlap of two triangles of a plane: `a` clipped by each side of `b` in turn
/// (Sutherland–Hodgman), a convex polygon (empty when they do not overlap).
fn overlap(a: [[f64; 2]; 3], b: [[f64; 2]; 3]) -> Vec<[f64; 2]> {
    let cross = |o: [f64; 2], p: [f64; 2], q: [f64; 2]| (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0]);
    // `b` counter-clockwise.
    let b = if cross(b[0], b[1], b[2]) < 0.0 { [b[0], b[2], b[1]] } else { b };
    let mut poly: Vec<[f64; 2]> = a.to_vec();
    for k in 0..3 {
        let (e0, e1) = (b[k], b[(k + 1) % 3]);
        let inside = |p: [f64; 2]| cross(e0, e1, p) >= 0.0;
        let mut next = Vec::with_capacity(poly.len() + 1);
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            let (ip, iq) = (inside(p), inside(q));
            if ip {
                next.push(p);
            }
            if ip != iq {
                let (dp, dq) = (cross(e0, e1, p), cross(e0, e1, q));
                let t = dp / (dp - dq);
                next.push([p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]);
            }
        }
        poly = next;
        if poly.is_empty() {
            break;
        }
    }
    poly
}

/// A polygon's area (either way round).
fn polygon_area(p: &[[f64; 2]]) -> f64 {
    let n = p.len();
    0.5 * (0..n).map(|i| p[i][0] * p[(i + 1) % n][1] - p[(i + 1) % n][0] * p[i][1]).sum::<f64>().abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A slab of air `side` square and `w` thick across z, as 12 triangles facing out.
    fn slab(side: f64, w: f64) -> Tessellation {
        let s = crate::clip::half_space(&[], emcraft_model::Aabb::new(Point3::ORIGIN, Point3::new(side, side, w)), 0.0);
        Tessellation { positions: s.positions, triangles: s.triangles, tri_face: s.tags, ..Tessellation::default() }
    }

    #[test]
    fn narrow_gaps_get_sizes_as_small_as_they_are_within_the_caps() {
        let (size, flush) = (1.0, 1e-5);
        // 0.05 thick: the two faces' triangles overlap in the whole square, once.
        let (sizes, tets) = gap_sizes(&slab(2.0, 0.05), size, flush).unwrap();
        assert!(!sizes.is_empty() && sizes.iter().all(|l| l.size == 0.05), "{sizes:?}");
        assert!((tets - TETS_PER_AREA * 4.0 / 0.0025).abs() < 1e-6 * tets, "{tets}");
        // As thick as a tenth of the size or more, or under the smallest size: nothing added.
        assert!(gap_sizes(&slab(2.0, 0.1), size, flush).unwrap().0.is_empty());
        assert!(gap_sizes(&slab(2.0, 0.01), size, flush).unwrap().0.is_empty());
        // Too thin for any element, or too wide an area for the limit: refused, named.
        let e = gap_sizes(&slab(2.0, 0.001), size, flush).unwrap_err().to_string();
        assert!(e.contains("a gap 1.0000 mm wide") && e.contains("flat"), "{e}");
        let e = gap_sizes(&slab(200.0, 0.03), size, flush).unwrap_err().to_string();
        assert!(e.contains("tetrahedra") && e.contains("limit is 3000000"), "{e}");
        // Overlaps clipped exactly: two unit right triangles, one turned by half a turn.
        let a = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        assert!((polygon_area(&overlap(a, [[1.0, 1.0], [0.0, 1.0], [1.0, 0.0]]))).abs() < 1e-15);
        assert!((polygon_area(&overlap(a, [[0.5, 0.0], [0.5, 1.0], [-0.5, 0.0]])) - 0.3125).abs() < 1e-12);
    }
}

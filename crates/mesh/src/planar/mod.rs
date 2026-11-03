//! Planar meshes of printed circuit boards: one triangulation of a rectangular region shared by
//! every layer, each triangle knowing which net's copper covers it on each layer. The layered
//! mesher extrudes it through the board's dielectrics, the copper becoming conducting sheets on
//! the triangles it covers.
//!
//! The triangulation is made in two passes on a lattice of 1 nm ([`lattice`]), so the same board
//! gives the same mesh bit for bit:
//! 1. **Union.** Every copper outline (traces with round ends, discs, polygons: [`offset`]),
//!    clipped to the region, is put in a constrained Delaunay triangulation ([`cdt`]) with its
//!    edges split where they meet ([`split`]); each triangle's copper is then decided per layer
//!    ([`label`]). The edges between triangles covered differently on some layer are the
//!    outline of each layer's union of copper — the Boolean union found on the triangulation
//!    rather than on polygons, so overlapping traces, pads and pours need no polygon clipping.
//! 2. **Mesh.** Those edges alone (the overlaps' inner edges gone), split to the element size at
//!    the copper's edges, are triangulated again and refined (Ruppert) to a size field that is
//!    fine at the copper's edges and grows away from them ([`size`]); the triangles are
//!    labelled again.
//!
//! Every count is capped ([`MAX_POINTS`], [`MAX_TRIANGLES`]); the input is checked (finite,
//! within [`lattice::MAX_SIDE`]) and two nets covering the same place on a layer are refused.
//!
//! ```
//! use emcraft_mesh::planar::{Outline, Request, offset, triangulate};
//!
//! // A 0.3 mm trace across a 4 mm × 2 mm region on the top layer (metres).
//! let ring = offset::trace([-1e-3, 1e-3], [5e-3, 1e-3], 0.3e-3, 1e-6);
//! let request = Request {
//!     lo: [0.0, 0.0],
//!     hi: [4e-3, 2e-3],
//!     layers: &["top".into()],
//!     nets: &["signal".into()],
//!     outlines: &[Outline { layer: 0, net: 0, ring }],
//!     size: 0.5e-3,
//!     edge_size: 0.1e-3,
//! };
//! let mesh = triangulate(&request)?;
//! // The trace covers 4 mm × 0.3 mm of the region (its ends are clipped away).
//! assert!((mesh.copper_area(0) - 4e-3 * 0.3e-3).abs() < 1e-12);
//!
//! // A size that is not a positive number is refused.
//! assert!(triangulate(&Request { size: f64::NAN, ..request }).is_err());
//! # Ok::<(), emcraft_mesh::MeshError>(())
//! ```

mod cdt;
pub mod extrude;
mod label;
pub mod lattice;
pub mod offset;
mod size;
mod split;

use emcraft_tetmesh::predicates::ExactPoint2 as Point2;

use crate::{MeshError, Result};
use cdt::{Cdt, Kind};
use label::{Ring, Rings};
use lattice::{Lattice, MAX_SIDE};
use size::{GRADING, Near};
use split::Points;

/// The marker of a triangle without copper on a layer.
pub const NO_NET: u32 = u32::MAX;
/// The most outline points (a cap on hostile boards).
pub const MAX_POINTS: usize = 1 << 20;
/// The most triangles of one planar mesh.
pub const MAX_TRIANGLES: usize = 2_000_000;
/// The most layers.
pub const MAX_LAYERS: usize = 64;

/// A closed copper outline: a net's copper on a layer (metres; either way round; it may reach
/// outside the region).
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
    /// The layer's index into [`Request::layers`].
    pub layer: usize,
    /// The net's index into [`Request::nets`].
    pub net: u32,
    pub ring: Vec<[f64; 2]>,
}

/// What to triangulate: the region `lo`..`hi` (metres), the layers' and nets' names, the copper
/// outlines and the element sizes far from copper (`size`) and at its edges (`edge_size`).
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    pub lo: [f64; 2],
    pub hi: [f64; 2],
    pub layers: &'a [String],
    pub nets: &'a [String],
    pub outlines: &'a [Outline],
    pub size: f64,
    pub edge_size: f64,
}

/// A planar mesh: counterclockwise triangles over `points` (metres) and, for each layer, the net
/// over each triangle ([`NO_NET`] where there is no copper).
#[derive(Clone, Debug, PartialEq)]
pub struct Planar {
    pub points: Vec<[f64; 2]>,
    pub triangles: Vec<[u32; 3]>,
    pub nets: Vec<Vec<u32>>,
}

impl Planar {
    /// The area of `layer`'s copper (m²).
    pub fn copper_area(&self, layer: usize) -> f64 {
        let Some(nets) = self.nets.get(layer) else { return 0.0 };
        self.triangles.iter().zip(nets).filter(|(_, n)| **n != NO_NET).map(|(t, _)| self.area(t)).sum()
    }

    /// The area of triangle `t` (m²).
    pub fn area(&self, t: &[u32; 3]) -> f64 {
        let [a, b, c] = t.map(|v| self.points.get(v as usize).copied().unwrap_or([0.0; 2]));
        0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]))
    }
}

fn invalid(why: impl Into<String>) -> MeshError {
    MeshError::invalid("planar mesh", why)
}

/// Refuses a request that is not finite, too large or inconsistent.
fn check(r: &Request) -> Result<()> {
    let finite = |p: [f64; 2]| p[0].is_finite() && p[1].is_finite();
    if !(finite(r.lo) && finite(r.hi) && r.lo[0] < r.hi[0] && r.lo[1] < r.hi[1]) {
        return Err(invalid("the region needs finite corners, the second above and right of the first"));
    }
    if !(r.hi[0] - r.lo[0] <= MAX_SIDE && r.hi[1] - r.lo[1] <= MAX_SIDE) {
        return Err(invalid(format!("the region's sides may be at most {MAX_SIDE} m")));
    }
    if !(r.size > 0.0 && r.size.is_finite() && r.edge_size > 0.0 && r.edge_size <= r.size) {
        return Err(invalid("the element sizes must be positive numbers, the size at copper edges no larger than the size"));
    }
    if !(r.edge_size >= 1e3 * lattice::STEP) {
        return Err(invalid("the element size at copper edges must be at least 1 µm"));
    }
    let (w, h) = (r.hi[0] - r.lo[0], r.hi[1] - r.lo[1]);
    if w * h / (r.size * r.size) > MAX_TRIANGLES as f64 {
        return Err(MeshError::TooLarge { tets: (w * h / (r.size * r.size)).min(usize::MAX as f64) as usize, limit: MAX_TRIANGLES });
    }
    if r.layers.is_empty() || r.layers.len() > MAX_LAYERS {
        return Err(invalid(format!("a board needs 1 to {MAX_LAYERS} layers")));
    }
    let points: usize = r.outlines.iter().map(|o| o.ring.len()).sum();
    if points > MAX_POINTS {
        return Err(invalid(format!("the copper outlines have {points} points, more than the limit of {MAX_POINTS}")));
    }
    for o in r.outlines {
        if o.layer >= r.layers.len() || o.net as usize >= r.nets.len() {
            return Err(invalid("an outline names a layer or net the board does not have"));
        }
        if !o.ring.iter().all(|p| finite(*p)) {
            return Err(invalid("an outline has a point that is not finite"));
        }
    }
    Ok(())
}

/// Triangulates the request's region with its copper (the module docs say how).
pub fn triangulate(r: &Request) -> Result<Planar> {
    check(r)?;
    let lattice = Lattice::new(r.lo);
    let top = lattice.on(r.hi);
    let corners = [[0.0, 0.0], [top[0], 0.0], top, [0.0, top[1]]];

    // Pass 1: the outlines, split where they meet, and the copper over each triangle.
    let mut points = Points::default();
    let ids: Vec<u32> = corners.iter().map(|&c| points.id(c)).collect();
    let mut edges: Vec<[u32; 2]> = (0..4).map(|k| [ids[k], ids[(k + 1) % 4]]).collect();
    // Each kept outline's layer, net and first edge.
    let mut owners: Vec<(usize, u32, usize)> = Vec::new();
    for o in r.outlines {
        let mut ring: Vec<u32> = offset::clipped(&o.ring, r.lo, r.hi).into_iter().map(|p| points.id(lattice.on(p))).collect();
        ring.dedup();
        while ring.len() > 1 && ring.first() == ring.last() {
            ring.pop();
        }
        if ring.len() < 3 {
            continue;
        }
        owners.push((o.layer, o.net, edges.len()));
        edges.extend((0..ring.len()).map(|k| [ring[k], ring[(k + 1) % ring.len()]]));
    }
    let chains = split::split(&mut points, &edges)?;
    let exact = |xy: &[[f64; 2]]| xy.iter().map(|p| Point2::new(p[0], p[1]).map_err(|e| invalid(e.to_string()))).collect::<Result<Vec<_>>>();
    let rings: Vec<Ring> = owners
        .iter()
        .enumerate()
        .map(|(k, &(layer, net, first))| {
            let end = owners.get(k + 1).map_or(edges.len(), |o| o.2);
            let ids: Vec<u32> =
                chains.get(first..end).into_iter().flatten().flat_map(|c| c.split_last().map_or(&[][..], |(_, rest)| rest)).copied().collect();
            let xy: Vec<[f64; 2]> = ids.iter().filter_map(|&v| points.xy.get(v as usize).copied()).collect();
            Ok(Ring { layer, net, points: exact(&xy)? })
        })
        .collect::<Result<_>>()?;
    let rings = Rings::new(rings, [0.0, 0.0], top)?;
    let (segments, constrained) = pieces(&chains, 4);
    let first = Cdt::build(&exact(&points.xy)?, &segments, MAX_TRIANGLES)?.into_triangulation();
    let nets = label::label(&first.points, &first.triangles, &constrained, &rings, (r.layers, r.nets))?;

    // The edges between triangles covered differently, and the region's outline.
    let mut sides: Vec<([u32; 2], u32)> = first
        .triangles
        .iter()
        .enumerate()
        .flat_map(|(t, v)| (0..3).map(move |k| ([v[k].min(v[(k + 1) % 3]), v[k].max(v[(k + 1) % 3])], t as u32)))
        .collect();
    sides.sort_unstable();
    let differ = |s: u32, t: u32| nets.iter().any(|n| n.get(s as usize) != n.get(t as usize));
    let mut outline: Vec<([f64; 2], [f64; 2], bool)> = Vec::new();
    let mut k = 0;
    while let Some(&(e, s)) = sides.get(k) {
        let pair = sides.get(k + 1).filter(|x| x.0 == e).map(|x| x.1);
        k += if pair.is_some() { 2 } else { 1 };
        let copper = match pair {
            Some(t) if differ(s, t) => true,
            Some(_) => continue,
            None => false,
        };
        let at = |v: u32| first.points.get(v as usize).copied().ok_or_else(|| invalid("a point id out of range"));
        let (a, b) = (at(e[0])?, at(e[1])?);
        outline.push(if a <= b { (a, b, copper) } else { (b, a, copper) });
    }
    // In the order of their coordinates, so the second pass does not depend on the order the
    // outlines came in.
    outline
        .sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal).then(x.1.partial_cmp(&y.1).unwrap_or(std::cmp::Ordering::Equal)));

    // Pass 2: those edges split to the element size, triangulated and refined.
    let step = lattice::STEP;
    let (big, small) = (r.size / step, r.edge_size / step);
    let reach = (big - small) / GRADING;
    let near = Near::new(outline.iter().filter(|s| s.2).copied().collect(), [0.0, 0.0], top, big)?;
    let size = |p: [f64; 2]| (small + GRADING * near.distances(p, reach).0).min(big);
    let mut points = Points::default();
    let mut edges: Vec<[u32; 2]> = Vec::new();
    let mut is_copper: Vec<bool> = Vec::new();
    for &(a, b, copper) in &outline {
        let mut prev = points.id(a);
        let along = if copper { vec![] } else { spaced(a, b, &size, small) };
        let n = if copper { ((b[0] - a[0]).hypot(b[1] - a[1]) / small).ceil().clamp(1.0, 1e6) as usize } else { along.len() + 1 };
        for i in 1..=n {
            let t = if copper { i as f64 / n as f64 } else { along.get(i - 1).copied().unwrap_or(1.0) };
            let next = if i == n { points.id(b) } else { points.id([(a[0] + t * (b[0] - a[0])).round(), (a[1] + t * (b[1] - a[1])).round()]) };
            if next != prev {
                edges.push([prev, next]);
                is_copper.push(copper);
            }
            prev = next;
        }
        if points.xy.len() > MAX_POINTS {
            return Err(invalid(format!("the copper's edges need more than {MAX_POINTS} points; use a larger element size at copper edges")));
        }
    }
    let chains = split::split(&mut points, &edges)?;
    let mut segments: Vec<([u32; 2], Kind)> = chains
        .iter()
        .zip(&is_copper)
        .flat_map(|(c, &copper)| c.windows(2).map(move |w| ([w[0], w[1]], if copper { Kind::Segment } else { Kind::Outline })))
        .collect();
    segments.sort_unstable_by_key(|s| [s.0[0].min(s.0[1]), s.0[0].max(s.0[1])]);
    segments.dedup_by_key(|s| [s.0[0].min(s.0[1]), s.0[0].max(s.0[1])]);
    let mut constrained: Vec<[u32; 2]> = segments.iter().map(|s| [s.0[0].min(s.0[1]), s.0[0].max(s.0[1])]).collect();
    constrained.sort_unstable();
    let guard = Near::new(
        segments.iter().filter_map(|s| Some((*points.xy.get(s.0[0] as usize)?, *points.xy.get(s.0[1] as usize)?, false))).collect(),
        [0.0, 0.0],
        top,
        big,
    )?;
    let mut mesh = Cdt::build(&exact(&points.xy)?, &segments, MAX_TRIANGLES)?;
    mesh.refine(&size, &|p, h| guard.distances(p, h).1 >= cdt::CLEARANCE * h)?;
    let mesh = mesh.into_triangulation();
    let nets = label::label(&mesh.points, &mesh.triangles, &constrained, &rings, (r.layers, r.nets))?;
    Ok(compact(mesh.points, mesh.triangles, nets, lattice))
}

/// The points (as fractions of the way, strictly between 0 and 1) splitting the edge `a`–`b`
/// into pieces of about the local size `size`: the k-th where ∫ ds / h reaches k times the whole
/// integral over its rounded-up value (a graded spacing, finer where `size` is smaller). `finest`
/// is the smallest the size gets, which sets the samples of the integral.
fn spaced(a: [f64; 2], b: [f64; 2], size: &dyn Fn([f64; 2]) -> f64, finest: f64) -> Vec<f64> {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    let samples = ((2.0 * len / finest).ceil() as usize).clamp(2, 1 << 16);
    let at = |t: f64| size([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
    // The running integral of 1/h at the samples (trapezoids).
    let mut sum = vec![0.0; samples + 1];
    let mut last = 1.0 / at(0.0);
    for k in 1..=samples {
        let here = 1.0 / at(k as f64 / samples as f64);
        sum[k] = sum[k - 1] + 0.5 * (last + here) * len / samples as f64;
        last = here;
    }
    let total = sum[samples];
    let n = (total.ceil() as usize).clamp(1, 1 << 20);
    let mut out = Vec::with_capacity(n.saturating_sub(1));
    let mut k = 0;
    for i in 1..n {
        let target = total * i as f64 / n as f64;
        while k < samples && sum[k + 1] < target {
            k += 1;
        }
        let (lo, hi) = (sum[k], sum[(k + 1).min(samples)]);
        let frac = if hi > lo { (target - lo) / (hi - lo) } else { 0.0 };
        out.push((k as f64 + frac) / samples as f64);
    }
    out
}

/// The pieces of `chains` as constrained segments (the first `outline` chains the region's
/// outline, the rest copper), each once, and the same as sorted `[low, high]` pairs.
fn pieces(chains: &[Vec<u32>], outline: usize) -> (Vec<([u32; 2], Kind)>, Vec<[u32; 2]>) {
    let mut segments: Vec<([u32; 2], Kind)> = chains
        .iter()
        .enumerate()
        .flat_map(|(k, c)| c.windows(2).map(move |w| ([w[0], w[1]], if k < outline { Kind::Outline } else { Kind::Segment })))
        .collect();
    // The region's outline first, so a copper edge along it keeps it the outline.
    segments.sort_by_key(|s| (s.1 != Kind::Outline, [s.0[0].min(s.0[1]), s.0[0].max(s.0[1])]));
    let mut constrained: Vec<[u32; 2]> = segments.iter().map(|s| [s.0[0].min(s.0[1]), s.0[0].max(s.0[1])]).collect();
    constrained.sort_unstable();
    constrained.dedup();
    (segments, constrained)
}

/// The mesh with only the points its triangles use, in metres.
fn compact(points: Vec<[f64; 2]>, triangles: Vec<[u32; 3]>, nets: Vec<Vec<u32>>, lattice: Lattice) -> Planar {
    let mut id = vec![NO_NET; points.len()];
    for &v in triangles.iter().flatten() {
        if let Some(x) = id.get_mut(v as usize) {
            *x = 0;
        }
    }
    let mut kept = Vec::new();
    for (x, p) in id.iter_mut().zip(&points) {
        if *x == 0 {
            *x = kept.len() as u32;
            kept.push(lattice.back(*p));
        }
    }
    let triangles = triangles.iter().map(|t| t.map(|v| id.get(v as usize).copied().unwrap_or(0))).collect();
    Planar { points: kept, triangles, nets }
}

#[cfg(test)]
mod tests;

//! The outlines' edges split where they meet, so that a constrained triangulation can take them:
//! no two pieces cross, and no point lies inside a piece.
//!
//! Points are on the lattice (whole numbers, [`super::lattice`]); each decision is an exact
//! `orient2d` or a comparison of whole numbers. Where two edges cross, the crossing is rounded to
//! the nearest lattice point and both edges are split there (moving each by under a step, as in
//! snap rounding: J. D. Hobby, *Practical segment intersection with finite precision output*,
//! Computational Geometry 13(4), 1999); where an end of one edge lies on another, or two run
//! along each other, the other is split at that end. Rounding can make new meetings, so the
//! passes repeat until one finds none (at most [`MAX_PASSES`]).
//!
//! Edges are found near each other through a uniform grid of cells; a pair is tested in the
//! first cell both boxes share only, so it is tested once.

use std::collections::BTreeMap;

use emcraft_tetmesh::predicates::{ExactPoint2 as Point2, Sign, orient2d};

use crate::{MeshError, Result};

/// The most passes over the edges before splitting gives up.
const MAX_PASSES: usize = 16;
/// The most pair tests in one pass (a cap on hostile inputs).
const MAX_WORK: usize = 1 << 28;

/// Points by their lattice coordinates, each with its id.
#[derive(Default)]
pub(super) struct Points {
    pub xy: Vec<[f64; 2]>,
    ids: BTreeMap<[i64; 2], u32>,
}

impl Points {
    /// The id of lattice point `p`, added when new.
    pub fn id(&mut self, p: [f64; 2]) -> u32 {
        let key = [p[0] as i64, p[1] as i64];
        *self.ids.entry(key).or_insert_with(|| {
            self.xy.push(p);
            self.xy.len() as u32 - 1
        })
    }

    fn exact(&self, v: u32) -> Result<Point2> {
        let p = self.xy.get(v as usize).ok_or_else(|| fail("a point id out of range"))?;
        Point2::new(p[0], p[1]).map_err(|e| fail(&e.to_string()))
    }
}

fn fail(why: &str) -> MeshError {
    MeshError::invalid("board outline", why)
}

/// Each input edge as a chain of point ids from its start to its end, every piece free of
/// crossings with every other (`edges` holds point ids into `points`).
pub(super) fn split(points: &mut Points, edges: &[[u32; 2]]) -> Result<Vec<Vec<u32>>> {
    let mut chains: Vec<Vec<u32>> = edges.iter().map(|e| if e[0] == e[1] { vec![e[0]] } else { e.to_vec() }).collect();
    for _ in 0..MAX_PASSES {
        let pieces: Vec<[u32; 2]> = chains.iter().flat_map(|c| c.windows(2).map(|w| [w[0], w[1]])).collect();
        let cuts = meetings(points, &pieces)?;
        if cuts.iter().all(Vec::is_empty) {
            return Ok(chains);
        }
        // Each piece's cuts in order along it, the chains rebuilt piece by piece.
        let mut k = 0;
        for chain in &mut chains {
            let mut out = Vec::with_capacity(chain.len());
            for w in chain.windows(2) {
                let [a, b] = [w[0], w[1]];
                let (pa, pb) = (point(points, a)?, point(points, b)?);
                let mut on: Vec<(f64, u32)> = cuts
                    .get(k)
                    .into_iter()
                    .flatten()
                    .filter(|&&v| v != a && v != b)
                    .map(|&v| point(points, v).map(|p| ((p[0] - pa[0]) * (pb[0] - pa[0]) + (p[1] - pa[1]) * (pb[1] - pa[1]), v)))
                    .collect::<Result<_>>()?;
                on.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
                on.dedup_by_key(|x| x.1);
                out.push(a);
                out.extend(on.iter().map(|x| x.1));
                k += 1;
            }
            if let Some(&last) = chain.last() {
                out.push(last);
            }
            out.dedup();
            *chain = out;
        }
    }
    Err(fail(&format!("its edges still meet after {MAX_PASSES} passes of splitting; simplify the copper outlines")))
}

fn point(points: &Points, v: u32) -> Result<[f64; 2]> {
    points.xy.get(v as usize).copied().ok_or_else(|| fail("a point id out of range"))
}

/// For each piece, the points where it must be split (new crossing points are added to `points`).
fn meetings(points: &mut Points, pieces: &[[u32; 2]]) -> Result<Vec<Vec<u32>>> {
    let mut cuts = vec![Vec::new(); pieces.len()];
    if pieces.is_empty() {
        return Ok(cuts);
    }
    let boxes: Vec<([f64; 2], [f64; 2])> = pieces
        .iter()
        .map(|&[a, b]| {
            let (pa, pb) = (point(points, a)?, point(points, b)?);
            Ok(([pa[0].min(pb[0]), pa[1].min(pb[1])], [pa[0].max(pb[0]), pa[1].max(pb[1])]))
        })
        .collect::<Result<_>>()?;
    let lo = boxes.iter().fold([f64::INFINITY; 2], |l, b| [l[0].min(b.0[0]), l[1].min(b.0[1])]);
    let hi = boxes.iter().fold([f64::NEG_INFINITY; 2], |h, b| [h[0].max(b.1[0]), h[1].max(b.1[1])]);
    // Cells about as many as pieces, square in the box.
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0);
    let side = (span / (pieces.len() as f64).sqrt().max(1.0)).max(1.0);
    let cells = [((hi[0] - lo[0]) / side).floor() as usize + 1, ((hi[1] - lo[1]) / side).floor() as usize + 1];
    let cell = |x: f64, d: usize| (((x - lo[d]) / side).floor().max(0.0) as usize).min(cells[d] - 1);
    let range = |b: &([f64; 2], [f64; 2])| [[cell(b.0[0], 0), cell(b.1[0], 0)], [cell(b.0[1], 1), cell(b.1[1], 1)]];
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); cells[0].saturating_mul(cells[1]).min(MAX_WORK)];
    let mut work = 0usize;
    for (k, b) in boxes.iter().enumerate() {
        let r = range(b);
        for i in r[0][0]..=r[0][1] {
            for j in r[1][0]..=r[1][1] {
                work += 1;
                if work > MAX_WORK {
                    return Err(fail("too many edges near each other"));
                }
                if let Some(c) = grid.get_mut(i * cells[1] + j) {
                    c.push(k as u32);
                }
            }
        }
    }
    for (c, members) in grid.iter().enumerate() {
        let (i, j) = (c / cells[1], c % cells[1]);
        for (m, &p) in members.iter().enumerate() {
            for &q in members.iter().skip(m + 1) {
                work += 1;
                if work > MAX_WORK {
                    return Err(fail("too many edges near each other"));
                }
                let (bp, bq) = (boxes[p as usize], boxes[q as usize]);
                if bp.0[0] > bq.1[0] || bq.0[0] > bp.1[0] || bp.0[1] > bq.1[1] || bq.0[1] > bp.1[1] {
                    continue;
                }
                // Tested in the first cell both boxes share only.
                let (rp, rq) = (range(&bp), range(&bq));
                if (rp[0][0].max(rq[0][0]), rp[1][0].max(rq[1][0])) != (i, j) {
                    continue;
                }
                meet(points, pieces, &mut cuts, p as usize, q as usize)?;
            }
        }
    }
    Ok(cuts)
}

/// Records where pieces `p` and `q` meet: a crossing (rounded), or an end of one inside the
/// other.
fn meet(points: &mut Points, pieces: &[[u32; 2]], cuts: &mut [Vec<u32>], p: usize, q: usize) -> Result<()> {
    let ([a, b], [c, d]) = (pieces[p], pieces[q]);
    let [ea, eb, ec, ed] = [points.exact(a)?, points.exact(b)?, points.exact(c)?, points.exact(d)?];
    let (o1, o2) = (orient2d(&ea, &eb, &ec), orient2d(&ea, &eb, &ed));
    let (o3, o4) = (orient2d(&ec, &ed, &ea), orient2d(&ec, &ed, &eb));
    if [o1, o2, o3, o4].iter().all(|o| !o.is_zero()) {
        if o1 != o2 && o3 != o4 {
            let [pa, pb, pc, pd] = [ea.coords(), eb.coords(), ec.coords(), ed.coords()];
            let (r, s) = ([pb[0] - pa[0], pb[1] - pa[1]], [pd[0] - pc[0], pd[1] - pc[1]]);
            let t = ((pc[0] - pa[0]) * s[1] - (pc[1] - pa[1]) * s[0]) / (r[0] * s[1] - r[1] * s[0]);
            let x = [(pa[0] + t * r[0]).round(), (pa[1] + t * r[1]).round()];
            if !(x[0].is_finite() && x[1].is_finite()) {
                return Err(fail("a crossing could not be placed"));
            }
            let v = points.id(x);
            for (k, ends) in [(p, [a, b]), (q, [c, d])] {
                if !ends.contains(&v)
                    && let Some(list) = cuts.get_mut(k)
                {
                    list.push(v);
                }
            }
        }
        return Ok(());
    }
    // An end of one on the other (touching, or running along it).
    for (k, [u, w], ends, os) in [(p, [ea, eb], [c, d], [(o1, ec), (o2, ed)]), (q, [ec, ed], [a, b], [(o3, ea), (o4, eb)])] {
        for (&v, (o, e)) in ends.iter().zip(os) {
            if o == Sign::Zero
                && strictly_between(&u, &w, &e)
                && let Some(list) = cuts.get_mut(k)
            {
                list.push(v);
            }
        }
    }
    Ok(())
}

/// Whether `x`, on the line through `a` and `b`, lies strictly between them.
fn strictly_between(a: &Point2, b: &Point2, x: &Point2) -> bool {
    let (a, b, x) = (a.coords(), b.coords(), x.coords());
    let d = if a[0] != b[0] { 0 } else { 1 };
    (a[d] < x[d] && x[d] < b[d]) || (b[d] < x[d] && x[d] < a[d])
}

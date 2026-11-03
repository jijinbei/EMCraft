//! The element size over the region: fine at the copper's edges, where the fields are singular,
//! growing away from them. `h(p) = min(H, h₀ + g · d(p))`, `d` the distance to the nearest copper
//! edge, `h₀` the size at the edges, `H` the size far away and `g` the grading.

use crate::{MeshError, Result};

/// How fast the size grows away from the copper: half the distance.
pub(super) const GRADING: f64 = 0.5;
/// The most cells of the edges' grid, and of cell visits while filling it.
const MAX_CELLS: usize = 1 << 22;

/// Segments with a grid of the cells they cross, for the distance to the nearest.
pub(super) struct Near {
    segments: Vec<([f64; 2], [f64; 2], bool)>,
    lo: [f64; 2],
    side: f64,
    cells: [usize; 2],
    grid: Vec<Vec<u32>>,
}

fn distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (d, q) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
    let len2 = d[0] * d[0] + d[1] * d[1];
    let t = if len2 > 0.0 { ((q[0] * d[0] + q[1] * d[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (q[0] - t * d[0]).hypot(q[1] - t * d[1])
}

impl Near {
    /// Indexes `segments` (ends, and whether the segment is copper's) in the box `lo`..`hi`
    /// with cells of about `side`.
    pub fn new(segments: Vec<([f64; 2], [f64; 2], bool)>, lo: [f64; 2], hi: [f64; 2], side: f64) -> Result<Near> {
        let fewest = (((hi[0] - lo[0]) * (hi[1] - lo[1])).max(0.0) / MAX_CELLS as f64).sqrt();
        let side = side.max(fewest).max(1.0);
        let cells = [((hi[0] - lo[0]) / side).floor() as usize + 1, ((hi[1] - lo[1]) / side).floor() as usize + 1];
        let mut near = Near { segments: Vec::new(), lo, side, cells, grid: vec![Vec::new(); cells[0].saturating_mul(cells[1]).min(MAX_CELLS)] };
        let mut visits = 0usize;
        for (k, (a, b, _)) in segments.iter().enumerate() {
            let ([i0, j0], [i1, j1]) = (near.cell([a[0].min(b[0]), a[1].min(b[1])]), near.cell([a[0].max(b[0]), a[1].max(b[1])]));
            for i in i0..=i1 {
                for j in j0..=j1 {
                    visits += 1;
                    if visits > 4 * MAX_CELLS {
                        return Err(MeshError::invalid(
                            "board copper",
                            "too many edges in the region; use a smaller region or a larger element size",
                        ));
                    }
                    if let Some(c) = near.grid.get_mut(i * cells[1] + j) {
                        c.push(k as u32);
                    }
                }
            }
        }
        near.segments = segments;
        Ok(near)
    }

    fn cell(&self, p: [f64; 2]) -> [usize; 2] {
        let at = |d: usize| (((p[d] - self.lo[d]) / self.side).floor().max(0.0) as usize).min(self.cells[d] - 1);
        [at(0), at(1)]
    }

    /// The distances from `p` to the nearest copper segment and to the nearest of any, each
    /// capped at `reach` (only the cells within it are searched).
    pub fn distances(&self, p: [f64; 2], reach: f64) -> (f64, f64) {
        let ([i0, j0], [i1, j1]) = (self.cell([p[0] - reach, p[1] - reach]), self.cell([p[0] + reach, p[1] + reach]));
        let (mut copper, mut any) = (reach, reach);
        for i in i0..=i1 {
            for j in j0..=j1 {
                for &k in self.grid.get(i * self.cells[1] + j).into_iter().flatten() {
                    if let Some(&(a, b, is_copper)) = self.segments.get(k as usize) {
                        let d = distance(p, a, b);
                        any = any.min(d);
                        if is_copper {
                            copper = copper.min(d);
                        }
                    }
                }
            }
        }
        (copper, any)
    }
}

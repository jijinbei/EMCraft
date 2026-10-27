//! The factorization's kernels on a front held by the device: complex f64, (re, im)
//! interleaved, column-major m × m.

use cubecl::prelude::*;

/// Pivots per block, as the solver's (`emcraft_solver::ldlt`'s BLOCK): the shared tiles' depth.
pub(crate) const BLOCK: usize = 64;

/// The most units per side of an update cube (a device may hold fewer); each unit takes
/// REACH × REACH entries, so a cube of u × u units covers a tile of side u REACH.
pub(crate) const UNITS: u32 = 16;
pub(crate) const REACH: usize = 4;

/// Pivots staged in shared memory at a time.
const DEPTH: usize = 16;

/// Interchanges rows p and q of column `first + ABSOLUTE_POS` for each pair of `swaps` (p₀,
/// q₀, p₁, q₁, …) in order; one unit per column.
#[cube(launch)]
pub(crate) fn swap_rows(front: &mut [f64], swaps: &[u32], pairs: u32, m: u32, first: u32, columns: u32) {
    let c = ABSOLUTE_POS;
    if c < columns as usize {
        let base = (first as usize + c) * (m as usize);
        for s in 0..pairs as usize {
            let p = 2 * (base + swaps[2 * s] as usize);
            let q = 2 * (base + swaps[2 * s + 1] as usize);
            let pr = front[p];
            let pi = front[p + 1];
            front[p] = front[q];
            front[p + 1] = front[q + 1];
            front[q] = pr;
            front[q + 1] = pi;
        }
    }
}

/// F[i][j] −= Σₜ W[i − be][t] L[j − be][t] for rows and columns from `be`, on columns before `k`
/// every row, after it the lower part; W and L are `below` × `nb` (complex, column-major). Each
/// cube of `units` × `units` takes a tile of side `units` REACH, staging DEPTH pivots of its rows
/// of W and of its columns' rows of L in shared memory at a time; unit (x, y) sums the entries
/// (x + units a, y + units b) of the tile in registers, each over t in order.
#[cube(launch)]
pub(crate) fn update(front: &mut [f64], w: &[f64], l: &[f64], m: u32, be: u32, k: u32, nb: u32, #[comptime] units: usize) {
    let side = units * REACH;
    let m = m as usize;
    let be = be as usize;
    let k = k as usize;
    let nb = nb as usize;
    let below = m - be;
    let r0 = (CUBE_POS_X as usize) * side;
    let c0 = (CUBE_POS_Y as usize) * side;
    // A tile wholly above the diagonal among the trailing columns has nothing to do (every unit
    // of the cube agrees, before any synchronization).
    let skip = be + c0 >= k && r0 + side <= c0;
    if !skip {
        let mut ws = Shared::<[f64]>::new_slice(2 * units * REACH * DEPTH);
        let mut ls = Shared::<[f64]>::new_slice(2 * units * REACH * DEPTH);
        let mut acc = Array::<f64>::new(2 * REACH * REACH);
        #[unroll]
        for q in 0..2 * REACH * REACH {
            acc[q] = 0.0;
        }
        let x = UNIT_POS_X as usize;
        let y = UNIT_POS_Y as usize;
        let u = y * units + x;
        let mut t0 = 0;
        while t0 < nb {
            let depth = if nb - t0 < DEPTH { nb - t0 } else { DEPTH };
            // Stage pivots t0..t0 + depth of the tile's rows (side of them) of W and L.
            let mut q = u;
            while q < side * depth {
                let rr = q % side;
                let t = t0 + q / side;
                let wi = 2 * (t * below + r0 + rr);
                let li = 2 * (t * below + c0 + rr);
                let mut wr = 0.0;
                let mut wim = 0.0;
                if r0 + rr < below {
                    wr = w[wi];
                    wim = w[wi + 1];
                }
                let mut lr = 0.0;
                let mut lim = 0.0;
                if c0 + rr < below {
                    lr = l[li];
                    lim = l[li + 1];
                }
                ws[2 * q] = wr;
                ws[2 * q + 1] = wim;
                ls[2 * q] = lr;
                ls[2 * q + 1] = lim;
                q += units * units;
            }
            sync_cube();
            for t in 0..depth {
                let mut wv = Array::<f64>::new(2 * REACH);
                let mut lv = Array::<f64>::new(2 * REACH);
                #[unroll]
                for a in 0..REACH {
                    let o = 2 * (t * side + x + units * a);
                    wv[2 * a] = ws[o];
                    wv[2 * a + 1] = ws[o + 1];
                    let p = 2 * (t * side + y + units * a);
                    lv[2 * a] = ls[p];
                    lv[2 * a + 1] = ls[p + 1];
                }
                #[unroll]
                for a in 0..REACH {
                    #[unroll]
                    for b in 0..REACH {
                        let q = 2 * (a * REACH + b);
                        acc[q] += wv[2 * a] * lv[2 * b] - wv[2 * a + 1] * lv[2 * b + 1];
                        acc[q + 1] += wv[2 * a] * lv[2 * b + 1] + wv[2 * a + 1] * lv[2 * b];
                    }
                }
            }
            sync_cube();
            t0 += DEPTH;
        }
        #[unroll]
        for a in 0..REACH {
            #[unroll]
            for b in 0..REACH {
                let r = r0 + x + units * a;
                let c = c0 + y + units * b;
                let i = be + r;
                let j = be + c;
                if r < below && c < below && (j < k || i >= j) {
                    let q = 2 * (a * REACH + b);
                    let at = 2 * (j * m + i);
                    front[at] -= acc[q];
                    front[at + 1] -= acc[q + 1];
                }
            }
        }
    }
}

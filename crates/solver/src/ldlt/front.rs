//! A front's elimination with Bunch–Kaufman block pivoting, on the CPU or with the front held by
//! a device ([`crate::accel`]).

use std::ops::Range;

use faer::linalg::matmul::triangular::{BlockStructure, matmul};
use faer::{Accum, MatMut, MatRef, Par};
use num_complex::Complex64 as C;

use crate::accel::{Accelerator, DeviceError};
use crate::multifrontal::FactorError;

const ALPHA: f64 = 0.640_388_203_202_208; // (1 + √17)/8

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Pivot {
    One(C),
    /// A 2 × 2 block [[a, b], [b, c]] at this column and the next.
    Two(C, C, C),
    /// The second column of a 2 × 2 block.
    Second,
}

/// What a front's elimination counts.
#[derive(Default)]
pub(super) struct PivotCounts {
    /// Pivots replaced by the static-pivot threshold.
    pub perturbed: usize,
    pub two_by_two: usize,
}

/// Largest |F[i][j]| over rows `from..m` other than `j` (column j of a column-major m × m front).
fn colmax(f: &[C], m: usize, j: usize, from: usize) -> (usize, f64) {
    let mut best = (usize::MAX, 0.0);
    for i in from..m {
        if i != j {
            let v = f[j * m + i].norm();
            if v > best.1 {
                best = (i, v);
            }
        }
    }
    best
}

/// Interchanges fully summed variables p and q (< k) of the front.
fn interchange(f: &mut [C], m: usize, k: usize, rows: &mut [usize], p: usize, q: usize) {
    if p == q {
        return;
    }
    for c in 0..k {
        f.swap(c * m + p, c * m + q);
    }
    for r in 0..m {
        f.swap(p * m + r, q * m + r);
    }
    rows.swap(p, q);
}

/// Pivot columns per block: pivots are chosen within a block, then the block updates the rest
/// of the front at once.
const BLOCK: usize = 64;

/// Fronts whose block update is smaller than this many complex multiply–adds run on one thread.
const PARALLEL_WORK: usize = 1 << 20;

/// Panel updates (rows × columns) from this size run on threads.
const PANEL_PARALLEL: usize = 1 << 16;

/// Eliminates the first `k` columns of a front (column-major m × m: full in the first k columns,
/// lower in the rest) in blocks of [`BLOCK`] pivots. Within a block, pivots are found by the
/// Bunch–Kaufman test among the block's columns and applied right-looking to the block alone;
/// the block then updates every later column with one product, F[i][j] −= Σ_t W[i][t] L[j][t]
/// (W = L D), split by columns over threads. Each entry is summed in the same order on any
/// number of threads, so the result is the same bit for bit. On return the first k columns hold
/// L (unit diagonal) and the rest the Schur complement (lower); `rows` follows the interchanges.
pub(super) fn factor_front(f: &mut [C], m: usize, k: usize, rows: &mut [usize], tau: f64) -> (Vec<Pivot>, PivotCounts) {
    let mut e = Elimination::new(m, k, rows, tau);
    let mut b0 = 0;
    while b0 < k {
        let be = (b0 + BLOCK).min(k);
        e.factor_block(f, b0..be);
        update_below(f, m, k, b0..be, &e.pivots);
        b0 = be;
    }
    (e.pivots, e.counts)
}

/// [`factor_front`] with the front held by `device`: each block's columns are read back, its
/// pivots found here (the same code), its interchanges and its update of the later columns done
/// there; the Schur complement is read back at the end.
pub(super) fn factor_front_on(
    device: &dyn Accelerator,
    f: &mut [C],
    m: usize,
    k: usize,
    rows: &mut [usize],
    tau: f64,
) -> Result<(Vec<Pivot>, PivotCounts), FactorError> {
    let failed = |e: DeviceError| FactorError::Device(format!("{}: {e}", device.name()));
    let mut front = device.front(f, m).map_err(failed)?;
    let mut e = Elimination::new(m, k, rows, tau);
    let mut b0 = 0;
    while b0 < k {
        let be = (b0 + BLOCK).min(k);
        front.read(b0..be, &mut f[b0 * m..be * m]).map_err(failed)?;
        e.swaps.clear();
        e.factor_block(f, b0..be);
        if be < k && !e.swaps.is_empty() {
            front.swap_rows(be..k, &e.swaps).map_err(failed)?;
        }
        if be < m {
            let w = block_w(f, m, b0..be, &e.pivots);
            let l: Vec<C> = (b0..be).flat_map(|c| f[c * m + be..(c + 1) * m].iter().copied()).collect();
            front.update(be, k, &w, &l).map_err(failed)?;
        }
        b0 = be;
    }
    if k < m {
        front.read(k..m, &mut f[k * m..]).map_err(failed)?;
    }
    Ok((e.pivots, e.counts))
}

/// A front's pivoting: its order m and k pivots, the front's rows (following the
/// interchanges), the static-pivot threshold, and what it found.
struct Elimination<'a> {
    m: usize,
    k: usize,
    rows: &'a mut [usize],
    tau: f64,
    pivots: Vec<Pivot>,
    counts: PivotCounts,
    /// The interchanges of variables of the current block.
    swaps: Vec<(usize, usize)>,
}

impl<'a> Elimination<'a> {
    fn new(m: usize, k: usize, rows: &'a mut [usize], tau: f64) -> Elimination<'a> {
        Elimination { m, k, rows, tau, pivots: vec![Pivot::Second; k], counts: PivotCounts::default(), swaps: Vec::new() }
    }

    /// Finds the pivots of the block of columns `block` by the Bunch–Kaufman test among its
    /// columns and applies them right-looking to the block alone (rows below p, the block's
    /// later columns), recording each interchange of variables in `swaps`.
    fn factor_block(&mut self, f: &mut [C], block: Range<usize>) {
        let be = block.end;
        let mut p = block.start;
        while p < be {
            p = match self.choose(f, p, be) {
                (col, None) => self.one(f, p, col, be),
                (_, Some(q)) => self.two(f, p, q, be),
            };
        }
    }

    /// The pivot at column p of a block ending at `be`: (the column to bring to p, its 2 × 2
    /// partner).
    fn choose(&self, f: &[C], p: usize, be: usize) -> (usize, Option<usize>) {
        let (m, tau) = (self.m, self.tau);
        let (r, w1) = colmax(f, m, p, p);
        let app = f[p * m + p].norm();
        let mut choice: Option<(usize, Option<usize>)> = None;
        if app >= ALPHA * w1 && (app > 0.0 || w1 == 0.0) {
            choice = Some((p, None));
        } else if r < be {
            let (_, wr) = colmax(f, m, r, p);
            let arr = f[r * m + r].norm();
            if app * wr >= ALPHA * w1 * w1 {
                choice = Some((p, None));
            } else if arr >= ALPHA * wr {
                choice = Some((r, None));
            } else if p + 1 < be {
                choice = Some((p, Some(r)));
            }
        }
        if choice.is_none() {
            // The largest entry lies outside the block: another column of the block that
            // passes the test, else the best 2 × 2 inside it, else column p.
            for j in p + 1..be {
                let (_, wj) = colmax(f, m, j, p);
                if f[j * m + j].norm() >= ALPHA * wj && f[j * m + j].norm() > tau {
                    choice = Some((j, None));
                    break;
                }
            }
        }
        if choice.is_none() && p + 1 < be {
            let (q, wq) = (p + 1..be).fold((usize::MAX, 0.0), |b, i| if f[p * m + i].norm() > b.1 { (i, f[p * m + i].norm()) } else { b });
            if q != usize::MAX {
                let det = f[p * m + p] * f[q * m + q] - f[p * m + q] * f[p * m + q];
                if det.norm() > tau * wq {
                    choice = Some((p, Some(q)));
                }
            }
        }
        choice.unwrap_or((p, None))
    }

    /// Records an interchange of variables p and q of the front.
    fn interchange(&mut self, f: &mut [C], p: usize, q: usize) {
        interchange(f, self.m, self.k, self.rows, p, q);
        if q != p {
            self.swaps.push((p, q));
        }
    }

    /// Column `col` brought to p and eliminated as a 1 × 1 pivot; returns the next column.
    fn one(&mut self, f: &mut [C], p: usize, col: usize, be: usize) -> usize {
        let m = self.m;
        self.interchange(f, p, col);
        let mut d = f[p * m + p];
        if !(d.norm() > self.tau) {
            d = if d.norm() > 0.0 { d / d.norm() * self.tau } else { C::new(self.tau, 0.0) };
            self.counts.perturbed += 1;
        }
        self.pivots[p] = Pivot::One(d);
        // Original column values c_i, then l_i = c_i / d.
        let c: Vec<C> = f[p * m + p + 1..p * m + m].to_vec();
        let dinv = d.inv();
        for (x, ci) in f[p * m + p + 1..p * m + m].iter_mut().zip(&c) {
            *x = ci * dinv;
        }
        f[p * m + p] = C::new(1.0, 0.0);
        // The rest of the block's columns, every row below p: F −= l cᵀ.
        if p + 1 < be {
            let (rows, cols) = (m - p - 1, be - p - 1);
            let (left, right) = f.split_at_mut((p + 1) * m);
            let lcol = MatRef::from_column_major_slice(&left[p * m + p + 1..p * m + m], rows, 1);
            let crow = MatRef::from_row_major_slice(&c[..cols], 1, cols);
            let dst = MatMut::from_column_major_slice_mut(&mut right[..cols * m], m, cols).submatrix_mut(p + 1, 0, rows, cols);
            panel_update(dst, lcol, crow);
        }
        p + 1
    }

    /// Column q brought to p + 1 and (p, p + 1) eliminated as a 2 × 2 pivot; returns the next
    /// column.
    fn two(&mut self, f: &mut [C], p: usize, q: usize, be: usize) -> usize {
        let m = self.m;
        self.interchange(f, p + 1, q);
        let (a11, a21, a22) = (f[p * m + p], f[p * m + p + 1], f[(p + 1) * m + p + 1]);
        let det = a11 * a22 - a21 * a21;
        let (i11, i21, i22) = (a22 / det, -a21 / det, a11 / det);
        self.pivots[p] = Pivot::Two(a11, a21, a22);
        self.pivots[p + 1] = Pivot::Second;
        self.counts.two_by_two += 1;
        let c0v: Vec<C> = f[p * m + p + 2..p * m + m].to_vec();
        let c1v: Vec<C> = f[(p + 1) * m + p + 2..(p + 1) * m + m].to_vec();
        for i in 0..c0v.len() {
            f[p * m + p + 2 + i] = c0v[i] * i11 + c1v[i] * i21;
            f[(p + 1) * m + p + 2 + i] = c0v[i] * i21 + c1v[i] * i22;
        }
        f[p * m + p] = C::new(1.0, 0.0);
        f[p * m + p + 1] = C::new(0.0, 0.0);
        f[(p + 1) * m + p + 1] = C::new(1.0, 0.0);
        // F −= [l₀ l₁] [c₀ c₁]ᵀ over the rest of the block.
        if p + 2 < be {
            let (rows, cols) = (m - p - 2, be - p - 2);
            let (left, right) = f.split_at_mut((p + 2) * m);
            let lcols = MatRef::from_column_major_slice(&left[p * m..(p + 2) * m], m, 2).subrows(p + 2, rows);
            let crows: Vec<C> = c0v[..cols].iter().chain(&c1v[..cols]).copied().collect();
            let cm = MatRef::from_row_major_slice(&crows, 2, cols);
            let dst = MatMut::from_column_major_slice_mut(&mut right[..cols * m], m, cols).submatrix_mut(p + 2, 0, rows, cols);
            panel_update(dst, lcols, cm);
        }
        p + 2
    }
}

/// dst −= l c within a block's panel, threaded from [`PANEL_PARALLEL`] entries.
fn panel_update(dst: MatMut<'_, C>, l: MatRef<'_, C>, c: MatRef<'_, C>) {
    let par = if dst.nrows() * dst.ncols() >= PANEL_PARALLEL { Par::rayon(0) } else { Par::Seq };
    matmul(dst, BlockStructure::Rectangular, Accum::Add, l, BlockStructure::Rectangular, c, BlockStructure::Rectangular, C::new(-1.0, 0.0), par);
}

/// W = L D of the block `block`: its columns' rows below it times their pivots (column-major,
/// (m − be) × nb).
fn block_w(f: &[C], m: usize, block: Range<usize>, pivots: &[Pivot]) -> Vec<C> {
    let (b0, be) = (block.start, block.end);
    let (nb, below) = (be - b0, m - be);
    let mut w = vec![C::new(0.0, 0.0); below * nb];
    let col = |t: usize| &f[(b0 + t) * m + be..(b0 + t + 1) * m];
    let mut t = 0;
    while t < nb {
        match pivots[b0 + t] {
            Pivot::One(d) => {
                for (x, l) in w[t * below..(t + 1) * below].iter_mut().zip(col(t)) {
                    *x = l * d;
                }
                t += 1;
            }
            Pivot::Two(a11, a21, a22) => {
                let (c0, c1) = (col(t), col(t + 1));
                for i in 0..below {
                    let (l0, l1) = (c0[i], c1[i]);
                    w[t * below + i] = l0 * a11 + l1 * a21;
                    w[(t + 1) * below + i] = l0 * a21 + l1 * a22;
                }
                t += 2;
            }
            Pivot::Second => t += 1,
        }
    }
    w
}

/// The block `block`'s update of every later column, F −= W Lᵀ, by a blocked matrix product
/// (faer): the later panel columns over every row below the block (they pivot among
/// themselves), the trailing block's lower part. Its sums run in an order that does not depend
/// on the threads.
fn update_below(f: &mut [C], m: usize, k: usize, block: Range<usize>, pivots: &[Pivot]) {
    let (b0, be) = (block.start, block.end);
    let (nb, below) = (be - b0, m - be);
    if below == 0 {
        return;
    }
    // W = L D, column-major as the products take it best; the block's L below it is its
    // columns' lower part, in place.
    let w = block_w(f, m, b0..be, pivots);
    let (left, right) = f.split_at_mut(be * m);
    let lm = MatRef::from_column_major_slice(&left[b0 * m..], m, nb).subrows(be, below);
    let kp = k.saturating_sub(be).min(below);
    let par = if below * below * nb < PARALLEL_WORK { Par::Seq } else { Par::rayon(0) };
    let wm = MatRef::from_column_major_slice(&w, below, nb);
    // The columns be.. of the front, rows from be: column c of `right` is front column be + c.
    let mut fm = MatMut::from_column_major_slice_mut(right, m, m - be);
    let minus = C::new(-1.0, 0.0);
    if kp > 0 {
        let dst = fm.as_mut().submatrix_mut(be, 0, below, kp);
        matmul(
            dst,
            BlockStructure::Rectangular,
            Accum::Add,
            wm,
            BlockStructure::Rectangular,
            lm.subrows(0, kp).transpose(),
            BlockStructure::Rectangular,
            minus,
            par,
        );
    }
    if kp < below {
        let rest = below - kp;
        let dst = fm.submatrix_mut(be + kp, kp, rest, rest);
        matmul(
            dst,
            BlockStructure::TriangularLower,
            Accum::Add,
            wm.subrows(kp, rest),
            BlockStructure::Rectangular,
            lm.subrows(kp, rest).transpose(),
            BlockStructure::Rectangular,
            minus,
            par,
        );
    }
}

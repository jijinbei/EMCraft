//! Sparse complex symmetric factorization P A Pᵀ = L D Lᵀ (A = Aᵀ, not Hermitian) by the
//! multifrontal method: L unit lower triangular, D block diagonal with 1 × 1 and 2 × 2 pivots.
//!
//! **Structure.** The unknowns are ordered by nested dissection ([`crate::ordering`]), then by a
//! postorder of the elimination tree (J. W. H. Liu, *The role of elimination trees in sparse
//! factorization*, SIAM J. Matrix Anal. Appl. 11(1):134–172, 1990), and consecutive columns
//! with nested structures are merged into fundamental supernodes. Each supernode is factored in
//! a dense frontal matrix: its columns of A plus the update matrices its children left behind
//! (extend-add), the supernode's own columns eliminated, and the Schur complement passed to the
//! parent (I. S. Duff, J. K. Reid, *The multifrontal solution of indefinite sparse symmetric
//! linear equations*, ACM TOMS 9(3):302–325, 1983; J. W. H. Liu, *The multifrontal method for
//! sparse matrix solution: theory and practice*, SIAM Review 34(1):82–109, 1992).
//!
//! **Pivoting.** Within a front, the supernode's columns are fully summed and may be
//! interchanged freely without changing any structure. The pivot is chosen among them by the
//! Bunch–Kaufman test (J. R. Bunch, L. Kaufman, *Some stable methods for calculating inertia
//! and solving symmetric linear systems*, Math. Comp. 31(137):163–179, 1977) on the moduli of the
//! complex entries, as LAPACK's `zsytrf` does for complex symmetric matrices: with
//! α = (1 + √17)/8 and ω_j the largest off-diagonal modulus of column j, column p is a 1 × 1
//! pivot when |a_pp| ≥ α ω_p; otherwise its largest entry's row r gives a 1 × 1 pivot at r or the
//! 2 × 2 pivot (p, r). When r lies outside the supernode (not yet fully summed), another column
//! of the supernode that passes the test is taken, else the best 2 × 2 or 1 × 1 inside it, and a
//! pivot below √ε ‖A‖ is replaced by that value (static pivoting, as in O. Schenk, K. Gärtner,
//! *Solving unsymmetric sparse systems of linear equations with PARDISO*, FGCS 20(3):475–487,
//! 2004); [`Ldlt::solve_refined`] then recovers the accuracy by iterative refinement and reports
//! the backward error.

use emcraft_model::Point3;
use faer::linalg::matmul::triangular::{BlockStructure, matmul};
use faer::{Accum, MatMut, MatRef, Par};
use num_complex::Complex64 as C;
use rayon::prelude::*;

use crate::sparse::Csr;

/// The largest frontal matrix (rows) a factorization may form: m² complex entries.
pub const MAX_FRONT: usize = 20_000;

const ALPHA: f64 = 0.640_388_203_202_208; // (1 + √17)/8

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum FactorError {
    #[error("a frontal matrix of {rows} rows exceeds the limit of {limit}: the problem is too large for the direct solver")]
    FrontTooLarge { rows: usize, limit: usize },
    #[error("the matrix has a non-finite entry")]
    NotFinite,
    #[error("the matrix is zero")]
    Zero,
    #[error("dimension mismatch: {0}")]
    Dimension(String),
    #[error("the factor's scratch file: {0}")]
    Scratch(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pivot {
    One(C),
    /// A 2 × 2 block [[a, b], [b, c]] at this column and the next.
    Two(C, C, C),
    /// The second column of a 2 × 2 block.
    Second,
}

/// Where a supernode's factored columns are kept.
#[derive(Clone, Debug)]
enum Columns {
    Memory(Vec<C>),
    /// In the scratch file: `len` complex numbers from byte `offset`.
    Disk {
        offset: u64,
        len: usize,
    },
}

#[derive(Clone, Debug)]
struct Supernode {
    /// Positions of the front's rows; the first `k` are the supernode's columns in pivot order.
    rows: Vec<usize>,
    k: usize,
    /// The front's first k columns after elimination: L below the diagonal (m × k, by column).
    l: Columns,
    pivots: Vec<Pivot>,
}

/// The factor's columns beyond the memory budget, written to a file on disk (out-of-core):
/// the file is unlinked as soon as it is made, so it goes when the factorization does.
#[derive(Debug)]
struct Scratch {
    file: std::sync::Mutex<std::fs::File>,
    end: u64,
}

/// Bytes of factor kept in memory before the rest goes to the scratch file: the environment's
/// `EMCRAFT_FACTOR_MEMORY_GB`, else a quarter of the memory the system reports available (Linux; the
/// fronts and update matrices need the rest), else
/// 8 GB.
pub(crate) fn memory_budget() -> u64 {
    if let Some(gb) = std::env::var("EMCRAFT_FACTOR_MEMORY_GB").ok().and_then(|v| v.trim().parse::<f64>().ok()).filter(|g| g.is_finite() && *g >= 0.0)
    {
        return (gb * 1e9) as u64;
    }
    let available = std::fs::read_to_string("/proc/meminfo").ok().and_then(|t| {
        t.lines().find(|l| l.starts_with("MemAvailable:")).and_then(|l| l.split_whitespace().nth(1)).and_then(|kb| kb.parse::<u64>().ok())
    });
    available.map_or(8_000_000_000, |kb| kb * 1024 / 4)
}

/// The directory for scratch files: `EMCRAFT_SCRATCH`, else `~/.cache/emcraft` (on disk; the
/// system's temporary directory is often in memory), else the temporary directory.
fn scratch_dir() -> std::path::PathBuf {
    if let Some(d) = std::env::var_os("EMCRAFT_SCRATCH") {
        return d.into();
    }
    match std::env::var_os("HOME") {
        Some(h) => std::path::Path::new(&h).join(".cache").join("emcraft"),
        None => std::env::temp_dir(),
    }
}

impl Scratch {
    fn new() -> Result<Scratch, FactorError> {
        static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = scratch_dir();
        let fail = |e: std::io::Error| FactorError::Scratch(format!("{}: {e}", dir.display()));
        std::fs::create_dir_all(&dir).map_err(fail)?;
        let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = dir.join(format!("factor-{}-{n}.bin", std::process::id()));
        let file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path).map_err(fail)?;
        // Unlinked at once where the system allows it (the open handle keeps the data).
        let _ = std::fs::remove_file(&path);
        Ok(Scratch { file: std::sync::Mutex::new(file), end: 0 })
    }

    fn write(&mut self, v: &[C]) -> Result<Columns, FactorError> {
        use std::io::{Seek, SeekFrom, Write};
        let mut bytes = Vec::with_capacity(v.len() * 16);
        for z in v {
            bytes.extend_from_slice(&z.re.to_le_bytes());
            bytes.extend_from_slice(&z.im.to_le_bytes());
        }
        let mut f = self.file.lock().map_err(|_| FactorError::Scratch("the file's lock was poisoned".into()))?;
        f.seek(SeekFrom::Start(self.end)).and_then(|_| f.write_all(&bytes)).map_err(|e| FactorError::Scratch(e.to_string()))?;
        let offset = self.end;
        self.end += bytes.len() as u64;
        Ok(Columns::Disk { offset, len: v.len() })
    }

    fn read(&self, offset: u64, len: usize) -> Result<Vec<C>, FactorError> {
        use std::io::{Read, Seek, SeekFrom};
        let mut bytes = vec![0u8; len * 16];
        let mut f = self.file.lock().map_err(|_| FactorError::Scratch("the file's lock was poisoned".into()))?;
        f.seek(SeekFrom::Start(offset)).and_then(|_| f.read_exact(&mut bytes)).map_err(|e| FactorError::Scratch(e.to_string()))?;
        Ok(bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|c| {
                let mut re = [0u8; 8];
                let mut im = [0u8; 8];
                re.copy_from_slice(&c[..8]);
                im.copy_from_slice(&c[8..]);
                C::new(f64::from_le_bytes(re), f64::from_le_bytes(im))
            })
            .collect())
    }
}

/// A factorization.
#[derive(Clone, Debug)]
pub struct Ldlt {
    n: usize,
    /// `perm[position] = unknown`.
    perm: Vec<usize>,
    supernodes: Vec<Supernode>,
    /// The columns kept on disk, when the factor outgrew the memory budget.
    scratch: Option<std::sync::Arc<Scratch>>,
    /// Bytes of the factor on disk.
    pub bytes_on_disk: u64,
    /// The symmetric scaling the factor is of: S A S, s_i = 1/√(max_j |a_ij|).
    scale: Vec<f64>,
    /// Pivots replaced by √ε ‖S A S‖.
    pub perturbed: usize,
    pub two_by_two: usize,
    /// Entries of L (with the diagonal blocks).
    pub entries: usize,
    /// The largest front.
    pub largest_front: usize,
    /// Seconds of the symbolic analysis (when made with it) and of the numeric factorization.
    pub analysis_seconds: f64,
    pub numeric_seconds: f64,
}

/// The elimination tree of the pattern in position order: `parent[j]`, `usize::MAX` for roots.
fn etree(lower_rows_of: &[Vec<usize>]) -> Vec<usize> {
    // lower_rows_of[k] = positions i < k with a_ki ≠ 0 (Liu's algorithm with path compression).
    let n = lower_rows_of.len();
    let mut parent = vec![usize::MAX; n];
    let mut ancestor = vec![usize::MAX; n];
    for k in 0..n {
        for &i in &lower_rows_of[k] {
            let mut r = i;
            while ancestor[r] != usize::MAX && ancestor[r] != k {
                let next = ancestor[r];
                ancestor[r] = k;
                r = next;
            }
            if ancestor[r] == usize::MAX {
                ancestor[r] = k;
                parent[r] = k;
            }
        }
    }
    parent
}

/// A postorder of a forest: children before parents, each subtree contiguous.
fn postorder(parent: &[usize]) -> Vec<usize> {
    let n = parent.len();
    let mut head = vec![usize::MAX; n];
    let mut next = vec![usize::MAX; n];
    // Children in ascending order: insert in reverse.
    for j in (0..n).rev() {
        let p = parent[j];
        if p != usize::MAX {
            next[j] = head[p];
            head[p] = j;
        }
    }
    let mut out = Vec::with_capacity(n);
    let mut stack = Vec::new();
    for root in 0..n {
        if parent[root] != usize::MAX {
            continue;
        }
        stack.push((root, false));
        while let Some((v, expanded)) = stack.pop() {
            if expanded {
                out.push(v);
                continue;
            }
            stack.push((v, true));
            // Push children so the smallest is visited first.
            let mut kids = Vec::new();
            let mut c = head[v];
            while c != usize::MAX {
                kids.push(c);
                c = next[c];
            }
            for &c in kids.iter().rev() {
                stack.push((c, false));
            }
        }
    }
    out
}

/// For each position, the positions of the smaller neighbours (the row's strictly lower part).
fn lower_rows(a: &Csr, perm: &[usize], inv: &[usize]) -> Vec<Vec<usize>> {
    perm.iter()
        .enumerate()
        .map(|(k, &u)| {
            let mut r: Vec<usize> = a.col[a.ptr[u]..a.ptr[u + 1]].iter().map(|&v| inv[v]).filter(|&i| i < k).collect();
            r.sort_unstable();
            r
        })
        .collect()
}

fn invert(perm: &[usize]) -> Vec<usize> {
    let mut inv = vec![0; perm.len()];
    for (k, &u) in perm.iter().enumerate() {
        inv[u] = k;
    }
    inv
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

/// The start of column b of a packed lower triangle of order m (columns stored from their
/// diagonal down).
fn packed(b: usize, m: usize) -> usize {
    b * m - b * b.saturating_sub(1) / 2
}

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
/// L (unit diagonal) and the rest the Schur complement (lower).
fn factor_front(f: &mut [C], m: usize, k: usize, rows: &mut [usize], tau: f64, perturbed: &mut usize, two_by_two: &mut usize) -> Vec<Pivot> {
    let mut pivots = vec![Pivot::Second; k];
    let mut b0 = 0;
    while b0 < k {
        let be = (b0 + BLOCK).min(k);
        let mut p = b0;
        while p < be {
            let (r, w1) = colmax(f, m, p, p);
            let app = f[p * m + p].norm();
            // (column to bring to p, 2 × 2 partner)
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
            let (col, partner) = choice.unwrap_or((p, None));
            match partner {
                None => {
                    interchange(f, m, k, rows, p, col);
                    let mut d = f[p * m + p];
                    if !(d.norm() > tau) {
                        d = if d.norm() > 0.0 { d / d.norm() * tau } else { C::new(tau, 0.0) };
                        *perturbed += 1;
                    }
                    pivots[p] = Pivot::One(d);
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
                        matmul(
                            dst,
                            BlockStructure::Rectangular,
                            Accum::Add,
                            lcol,
                            BlockStructure::Rectangular,
                            crow,
                            BlockStructure::Rectangular,
                            C::new(-1.0, 0.0),
                            if rows * cols >= PANEL_PARALLEL { Par::rayon(0) } else { Par::Seq },
                        );
                    }
                    p += 1;
                }
                Some(q) => {
                    interchange(f, m, k, rows, p + 1, q);
                    let (a11, a21, a22) = (f[p * m + p], f[p * m + p + 1], f[(p + 1) * m + p + 1]);
                    let det = a11 * a22 - a21 * a21;
                    let (i11, i21, i22) = (a22 / det, -a21 / det, a11 / det);
                    pivots[p] = Pivot::Two(a11, a21, a22);
                    pivots[p + 1] = Pivot::Second;
                    *two_by_two += 1;
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
                        matmul(
                            dst,
                            BlockStructure::Rectangular,
                            Accum::Add,
                            lcols,
                            BlockStructure::Rectangular,
                            cm,
                            BlockStructure::Rectangular,
                            C::new(-1.0, 0.0),
                            if rows * cols >= PANEL_PARALLEL { Par::rayon(0) } else { Par::Seq },
                        );
                    }
                    p += 2;
                }
            }
        }

        // The block's L below it is its columns' lower part, in place; W = L D column by column
        // (both column-major, as the products take them best).
        let nb = be - b0;
        let below = m - be;
        if below > 0 {
            let (left, right) = f.split_at_mut(be * m);
            let lm = MatRef::from_column_major_slice(&left[b0 * m..], m, nb).subrows(be, below);
            let mut w = vec![C::new(0.0, 0.0); below * nb];
            let col = |t: usize| &left[(b0 + t) * m + be..(b0 + t + 1) * m];
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
            // F −= W Lᵀ by a blocked matrix product (faer): the later panel columns over every
            // row below the block (they pivot among themselves), the trailing block's lower part.
            // Its sums run in an order that does not depend on the threads.
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
        b0 = be;
    }
    pivots
}

/// Entries of a supernode's columns of L: k columns over m rows.
fn fill(k: usize, m: usize) -> usize {
    k * m - k * k.saturating_sub(1) / 2
}

/// Relaxed supernode amalgamation (C. Ashcraft, R. Grimes, *The influence of relaxed supernode
/// partitions on the multifrontal method*, ACM TOMS 15(4):291–309, 1989): a supernode is merged
/// into its parent when its columns come right before the parent's and the merged front stores
/// few explicit zeros: always up to 4 columns, up to 16 with under 80 % zeros, up to 48 with
/// under 10 %, and any size under 5 % (the thresholds of CHOLMOD: Y. Chen, T. A. Davis, W. W.
/// Hager, S. Rajamanickam, *Algorithm 887: CHOLMOD*, ACM TOMS 35(3):22, 2008). Small supernodes
/// would otherwise each build and clear a front as large as their parent's.
fn amalgamate(starts: &[usize], rows: Vec<Vec<usize>>, parent: &[usize], n: usize) -> (Vec<usize>, Vec<Vec<usize>>) {
    let ns = rows.len();
    let mut snode_of = vec![0usize; n];
    for s in 0..ns {
        for j in starts[s]..starts[s + 1] {
            snode_of[j] = s;
        }
    }
    let sparent: Vec<usize> = (0..ns).map(|s| parent.get(starts[s + 1] - 1).and_then(|&q| snode_of.get(q).copied()).unwrap_or(usize::MAX)).collect();
    // Merged supernodes keep the parent's id; `fills` counts the true entries they hold.
    let mut start: Vec<usize> = starts[..ns].to_vec();
    let mut rows: Vec<Vec<usize>> = rows;
    let mut fills: Vec<usize> = (0..ns).map(|s| fill(starts[s + 1] - starts[s], rows[s].len())).collect();
    let mut alive: Vec<usize> = Vec::with_capacity(ns);
    for p in 0..ns {
        while let Some(&c) = alive.last() {
            if sparent[c] != p {
                break;
            }
            let kc = starts[c + 1] - start[c];
            let kp = starts[p + 1] - start[p];
            let k = kc + kp;
            // The union of the two row sets (both ascending).
            let (a, b) = (&rows[c], &rows[p]);
            let mut merged = Vec::with_capacity(a.len() + b.len());
            let (mut i, mut j) = (0, 0);
            while i < a.len() || j < b.len() {
                let next = match (a.get(i), b.get(j)) {
                    (Some(&x), Some(&y)) if x == y => {
                        i += 1;
                        j += 1;
                        x
                    }
                    (Some(&x), Some(&y)) if x < y => {
                        i += 1;
                        x
                    }
                    (Some(_), Some(&y)) => {
                        j += 1;
                        y
                    }
                    (Some(&x), None) => {
                        i += 1;
                        x
                    }
                    (None, Some(&y)) => {
                        j += 1;
                        y
                    }
                    (None, None) => break,
                };
                merged.push(next);
            }
            let total = fill(k, merged.len());
            let real = fills[c] + fills[p];
            let zeros = total.saturating_sub(real) as f64 / total.max(1) as f64;
            let merge = k <= 4 || (k <= 16 && zeros < 0.8) || (k <= 48 && zeros < 0.1) || zeros < 0.05;
            if !merge {
                break;
            }
            alive.pop();
            start[p] = start[c];
            rows[p] = merged;
            fills[p] = real;
            rows[c] = Vec::new();
        }
        alive.push(p);
    }
    let mut out_starts: Vec<usize> = alive.iter().map(|&s| start[s]).collect();
    out_starts.push(n);
    let out_rows = alive.into_iter().map(|s| std::mem::take(&mut rows[s])).collect();
    (out_starts, out_rows)
}

/// The symbolic part of a factorization, which depends only on the pattern and the points: the
/// fill-reducing order, the elimination tree and the supernodes with their row structures.
#[derive(Clone, Debug)]
pub struct Analysis {
    n: usize,
    /// The pattern it was made for.
    ptr: Vec<usize>,
    col: Vec<usize>,
    perm: Vec<usize>,
    inv: Vec<usize>,
    parent: Vec<usize>,
    starts: Vec<usize>,
    super_rows: Vec<Vec<usize>>,
    snode_of: Vec<usize>,
    /// Seconds it took.
    pub seconds: f64,
}

impl Analysis {
    /// Orders the unknowns of `a`'s pattern (nested dissection on `xyz`, the last `last` at the
    /// end) and finds its supernodes.
    pub fn new(a: &Csr, xyz: &[Point3], last: usize) -> Result<Analysis, FactorError> {
        let t0 = std::time::Instant::now();
        let n = a.n;
        if xyz.len() != n || a.ptr.len() != n + 1 {
            return Err(FactorError::Dimension(format!("{} unknowns, {} points", n, xyz.len())));
        }
        // Ordering: nested dissection, then a postorder of its elimination tree.
        let nd = crate::ordering::fill_reducing_order_last(xyz, &a.adjacency(), last);
        let parent0 = etree(&lower_rows(a, &nd, &invert(&nd)));
        let post = postorder(&parent0);
        let perm: Vec<usize> = post.iter().map(|&p| nd[p]).collect();
        let inv = invert(&perm);
        let lower = lower_rows(a, &perm, &inv);
        let parent = etree(&lower);

        // Column structures (rows below the diagonal), children merged in; fundamental supernodes.
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (j, &p) in parent.iter().enumerate() {
            if p != usize::MAX {
                children[p].push(j);
            }
        }
        // Rows j' > j of column j of A, by position.
        let mut a_below: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (k, rows) in lower.iter().enumerate() {
            for &i in rows {
                a_below[i].push(k);
            }
        }
        let mut structure: Vec<Option<Vec<usize>>> = vec![None; n];
        let mut lengths = vec![0usize; n];
        let mut starts: Vec<usize> = Vec::new();
        let mut super_rows: Vec<Vec<usize>> = Vec::new();
        for j in 0..n {
            let mut s = std::mem::take(&mut a_below[j]);
            for &c in &children[j] {
                if let Some(cs) = structure[c].take() {
                    s.extend(cs.into_iter().filter(|&r| r > j));
                }
            }
            s.sort_unstable();
            s.dedup();
            lengths[j] = s.len();
            let extends = j > 0 && parent[j - 1] == j && children[j].len() == 1 && lengths[j - 1] == lengths[j] + 1;
            if !extends {
                starts.push(j);
                let mut rows = Vec::with_capacity(s.len() + 1);
                rows.push(j);
                rows.extend_from_slice(&s);
                super_rows.push(rows);
            }
            structure[j] = Some(s);
        }
        drop(structure);
        starts.push(n);

        let (starts, super_rows) = amalgamate(&starts, super_rows, &parent, n);
        let ns = super_rows.len();
        let mut snode_of = vec![0usize; n];
        for s in 0..ns {
            for j in starts[s]..starts[s + 1] {
                snode_of[j] = s;
            }
        }
        Ok(Analysis {
            n,
            ptr: a.ptr.clone(),
            col: a.col.clone(),
            perm,
            inv,
            parent,
            starts,
            super_rows,
            snode_of,
            seconds: t0.elapsed().as_secs_f64(),
        })
    }

    /// Whether `a` has the pattern this analysis is of.
    pub fn fits(&self, a: &Csr) -> bool {
        a.n == self.n && a.ptr == self.ptr && a.col == self.col
    }
}

/// What the threads of a factorization share: A's columns, the supernodes' first columns, the
/// pivot threshold, and the memory budget with the scratch file beyond it.
struct Shared<'a> {
    a_cols: &'a [Vec<(usize, C)>],
    starts: &'a [usize],
    tau: f64,
    budget: u64,
    in_memory: std::sync::atomic::AtomicU64,
    scratch: std::sync::Mutex<Option<Scratch>>,
}

/// One supernode factored: its node, its update for the parent, and counts.
struct Factored {
    node: Supernode,
    update: Option<(Vec<usize>, Vec<C>)>,
    m: usize,
    perturbed: usize,
    two_by_two: usize,
    entries: usize,
}

/// A subtree factored on a thread.
struct Subtree {
    root: usize,
    nodes: Vec<(usize, Supernode)>,
    update: Option<(Vec<usize>, Vec<C>)>,
    perturbed: usize,
    two_by_two: usize,
    entries: usize,
    largest_front: usize,
}

impl Shared<'_> {
    /// Factors the subtree of `root` (its supernodes in postorder with their rows), as the
    /// sequential factorization would.
    fn subtree(&self, root: usize, list: Vec<(usize, Vec<usize>)>, parent_s: &[Option<usize>]) -> Result<Subtree, FactorError> {
        let mut pending: std::collections::BTreeMap<usize, Vec<(Vec<usize>, Vec<C>)>> = std::collections::BTreeMap::new();
        let mut out =
            Subtree { root, nodes: Vec::with_capacity(list.len()), update: None, perturbed: 0, two_by_two: 0, entries: 0, largest_front: 0 };
        for (s, rows) in list {
            let updates = pending.remove(&s).unwrap_or_default();
            let f = self.node(s, rows, updates)?;
            out.perturbed += f.perturbed;
            out.two_by_two += f.two_by_two;
            out.entries += f.entries;
            out.largest_front = out.largest_front.max(f.m);
            if s == root {
                out.update = f.update;
            } else if let (Some(u), Some(p)) = (f.update, parent_s.get(s).copied().flatten()) {
                pending.entry(p).or_default().push(u);
            }
            out.nodes.push((s, f.node));
        }
        Ok(out)
    }

    /// Assembles supernode `s`'s front from A and its children's updates (in the children's
    /// order), factors its columns and keeps them (in memory, or on disk beyond the budget).
    fn node(&self, s: usize, mut rows: Vec<usize>, updates: Vec<(Vec<usize>, Vec<C>)>) -> Result<Factored, FactorError> {
        let (c0, c1) = (self.starts[s], self.starts[s + 1]);
        let k = c1 - c0;
        let m = rows.len();
        if m > MAX_FRONT {
            return Err(FactorError::FrontTooLarge { rows: m, limit: MAX_FRONT });
        }
        // Rows ascend: a position's place in the front by binary search.
        let at = |p: usize| rows.binary_search(&p).unwrap_or(usize::MAX);
        let mut f = vec![C::new(0.0, 0.0); m * m];
        for j in c0..c1 {
            let cj = j - c0;
            for &(i, v) in self.a_cols.get(j).map(Vec::as_slice).unwrap_or(&[]) {
                let r = at(i);
                if r >= m {
                    return Err(FactorError::Dimension("an entry outside its front".into()));
                }
                f[cj * m + r] += v;
                if r != cj {
                    f[r * m + cj] += v;
                }
            }
        }
        for (urows, u) in updates {
            // U is stored packed lower by columns (i ≥ j); positions in this front ascend with
            // its rows.
            let mu = urows.len();
            let place: Vec<usize> = urows.iter().map(|&p| at(p)).collect();
            if place.iter().any(|&r| r >= m) {
                return Err(FactorError::Dimension("an update outside its parent's front".into()));
            }
            for b in 0..mu {
                let cb = place[b];
                let start = packed(b, mu);
                for a in b..mu {
                    let v = u[start + a - b];
                    let ra = place[a];
                    f[cb * m + ra] += v;
                    if ra != cb {
                        f[ra * m + cb] += v;
                    }
                }
            }
        }
        let (mut perturbed, mut two_by_two) = (0, 0);
        let pivots = factor_front(&mut f, m, k, &mut rows, self.tau, &mut perturbed, &mut two_by_two);
        // The update matrix U = F₂₂ − L₂₁ D L₂₁ᵀ (lower) for the parent: the trailing block.
        let mu = m - k;
        let update = (mu > 0).then(|| {
            let mut u = Vec::with_capacity(mu * (mu + 1) / 2);
            for j in 0..mu {
                u.extend_from_slice(&f[(k + j) * m + k + j..(k + j + 1) * m]);
            }
            (rows[k..].to_vec(), u)
        });
        // Keep the k factored columns only, in an allocation of their own (the front's m × m is
        // freed).
        let l = f[..k * m].to_vec();
        drop(f);
        let bytes = (l.len() * 16) as u64;
        use std::sync::atomic::Ordering;
        // Beyond the memory budget the columns go to the scratch file.
        let fits =
            self.in_memory.try_update(Ordering::SeqCst, Ordering::SeqCst, |used| (used + bytes <= self.budget).then_some(used + bytes)).is_ok();
        let l = if fits {
            Columns::Memory(l)
        } else {
            let mut guard = self.scratch.lock().map_err(|_| FactorError::Dimension("the scratch file's lock was poisoned".into()))?;
            if guard.is_none() {
                *guard = Some(Scratch::new()?);
            }
            match guard.as_mut() {
                Some(sc) => sc.write(&l)?,
                None => Columns::Memory(l),
            }
        };
        Ok(Factored { node: Supernode { rows, k, l, pivots }, update, m, perturbed, two_by_two, entries: k * m - k * k.saturating_sub(1) / 2 })
    }
}

impl Ldlt {
    /// Factors `a` (complex symmetric, both triangles stored) with the unknowns ordered by
    /// nested dissection on `xyz` (a point per unknown).
    pub fn factor(a: &Csr, xyz: &[Point3]) -> Result<Ldlt, FactorError> {
        Ldlt::factor_within(a, xyz, memory_budget())
    }

    /// [`Ldlt::factor`] keeping at most `budget` bytes of the factor in memory, the rest in a
    /// scratch file on disk.
    pub fn factor_within(a: &Csr, xyz: &[Point3], budget: u64) -> Result<Ldlt, FactorError> {
        Ldlt::factor_bordered(a, xyz, budget, 0)
    }

    /// [`Ldlt::factor_within`] with the last `last` unknowns eliminated after all the others
    /// (a bordered matrix's border: unknowns each coupled to many).
    pub fn factor_bordered(a: &Csr, xyz: &[Point3], budget: u64, last: usize) -> Result<Ldlt, FactorError> {
        let analysis = Analysis::new(a, xyz, last)?;
        Ldlt::factor_analysed(a, &analysis, budget)
    }

    /// Factors `a` with an [`Analysis`] of its pattern made before (the same pattern: a sweep's
    /// frequencies share one), skipping the ordering and the symbolic factorization.
    pub fn factor_analysed(a: &Csr, an: &Analysis, budget: u64) -> Result<Ldlt, FactorError> {
        let t0 = std::time::Instant::now();
        let n = a.n;
        if !an.fits(a) {
            return Err(FactorError::Dimension("the analysis is of another pattern".into()));
        }
        if a.val.iter().any(|v| !v.re.is_finite() || !v.im.is_finite()) {
            return Err(FactorError::NotFinite);
        }
        if n > 0 && !(a.max_abs() > 0.0) {
            return Err(FactorError::Zero);
        }
        // Equilibrate: S A S with each row's largest entry about one, so the static-pivot
        // threshold below means the same for every unknown. (Graded meshes mix elements whose
        // matrices differ by many orders; against the largest entry of the whole matrix, the
        // threshold would replace the pivots of every small one.)
        let scale: Vec<f64> = (0..n)
            .map(|i| {
                let m = a.val[a.ptr[i]..a.ptr[i + 1]].iter().fold(0.0f64, |m, v| m.max(v.norm()));
                if m > 0.0 { 1.0 / m.sqrt() } else { 1.0 }
            })
            .collect();
        let mut scaled = a.clone();
        for i in 0..n {
            for k in scaled.ptr[i]..scaled.ptr[i + 1] {
                scaled.val[k] *= scale[i] * scale[scaled.col[k]];
            }
        }
        let a = &scaled;
        let norm = a.max_abs();
        let tau = f64::EPSILON.sqrt() * norm;

        let (perm, inv, parent) = (an.perm.clone(), &an.inv, &an.parent);
        let (starts, super_rows, snode_of) = (&an.starts, an.super_rows.clone(), &an.snode_of);
        let ns = super_rows.len();

        // Columns of A by position, lower part with the diagonal: (row position, value).
        let mut a_cols: Vec<Vec<(usize, C)>> = vec![Vec::new(); n];
        for u in 0..n {
            let pu = inv[u];
            for (&v, &x) in a.col[a.ptr[u]..a.ptr[u + 1]].iter().zip(&a.val[a.ptr[u]..a.ptr[u + 1]]) {
                let pv = inv[v];
                if pu >= pv {
                    a_cols[pv].push((pu, x));
                }
            }
        }

        // The supernodal tree: each supernode's parent (the supernode of its last column's etree
        // parent) and children, in postorder (children before parents, ascending).
        let parent_s: Vec<Option<usize>> = (0..ns).map(|s| parent.get(starts[s + 1] - 1).and_then(|&q| snode_of.get(q)).copied()).collect();
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); ns];
        for (s, p) in parent_s.iter().enumerate() {
            if let Some(p) = p {
                children[*p].push(s);
            }
        }
        // Subtrees light enough go to threads whole, each in postorder as alone (so each keeps
        // the memory of a sequential factorization); the heavy top, whose fronts are large, after
        // them in order, its fronts' products threaded inside. A subtree is the contiguous range
        // of its first descendant to itself.
        let mut first: Vec<usize> = (0..ns).collect();
        let mut work = vec![0.0f64; ns];
        for s in 0..ns {
            let (k, m) = ((starts[s + 1] - starts[s]) as f64, super_rows[s].len() as f64);
            work[s] += k * m * m;
            if let Some(p) = parent_s[s] {
                first[p] = first[p].min(first[s]);
                work[p] += work[s];
            }
        }
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let total: f64 = (0..ns).filter(|&s| parent_s[s].is_none()).map(|s| work[s]).sum();
        let light = total / (8 * threads) as f64;
        let (mut tasks, mut top): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
        let mut stack: Vec<usize> = (0..ns).filter(|&s| parent_s[s].is_none()).collect();
        while let Some(s) = stack.pop() {
            if work[s] <= light || children[s].is_empty() {
                tasks.push(s);
            } else {
                top.push(s);
                stack.extend(children[s].iter().copied());
            }
        }
        tasks.sort_unstable();
        top.sort_unstable();

        let shared =
            Shared { a_cols: &a_cols, starts, tau, budget, in_memory: std::sync::atomic::AtomicU64::new(0), scratch: std::sync::Mutex::new(None) };
        let mut nodes: Vec<Option<Supernode>> = (0..ns).map(|_| None).collect();
        let mut super_rows: Vec<Option<Vec<usize>>> = super_rows.into_iter().map(Some).collect();
        // Each task's subtree: its rows taken out for the thread.
        let jobs: Vec<(usize, Vec<(usize, Vec<usize>)>)> =
            tasks.iter().map(|&s| (s, (first[s]..=s).map(|t| (t, super_rows[t].take().unwrap_or_default())).collect())).collect();
        let done: Vec<Result<Subtree, FactorError>> = jobs.into_par_iter().map(|(root, list)| shared.subtree(root, list, &parent_s)).collect();
        // Updates waiting for a top supernode: (child, rows, values).
        let mut pending: Vec<Vec<(usize, Vec<usize>, Vec<C>)>> = vec![Vec::new(); ns];
        let (mut perturbed, mut two_by_two, mut entries, mut largest_front) = (0, 0, 0, 0);
        for d in done {
            let d = d?;
            perturbed += d.perturbed;
            two_by_two += d.two_by_two;
            entries += d.entries;
            largest_front = largest_front.max(d.largest_front);
            for (s, node) in d.nodes {
                nodes[s] = Some(node);
            }
            if let (Some((rows, u)), Some(p)) = (d.update, parent_s[d.root]) {
                pending[p].push((d.root, rows, u));
            }
        }
        for s in top {
            let rows = super_rows[s].take().unwrap_or_default();
            let mut from_children = std::mem::take(&mut pending[s]);
            from_children.sort_by_key(|c| c.0);
            let updates = from_children.into_iter().map(|(_, r, u)| (r, u)).collect();
            let f = shared.node(s, rows, updates)?;
            perturbed += f.perturbed;
            two_by_two += f.two_by_two;
            entries += f.entries;
            largest_front = largest_front.max(f.m);
            if let (Some(u), Some(p)) = (f.update, parent_s[s]) {
                pending[p].push((s, u.0, u.1));
            }
            nodes[s] = Some(f.node);
        }
        let supernodes =
            nodes.into_iter().map(|n| n.ok_or(FactorError::Dimension("a supernode was left unfactored".into()))).collect::<Result<Vec<_>, _>>()?;
        let scratch = shared.scratch.into_inner().map_err(|_| FactorError::Dimension("the scratch file's lock was poisoned".into()))?;
        let bytes_on_disk = scratch.as_ref().map_or(0, |s| s.end);
        Ok(Ldlt {
            numeric_seconds: t0.elapsed().as_secs_f64(),
            analysis_seconds: an.seconds,
            n,
            perm,
            supernodes,
            scratch: scratch.map(std::sync::Arc::new),
            bytes_on_disk,
            scale,
            perturbed,
            two_by_two,
            entries,
            largest_front,
        })
    }

    pub fn n(&self) -> usize {
        self.n
    }

    /// Solves A x = b in place.
    pub fn solve(&self, b: &mut [C]) -> Result<(), FactorError> {
        let mut one = [b.to_vec()];
        self.solve_many(&mut one)?;
        b.copy_from_slice(&one[0]);
        Ok(())
    }

    /// A supernode's factored columns, from memory or read back from the scratch file.
    fn columns<'a>(&self, s: &'a Supernode) -> Result<std::borrow::Cow<'a, [C]>, FactorError> {
        match &s.l {
            Columns::Memory(v) => Ok(std::borrow::Cow::Borrowed(v)),
            Columns::Disk { offset, len } => {
                let sc = self.scratch.as_ref().ok_or_else(|| FactorError::Scratch("columns on disk without a file".into()))?;
                Ok(std::borrow::Cow::Owned(sc.read(*offset, *len)?))
            }
        }
    }

    /// Solves A X = B in place for several right-hand sides at once: each pass over the factor
    /// serves them all (the solve reads the whole factor and does little with each entry). Each
    /// column's arithmetic is the single solve's, in the same order.
    pub fn solve_many(&self, bs: &mut [Vec<C>]) -> Result<(), FactorError> {
        let r = bs.len();
        if r == 0 {
            return Ok(());
        }
        if bs.iter().any(|b| b.len() != self.n) {
            return Err(FactorError::Dimension(format!("right-hand sides for {} unknowns", self.n)));
        }
        // y[position · r + j].
        let mut y = vec![C::new(0.0, 0.0); self.n * r];
        for (pos, &u) in self.perm.iter().enumerate() {
            for (j, b) in bs.iter().enumerate() {
                y[pos * r + j] = b[u] * self.scale[u];
            }
        }
        let zero = C::new(0.0, 0.0);
        let mut local: Vec<C> = Vec::new();
        // L z = b.
        for s in &self.supernodes {
            let cols = self.columns(s)?;
            let m = s.rows.len();
            local.clear();
            for &row in &s.rows {
                local.extend_from_slice(&y[row * r..row * r + r]);
            }
            let mut p = 0;
            while p < s.k {
                let width = if matches!(s.pivots[p], Pivot::Two(..)) { 2 } else { 1 };
                for c in p..p + width {
                    let xc: Vec<C> = local[c * r..c * r + r].to_vec();
                    if xc.iter().all(|v| *v == zero) {
                        continue;
                    }
                    for i in p + width..m {
                        let l = cols[c * m + i];
                        if l == zero {
                            continue;
                        }
                        for (t, x) in local[i * r..i * r + r].iter_mut().zip(&xc) {
                            *t -= l * x;
                        }
                    }
                }
                p += width;
            }
            for (k, &row) in s.rows.iter().enumerate() {
                y[row * r..row * r + r].copy_from_slice(&local[k * r..k * r + r]);
            }
        }
        // D w = z.
        for s in &self.supernodes {
            let mut p = 0;
            while p < s.k {
                match s.pivots[p] {
                    Pivot::One(d) => {
                        for v in &mut y[s.rows[p] * r..s.rows[p] * r + r] {
                            *v /= d;
                        }
                        p += 1;
                    }
                    Pivot::Two(a11, a21, a22) => {
                        let (r0, r1) = (s.rows[p], s.rows[p + 1]);
                        let det = a11 * a22 - a21 * a21;
                        for j in 0..r {
                            let (b0, b1) = (y[r0 * r + j], y[r1 * r + j]);
                            y[r0 * r + j] = (a22 * b0 - a21 * b1) / det;
                            y[r1 * r + j] = (a11 * b1 - a21 * b0) / det;
                        }
                        p += 2;
                    }
                    Pivot::Second => p += 1,
                }
            }
        }
        // Lᵀ x = w.
        let mut acc = vec![zero; r];
        for s in self.supernodes.iter().rev() {
            let cols = self.columns(s)?;
            let m = s.rows.len();
            local.clear();
            for &row in &s.rows {
                local.extend_from_slice(&y[row * r..row * r + r]);
            }
            let mut p = s.k;
            while p > 0 {
                let width = if p >= 2 && matches!(s.pivots[p - 2], Pivot::Two(..)) { 2 } else { 1 };
                let start = p - width;
                for c in start..p {
                    acc.iter_mut().for_each(|a| *a = zero);
                    for i in p..m {
                        let l = cols[c * m + i];
                        for (a, x) in acc.iter_mut().zip(&local[i * r..i * r + r]) {
                            *a += l * x;
                        }
                    }
                    for (t, a) in local[c * r..c * r + r].iter_mut().zip(&acc) {
                        *t -= a;
                    }
                }
                p = start;
            }
            for (k, &row) in s.rows.iter().enumerate().take(s.k) {
                y[row * r..row * r + r].copy_from_slice(&local[k * r..k * r + r]);
            }
        }
        for (pos, &u) in self.perm.iter().enumerate() {
            for (j, b) in bs.iter_mut().enumerate() {
                b[u] = y[pos * r + j] * self.scale[u];
            }
        }
        Ok(())
    }

    /// Solves A x = b with iterative refinement until the backward error
    /// ‖b − A x‖∞ / (‖A‖ ‖x‖∞ + ‖b‖∞) stops falling (at most `steps` corrections); returns x and
    /// that error.
    pub fn solve_refined(&self, a: &Csr, b: &[C], steps: usize) -> Result<(Vec<C>, f64), FactorError> {
        let mut x = b.to_vec();
        self.solve(&mut x)?;
        let norm_a = a.max_abs();
        let inf = |v: &[C]| v.iter().fold(0.0f64, |m, z| m.max(z.norm()));
        let berr = |x: &[C]| -> (Vec<C>, f64) {
            let ax = a.mul(x);
            let r: Vec<C> = b.iter().zip(&ax).map(|(b, a)| b - a).collect();
            let e = inf(&r) / (norm_a * inf(x) + inf(b)).max(f64::MIN_POSITIVE);
            (r, e)
        };
        let (mut r, mut err) = berr(&x);
        for _ in 0..steps.min(20) {
            if err < 4.0 * f64::EPSILON {
                break;
            }
            self.solve(&mut r)?;
            let trial: Vec<C> = x.iter().zip(&r).map(|(x, d)| x + d).collect();
            let (r2, e2) = berr(&trial);
            if !(e2 < err) {
                break;
            }
            x = trial;
            r = r2;
            err = e2;
        }
        Ok((x, err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic pseudo-random sequence (xorshift).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        }
        fn c(&mut self) -> C {
            C::new(self.next(), self.next())
        }
    }

    /// A random complex symmetric matrix on a 3D grid graph (7-point stencil), points per node.
    fn grid_matrix(nx: usize, ny: usize, nz: usize, diag: C, rng: &mut Rng) -> (Csr, Vec<Point3>) {
        let id = |i: usize, j: usize, k: usize| i + nx * (j + ny * k);
        let n = nx * ny * nz;
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut xyz = vec![Point3::ORIGIN; n];
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    xyz[id(i, j, k)] = Point3::new(i as f64, j as f64, k as f64);
                    if i + 1 < nx {
                        groups.push(vec![id(i, j, k), id(i + 1, j, k)]);
                    }
                    if j + 1 < ny {
                        groups.push(vec![id(i, j, k), id(i, j + 1, k)]);
                    }
                    if k + 1 < nz {
                        groups.push(vec![id(i, j, k), id(i, j, k + 1)]);
                    }
                }
            }
        }
        let mut a = Csr::from_groups(n, groups.iter().map(|g| g.as_slice()));
        for g in &groups {
            let v = rng.c();
            a.add(g[0], g[1], v);
            a.add(g[1], g[0], v);
        }
        for i in 0..n {
            let v = diag + rng.c() * 0.1;
            a.add(i, i, v);
        }
        (a, xyz)
    }

    fn residual(a: &Csr, x: &[C], b: &[C]) -> f64 {
        let ax = a.mul(x);
        let r = b.iter().zip(&ax).fold(0.0f64, |m, (b, a)| m.max((b - a).norm()));
        let xn = x.iter().fold(0.0f64, |m, v| m.max(v.norm()));
        r / (a.max_abs() * xn)
    }

    #[test]
    fn solves_random_complex_symmetric_systems() {
        let mut rng = Rng(0x9e3779b97f4a7c15);
        for (diag, label) in [(C::new(4.0, 0.0), "dominant"), (C::new(0.0, 0.0), "indefinite"), (C::new(-1.0, 0.3), "shifted")] {
            let (a, xyz) = grid_matrix(7, 5, 6, diag, &mut rng);
            let f = Ldlt::factor(&a, &xyz).unwrap();
            let x_true: Vec<C> = (0..a.n).map(|_| rng.c()).collect();
            let b = a.mul(&x_true);
            let mut x = b.clone();
            f.solve(&mut x).unwrap();
            assert!(residual(&a, &x, &b) < 1e-11, "{label}: {}", residual(&a, &x, &b));
            let (xr, berr) = f.solve_refined(&a, &b, 5).unwrap();
            assert!(berr < 1e-14, "{label}: {berr}");
            let err = xr.iter().zip(&x_true).fold(0.0f64, |m, (p, q)| m.max((p - q).norm()));
            assert!(err < 1e-8, "{label}: {err}");
        }
    }

    #[test]
    fn needs_two_by_two_pivots() {
        // A zero diagonal with the largest entries inside one supernode: [[0, 1], [1, 0]] blocks
        // coupled densely, all in one front.
        let n = 6;
        let groups = [(0..n).collect::<Vec<_>>()];
        let mut a = Csr::from_groups(n, groups.iter().map(|g| g.as_slice()));
        let mut rng = Rng(7);
        for i in 0..n {
            for j in i + 1..n {
                let v = if j == i + 1 && i % 2 == 0 { C::new(5.0, 1.0) } else { rng.c() * 0.2 };
                a.add(i, j, v);
                a.add(j, i, v);
            }
        }
        let xyz: Vec<Point3> = (0..n).map(|i| Point3::new(i as f64, 0.0, 0.0)).collect();
        let f = Ldlt::factor(&a, &xyz).unwrap();
        assert!(f.two_by_two > 0);
        assert_eq!(f.perturbed, 0);
        let b: Vec<C> = (0..n).map(|i| C::new(i as f64, 1.0)).collect();
        let mut x = b.clone();
        f.solve(&mut x).unwrap();
        assert!(residual(&a, &x, &b) < 1e-13);
    }

    #[test]
    fn deterministic_bit_for_bit() {
        let mut r1 = Rng(42);
        let (a, xyz) = grid_matrix(6, 6, 6, C::new(0.5, 0.0), &mut r1);
        let b: Vec<C> = (0..a.n).map(|i| C::new((i % 7) as f64, 1.0)).collect();
        let run = || {
            let mut x = b.clone();
            Ldlt::factor(&a, &xyz).unwrap().solve(&mut x).unwrap();
            x
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn badly_scaled_matrices_keep_their_pivots() {
        // Two blocks of a graded mesh's sizes, 1e12 apart: against the whole matrix's largest
        // entry, √ε‖A‖ would replace every pivot of the small one; equilibrated, none.
        let n = 40;
        let groups: Vec<Vec<usize>> = (0..n - 1).map(|i| vec![i, i + 1]).collect();
        let mut a = Csr::from_groups(n, groups.iter().map(|g| g.as_slice()));
        for i in 0..n {
            let s = if i < n / 2 { 1e12 } else { 1.0 };
            a.add(i, i, C::new(4.0 * s, 0.3));
            if i + 1 < n {
                let s = if i + 1 < n / 2 { 1e12 } else { 1.0 };
                a.add(i, i + 1, C::new(-s, 0.0));
                a.add(i + 1, i, C::new(-s, 0.0));
            }
        }
        let xyz: Vec<Point3> = (0..n).map(|i| Point3::new(i as f64, 0.0, 0.0)).collect();
        let f = Ldlt::factor(&a, &xyz).unwrap();
        assert_eq!(f.perturbed, 0);
        let x: Vec<C> = (0..n).map(|i| C::new(1.0 + i as f64, -0.5)).collect();
        let mut b = a.mul(&x);
        f.solve(&mut b).unwrap();
        let err = b.iter().zip(&x).map(|(p, q)| (p - q).norm()).fold(0.0, f64::max);
        assert!(err < 1e-8, "{err:e}");
    }

    #[test]
    fn hostile_matrices_are_refused() {
        let groups = [vec![0, 1]];
        let mut a = Csr::from_groups(2, groups.iter().map(|g| g.as_slice()));
        let xyz = [Point3::ORIGIN, Point3::new(1.0, 0.0, 0.0)];
        assert_eq!(Ldlt::factor(&a, &xyz).unwrap_err(), FactorError::Zero);
        a.add(0, 0, C::new(f64::NAN, 0.0));
        assert_eq!(Ldlt::factor(&a, &xyz).unwrap_err(), FactorError::NotFinite);
        assert!(matches!(Ldlt::factor(&a, &xyz[..1]), Err(FactorError::Dimension(_))));
    }

    #[test]
    fn a_factor_on_disk_solves_the_same() {
        // Out of core from the first column: the same solution bit for bit.
        let mut rng = Rng(7);
        let (a, xyz) = grid_matrix(6, 5, 4, C::new(0.3, 0.1), &mut rng);
        let b: Vec<C> = (0..a.n).map(|i| C::new((i % 5) as f64, -1.0)).collect();
        let mem = Ldlt::factor_within(&a, &xyz, u64::MAX).unwrap();
        let disk = Ldlt::factor_within(&a, &xyz, 0).unwrap();
        assert_eq!(mem.bytes_on_disk, 0);
        assert_eq!(disk.bytes_on_disk, 16 * disk.supernodes.iter().map(|s| s.k * s.rows.len()).sum::<usize>() as u64);
        let (mut x, mut y) = (b.clone(), b.clone());
        mem.solve(&mut x).unwrap();
        disk.solve(&mut y).unwrap();
        assert_eq!(x, y);
        // Clones share the file.
        let again = disk.clone();
        let mut z = b.clone();
        again.solve(&mut z).unwrap();
        assert_eq!(x, z);
    }

    #[test]
    #[ignore = "benchmark: cargo test --release -p emcraft-solver --lib dense_front_rate -- --ignored --nocapture"]
    fn dense_front_rate() {
        // A dense complex symmetric matrix, one front: the factorization's arithmetic rate.
        for n in [800usize, 1600] {
            let all: Vec<usize> = (0..n).collect();
            let mut a = Csr::from_groups(n, std::iter::once(all.as_slice()));
            let mut rng = Rng(0x1234_5678);
            for i in 0..n {
                for j in 0..=i {
                    let v = C::new(rng.next(), rng.next());
                    let v = if i == j { v + C::new(n as f64, 0.0) } else { v };
                    a.add(i, j, v);
                    if i != j {
                        a.add(j, i, v);
                    }
                }
            }
            let xyz: Vec<Point3> = (0..n).map(|i| Point3::new(i as f64, 0.0, 0.0)).collect();
            let t = std::time::Instant::now();
            let f = Ldlt::factor(&a, &xyz).unwrap();
            let s = t.elapsed().as_secs_f64();
            // n³/3 complex multiply–adds of 8 real flops each.
            let gflops = 8.0 * (n as f64).powi(3) / 3.0 / s / 1e9;
            eprintln!("n {n}: {s:.3} s, {gflops:.1} GFLOP/s, front {}", f.largest_front);
        }
    }
}

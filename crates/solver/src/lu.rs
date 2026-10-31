//! Sparse LU factorization P A = L U of a complex matrix whose pattern is symmetric but whose
//! values are not, by the multifrontal method: the unsymmetric systems of periodic (Floquet)
//! problems, whose phase-shifted boundary conditions make A Hermitian-like rather than complex
//! symmetric ([`crate::driven::periodic`]).
//!
//! **Structure.** The pattern being symmetric, the machinery the LDLᵀ ([`crate::ldlt`]) uses
//! serves unchanged ([`crate::multifrontal`]): its [`Analysis`] (nested dissection, the
//! elimination tree's postorder, relaxed supernodes) gives the fronts, each assembled from A's
//! rows and columns of its supernode and the update matrices of its children (extend-add), its
//! supernode's k variables eliminated and the Schur complement passed to the parent (I. S. Duff,
//! J. K. Reid, *The multifrontal solution of unsymmetric sets of linear equations*, SIAM J. Sci.
//! Stat. Comput. 5(3):633–641, 1984). The update matrices are passed full.
//!
//! **Pivoting.** Within a front, the k fully summed rows may be interchanged freely without
//! changing any structure: column j's pivot is its largest entry among the fully summed rows
//! j..k (partial pivoting restricted to the front's fully summed block, as in Duff & Reid). A
//! pivot below √ε ‖A‖ (the matrix equilibrated first, rows then columns to a largest entry of
//! one) is replaced by that value — static pivoting (X. S. Li, J. W. Demmel, *Making sparse
//! Gaussian elimination scalable by static pivoting*, Proc. SC98, 1998) — and
//! [`Lu::solve_refined`] recovers the accuracy by iterative refinement, reporting the backward
//! error.
//!
//! **Blocks.** Each front is factored in blocks of [`BLOCK`] columns: the block's panel by
//! rank-one updates, the block's rows of U by a unit lower triangular solve, the trailing
//! matrix by one product (faer), whose sums run in an order that does not depend on the threads,
//! so the factor is the same bit for bit on any machine. The supernodes are factored by the
//! multifrontal driver (light subtrees on threads, each front's updates summed in its children's
//! order); the factor is kept in memory (its size is checked against the memory budget first).

use std::ops::Range;

use faer::linalg::matmul::matmul;
use faer::{Accum, MatMut, MatRef, Par};
use num_complex::Complex64 as C;

use crate::multifrontal::{
    Analysis, Counts, Eliminated, FactorError, Front, FrontKernel, MAX_FRONT, Update, check, factor_tree, refine, refine_many,
};
use crate::sparse::Csr;

/// Pivot columns per block.
const BLOCK: usize = 64;

/// Fronts whose trailing update is smaller than this many multiply–adds run on one thread.
const PARALLEL_WORK: usize = 1 << 20;

/// One supernode's part of the factor.
#[derive(Clone, Debug)]
struct Node {
    /// Positions of the front's columns (and of its contribution rows); the first `k` are the
    /// supernode's variables in pivot order.
    rows: Vec<usize>,
    /// The position of the equation each of the first `k` front rows holds after the
    /// interchanges.
    eqs: Vec<usize>,
    k: usize,
    /// The front's first k columns (m × k, by column): L₁₁ strictly below the diagonal, U₁₁ on and
    /// above it, L₂₁ below.
    l: Vec<C>,
    /// U₁₂: the first k rows of the front's columns k.. (k × (m − k), by column).
    u: Vec<C>,
}

/// A factorization P R A C = L U (R, C the equilibrating scalings).
#[derive(Clone, Debug)]
pub struct Lu {
    n: usize,
    /// `perm[position] = unknown`.
    perm: Vec<usize>,
    nodes: Vec<Node>,
    row_scale: Vec<f64>,
    col_scale: Vec<f64>,
    /// Pivots replaced by √ε ‖R A C‖.
    pub perturbed: usize,
    /// Entries of L and U.
    pub entries: usize,
    pub largest_front: usize,
    pub analysis_seconds: f64,
    pub numeric_seconds: f64,
}

/// Equilibration: each row's largest entry made one, then each column's.
fn scalings(a: &Csr) -> (Vec<f64>, Vec<f64>) {
    let n = a.n;
    let row: Vec<f64> = (0..n)
        .map(|i| {
            let m = a.val[a.row(i)].iter().fold(0.0f64, |m, v| m.max(v.norm()));
            if m > 0.0 { 1.0 / m } else { 1.0 }
        })
        .collect();
    let mut colmax = vec![0.0f64; n];
    for i in 0..n {
        for k in a.row(i) {
            let j = a.col[k];
            colmax[j] = colmax[j].max(a.val[k].norm() * row[i]);
        }
    }
    let col = colmax.into_iter().map(|m| if m > 0.0 { 1.0 / m } else { 1.0 }).collect();
    (row, col)
}

/// Eliminates the first `k` columns of a front (column-major m × m) in blocks of [`BLOCK`],
/// interchanging rows among the first k (`eqs` follows them). On return the first k columns
/// hold L and U₁₁, rows 0..k of the rest U₁₂, and the trailing block the Schur complement.
fn factor_front(f: &mut [C], m: usize, k: usize, eqs: &mut [usize], tau: f64, perturbed: &mut usize) {
    let mut b0 = 0;
    while b0 < k {
        let b1 = (b0 + BLOCK).min(k);
        factor_panel(f, m, k, b0..b1, eqs, tau, perturbed);
        if b1 == m {
            break;
        }
        solve_u_rows(f, m, b0..b1);
        update_trailing(f, m, b0..b1);
        b0 = b1;
    }
}

/// The panel: columns `block`, every row from its first, by rank-one updates, each column's
/// pivot the largest among the fully summed rows j..k.
fn factor_panel(f: &mut [C], m: usize, k: usize, block: Range<usize>, eqs: &mut [usize], tau: f64, perturbed: &mut usize) {
    let zero = C::new(0.0, 0.0);
    let b1 = block.end;
    for j in block {
        let mut p = j;
        let mut best = f[j * m + j].norm();
        for i in j + 1..k {
            let v = f[j * m + i].norm();
            if v > best {
                best = v;
                p = i;
            }
        }
        if best < tau {
            let d = f[j * m + j];
            f[j * m + j] = if d.norm() > 0.0 { d / d.norm() * tau } else { C::new(tau, 0.0) };
            *perturbed += 1;
        } else if p != j {
            for c in 0..m {
                f.swap(c * m + j, c * m + p);
            }
            eqs.swap(j, p);
        }
        let inv = C::new(1.0, 0.0) / f[j * m + j];
        for i in j + 1..m {
            f[j * m + i] *= inv;
        }
        // Rank-one update of the panel's later columns.
        for c in j + 1..b1 {
            let u = f[c * m + j];
            if u == zero {
                continue;
            }
            for i in j + 1..m {
                let l = f[j * m + i];
                f[c * m + i] -= l * u;
            }
        }
    }
}

/// U's block rows over the later columns: L₁₁⁻¹ (unit lower) applied to rows `block`.
fn solve_u_rows(f: &mut [C], m: usize, block: Range<usize>) {
    let zero = C::new(0.0, 0.0);
    let (b0, b1) = (block.start, block.end);
    for c in b1..m {
        for j in b0..b1 {
            let u = f[c * m + j];
            if u == zero {
                continue;
            }
            for i in j + 1..b1 {
                let l = f[j * m + i];
                f[c * m + i] -= l * u;
            }
        }
    }
}

/// The trailing matrix: F[b1.., b1..] −= L[b1.., block] U[block, b1..], by one product.
fn update_trailing(f: &mut [C], m: usize, block: Range<usize>) {
    let (b0, b1) = (block.start, block.end);
    let nb = b1 - b0;
    let below = m - b1;
    let mut ublock = vec![C::new(0.0, 0.0); nb * below];
    for c in 0..below {
        ublock[c * nb..(c + 1) * nb].copy_from_slice(&f[(b1 + c) * m + b0..(b1 + c) * m + b1]);
    }
    let (left, right) = f.split_at_mut(b1 * m);
    let lm = MatRef::from_column_major_slice(&left[b0 * m..], m, nb).subrows(b1, below);
    let um = MatRef::from_column_major_slice(&ublock, nb, below);
    let fm = MatMut::from_column_major_slice_mut(right, m, below);
    let par = if below * below * nb < PARALLEL_WORK { Par::Seq } else { Par::rayon(0) };
    matmul(fm.submatrix_mut(b1, 0, below, below), Accum::Add, lm, um, C::new(-1.0, 0.0), par);
}

/// R A C's entries by position (`inv[unknown] = position`): column j holds (row position,
/// value) for rows ≥ j, row i holds (column position, value) for columns > i; and the largest
/// |entry|.
fn by_position(a: &Csr, inv: &[usize], row_scale: &[f64], col_scale: &[f64]) -> (Vec<Vec<(usize, C)>>, Vec<Vec<(usize, C)>>, f64) {
    let n = a.n;
    let mut lower: Vec<Vec<(usize, C)>> = vec![Vec::new(); n];
    let mut upper: Vec<Vec<(usize, C)>> = vec![Vec::new(); n];
    let mut norm: f64 = 0.0;
    for u in 0..n {
        let pu = inv[u];
        for k in a.row(u) {
            let v = a.col[k];
            let pv = inv[v];
            let x = a.val[k] * (row_scale[u] * col_scale[v]);
            norm = norm.max(x.norm());
            if pu >= pv {
                lower[pv].push((pu, x));
            } else {
                upper[pu].push((pv, x));
            }
        }
    }
    (lower, upper, norm)
}

/// Refuses a factor that would not fit in `budget` bytes (both triangles of every front's
/// columns), or a front beyond [`MAX_FRONT`], before any of it is made.
fn check_size(an: &Analysis, budget: u64) -> Result<(), FactorError> {
    let mut bytes: u64 = 0;
    for s in 0..an.supernodes() {
        let (k, m) = (an.columns(s).len() as u64, an.super_rows[s].len() as u64);
        if m as usize > MAX_FRONT {
            return Err(FactorError::FrontTooLarge { rows: m as usize, limit: MAX_FRONT });
        }
        bytes = bytes.saturating_add(16 * (2 * k * m).saturating_sub(k * k));
    }
    if bytes > budget {
        return Err(FactorError::Scratch(format!(
            "the LU factor needs {:.1} GB, beyond the {:.1} GB budget (the unsymmetric factor is kept in memory)",
            bytes as f64 / 1e9,
            budget as f64 / 1e9
        )));
    }
    Ok(())
}

/// The LU's fronts: R A C by position and the static-pivot threshold.
struct Kernel {
    lower: Vec<Vec<(usize, C)>>,
    upper: Vec<Vec<(usize, C)>>,
    tau: f64,
}

impl FrontKernel for Kernel {
    type Node = Node;

    fn assemble(&self, columns: Range<usize>, front: &mut Front) -> Result<(), FactorError> {
        let m = front.m();
        for j in columns.clone() {
            let cj = j - columns.start;
            for &(i, v) in self.lower.get(j).map(Vec::as_slice).unwrap_or(&[]) {
                let r = front.place_entry(i)?;
                front.f[cj * m + r] += v;
            }
            for &(c, v) in self.upper.get(j).map(Vec::as_slice).unwrap_or(&[]) {
                let r = front.place_entry(c)?;
                front.f[r * m + cj] += v;
            }
        }
        Ok(())
    }

    /// The update is full (m_u × m_u, by column).
    fn extend_add(&self, f: &mut [C], m: usize, place: &[usize], u: &[C]) {
        for (&cb, column) in place.iter().zip(u.chunks_exact(place.len().max(1))) {
            for (&ra, &v) in place.iter().zip(column) {
                f[cb * m + ra] += v;
            }
        }
    }

    fn eliminate(&self, front: Front) -> Result<Eliminated<Node>, FactorError> {
        let Front { rows, k, mut f } = front;
        let m = rows.len();
        let mut eqs: Vec<usize> = rows[..k].to_vec();
        let mut perturbed = 0;
        factor_front(&mut f, m, k, &mut eqs, self.tau, &mut perturbed);
        let mu = m - k;
        let update = (mu > 0).then(|| {
            let mut u = Vec::with_capacity(mu * mu);
            for j in 0..mu {
                u.extend_from_slice(&f[(k + j) * m + k..(k + j + 1) * m]);
            }
            Update { rows: rows[k..].to_vec(), values: u }
        });
        let mut upart = Vec::with_capacity(k * mu);
        for j in 0..mu {
            upart.extend_from_slice(&f[(k + j) * m..(k + j) * m + k]);
        }
        f.truncate(k * m);
        let counts = Counts { perturbed, two_by_two: 0, entries: k * m + k * mu, largest_front: 0 };
        Ok(Eliminated { node: Node { rows, eqs, k, l: f, u: upart }, update, counts })
    }
}

impl Lu {
    /// Factors `a` (its pattern symmetric) with an [`Analysis`] of that pattern; `budget`: the
    /// most bytes the factor may take.
    pub fn factor_analysed(a: &Csr, an: &Analysis, budget: u64) -> Result<Lu, FactorError> {
        let t0 = std::time::Instant::now();
        check(a, an)?;
        check_size(an, budget)?;
        let (row_scale, col_scale) = scalings(a);
        let (lower, upper, norm) = by_position(a, &an.inv, &row_scale, &col_scale);
        let kernel = Kernel { lower, upper, tau: f64::EPSILON.sqrt() * norm };
        let (nodes, counts) = factor_tree(&kernel, an)?;
        Ok(Lu {
            n: a.n,
            perm: an.perm.clone(),
            nodes,
            row_scale,
            col_scale,
            perturbed: counts.perturbed,
            entries: counts.entries,
            largest_front: counts.largest_front,
            analysis_seconds: an.seconds,
            numeric_seconds: t0.elapsed().as_secs_f64(),
        })
    }

    pub fn n(&self) -> usize {
        self.n
    }

    /// Solves A x = b in place.
    pub fn solve(&self, b: &mut [C]) -> Result<(), FactorError> {
        if b.len() != self.n {
            return Err(FactorError::Dimension(format!("a right-hand side for {} unknowns", self.n)));
        }
        let zero = C::new(0.0, 0.0);
        // y by position: the equations, scaled by R.
        let mut y = vec![zero; self.n];
        for (pos, &u) in self.perm.iter().enumerate() {
            y[pos] = b[u] * self.row_scale[u];
        }
        let mut t: Vec<C> = Vec::new();
        // L z = P y: each front's equations in pivot order.
        for nd in &self.nodes {
            let (m, k) = (nd.rows.len(), nd.k);
            t.clear();
            t.extend(nd.eqs.iter().map(|&e| y[e]));
            t.extend(nd.rows[k..].iter().map(|&r| y[r]));
            for j in 0..k {
                let x = t[j];
                if x == zero {
                    continue;
                }
                for i in j + 1..m {
                    t[i] -= nd.l[j * m + i] * x;
                }
            }
            for (i, &r) in nd.rows.iter().enumerate() {
                y[r] = t[i];
            }
        }
        // U x = z, from the last front back.
        for nd in self.nodes.iter().rev() {
            let (m, k) = (nd.rows.len(), nd.k);
            for j in (0..k).rev() {
                let mut acc = y[nd.rows[j]];
                for c in j + 1..k {
                    acc -= nd.l[c * m + j] * y[nd.rows[c]];
                }
                for c in k..m {
                    acc -= nd.u[(c - k) * k + j] * y[nd.rows[c]];
                }
                y[nd.rows[j]] = acc / nd.l[j * m + j];
            }
        }
        for (pos, &u) in self.perm.iter().enumerate() {
            b[u] = y[pos] * self.col_scale[u];
        }
        Ok(())
    }

    /// Solves A x = b with iterative refinement until the backward error
    /// ‖b − A x‖∞ / (‖A‖ ‖x‖∞ + ‖b‖∞) stops falling (at most `steps` corrections); returns x and
    /// that error.
    pub fn solve_refined(&self, a: &Csr, b: &[C], steps: usize) -> Result<(Vec<C>, f64), FactorError> {
        refine(a, b, steps, |x| self.solve(x))
    }

    /// [`Lu::solve_refined`] for several right-hand sides, each the same bits as its own (the LU
    /// solves them one at a time).
    pub fn solve_refined_many(&self, a: &Csr, bs: &[Vec<C>], steps: usize) -> Result<Vec<(Vec<C>, f64)>, FactorError> {
        refine_many(a, bs, steps, |xs| xs.iter_mut().try_for_each(|x| self.solve(x)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multifrontal::testing::{Rng, grid_pattern};
    use emcraft_model::Point3;

    /// An unsymmetric matrix on a 3D grid graph's symmetric pattern.
    fn grid_matrix(nx: usize, ny: usize, nz: usize, diag: C, rng: &mut Rng) -> (Csr, Vec<Point3>) {
        let (mut a, edges, xyz) = grid_pattern(nx, ny, nz);
        for g in &edges {
            a.add(g[0], g[1], rng.c());
            a.add(g[1], g[0], rng.c());
        }
        for i in 0..a.n {
            a.add(i, i, diag + rng.c() * 0.1);
        }
        (a, xyz)
    }

    #[test]
    fn solves_random_unsymmetric_systems() {
        let mut rng = Rng(0x2545f4914f6cdd1d);
        for (diag, label) in [(C::new(4.0, 0.0), "dominant"), (C::new(0.0, 0.0), "zero diagonal"), (C::new(-1.0, 0.3), "shifted")] {
            let (a, xyz) = grid_matrix(9, 6, 7, diag, &mut rng);
            let an = Analysis::new(&a, &xyz, 0).unwrap();
            let f = Lu::factor_analysed(&a, &an, u64::MAX).unwrap();
            let x_true: Vec<C> = (0..a.n).map(|_| rng.c()).collect();
            let b = a.mul(&x_true);
            let (x, berr) = f.solve_refined(&a, &b, 5).unwrap();
            assert!(berr < 1e-14, "{label}: {berr}");
            let err = x.iter().zip(&x_true).fold(0.0f64, |m, (p, q)| m.max((p - q).norm()));
            assert!(err < 1e-8, "{label}: {err}");
        }
    }

    #[test]
    fn needs_row_interchanges() {
        // A zero diagonal with the large entries off it: [[0, 5], [3, 0]] blocks in one front.
        let n = 6;
        let groups = [(0..n).collect::<Vec<_>>()];
        let mut a = Csr::from_groups(n, groups.iter().map(|g| g.as_slice()));
        let mut rng = Rng(11);
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                let big = (i / 2 == j / 2) as u8 as f64;
                a.add(i, j, C::new(5.0 * big, 1.0 * big) + rng.c() * 0.2);
            }
        }
        let xyz: Vec<Point3> = (0..n).map(|i| Point3::new(i as f64, 0.0, 0.0)).collect();
        let an = Analysis::new(&a, &xyz, 0).unwrap();
        let f = Lu::factor_analysed(&a, &an, u64::MAX).unwrap();
        assert_eq!(f.perturbed, 0);
        let b: Vec<C> = (0..n).map(|i| C::new(i as f64, 1.0)).collect();
        let (x, berr) = f.solve_refined(&a, &b, 3).unwrap();
        assert!(berr < 1e-15, "{berr}");
        assert!(x.iter().all(|v| v.norm().is_finite()));
    }

    #[test]
    fn refuses_a_factor_beyond_the_budget() {
        let mut rng = Rng(3);
        let (a, xyz) = grid_matrix(5, 5, 5, C::new(4.0, 0.0), &mut rng);
        let an = Analysis::new(&a, &xyz, 0).unwrap();
        assert!(matches!(Lu::factor_analysed(&a, &an, 1000), Err(FactorError::Scratch(_))));
    }
}

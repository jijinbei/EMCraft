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

use num_complex::Complex64 as C;

use crate::sparse::Csr;

/// The largest frontal matrix (rows) a factorization may form: m² complex entries.
pub const MAX_FRONT: usize = 8000;

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
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pivot {
    One(C),
    /// A 2 × 2 block [[a, b], [b, c]] at this column and the next.
    Two(C, C, C),
    /// The second column of a 2 × 2 block.
    Second,
}

#[derive(Clone, Debug)]
struct Supernode {
    /// Positions of the front's rows; the first `k` are the supernode's columns in pivot order.
    rows: Vec<usize>,
    k: usize,
    /// The front's first k columns after elimination: L below the diagonal (m × k, by column).
    l: Vec<C>,
    pivots: Vec<Pivot>,
}

/// A factorization.
#[derive(Clone, Debug)]
pub struct Ldlt {
    n: usize,
    /// `perm[position] = unknown`.
    perm: Vec<usize>,
    supernodes: Vec<Supernode>,
    /// Pivots replaced by √ε ‖A‖.
    pub perturbed: usize,
    pub two_by_two: usize,
    /// Entries of L (with the diagonal blocks).
    pub entries: usize,
    /// The largest front.
    pub largest_front: usize,
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

impl Ldlt {
    /// Factors `a` (complex symmetric, both triangles stored) with the unknowns ordered by
    /// nested dissection on `xyz` (a point per unknown).
    pub fn factor(a: &Csr, xyz: &[[f64; 3]]) -> Result<Ldlt, FactorError> {
        let n = a.n;
        if xyz.len() != n || a.ptr.len() != n + 1 {
            return Err(FactorError::Dimension(format!("{} unknowns, {} points", n, xyz.len())));
        }
        if a.val.iter().any(|v| !v.re.is_finite() || !v.im.is_finite()) {
            return Err(FactorError::NotFinite);
        }
        let norm = a.max_abs();
        if n > 0 && !(norm > 0.0) {
            return Err(FactorError::Zero);
        }
        let tau = f64::EPSILON.sqrt() * norm;

        // Ordering: nested dissection, then a postorder of its elimination tree.
        let nd = crate::ordering::nested_dissection(xyz, &a.adjacency());
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

        let ns = super_rows.len();
        let mut snode_of = vec![0usize; n];
        for s in 0..ns {
            for j in starts[s]..starts[s + 1] {
                snode_of[j] = s;
            }
        }
        // Pending update matrices: (rows, m × m values) per supernode, consumed by the parent.
        let mut pending: Vec<Vec<(Vec<usize>, Vec<C>)>> = vec![Vec::new(); ns];
        let mut pos = vec![usize::MAX; n];
        let mut supernodes = Vec::with_capacity(ns);
        let (mut perturbed, mut two_by_two, mut entries, mut largest_front) = (0, 0, 0, 0);
        for (s, mut rows) in super_rows.into_iter().enumerate() {
            let (c0, c1) = (starts[s], starts[s + 1]);
            let k = c1 - c0;
            let m = rows.len();
            if m > MAX_FRONT {
                return Err(FactorError::FrontTooLarge { rows: m, limit: MAX_FRONT });
            }
            largest_front = largest_front.max(m);
            for (r, &p) in rows.iter().enumerate() {
                pos[p] = r;
            }
            let mut f = vec![C::new(0.0, 0.0); m * m];
            for j in c0..c1 {
                let cj = j - c0;
                for &(i, v) in &a_cols[j] {
                    let r = pos[i];
                    f[cj * m + r] += v;
                    if r != cj {
                        f[r * m + cj] += v;
                    }
                }
            }
            for (urows, u) in pending[s].drain(..) {
                let mu = urows.len();
                let at: Vec<usize> = urows.iter().map(|&p| pos[p]).collect();
                for b in 0..mu {
                    let cb = at[b] * m;
                    for a in 0..mu {
                        f[cb + at[a]] += u[b * mu + a];
                    }
                }
            }

            // Partial factorization of the first k columns, right-looking within the panel.
            let mut pivots = vec![Pivot::Second; k];
            let mut p = 0;
            while p < k {
                let (r, w1) = colmax(&f, m, p, p);
                let app = f[p * m + p].norm();
                let mut choice: Option<(usize, Option<usize>)> = None; // (column to bring to p, 2×2 partner)
                if app >= ALPHA * w1 && (app > 0.0 || w1 == 0.0) {
                    choice = Some((p, None));
                } else if r < k {
                    let (_, wr) = colmax(&f, m, r, p);
                    let arr = f[r * m + r].norm();
                    if app * wr >= ALPHA * w1 * w1 {
                        choice = Some((p, None));
                    } else if arr >= ALPHA * wr {
                        choice = Some((r, None));
                    } else if p + 1 < k {
                        choice = Some((p, Some(r)));
                    }
                }
                if choice.is_none() {
                    // The largest entry lies outside the supernode: another column that passes
                    // the test, else the best 2 × 2 inside it, else column p.
                    for j in p + 1..k {
                        let (_, wj) = colmax(&f, m, j, p);
                        if f[j * m + j].norm() >= ALPHA * wj && f[j * m + j].norm() > tau {
                            choice = Some((j, None));
                            break;
                        }
                    }
                }
                if choice.is_none() && p + 1 < k {
                    let (q, wq) = (p + 1..k).fold((usize::MAX, 0.0), |b, i| if f[p * m + i].norm() > b.1 { (i, f[p * m + i].norm()) } else { b });
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
                        interchange(&mut f, m, k, &mut rows, p, col);
                        let mut d = f[p * m + p];
                        if !(d.norm() > tau) {
                            d = if d.norm() > 0.0 { d / d.norm() * tau } else { C::new(tau, 0.0) };
                            perturbed += 1;
                        }
                        pivots[p] = Pivot::One(d);
                        // Original column values c_i, then l_i = c_i / d.
                        let c: Vec<C> = f[p * m + p + 1..p * m + m].to_vec();
                        let dinv = d.inv();
                        for (i, ci) in c.iter().enumerate() {
                            f[p * m + p + 1 + i] = ci * dinv;
                        }
                        f[p * m + p] = C::new(1.0, 0.0);
                        // Update the rest of the panel: F[i][j] −= l_i c_j.
                        for j in p + 1..k {
                            let cj = c[j - p - 1];
                            if cj == C::new(0.0, 0.0) {
                                continue;
                            }
                            for i in p + 1..m {
                                let li = f[p * m + i];
                                f[j * m + i] -= li * cj;
                            }
                        }
                        p += 1;
                    }
                    Some(q) => {
                        interchange(&mut f, m, k, &mut rows, p + 1, q);
                        let (a11, a21, a22) = (f[p * m + p], f[p * m + p + 1], f[(p + 1) * m + p + 1]);
                        let det = a11 * a22 - a21 * a21;
                        let (i11, i21, i22) = (a22 / det, -a21 / det, a11 / det);
                        pivots[p] = Pivot::Two(a11, a21, a22);
                        pivots[p + 1] = Pivot::Second;
                        two_by_two += 1;
                        let c0v: Vec<C> = f[p * m + p + 2..p * m + m].to_vec();
                        let c1v: Vec<C> = f[(p + 1) * m + p + 2..(p + 1) * m + m].to_vec();
                        for i in 0..c0v.len() {
                            f[p * m + p + 2 + i] = c0v[i] * i11 + c1v[i] * i21;
                            f[(p + 1) * m + p + 2 + i] = c0v[i] * i21 + c1v[i] * i22;
                        }
                        f[p * m + p] = C::new(1.0, 0.0);
                        f[p * m + p + 1] = C::new(0.0, 0.0);
                        f[(p + 1) * m + p + 1] = C::new(1.0, 0.0);
                        for j in p + 2..k {
                            let (cj0, cj1) = (c0v[j - p - 2], c1v[j - p - 2]);
                            for i in p + 2..m {
                                let (l0, l1) = (f[p * m + i], f[(p + 1) * m + i]);
                                f[j * m + i] -= l0 * cj0 + l1 * cj1;
                            }
                        }
                        p += 2;
                    }
                }
            }

            // The update matrix U = F₂₂ − L₂₁ D L₂₁ᵀ for the parent.
            let mu = m - k;
            if mu > 0 {
                // W = L₂₁ D (mu × k, by column).
                let mut w = vec![C::new(0.0, 0.0); mu * k];
                let mut q = 0;
                while q < k {
                    match pivots[q] {
                        Pivot::One(d) => {
                            for i in 0..mu {
                                w[q * mu + i] = f[q * m + k + i] * d;
                            }
                            q += 1;
                        }
                        Pivot::Two(a11, a21, a22) => {
                            for i in 0..mu {
                                let (l0, l1) = (f[q * m + k + i], f[(q + 1) * m + k + i]);
                                w[q * mu + i] = l0 * a11 + l1 * a21;
                                w[(q + 1) * mu + i] = l0 * a21 + l1 * a22;
                            }
                            q += 2;
                        }
                        Pivot::Second => q += 1,
                    }
                }
                let mut u = vec![C::new(0.0, 0.0); mu * mu];
                for j in 0..mu {
                    for i in j..mu {
                        u[j * mu + i] = f[(k + j) * m + k + i];
                    }
                }
                for q in 0..k {
                    let wq = &w[q * mu..(q + 1) * mu];
                    let lq = &f[q * m + k..q * m + m];
                    for j in 0..mu {
                        let lj = lq[j];
                        if lj == C::new(0.0, 0.0) {
                            continue;
                        }
                        let col = &mut u[j * mu + j..(j + 1) * mu];
                        for (x, wi) in col.iter_mut().zip(&wq[j..]) {
                            *x -= wi * lj;
                        }
                    }
                }
                for j in 0..mu {
                    for i in j + 1..mu {
                        u[i * mu + j] = u[j * mu + i];
                    }
                }
                // The first row below is the etree parent of the supernode's last column.
                if let Some(&ps) = parent.get(c1 - 1).and_then(|&q| snode_of.get(q)) {
                    pending[ps].push((rows[k..].to_vec(), u));
                }
            }
            f.truncate(k * m);
            entries += k * m - k * (k - 1) / 2;
            supernodes.push(Supernode { rows, k, l: f, pivots });
        }
        Ok(Ldlt { n, perm, supernodes, perturbed, two_by_two, entries, largest_front })
    }

    pub fn n(&self) -> usize {
        self.n
    }

    /// Solves A x = b in place.
    pub fn solve(&self, b: &mut [C]) -> Result<(), FactorError> {
        if b.len() != self.n {
            return Err(FactorError::Dimension(format!("right-hand side of {} for {} unknowns", b.len(), self.n)));
        }
        let mut y: Vec<C> = self.perm.iter().map(|&u| b[u]).collect();
        let mut local = Vec::new();
        // L z = b.
        for s in &self.supernodes {
            let m = s.rows.len();
            local.clear();
            local.extend(s.rows.iter().map(|&r| y[r]));
            let mut p = 0;
            while p < s.k {
                let width = if matches!(s.pivots[p], Pivot::Two(..)) { 2 } else { 1 };
                for c in p..p + width {
                    let xc = local[c];
                    if xc == C::new(0.0, 0.0) {
                        continue;
                    }
                    for i in p + width..m {
                        local[i] -= s.l[c * m + i] * xc;
                    }
                }
                p += width;
            }
            for (r, v) in s.rows.iter().zip(&local) {
                y[*r] = *v;
            }
        }
        // D w = z.
        for s in &self.supernodes {
            let mut p = 0;
            while p < s.k {
                match s.pivots[p] {
                    Pivot::One(d) => {
                        y[s.rows[p]] /= d;
                        p += 1;
                    }
                    Pivot::Two(a11, a21, a22) => {
                        let (r0, r1) = (s.rows[p], s.rows[p + 1]);
                        let det = a11 * a22 - a21 * a21;
                        let (b0, b1) = (y[r0], y[r1]);
                        y[r0] = (a22 * b0 - a21 * b1) / det;
                        y[r1] = (a11 * b1 - a21 * b0) / det;
                        p += 2;
                    }
                    Pivot::Second => p += 1,
                }
            }
        }
        // Lᵀ x = w.
        for s in self.supernodes.iter().rev() {
            let m = s.rows.len();
            local.clear();
            local.extend(s.rows.iter().map(|&r| y[r]));
            let mut p = s.k;
            while p > 0 {
                let width = if p >= 2 && matches!(s.pivots[p - 2], Pivot::Two(..)) { 2 } else { 1 };
                let start = p - width;
                for c in start..p {
                    let mut acc = C::new(0.0, 0.0);
                    for i in p..m {
                        acc += s.l[c * m + i] * local[i];
                    }
                    local[c] -= acc;
                }
                p = start;
            }
            for (r, v) in s.rows.iter().zip(&local).take(s.k) {
                y[*r] = *v;
            }
        }
        for (pos, &u) in self.perm.iter().enumerate() {
            b[u] = y[pos];
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
    fn grid_matrix(nx: usize, ny: usize, nz: usize, diag: C, rng: &mut Rng) -> (Csr, Vec<[f64; 3]>) {
        let id = |i: usize, j: usize, k: usize| i + nx * (j + ny * k);
        let n = nx * ny * nz;
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut xyz = vec![[0.0; 3]; n];
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    xyz[id(i, j, k)] = [i as f64, j as f64, k as f64];
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
            assert!(residual(&a, &x, &b) < 1e-12, "{label}: {}", residual(&a, &x, &b));
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
        let xyz: Vec<[f64; 3]> = (0..n).map(|i| [i as f64, 0.0, 0.0]).collect();
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
    fn hostile_matrices_are_refused() {
        let groups = [vec![0, 1]];
        let mut a = Csr::from_groups(2, groups.iter().map(|g| g.as_slice()));
        let xyz = [[0.0; 3], [1.0, 0.0, 0.0]];
        assert_eq!(Ldlt::factor(&a, &xyz).unwrap_err(), FactorError::Zero);
        a.add(0, 0, C::new(f64::NAN, 0.0));
        assert_eq!(Ldlt::factor(&a, &xyz).unwrap_err(), FactorError::NotFinite);
        assert!(matches!(Ldlt::factor(&a, &xyz[..1]), Err(FactorError::Dimension(_))));
    }
}

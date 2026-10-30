//! The symbolic analysis: the fill-reducing order, the elimination tree, the supernodes with
//! their row structures, and the supernodal tree the numeric factorizations traverse.

use std::ops::Range;

use emcraft_model::Point3;

use super::FactorError;
use crate::sparse::Csr;

/// The elimination tree of the pattern in position order: `parent[j]`, `usize::MAX` for roots.
fn etree(lower_rows_of: &[Vec<usize>]) -> Vec<usize> {
    // lower_rows_of[k] = positions i < k with a_ki ≠ 0 (Liu's algorithm with path compression).
    let n = lower_rows_of.len();
    let mut parent = vec![usize::MAX; n];
    let mut ancestor = vec![usize::MAX; n];
    for k in 0..n {
        for &i in &lower_rows_of[k] {
            crate::ordering::etree_link(&mut parent, &mut ancestor, i, k);
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
            let mut r: Vec<usize> = a.col[a.row(u)].iter().map(|&v| inv[v]).filter(|&i| i < k).collect();
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

/// Entries of a supernode's columns of L: k columns over m rows (the lower triangle of the
/// first k).
pub(crate) fn fill(k: usize, m: usize) -> usize {
    k * m - k * k.saturating_sub(1) / 2
}

/// Which supernode each position belongs to.
fn supernode_of(starts: &[usize], n: usize) -> Vec<usize> {
    let mut snode_of = vec![0usize; n];
    for (s, w) in starts.windows(2).enumerate() {
        for j in w[0]..w[1] {
            snode_of[j] = s;
        }
    }
    snode_of
}

/// The union of two ascending row sets.
fn merge_rows(a: &[usize], b: &[usize]) -> Vec<usize> {
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
    merged
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
    let snode_of = supernode_of(starts, n);
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
            let merged = merge_rows(&rows[c], &rows[p]);
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
    pub(crate) n: usize,
    /// The pattern it was made for.
    ptr: Vec<usize>,
    col: Vec<usize>,
    /// `perm[position] = unknown`, and its inverse.
    pub(crate) perm: Vec<usize>,
    pub(crate) inv: Vec<usize>,
    /// The elimination tree by position (`usize::MAX` for roots).
    parent: Vec<usize>,
    /// Supernode s holds the positions `starts[s]..starts[s + 1]`.
    starts: Vec<usize>,
    /// Each supernode's front rows (positions, ascending), its own columns first.
    pub(crate) super_rows: Vec<Vec<usize>>,
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
        let (starts, super_rows) = fundamental_supernodes(&lower, &parent);
        let (starts, super_rows) = amalgamate(&starts, super_rows, &parent, n);
        let snode_of = supernode_of(&starts, n);
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

    /// The number of supernodes.
    pub(crate) fn supernodes(&self) -> usize {
        self.super_rows.len()
    }

    /// Supernode `s`'s own columns (positions).
    pub(crate) fn columns(&self, s: usize) -> Range<usize> {
        self.starts[s]..self.starts[s + 1]
    }

    /// Each supernode's parent: the supernode of its last column's parent in the elimination
    /// tree.
    pub(crate) fn supernode_parents(&self) -> Vec<Option<usize>> {
        (0..self.supernodes()).map(|s| self.parent.get(self.starts[s + 1] - 1).and_then(|&q| self.snode_of.get(q)).copied()).collect()
    }
}

/// The column structures (rows below the diagonal), children merged in, and the fundamental
/// supernodes they make: each supernode's first position (then n) and its front rows.
fn fundamental_supernodes(lower: &[Vec<usize>], parent: &[usize]) -> (Vec<usize>, Vec<Vec<usize>>) {
    let n = lower.len();
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
    starts.push(n);
    (starts, super_rows)
}

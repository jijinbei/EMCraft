//! A sparse complex symmetric matrix in compressed rows, both triangles stored: the pattern is
//! made once from the element connectivity, then values are added by binary search.

use std::ops::Range;

use num_complex::Complex64 as C;

#[derive(Clone, Debug, Default)]
pub struct Csr {
    pub n: usize,
    pub ptr: Vec<usize>,
    pub col: Vec<usize>,
    pub val: Vec<C>,
}

impl Csr {
    /// The pattern of every pair of unknowns that appear together in one of `groups`.
    pub fn from_groups<'a>(n: usize, groups: impl Iterator<Item = &'a [usize]> + Clone) -> Csr {
        let mut rows: Vec<Vec<usize>> = vec![Vec::new(); n];
        for g in groups {
            for &i in g {
                if let Some(r) = rows.get_mut(i) {
                    r.extend(g.iter().copied().filter(|&j| j < n));
                }
            }
        }
        let mut ptr = Vec::with_capacity(n + 1);
        ptr.push(0);
        let mut col = Vec::new();
        for (i, mut r) in rows.into_iter().enumerate() {
            r.push(i);
            r.sort_unstable();
            r.dedup();
            col.extend(r);
            ptr.push(col.len());
        }
        let val = vec![C::new(0.0, 0.0); col.len()];
        Csr { n, ptr, col, val }
    }

    /// A matrix of `n` rows built row by row.
    pub fn builder(n: usize) -> CsrBuilder {
        CsrBuilder { csr: Csr { n, ptr: vec![0], col: Vec::new(), val: Vec::new() } }
    }

    /// The entries of row `i`: their indices into `col` and `val` (none for a row outside).
    pub fn row(&self, i: usize) -> Range<usize> {
        match (self.ptr.get(i), self.ptr.get(i + 1)) {
            (Some(&a), Some(&b)) if a <= b => a..b,
            _ => 0..0,
        }
    }

    /// Row `i`'s (column, value) pairs.
    pub fn entries(&self, i: usize) -> impl Iterator<Item = (usize, C)> + '_ {
        self.row(i).map(|k| (self.col[k], self.val[k]))
    }

    /// The same matrix with structural zeros where its pattern lacks the transpose of an entry
    /// or a diagonal, each row sorted by column (a stable sort: entries of equal column keep
    /// their order, the first kept).
    pub fn with_symmetric_pattern(&self) -> Csr {
        let n = self.n;
        let mut missing: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, m) in missing.iter_mut().enumerate() {
            if self.position(i, i).is_none() {
                m.push(i);
            }
        }
        for i in 0..n {
            for (j, _) in self.entries(i) {
                if j != i
                    && self.position(j, i).is_none()
                    && let Some(m) = missing.get_mut(j)
                {
                    m.push(i);
                }
            }
        }
        let mut out = Csr::builder(n);
        for (i, extra) in missing.into_iter().enumerate() {
            let mut row: Vec<(usize, C)> = self.entries(i).chain(extra.into_iter().map(|j| (j, C::new(0.0, 0.0)))).collect();
            row.sort_by_key(|e| e.0);
            row.dedup_by_key(|e| e.0);
            out.row(row);
        }
        out.finish()
    }

    /// The same pattern with every value zero.
    pub fn zeroed(&self) -> Csr {
        Csr { n: self.n, ptr: self.ptr.clone(), col: self.col.clone(), val: vec![C::new(0.0, 0.0); self.col.len()] }
    }

    fn position(&self, i: usize, j: usize) -> Option<usize> {
        let (a, b) = (*self.ptr.get(i)?, *self.ptr.get(i + 1)?);
        let row = self.col.get(a..b)?;
        row.binary_search(&j).ok().map(|k| a + k)
    }

    /// Adds `v` at (i, j); `false` when (i, j) is not in the pattern.
    pub fn add(&mut self, i: usize, j: usize, v: C) -> bool {
        match self.position(i, j) {
            Some(k) => {
                if let Some(x) = self.val.get_mut(k) {
                    *x += v;
                }
                true
            }
            None => false,
        }
    }

    pub fn get(&self, i: usize, j: usize) -> C {
        self.position(i, j).and_then(|k| self.val.get(k).copied()).unwrap_or_default()
    }

    /// y = A x.
    pub fn mul(&self, x: &[C]) -> Vec<C> {
        (0..self.n)
            .map(|i| {
                let (a, b) = (self.ptr[i], self.ptr[i + 1]);
                self.col[a..b].iter().zip(&self.val[a..b]).map(|(&j, v)| v * x.get(j).copied().unwrap_or_default()).sum()
            })
            .collect()
    }

    /// The largest |entry|.
    pub fn max_abs(&self) -> f64 {
        self.val.iter().fold(0.0, |m, v| m.max(v.norm()))
    }

    /// Off-diagonal neighbours of each unknown.
    pub fn adjacency(&self) -> Vec<Vec<usize>> {
        (0..self.n).map(|i| self.col[self.ptr[i]..self.ptr[i + 1]].iter().copied().filter(|&j| j != i).collect()).collect()
    }
}

/// A [`Csr`] under construction, row by row ([`Csr::builder`]).
pub struct CsrBuilder {
    csr: Csr,
}

impl CsrBuilder {
    /// Adds an entry to the current row.
    pub fn push(&mut self, j: usize, v: C) {
        self.csr.col.push(j);
        self.csr.val.push(v);
    }

    /// Ends the current row.
    pub fn end_row(&mut self) {
        self.csr.ptr.push(self.csr.col.len());
    }

    /// Adds a whole row.
    pub fn row(&mut self, entries: impl IntoIterator<Item = (usize, C)>) {
        for (j, v) in entries {
            self.push(j, v);
        }
        self.end_row();
    }

    pub fn finish(self) -> Csr {
        self.csr
    }
}

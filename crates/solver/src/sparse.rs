//! A sparse complex symmetric matrix in compressed rows, both triangles stored: the pattern is
//! made once from the element connectivity, then values are added by binary search.

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

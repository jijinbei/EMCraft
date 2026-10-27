//! The multifrontal method shared by the symmetric [`crate::ldlt`] and the unsymmetric
//! [`crate::lu`] factorizations of matrices with a symmetric pattern.
//!
//! **Structure.** The unknowns are ordered by nested dissection ([`crate::ordering`]), then by a
//! postorder of the elimination tree (J. W. H. Liu, *The role of elimination trees in sparse
//! factorization*, SIAM J. Matrix Anal. Appl. 11(1):134–172, 1990), and consecutive columns
//! with nested structures are merged into supernodes ([`Analysis`]). Each supernode is factored
//! in a dense frontal matrix: its rows and columns of A plus the update matrices its children
//! left behind (extend-add), the supernode's own variables eliminated, and the Schur complement
//! passed to the parent (I. S. Duff, J. K. Reid, *The multifrontal solution of indefinite sparse
//! symmetric linear equations*, ACM TOMS 9(3):302–325, 1983; J. W. H. Liu, *The multifrontal
//! method for sparse matrix solution: theory and practice*, SIAM Review 34(1):82–109, 1992).
//!
//! **The driver** ([`factor_tree`]) is the same for every factorization; what a front holds,
//! how A and the updates enter it and how its variables are eliminated is a [`FrontKernel`]'s.
//! Subtrees light enough go to threads whole, each in postorder as alone (so each keeps the
//! memory of a sequential factorization); the heavy top, whose fronts are large, follows on the
//! calling thread in order, its fronts' products threaded inside. Every front receives its
//! children's updates in the children's order whichever thread made them, so the factor is the
//! same bit for bit on any number of threads.

use std::collections::BTreeMap;
use std::ops::Range;

use num_complex::Complex64 as C;
use rayon::prelude::*;

use crate::sparse::Csr;

mod analysis;

pub use analysis::Analysis;
pub(crate) use analysis::fill;

/// The largest frontal matrix (rows) a factorization may form: m² complex entries.
pub const MAX_FRONT: usize = 20_000;

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
    #[error("the device failed: {0}")]
    Device(String),
}

/// A supernode's dense frontal matrix, column-major m × m.
pub(crate) struct Front {
    /// Positions of the front's rows (and columns), ascending; the first `k` are the
    /// supernode's own variables.
    pub rows: Vec<usize>,
    pub k: usize,
    pub f: Vec<C>,
}

impl Front {
    /// The order m.
    pub fn m(&self) -> usize {
        self.rows.len()
    }

    /// The front row of position `p`.
    pub fn place(&self, p: usize) -> Option<usize> {
        self.rows.binary_search(&p).ok()
    }

    /// [`Front::place`] of an entry of A, refused when outside the front.
    pub fn place_entry(&self, p: usize) -> Result<usize, FactorError> {
        self.place(p).ok_or_else(|| FactorError::Dimension("an entry outside its front".into()))
    }
}

/// A Schur complement passed to the parent: its rows (positions) and its values in the kernel's
/// layout.
pub(crate) struct Update {
    pub rows: Vec<usize>,
    pub values: Vec<C>,
}

/// What a factorization counts.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Counts {
    /// Pivots replaced by the static-pivot threshold.
    pub perturbed: usize,
    /// 2 × 2 pivots.
    pub two_by_two: usize,
    /// Entries of the factor.
    pub entries: usize,
    /// The largest front (rows).
    pub largest_front: usize,
}

impl Counts {
    fn add(&mut self, o: Counts) {
        self.perturbed += o.perturbed;
        self.two_by_two += o.two_by_two;
        self.entries += o.entries;
        self.largest_front = self.largest_front.max(o.largest_front);
    }
}

/// A front eliminated: what the factor keeps of it, the update for the parent and its counts.
pub(crate) struct Eliminated<N> {
    pub node: N,
    pub update: Option<Update>,
    pub counts: Counts,
}

/// One factorization's part of the multifrontal method: how A's entries and the children's
/// updates enter a front, and how the front's own variables are eliminated.
pub(crate) trait FrontKernel: Sync {
    /// What the factor keeps of a supernode.
    type Node: Send;

    /// Adds A's entries of the supernode's own `columns` (positions) into `front`.
    fn assemble(&self, columns: Range<usize>, front: &mut Front) -> Result<(), FactorError>;

    /// Adds a child's update `values` (of [`FrontKernel::eliminate`]'s layout), its row a at
    /// front row `place[a]`, into the front `f` of order m.
    fn extend_add(&self, f: &mut [C], m: usize, place: &[usize], values: &[C]);

    /// Eliminates the front's first k variables.
    fn eliminate(&self, front: Front) -> Result<Eliminated<Self::Node>, FactorError>;
}

/// Refuses a matrix that is not of `an`'s pattern, has a non-finite entry or is zero.
pub(crate) fn check(a: &Csr, an: &Analysis) -> Result<(), FactorError> {
    if !an.fits(a) {
        return Err(FactorError::Dimension("the analysis is of another pattern".into()));
    }
    if a.val.iter().any(|v| !v.re.is_finite() || !v.im.is_finite()) {
        return Err(FactorError::NotFinite);
    }
    if a.n > 0 && !(a.max_abs() > 0.0) {
        return Err(FactorError::Zero);
    }
    Ok(())
}

/// The supernodal tree: each supernode's parent and children (ascending), and the first
/// supernode of its subtree (a subtree is the contiguous range from it to the root).
struct Tree {
    parent: Vec<Option<usize>>,
    children: Vec<Vec<usize>>,
    first: Vec<usize>,
    /// Multiply–adds of each subtree, about k m² per front.
    work: Vec<f64>,
}

impl Tree {
    fn new(an: &Analysis) -> Tree {
        let ns = an.supernodes();
        let parent = an.supernode_parents();
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); ns];
        for (s, p) in parent.iter().enumerate() {
            if let Some(p) = p {
                children[*p].push(s);
            }
        }
        let mut first: Vec<usize> = (0..ns).collect();
        let mut work = vec![0.0f64; ns];
        for s in 0..ns {
            let (k, m) = (an.columns(s).len() as f64, an.super_rows[s].len() as f64);
            work[s] += k * m * m;
            if let Some(p) = parent[s] {
                first[p] = first[p].min(first[s]);
                work[p] += work[s];
            }
        }
        Tree { parent, children, first, work }
    }

    fn roots(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.parent.len()).filter(|&s| self.parent[s].is_none())
    }

    /// The roots of the subtrees that go to threads whole (each with under 1/(8 · threads) of
    /// the work, or a leaf), and the supernodes above them, both ascending.
    fn split(&self, threads: usize) -> (Vec<usize>, Vec<usize>) {
        let total: f64 = self.roots().map(|s| self.work[s]).sum();
        let light = total / (8 * threads) as f64;
        let (mut tasks, mut top): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
        let mut stack: Vec<usize> = self.roots().collect();
        while let Some(s) = stack.pop() {
            if self.work[s] <= light || self.children[s].is_empty() {
                tasks.push(s);
            } else {
                top.push(s);
                stack.extend(self.children[s].iter().copied());
            }
        }
        tasks.sort_unstable();
        top.sort_unstable();
        (tasks, top)
    }
}

/// A subtree factored on a thread.
struct Subtree<N> {
    root: usize,
    nodes: Vec<(usize, N)>,
    update: Option<Update>,
    counts: Counts,
}

/// Assembles supernode `s`'s front from A and its children's `updates` (in the children's
/// order) and eliminates it.
fn node<K: FrontKernel>(kernel: &K, an: &Analysis, s: usize, rows: Vec<usize>, updates: Vec<Update>) -> Result<Eliminated<K::Node>, FactorError> {
    let columns = an.columns(s);
    let m = rows.len();
    if m > MAX_FRONT {
        return Err(FactorError::FrontTooLarge { rows: m, limit: MAX_FRONT });
    }
    let mut front = Front { rows, k: columns.len(), f: vec![C::new(0.0, 0.0); m * m] };
    kernel.assemble(columns, &mut front)?;
    for u in updates {
        let place = u.rows.iter().map(|&p| front.place(p)).collect::<Option<Vec<usize>>>();
        let place = place.ok_or_else(|| FactorError::Dimension("an update outside its parent's front".into()))?;
        kernel.extend_add(&mut front.f, m, &place, &u.values);
    }
    let mut done = kernel.eliminate(front)?;
    done.counts.largest_front = m;
    Ok(done)
}

/// Factors the subtree of `root` (its supernodes in postorder with their rows), as the
/// sequential factorization would.
fn subtree<K: FrontKernel>(
    kernel: &K,
    an: &Analysis,
    tree: &Tree,
    root: usize,
    list: Vec<(usize, Vec<usize>)>,
) -> Result<Subtree<K::Node>, FactorError> {
    let mut pending: BTreeMap<usize, Vec<Update>> = BTreeMap::new();
    let mut out = Subtree { root, nodes: Vec::with_capacity(list.len()), update: None, counts: Counts::default() };
    for (s, rows) in list {
        let updates = pending.remove(&s).unwrap_or_default();
        let f = node(kernel, an, s, rows, updates)?;
        out.counts.add(f.counts);
        if s == root {
            out.update = f.update;
        } else if let (Some(u), Some(p)) = (f.update, tree.parent.get(s).copied().flatten()) {
            pending.entry(p).or_default().push(u);
        }
        out.nodes.push((s, f.node));
    }
    Ok(out)
}

/// Factors every supernode of `an` with `kernel`: the light subtrees on threads, the top after
/// them on the calling thread (where [`crate::accel`]'s device is). Returns the nodes by
/// supernode and the counts.
pub(crate) fn factor_tree<K: FrontKernel>(kernel: &K, an: &Analysis) -> Result<(Vec<K::Node>, Counts), FactorError> {
    let ns = an.supernodes();
    let tree = Tree::new(an);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let (tasks, top) = tree.split(threads);

    let mut nodes: Vec<Option<K::Node>> = (0..ns).map(|_| None).collect();
    let mut super_rows: Vec<Option<Vec<usize>>> = an.super_rows.iter().cloned().map(Some).collect();
    // Each task's subtree: its rows taken out for the thread.
    let jobs: Vec<(usize, Vec<(usize, Vec<usize>)>)> =
        tasks.iter().map(|&s| (s, (tree.first[s]..=s).map(|t| (t, super_rows[t].take().unwrap_or_default())).collect())).collect();
    let done: Vec<Result<Subtree<K::Node>, FactorError>> = jobs.into_par_iter().map(|(root, list)| subtree(kernel, an, &tree, root, list)).collect();
    // Updates waiting for a top supernode: (child, update).
    let mut pending: Vec<Vec<(usize, Update)>> = (0..ns).map(|_| Vec::new()).collect();
    let mut counts = Counts::default();
    for d in done {
        let d = d?;
        counts.add(d.counts);
        for (s, node) in d.nodes {
            nodes[s] = Some(node);
        }
        if let (Some(u), Some(p)) = (d.update, tree.parent[d.root]) {
            pending[p].push((d.root, u));
        }
    }
    for s in top {
        let rows = super_rows[s].take().unwrap_or_default();
        let mut from_children = std::mem::take(&mut pending[s]);
        from_children.sort_by_key(|c| c.0);
        let updates = from_children.into_iter().map(|(_, u)| u).collect();
        let f = node(kernel, an, s, rows, updates)?;
        counts.add(f.counts);
        if let (Some(u), Some(p)) = (f.update, tree.parent[s]) {
            pending[p].push((s, u));
        }
        nodes[s] = Some(f.node);
    }
    let nodes =
        nodes.into_iter().map(|n| n.ok_or(FactorError::Dimension("a supernode was left unfactored".into()))).collect::<Result<Vec<_>, _>>()?;
    Ok((nodes, counts))
}

/// Solves A x = b by `solve` with iterative refinement until the backward error
/// ‖b − A x‖∞ / (‖A‖ ‖x‖∞ + ‖b‖∞) stops falling (at most `steps` corrections); returns x and
/// that error.
pub(crate) fn refine(a: &Csr, b: &[C], steps: usize, solve: impl Fn(&mut [C]) -> Result<(), FactorError>) -> Result<(Vec<C>, f64), FactorError> {
    let mut x = b.to_vec();
    solve(&mut x)?;
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
        solve(&mut r)?;
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

/// Shared by the factorizations' tests: a deterministic pseudo-random sequence and matrices on
/// a 3D grid graph.
#[cfg(test)]
pub(crate) mod testing {
    use emcraft_model::Point3;
    use num_complex::Complex64 as C;

    use crate::sparse::Csr;

    /// A deterministic pseudo-random sequence (xorshift).
    pub struct Rng(pub u64);

    impl Rng {
        pub fn next(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        }

        pub fn c(&mut self) -> C {
            C::new(self.next(), self.next())
        }
    }

    /// The pattern of a 3D grid graph (7-point stencil), its edges and a point per node.
    pub fn grid_pattern(nx: usize, ny: usize, nz: usize) -> (Csr, Vec<[usize; 2]>, Vec<Point3>) {
        let id = |i: usize, j: usize, k: usize| i + nx * (j + ny * k);
        let n = nx * ny * nz;
        let mut edges: Vec<[usize; 2]> = Vec::new();
        let mut xyz = vec![Point3::ORIGIN; n];
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    xyz[id(i, j, k)] = Point3::new(i as f64, j as f64, k as f64);
                    if i + 1 < nx {
                        edges.push([id(i, j, k), id(i + 1, j, k)]);
                    }
                    if j + 1 < ny {
                        edges.push([id(i, j, k), id(i, j + 1, k)]);
                    }
                    if k + 1 < nz {
                        edges.push([id(i, j, k), id(i, j, k + 1)]);
                    }
                }
            }
        }
        let a = Csr::from_groups(n, edges.iter().map(|g| g.as_slice()));
        (a, edges, xyz)
    }
}

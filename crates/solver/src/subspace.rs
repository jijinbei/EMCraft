//! The skeleton the subspace eigensolvers share — the cavity's ([`crate::eigen`], real
//! gradients) and the periodic cell's ([`crate::bloch`], Bloch-periodic complex ones): the
//! discrete gradients of the nodal functions as sparse columns, the projection off them
//! (P. Arbenz, R. Geus, *Multilevel preconditioned iterative eigensolvers for Maxwell eigenvalue
//! problems*, Appl. Numer. Math. 54(2):107–121, 2005), and deterministic start vectors.

use std::collections::BTreeMap;

use emcraft_model::Point3;
use num_complex::Complex64 as C;

use crate::dense::Scalar;
use crate::driven::{Driven, Result};
use crate::ldlt::{Analysis, Ldlt, memory_budget};
use crate::lu::Lu;
use crate::sparse::Csr;

/// The scalars of the gradients' coefficients: real for a cavity (ZᵀMZ symmetric, factored by
/// LDLᵀ), complex for a periodic cell (ZᴴMZ Hermitian, factored by LU).
pub(crate) trait Coefficient: Scalar {
    /// Z*MZ factored.
    type Factor;
    /// `self` times the complex `x`.
    fn times(self, x: C) -> C;
    /// Factors Z*MZ, `points` one per column (for the ordering).
    fn factor(a: Csr, points: &[Point3]) -> Result<Self::Factor>;
    /// Solves with the factor for each right-hand side in place.
    fn solve(f: &Self::Factor, rhs: &mut [Vec<C>]) -> Result<()>;
    /// A start vector's entry from a sequence uniform in [−½, ½).
    fn random(rand: &mut impl FnMut() -> f64) -> C;
}

impl Coefficient for f64 {
    type Factor = Ldlt;

    fn times(self, x: C) -> C {
        x * self
    }

    fn factor(a: Csr, points: &[Point3]) -> Result<Ldlt> {
        Ok(Ldlt::factor(&a, points)?)
    }

    fn solve(f: &Ldlt, rhs: &mut [Vec<C>]) -> Result<()> {
        Ok(f.solve_many(rhs)?)
    }

    fn random(rand: &mut impl FnMut() -> f64) -> C {
        C::new(rand(), 0.0)
    }
}

impl Coefficient for C {
    type Factor = Lu;

    fn times(self, x: C) -> C {
        self * x
    }

    /// ZᴴMZ is not complex symmetric: LU, on a symmetric pattern for the analysis.
    fn factor(a: Csr, points: &[Point3]) -> Result<Lu> {
        let a = a.with_symmetric_pattern();
        let an = Analysis::new(&a, points, 0)?;
        Ok(Lu::factor_analysed(&a, &an, memory_budget())?)
    }

    fn solve(f: &Lu, rhs: &mut [Vec<C>]) -> Result<()> {
        for r in rhs {
            f.solve(r)?;
        }
        Ok(())
    }

    fn random(rand: &mut impl FnMut() -> f64) -> C {
        C::new(rand(), rand())
    }
}

/// The small matrix Z* M Z of the sparse columns Z (each (unknown, z)) and M on `pattern`, row a
/// by row: w = Σ_i over z_a's entries of `weigh(k, z_a,i)` into column `pattern.col[k]` for each
/// entry k of M's row i, then entry (a, b) = Σ_j `times(w_j, z_b,j)`; both summed in a fixed
/// order. (Z* is Zᵀ or Zᴴ as `weigh` conjugates or not.)
pub(crate) fn galerkin<Z: Copy>(columns: &[Vec<(usize, Z)>], pattern: &Csr, weigh: impl Fn(usize, Z) -> C, times: impl Fn(C, Z) -> C) -> Csr {
    // Which columns hold each unknown.
    let mut owners: Vec<Vec<(usize, Z)>> = vec![Vec::new(); pattern.n];
    for (c, col) in columns.iter().enumerate() {
        for &(u, z) in col {
            if let Some(o) = owners.get_mut(u) {
                o.push((c, z));
            }
        }
    }
    let mut out = Csr::builder(columns.len());
    for za in columns {
        let mut w: BTreeMap<usize, C> = BTreeMap::new();
        for &(i, z) in za {
            for k in pattern.row(i) {
                *w.entry(pattern.col[k]).or_insert(C::new(0.0, 0.0)) += weigh(k, z);
            }
        }
        let mut row: BTreeMap<usize, C> = BTreeMap::new();
        for (j, wj) in w {
            for &(b, zb) in owners.get(j).map_or(&[][..], Vec::as_slice) {
                *row.entry(b).or_insert(C::new(0.0, 0.0)) += times(wj, zb);
            }
        }
        out.row(row);
    }
    out.finish()
}

/// A deterministic sequence uniform in [−½, ½): a xorshift generator from `seed`.
pub(crate) fn uniform(seed: u64) -> impl FnMut() -> f64 {
    let mut state = seed;
    move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    }
}

/// Deterministic start vectors: `q` of length `n`, entries from a xorshift generator from
/// `seed` (real or complex as `T`).
pub(crate) fn start_vectors<T: Coefficient>(q: usize, n: usize, seed: u64) -> Vec<Vec<C>> {
    let mut rand = uniform(seed);
    (0..q).map(|_| (0..n).map(|_| T::random(&mut rand)).collect()).collect()
}

/// The discrete gradients as sparse columns over the unknowns, with a point per column (for
/// the ordering of Z*MZ): Whitney coefficients φ(b) − φ(a) on the edges, and at order 2 each
/// edge's gradient function ∇(λ_iλ_j) alone.
///
/// `node(n)`: the group of node n (its master node) and the value there of its group's nodal
/// function, `None` to leave out the edges at n. `unknown(slot)`: the unknown of the global
/// degree of freedom `slot`, `None` when the gradients do not reach it; such an edge's nodes
/// are left out when `blocks(slot)`. Columns with no entry above 1e-12 are dropped, entries of
/// 1e-14 or less too.
pub(crate) fn gradients<T: Coefficient>(
    d: &Driven,
    node: impl Fn(usize) -> Option<(usize, T)>,
    unknown: impl Fn(usize) -> Option<usize>,
    blocks: impl Fn(usize) -> bool,
) -> (Vec<Vec<(usize, T)>>, Vec<Point3>) {
    let (topo, mesh) = (&d.topo, &d.mesh);
    let pe = d.dofs.order.per_edge();
    let nn = mesh.nodes.len();
    let mut columns: Vec<BTreeMap<usize, T>> = vec![BTreeMap::new(); nn];
    let mut blocked = vec![false; nn];
    for (e, &[a, b]) in topo.edges.iter().enumerate() {
        let (Some((ma, pa)), Some((mb, pb))) = (node(a as usize), node(b as usize)) else { continue };
        match unknown(e * pe) {
            // ∇φ_n = Σ_edges (φ(b) − φ(a)) w_ab.
            Some(r) => {
                if let Some(c) = columns.get_mut(ma) {
                    *c.entry(r).or_default() -= pa;
                }
                if let Some(c) = columns.get_mut(mb) {
                    *c.entry(r).or_default() += pb;
                }
            }
            None if blocks(e * pe) => {
                for m in [ma, mb] {
                    if let Some(x) = blocked.get_mut(m) {
                        *x = true;
                    }
                }
            }
            None => {}
        }
    }
    let mut out = Vec::new();
    let mut points = Vec::new();
    for (n, col) in columns.into_iter().enumerate() {
        if !blocked[n] && !col.is_empty() && col.values().any(|v| v.norm() > 1e-12) {
            out.push(col.into_iter().filter(|(_, v)| v.norm() > 1e-14).collect());
            points.push(mesh.nodes[n]);
        }
    }
    if pe > 1 {
        for (e, &[a, b]) in topo.edges.iter().enumerate() {
            if let Some(r) = unknown(e * pe + 1) {
                out.push(vec![(r, T::real(1.0))]);
                points.push(mesh.nodes[a as usize].midpoint(mesh.nodes[b as usize]));
            }
        }
    }
    (out, points)
}

/// x ↦ x − Z (Z*MZ)⁻¹ Z*M x: the M-orthogonal projection off the gradients Z.
pub(crate) struct Deflation<T: Coefficient> {
    columns: Vec<Vec<(usize, T)>>,
    factor: Option<T::Factor>,
}

impl<T: Coefficient> Deflation<T> {
    /// The projection off `columns` (with their `points`), `mass(k)` M's entry k on `pattern`.
    pub(crate) fn new(columns: Vec<Vec<(usize, T)>>, points: &[Point3], pattern: &Csr, mass: impl Fn(usize) -> C) -> Result<Deflation<T>> {
        if columns.is_empty() {
            return Ok(Deflation { columns, factor: None });
        }
        // Z*MZ: w = M z_a (row i of z_a*M is Σ_i z̄_i M_ij), then (Z*MZ)_ab = w · z_b.
        let a = galerkin(&columns, pattern, |k, z: T| z.conj().times(mass(k)), |wj, zb: T| zb.times(wj));
        let factor = T::factor(a, points)?;
        Ok(Deflation { columns, factor: Some(factor) })
    }

    /// Projects each vector of `xs` in place; `mys` holds M times each.
    pub(crate) fn project(&self, xs: &mut [Vec<C>], mys: &[Vec<C>]) -> Result<()> {
        let Some(f) = &self.factor else { return Ok(()) };
        let mut rhs: Vec<Vec<C>> =
            mys.iter().map(|my| self.columns.iter().map(|z| z.iter().map(|&(u, c)| c.conj().times(my[u])).sum()).collect()).collect();
        T::solve(f, &mut rhs)?;
        for (x, s) in xs.iter_mut().zip(&rhs) {
            for (z, coef) in self.columns.iter().zip(s) {
                for &(u, c) in z {
                    x[u] -= c.times(*coef);
                }
            }
        }
        Ok(())
    }
}

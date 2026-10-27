//! Sparse complex symmetric factorization P A Pᵀ = L D Lᵀ (A = Aᵀ, not Hermitian) by the
//! multifrontal method: L unit lower triangular, D block diagonal with 1 × 1 and 2 × 2 pivots.
//!
//! **Structure.** The method's ordering, supernodes and traversal are
//! [`crate::multifrontal`]'s: each supernode is factored in a dense frontal matrix holding its
//! columns of A plus the update matrices its children left behind (extend-add), the supernode's
//! own columns eliminated, and the Schur complement passed to the parent (I. S. Duff, J. K. Reid,
//! *The multifrontal solution of indefinite sparse symmetric linear equations*, ACM TOMS
//! 9(3):302–325, 1983; J. W. H. Liu, *The multifrontal method for sparse matrix solution: theory
//! and practice*, SIAM Review 34(1):82–109, 1992). A front is symmetric: its lower triangle is
//! assembled into both, and the update matrices are passed packed (lower, by column).
//!
//! **Pivoting** ([`front`]). Within a front, the supernode's columns are fully summed and may be
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
//!
//! **Storage** (`storage`). The factored columns are kept in memory up to a budget, the rest
//! in a scratch file on disk.

use std::ops::Range;
use std::sync::Arc;

use emcraft_model::Point3;
use num_complex::Complex64 as C;

use crate::multifrontal::{Counts, Eliminated, Front, FrontKernel, Update, check, factor_tree, fill};
use crate::sparse::Csr;

mod front;
mod solve;
mod storage;

pub use crate::multifrontal::{Analysis, FactorError, MAX_FRONT};
pub(crate) use storage::memory_budget;

use front::{Pivot, factor_front, factor_front_on};
use storage::{Columns, Scratch, Store};

#[derive(Clone, Debug)]
struct Supernode {
    /// Positions of the front's rows; the first `k` are the supernode's columns in pivot order.
    rows: Vec<usize>,
    k: usize,
    /// The front's first k columns after elimination: L below the diagonal (m × k, by column).
    l: Columns,
    pivots: Vec<Pivot>,
}

/// A factorization.
#[derive(Clone, Debug)]
pub struct Ldlt {
    n: usize,
    /// `perm[position] = unknown`.
    perm: Vec<usize>,
    supernodes: Vec<Supernode>,
    /// The columns kept on disk, when the factor outgrew the memory budget.
    scratch: Option<Arc<Scratch>>,
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

/// The start of column b of a packed lower triangle of order m (columns stored from their
/// diagonal down).
fn packed(b: usize, m: usize) -> usize {
    fill(b, m)
}

/// The LDLᵀ's fronts: A's lower columns by position (S A S), the static-pivot threshold, and
/// where the factored columns go.
struct Kernel {
    /// Column j's entries on and below the diagonal: (row position, value).
    a_cols: Vec<Vec<(usize, C)>>,
    tau: f64,
    store: Store,
}

impl FrontKernel for Kernel {
    type Node = Supernode;

    fn assemble(&self, columns: Range<usize>, front: &mut Front) -> Result<(), FactorError> {
        let m = front.m();
        for j in columns.clone() {
            let cj = j - columns.start;
            for &(i, v) in self.a_cols.get(j).map(Vec::as_slice).unwrap_or(&[]) {
                let r = front.place_entry(i)?;
                front.f[cj * m + r] += v;
                if r != cj {
                    front.f[r * m + cj] += v;
                }
            }
        }
        Ok(())
    }

    /// The update is packed lower by columns (i ≥ j); positions in this front ascend with its
    /// rows.
    fn extend_add(&self, f: &mut [C], m: usize, place: &[usize], u: &[C]) {
        let mu = place.len();
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

    /// Factors the front's columns (on the calling thread's device when it takes the front) and
    /// keeps them (in memory, or on disk beyond the budget).
    fn eliminate(&self, front: Front) -> Result<Eliminated<Supernode>, FactorError> {
        let Front { mut rows, k, mut f } = front;
        let m = rows.len();
        let (pivots, pc) = match crate::accel::current().filter(|a| a.takes(m, k)) {
            Some(device) => factor_front_on(&*device, &mut f, m, k, &mut rows, self.tau)?,
            None => factor_front(&mut f, m, k, &mut rows, self.tau),
        };
        // The update matrix U = F₂₂ − L₂₁ D L₂₁ᵀ (lower) for the parent: the trailing block.
        let mu = m - k;
        let update = (mu > 0).then(|| {
            let mut u = Vec::with_capacity(mu * (mu + 1) / 2);
            for j in 0..mu {
                u.extend_from_slice(&f[(k + j) * m + k + j..(k + j + 1) * m]);
            }
            Update { rows: rows[k..].to_vec(), values: u }
        });
        // Keep the k factored columns only, in an allocation of their own (the front's m × m is
        // freed).
        let l = f[..k * m].to_vec();
        drop(f);
        let l = self.store.keep(l)?;
        let counts = Counts { perturbed: pc.perturbed, two_by_two: pc.two_by_two, entries: fill(k, m), largest_front: 0 };
        Ok(Eliminated { node: Supernode { rows, k, l, pivots }, update, counts })
    }
}

/// The equilibration S A S with each row's largest entry about one: s_i = 1/√(max_j |a_ij|).
fn symmetric_scaling(a: &Csr) -> Vec<f64> {
    (0..a.n)
        .map(|i| {
            let m = a.val[a.ptr[i]..a.ptr[i + 1]].iter().fold(0.0f64, |m, v| m.max(v.norm()));
            if m > 0.0 { 1.0 / m.sqrt() } else { 1.0 }
        })
        .collect()
}

/// The columns of `a` by position (`inv[unknown] = position`), lower part with the diagonal:
/// (row position, value).
fn lower_columns(a: &Csr, inv: &[usize]) -> Vec<Vec<(usize, C)>> {
    let mut a_cols: Vec<Vec<(usize, C)>> = vec![Vec::new(); a.n];
    for u in 0..a.n {
        let pu = inv[u];
        for (&v, &x) in a.col[a.ptr[u]..a.ptr[u + 1]].iter().zip(&a.val[a.ptr[u]..a.ptr[u + 1]]) {
            let pv = inv[v];
            if pu >= pv {
                a_cols[pv].push((pu, x));
            }
        }
    }
    a_cols
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
        check(a, an)?;
        // Equilibrate, so the static-pivot threshold below means the same for every unknown.
        // (Graded meshes mix elements whose matrices differ by many orders; against the largest
        // entry of the whole matrix, the threshold would replace the pivots of every small one.)
        let scale = symmetric_scaling(a);
        let mut scaled = a.clone();
        for i in 0..scaled.n {
            for k in scaled.ptr[i]..scaled.ptr[i + 1] {
                scaled.val[k] *= scale[i] * scale[scaled.col[k]];
            }
        }
        let tau = f64::EPSILON.sqrt() * scaled.max_abs();
        let kernel = Kernel { a_cols: lower_columns(&scaled, &an.inv), tau, store: Store::new(budget) };
        drop(scaled);
        let (supernodes, counts) = factor_tree(&kernel, an)?;
        let scratch = kernel.store.into_scratch()?;
        let bytes_on_disk = scratch.as_ref().map_or(0, |s| s.end);
        Ok(Ldlt {
            numeric_seconds: t0.elapsed().as_secs_f64(),
            analysis_seconds: an.seconds,
            n: a.n,
            perm: an.perm.clone(),
            supernodes,
            scratch: scratch.map(Arc::new),
            bytes_on_disk,
            scale,
            perturbed: counts.perturbed,
            two_by_two: counts.two_by_two,
            entries: counts.entries,
            largest_front: counts.largest_front,
        })
    }

    pub fn n(&self) -> usize {
        self.n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accel::Accelerator;
    use crate::multifrontal::testing::{Rng, grid_pattern};

    /// A random complex symmetric matrix on a 3D grid graph (7-point stencil), points per node.
    fn grid_matrix(nx: usize, ny: usize, nz: usize, diag: C, rng: &mut Rng) -> (Csr, Vec<Point3>) {
        let (mut a, edges, xyz) = grid_pattern(nx, ny, nz);
        for g in &edges {
            let v = rng.c();
            a.add(g[0], g[1], v);
            a.add(g[1], g[0], v);
        }
        for i in 0..a.n {
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

    /// A device held on the host, doing what `DeviceFront` documents in the plainest way.
    struct HostDevice(std::sync::atomic::AtomicUsize);

    struct HostFront {
        f: Vec<C>,
        m: usize,
    }

    impl Accelerator for HostDevice {
        fn name(&self) -> String {
            "host".into()
        }
        fn takes(&self, m: usize, _: usize) -> bool {
            m >= 40
        }
        fn front(&self, f: &[C], m: usize) -> Result<Box<dyn crate::accel::DeviceFront>, String> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(Box::new(HostFront { f: f.to_vec(), m }))
        }
    }

    impl crate::accel::DeviceFront for HostFront {
        fn read(&mut self, cols: Range<usize>, out: &mut [C]) -> Result<(), String> {
            out.copy_from_slice(&self.f[cols.start * self.m..cols.end * self.m]);
            Ok(())
        }
        fn swap_rows(&mut self, cols: Range<usize>, swaps: &[(usize, usize)]) -> Result<(), String> {
            for c in cols {
                for &(p, q) in swaps {
                    self.f.swap(c * self.m + p, c * self.m + q);
                }
            }
            Ok(())
        }
        fn update(&mut self, be: usize, k: usize, w: &[C], l: &[C]) -> Result<(), String> {
            let (m, below) = (self.m, self.m - be);
            let nb = w.len() / below;
            for j in be..m {
                for i in (if j < k { be } else { j })..m {
                    let sum = (0..nb).fold(C::new(0.0, 0.0), |s, t| s + w[t * below + i - be] * l[t * below + j - be]);
                    self.f[j * m + i] -= sum;
                }
            }
            Ok(())
        }
    }

    #[test]
    fn a_device_holding_the_large_fronts_factors_alike() {
        let mut rng = Rng(7);
        for diag in [C::new(4.0, 0.0), C::new(0.0, 0.0)] {
            let (a, xyz) = grid_matrix(9, 8, 7, diag, &mut rng);
            let x_true: Vec<C> = (0..a.n).map(|_| rng.c()).collect();
            let b = a.mul(&x_true);
            let host = Ldlt::factor(&a, &xyz).unwrap();
            let device = std::sync::Arc::new(HostDevice(Default::default()));
            let on = crate::accel::with(Some(device.clone()), || Ldlt::factor(&a, &xyz)).unwrap();
            assert!(device.0.load(std::sync::atomic::Ordering::Relaxed) > 0);
            assert_eq!((on.perturbed, on.two_by_two), (host.perturbed, host.two_by_two));
            let (mut x, mut y) = (b.clone(), b.clone());
            host.solve(&mut x).unwrap();
            on.solve(&mut y).unwrap();
            let gap = x.iter().zip(&y).fold(0.0f64, |m, (p, q)| m.max((p - q).norm()));
            assert!(residual(&a, &y, &b) < 1e-11 && gap < 1e-9, "{gap:e}");
        }
        // Outside `with`, no device.
        assert!(crate::accel::current().is_none());
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

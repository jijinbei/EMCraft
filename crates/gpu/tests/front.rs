//! The factorization's large fronts held by a device against the CPU's factor: the same
//! solution to rounding, the same factor twice.

use std::sync::Arc;
use std::time::Instant;

use emcraft_gpu::{Backend, Fronts, available};
use emcraft_model::Point3;
use emcraft_solver::Complex64 as C;
use emcraft_solver::accel;
use emcraft_solver::ldlt::Ldlt;
use emcraft_solver::sparse::Csr;

/// A complex symmetric matrix on a 3D grid graph (7-point stencil) with pseudo-random entries
/// and `diag` on the diagonal, a point per unknown.
fn grid(n: [usize; 3], diag: C) -> (Csr, Vec<Point3>) {
    let [nx, ny, nz] = n;
    let id = |i: usize, j: usize, k: usize| i + nx * (j + ny * k);
    let mut seed = 0x9e3779b97f4a7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    };
    let (mut groups, mut xyz) = (Vec::new(), vec![Point3::ORIGIN; nx * ny * nz]);
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                xyz[id(i, j, k)] = Point3::new(i as f64, j as f64, k as f64);
                for (di, dj, dk) in [(1, 0, 0), (0, 1, 0), (0, 0, 1)] {
                    if i + di < nx && j + dj < ny && k + dk < nz {
                        groups.push(vec![id(i, j, k), id(i + di, j + dj, k + dk)]);
                    }
                }
            }
        }
    }
    let mut a = Csr::from_groups(xyz.len(), groups.iter().map(|g| g.as_slice()));
    for g in &groups {
        let v = C::new(next(), next());
        a.add(g[0], g[1], v);
        a.add(g[1], g[0], v);
    }
    for i in 0..xyz.len() {
        let v = diag + C::new(next(), next()) * 0.1;
        a.add(i, i, v);
    }
    (a, xyz)
}

/// The solutions of the CPU's factor and of the device's for one right-hand side, the largest
/// difference relative to the solution, and the device's solution.
fn compare(a: &Csr, xyz: &[Point3], device: Arc<Fronts>) -> (f64, Vec<C>) {
    let b: Vec<C> = (0..a.n).map(|i| C::new((i % 7) as f64 - 3.0, 1.0)).collect();
    let t = Instant::now();
    let host = Ldlt::factor(a, xyz).unwrap();
    let t_host = t.elapsed();
    let t = Instant::now();
    let on = accel::with(Some(device), || Ldlt::factor(a, xyz)).unwrap();
    eprintln!("{} unknowns, largest front {}: CPU {t_host:.2?}, device {:.2?}", a.n, host.largest_front, t.elapsed());
    let (mut x, mut y) = (b.clone(), b);
    host.solve(&mut x).unwrap();
    on.solve(&mut y).unwrap();
    let scale = x.iter().fold(0.0f64, |m, v| m.max(v.norm()));
    (x.iter().zip(&y).fold(0.0f64, |m, (p, q)| m.max((p - q).norm())) / scale, y)
}

#[test]
fn fronts_on_the_cpu_backend_factor_alike() {
    // Fronts from 24 rows to the device, here CubeCL's CPU runtime (slow: a small grid, an
    // indefinite diagonal for 2 × 2 pivots and interchanges).
    let device = Arc::new(Fronts::new(Backend::Cpu, Some(24)).unwrap());
    let (a, xyz) = grid([6, 5, 5], C::new(0.0, 0.0));
    let (gap, _) = compare(&a, &xyz, device);
    assert!(gap < 1e-9, "{gap:e}");
}

#[test]
#[ignore = "needs an NVIDIA GPU: cargo test -p emcraft-gpu --features cuda -- --ignored"]
fn fronts_on_cuda_factor_alike() {
    let cuda = available().into_iter().find(|a| a.backend == Backend::Cuda).unwrap();
    assert!(cuda.problem.is_none(), "{cuda:?}");
    let device = Arc::new(Fronts::new(Backend::Cuda, None).unwrap());
    let n = std::env::var("EMCRAFT_GRID").ok().and_then(|v| v.parse().ok()).unwrap_or(36);
    let (a, xyz) = grid([n, n, n], C::new(0.5, 0.1));
    let (gap, first) = compare(&a, &xyz, device.clone());
    assert!(gap < 1e-9, "{gap:e}");
    let (_, again) = compare(&a, &xyz, device);
    assert_eq!(first, again);
}

/// A dense complex symmetric matrix of order n (one front), diagonally dominant.
fn dense(n: usize) -> (Csr, Vec<Point3>) {
    let all: Vec<usize> = (0..n).collect();
    let mut a = Csr::from_groups(n, std::iter::once(all.as_slice()));
    for i in 0..n {
        for j in 0..=i {
            let v = if i == j { C::new(n as f64, 1.0) } else { C::new(((i * 7 + j * 13) % 17) as f64 / 17.0 - 0.5, ((i + j) % 5) as f64 / 5.0) };
            a.add(i, j, v);
            if i != j {
                a.add(j, i, v);
            }
        }
    }
    (a, (0..n).map(|i| Point3::new(i as f64, 0.0, 0.0)).collect())
}

#[test]
#[ignore = "benchmark, needs an NVIDIA GPU: EMCRAFT_DENSE=3000 cargo test --release -p emcraft-gpu --features cuda --test front dense -- --ignored --nocapture"]
fn a_dense_front_rate() {
    let n = std::env::var("EMCRAFT_DENSE").ok().and_then(|v| v.parse().ok()).unwrap_or(2000);
    let (a, xyz) = dense(n);
    let device = Arc::new(Fronts::new(Backend::Cuda, None).unwrap());
    for _ in 0..2 {
        let (gap, _) = compare(&a, &xyz, device.clone());
        assert!(gap < 1e-9, "{gap:e}");
    }
    // Complex LDLᵀ: about 8n³/3 real flops.
    eprintln!("{:.0} GFLOP per factorization", 8.0 * (n as f64).powi(3) / 3.0 / 1e9);
}

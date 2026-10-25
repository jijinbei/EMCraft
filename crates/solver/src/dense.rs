//! Eigenvalues and eigenvectors of small dense real matrices that need not be symmetric: the
//! projected problems of the port mode solver ([`crate::portmode`]). Householder reduction to
//! upper Hessenberg form, then the implicitly shifted (Francis double-shift) QR algorithm with
//! deflation (G. H. Golub, C. F. Van Loan, *Matrix Computations*, 4th ed., Johns Hopkins 2013,
//! §7.4–7.5, Algorithms 7.4.2, 7.5.1, 7.5.2); eigenvectors of real eigenvalues by inverse
//! iteration (§7.6.1), each kept orthogonal to those of eigenvalues it cannot be told from.
//!
//! Matrices are row-major `n × n` slices.

/// Iterations of the QR algorithm per eigenvalue before it gives up.
const MAX_SWEEPS: usize = 60;

/// A real eigenvalue with its unit eigenvector, or a complex pair's one member.
#[derive(Clone, Debug, PartialEq)]
pub struct Eigen {
    pub re: f64,
    pub im: f64,
    /// For a real eigenvalue, its unit eigenvector; empty for a complex one.
    pub vector: Vec<f64>,
}

/// The eigenvalues of `a` (n × n), with eigenvectors for the real ones; `None` when the QR
/// algorithm does not converge or the input is not finite.
pub fn eigen(a: &[f64], n: usize) -> Option<Vec<Eigen>> {
    if a.len() != n * n || !a.iter().all(|x| x.is_finite()) {
        return None;
    }
    let values = eigenvalues(a.to_vec(), n)?;
    let scale = a.iter().fold(0.0f64, |m, x| m.max(x.abs())).max(f64::MIN_POSITIVE);
    let mut found: Vec<Eigen> = Vec::with_capacity(n);
    for (re, im) in values {
        if im != 0.0 {
            found.push(Eigen { re, im, vector: Vec::new() });
            continue;
        }
        // Vectors of eigenvalues within rounding of this one: the new one is kept apart from them.
        let cluster: Vec<&[f64]> = found.iter().filter(|e| e.im == 0.0 && (e.re - re).abs() <= 1e-8 * scale).map(|e| e.vector.as_slice()).collect();
        let vector = inverse_iteration(a, n, re, &cluster, scale)?;
        found.push(Eigen { re, im, vector });
    }
    Some(found)
}

/// The eigenvalues (re, im) of `a`, in no particular order.
fn eigenvalues(mut h: Vec<f64>, n: usize) -> Option<Vec<(f64, f64)>> {
    hessenberg(&mut h, n);
    let at = |i: usize, j: usize| i * n + j;
    let mut out = Vec::with_capacity(n);
    let norm = h.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    let mut hi = n;
    let mut sweeps = 0;
    while hi > 0 {
        let last = hi - 1;
        if last == 0 {
            out.push((h[at(0, 0)], 0.0));
            break;
        }
        // Deflate the negligible subdiagonal entries of the active part.
        for k in 1..=last {
            // Relative to its neighbours on the diagonal, and absolutely to the matrix (what
            // lies below a rounding of the largest entry is zero for our purposes).
            let s = (h[at(k, k)].abs() + h[at(k - 1, k - 1)].abs()).max(1e-3 * norm);
            if h[at(k, k - 1)].abs() <= f64::EPSILON * s {
                h[at(k, k - 1)] = 0.0;
            }
        }
        let lo = (1..=last).rev().find(|&k| h[at(k, k - 1)] == 0.0).unwrap_or(0);
        if lo == last {
            out.push((h[at(last, last)], 0.0));
            hi -= 1;
            sweeps = 0;
            continue;
        }
        if lo + 1 == last {
            let (a, b, c, d) = (h[at(lo, lo)], h[at(lo, last)], h[at(last, lo)], h[at(last, last)]);
            let (m, disc) = (0.5 * (a + d), 0.25 * (a - d) * (a - d) + b * c);
            if disc >= 0.0 {
                let r = disc.sqrt();
                // The root of larger magnitude directly, the other from the determinant.
                let big = if m >= 0.0 { m + r } else { m - r };
                let small = if big != 0.0 { (a * d - b * c) / big } else { 0.0 };
                out.push((big, 0.0));
                out.push((small, 0.0));
            } else {
                let r = (-disc).sqrt();
                out.push((m, r));
                out.push((m, -r));
            }
            hi -= 2;
            sweeps = 0;
            continue;
        }
        sweeps += 1;
        if sweeps > MAX_SWEEPS {
            return None;
        }
        // The shifts: the trailing 2 × 2 block's eigenvalues, or now and then an exceptional pair.
        let (s, t) = if sweeps % 11 == 0 {
            let e = h[at(last, last - 1)].abs() + h[at(last - 1, last - 2)].abs();
            let x = h[at(last, last)] + 0.75 * e;
            (2.0 * x, x * x - 0.4375 * e * e)
        } else {
            let (a, b, c, d) = (h[at(last - 1, last - 1)], h[at(last - 1, last)], h[at(last, last - 1)], h[at(last, last)]);
            (a + d, a * d - b * c)
        };
        francis_step(&mut h, n, lo, last, s, t);
    }
    Some(out)
}

/// One Francis double-shift QR step on the active block `lo..=hi` of the Hessenberg matrix `h`,
/// with shifts whose sum is `s` and product `t`.
fn francis_step(h: &mut [f64], n: usize, lo: usize, hi: usize, s: f64, t: f64) {
    let at = |i: usize, j: usize| i * n + j;
    let mut x = h[at(lo, lo)] * h[at(lo, lo)] + h[at(lo, lo + 1)] * h[at(lo + 1, lo)] - s * h[at(lo, lo)] + t;
    let mut y = h[at(lo + 1, lo)] * (h[at(lo, lo)] + h[at(lo + 1, lo + 1)] - s);
    let mut z = h[at(lo + 2, lo + 1)] * h[at(lo + 1, lo)];
    for k in lo..hi - 1 {
        let Some(v) = householder3(x, y, z) else { break };
        let first = if k > lo { k - 1 } else { lo };
        // P from the left on rows k..k+3, from the right on columns k..k+3.
        for j in first..=hi {
            let d = v[0] * h[at(k, j)] + v[1] * h[at(k + 1, j)] + v[2] * h[at(k + 2, j)];
            h[at(k, j)] -= 2.0 * v[0] * d;
            h[at(k + 1, j)] -= 2.0 * v[1] * d;
            h[at(k + 2, j)] -= 2.0 * v[2] * d;
        }
        let last = (k + 3).min(hi);
        for i in lo..=last {
            let d = v[0] * h[at(i, k)] + v[1] * h[at(i, k + 1)] + v[2] * h[at(i, k + 2)];
            h[at(i, k)] -= 2.0 * v[0] * d;
            h[at(i, k + 1)] -= 2.0 * v[1] * d;
            h[at(i, k + 2)] -= 2.0 * v[2] * d;
        }
        x = h[at(k + 1, k)];
        y = h[at(k + 2, k)];
        z = if k + 3 <= hi { h[at(k + 3, k)] } else { 0.0 };
    }
    // The last rotation, on rows and columns hi − 1, hi.
    let r = x.hypot(y);
    if r > 0.0 {
        let (c, sn) = (x / r, y / r);
        let k = hi - 1;
        for j in (k - 1).max(lo)..=hi {
            let (p, q) = (h[at(k, j)], h[at(k + 1, j)]);
            h[at(k, j)] = c * p + sn * q;
            h[at(k + 1, j)] = -sn * p + c * q;
        }
        for i in lo..=hi {
            let (p, q) = (h[at(i, k)], h[at(i, k + 1)]);
            h[at(i, k)] = c * p + sn * q;
            h[at(i, k + 1)] = -sn * p + c * q;
        }
    }
}

/// The unit vector v of the reflection I − 2vvᵀ taking (x, y, z) onto the first axis; `None`
/// for the zero vector.
fn householder3(x: f64, y: f64, z: f64) -> Option<[f64; 3]> {
    let norm = (x * x + y * y + z * z).sqrt();
    if norm == 0.0 {
        return None;
    }
    let v0 = x + if x >= 0.0 { norm } else { -norm };
    let len = (v0 * v0 + y * y + z * z).sqrt();
    Some([v0 / len, y / len, z / len])
}

/// Reduces `a` to upper Hessenberg form by Householder reflections (a similarity).
fn hessenberg(a: &mut [f64], n: usize) {
    let at = |i: usize, j: usize| i * n + j;
    for k in 0..n.saturating_sub(2) {
        let x: Vec<f64> = (k + 1..n).map(|i| a[at(i, k)]).collect();
        let norm = x.iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm == 0.0 {
            continue;
        }
        let mut v = x;
        v[0] += if v[0] >= 0.0 { norm } else { -norm };
        let len = v.iter().map(|c| c * c).sum::<f64>().sqrt();
        v.iter_mut().for_each(|c| *c /= len);
        for j in 0..n {
            let d: f64 = v.iter().enumerate().map(|(r, vr)| vr * a[at(k + 1 + r, j)]).sum();
            v.iter().enumerate().for_each(|(r, vr)| a[at(k + 1 + r, j)] -= 2.0 * vr * d);
        }
        for i in 0..n {
            let d: f64 = v.iter().enumerate().map(|(c, vc)| vc * a[at(i, k + 1 + c)]).sum();
            v.iter().enumerate().for_each(|(c, vc)| a[at(i, k + 1 + c)] -= 2.0 * vc * d);
        }
        for i in k + 2..n {
            a[at(i, k)] = 0.0;
        }
    }
}

/// A unit eigenvector of `a` for the real eigenvalue `theta`, orthogonal to `apart`: inverse
/// iteration with (a − θI) factored once, the shift nudged off θ by a rounding's worth.
fn inverse_iteration(a: &[f64], n: usize, theta: f64, apart: &[&[f64]], scale: f64) -> Option<Vec<f64>> {
    let shift = theta + 1e-12 * scale;
    let mut m = a.to_vec();
    for i in 0..n {
        m[i * n + i] -= shift;
    }
    let lu = Lu::new(m, n, 1e-15 * scale)?;
    // A deterministic start with every component.
    let mut x: Vec<f64> = (0..n).map(|i| 1.0 + 0.37 * ((i * 7919 % 101) as f64 / 101.0)).collect();
    for _ in 0..4 {
        for v in apart {
            let d: f64 = x.iter().zip(v.iter()).map(|(p, q)| p * q).sum();
            x.iter_mut().zip(v.iter()).for_each(|(p, q)| *p -= d * q);
        }
        x = lu.solve(&x);
        for v in apart {
            let d: f64 = x.iter().zip(v.iter()).map(|(p, q)| p * q).sum();
            x.iter_mut().zip(v.iter()).for_each(|(p, q)| *p -= d * q);
        }
        let len = x.iter().map(|c| c * c).sum::<f64>().sqrt();
        if !(len > 0.0 && len.is_finite()) {
            return None;
        }
        x.iter_mut().for_each(|c| *c /= len);
    }
    Some(x)
}

/// A dense LU factorization with partial pivoting; tiny pivots are replaced by `floor` (inverse
/// iteration factors a nearly singular matrix on purpose).
pub(crate) struct Lu {
    n: usize,
    a: Vec<f64>,
    perm: Vec<usize>,
}

impl Lu {
    pub(crate) fn new(mut a: Vec<f64>, n: usize, floor: f64) -> Option<Lu> {
        if a.len() != n * n {
            return None;
        }
        let mut perm: Vec<usize> = (0..n).collect();
        for k in 0..n {
            let p = (k..n).max_by(|&i, &j| a[i * n + k].abs().total_cmp(&a[j * n + k].abs()))?;
            if p != k {
                for j in 0..n {
                    a.swap(k * n + j, p * n + j);
                }
                perm.swap(k, p);
            }
            if a[k * n + k].abs() < floor {
                a[k * n + k] = if a[k * n + k] < 0.0 { -floor } else { floor };
            }
            if a[k * n + k] == 0.0 {
                return None;
            }
            for i in k + 1..n {
                let f = a[i * n + k] / a[k * n + k];
                a[i * n + k] = f;
                for j in k + 1..n {
                    a[i * n + j] -= f * a[k * n + j];
                }
            }
        }
        Some(Lu { n, a, perm })
    }

    pub(crate) fn solve(&self, b: &[f64]) -> Vec<f64> {
        let n = self.n;
        let mut x: Vec<f64> = self.perm.iter().map(|&p| b.get(p).copied().unwrap_or(0.0)).collect();
        for i in 0..n {
            let s: f64 = (0..i).map(|j| self.a[i * n + j] * x[j]).sum();
            x[i] -= s;
        }
        for i in (0..n).rev() {
            let s: f64 = (i + 1..n).map(|j| self.a[i * n + j] * x[j]).sum();
            x[i] = (x[i] - s) / self.a[i * n + i];
        }
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic pseudo-random sequence in [−½, ½).
    fn rng(seed: u64) -> impl FnMut() -> f64 {
        let mut s = seed;
        move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        }
    }

    /// V diag(d) V⁻¹ for a random V.
    fn similar(d: &[f64], seed: u64) -> Vec<f64> {
        let n = d.len();
        let mut r = rng(seed);
        let v: Vec<f64> = (0..n * n).map(|k| r() + if k % (n + 1) == 0 { 2.0 } else { 0.0 }).collect();
        let lu = Lu::new(v.clone(), n, 0.0).unwrap();
        // Columns of V⁻¹ from unit vectors, then A = V D V⁻¹.
        let inv: Vec<Vec<f64>> = (0..n).map(|j| lu.solve(&(0..n).map(|i| if i == j { 1.0 } else { 0.0 }).collect::<Vec<_>>())).collect();
        let mut a = vec![0.0; n * n];
        for i in 0..n {
            for j in 0..n {
                a[i * n + j] = (0..n).map(|k| v[i * n + k] * d[k] * inv[j][k]).sum();
            }
        }
        a
    }

    fn check(a: &[f64], n: usize, expected: &[f64]) {
        let e = eigen(a, n).unwrap();
        let mut got: Vec<f64> = e.iter().map(|x| x.re).collect();
        got.sort_by(f64::total_cmp);
        let mut want = expected.to_vec();
        want.sort_by(f64::total_cmp);
        for (g, w) in got.iter().zip(&want) {
            assert!((g - w).abs() < 1e-9 * (1.0 + w.abs()), "{got:?} {want:?}");
        }
        for x in &e {
            assert_eq!(x.im, 0.0);
            let r: f64 = (0..n).map(|i| ((0..n).map(|j| a[i * n + j] * x.vector[j]).sum::<f64>() - x.re * x.vector[i]).powi(2)).sum::<f64>().sqrt();
            assert!(r < 1e-8, "{} {r:e}", x.re);
        }
    }

    #[test]
    fn real_spectra_are_found_with_their_vectors() {
        check(&similar(&[3.0, -1.5, 0.25, 7.0, -4.0, 1e-3], 7), 6, &[3.0, -1.5, 0.25, 7.0, -4.0, 1e-3]);
        let d: Vec<f64> = (0..24).map(|k| (k as f64 - 9.5) * 0.7 + 0.01 * k as f64 * k as f64).collect();
        check(&similar(&d, 11), 24, &d);
        // A repeated eigenvalue (a degenerate pair of modes): two independent vectors.
        let a = similar(&[2.0, 2.0, -1.0, 5.0], 3);
        check(&a, 4, &[2.0, 2.0, -1.0, 5.0]);
        let e = eigen(&a, 4).unwrap();
        let pair: Vec<&Eigen> = e.iter().filter(|x| (x.re - 2.0).abs() < 1e-6).collect();
        let d: f64 = pair[0].vector.iter().zip(&pair[1].vector).map(|(p, q)| p * q).sum();
        assert!(d.abs() < 1e-6, "{d}");
    }

    #[test]
    fn complex_pairs_and_hostile_input() {
        // A rotation by 90° scaled by 2, and a real eigenvalue 3.
        let a = [0.0, -2.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 3.0];
        let e = eigen(&a, 3).unwrap();
        let mut im: Vec<f64> = e.iter().map(|x| x.im).collect();
        im.sort_by(f64::total_cmp);
        assert!((im[0] + 2.0).abs() < 1e-12 && im[1] == 0.0 && (im[2] - 2.0).abs() < 1e-12);
        assert!(eigen(&[f64::NAN], 1).is_none());
        assert!(eigen(&[1.0, 2.0], 2).is_none());
        assert_eq!(eigen(&[4.0], 1).unwrap()[0].re, 4.0);
    }
}

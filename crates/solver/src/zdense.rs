//! Eigenvalues and eigenvectors of small dense complex matrices: the projected problems of the
//! lossy cavity eigensolver ([`crate::eigen`]). Householder reduction to upper Hessenberg
//! form, then the shifted QR algorithm on it with Givens rotations, the Wilkinson shift and
//! deflation (G. H. Golub, C. F. Van Loan, *Matrix Computations*, 4th ed., Johns Hopkins 2013,
//! §7.4–7.5, the complex single-shift variant; a trailing 2 × 2 block that will not deflate — a
//! degenerate pair coupled by rounding — solved directly); eigenvectors by inverse iteration on the
//! original matrix (§7.6.1).
//!
//! Matrices are row-major `n × n` slices; [`solve`] takes a small dense system by rows (the
//! reduced systems of the fast sweep, the eigensolver's projected pencil, S renormalised).

use num_complex::Complex64 as C;

/// Solves a small dense complex system by Gaussian elimination with partial pivoting; `None`
/// when it is singular.
pub(crate) fn solve(mut a: Vec<Vec<C>>, mut b: Vec<C>) -> Option<Vec<C>> {
    let n = b.len();
    for k in 0..n {
        let piv = (k..n).max_by(|&i, &j| a[i][k].norm().total_cmp(&a[j][k].norm()))?;
        if a[piv][k].norm() == 0.0 {
            return None;
        }
        a.swap(k, piv);
        b.swap(k, piv);
        for i in k + 1..n {
            let f = a[i][k] / a[k][k];
            for j in k..n {
                let x = a[k][j];
                a[i][j] -= f * x;
            }
            let x = b[k];
            b[i] -= f * x;
        }
    }
    let mut x = vec![C::new(0.0, 0.0); n];
    for i in (0..n).rev() {
        let s: C = (i + 1..n).map(|j| a[i][j] * x[j]).sum();
        x[i] = (b[i] - s) / a[i][i];
    }
    Some(x)
}

/// QR sweeps per eigenvalue before giving up.
const MAX_SWEEPS: usize = 80;
/// Sweeps on a trailing 2 × 2 block before its eigenvalues are taken directly.
const STUCK_PAIR: usize = 20;

/// The eigenvalues of `a` (n × n) with unit eigenvectors; `None` when the input is not finite
/// or the QR algorithm does not converge.
pub fn eigen(a: &[C], n: usize) -> Option<Vec<(C, Vec<C>)>> {
    if a.len() != n * n || !a.iter().all(|x| x.re.is_finite() && x.im.is_finite()) {
        return None;
    }
    let values = eigenvalues(a.to_vec(), n)?;
    let scale = a.iter().fold(0.0f64, |m, x| m.max(x.norm())).max(f64::MIN_POSITIVE);
    values.into_iter().map(|l| Some((l, vector(a, n, l, scale)?))).collect()
}

/// A unit eigenvector of `a` for `lambda`, by three steps of inverse iteration with the shift
/// moved off the eigenvalue by a hair (so the system is solvable).
fn vector(a: &[C], n: usize, lambda: C, scale: f64) -> Option<Vec<C>> {
    let shift = lambda + C::new(1e-10 * scale, 1e-10 * scale);
    let m: Vec<Vec<C>> = (0..n).map(|i| (0..n).map(|j| a[i * n + j] - if i == j { shift } else { C::new(0.0, 0.0) }).collect()).collect();
    let mut v: Vec<C> = (0..n).map(|i| C::new(1.0 + 0.1 * i as f64, 0.05 * i as f64)).collect();
    for _ in 0..3 {
        v = solve(m.clone(), v)?;
        let norm = v.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
        if !(norm > 0.0 && norm.is_finite()) {
            return None;
        }
        v.iter_mut().for_each(|c| *c /= norm);
    }
    Some(v)
}

fn eigenvalues(mut h: Vec<C>, n: usize) -> Option<Vec<C>> {
    hessenberg(&mut h, n);
    let at = |h: &Vec<C>, i: usize, j: usize| h[i * n + j];
    let mut out = vec![C::new(0.0, 0.0); n];
    let mut hi = n;
    let mut sweeps = 0;
    while hi > 0 {
        // The active block ends at hi − 1; find where it starts.
        let mut lo = hi - 1;
        while lo > 0 {
            let s = at(&h, lo - 1, lo - 1).norm() + at(&h, lo, lo).norm();
            if at(&h, lo, lo - 1).norm() <= f64::EPSILON * s.max(f64::MIN_POSITIVE) {
                h[lo * n + lo - 1] = C::new(0.0, 0.0);
                break;
            }
            lo -= 1;
        }
        if lo == hi - 1 {
            out[hi - 1] = at(&h, hi - 1, hi - 1);
            hi -= 1;
            sweeps = 0;
            continue;
        }
        // A 2 × 2 block that will not deflate (a degenerate pair whose coupling is rounding,
        // which the shifts cannot shrink): its two eigenvalues directly.
        if lo + 2 == hi && sweeps >= STUCK_PAIR {
            let (a11, a12, a21, a22) = (at(&h, lo, lo), at(&h, lo, lo + 1), at(&h, lo + 1, lo), at(&h, lo + 1, lo + 1));
            let half = (a11 + a22) / 2.0;
            let disc = (((a11 - a22) / 2.0).powi(2) + a12 * a21).sqrt();
            (out[lo], out[lo + 1]) = (half + disc, half - disc);
            hi = lo;
            sweeps = 0;
            continue;
        }
        sweeps += 1;
        if sweeps > MAX_SWEEPS {
            return None;
        }
        // The Wilkinson shift: the trailing 2 × 2 block's eigenvalue nearer its corner, an
        // exceptional one now and then.
        let (p, q) = (hi - 2, hi - 1);
        let (a11, a12, a21, a22) = (at(&h, p, p), at(&h, p, q), at(&h, q, p), at(&h, q, q));
        let tr = a11 + a22;
        let det = a11 * a22 - a12 * a21;
        let disc = (tr * tr / 4.0 - det).sqrt();
        let (l1, l2) = (tr / 2.0 + disc, tr / 2.0 - disc);
        let mut mu = if (l1 - a22).norm() < (l2 - a22).norm() { l1 } else { l2 };
        if sweeps % 11 == 0 {
            mu += C::new(at(&h, q, p).norm(), 0.0);
        }
        // One QR step on the block lo..hi of H − μI with Givens rotations.
        for k in lo..hi {
            h[k * n + k] -= mu;
        }
        let mut rotations = Vec::with_capacity(hi - lo);
        for k in lo..hi - 1 {
            let (x, y) = (at(&h, k, k), at(&h, k + 1, k));
            let r = (x.norm_sqr() + y.norm_sqr()).sqrt();
            let (c, s) = if r == 0.0 { (C::new(1.0, 0.0), C::new(0.0, 0.0)) } else { (x / r, y / r) };
            // [c̄ s̄; −s c] applied to rows k, k+1.
            for j in k..n {
                let (u, v) = (at(&h, k, j), at(&h, k + 1, j));
                h[k * n + j] = c.conj() * u + s.conj() * v;
                h[(k + 1) * n + j] = -s * u + c * v;
            }
            rotations.push((c, s));
        }
        // R Q: the rotations' adjoints from the right on columns k, k+1.
        for (i, &(c, s)) in rotations.iter().enumerate() {
            let k = lo + i;
            for r in 0..(k + 2).min(hi) {
                let (u, v) = (at(&h, r, k), at(&h, r, k + 1));
                h[r * n + k] = u * c + v * s;
                h[r * n + k + 1] = -u * s.conj() + v * c.conj();
            }
        }
        for k in lo..hi {
            h[k * n + k] += mu;
        }
    }
    Some(out)
}

/// Reduces `a` in place to upper Hessenberg form by Householder reflections.
fn hessenberg(a: &mut [C], n: usize) {
    for k in 0..n.saturating_sub(2) {
        let x: Vec<C> = (k + 1..n).map(|i| a[i * n + k]).collect();
        let alpha = x.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
        if alpha == 0.0 {
            continue;
        }
        let phase = if x[0].norm() > 0.0 { x[0] / x[0].norm() } else { C::new(1.0, 0.0) };
        let mut v = x.clone();
        v[0] += phase * alpha;
        let vn = v.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
        if vn == 0.0 {
            continue;
        }
        v.iter_mut().for_each(|c| *c /= vn);
        // A ← (I − 2vv*) A (I − 2vv*), v on rows/columns k+1..n.
        for j in 0..n {
            let s: C = v.iter().enumerate().map(|(i, vi)| vi.conj() * a[(k + 1 + i) * n + j]).sum();
            for (i, vi) in v.iter().enumerate() {
                a[(k + 1 + i) * n + j] -= *vi * s * 2.0;
            }
        }
        for r in 0..n {
            let s: C = v.iter().enumerate().map(|(i, vi)| a[r * n + k + 1 + i] * vi).sum();
            for (i, vi) in v.iter().enumerate() {
                a[r * n + k + 1 + i] -= s * vi.conj() * 2.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// V D V⁻¹ for a fixed, well-conditioned complex V.
    fn similar(d: &[C]) -> Vec<C> {
        let n = d.len();
        let v: Vec<Vec<C>> = (0..n)
            .map(|i| {
                (0..n).map(|j| C::new(if i == j { 2.0 } else { 0.0 } + 0.1 * ((i * 7 + j * 3) % 5) as f64, 0.05 * ((i + 2 * j) % 3) as f64)).collect()
            })
            .collect();
        let inv: Vec<Vec<C>> =
            (0..n).map(|j| solve(v.clone(), (0..n).map(|i| C::new(if i == j { 1.0 } else { 0.0 }, 0.0)).collect()).unwrap()).collect();
        let mut a = vec![C::new(0.0, 0.0); n * n];
        for i in 0..n {
            for j in 0..n {
                a[i * n + j] = (0..n).map(|k| v[i][k] * d[k] * inv[j][k]).sum();
            }
        }
        a
    }

    #[test]
    fn complex_spectra_and_their_vectors() {
        let d: Vec<C> = (0..16).map(|k| C::new(k as f64 * 1.3 - 6.0, (k as f64 * 0.37).sin())).collect();
        let a = similar(&d);
        let e = eigen(&a, d.len()).unwrap();
        for want in &d {
            assert!(e.iter().any(|(l, _)| (l - want).norm() < 1e-9 * (1.0 + want.norm())), "{want}");
        }
        for (l, v) in &e {
            let n = d.len();
            let r: f64 = (0..n).map(|i| ((0..n).map(|j| a[i * n + j] * v[j]).sum::<C>() - l * v[i]).norm_sqr()).sum::<f64>().sqrt();
            assert!(r < 1e-7, "{l} {r:e}");
        }
        assert!(eigen(&[C::new(f64::NAN, 0.0)], 1).is_none());
        assert!(eigen(&[C::new(1.0, 0.0)], 2).is_none());
        assert_eq!(eigen(&[C::new(3.0, -1.0)], 1).unwrap()[0].0, C::new(3.0, -1.0));
    }

    #[test]
    fn a_degenerate_pair_converges() {
        // A double eigenvalue of a diagonalisable matrix: its 2 × 2 block's coupling is rounding
        // the shifts cannot shrink; the block's eigenvalues are taken directly.
        let d: Vec<C> = [3e-5, 3e-5, 1e-5, -2e-6, 5e-7].iter().map(|x| C::new(*x, -0.08 * x)).collect();
        let e = eigen(&similar(&d), d.len()).unwrap();
        for want in &d {
            assert!(e.iter().any(|(l, _)| (l - want).norm() < 1e-9 * 3e-5), "{want} {e:?}");
        }
    }
}

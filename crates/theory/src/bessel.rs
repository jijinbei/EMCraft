//! Bessel functions and their zeros, for the resonances of cylindrical and spherical cavities.
//!
//! J_n(x) of integer order by Miller's backward recurrence normalised with
//! J₀ + 2ΣJ_{2k} = 1 (M. Abramowitz, I. A. Stegun, *Handbook of Mathematical Functions*, 1964,
//! §9.12; the algorithm of J. C. P. Miller in the British Association Mathematical Tables,
//! 1952); J_n' = (J_{n−1} − J_{n+1})/2. Spherical Bessel functions j_n(x) from j₀ = sin x/x,
//! j₁ = sin x/x² − cos x/x and the upward recurrence j_{n+1} = (2n+1)/x j_n − j_{n−1} (stable for
//! x > n, where the zeros used here lie). Zeros are bracketed on a fine grid and bisected to the
//! last bit.

use std::f64::consts::PI;

use emcraft_model::units::C0;

/// J_0 … J_{n+1} at x > 0.
fn bessel_all(n: usize, x: f64) -> Vec<f64> {
    let top = 2 * ((n as f64 + x).ceil() as usize + 20);
    let top = top + top % 2;
    let mut j = vec![0.0; top + 2];
    j[top + 1] = 0.0;
    j[top] = 1e-30;
    for k in (1..=top).rev() {
        j[k - 1] = 2.0 * k as f64 / x * j[k] - j[k + 1];
        // Rescale against overflow.
        if j[k - 1].abs() > 1e250 {
            for v in j.iter_mut().skip(k - 1) {
                *v *= 1e-250;
            }
        }
    }
    let norm = j[0] + 2.0 * j.iter().skip(2).step_by(2).sum::<f64>();
    j.truncate(n + 2);
    j.iter().map(|v| v / norm).collect()
}

/// J_n(x).
pub fn j(n: usize, x: f64) -> f64 {
    if x == 0.0 {
        return if n == 0 { 1.0 } else { 0.0 };
    }
    bessel_all(n, x.abs())[n] * if x < 0.0 && n % 2 == 1 { -1.0 } else { 1.0 }
}

/// J_n'(x).
pub fn jp(n: usize, x: f64) -> f64 {
    if n == 0 {
        return -j(1, x);
    }
    0.5 * (j(n - 1, x) - j(n + 1, x))
}

/// The spherical Bessel function j_n(x), x > 0.
pub fn sph_j(n: usize, x: f64) -> f64 {
    let (s, c) = x.sin_cos();
    let mut a = s / x;
    if n == 0 {
        return a;
    }
    let mut b = s / (x * x) - c / x;
    for k in 1..n {
        let next = (2 * k + 1) as f64 / x * b - a;
        a = b;
        b = next;
    }
    b
}

/// d/dx [x j_n(x)] = x j_{n−1}(x) − n j_n(x).
pub fn sph_xj_prime(n: usize, x: f64) -> f64 {
    if n == 0 {
        return x.cos();
    }
    x * sph_j(n - 1, x) - n as f64 * sph_j(n, x)
}

/// The first `count` positive zeros of `f` in (0, x_max].
pub fn zeros(f: impl Fn(f64) -> f64, count: usize, x_max: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let step = 1e-3;
    let mut a = 1e-6;
    let mut fa = f(a);
    while out.len() < count && a < x_max {
        let b = a + step;
        let fb = f(b);
        if fa == 0.0 || fa * fb < 0.0 {
            let (mut lo, mut hi, mut flo) = (a, b, fa);
            for _ in 0..200 {
                let mid = 0.5 * (lo + hi);
                if mid <= lo || mid >= hi {
                    break;
                }
                let fm = f(mid);
                if (fm < 0.0) == (flo < 0.0) {
                    lo = mid;
                    flo = fm;
                } else {
                    hi = mid;
                }
            }
            out.push(0.5 * (lo + hi));
        }
        a = b;
        fa = fb;
    }
    out
}

/// The resonances of a cylindrical cavity of radius a and length d filled with εr, lowest first,
/// each degenerate pair listed twice (n ≥ 1): TM_nmp, f = c/(2π√εr) √((x_nm/a)² + (pπ/d)²) with
/// x_nm the m-th zero of J_n, p ≥ 0; TE_nmp with the zeros x'_nm of J_n', p ≥ 1 (D. M. Pozar,
/// *Microwave Engineering*, 4th ed., §6.4).
pub fn cylinder_cavity_resonances(a: f64, d: f64, eps_r: f64, count: usize) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    let v = C0 / (2.0 * PI * eps_r.sqrt());
    for n in 0..6 {
        let tm = zeros(|x| j(n, x), 4, 30.0);
        let te = zeros(|x| jp(n, x), 4, 30.0);
        let copies = if n == 0 { 1 } else { 2 };
        for (m, x) in tm.iter().enumerate() {
            for p in 0..5 {
                let f = v * ((x / a).powi(2) + (p as f64 * PI / d).powi(2)).sqrt();
                for _ in 0..copies {
                    out.push((f, format!("TM{n}{}{p}", m + 1)));
                }
            }
        }
        // J₀' vanishes at 0 too: that root is not a mode.
        for (m, x) in te.iter().filter(|x| **x > 1e-3).enumerate() {
            for p in 1..5 {
                let f = v * ((x / a).powi(2) + (p as f64 * PI / d).powi(2)).sqrt();
                for _ in 0..copies {
                    out.push((f, format!("TE{n}{}{p}", m + 1)));
                }
            }
        }
    }
    out.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
    out.truncate(count);
    out
}

/// The resonances of a spherical cavity of radius a filled with εr, lowest first, each listed
/// 2n + 1 times: TM_np with k a the p-th zero of [x j_n(x)]', TE_np with the zeros of j_n
/// (R. F. Harrington, *Time-Harmonic Electromagnetic Fields*, McGraw-Hill 1961, §6.10).
pub fn sphere_cavity_resonances(a: f64, eps_r: f64, count: usize) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    let v = C0 / (2.0 * PI * eps_r.sqrt() * a);
    for n in 1..6 {
        // The upward recurrence is unstable below x = n, where these functions have no zeros.
        let above = |v: Vec<f64>| v.into_iter().filter(|x| *x > n as f64).take(3).collect::<Vec<f64>>();
        for (p, x) in above(zeros(|x| sph_xj_prime(n, x), 8, 25.0)).iter().enumerate() {
            for _ in 0..2 * n + 1 {
                out.push((v * x, format!("TM{n}{}", p + 1)));
            }
        }
        for (p, x) in above(zeros(|x| sph_j(n, x), 8, 25.0)).iter().enumerate() {
            for _ in 0..2 * n + 1 {
                out.push((v * x, format!("TE{n}{}", p + 1)));
            }
        }
    }
    out.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
    out.truncate(count);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabulated_zeros() {
        // Abramowitz & Stegun, Tables 9.5 and 10.6.
        let z0 = zeros(|x| j(0, x), 3, 20.0);
        assert!((z0[0] - 2.404825557695773).abs() < 1e-12);
        assert!((z0[1] - 5.520078110286311).abs() < 1e-12);
        assert!((zeros(|x| j(1, x), 1, 20.0)[0] - 3.831705970207512).abs() < 1e-12);
        assert!((zeros(|x| jp(1, x), 1, 20.0)[0] - 1.841183781340659).abs() < 1e-12);
        assert!((zeros(|x| sph_j(1, x), 1, 20.0)[0] - 4.493409457909064).abs() < 1e-12);
        assert!((zeros(|x| sph_xj_prime(1, x), 1, 20.0)[0] - 2.743707269992269).abs() < 1e-12);
    }

    #[test]
    fn values_and_identities() {
        // J₀(1) = 0.7651976865579666; Wronskian-like identity J₁' = J₀ − J₁/x.
        assert!((j(0, 1.0) - 0.7651976865579666).abs() < 1e-14);
        for x in [0.5, 3.0, 12.0] {
            assert!((jp(1, x) - (j(0, x) - j(1, x) / x)).abs() < 1e-13);
            // j₀ = J_{1/2} √(π/2x): compare with the closed form.
            assert!((sph_j(0, x) - x.sin() / x).abs() < 1e-15);
        }
    }

    #[test]
    fn cavity_mode_orders() {
        // A cylinder taller than its diameter: TE111 below TM010.
        let r = cylinder_cavity_resonances(0.01, 0.03, 1.0, 3);
        assert_eq!(r[0].1, "TE111");
        assert_eq!(r[1].1, "TE111");
        assert_eq!(r[2].1, "TM010");
        let s = sphere_cavity_resonances(0.01, 1.0, 4);
        assert!(s[..3].iter().all(|m| m.1 == "TM11") && s[3].1 == "TM21", "{s:?}");
    }
}

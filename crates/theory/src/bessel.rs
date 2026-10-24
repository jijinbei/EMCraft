//! Bessel functions and their zeros, for the resonances of cylindrical and spherical cavities.
//!
//! J_n(x) of integer order and the zeros come from [`emcraft_fem::bessel`] (Miller's backward
//! recurrence, the circular ports' modes use them too). Spherical Bessel functions j_n(x) from j₀ = sin x/x,
//! j₁ = sin x/x² − cos x/x and the upward recurrence j_{n+1} = (2n+1)/x j_n − j_{n−1} (stable for
//! x > n, where the zeros used here lie).

use std::f64::consts::PI;

use emcraft_model::units::C0;

pub use emcraft_fem::bessel::{j, jp, zeros};

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

/// The spherical Neumann function y_n(x), x > 0 (upward recurrence, stable).
pub fn sph_y(n: usize, x: f64) -> f64 {
    let (s, c) = x.sin_cos();
    let mut a = -c / x;
    if n == 0 {
        return a;
    }
    let mut b = -c / (x * x) - s / x;
    for k in 1..n {
        let next = (2 * k + 1) as f64 / x * b - a;
        a = b;
        b = next;
    }
    b
}

/// d/dx [x y_n(x)] = x y_{n−1}(x) − n y_n(x).
pub fn sph_xy_prime(n: usize, x: f64) -> f64 {
    if n == 0 {
        return x.sin();
    }
    x * sph_y(n - 1, x) - n as f64 * sph_y(n, x)
}

/// d/dx [x j_n(x)] = x j_{n−1}(x) − n j_n(x).
pub fn sph_xj_prime(n: usize, x: f64) -> f64 {
    if n == 0 {
        return x.cos();
    }
    x * sph_j(n - 1, x) - n as f64 * sph_j(n, x)
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

/// The resonances of a spherical cavity of radius `r_out` (vacuum) holding a concentric
/// dielectric sphere of radius `r_in` and permittivity εr, lowest first, each listed 2n + 1 times.
/// With the radial function f of the field tangential to the spheres (E for TE, H for TM), j_n
/// inside and a combination of j_n and y_n outside: TE has f = 0 at the wall and f, (r f)'
/// continuous at r_in; TM has (r f)' = 0 at the wall and f, (r f)'/ε continuous (Harrington
/// 1961, §6.10, extended to two layers).
pub fn layered_sphere_resonances(r_in: f64, eps_r: f64, r_out: f64, count: usize) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    let n1 = eps_r.sqrt();
    // Scan k0 up to where the empty cavity's tenth resonance lies, enough for the lowest modes.
    let k_max = 25.0 / r_out;
    for n in 1..5usize {
        let gj = |x: f64| sph_xj_prime(n, x);
        let gy = |x: f64| sph_xy_prime(n, x);
        let te = |k0: f64| {
            let (x1, xa, xr) = (k0 * n1 * r_in, k0 * r_in, k0 * r_out);
            let (f1, g1) = (sph_j(n, x1), gj(x1));
            let f2 = sph_j(n, xa) * sph_y(n, xr) - sph_y(n, xa) * sph_j(n, xr);
            let g2 = gj(xa) * sph_y(n, xr) - gy(xa) * sph_j(n, xr);
            f1 * g2 - f2 * g1
        };
        let tm = |k0: f64| {
            let (x1, xa, xr) = (k0 * n1 * r_in, k0 * r_in, k0 * r_out);
            let (f1, g1) = (sph_j(n, x1), gj(x1));
            let f2 = sph_j(n, xa) * gy(xr) - sph_y(n, xa) * gj(xr);
            let g2 = gj(xa) * gy(xr) - gy(xa) * gj(xr);
            f1 * g2 - f2 * g1 / eps_r
        };
        // In k0 r_out (well scaled), above x = n/2 where the recurrences hold.
        for (kind, f) in [("TE", &te as &dyn Fn(f64) -> f64), ("TM", &tm)] {
            let roots = zeros(|x| f(x / r_out), 6, k_max * r_out);
            for (p, x) in roots.iter().filter(|x| **x > 0.5 * n as f64).take(3).enumerate() {
                for _ in 0..2 * n + 1 {
                    out.push((C0 * x / r_out / (2.0 * PI), format!("{kind}{n}{}", p + 1)));
                }
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
    fn a_vacuum_core_is_the_plain_sphere_and_a_dielectric_one_lowers_it() {
        let plain = sphere_cavity_resonances(0.01, 1.0, 6);
        let same = layered_sphere_resonances(0.004, 1.0, 0.01, 6);
        for (a, b) in plain.iter().zip(&same) {
            assert!((a.0 - b.0).abs() < 1e-9 * a.0 && a.1 == b.1, "{a:?} {b:?}");
        }
        let loaded = layered_sphere_resonances(0.004, 4.0, 0.01, 3);
        assert!(loaded[0].0 < plain[0].0 && loaded[0].0 > plain[0].0 / 2.0);
        // y_n against closed forms.
        assert!((sph_y(1, 2.0) - (-(2f64.cos()) / 4.0 - 2f64.sin() / 2.0)).abs() < 1e-15);
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

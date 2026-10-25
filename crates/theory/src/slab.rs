//! The modes of a rectangular waveguide a × b (perfectly conducting walls) with a dielectric slab
//! of permittivity εr filling 0 < y < d across its whole width: hybrid modes, longitudinal-
//! section electric (LSE^y, no E_y) and magnetic (LSM^y, no H_y) to the slab's faces (R. E.
//! Collin, *Field Theory of Guided Waves*, 2nd ed., IEEE Press 1991, §6.1; R. F. Harrington,
//! *Time-Harmonic Electromagnetic Fields*, McGraw-Hill 1961, §4-7).
//!
//! With k_x = mπ/a, k_y1² = k₀²εr − k_x² − β² in the slab and k_y2² = k₀² − k_x² − β² above it,
//! L = b − d:
//! - LSE^y_mn (m ≥ 0): ψ = sin(k_y1 y) and A sin(k_y2 (b − y)), ψ and ∂ψ/∂y continuous:
//!   cos(k_y1 d) sin(k_y2 L)/k_y2 + cos(k_y2 L) sin(k_y1 d)/k_y1 = 0;
//! - LSM^y_mn (m ≥ 1): φ = cos(k_y1 y) and A cos(k_y2 (b − y)), φ and ε⁻¹ ∂φ/∂y continuous:
//!   k_y1 sin(k_y1 d) cos(k_y2 L)/εr + k_y2 sin(k_y2 L) cos(k_y1 d) = 0.
//!
//! Both are written without poles (sin(k x)/k and k sin(k x) are entire in k²), continued to
//! imaginary k_y through cosh and sinh, so their roots in β² are found by bracketing.

/// cos(√s x), continued to s < 0.
fn cos_s(s: f64, x: f64) -> f64 {
    if s >= 0.0 { (s.sqrt() * x).cos() } else { ((-s).sqrt() * x).cosh() }
}

/// sin(√s x)/√s (x at s = 0).
fn sinc_s(s: f64, x: f64) -> f64 {
    if s > 0.0 {
        let k = s.sqrt();
        (k * x).sin() / k
    } else if s < 0.0 {
        let k = (-s).sqrt();
        (k * x).sinh() / k
    } else {
        x
    }
}

/// √s sin(√s x) (−√−s sinh(√−s x) for s < 0).
fn ksin_s(s: f64, x: f64) -> f64 {
    if s >= 0.0 {
        let k = s.sqrt();
        k * (k * x).sin()
    } else {
        let k = (-s).sqrt();
        -k * (k * x).sinh()
    }
}

/// β² (rad²/m²) of the `count` modes with the largest β² at wavenumber `k0`, descending, from
/// k₀²εr down to `lowest` (the evanescent modes to include have β² above it).
pub fn slab_loaded_modes(a: f64, b: f64, d: f64, eps_r: f64, k0: f64, lowest: f64, count: usize) -> Vec<f64> {
    let l = b - d;
    let k2 = k0 * k0;
    let mut roots = Vec::new();
    for m in 0..64u32 {
        let kx2 = (m as f64 * std::f64::consts::PI / a).powi(2);
        let top = k2 * eps_r - kx2;
        if top < lowest {
            break;
        }
        let s1 = |beta2: f64| k2 * eps_r - kx2 - beta2;
        let s2 = |beta2: f64| k2 - kx2 - beta2;
        let lse = |beta2: f64| cos_s(s1(beta2), d) * sinc_s(s2(beta2), l) + cos_s(s2(beta2), l) * sinc_s(s1(beta2), d);
        let lsm = |beta2: f64| ksin_s(s1(beta2), d) * cos_s(s2(beta2), l) / eps_r + ksin_s(s2(beta2), l) * cos_s(s1(beta2), d);
        roots.extend(brackets(&lse, lowest, top));
        if m >= 1 {
            roots.extend(brackets(&lsm, lowest, top));
        }
    }
    roots.sort_by(|p, q| q.total_cmp(p));
    roots.truncate(count);
    roots
}

/// The roots of `f` in (lo, hi) by sign changes on a fine grid, each refined by bisection.
fn brackets(f: &dyn Fn(f64) -> f64, lo: f64, hi: f64) -> Vec<f64> {
    let n = 20_000;
    let x = |i: usize| hi - (hi - lo) * i as f64 / n as f64;
    let mut out = Vec::new();
    // (At the top, k_y1 = 0: a root there only when εr = 1, the empty guide's TE_m0.)
    let (mut xa, mut fa) = (x(0), f(x(0)));
    for i in 1..=n {
        let (xb, fb) = (x(i), f(x(i)));
        if fa == 0.0 {
            out.push(xa);
        } else if fa * fb < 0.0 {
            let (mut p, mut q, mut fp) = (xa, xb, fa);
            for _ in 0..200 {
                let mid = 0.5 * (p + q);
                let fm = f(mid);
                if fm == 0.0 || (q - p).abs() <= 1e-15 * mid.abs().max(1.0) {
                    p = mid;
                    q = mid;
                    break;
                }
                if fp * fm < 0.0 {
                    q = mid;
                } else {
                    p = mid;
                    fp = fm;
                }
            }
            out.push(0.5 * (p + q));
        }
        (xa, fa) = (xb, fb);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn a_slab_of_air_leaves_the_empty_guide() {
        // εr = 1: TE_mn and TM_mn, β² = k² − (mπ/a)² − (nπ/b)², each pair degenerate.
        let (a, b, k0) = (0.02286, 0.01016, 2.0 * PI * 12e9 / emcraft_model::units::C0);
        let got = slab_loaded_modes(a, b, 0.004, 1.0, k0, -1.2e5, 6);
        let mut want: Vec<f64> = (0..4)
            .flat_map(|m| (0..4).map(move |n| (m, n)))
            .filter(|&(m, n)| m + n > 0)
            .flat_map(|(m, n)| {
                let b2 = k0 * k0 - (m as f64 * PI / a).powi(2) - (n as f64 * PI / b).powi(2);
                // TE_mn always, TM_mn when m, n ≥ 1.
                if m >= 1 && n >= 1 { vec![b2, b2] } else { vec![b2] }
            })
            .collect();
        want.sort_by(|p, q| q.total_cmp(p));
        for (g, w) in got.iter().zip(&want) {
            assert!((g - w).abs() < 1e-8 * k0 * k0, "{got:?} {want:?}");
        }
    }

    #[test]
    fn a_full_slab_scales_the_wavenumber_and_the_dominant_mode_lies_between() {
        let (a, b, k0) = (0.02286, 0.01016, 2.0 * PI * 10e9 / emcraft_model::units::C0);
        // Filled (d = b, L = 0): TE10 with k² εr.
        let full = slab_loaded_modes(a, b, b, 2.25, k0, 0.0, 1);
        assert!((full[0] - (k0 * k0 * 2.25 - (PI / a).powi(2))).abs() < 1e-8 * k0 * k0, "{full:?}");
        // Half filled: the TE10-like LSM mode's β² between the empty and the full guide's.
        let half = slab_loaded_modes(a, b, b / 2.0, 2.25, k0, 0.0, 1)[0];
        assert!(half > k0 * k0 - (PI / a).powi(2) && half < full[0], "{half}");
    }
}

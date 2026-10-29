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

use num_complex::Complex64 as C;

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

/// cos(√s x) for complex s (even in √s: either root).
fn cos_c(s: C, x: f64) -> C {
    (s.sqrt() * x).cos()
}

/// sin(√s x)/√s for complex s (x at s = 0, by its series near there).
fn sinc_c(s: C, x: f64) -> C {
    if (s * x * x).norm() < 1e-8 {
        return x * (1.0 - s * x * x / 6.0);
    }
    let k = s.sqrt();
    (k * x).sin() / k
}

/// √s sin(√s x) for complex s.
fn ksin_c(s: C, x: f64) -> C {
    let k = s.sqrt();
    k * (k * x).sin()
}

/// Continuation steps from the lossless slab to the lossy one.
const STEPS: usize = 32;
/// Newton iterations per step.
const NEWTON: usize = 60;

/// The complex β² = −γ² (rad²/m²) of the `count` modes with the largest Re β² of the guide of
/// [`slab_loaded_modes`] with a lossy slab, εr = ε′ − jε″ (ε″ ≥ 0, e^{jωt}): the same LSE^y and
/// LSM^y equations, entire in β² and εr, solved in complex arithmetic. Each root of the lossless
/// slab (εr = ε′) is followed as ε″ grows from 0 in [`STEPS`] steps, each refined by Newton's
/// method (the derivative by a complex difference). Sorted by Re β², descending; the mode goes as
/// e^{−γz}, γ = √(−β²) = α + jβ with Re γ ≥ 0 ([`gamma_of`]).
pub fn slab_loaded_modes_lossy(a: f64, b: f64, d: f64, eps_r: C, k0: f64, lowest: f64, count: usize) -> Vec<C> {
    let l = b - d;
    let k2 = k0 * k0;
    let mut roots: Vec<C> = Vec::new();
    for m in 0..64u32 {
        let kx2 = (m as f64 * std::f64::consts::PI / a).powi(2);
        let top = k2 * eps_r.re - kx2;
        if top < lowest {
            break;
        }
        let lse = |beta2: C, e: C| {
            let (s1, s2) = (k2 * e - kx2 - beta2, C::new(k2 - kx2, 0.0) - beta2);
            cos_c(s1, d) * sinc_c(s2, l) + cos_c(s2, l) * sinc_c(s1, d)
        };
        let lsm = |beta2: C, e: C| {
            let (s1, s2) = (k2 * e - kx2 - beta2, C::new(k2 - kx2, 0.0) - beta2);
            ksin_c(s1, d) * cos_c(s2, l) / e + ksin_c(s2, l) * cos_c(s1, d)
        };
        let kinds: [&dyn Fn(C, C) -> C; 2] = [&lse, &lsm];
        for (kind, f) in kinds.iter().enumerate() {
            if kind == 1 && m == 0 {
                continue;
            }
            let real = |beta2: f64| f(C::new(beta2, 0.0), C::new(eps_r.re, 0.0)).re;
            for start in brackets(&real, lowest, top) {
                if let Some(r) = follow(f, C::new(start, 0.0), eps_r, k2) {
                    roots.push(r);
                }
            }
        }
    }
    roots.sort_by(|p, q| q.re.total_cmp(&p.re));
    roots.truncate(count);
    roots
}

/// A root of f(·, εr) followed from `start` (a root at εr = Re `eps`) to `eps`.
fn follow(f: &dyn Fn(C, C) -> C, start: C, eps: C, k2: f64) -> Option<C> {
    let mut x = start;
    for step in 1..=STEPS {
        let e = C::new(eps.re, eps.im * step as f64 / STEPS as f64);
        for _ in 0..NEWTON {
            let h = 1e-7 * k2;
            let fx = f(x, e);
            let df = (f(x + h, e) - f(x - h, e)) / (2.0 * h);
            if !(df.norm() > 0.0) {
                return None;
            }
            let dx = fx / df;
            x -= dx;
            if !x.is_finite() {
                return None;
            }
            if dx.norm() <= 1e-15 * k2 {
                break;
            }
        }
    }
    Some(x)
}

/// γ = √(−β²) with Re γ ≥ 0 (a decaying wave e^{−γz}).
pub fn gamma_of(beta2: C) -> C {
    let g = (-beta2).sqrt();
    if g.re < 0.0 { -g } else { g }
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

    #[test]
    fn a_lossy_slab_follows_the_lossless_roots() {
        let (a, b, k0) = (0.02286, 0.01016, 2.0 * PI * 10e9 / emcraft_model::units::C0);
        // Filled (d = b): TE10 with the complex k² εr exactly.
        let eps = C::new(2.25, -2.25 * 0.02);
        let full = slab_loaded_modes_lossy(a, b, b, eps, k0, 0.0, 1);
        let want = k0 * k0 * eps - (PI / a).powi(2);
        assert!((full[0] - want).norm() < 1e-9 * k0 * k0, "{full:?} {want}");
        // Half filled: without loss the real roots; with it, Im β² < 0 (α > 0), and α small
        // against β, below the filled guide's (part of the field is in the air).
        let real = slab_loaded_modes(a, b, b / 2.0, 2.25, k0, -3.0 * k0 * k0, 4);
        let none = slab_loaded_modes_lossy(a, b, b / 2.0, C::new(2.25, 0.0), k0, -3.0 * k0 * k0, 4);
        for (r, c) in real.iter().zip(&none) {
            assert!((c - r).norm() < 1e-9 * k0 * k0 && c.im == 0.0, "{r} {c}");
        }
        let lossy = slab_loaded_modes_lossy(a, b, b / 2.0, eps, k0, 0.0, 1);
        let g = gamma_of(lossy[0]);
        let g_full = gamma_of(full[0]);
        assert!(g.re > 0.0 && g.re < g_full.re && g.re < 0.05 * g.im, "{g} {g_full}");
        // The root satisfies the LSM equation (the dominant mode): its residual against its terms.
        let kx2 = (PI / a).powi(2);
        let (s1, s2) = (k0 * k0 * eps - kx2 - lossy[0], C::new(k0 * k0 - kx2, 0.0) - lossy[0]);
        let (d, l) = (b / 2.0, b / 2.0);
        let (p, q) = (ksin_c(s1, d) * cos_c(s2, l) / eps, ksin_c(s2, l) * cos_c(s1, d));
        assert!((p + q).norm() < 1e-9 * p.norm().max(q.norm()), "{p} {q}");
    }

    /// The references of the oracle case `lossy-slab-guide` (its expected.json): WR-90 with
    /// εr = 2.25, tan δ = 0.02 in its lower half, 20 mm long.
    #[test]
    fn the_oracles_references() {
        let (a, b, l) = (0.02286, 0.01016, 0.02);
        let eps = C::new(2.25, -2.25 * 0.02);
        let db = 20.0 * std::f64::consts::LOG10_E;
        let close = |x: f64, want: f64| (x - want).abs() <= 1e-9 * want.abs().max(1.0);
        // (GHz, ε_eff = −Re γ²/k₀², α in dB/mm, |S21| in dB, arg S21 in degrees)
        for (f, eps_eff, att, s21_db, s21_deg) in [
            (8.0, 0.8036894764051563, 0.012193127248945768, -0.2438625449789154, -172.25223277210745),
            (10.0, 1.103149042794255, 0.01686754835768551, -0.33735096715371027, 107.74151176266264),
            (12.0, 1.3057035873530582, 0.023702221207120386, -0.47404442414240805, 30.667253671338447),
        ] {
            let k0 = 2.0 * PI * f * 1e9 / emcraft_model::units::C0;
            let g = gamma_of(slab_loaded_modes_lossy(a, b, b / 2.0, eps, k0, 0.0, 1)[0]);
            let s21 = (-g * l).exp();
            assert!(close(-(g * g).re / (k0 * k0), eps_eff) && close(db * g.re * 1e-3, att), "{f}: {g}");
            assert!(close(db * s21.norm().ln(), s21_db) && close(s21.arg().to_degrees(), s21_deg), "{f}: {s21}");
        }
    }
}

//! Closed forms for open problems: the Hertzian dipole's radiation and the scattering of a plane
//! wave by a dielectric sphere (the Mie series).
//!
//! - **Hertzian dipole** of moment I·l along ẑ in free space: far field
//!   E_θ = jηk I l sin θ e^{−jkr}/(4πr), radiated power P = η (k I l)²/(12π), directivity 3/2
//!   (C. A. Balanis, *Antenna Theory*, 4th ed., Wiley 2016, §4.2).
//! - **Mie series**: a plane wave (E along x̂, travelling along +ẑ) on a sphere of radius a and
//!   relative refractive index m = √εr (μr = 1), its scattering amplitudes S₁(θ), S₂(θ) from the
//!   coefficients aₙ, bₙ with Riccati–Bessel functions ψₙ(ρ) = ρ jₙ(ρ), ξₙ(ρ) = ρ hₙ⁽¹⁾(ρ) and
//!   the angular functions πₙ, τₙ (C. F. Bohren, D. R. Huffman, *Absorption and Scattering of
//!   Light by Small Particles*, Wiley 1983, §4.4, eqs. 4.53, 4.74, 4.46–4.47). The bistatic
//!   radar cross-section is σ = 4π|S₂|²/k² in the E-plane (φ = 0, the xz plane) and 4π|S₁|²/k²
//!   in the H-plane (φ = 90°). Spherical Bessel functions jₙ of the series come by downward
//!   recurrence, normalised by j₀ = sin ρ/ρ (stable for n > ρ, where the upward one is not).

use std::f64::consts::PI;

use emcraft_model::units::{C0, MU0};
use num_complex::Complex64 as C;

/// η₀ = μ₀c (Ω).
pub fn eta0() -> f64 {
    MU0 * C0
}

/// The power (W) a Hertzian dipole of moment `il` (A·m, peak) radiates at `frequency` in free
/// space.
pub fn dipole_power(il: f64, frequency: f64) -> f64 {
    let k = 2.0 * PI * frequency / C0;
    eta0() * (k * il).powi(2) / (12.0 * PI)
}

/// |r e^{jkr} E_θ| of the dipole (V) at polar angle θ from its axis.
pub fn dipole_far_field(il: f64, frequency: f64, theta: f64) -> f64 {
    let k = 2.0 * PI * frequency / C0;
    eta0() * k * il.abs() * theta.sin().abs() / (4.0 * PI)
}

/// j₀(ρ) … j_nmax(ρ), ρ > 0, by Miller's downward recurrence.
fn sph_j_all(nmax: usize, rho: f64) -> Vec<f64> {
    let top = nmax + (rho.abs() as usize) + 30;
    let mut j = vec![0.0; top + 2];
    j[top] = 1e-300_f64.sqrt();
    for n in (1..=top).rev() {
        j[n - 1] = (2 * n + 1) as f64 / rho * j[n] - j[n + 1];
        if j[n - 1].abs() > 1e200 {
            for v in j.iter_mut().skip(n - 1) {
                *v *= 1e-200;
            }
        }
    }
    let scale = (rho.sin() / rho) / j[0];
    // Near a zero of j₀ normalise by j₁ instead.
    let scale = if (rho.sin() / rho).abs() < 1e-3 { (rho.sin() / (rho * rho) - rho.cos() / rho) / j[1] } else { scale };
    j.truncate(nmax + 1);
    j.iter().map(|v| v * scale).collect()
}

/// The Mie coefficients aₙ, bₙ (n = 1 …) for size parameter x = ka and index m.
pub fn mie_coefficients(x: f64, m: f64) -> Vec<(C, C)> {
    let nmax = (x + 4.0 * x.cbrt() + 2.0).ceil() as usize;
    let (jx, jmx) = (sph_j_all(nmax + 1, x), sph_j_all(nmax + 1, m * x));
    let y = |n: usize| crate::bessel::sph_y(n, x);
    // ψₙ(ρ) = ρ jₙ, ψₙ'(ρ) = ρ jₙ₋₁ − n jₙ; ξₙ = ρ (jₙ + i yₙ), ξₙ' likewise.
    let psi = |j: &[f64], rho: f64, n: usize| rho * j[n];
    let dpsi = |j: &[f64], rho: f64, n: usize| rho * j[n - 1] - n as f64 * j[n];
    (1..=nmax)
        .map(|n| {
            let xi = C::new(psi(&jx, x, n), x * y(n));
            let dxi = C::new(dpsi(&jx, x, n), x * y(n - 1) - n as f64 * y(n));
            let (pm, dpm) = (psi(&jmx, m * x, n), dpsi(&jmx, m * x, n));
            let (px, dpx) = (psi(&jx, x, n), dpsi(&jx, x, n));
            let a = (m * pm * dpx - px * dpm) / (m * pm * dxi - xi * dpm);
            let b = (pm * dpx - m * px * dpm) / (pm * dxi - m * xi * dpm);
            (C::new(a.re, a.im), C::new(b.re, b.im))
        })
        .collect()
}

/// S₁(θ), S₂(θ) for size parameter x and index m.
pub fn mie_amplitudes(x: f64, m: f64, theta: f64) -> (C, C) {
    let mu = theta.cos();
    let (mut s1, mut s2) = (C::new(0.0, 0.0), C::new(0.0, 0.0));
    let (mut pi_prev, mut pi) = (0.0, 1.0);
    for (k, (a, b)) in mie_coefficients(x, m).into_iter().enumerate() {
        let n = (k + 1) as f64;
        let tau = n * mu * pi - (n + 1.0) * pi_prev;
        let f = (2.0 * n + 1.0) / (n * (n + 1.0));
        s1 += f * (a * pi + b * tau);
        s2 += f * (a * tau + b * pi);
        let next = ((2.0 * n + 1.0) * mu * pi - (n + 1.0) * pi_prev) / n;
        pi_prev = pi;
        pi = next;
    }
    (s1, s2)
}

/// The bistatic radar cross-section (m²) of a dielectric sphere (radius `a`, εr) at
/// `frequency`, at scattering angle θ from the forward direction, in the E-plane (`e_plane`) or
/// the H-plane.
pub fn sphere_rcs(a: f64, eps_r: f64, frequency: f64, theta: f64, e_plane: bool) -> f64 {
    let k = 2.0 * PI * frequency / C0;
    let (s1, s2) = mie_amplitudes(k * a, eps_r.sqrt(), theta);
    4.0 * PI * if e_plane { s2.norm_sqr() } else { s1.norm_sqr() } / (k * k)
}

/// The scattering and extinction efficiencies Q_sca, Q_ext (cross-sections over πa²).
pub fn mie_efficiencies(x: f64, m: f64) -> (f64, f64) {
    let c = mie_coefficients(x, m);
    let sca: f64 = c.iter().enumerate().map(|(k, (a, b))| (2 * k + 3) as f64 * (a.norm_sqr() + b.norm_sqr())).sum();
    let ext: f64 = c.iter().enumerate().map(|(k, (a, b))| (2 * k + 3) as f64 * (a.re + b.re)).sum();
    (2.0 * sca / (x * x), 2.0 * ext / (x * x))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dipole_closed_forms() {
        // P = 40π² (Il/λ)² (Balanis 4-19 with η = 120π, ours with η₀ = μ₀c).
        let (il, f) = (1e-3, 1e9);
        let lambda = C0 / f;
        let p = dipole_power(il, f);
        assert!((p / (eta0() / (120.0 * PI) * 40.0 * PI * PI * (il / lambda).powi(2)) - 1.0).abs() < 1e-12);
        // The pattern integrates to the power: P = (1/2η) ∮ |F|² dΩ.
        let n = 2000;
        let total: f64 = (0..n)
            .map(|i| {
                let t = PI * (i as f64 + 0.5) / n as f64;
                dipole_far_field(il, f, t).powi(2) * t.sin() * PI / n as f64 * 2.0 * PI
            })
            .sum();
        assert!((total / (2.0 * eta0()) / p - 1.0).abs() < 1e-6);
    }

    #[test]
    fn spherical_bessel_by_downward_recurrence() {
        let j = sph_j_all(12, 0.7);
        assert!((j[0] - 0.7f64.sin() / 0.7).abs() < 1e-15);
        assert!((j[1] - (0.7f64.sin() / 0.49 - 0.7f64.cos() / 0.7)).abs() < 1e-15);
        // Where the upward recurrence is accurate (n < ρ) the two agree.
        let j = sph_j_all(5, 9.3);
        for n in 0..5 {
            assert!((j[n] - crate::bessel::sph_j(n, 9.3)).abs() < 1e-13, "{n}");
        }
        // j₁₀(0.7) ≈ 0.7¹⁰/21!! (the small-argument limit).
        let df: f64 = (1..=21).step_by(2).map(|k| k as f64).product();
        assert!((sph_j_all(10, 0.7)[10] / (0.7f64.powi(10) / df) - 1.0).abs() < 2e-2);
    }

    #[test]
    fn mie_series_meets_bohren_and_huffmans_example() {
        // Their BHMIE listing (Appendix A): m = 1.55, a = 0.525 µm, λ = 0.6328 µm gives
        // Q_ext = Q_sca = 3.10543 and Q_back = 2.92534.
        let x = 2.0 * PI * 0.525 / 0.6328;
        let (sca, ext) = mie_efficiencies(x, 1.55);
        assert!((ext - 3.10543).abs() < 1e-5 && (sca - 3.10543).abs() < 1e-5, "{ext} {sca}");
        let (s1, _) = mie_amplitudes(x, 1.55, PI);
        let back = 4.0 * s1.norm_sqr() / (x * x);
        assert!((back - 2.92534).abs() < 1e-5, "{back}");
    }

    #[test]
    fn mie_series_conserves_power_and_meets_rayleigh() {
        // Lossless: the extinction is all scattering (the optical theorem).
        for (x, m) in [(0.3, 1.5), (1.0, 2.0), (2.5, 1.33), (4.0, 3.0)] {
            let (sca, ext) = mie_efficiencies(x, m);
            assert!((sca / ext - 1.0).abs() < 1e-10, "{x} {m}: {sca} {ext}");
        }
        // Small spheres: Q_sca → (8/3) x⁴ ((m² − 1)/(m² + 2))², and the back-scatter
        // σ = 4π k⁴ a⁶ ((m² − 1)/(m² + 2))².
        let (x, m) = (0.01, 2.0);
        let l = (m * m - 1.0) / (m * m + 2.0);
        assert!((mie_efficiencies(x, m).0 / (8.0 / 3.0 * x.powi(4) * l * l) - 1.0).abs() < 1e-3);
        let (a, f) = (1e-4, 3e9);
        let k = 2.0 * PI * f / C0;
        let back = sphere_rcs(a, 4.0, f, PI, true);
        assert!((back / (4.0 * PI * k.powi(4) * a.powi(6) * l * l) - 1.0).abs() < 1e-3);
        // E- and H-planes agree forwards and backwards (S₁ = S₂ there up to sign).
        assert!((sphere_rcs(0.01, 4.0, 10e9, 0.0, true) - sphere_rcs(0.01, 4.0, 10e9, 0.0, false)).abs() < 1e-12);
        assert!((sphere_rcs(0.01, 4.0, 10e9, PI, true) - sphere_rcs(0.01, 4.0, 10e9, PI, false)).abs() < 1e-12);
    }
}

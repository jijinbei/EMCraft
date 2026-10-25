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
use emcraft_model::{CVec3, Point3, Vec3};
use num_complex::Complex64 as C;

/// η₀ = μ₀c (Ω).
pub fn eta0() -> f64 {
    MU0 * C0
}

/// A field sample on a Huygens surface: its point, the area it stands for, the outward unit
/// normal, and the E and H phasors there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HuygensSample {
    pub point: Point3,
    pub weight: f64,
    pub normal: Vec3,
    pub e: CVec3,
    pub h: CVec3,
}

/// The fields on a closed surface round everything that radiates, and what they radiate: the
/// equivalent currents J = n̂ × H and M = −n̂ × E give the far field
/// r e^{jkr} E_θ = −jk/(4π) (L_φ + η N_θ), r e^{jkr} E_φ = jk/(4π) (L_θ − η N_φ), with
/// N = ∮ J e^{jk r̂·r'} dS' and L = ∮ M e^{jk r̂·r'} dS' (C. A. Balanis, *Advanced Engineering
/// Electromagnetics*, 2nd ed., Wiley 2012, §6.8, eqs. 6-122, 6-125), r' from `centre`; the
/// power out through it is ½ Re ∮ (E × H*) · n̂ dS. The frequency- and time-domain solvers both
/// fill one.
#[derive(Clone, Debug, PartialEq)]
pub struct Huygens {
    pub frequency: f64,
    pub centre: Point3,
    pub samples: Vec<HuygensSample>,
}

impl Huygens {
    /// r e^{jkr} (E_θ, E_φ) in direction (θ, φ).
    pub fn far_field(&self, theta: f64, phi: f64) -> (C, C) {
        let k = 2.0 * PI * self.frequency / C0;
        let [r, th, ph] = Vec3::spherical(theta, phi);
        let (n, l) = self.samples.iter().fold((CVec3::ZERO, CVec3::ZERO), |(n, l), s| {
            let phase = C::from_polar(s.weight, k * r.dot(s.point - self.centre));
            let nrm = CVec3::from_real(s.normal);
            (n + nrm.cross(s.h) * phase, l - nrm.cross(s.e) * phase)
        });
        let c = C::new(0.0, k / (4.0 * PI));
        let eta = eta0();
        (-c * (l.dot_real(ph) + eta * n.dot_real(th)), c * (l.dot_real(th) - eta * n.dot_real(ph)))
    }

    /// The power (W) out through the surface.
    pub fn power(&self) -> f64 {
        self.samples.iter().map(|s| 0.5 * s.weight * s.e.cross(s.h.conj()).re().dot(s.normal)).sum()
    }

    /// The directivity in direction (θ, φ): 4π U/P, U = |F|²/(2η).
    pub fn directivity(&self, theta: f64, phi: f64) -> f64 {
        let (a, b) = self.far_field(theta, phi);
        4.0 * PI * (a.norm_sqr() + b.norm_sqr()) / (2.0 * eta0() * self.power())
    }
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

const EULER: f64 = std::f64::consts::EULER_GAMMA;

/// The sine and cosine integrals Si(x) and Ci(x), x > 0, by their power series (Abramowitz &
/// Stegun 5.2.14, 5.2.16), exact to rounding for the arguments up to about 20 used here.
pub fn si_ci(x: f64) -> (f64, f64) {
    let (mut si, mut ci) = (0.0, EULER + x.ln());
    // Terms (−1)^k x^(2k+1)/((2k+1)(2k+1)!) and (−1)^k x^(2k)/((2k)(2k)!).
    let mut t = x; // x^(2k+1)/(2k+1)!
    for k in 0..200 {
        let n = (2 * k + 1) as f64;
        let term_si = t / n;
        si += if k % 2 == 0 { term_si } else { -term_si };
        // x^(2k+2)/(2k+2)!
        let u = t * x / (n + 1.0);
        let term_ci = u / (n + 1.0);
        ci += if k % 2 == 0 { -term_ci } else { term_ci };
        t = u * x / (n + 2.0);
        if term_si.abs() < 1e-18 * si.abs().max(1e-300) && term_ci.abs() < 1e-18 {
            break;
        }
    }
    (si, ci)
}

/// The input impedance (Ω) of a centre-fed dipole of total length `length` and wire radius
/// `radius` by the induced-EMF method with a sinusoidal current: the radiation resistance and
/// reactance referred to the current maximum (C. A. Balanis, *Antenna Theory*, 4th ed., eqs.
/// 8-60a, 8-60b) moved to the feed by 1/sin²(kl/2). (73.1 + j42.5 Ω for a thin half-wave
/// dipole.)
pub fn dipole_impedance(length: f64, radius: f64, frequency: f64) -> C {
    let k = 2.0 * PI * frequency / C0;
    let kl = k * length;
    let eta = eta0();
    let (si1, ci1) = si_ci(kl);
    let (si2, ci2) = si_ci(2.0 * kl);
    let (_, ci3) = si_ci(2.0 * k * radius * radius / length);
    let r = eta / (2.0 * PI)
        * (EULER + kl.ln() - ci1 + 0.5 * kl.sin() * (si2 - 2.0 * si1) + 0.5 * kl.cos() * (EULER + (kl / 2.0).ln() + ci2 - 2.0 * ci1));
    let x = eta / (4.0 * PI) * (2.0 * si1 + kl.cos() * (2.0 * si1 - si2) - kl.sin() * (2.0 * ci1 - ci2 - ci3));
    let s2 = (kl / 2.0).sin().powi(2);
    C::new(r / s2, x / s2)
}

/// The open-circuit mutual impedance Z₂₁ = V₂/I₁ (Ω) of two short gaps along ẑ carrying uniform
/// currents, of lengths `l1` and `l2`, the second's centre at `offset` from the first's: with
/// V₂ = −∫ E₁ · ẑ dz along the second, the field of the first summed from its current
/// elements, each the exact field of an infinitesimal dipole (Balanis, *Antenna Theory*, 4th
/// ed., eqs. 4-10a, 4-10b), by 16 × 16 Gauss points.
pub fn mutual_impedance_parallel(l1: f64, l2: f64, offset: Vec3, frequency: f64) -> C {
    let k = 2.0 * PI * frequency / C0;
    let eta = eta0();
    let n = 16;
    let (x, w) = gauss(n);
    let rho = offset.x.hypot(offset.y);
    let pairs =
        x.iter().zip(&w).flat_map(|(a, wa)| x.iter().zip(&w).map(move |(b, wb)| ((a - 0.5) * l1, offset.z + (b - 0.5) * l2, wa * l1 * wb * l2)));
    -pairs
        .map(|(z1, z2, weight)| {
            let dz = z2 - z1;
            let r = rho.hypot(dz);
            let (ct, st) = (dz / r, rho / r);
            let kr = k * r;
            let phase = C::from_polar(1.0, -kr);
            let jkr = C::new(0.0, kr);
            let er = eta * ct / (2.0 * PI * r * r) * (1.0 + 1.0 / jkr) * phase;
            let et = C::new(0.0, eta * k * st / (4.0 * PI * r)) * (1.0 + 1.0 / jkr - 1.0 / (kr * kr)) * phase;
            (er * ct - et * st) * weight
        })
        .sum::<C>()
}

/// An array of short gaps along ẑ, each of length `l` at `positions` carrying `currents`: its
/// directivity in direction (θ, φ). The far field is the element's (a Hertzian dipole's, ∝ sin θ)
/// times the array factor Σ Iₙ e^{jk r̂·rₙ}; the power is ½ Σ Re(Z_mn I_n I_m*) with the exact
/// mutual impedances ([`mutual_impedance_parallel`]) and each gap's radiation resistance
/// 80π²(l/λ)² (C. A. Balanis, *Antenna Theory*, 4th ed., §6.2 and §8.6).
pub fn gap_array_directivity(positions: &[Point3], currents: &[C], l: f64, frequency: f64, theta: f64, phi: f64) -> f64 {
    let k = 2.0 * PI * frequency / C0;
    let eta = eta0();
    let [r, _, _] = Vec3::spherical(theta, phi);
    let af: C = positions.iter().zip(currents).map(|(p, i)| i * C::from_polar(1.0, k * r.dot(p.to_vec()))).sum();
    let f2 = (eta * k * l * theta.sin() / (4.0 * PI)).powi(2) * af.norm_sqr();
    let self_resistance = C::new(80.0 * PI * PI * (l * frequency / C0).powi(2), 0.0);
    let elements = || positions.iter().zip(currents).enumerate();
    let p: f64 = elements()
        .flat_map(|(m, (pm, im))| {
            elements().map(move |(n, (pn, inn))| {
                let z = if m == n { self_resistance } else { mutual_impedance_parallel(l, l, *pm - *pn, frequency) };
                0.5 * (z * inn * im.conj()).re
            })
        })
        .sum();
    4.0 * PI * f2 / (2.0 * eta * p)
}

/// Gauss–Legendre points on [0, 1] and their weights (Golub–Welsch by Newton on Pₙ).
fn gauss(n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut x = Vec::with_capacity(n);
    let mut w = Vec::with_capacity(n);
    for i in 0..n {
        let mut t = (PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
        for _ in 0..100 {
            let (mut p0, mut p1) = (1.0, t);
            for k in 2..=n {
                let p2 = ((2 * k - 1) as f64 * t * p1 - (k - 1) as f64 * p0) / k as f64;
                p0 = p1;
                p1 = p2;
            }
            let dp = n as f64 * (t * p1 - p0) / (t * t - 1.0);
            let dt = p1 / dp;
            t -= dt;
            if dt.abs() < 1e-16 {
                let dp = {
                    let (mut q0, mut q1) = (1.0, t);
                    for k in 2..=n {
                        let q2 = ((2 * k - 1) as f64 * t * q1 - (k - 1) as f64 * q0) / k as f64;
                        q0 = q1;
                        q1 = q2;
                    }
                    n as f64 * (t * q1 - q0) / (t * t - 1.0)
                };
                x.push(0.5 * (1.0 - t));
                w.push(1.0 / ((1.0 - t * t) * dp * dp));
                break;
            }
        }
    }
    (x, w)
}

/// The directivity of a thin half-wave dipole with a sinusoidal current at polar angle θ from
/// its axis: D = (4/Cin(2π)) [cos(π/2 cos θ)/sin θ]², 1.641 broadside (Balanis, *Antenna
/// Theory*, 4th ed., §4.6, eqs. 4-84 to 4-91; Cin(x) = γ + ln x − Ci(x)).
pub fn halfwave_directivity(theta: f64) -> f64 {
    let (_, ci) = si_ci(2.0 * PI);
    let cin = EULER + (2.0 * PI).ln() - ci;
    let s = theta.sin();
    if s.abs() < 1e-12 {
        return 0.0;
    }
    4.0 / cin * ((PI / 2.0 * theta.cos()).cos() / s).powi(2)
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

/// The Mie coefficients of a perfectly conducting sphere (the limit m → ∞): aₙ = ψₙ'(x)/ξₙ'(x),
/// bₙ = ψₙ(x)/ξₙ(x) (J. A. Stratton, *Electromagnetic Theory*, McGraw-Hill 1941, §9.25).
pub fn mie_coefficients_pec(x: f64) -> Vec<(C, C)> {
    let nmax = (x + 4.0 * x.cbrt() + 2.0).ceil() as usize;
    let jx = sph_j_all(nmax + 1, x);
    let y = |n: usize| crate::bessel::sph_y(n, x);
    (1..=nmax)
        .map(|n| {
            let (psi, dpsi) = (x * jx[n], x * jx[n - 1] - n as f64 * jx[n]);
            let xi = C::new(psi, x * y(n));
            let dxi = C::new(dpsi, x * y(n - 1) - n as f64 * y(n));
            (C::new(dpsi, 0.0) / dxi, C::new(psi, 0.0) / xi)
        })
        .collect()
}

/// S₁(θ), S₂(θ) for size parameter x and index m.
pub fn mie_amplitudes(x: f64, m: f64, theta: f64) -> (C, C) {
    amplitudes(&mie_coefficients(x, m), theta)
}

/// S₁(θ), S₂(θ) of a perfectly conducting sphere.
pub fn mie_amplitudes_pec(x: f64, theta: f64) -> (C, C) {
    amplitudes(&mie_coefficients_pec(x), theta)
}

/// S₁(θ), S₂(θ) from Mie coefficients ([`mie_coefficients`], [`mie_coefficients_pec`]), for
/// evaluating many angles from one set.
pub fn amplitudes(coefficients: &[(C, C)], theta: f64) -> (C, C) {
    let mu = theta.cos();
    let (mut s1, mut s2) = (C::new(0.0, 0.0), C::new(0.0, 0.0));
    let (mut pi_prev, mut pi) = (0.0, 1.0);
    for (k, &(a, b)) in coefficients.iter().enumerate() {
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

/// The scattering and extinction efficiencies Q_sca, Q_ext (cross-sections over πa²); `m`
/// `None` for a perfect conductor.
pub fn mie_efficiencies(x: f64, m: impl Into<Option<f64>>) -> (f64, f64) {
    let c = match m.into() {
        Some(m) => mie_coefficients(x, m),
        None => mie_coefficients_pec(x),
    };
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
    fn sine_cosine_integrals_and_the_half_wave_dipole() {
        // Abramowitz & Stegun, Table 5.1: Si(1) = 0.946083070, Ci(1) = 0.337403923,
        // Si(10) = 1.658347594, Ci(10) = −0.045456433.
        let (s, c) = si_ci(1.0);
        assert!((s - 0.946_083_070_367).abs() < 1e-11 && (c - 0.337_403_922_901).abs() < 1e-11);
        let (s, c) = si_ci(10.0);
        assert!((s - 1.658_347_594_219).abs() < 1e-10 && (c + 0.045_456_433_005).abs() < 1e-10);
        // Its directivity, 1.643 (Balanis eq. 4-91).
        assert!((halfwave_directivity(PI / 2.0) - 1.641).abs() < 1e-3);
        // The thin half-wave dipole: 73.1 + j42.5 Ω (Balanis §4.6).
        let f = 1e9;
        let z = dipole_impedance(C0 / f / 2.0, 1e-6, f);
        assert!((z.re - 73.1).abs() < 0.1 && (z.im - 42.5).abs() < 0.1, "{z}");
    }

    #[test]
    fn mutual_impedance_of_short_gaps() {
        let f = 10e9;
        let k = 2.0 * PI * f / C0;
        // Short gaps far apart: the point-dipole broadside field, V₂ = E_θ l₂ (E_z = −E_θ),
        // jηk l₁ l₂/(4πd) (1 + 1/(jkd) − 1/(kd)²) e^{−jkd}.
        let (l, d) = (1e-5, 0.01);
        let z = mutual_impedance_parallel(l, l, Vec3::new(d, 0.0, 0.0), f);
        let kd = k * d;
        let point = C::new(0.0, eta0() * k * l * l / (4.0 * PI * d)) * (1.0 + 1.0 / C::new(0.0, kd) - 1.0 / (kd * kd)) * C::from_polar(1.0, -kd);
        // (The gaps' finite length shifts it by about (l/d)² = 1e-6.)
        assert!((z - point).norm() < 1e-5 * point.norm(), "{z} {point}");
        // Reciprocal, and the Gauss rule integrates polynomials on [0, 1].
        let (a, b) = (
            mutual_impedance_parallel(1e-3, 2e-3, Vec3::new(0.004, 0.003, 0.002), f),
            mutual_impedance_parallel(2e-3, 1e-3, Vec3::new(-0.004, -0.003, -0.002), f),
        );
        assert!((a - b).norm() < 1e-12 * a.norm());
        let (x, w) = gauss(16);
        assert!((x.iter().zip(&w).map(|(x, w)| w * x.powi(31)).sum::<f64>() - 1.0 / 32.0).abs() < 1e-15);
    }

    #[test]
    fn a_gap_array_conserves_power() {
        // The directivity integrates to 4π over the sphere (the power from the mutual impedances
        // agrees with the pattern's), for an array of three gaps with phases.
        let f = 10e9;
        let pos = [Point3::ORIGIN, Point3::new(0.012, 0.0, 0.0), Point3::new(0.004, 0.009, 0.003)];
        let cur = [C::new(1.0, 0.0), C::from_polar(0.7, 1.0), C::from_polar(1.2, -2.0)];
        let (n, m) = (60, 120);
        let mut total = 0.0;
        for i in 0..n {
            let t = PI * (i as f64 + 0.5) / n as f64;
            for j in 0..m {
                let ph = 2.0 * PI * (j as f64 + 0.5) / m as f64;
                total += gap_array_directivity(&pos, &cur, 1e-3, f, t, ph) * t.sin() * (PI / n as f64) * (2.0 * PI / m as f64);
            }
        }
        // The Hertzian self-resistance of a 1 mm gap differs from its finite length's by
        // about (kl)²/10.
        assert!((total / (4.0 * PI) - 1.0).abs() < 1e-2, "{}", total / (4.0 * PI));
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
    fn a_conducting_sphere() {
        // Lossless too. Small spheres (Rayleigh): σ_back = 9πa²(ka)⁴ and Q_sca = (10/3)(ka)⁴
        // (J. J. Bowman, T. B. A. Senior, P. L. E. Uslenghi, Electromagnetic and Acoustic
        // Scattering by Simple Shapes, 1969, §10.2).
        for x in [0.5, 1.0, 3.0] {
            let (sca, ext) = mie_efficiencies(x, None);
            assert!((sca / ext - 1.0).abs() < 1e-10, "{x}");
        }
        let x: f64 = 0.01;
        assert!((mie_efficiencies(x, None).0 / (10.0 / 3.0 * x.powi(4)) - 1.0).abs() < 1e-3);
        let (s1, _) = mie_amplitudes_pec(x, PI);
        let back = 4.0 * s1.norm_sqr() / (x * x);
        assert!((back / (9.0 * x.powi(4)) - 1.0).abs() < 1e-3, "{back}");
        // Large spheres reflect like their geometric cross-section (σ_back → πa²) and extinguish
        // twice it.
        let x = 60.0;
        let (s1, _) = mie_amplitudes_pec(x, PI);
        assert!((4.0 * s1.norm_sqr() / (x * x) - 1.0).abs() < 0.05);
        assert!((mie_efficiencies(x, None).1 - 2.0).abs() < 0.05);
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

//! Closed forms of plasmas and beams for the particle-in-cell solver's verification
//! (docs/adr/0012-particles.md, stage B).
//!
//! **The plasma frequency** ω_p = √(n q²/(ε₀ m)): a cold plasma's charge displaced by ξ sets up
//! E = n q ξ/ε₀, which pulls it back at ω_p whatever the wavelength (C. K. Birdsall,
//! A. B. Langdon, *Plasma Physics via Computer Simulation*, 1991, §4-2; F. F. Chen,
//! *Introduction to Plasma Physics and Controlled Fusion*, 3rd ed., Springer 2016, §4.3).
//!
//! **The space-charge-limited current of a planar diode** (C. D. Child, Phys. Rev. 32:492,
//! 1911; I. Langmuir, Phys. Rev. 2:450, 1913): particles leaving a plane cathode at rest,
//! accelerated by V across a gap d, carry at most J = (4ε₀/9) √(2q/m) V^{3/2}/d², when the field
//! at the cathode is zero. Without the slow-particle approximation (H. R. Jory, A. W.
//! Trivelpiece, *Exact relativistic solution for the one-dimensional diode*, J. Appl. Phys.
//! 40:3924, 1969): in the steady state Poisson's law φ″ = J/(ε₀ v(φ)), with energy conservation
//! γ(φ) = 1 + qφ/(mc²), integrates once to φ′² = (2Jmc/(ε₀q)) √(γ² − 1) (zero field at the
//! cathode, ∫dφ/v = (mc/q)√(γ² − 1)), and once more to
//!
//! J = (ε₀ q/(2mc d²)) [∫₀^V (γ² − 1)^{−1/4} dφ]²,
//!
//! which tends to Child's law as qV/(mc²) → 0. The integral is taken with φ = V s⁴, which makes
//! its integrand smooth, by Simpson's rule.

use emcraft_model::units::{C0, EPS0};

/// Simpson panels of the relativistic diode's integral: its integrand is smooth after the
/// substitution, so the rule's error is far below 1e-12.
const PANELS: usize = 2000;

/// The plasma frequency ω_p (rad/s) of `density` (m⁻³) particles of `charge` (C) and `mass`
/// (kg).
///
/// ```
/// use emcraft_theory::plasma::plasma_frequency;
/// // 10¹⁶ electrons per m³: f_p = ω_p/2π ≈ 0.898 GHz.
/// let w = plasma_frequency(1e16, -1.602_176_634e-19, 9.109_383_701_5e-31);
/// assert!((w / (2.0 * std::f64::consts::PI) / 0.8979e9 - 1.0).abs() < 1e-3);
/// ```
pub fn plasma_frequency(density: f64, charge: f64, mass: f64) -> f64 {
    (density * charge * charge / (EPS0 * mass)).sqrt()
}

/// Child's space-charge-limited current density (A/m²) of particles of `charge` (C, its size)
/// and `mass` (kg) across `gap` (m) at `voltage` (V), slow particles.
///
/// ```
/// use emcraft_theory::plasma::child_langmuir;
/// // Electrons: the perveance 2.334 µA/V^{3/2} on a 1 m² cathode 1 m away.
/// let j = child_langmuir(1.0, 1.0, 1.602_176_634e-19, 9.109_383_701_5e-31);
/// assert!((j / 2.334e-6 - 1.0).abs() < 1e-3);
/// ```
pub fn child_langmuir(voltage: f64, gap: f64, charge: f64, mass: f64) -> f64 {
    4.0 * EPS0 / 9.0 * (2.0 * charge.abs() / mass).sqrt() * voltage.powf(1.5) / (gap * gap)
}

/// The space-charge-limited current density (A/m²) at any speed (Jory & Trivelpiece 1969), as
/// [`child_langmuir`]; `None` for a voltage, gap, charge or mass that is not a finite number > 0.
///
/// ```
/// use emcraft_theory::plasma::{child_langmuir, child_langmuir_relativistic};
/// let (q, m) = (1.602_176_634e-19, 9.109_383_701_5e-31);
/// // At 10 V Child's law holds; at 100 kV the electrons carry 0.98 of what it says, at 1 MV 0.85.
/// let ratio = |v: f64| child_langmuir_relativistic(v, 1e-2, q, m).unwrap() / child_langmuir(v, 1e-2, q, m);
/// assert!((ratio(10.0) - 1.0).abs() < 1e-4);
/// assert!((ratio(1e5) - 0.9799).abs() < 1e-4 && (ratio(1e6) - 0.8506).abs() < 1e-4);
/// assert!(child_langmuir_relativistic(-1.0, 1e-2, q, m).is_none());
/// ```
pub fn child_langmuir_relativistic(voltage: f64, gap: f64, charge: f64, mass: f64) -> Option<f64> {
    let q = charge.abs();
    let ok = |x: f64| x.is_finite() && x > 0.0;
    if !(ok(voltage) && ok(gap) && ok(q) && ok(mass)) {
        return None;
    }
    // γ² − 1 = a s⁴ (2 + a s⁴) at φ = V s⁴, a = qV/(mc²); dφ = 4V s³ ds, so the integrand is
    // 4V s² (a (2 + a s⁴))^{−1/4}.
    let a = q * voltage / (mass * C0 * C0);
    let f = |s: f64| 4.0 * voltage * s * s * (a * (2.0 + a * s.powi(4))).powf(-0.25);
    let h = 1.0 / PANELS as f64;
    let inner: f64 = (1..PANELS).map(|k| f(k as f64 * h) * if k % 2 == 1 { 4.0 } else { 2.0 }).sum();
    let integral = h / 3.0 * (f(0.0) + inner + f(1.0));
    let j = EPS0 * q / (2.0 * mass * C0 * gap * gap) * integral * integral;
    j.is_finite().then_some(j)
}

#[cfg(test)]
mod tests {
    use super::*;

    const Q: f64 = 1.602_176_634e-19;
    const M: f64 = 9.109_383_701_5e-31;

    #[test]
    fn a_weak_beam_grows_as_the_cube_root_of_its_density() {
        // n_b ≪ n_p at k v₀ = ω_p: γ → (√3/2)(n_b/2n_p)^{1/3} ω_p (Chen §6.6), within a few % at
        // 1e-4 and approaching it as n_b falls.
        let wp = 1e10;
        let k = 1.0;
        let off = |ratio: f64| {
            let g = two_stream_growth(wp, 0.0, wp * ratio.sqrt(), wp / k, k).unwrap_or(0.0);
            g / (3f64.sqrt() / 2.0 * (ratio / 2.0).cbrt() * wp) - 1.0
        };
        assert!(off(1e-4).abs() < 0.03 && off(1e-6).abs() < off(1e-4).abs());
    }

    #[test]
    fn the_relativistic_diode_tends_to_childs_law_and_falls_below_it() {
        // Below Child's law, the more so the faster the particles.
        let ratio = |v: f64| child_langmuir_relativistic(v, 1e-2, Q, M).unwrap_or(0.0) / child_langmuir(v, 1e-2, Q, M);
        assert!((ratio(1.0) - 1.0).abs() < 1e-6);
        assert!(ratio(1e4) < 1.0 && ratio(1e5) < ratio(1e4) && ratio(1e6) < ratio(1e5));
        // Far above mc², the particles at c: J → 2ε₀cV/d², growing as V, not V^{3/2}.
        let high = |v: f64| child_langmuir_relativistic(v, 1e-2, Q, M).unwrap_or(0.0);
        assert!((high(2e8) / high(1e8) - 2.0).abs() < 0.1);
        assert!((high(1e9) / (2.0 * EPS0 * C0 * 1e9 / 1e-4) - 1.0).abs() < 0.1);
    }
}

/// I₀ = 4πε₀mc³/|q| (A): the current scale of relativistic beams of particles of `charge` (C) and
/// `mass` (kg), 17.045 kA for electrons.
///
/// ```
/// use emcraft_theory::plasma::current_scale;
/// assert!((current_scale(1.602_176_634e-19, 9.109_383_701_5e-31) / 17_045.0 - 1.0).abs() < 1e-4);
/// ```
pub fn current_scale(charge: f64, mass: f64) -> f64 {
    4.0 * std::f64::consts::PI * EPS0 * mass * C0.powi(3) / charge.abs()
}

/// A thin annular beam of radius r_b in a grounded pipe of radius R, held on its lines by an
/// infinite axial magnetic field, injected at γ₀ through a grounded plane (L. S. Bogdankevich,
/// A. A. Rukhadze, Sov. Phys. Usp. 14:163, 1971; R. B. Miller, *An Introduction to the Physics of
/// Intense Charged Particle Beams*, Plenum 1982, §3.2): its own charge sinks its potential, the
/// particles slow to γ_b with I = I₀ (γ₀ − γ_b) β_b / (2 ln(R/r_b)), and no more than the
/// limiting current I_L = I₀ (γ₀^{2/3} − 1)^{3/2} / (2 ln(R/r_b)) passes (the most of that, at
/// γ_b = γ₀^{1/3}).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnnularBeam {
    pub gamma0: f64,
    /// R/r_b.
    pub ratio: f64,
    pub charge: f64,
    pub mass: f64,
}

impl AnnularBeam {
    fn scale(&self) -> f64 {
        current_scale(self.charge, self.mass) / (2.0 * self.ratio.ln())
    }

    /// The current (A) at the beam's γ_b.
    fn current_at(&self, gb: f64) -> f64 {
        self.scale() * (self.gamma0 - gb) * (1.0 - 1.0 / (gb * gb)).max(0.0).sqrt()
    }

    /// The limiting current (A); none for γ₀ ≤ 1 or R ≤ r_b.
    ///
    /// ```
    /// use emcraft_theory::plasma::AnnularBeam;
    /// // γ₀ = 3 in a pipe 10/6 of the beam's radius: 18.7 kA.
    /// let b = AnnularBeam { gamma0: 3.0, ratio: 10.0 / 6.0, charge: 1.602_176_634e-19, mass: 9.109_383_701_5e-31 };
    /// assert!((b.limiting().unwrap() / 18.72e3 - 1.0).abs() < 1e-3);
    /// ```
    pub fn limiting(&self) -> Option<f64> {
        (self.gamma0 > 1.0 && self.ratio > 1.0 && self.ratio.is_finite()).then(|| self.scale() * (self.gamma0.powf(2.0 / 3.0) - 1.0).powf(1.5))
    }

    /// The beam's γ_b at `current` (A) below the limiting current: the faster of the two
    /// solutions, which a beam rising from nothing takes; none at or above the limit.
    ///
    /// ```
    /// use emcraft_theory::plasma::AnnularBeam;
    /// let b = AnnularBeam { gamma0: 3.0, ratio: 10.0 / 6.0, charge: 1.602_176_634e-19, mass: 9.109_383_701_5e-31 };
    /// let il = b.limiting().unwrap();
    /// let g = b.depressed(0.5 * il).unwrap();
    /// assert!(g < 3.0 && g > 3f64.cbrt());
    /// assert!(b.depressed(1.1 * il).is_none());
    /// ```
    pub fn depressed(&self, current: f64) -> Option<f64> {
        let limit = self.limiting()?;
        if !(current.is_finite() && current >= 0.0 && current < limit) {
            return None;
        }
        // current_at falls from the limit at γ₀^{1/3} to 0 at γ₀: bisection.
        let (mut lo, mut hi) = (self.gamma0.cbrt(), self.gamma0);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if self.current_at(mid) > current {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(0.5 * (lo + hi))
    }
}

/// The growth rate (1/s) of a longitudinal wave of wavenumber `k` (1/m) on two cold streams of
/// plasma frequencies `wa`, `wb` (rad/s) moving at `va`, `vb` (m/s), slow particles: the largest
/// imaginary part of the roots ω of 1 = ω_a²/(ω − k v_a)² + ω_b²/(ω − k v_b)² (C. K. Birdsall,
/// A. B. Langdon, *Plasma Physics via Computer Simulation*, 1991, §5-6; F. F. Chen,
/// *Introduction to Plasma Physics*, 3rd ed., 2016, §6.6), 0 when stable. The quartic's roots
/// by the Durand–Kerner iteration, bounded.
///
/// Two equal streams at ±v₀ grow fastest, at ω_b/2, where k v₀ = (√3/2) ω_b; a weak beam (n_b ≪
/// n_p) through a plasma at rest at k v₀ = ω_p, at (√3/2)(n_b/2n_p)^{1/3} ω_p — the cube root of
/// Pierce's travelling-wave gain.
///
/// ```
/// use emcraft_theory::plasma::two_stream_growth;
/// let wb = 1e9;
/// let v0 = 1e7;
/// let k = 3f64.sqrt() / 2.0 * wb / v0;
/// let g = two_stream_growth(wb, v0, wb, -v0, k).unwrap();
/// assert!((g / (wb / 2.0) - 1.0).abs() < 1e-9);
/// // Past k v₀ = √2 ω_b it is stable.
/// assert_eq!(two_stream_growth(wb, v0, wb, -v0, 1.5 * wb / v0), Some(0.0));
/// ```
pub fn two_stream_growth(wa: f64, va: f64, wb: f64, vb: f64, k: f64) -> Option<f64> {
    use num_complex::Complex64 as Z;
    if ![wa, va, wb, vb, k].iter().all(|x| x.is_finite()) {
        return None;
    }
    let (a, b) = (k * va, k * vb);
    // (ω − a)²(ω − b)² − ω_a²(ω − b)² − ω_b²(ω − a)², a monic quartic: its coefficients, low first.
    let mul = |p: &[f64], q: &[f64]| -> Vec<f64> {
        let mut r = vec![0.0; p.len() + q.len() - 1];
        for (i, x) in p.iter().enumerate() {
            for (j, y) in q.iter().enumerate() {
                r[i + j] += x * y;
            }
        }
        r
    };
    let sq = |c: f64| mul(&[-c, 1.0], &[-c, 1.0]);
    let (pa, pb) = (sq(a), sq(b));
    let mut poly = mul(&pa, &pb);
    for (i, (x, y)) in pb.iter().zip(&pa).enumerate() {
        poly[i] -= wa * wa * x + wb * wb * y;
    }
    let eval = |z: Z| poly.iter().rev().fold(Z::new(0.0, 0.0), |acc, c| acc * z + c);
    let scale = a.abs().max(b.abs()).max(wa).max(wb).max(1.0);
    let mut roots: Vec<Z> = (0..4).map(|i| Z::from_polar(scale, 0.4 + 1.3 * i as f64)).collect();
    for _ in 0..2000 {
        let mut moved = 0.0f64;
        for i in 0..4 {
            let denom = (0..4).filter(|j| *j != i).fold(Z::new(1.0, 0.0), |acc, j| acc * (roots[i] - roots[j]));
            if denom.norm() == 0.0 {
                continue;
            }
            let step = eval(roots[i]) / denom;
            roots[i] -= step;
            moved = moved.max(step.norm() / scale);
        }
        if moved < 1e-15 {
            break;
        }
    }
    let g = roots.iter().map(|z| z.im).fold(0.0f64, f64::max);
    // Round-off of a real root is not growth.
    Some(if g < 1e-9 * scale { 0.0 } else { g })
}

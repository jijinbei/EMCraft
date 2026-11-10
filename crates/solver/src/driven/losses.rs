//! Losses and stored energy: the walls' surface resistance and their Leontovich term, the
//! powers the dielectrics and the walls absorb, and the energy a resonance stores.

use emcraft_model::units::angular;
use std::f64::consts::PI;

use emcraft_model::units::{C0, MU0};
use emcraft_model::{CVec3, Vec3};
use num_complex::Complex64 as C;

use super::{Driven, Field, Result};

/// Walls of finite conductivity: σ (S/m), their thickness (m, infinite when `None`) and the
/// rms height of their roughness (m, smooth when `None`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wall {
    pub sigma: f64,
    pub thickness: Option<f64>,
    pub roughness: Option<f64>,
}

impl Wall {
    /// Smooth walls of `sigma`, infinitely thick.
    pub const fn thick(sigma: f64) -> Wall {
        Wall { sigma, thickness: None, roughness: None }
    }

    /// The surface impedance Z_s (Ω): [`wall_impedance`] times the roughness factor
    /// [`roughness_factor`] where the walls are rough.
    ///
    /// ```
    /// use emcraft_solver::driven::{Wall, roughness_factor, wall_impedance};
    /// let (f, cu) = (10e9, 5.8e7);
    /// let smooth = Wall::thick(cu);
    /// assert_eq!(smooth.impedance(f), wall_impedance(f, cu, None));
    /// // 1 µm rms on copper (δ ≈ 0.66 µm at 10 GHz): the loss grows by about 80 %.
    /// let rough = Wall { roughness: Some(1e-6), ..smooth };
    /// let k = roughness_factor(f, cu, 1e-6);
    /// assert!((rough.impedance(f) - smooth.impedance(f) * k).norm() < 1e-15 && k > 1.8 && k < 1.81);
    /// ```
    pub fn impedance(&self, frequency: f64) -> C {
        let z = wall_impedance(frequency, self.sigma, self.thickness);
        match self.roughness {
            Some(h) => z * roughness_factor(frequency, self.sigma, h),
            None => z,
        }
    }
}

/// The Hammerstad–Jensen roughness factor K = 1 + (2/π) arctan(1.4 (Δ/δ)²) by which a
/// surface of rms height Δ (m) multiplies the loss of a smooth one (E. Hammerstad and Ø. Jensen,
/// "Accurate models for microstrip computer-aided design", IEEE MTT-S Int. Microwave Symp.
/// Digest, 1980, pp. 407–409): 1 for Δ ≪ δ, towards 2 for Δ ≫ δ. δ = 1/√(πfμ₀σ).
///
/// ```
/// use emcraft_solver::driven::roughness_factor;
/// assert_eq!(roughness_factor(10e9, 5.8e7, 0.0), 1.0);
/// assert!(roughness_factor(10e9, 5.8e7, 1e-3) > 1.999);
/// ```
pub fn roughness_factor(frequency: f64, sigma: f64, roughness: f64) -> f64 {
    let delta = 1.0 / (PI * frequency * MU0 * sigma).sqrt();
    let ratio = roughness / delta;
    1.0 + 2.0 / PI * (1.4 * ratio * ratio).atan()
}

/// Walls of finite conductivity in a solve.
#[derive(Clone, Debug)]
pub(crate) struct WallLoss {
    pub wall: Wall,
    /// The walls' tangential mass matrix ∫ N_i,t · N_j,t dS on the pattern.
    pub mass: Vec<f64>,
}

/// ε₀ (F/m).
fn eps0() -> f64 {
    1.0 / (MU0 * C0 * C0)
}

impl Driven {
    /// The power (W) the dielectrics absorb for the coefficients `x`: ½ ω ε₀ ∫ εr tan δ |E|² dV
    /// (fields in peak values).
    pub fn dielectric_loss(&self, x: &[C], frequency: f64) -> Result<f64> {
        let omega = angular(frequency);
        let loss = |t: usize| -self.eps_at(t, frequency).im;
        let total = self.integrate_e(x, frequency, |t| loss(t) != 0.0, |t, e| loss(t) * e.norm_sqr())?;
        Ok(0.5 * omega * eps0() * total)
    }

    /// The power (W) the magnetically lossy materials absorb for the coefficients `x`:
    /// ½ ω μ₀ ∫ μ″ |H|² dV, μ = μ′ − jμ″ (fields in peak values).
    pub fn magnetic_loss(&self, x: &[C], frequency: f64) -> Result<f64> {
        let omega = angular(frequency);
        let mut total = 0.0;
        if self.media.magnetically_lossy() {
            let loss = |t: usize| -self.mu_of(t).im;
            total += 0.5 * omega * MU0 * self.integrate_field(x, frequency, Field::H, |t| loss(t) != 0.0, |t, h| loss(t) * h.norm_sqr())?;
        }
        // A lossy ferrite's: its Polder tensor's anti-Hermitian part.
        let n = super::assembly::rule_points(self.order);
        for t in (0..self.mesh.tets.len()).filter(|t| self.media.ferrite(*t).is_some_and(|f| f.linewidth > 0.0)) {
            let el = self.element_of(t)?;
            total += el.rule(n).into_iter().map(|(l, w)| w * self.ferrite_loss_at((t, &el), l, x, frequency)).sum::<f64>();
        }
        Ok(total)
    }

    /// The power (W) walls `wall` would absorb for a field computed with perfectly conducting
    /// walls: ½ Re Z_s ∫ |H_t|² dS (the perturbation estimate, [`Wall::impedance`]).
    pub fn wall_loss_perturbation(&self, x: &[C], frequency: f64, wall: Wall) -> Result<f64> {
        let rs = wall.impedance(frequency).re;
        let total = self.integrate_walls(x, frequency, Field::H, |h, n| tangential_c(h, n).norm_sqr())?;
        Ok(0.5 * rs * total)
    }

    /// The time-averaged stored energy (J) at resonance, twice the electric: ½ ε₀ ∫ εr |E|² dV.
    pub fn stored_energy(&self, x: &[C], frequency: f64) -> Result<f64> {
        let total = self.integrate_e(x, frequency, |_| true, |t, e| self.eps_at(t, frequency).re * e.norm_sqr())?;
        Ok(0.5 * eps0() * total)
    }

    /// The power (W) the walls of finite conductivity absorb: ½ Re Z_s/|Z_s|² ∫ |E_t|² dS (= ½ Re
    /// Z_s ∫ |H_t|² dS with the Leontovich condition; ∫ |E_t|²/(4R_s) dS for thick walls).
    pub fn wall_loss(&self, x: &[C], frequency: f64) -> Result<f64> {
        let Some(wall) = &self.wall else { return Ok(0.0) };
        let total = self.integrate_walls(x, frequency, Field::E, |e, n| tangential_c(e, n).norm_sqr())?;
        Ok(match wall.wall {
            Wall { sigma, thickness: None, roughness: None } => total / (4.0 * surface_resistance(frequency, sigma)),
            w => {
                let z = w.impedance(frequency);
                0.5 * z.re / z.norm_sqr() * total
            }
        })
    }
}

/// The part of a complex field across the unit normal n: v − (v·n) n.
fn tangential_c(v: CVec3, n: Vec3) -> CVec3 {
    v - n * v.dot_real(n)
}

/// R_s = √(ωμ₀/2σ) (Ω).
pub fn surface_resistance(frequency: f64, sigma: f64) -> f64 {
    (PI * frequency * MU0 / sigma).sqrt()
}

/// Walls this many skin depths thick or more are taken as infinitely thick: tanh γt then
/// differs from 1 by 2e^{−2·20} ≈ 1e-17.
const THICK: f64 = 20.0;

/// The surface impedance Z_s (Ω) of walls of conductivity `sigma` (S/m) and `thickness` (m,
/// infinite when `None`) with free space behind them, seen from inside the guide. The metal is
/// never meshed: the wave crosses the wall's layer, intrinsic impedance η_c = (1 + j)R_s and
/// propagation constant γ = (1 + j)/δ (δ = 1/√(πfμ₀σ) the skin depth), along the normal (a
/// good conductor refracts every wave to it), so the wall is a lossy line of length t loaded by
/// η₀: Z_s = η_c (η₀ + η_c tanh γt)/(η_c + η₀ tanh γt) (D. M. Pozar, *Microwave Engineering*,
/// 4th ed., Wiley 2012, eq. 2.91; Senior & Volakis 1995, §2.4). Thick walls give the
/// Leontovich η_c; a wall much thinner than δ the sheet 1/(σt) beside η₀.
///
/// ```
/// use emcraft_solver::driven::{surface_resistance, wall_impedance};
/// let (f, cu) = (10e9, 5.8e7);
/// let rs = surface_resistance(f, cu);
/// // Thick copper: (1 + j)R_s, R_s ≈ 26 mΩ at 10 GHz.
/// let z = wall_impedance(f, cu, None);
/// assert!((z.re - rs).abs() < 1e-15 && (z.im - rs).abs() < 1e-15);
/// assert_eq!(wall_impedance(f, cu, Some(1e-3)), z);
/// // One skin depth (0.66 µm) of copper loses more.
/// let delta = 1.0 / (std::f64::consts::PI * f * 4e-7 * std::f64::consts::PI * cu).sqrt();
/// assert!(wall_impedance(f, cu, Some(delta)).re > 1.05 * rs);
/// ```
pub fn wall_impedance(frequency: f64, sigma: f64, thickness: Option<f64>) -> C {
    let rs = surface_resistance(frequency, sigma);
    let eta = C::new(rs, rs);
    let Some(t) = thickness else { return eta };
    let delta = 1.0 / (PI * frequency * MU0 * sigma).sqrt();
    let u = t / delta;
    if !(u < THICK) {
        return eta;
    }
    let tanh = C::new(u, u).tanh();
    let eta0 = MU0 * C0;
    eta * (eta0 + eta * tanh) / (eta + eta0 * tanh)
}

/// jωμ₀/Z_s (1/m): ωμ₀(1 + j)/(2R_s) for thick smooth walls ([`Wall::impedance`]).
pub(crate) fn wall_factor(frequency: f64, wall: Wall) -> C {
    let omega = angular(frequency);
    if wall.roughness.is_some() {
        return C::new(0.0, omega * MU0) / wall.impedance(frequency);
    }
    let Wall { sigma, thickness, .. } = wall;
    let z = wall_impedance(frequency, sigma, thickness);
    if z == C::new(z.re, z.re) {
        // Thick walls: the Leontovich factor as it always was, to the bit.
        return C::new(1.0, 1.0) * (omega * MU0 / (2.0 * surface_resistance(frequency, sigma)));
    }
    C::new(0.0, omega * MU0) / z
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thin_wall_is_a_sheet_beside_free_space_and_a_thick_one_leontovich() {
        let (f, sigma) = (10e9, 5.8e7);
        let delta = 1.0 / (PI * f * MU0 * sigma).sqrt();
        // Far thinner than δ: tanh γt ≈ γt, so Z_s ≈ (1/σt) ∥ η₀.
        let t = 1e-4 * delta;
        let (sheet, eta0) = (1.0 / (sigma * t), MU0 * C0);
        let z = wall_impedance(f, sigma, Some(t));
        let parallel = sheet * eta0 / (sheet + eta0);
        assert!((z.re - parallel).abs() < 1e-3 * parallel && z.im.abs() < 1e-3 * parallel, "{z} against {parallel}");
        // Thicker and thicker: towards (1 + j)R_s, met to the bit from 20 skin depths.
        let rs = surface_resistance(f, sigma);
        let far = wall_impedance(f, sigma, Some(8.0 * delta));
        assert!((far - C::new(rs, rs)).norm() < 1e-6 * rs);
        assert_eq!(wall_impedance(f, sigma, Some(20.0 * delta)), C::new(rs, rs));
        assert_eq!(wall_factor(f, Wall { thickness: Some(1.0), ..Wall::thick(sigma) }), wall_factor(f, Wall::thick(sigma)));
        // The factor is jωμ₀/Z_s.
        let w = wall_factor(f, Wall { thickness: Some(delta), ..Wall::thick(sigma) });
        let back = C::new(0.0, 2.0 * PI * f * MU0) / w;
        assert!((back - wall_impedance(f, sigma, Some(delta))).norm() < 1e-12 * rs);
    }
}

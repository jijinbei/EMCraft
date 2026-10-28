//! Losses and stored energy: the walls' surface resistance and their Leontovich term, the
//! powers the dielectrics and the walls absorb, and the energy a resonance stores.

use std::f64::consts::PI;

use emcraft_model::units::{C0, MU0};
use emcraft_model::{CVec3, Vec3};
use num_complex::Complex64 as C;

use super::{Driven, Field, Result};

/// Walls of finite conductivity.
#[derive(Clone, Debug)]
pub(crate) struct WallLoss {
    /// σ (S/m).
    pub sigma: f64,
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
        let omega = 2.0 * PI * frequency;
        let loss = |t: usize| -self.eps_at(t, frequency).im;
        let total = self.integrate_e(x, frequency, |t| loss(t) != 0.0, |t, e| loss(t) * e.norm_sqr())?;
        Ok(0.5 * omega * eps0() * total)
    }

    /// The power (W) the magnetically lossy materials absorb for the coefficients `x`:
    /// ½ ω μ₀ ∫ μ″ |H|² dV, μ = μ′ − jμ″ (fields in peak values).
    pub fn magnetic_loss(&self, x: &[C], frequency: f64) -> Result<f64> {
        if !self.media.magnetically_lossy() {
            return Ok(0.0);
        }
        let omega = 2.0 * PI * frequency;
        let loss = |t: usize| -self.mu_of(t).im;
        let total = self.integrate_field(x, frequency, Field::H, |t| loss(t) != 0.0, |t, h| loss(t) * h.norm_sqr())?;
        Ok(0.5 * omega * MU0 * total)
    }

    /// The power (W) walls of conductivity `sigma` would absorb for a field computed with
    /// perfectly conducting walls: ½ R_s ∫ |H_t|² dS (the perturbation estimate).
    pub fn wall_loss_perturbation(&self, x: &[C], frequency: f64, sigma: f64) -> Result<f64> {
        let rs = surface_resistance(frequency, sigma);
        let total = self.integrate_walls(x, frequency, Field::H, |h, n| tangential_c(h, n).norm_sqr())?;
        Ok(0.5 * rs * total)
    }

    /// The time-averaged stored energy (J) at resonance, twice the electric: ½ ε₀ ∫ εr |E|² dV.
    pub fn stored_energy(&self, x: &[C], frequency: f64) -> Result<f64> {
        let total = self.integrate_e(x, frequency, |_| true, |t, e| self.eps_at(t, frequency).re * e.norm_sqr())?;
        Ok(0.5 * eps0() * total)
    }

    /// The power (W) the walls of finite conductivity absorb: ∫ |E_t|²/(4R_s) dS (= ½ R_s ∫ |H_t|²
    /// dS with the Leontovich condition).
    pub fn wall_loss(&self, x: &[C], frequency: f64) -> Result<f64> {
        let Some(wall) = &self.wall else { return Ok(0.0) };
        let rs = surface_resistance(frequency, wall.sigma);
        let total = self.integrate_walls(x, frequency, Field::E, |e, n| tangential_c(e, n).norm_sqr())?;
        Ok(total / (4.0 * rs))
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

/// jωμ₀/Z_s = ωμ₀(1 + j)/(2R_s) (1/m).
pub(crate) fn wall_factor(frequency: f64, sigma: f64) -> C {
    let omega = 2.0 * PI * frequency;
    C::new(1.0, 1.0) * (omega * MU0 / (2.0 * surface_resistance(frequency, sigma)))
}

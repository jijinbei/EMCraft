//! Materials: linear and isotropic, optionally lossy (a loss tangent) or dispersive (poles).

use std::f64::consts::PI;

use num_complex::Complex64;
use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid, named, positive};

/// A linear, isotropic material, lossless unless it has a loss tangent or dispersion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// Relative permittivity εr; with dispersion, its value at infinite frequency ε∞.
    pub eps_r: f64,
    /// Relative permeability μr.
    pub mu_r: f64,
    /// Dielectric loss tangent tan δ: the permittivity is εr (1 − j tan δ) (time dependence
    /// e^{jωt}).
    #[serde(default)]
    pub loss_tangent: f64,
    /// Dispersion: poles whose susceptibilities add to the permittivity ([`Material::eps_at`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dispersion: Vec<Pole>,
}

/// The most poles one material may carry.
pub const MAX_POLES: usize = 16;

/// One pole of a dispersive permittivity: a susceptibility χ(ω) added to ε∞. The forms are the
/// standard ones (A. Taflove, S. C. Hagness, *Computational Electrodynamics: The
/// Finite-Difference Time-Domain Method*, 3rd ed., Artech House 2005, §9.2 and §9.4), written
/// for the time dependence e^{jωt}, so that a lossy medium has ε = ε′ − jε″ with ε″ > 0.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Pole {
    /// Debye relaxation: χ = Δε / (1 + jωτ), τ the relaxation time (s).
    Debye { delta_eps: f64, tau: f64 },
    /// Lorentz resonance: χ = Δε ω₀² / (ω₀² − ω² + jωγ), ω₀ = 2π `frequency`, γ = 2π
    /// `damping` (both in Hz).
    Lorentz { delta_eps: f64, frequency: f64, damping: f64 },
    /// Drude (free carriers, a Lorentz pole with ω₀ = 0): χ = −ω_p² / (ω² − jωγ), ω_p = 2π
    /// `plasma_frequency`, γ = 2π `damping` (both in Hz).
    Drude { plasma_frequency: f64, damping: f64 },
}

/// A second-order pole, χ = `strength` / (ω₀² − ω² + jωγ), all in rad/s: a Lorentz pole
/// (strength Δε ω₀²) or a Drude one (ω₀ = 0, strength ω_p²).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SecondOrder {
    /// The numerator a (rad²/s²).
    pub strength: f64,
    /// The resonance ω₀ (rad/s).
    pub omega0: f64,
    /// The damping γ (rad/s).
    pub gamma: f64,
}

/// An angular frequency (rad/s) from a frequency (Hz).
fn angular(f: f64) -> f64 {
    2.0 * PI * f
}

impl Pole {
    /// The pole's kind as it is named in summaries.
    pub fn kind(self) -> &'static str {
        match self {
            Pole::Debye { .. } => "Debye",
            Pole::Lorentz { .. } => "Lorentz",
            Pole::Drude { .. } => "Drude",
        }
    }

    /// Checks the pole: finite parameters, all > 0 (an undamped pole is singular at its
    /// resonance, an undamped Drude pole at zero frequency).
    pub fn checked(self) -> Result<Pole> {
        match self {
            Pole::Debye { delta_eps, tau } => {
                positive("Debye pole Δε", delta_eps)?;
                positive("Debye relaxation time", tau)?;
            }
            Pole::Lorentz { delta_eps, frequency, damping } => {
                positive("Lorentz pole Δε", delta_eps)?;
                positive("Lorentz resonance frequency", frequency)?;
                positive("Lorentz damping", damping)?;
            }
            Pole::Drude { plasma_frequency, damping } => {
                positive("Drude plasma frequency", plasma_frequency)?;
                positive("Drude damping", damping)?;
            }
        }
        Ok(self)
    }

    /// The pole as a second-order one, or `None` for Debye.
    pub fn second_order(self) -> Option<SecondOrder> {
        match self {
            Pole::Debye { .. } => None,
            Pole::Lorentz { delta_eps, frequency, damping } => {
                let omega0 = angular(frequency);
                Some(SecondOrder { strength: delta_eps * omega0 * omega0, omega0, gamma: angular(damping) })
            }
            Pole::Drude { plasma_frequency, damping } => {
                let wp = angular(plasma_frequency);
                Some(SecondOrder { strength: wp * wp, omega0: 0.0, gamma: angular(damping) })
            }
        }
    }

    /// The susceptibility χ(ω) at `frequency` (Hz).
    pub fn susceptibility(self, frequency: f64) -> Complex64 {
        let omega = angular(frequency);
        match (self, self.second_order()) {
            (Pole::Debye { delta_eps, tau }, _) => Complex64::new(delta_eps, 0.0) / Complex64::new(1.0, omega * tau),
            (_, Some(SecondOrder { strength, omega0, gamma })) => {
                Complex64::new(strength, 0.0) / Complex64::new(omega0 * omega0 - omega * omega, omega * gamma)
            }
            (_, None) => Complex64::new(0.0, 0.0),
        }
    }
}

impl Material {
    pub fn new(name: &str, eps_r: f64, mu_r: f64) -> Result<Material> {
        named("material name", name)?;
        Ok(Material {
            name: name.to_owned(),
            eps_r: positive("relative permittivity", eps_r)?,
            mu_r: positive("relative permeability", mu_r)?,
            loss_tangent: 0.0,
            dispersion: Vec::new(),
        })
    }

    /// The same material with a dielectric loss tangent (finite, ≥ 0).
    pub fn with_loss_tangent(mut self, tan_d: f64) -> Result<Material> {
        if !(tan_d.is_finite() && tan_d >= 0.0) {
            return Err(invalid("loss tangent", format!("must be a finite number ≥ 0, got {tan_d}")));
        }
        self.loss_tangent = tan_d;
        Ok(self)
    }

    /// The same material with dispersion poles (each checked, at most [`MAX_POLES`]).
    pub fn with_dispersion(mut self, poles: Vec<Pole>) -> Result<Material> {
        if poles.len() > MAX_POLES {
            return Err(invalid("dispersion", format!("holds at most {MAX_POLES} poles, got {}", poles.len())));
        }
        self.dispersion = poles.into_iter().map(Pole::checked).collect::<Result<_>>()?;
        Ok(self)
    }

    /// The permittivity in brief for lists: "εr 2.2, μr 1, tan δ 0.01", or for a dispersive
    /// material "ε∞ 2, 1 Debye + 1 Lorentz pole(s), μr 1".
    pub fn summary(&self) -> String {
        let loss = if self.loss_tangent > 0.0 { format!(", tan δ {}", self.loss_tangent) } else { String::new() };
        if !self.is_dispersive() {
            return format!("εr {}, μr {}{loss}", self.eps_r, self.mu_r);
        }
        let kinds: Vec<String> = ["Debye", "Lorentz", "Drude"]
            .into_iter()
            .map(|k| (self.dispersion.iter().filter(|p| p.kind() == k).count(), k))
            .filter(|(n, _)| *n > 0)
            .map(|(n, k)| format!("{n} {k}"))
            .collect();
        let plural = if self.dispersion.len() == 1 { "" } else { "s" };
        format!("ε∞ {}, {} pole{plural}, μr {}{loss}", self.eps_r, kinds.join(" + "), self.mu_r)
    }

    /// Whether the permittivity depends on the frequency.
    pub fn is_dispersive(&self) -> bool {
        !self.dispersion.is_empty()
    }

    /// The refractive index that sets the wavelength at `frequency` (Hz): √(εr μr), with
    /// dispersion √(|εr(ω)| μr).
    pub fn index_at(&self, frequency: f64) -> f64 {
        if self.is_dispersive() { (self.eps_at(frequency).norm() * self.mu_r).sqrt() } else { (self.eps_r * self.mu_r).sqrt() }
    }

    /// The complex relative permittivity at `frequency` (Hz): εr (1 − j tan δ) + Σ χ_k(ω)
    /// (time dependence e^{jωt}, so ε = ε′ − jε″).
    pub fn eps_at(&self, frequency: f64) -> Complex64 {
        let base = Complex64::new(self.eps_r, -self.eps_r * self.loss_tangent);
        self.dispersion.iter().fold(base, |e, p| e + p.susceptibility(frequency))
    }

    /// Checks every field again (a model read from a file has not been through the
    /// constructors).
    pub fn checked(&self) -> Result<()> {
        let again = Material::new(&self.name, self.eps_r, self.mu_r)?.with_loss_tangent(self.loss_tangent)?;
        again.with_dispersion(self.dispersion.clone()).map(|_| ())
    }

    /// Free space: εr = μr = 1.
    pub fn vacuum() -> Material {
        Material { name: "vacuum".into(), eps_r: 1.0, mu_r: 1.0, loss_tangent: 0.0, dispersion: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_summaries_name_their_poles() {
        let plain = Material::new("ptfe", 2.1, 1.0).unwrap();
        assert_eq!(plain.summary(), "εr 2.1, μr 1");
        let lossy = plain.clone().with_loss_tangent(0.01).unwrap();
        assert_eq!(lossy.summary(), "εr 2.1, μr 1, tan δ 0.01");
        let d = Material::new("water", 5.0, 1.0)
            .unwrap()
            .with_dispersion(vec![Pole::Debye { delta_eps: 75.0, tau: 8e-12 }, Pole::Lorentz { delta_eps: 1.0, frequency: 1e12, damping: 1e10 }])
            .unwrap();
        assert_eq!(d.summary(), "ε∞ 5, 1 Debye + 1 Lorentz poles, μr 1");
        let one = Material::new("gas", 1.0, 1.0).unwrap().with_dispersion(vec![Pole::Drude { plasma_frequency: 1e9, damping: 1e8 }]).unwrap();
        assert_eq!(one.summary(), "ε∞ 1, 1 Drude pole, μr 1");
    }

    #[test]
    fn materials_are_checked() {
        assert!(Material::new("x", f64::NAN, 1.0).is_err());
        assert!(Material::new("x", 2.0, -1.0).is_err());
        assert!(Material::new(" ", 2.0, 1.0).is_err());
        assert!(Material::new("x", 2.0, 1.0).unwrap().with_loss_tangent(-0.1).is_err());
        assert!(Material::new("x", 2.0, 1.0).unwrap().with_loss_tangent(f64::NAN).is_err());
    }

    #[test]
    fn dispersion_is_checked_and_evaluated() {
        let x = || Material::new("x", 2.0, 1.0).unwrap();
        for bad in [
            Pole::Debye { delta_eps: f64::NAN, tau: 1e-11 },
            Pole::Debye { delta_eps: 1.0, tau: 0.0 },
            Pole::Debye { delta_eps: -1.0, tau: 1e-11 },
            Pole::Lorentz { delta_eps: 1.0, frequency: f64::INFINITY, damping: 1e9 },
            Pole::Lorentz { delta_eps: 1.0, frequency: 1e10, damping: -1.0 },
            Pole::Drude { plasma_frequency: 1e10, damping: 0.0 },
        ] {
            assert!(x().with_dispersion(vec![bad]).is_err(), "{bad:?}");
        }
        assert!(x().with_dispersion(vec![Pole::Debye { delta_eps: 1.0, tau: 1e-11 }; MAX_POLES + 1]).is_err());
        // Debye: ε(0) = ε∞ + Δε, ε(∞) = ε∞, at ωτ = 1 the loss peak ε″ = Δε/2.
        let tau = 1e-11;
        let d = x().with_dispersion(vec![Pole::Debye { delta_eps: 3.0, tau }]).unwrap();
        let e = d.eps_at(1.0 / (2.0 * PI * tau));
        assert!((e.re - 3.5).abs() < 1e-12 && (e.im + 1.5).abs() < 1e-12, "{e}");
        assert!((d.eps_at(1e-3).re - 5.0).abs() < 1e-9);
        // Lorentz: static ε∞ + Δε; above the resonance below ε∞; lossy (ε″ > 0).
        let l = x().with_dispersion(vec![Pole::Lorentz { delta_eps: 2.0, frequency: 1e10, damping: 1e9 }]).unwrap();
        assert!((l.eps_at(1.0).re - 4.0).abs() < 1e-9);
        assert!(l.eps_at(1e10).im < 0.0 && l.eps_at(3e10).re < 2.0);
        // Drude: χ = −ωp²/(ω² − jωγ).
        let (fp, g) = (1e10, 1e9);
        let dr = x().with_dispersion(vec![Pole::Drude { plasma_frequency: fp, damping: g }]).unwrap();
        let w = 2.0 * PI * 2e10;
        let expect = Complex64::new(2.0, 0.0) - (2.0 * PI * fp).powi(2) / Complex64::new(w * w, -w * 2.0 * PI * g);
        assert!((dr.eps_at(2e10) - expect).norm() < 1e-12);
        assert!(dr.eps_at(2e10).im < 0.0);
        dr.checked().unwrap();
        // Old models (no dispersion) still read; new ones round-trip.
        let old: Material = serde_json::from_str(r#"{"name":"a","eps_r":2.0,"mu_r":1.0}"#).unwrap();
        assert!(!old.is_dispersive());
        let back: Material = serde_json::from_str(&serde_json::to_string(&l).unwrap()).unwrap();
        assert_eq!(back, l);
    }
}

//! Materials: linear and isotropic, optionally lossy (dielectric and magnetic loss tangents) or
//! dispersive (poles).

use std::f64::consts::PI;

use num_complex::Complex64;
use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid, named};
pub use crate::quantity::MAX_LOSS_TANGENT;
use crate::quantity::{Hertz, LossTangent, Positive, Quantity};

/// A linear, isotropic material, lossless unless it has a loss tangent or dispersion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// Relative permittivity εr; with dispersion, its value at infinite frequency ε∞.
    pub eps_r: Positive,
    /// Relative permeability μr.
    pub mu_r: Positive,
    /// Dielectric loss tangent tan δ: the permittivity is εr (1 − j tan δ) (time dependence
    /// e^{jωt}).
    #[serde(default)]
    pub loss_tangent: LossTangent,
    /// Magnetic loss tangent tan δ_m: the permeability is μr (1 − j tan δ_m) (time dependence
    /// e^{jωt}, so μ = μ′ − jμ″ with μ″ ≥ 0).
    #[serde(default, skip_serializing_if = "LossTangent::is_zero")]
    pub magnetic_loss_tangent: LossTangent,
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
///
/// Every parameter is > 0 by its type (an undamped pole is singular at its resonance, an
/// undamped Drude pole at zero frequency).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Pole {
    /// Debye relaxation: χ = Δε / (1 + jωτ), τ the relaxation time (s).
    Debye { delta_eps: Positive, tau: Positive },
    /// Lorentz resonance: χ = Δε ω₀² / (ω₀² − ω² + jωγ), ω₀ = 2π `frequency`, γ = 2π
    /// `damping` (both in Hz).
    Lorentz { delta_eps: Positive, frequency: Hertz, damping: Hertz },
    /// Drude (free carriers, a Lorentz pole with ω₀ = 0): χ = −ω_p² / (ω² − jωγ), ω_p = 2π
    /// `plasma_frequency`, γ = 2π `damping` (both in Hz).
    Drude { plasma_frequency: Hertz, damping: Hertz },
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
    /// A Debye pole of strength `delta_eps` and relaxation time `tau` (s), checked.
    ///
    /// ```
    /// use emcraft_model::Pole;
    ///
    /// let water = Pole::debye(75.0, 8e-12)?; // Δε, τ in s
    /// assert_eq!(water.kind(), "Debye");
    /// assert!(Pole::debye(75.0, 0.0).is_err() && Pole::debye(f64::NAN, 8e-12).is_err());
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    pub fn debye(delta_eps: f64, tau: f64) -> Result<Pole> {
        Ok(Pole::Debye { delta_eps: Positive::named("Debye pole Δε", delta_eps)?, tau: Positive::named("Debye relaxation time", tau)? })
    }

    /// A Lorentz pole of strength `delta_eps` at `frequency`, damped by `damping` (both Hz),
    /// checked.
    ///
    /// ```
    /// use emcraft_model::Pole;
    ///
    /// let p = Pole::lorentz(2.0, 1e10, 1e9)?; // Δε, f₀ and γ in Hz
    /// assert_eq!(p.kind(), "Lorentz");
    /// assert!(Pole::lorentz(2.0, 1e10, -1.0).is_err());
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    pub fn lorentz(delta_eps: f64, frequency: f64, damping: f64) -> Result<Pole> {
        Ok(Pole::Lorentz {
            delta_eps: Positive::named("Lorentz pole Δε", delta_eps)?,
            frequency: Hertz::named("Lorentz resonance frequency", frequency)?,
            damping: Hertz::named("Lorentz damping", damping)?,
        })
    }

    /// A Drude pole of plasma frequency `plasma_frequency`, damped by `damping` (both Hz),
    /// checked.
    ///
    /// ```
    /// use emcraft_model::Pole;
    ///
    /// let p = Pole::drude(1e10, 1e8)?; // f_p and γ in Hz
    /// assert_eq!(p.kind(), "Drude");
    /// assert!(Pole::drude(1e10, 0.0).is_err());
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    pub fn drude(plasma_frequency: f64, damping: f64) -> Result<Pole> {
        Ok(Pole::Drude {
            plasma_frequency: Hertz::named("Drude plasma frequency", plasma_frequency)?,
            damping: Hertz::named("Drude damping", damping)?,
        })
    }
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

    /// The pole as a second-order one, or `None` for Debye.
    pub fn second_order(self) -> Option<SecondOrder> {
        match self {
            Pole::Debye { .. } => None,
            Pole::Lorentz { delta_eps, frequency, damping } => {
                let omega0 = angular(frequency.get());
                Some(SecondOrder { strength: delta_eps.get() * omega0 * omega0, omega0, gamma: angular(damping.get()) })
            }
            Pole::Drude { plasma_frequency, damping } => {
                let wp = angular(plasma_frequency.get());
                Some(SecondOrder { strength: wp * wp, omega0: 0.0, gamma: angular(damping.get()) })
            }
        }
    }

    /// The susceptibility χ(ω) at `frequency` (Hz).
    pub fn susceptibility(self, frequency: f64) -> Complex64 {
        let omega = angular(frequency);
        match (self, self.second_order()) {
            (Pole::Debye { delta_eps, tau }, _) => Complex64::new(delta_eps.get(), 0.0) / Complex64::new(1.0, omega * tau.get()),
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
            eps_r: Positive::named("relative permittivity", eps_r)?,
            mu_r: Positive::named("relative permeability", mu_r)?,
            loss_tangent: LossTangent::ZERO,
            magnetic_loss_tangent: LossTangent::ZERO,
            dispersion: Vec::new(),
        })
    }

    /// The same material with a dielectric loss tangent (finite, 0 to [`MAX_LOSS_TANGENT`]).
    pub fn with_loss_tangent(mut self, tan_d: f64) -> Result<Material> {
        self.loss_tangent = LossTangent::named("loss tangent", tan_d)?;
        Ok(self)
    }

    /// The same material with a magnetic loss tangent (finite, 0 to [`MAX_LOSS_TANGENT`]).
    pub fn with_magnetic_loss_tangent(mut self, tan_dm: f64) -> Result<Material> {
        self.magnetic_loss_tangent = LossTangent::named("magnetic loss tangent", tan_dm)?;
        Ok(self)
    }

    /// The same material with dispersion poles (at most [`MAX_POLES`]; each one's parameters
    /// are checked by their types).
    pub fn with_dispersion(mut self, poles: Vec<Pole>) -> Result<Material> {
        check_pole_count(poles.len())?;
        self.dispersion = poles;
        Ok(self)
    }

    /// The permittivity in brief for lists: "εr 2.2, μr 1, tan δ 0.01" (", tan δm 0.02" with
    /// magnetic loss), or for a dispersive material "ε∞ 2, 1 Debye + 1 Lorentz pole(s), μr 1".
    pub fn summary(&self) -> String {
        let mut loss = if self.loss_tangent.is_zero() { String::new() } else { format!(", tan δ {}", self.loss_tangent) };
        if !self.magnetic_loss_tangent.is_zero() {
            loss += &format!(", tan δm {}", self.magnetic_loss_tangent);
        }
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
        if self.is_dispersive() { (self.eps_at(frequency).norm() * self.mu_r.get()).sqrt() } else { (self.eps_r.get() * self.mu_r.get()).sqrt() }
    }

    /// The complex relative permittivity at `frequency` (Hz): εr (1 − j tan δ) + Σ χ_k(ω)
    /// (time dependence e^{jωt}, so ε = ε′ − jε″).
    pub fn eps_at(&self, frequency: f64) -> Complex64 {
        let base = Complex64::new(self.eps_r.get(), -self.eps_r.get() * self.loss_tangent.get());
        self.dispersion.iter().fold(base, |e, p| e + p.susceptibility(frequency))
    }

    /// The complex relative permeability μr (1 − j tan δ_m) (μ = μ′ − jμ″).
    pub fn mu_c(&self) -> Complex64 {
        Complex64::new(self.mu_r.get(), -self.mu_r.get() * self.magnetic_loss_tangent.get())
    }

    /// Whether the permeability has losses.
    pub fn is_magnetically_lossy(&self) -> bool {
        !self.magnetic_loss_tangent.is_zero()
    }

    /// Whether the material absorbs (a loss tangent, dielectric or magnetic, or dispersion).
    pub fn is_lossy(&self) -> bool {
        !self.loss_tangent.is_zero() || self.is_magnetically_lossy() || self.is_dispersive()
    }

    /// Checks what the types of its fields leave open: a name, at most [`MAX_POLES`] poles (a
    /// material built field by field has not been through the constructors).
    pub fn checked(&self) -> Result<()> {
        named("material name", &self.name)?;
        check_pole_count(self.dispersion.len())
    }

    /// Free space: εr = μr = 1.
    pub fn vacuum() -> Material {
        Material {
            name: "vacuum".into(),
            eps_r: Positive::ONE,
            mu_r: Positive::ONE,
            loss_tangent: LossTangent::ZERO,
            magnetic_loss_tangent: LossTangent::ZERO,
            dispersion: Vec::new(),
        }
    }
}

/// Refuses more than [`MAX_POLES`] poles.
fn check_pole_count(n: usize) -> Result<()> {
    if n > MAX_POLES { Err(invalid("dispersion", format!("holds at most {MAX_POLES} poles, got {n}"))) } else { Ok(()) }
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModelError;

    #[test]
    fn material_summaries_name_their_poles() {
        let plain = Material::new("ptfe", 2.1, 1.0).unwrap();
        assert_eq!(plain.summary(), "εr 2.1, μr 1");
        let lossy = plain.clone().with_loss_tangent(0.01).unwrap();
        assert_eq!(lossy.summary(), "εr 2.1, μr 1, tan δ 0.01");
        let d = Material::new("water", 5.0, 1.0)
            .unwrap()
            .with_dispersion(vec![Pole::debye(75.0, 8e-12).unwrap(), Pole::lorentz(1.0, 1e12, 1e10).unwrap()])
            .unwrap();
        assert_eq!(d.summary(), "ε∞ 5, 1 Debye + 1 Lorentz poles, μr 1");
        let one = Material::new("gas", 1.0, 1.0).unwrap().with_dispersion(vec![Pole::drude(1e9, 1e8).unwrap()]).unwrap();
        assert_eq!(one.summary(), "ε∞ 1, 1 Drude pole, μr 1");
    }

    /// The numbers' ranges are their types' (`quantity`); here, that each field names itself.
    #[test]
    fn materials_are_checked() {
        let what = |r: Result<Material>| match r {
            Err(ModelError::Invalid { what, .. }) => what,
            other => format!("{other:?}"),
        };
        assert_eq!(what(Material::new("x", f64::NAN, 1.0)), "relative permittivity");
        assert_eq!(what(Material::new("x", 2.0, -1.0)), "relative permeability");
        assert_eq!(what(Material::new(" ", 2.0, 1.0)), "material name");
        assert_eq!(what(Material::new("x", 2.0, 1.0).unwrap().with_loss_tangent(-0.1)), "loss tangent");
        assert_eq!(what(Material::new("x", 2.0, 1.0).unwrap().with_magnetic_loss_tangent(1e300)), "magnetic loss tangent");
        let m = Material::new("ferrite", 2.0, 3.0).unwrap().with_magnetic_loss_tangent(0.05).unwrap();
        assert!((m.mu_c() - Complex64::new(3.0, -0.15)).norm() < 1e-15);
        assert_eq!(m.summary(), "εr 2, μr 3, tan δm 0.05");
        assert!(m.is_lossy() && m.is_magnetically_lossy());
        m.checked().unwrap();
        // Old models (no magnetic loss) read as lossless; new ones round-trip; a file's numbers
        // are checked as the constructors check them.
        let old: Material = serde_json::from_str(r#"{"name":"a","eps_r":2.0,"mu_r":1.0,"loss_tangent":0.1}"#).unwrap();
        assert!(old.magnetic_loss_tangent.is_zero());
        let back: Material = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back, m);
        assert!(serde_json::from_str::<Material>(r#"{"name":"a","eps_r":2.0,"mu_r":1.0,"loss_tangent":-0.1}"#).is_err());
    }

    #[test]
    fn dispersion_is_checked_and_evaluated() {
        let x = || Material::new("x", 2.0, 1.0).unwrap();
        assert!(Pole::debye(-1.0, 1e-11).is_err() && Pole::lorentz(1.0, f64::INFINITY, 1e9).is_err() && Pole::drude(1e10, 0.0).is_err());
        let mut many = x();
        many.dispersion = vec![Pole::debye(1.0, 1e-11).unwrap(); MAX_POLES + 1];
        assert!(x().with_dispersion(many.dispersion.clone()).is_err() && many.checked().is_err());
        // Debye: ε(0) = ε∞ + Δε, ε(∞) = ε∞, at ωτ = 1 the loss peak ε″ = Δε/2.
        let tau = 1e-11;
        let d = x().with_dispersion(vec![Pole::debye(3.0, tau).unwrap()]).unwrap();
        let e = d.eps_at(1.0 / (2.0 * PI * tau));
        assert!((e.re - 3.5).abs() < 1e-12 && (e.im + 1.5).abs() < 1e-12, "{e}");
        assert!((d.eps_at(1e-3).re - 5.0).abs() < 1e-9);
        // Lorentz: static ε∞ + Δε; above the resonance below ε∞; lossy (ε″ > 0).
        let l = x().with_dispersion(vec![Pole::lorentz(2.0, 1e10, 1e9).unwrap()]).unwrap();
        assert!((l.eps_at(1.0).re - 4.0).abs() < 1e-9);
        assert!(l.eps_at(1e10).im < 0.0 && l.eps_at(3e10).re < 2.0);
        // Drude: χ = −ωp²/(ω² − jωγ).
        let (fp, g) = (1e10, 1e9);
        let dr = x().with_dispersion(vec![Pole::drude(fp, g).unwrap()]).unwrap();
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

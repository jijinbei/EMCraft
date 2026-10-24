//! Units: the model, the mesh, the solvers and the results are in SI (metres, hertz, volts per
//! metre, amperes per metre). Command parameters, replies, panels and pictures convert at the
//! boundary through this module only (docs/units.md); no conversion factor is written anywhere
//! else.

use serde::{Deserialize, Serialize};

/// Speed of light in vacuum (m/s), exact by the SI definition.
pub const C0: f64 = 299_792_458.0;
/// Vacuum permeability μ₀ (H/m), CODATA 2018.
pub const MU0: f64 = 1.256_637_062_12e-6;
/// Vacuum permittivity ε₀ = 1/(μ₀c²) (F/m).
pub const EPS0: f64 = 1.0 / (MU0 * C0 * C0);
/// Free-space wave impedance η₀ = μ₀c (Ω).
pub const ETA0: f64 = MU0 * C0;

/// A length unit of the display system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Length {
    #[serde(rename = "m")]
    Metre,
    #[default]
    #[serde(rename = "mm")]
    Millimetre,
}

impl Length {
    /// Metres per unit.
    fn factor(self) -> f64 {
        match self {
            Length::Metre => 1.0,
            Length::Millimetre => 1e-3,
        }
    }

    /// A value in this unit, in metres.
    pub fn to_si(self, v: f64) -> f64 {
        v * self.factor()
    }

    /// A value in metres, in this unit.
    pub fn from_si(self, v: f64) -> f64 {
        v / self.factor()
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Length::Metre => "m",
            Length::Millimetre => "mm",
        }
    }
}

/// A frequency unit of the display system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Frequency {
    #[serde(rename = "Hz")]
    Hertz,
    #[serde(rename = "MHz")]
    Megahertz,
    #[default]
    #[serde(rename = "GHz")]
    Gigahertz,
}

impl Frequency {
    /// Hertz per unit.
    fn factor(self) -> f64 {
        match self {
            Frequency::Hertz => 1.0,
            Frequency::Megahertz => 1e6,
            Frequency::Gigahertz => 1e9,
        }
    }

    /// A value in this unit, in hertz.
    pub fn to_si(self, v: f64) -> f64 {
        v * self.factor()
    }

    /// A value in hertz, in this unit.
    pub fn from_si(self, v: f64) -> f64 {
        v / self.factor()
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Frequency::Hertz => "Hz",
            Frequency::Megahertz => "MHz",
            Frequency::Gigahertz => "GHz",
        }
    }
}

/// The display units of a session: what parameters are read in and replies written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Display {
    pub length: Length,
    pub frequency: Frequency,
}

/// Decibels of an amplitude ratio, 20 log₁₀|s|.
pub fn decibels(magnitude: f64) -> f64 {
    20.0 * magnitude.log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for u in [Length::Metre, Length::Millimetre] {
            assert!((u.from_si(u.to_si(22.86)) - 22.86).abs() < 1e-12);
        }
        assert_eq!(Length::Millimetre.to_si(1000.0), 1.0);
        assert_eq!(Frequency::Gigahertz.to_si(1.0), 1e9);
        assert!((Frequency::Megahertz.from_si(2.5e9) - 2500.0).abs() < 1e-9);
    }

    #[test]
    fn vacuum_constants_are_consistent() {
        assert!((1.0 / (MU0 * EPS0).sqrt() - C0).abs() / C0 < 1e-15);
        assert!((ETA0 - 376.730_313).abs() < 1e-5);
        assert!((decibels(0.1) + 20.0).abs() < 1e-12);
    }
}

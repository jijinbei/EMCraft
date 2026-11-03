//! Complex values in replies, each in the form its readers expect.

use emcraft_model::units::{Display, decibels};
use emcraft_solver::Complex64 as C;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Display units as replies give them: the length's and the frequency's symbols.
pub(crate) fn units(d: Display) -> Value {
    json!({"length": d.length.symbol(), "frequency": d.frequency.symbol()})
}

/// A complex wave: parts, magnitude, dB and degrees.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wave {
    pub re: f64,
    pub im: f64,
    pub mag: f64,
    pub db: f64,
    pub deg: f64,
}

impl Wave {
    pub fn of(c: emcraft_solver::Complex64) -> Wave {
        Wave { re: c.re, im: c.im, mag: c.norm(), db: emcraft_model::units::decibels(c.norm()), deg: c.arg().to_degrees() }
    }
}

/// An S-parameter: parts, magnitude (also in dB) and phase in degrees.
pub(crate) fn complex(c: C) -> Value {
    json!({"re": c.re, "im": c.im, "mag": c.norm(), "db": decibels(c.norm()), "deg": c.arg().to_degrees()})
}

/// A reflection: parts and magnitude, also in dB.
pub(crate) fn reflection(c: C) -> Value {
    json!({"re": c.re, "im": c.im, "mag": c.norm(), "db": decibels(c.norm())})
}

/// A field or a circuit quantity's phasor: parts and magnitude.
pub(crate) fn phasor(c: C) -> Value {
    json!({"re": c.re, "im": c.im, "mag": c.norm()})
}

/// An impedance, a current or a voltage: its parts.
pub(crate) fn parts(c: C) -> Value {
    json!({"re": c.re, "im": c.im})
}

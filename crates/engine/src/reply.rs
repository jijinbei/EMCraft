//! Complex values in replies, each in the form its readers expect.

use emcraft_model::units::decibels;
use emcraft_solver::Complex64 as C;
use serde_json::{Value, json};

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

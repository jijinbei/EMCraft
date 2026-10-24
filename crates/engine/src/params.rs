//! Reading command parameters: input-derived numbers are hostile, so every value is checked
//! (finite, in range) and converted from display units to SI here.

use serde_json::Value;

use crate::{EngineError, Result, Session};

fn bad(name: &str, why: impl std::fmt::Display) -> EngineError {
    EngineError::Params(format!("`{name}` {why}"))
}

pub fn get<'a>(p: &'a Value, name: &str) -> Option<&'a Value> {
    p.get(name).filter(|v| !v.is_null())
}

/// A finite number.
pub fn number(p: &Value, name: &str) -> Result<Option<f64>> {
    match get(p, name) {
        None => Ok(None),
        Some(v) => v.as_f64().filter(|x| x.is_finite()).map(Some).ok_or_else(|| bad(name, "must be a finite number")),
    }
}

pub fn required_number(p: &Value, name: &str) -> Result<f64> {
    number(p, name)?.ok_or_else(|| bad(name, "is required"))
}

/// A finite number > 0.
pub fn positive(p: &Value, name: &str) -> Result<Option<f64>> {
    match number(p, name)? {
        Some(x) if x > 0.0 => Ok(Some(x)),
        Some(x) => Err(bad(name, format!("must be > 0, got {x}"))),
        None => Ok(None),
    }
}

/// A whole number in `range`.
pub fn integer(p: &Value, name: &str, range: std::ops::RangeInclusive<u64>) -> Result<Option<u64>> {
    match get(p, name) {
        None => Ok(None),
        Some(v) => {
            let n = v.as_u64().ok_or_else(|| bad(name, "must be a whole number ≥ 0"))?;
            if range.contains(&n) { Ok(Some(n)) } else { Err(bad(name, format!("must be {} to {}, got {n}", range.start(), range.end()))) }
        }
    }
}

pub fn string<'a>(p: &'a Value, name: &str) -> Result<Option<&'a str>> {
    match get(p, name) {
        None => Ok(None),
        Some(v) => v.as_str().map(Some).ok_or_else(|| bad(name, "must be a string")),
    }
}

pub fn required_string<'a>(p: &'a Value, name: &str) -> Result<&'a str> {
    string(p, name)?.ok_or_else(|| bad(name, "is required"))
}

impl Session {
    /// A length parameter in display units, in metres.
    pub fn length(&self, p: &Value, name: &str) -> Result<Option<f64>> {
        Ok(number(p, name)?.map(|v| self.display.length.to_si(v)))
    }

    /// A positive length in metres.
    pub fn positive_length(&self, p: &Value, name: &str) -> Result<Option<f64>> {
        Ok(positive(p, name)?.map(|v| self.display.length.to_si(v)))
    }

    /// A positive frequency in hertz.
    pub fn frequency(&self, p: &Value, name: &str) -> Result<Option<f64>> {
        Ok(positive(p, name)?.map(|v| self.display.frequency.to_si(v)))
    }

    /// A frequency (Hz) as a value in display units.
    pub fn show_frequency(&self, hz: f64) -> f64 {
        self.display.frequency.from_si(hz)
    }

    pub fn show_length(&self, m: f64) -> f64 {
        self.display.length.from_si(m)
    }
}

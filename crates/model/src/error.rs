//! The model's error type and the checks its constructors and setters share.

/// Why a model change was refused.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ModelError {
    #[error("invalid {what}: {why}")]
    Invalid { what: String, why: String },
    #[error("{kind} `{name}` already exists")]
    Duplicate { kind: &'static str, name: String },
    #[error("unknown {kind} `{name}`")]
    Unknown { kind: &'static str, name: String },
}

pub type Result<T> = std::result::Result<T, ModelError>;

/// An [`ModelError::Invalid`] naming `what` and `why`.
pub(crate) fn invalid(what: impl Into<String>, why: impl Into<String>) -> ModelError {
    ModelError::Invalid { what: what.into(), why: why.into() }
}

/// A finite number > 0.
pub fn positive(what: &str, v: f64) -> Result<f64> {
    if v.is_finite() && v > 0.0 { Ok(v) } else { Err(invalid(what, format!("must be a finite number > 0, got {v}"))) }
}

/// Every value finite, else "`what` must be finite".
pub(crate) fn all_finite(what: impl Into<String>, values: &[f64]) -> Result<()> {
    if values.iter().all(|v| v.is_finite()) { Ok(()) } else { Err(invalid(what, "must be finite")) }
}

/// A name with something besides white space in it.
pub(crate) fn named(what: &str, name: &str) -> Result<()> {
    if name.trim().is_empty() { Err(invalid(what, "must not be empty")) } else { Ok(()) }
}

/// Whether `x` and `y` agree component by component within `tol`.
pub(crate) fn near<const N: usize>(x: [f64; N], y: [f64; N], tol: f64) -> bool {
    x.iter().zip(y).all(|(a, b)| (a - b).abs() <= tol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_name_what_they_refuse() {
        assert_eq!(positive("width", 2.0), Ok(2.0));
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(matches!(positive("width", bad), Err(ModelError::Invalid { what, .. }) if what == "width"));
        }
        assert!(all_finite("strip", &[1.0, 2.0]).is_ok());
        assert_eq!(all_finite("strip", &[1.0, f64::NAN]), Err(invalid("strip", "must be finite")));
        assert!(named("name", " \t").is_err() && named("name", "a").is_ok());
        assert!(near([1.0, 2.0], [1.0 + 1e-12, 2.0], 1e-9) && !near([1.0], [1.1], 1e-9));
    }
}

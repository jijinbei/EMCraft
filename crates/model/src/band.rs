//! The job's frequency band.

use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid};

/// A frequency band [start, stop] (Hz).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Band {
    pub start: f64,
    pub stop: f64,
}

impl Band {
    /// The band, checked: finite, 0 < start ≤ stop.
    pub fn new(start: f64, stop: f64) -> Result<Band> {
        if !(start.is_finite() && stop.is_finite() && start > 0.0 && stop >= start) {
            return Err(invalid("frequency band", format!("must be finite with 0 < start ≤ stop, got {start} to {stop}")));
        }
        Ok(Band { start, stop })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_run_upwards_from_above_zero() {
        assert_eq!(Band::new(1e9, 1e9), Ok(Band { start: 1e9, stop: 1e9 }));
        for (start, stop) in [(0.0, 1e9), (2e9, 1e9), (1e9, f64::INFINITY), (f64::NAN, 1e9)] {
            assert!(Band::new(start, stop).is_err(), "{start} to {stop}");
        }
    }
}

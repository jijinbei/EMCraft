//! The job's frequency band.

use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid};
use crate::quantity::Hertz;

/// A frequency band [start, stop] (Hz), start ≤ stop (each frequency finite and > 0 by its
/// type).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Band {
    pub start: Hertz,
    pub stop: Hertz,
}

impl Band {
    /// The band from `start` to `stop`, checked: start ≤ stop.
    ///
    /// ```
    /// use emcraft_model::{Band, Hertz};
    ///
    /// let (f6, f9) = (Hertz::new(6e9)?, Hertz::new(9e9)?); // Hz
    /// assert_eq!(Band::new(f6, f9)?.stop.get(), 9e9);
    /// assert!(Band::new(f9, f6).is_err());
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    pub fn new(start: Hertz, stop: Hertz) -> Result<Band> {
        if stop < start {
            return Err(invalid("frequency band", format!("must have start ≤ stop, got {start} to {stop}")));
        }
        Ok(Band { start, stop })
    }
}

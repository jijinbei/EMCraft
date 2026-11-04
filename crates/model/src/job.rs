//! The job: the kind of analysis and the run's settings, in one type. A job is open (nothing
//! chosen: the kind is inferred from the model), chosen (a kind, its settings still to confirm)
//! or ready (settings confirmed, `project.setup`). Only a ready job runs from the workflow; the
//! settings are one [`Setup`] per kind, so a kind's settings cannot be missing or another kind's.

use std::num::NonZeroUsize;

use serde::{Deserialize, Serialize};

use crate::analysis::Analysis;
use crate::band::Band;
use crate::error::{Result, invalid};
use crate::quantity::{Hertz, Metres};

/// The most eigenmodes a run may ask for.
pub const MAX_MODES: usize = 50;

/// The finite element's order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum Order {
    One,
    #[default]
    Two,
}

impl TryFrom<u8> for Order {
    type Error = String;
    fn try_from(n: u8) -> std::result::Result<Order, String> {
        match n {
            1 => Ok(Order::One),
            2 => Ok(Order::Two),
            _ => Err(format!("element order must be 1 or 2, got {n}")),
        }
    }
}

impl From<Order> for u8 {
    fn from(o: Order) -> u8 {
        match o {
            Order::One => 1,
            Order::Two => 2,
        }
    }
}

/// A kind's confirmed settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Setup {
    /// S-parameters at `points` frequencies over `band`.
    Frequency { band: Band, points: NonZeroUsize, order: Order },
    /// `count` modes near `near`.
    Eigenmodes { near: Hertz, count: NonZeroUsize, order: Order },
    /// The radiation at `frequency`.
    Radiation { frequency: Hertz, order: Order },
    /// A pulse over `band` on a grid of `cell`, its spectrum at `points` frequencies; one run
    /// for each port of `excite` (none: the first).
    TimeDomain {
        band: Band,
        points: NonZeroUsize,
        cell: Metres,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        excite: Vec<String>,
    },
}

impl Setup {
    /// The kind of analysis the settings are for.
    pub const fn analysis(&self) -> Analysis {
        match self {
            Setup::Frequency { .. } => Analysis::Frequency,
            Setup::Eigenmodes { .. } => Analysis::Eigenmodes,
            Setup::Radiation { .. } => Analysis::Radiation,
            Setup::TimeDomain { .. } => Analysis::TimeDomain,
        }
    }

    /// The frequencies the run works over (Hz): the band, or the one frequency twice.
    ///
    /// ```
    /// use emcraft_model::{Band, Hertz, Order, Setup};
    /// use std::num::NonZeroUsize;
    ///
    /// let band = Band::new(Hertz::new(6e9)?, Hertz::new(9e9)?)?; // Hz
    /// let s = Setup::Frequency { band, points: NonZeroUsize::MIN, order: Order::Two };
    /// assert_eq!(s.range().stop.get(), 9e9);
    /// assert_eq!(Setup::Radiation { frequency: Hertz::new(1e9)?, order: Order::One }.range().start.get(), 1e9);
    /// # Ok::<(), emcraft_model::ModelError>(())
    /// ```
    pub fn range(&self) -> Band {
        match self {
            Setup::Frequency { band, .. } | Setup::TimeDomain { band, .. } => *band,
            Setup::Eigenmodes { near: f, .. } | Setup::Radiation { frequency: f, .. } => Band { start: *f, stop: *f },
        }
    }
}

/// The job: open, a kind chosen, or ready to run.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", content = "of", rename_all = "snake_case")]
pub enum Job {
    /// Nothing chosen: the kind is inferred from the model.
    #[default]
    Open,
    /// A kind chosen; its settings are still to confirm.
    Chosen(Analysis),
    /// Settings confirmed.
    Ready(Setup),
}

impl Job {
    /// The kind chosen, if one is.
    pub const fn chosen(&self) -> Option<Analysis> {
        match self {
            Job::Open => None,
            Job::Chosen(a) => Some(*a),
            Job::Ready(s) => Some(s.analysis()),
        }
    }

    /// The confirmed settings, if any.
    pub const fn setup(&self) -> Option<&Setup> {
        match self {
            Job::Ready(s) => Some(s),
            _ => None,
        }
    }

    /// The frequencies the confirmed run works over (Hz), if any.
    pub fn range(&self) -> Option<Band> {
        self.setup().map(Setup::range)
    }

    /// The job with kind `a` chosen (`None`: open again): confirmed settings of that kind stay.
    pub fn choose(&self, a: Option<Analysis>) -> Job {
        match (self, a) {
            (_, None) => Job::Open,
            (Job::Ready(s), Some(a)) if s.analysis() == a => self.clone(),
            (_, Some(a)) => Job::Chosen(a),
        }
    }
}

/// A count of points or modes, checked: from 1 to `max`.
pub fn count(what: &str, n: usize, max: usize) -> Result<NonZeroUsize> {
    NonZeroUsize::new(n).filter(|n| n.get() <= max).ok_or_else(|| invalid(what, format!("must be from 1 to {max}, got {n}")))
}

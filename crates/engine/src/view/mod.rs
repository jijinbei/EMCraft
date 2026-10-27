//! Pictures of results, the same headless and in the desktop app, each subject in its own
//! module:
//!
//! - `field`: the electric and magnetic fields on a plane through the model;
//! - `port_mode`: a port mode's shape across the port's face;
//! - `sparameters`: S-parameters against frequency over the curves of the `reference` (the exact
//!   TE₁₀ cascade, mode matching, …) they are compared with;
//! - `tdr`: time-domain reflectometry;
//! - `dispersion`: a periodic cell's dispersion diagram;
//! - `pattern`: an open problem's far-field pattern;
//! - `study`: a parameter study's sweep or optimization.
//!
//! The `view.*` commands that write them as PNGs are in `commands`; what the charts share
//! (axes, the dB scale) in `chart`.

mod chart;
pub(crate) mod commands;
mod dispersion;
mod field;
mod pattern;
mod port_mode;
mod reference;
mod sparameters;
pub(crate) mod study;
mod tdr;

pub use commands::field_view;
pub use dispersion::dispersion_chart;
pub use field::{Component, FieldView, Plane, PlaneSamples, Probe, field_plot, robust_peak, sample_plane};
pub use pattern::{pattern_chart, pattern_chart_of};
pub use port_mode::port_mode_plot;
pub use reference::{ExactLine, MODE_MATCHING_MODES, Reference, reference, section_cutoffs, te10_line};
pub use sparameters::sparameter_chart;
pub use tdr::tdr_chart;

/// The model's axes by index.
pub(crate) const AXIS_NAMES: [&str; 3] = ["x", "y", "z"];

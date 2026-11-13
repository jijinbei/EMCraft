//! EMCraft's charged particles (docs/adr/0012-particles.md): the relativistic Boris and Vay
//! pushes ([`push`]) and particles tracked through fields along a line ([`track`]). The fields
//! come through a trait, so the same push serves a solved eigenmode or sweep (tracking, the
//! engine's `results.track`) and, later, the particle-in-cell grid.
//!
//! Units are SI throughout: metres, seconds, volts per metre, teslas; a particle's momentum is
//! carried as u = γv in metres per second.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod push;
pub mod track;

pub use push::{ELECTRON, Pusher, Species, V3, gamma, push, velocity};
pub use track::{End, Fields, Line, MAX_STEPS, Point, Sample, Settings, Start, TrackError, Trajectory, track};

#[cfg(test)]
mod tests;

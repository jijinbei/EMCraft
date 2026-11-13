//! EMCraft's frequency-domain solver: the curl–curl equation on Nédélec elements, waveguide
//! ports, a multifrontal complex symmetric LDLᵀ factorization, S-parameters and fields.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod accel;
pub mod adapt;
pub mod bloch;
pub mod bor;
pub mod coupling;
pub mod dense;
pub mod driven;
pub mod eigen;
pub mod ldlt;
pub mod lu;
pub mod memory;
mod multifrontal;
pub mod ordering;
pub mod portmode;
pub mod radiation;
pub mod smatrix;
pub mod sparse;
mod subspace;
pub mod sweep;
pub mod zdense;

pub use driven::{Driven, Field, FrequencyResult, SPort, SolveError};
pub use num_complex::Complex64;
pub use smatrix::{MixedWave, SMatrix, mixed_order};

/// An iteration's eigenvalue has converged when two successive estimates agree to this fraction
/// of its size: a thousand times the factorization's round-off, about the most it can reach.
pub(crate) const CONVERGED: f64 = 1e-11;

/// A new basis vector left shorter than this fraction of its length before orthogonalisation
/// lies in the span already built (the basis has broken down).
pub(crate) const BREAKDOWN: f64 = 1e-10;

/// Two coordinates are the same within this fraction of the length they are measured on (a
/// port's width, the model's size): far above the round-off of metres from display units.
pub(crate) const GEOMETRY_TOLERANCE: f64 = 1e-9;

/// The threads the solvers' parallel loops have (rayon's pool): the results do not depend on
/// it (each sum is taken in one fixed order), only the time.
///
/// ```
/// assert!(emcraft_solver::threads() >= 1);
/// ```
pub fn threads() -> usize {
    rayon::current_num_threads().max(1)
}

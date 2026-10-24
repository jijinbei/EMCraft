//! EMCraft's frequency-domain solver: the curl–curl equation on Nédélec elements, waveguide
//! ports, a multifrontal complex symmetric LDLᵀ factorization, S-parameters and fields.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod ldlt;
pub mod ordering;
pub mod sparse;

pub use num_complex::Complex64;

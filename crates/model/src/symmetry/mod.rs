//! Symmetry: planes that mirror a model ([`mirror`]), a rotation that turns it onto itself
//! ([`rotation`]) and its meridian profile as a body of revolution ([`meridian`]).

pub mod meridian;
pub mod mirror;
pub mod rotation;

pub use mirror::{Found, Image, Symmetry, SymmetryKind, mirror_group};

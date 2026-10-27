//! EMCraft's command engine. Everything a user can do is a command ([`CommandSpec`]: a dotted,
//! lower-case id, a label, documented parameters and a JSON `run`); the desktop UI, scripts and
//! the command line all reach the same commands through [`Session::execute`], which checks the
//! parameters, runs the command under a panic guard and restores the session when it fails.
//!
//! Parameters and replies are in the session's display units (millimetres and gigahertz by
//! default, [`emcraft_model::units`]); the model and the solvers are in SI.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod adapt;
mod args;
pub mod bodies;
mod cmd;
pub mod expr;
mod geometry;
mod meshing;
mod numbers;
pub mod open;
pub mod params;
pub mod periodic;
pub mod progress;
pub mod project;
mod reply;
mod results;
pub mod sample;
mod session;
mod solve;
pub mod study;
pub mod tdr;
pub mod view;
pub mod workflow;

use std::fmt;

pub use cmd::{COMMANDS, CommandSpec, Kind, ParamSpec, Quantity, features_markdown, find_command};
pub use params::Params;
pub use project::Effect;
pub use session::{Change, Limits, Results, Session, Sweep};
pub use solve::Compute;
pub use view::field_view;

/// results.tdr's chart, for the UI.
pub fn results_tdr_chart(s: &Session, p: &Params) -> Result<emcraft_render::chart::Chart> {
    let (t, port, z0) = results::tdr_of(s, p)?;
    Ok(view::tdr_chart(&t, &port, z0))
}

#[derive(Clone, Debug, thiserror::Error, PartialEq)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("{0}")]
    Params(String),
    #[error("{0}")]
    Failed(String),
    #[error("internal error in `{0}` (the session was restored): please report it")]
    Panicked(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

impl EngineError {
    /// A command that could not do its work, for `why`.
    pub fn failed(why: impl fmt::Display) -> EngineError {
        EngineError::Failed(why.to_string())
    }
}

/// The lower layers' errors: the command failed for the reason they give.
macro_rules! failed_from {
    ($($error:ty),* $(,)?) => {$(
        impl From<$error> for EngineError {
            fn from(e: $error) -> Self {
                EngineError::failed(e)
            }
        }
    )*};
}

failed_from!(
    emcraft_model::ModelError,
    emcraft_mesh::MeshError,
    emcraft_fdtd::FdtdError,
    emcraft_solver::SolveError,
    emcraft_theory::TheoryError,
    emcraft_io::Error,
);

/// The most eigenmodes one command may ask for.
pub(crate) const EIGEN_MAX: usize = emcraft_solver::eigen::MAX_MODES;

#[cfg(test)]
mod tests;

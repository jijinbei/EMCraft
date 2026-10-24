//! EMCraft's unstructured tetrahedral mesher, adapted from FieldCraft's `crates/mesh` (MIT OR
//! Apache-2.0, see ATTRIBUTION.md): exact and adaptive geometric predicates ([`predicates`]),
//! boundary-surface preparation and checks ([`surface`]), Bowyer–Watson Delaunay
//! tetrahedralization ([`delaunay`]), constrained boundary recovery after Diazzi et al. 2023
//! ([`cdt`]), the quality stage — graded size field, Delaunay refinement, smoothing and
//! improvement ([`tet`]) — and element quality ([`quality`]). The papers each algorithm comes
//! from are cited in the modules.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod cdt;
pub mod compat;
pub mod delaunay;
pub mod message;
pub mod predicates;
pub mod quality;
pub mod seeds;
pub mod size;
pub mod surface;
pub mod tet;
mod vec3;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("limit reached: {0}")]
    Limit(String),
    #[error("meshing failed: {0}")]
    Failed(String),
    /// The input is valid but outside what the mesher handles yet; the message says why.
    #[error("not supported yet: {0}")]
    Unsupported(String),
    /// The boundary surface has defects ([`surface::BoundarySurface::new`]).
    #[error("invalid boundary surface: {0}")]
    Surface(surface::Defects),
    /// A message that names lengths, areas, volumes or points ([`message`]).
    #[error("{0}: {1}")]
    Measured(Kind, message::Message),
}

/// What kind of failure a [`Error::Measured`] is, as the other variants say it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Invalid,
    Limit,
    Failed,
    Unsupported,
    Surface,
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Kind::Invalid => "invalid input",
            Kind::Limit => "limit reached",
            Kind::Failed => "meshing failed",
            Kind::Unsupported => "not supported yet",
            Kind::Surface => "invalid boundary surface",
        })
    }
}

impl Error {
    /// Invalid input, the message naming numbers.
    pub fn invalid(m: message::Message) -> Self {
        Error::Measured(Kind::Invalid, m)
    }
    /// A limit reached, the message naming numbers.
    pub fn limit(m: message::Message) -> Self {
        Error::Measured(Kind::Limit, m)
    }
    /// A failure, the message naming numbers.
    pub fn failed(m: message::Message) -> Self {
        Error::Measured(Kind::Failed, m)
    }

    /// The error as a message, its numbers as data (the other variants' text as it is).
    pub fn message(&self) -> message::Message {
        match self {
            Error::Measured(k, m) => message![format!("{k}: ")].then(m.clone()),
            Error::Surface(d) => message![format!("{}: ", Kind::Surface)].then(d.message()),
            e => message![e.to_string()],
        }
    }

    /// What kind of failure it is (`None` for a model error).
    pub fn kind(&self) -> Option<Kind> {
        match self {
            Error::Invalid(_) => Some(Kind::Invalid),
            Error::Limit(_) => Some(Kind::Limit),
            Error::Failed(_) => Some(Kind::Failed),
            Error::Unsupported(_) => Some(Kind::Unsupported),
            Error::Surface(_) => Some(Kind::Surface),
            Error::Measured(k, _) => Some(*k),
        }
    }

    /// The error's message without its kind ("limit reached: …"), its numbers as data.
    pub fn detail(&self) -> message::Message {
        match self {
            Error::Invalid(s) | Error::Limit(s) | Error::Failed(s) | Error::Unsupported(s) => message![s.clone()],
            Error::Measured(_, m) => m.clone(),
            Error::Surface(d) => d.message(),
        }
    }

    /// Whether a limit was reached.
    pub fn is_limit(&self) -> bool {
        self.kind() == Some(Kind::Limit)
    }

    /// The error with every number its message names shown in `units` (a surface's defects
    /// too): what the caller shows its user.
    pub fn shown(self, units: &dyn message::Units) -> Self {
        match self {
            Error::Measured(k, m) => Error::Measured(k, m.shown(units)),
            Error::Surface(d) => Error::Measured(Kind::Surface, message::Message::from(d.render(units))),
            e => e,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

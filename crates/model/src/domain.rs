//! The analysis domain: the region solved (its shape and what fills it) and what bounds it —
//! its outer walls (a conductor, or open into free space through a PML) and the planes that
//! cut it (symmetry, periodic boundaries, a rotational sector). Its bodies are the model's
//! ([`crate::Model::features`], [`crate::Model::inclusions`], the waveguide).

use serde::{Deserialize, Serialize};

use crate::open::Open;
use crate::periodic::Periodic;
use crate::quantity::{Conductivity, Metres};
use crate::shape::Solid;
use crate::symmetry::Symmetry;

/// The region solved and what bounds it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Domain {
    /// The region's shape and the material filling it (none with a waveguide: the guide is the
    /// region).
    #[serde(default)]
    pub solid: Option<Solid>,
    /// How the region is set: pinned as the solid was, or automatic round what the model holds
    /// ([`crate::region`]).
    #[serde(default, skip_serializing_if = "crate::model::is_pinned")]
    pub region: crate::region::Region,
    /// The background an automatic region is filled with before it has a solid (vacuum when
    /// none); the region's material once it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    /// The conductivity (S/m) of the outer walls (and a guide's irises and steps); perfectly
    /// conducting when `None`.
    #[serde(default)]
    pub wall_conductivity: Option<Conductivity>,
    /// The walls' thickness (m): drawn round the guide, and with a conductivity the layer the
    /// surface impedance sees through (never meshed); infinitely thick when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_thickness: Option<Metres>,
    /// An open boundary (radiation into free space through a PML) in place of the walls.
    #[serde(default)]
    pub open: Option<Open>,
    /// Symmetry planes (at most one across x and one across y): the region is meshed and
    /// solved on the side below them only.
    #[serde(default)]
    pub symmetry: Vec<Symmetry>,
    /// The symmetry planes were found for an eigenmode search with no port to decide their
    /// walls: every combination of walls is solved and the modes merged
    /// ([`crate::Model::with_walls`]); their kinds as set are the first combination's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub every_wall: bool,
    /// Periodic boundaries: the region is the unit cell of an infinite array.
    #[serde(default)]
    pub periodic: Option<Periodic>,
    /// Rotational symmetry: the region is meshed and solved on one sector of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<crate::symmetry::rotation::Rotation>,
}

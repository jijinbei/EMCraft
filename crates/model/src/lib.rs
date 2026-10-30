//! EMCraft's solver-neutral electromagnetic model: materials, the waveguide geometry, ports, the
//! tetrahedral mesh and the model check. Everything is stored in SI ([`units`]); the model knows
//! no file format, no solver and no display unit.
//!
//! The first geometry is a rectangular waveguide along z: a cross-section of width `a` (x) and
//! height `b` (y), and sections along z each filled with one material. Its side walls are
//! perfect electric conductors; its two end faces carry the ports.
//!
//! The modules, each re-exported here:
//! - [`error`]: [`ModelError`] and the checks the setters share;
//! - [`material`]: materials and their dispersion poles;
//! - [`waveguide`]: the rectangular guide, its sections and planar circuit (layers, strips, vias);
//! - [`shape`]: solids, the inclusions inside them and the features combined with them;
//! - [`port`]: waveguide ports;
//! - [`open`]: the open boundary, sources and lumped ports;
//! - [`symmetry`]: symmetry planes and the images they make;
//! - [`periodic`]: periodic boundaries (unit cells of infinite arrays);
//! - [`band`]: the frequency band;
//! - [`analysis`]: the kind of analysis the job is set up for;
//! - [`model`]: the [`Model`] and its checked setters;
//! - [`edit`]: changing and removing the model's items, each checked as a new one would be;
//! - [`measure`]: distances, gaps and angles between picked points, faces and edges.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod analysis;
pub mod band;
pub mod edit;
pub mod error;
pub mod geom;
pub mod material;
pub mod measure;
pub mod mesh;
pub mod model;
pub mod open;
pub mod periodic;
pub mod port;
pub mod prism;
pub mod shape;
pub mod symmetry;
pub mod units;
pub mod waveguide;

pub use analysis::Analysis;
pub use band::Band;
pub use edit::MaterialUse;
pub use error::{ModelError, Result, positive};
pub use geom::{Aabb, CVec3, Point3, Vec3};
pub use material::{MAX_POLES, Material, Pole, SecondOrder};
pub use mesh::{Boundary, BoundaryFace, Buckets, Grid, Mesh};
pub use model::{Issue, Model};
pub use num_complex::Complex64;
pub use open::{LumpedPort, Open, OpenShape, Source, SphericalLayer};
pub use periodic::{Periodic, Polarization};
pub use port::{DEFAULT_TERMINAL_IMPEDANCE, End, Frame, MAX_PORT_MODES, MAX_TERMINALS, ModeSolver, Port, PortFace, PortShape, Terminals};
pub use prism::MAX_PRISM_POINTS;
pub use shape::{Axis, Combine, FaceSurface, Feature, Inclusion, MAX_SURFACE_TRIANGLES, Shape, Solid, solid_angle};
pub use symmetry::{Image, Symmetry, SymmetryKind};
pub use waveguide::{Layer, Section, Span, Strip, Via, Waveguide};

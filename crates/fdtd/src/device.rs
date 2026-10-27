//! Running the waveguide leapfrog somewhere other than the CPU's threads: the run prepared as
//! plain arrays (a [`Plan`]), and what steps it (a [`Stepper`], such as `emcraft-fdtd-gpu`'s).
//! The CPU prepares, transforms and checks; the device only steps the fields, injects the
//! source and projects E_y on the two planes.

use std::ops::ControlFlow;

use crate::Result;
pub use crate::dispersion::{NO_SLOT, Poles};
use crate::yee::Pulse;

/// A waveguide run prepared for a device, in SI units and f64.
///
/// Each component lives on its own lattice of points (E_a a half cell along a, nodes across;
/// H_a nodes along a, half cells across), indexed i + dims₀ (j + dims₁ k) with x fastest.
pub struct Plan {
    /// Cells along x, y and z.
    pub n: [usize; 3],
    /// The time step (s).
    pub dt: f64,
    /// The dispersive edges and their poles, if any: such an edge advances its polarizations
    /// with E (the auxiliary differential equations) instead of E += c ∇×H.
    pub poles: Option<Poles>,
    /// dt/ε on the E edges of each component, 0 on and in conductors (the edge is not updated).
    pub e: [Vec<f64>; 3],
    /// dt/μ on the H faces of each component, 0 where neither cell exists.
    pub h: [Vec<f64>; 3],
    /// The cells' steps along each axis (n of them).
    pub primal: [Vec<f64>; 3],
    /// The steps between cell centres, half cells at the ends (n + 1 of them).
    pub dual: [Vec<f64>; 3],
    /// The CPML's (b, a) at the z nodes, for E (nz + 1 of them): ψ ← b ψ + a ∂.
    pub cpml_e: Vec<[f64; 2]>,
    /// The CPML's (b, a) at the cell centres along z, for H (nz of them).
    pub cpml_h: Vec<[f64; 2]>,
    /// The z node of the source's plane.
    pub source_plane: usize,
    /// The source's profile on the E_y nodes of a plane, (nx + 1) × ny, x fastest: after each
    /// step E_y += s(t) × profile on edges that are updated.
    pub source: Vec<f64>,
    /// The z nodes of the two port planes.
    pub planes: [usize; 2],
    /// Each plane's projection weights on its E_y nodes (the TE₁₀ profile times each node's
    /// dual area), laid out as `source`: v = Σ E_y w, summed in that order.
    pub weights: [Vec<f64>; 2],
    pub(crate) pulse: Pulse,
}

impl Plan {
    /// The source's amplitude s(t) injected after step `n` (from 0), at t = (n + 1) Δt.
    pub fn source_at(&self, n: usize) -> f64 {
        self.pulse.at((n + 1) as f64 * self.dt)
    }
}

/// What steps a [`Plan`].
pub trait Stepper {
    /// Runs up to `max_steps` steps. A step is H, then E, then the source, then the two
    /// projections; `observe(n, [v₁, v₂])` must see every step's projections in order, and
    /// breaks to stop. A device may run ahead of `observe` (by a batch of steps); it returns
    /// the steps observed.
    fn leapfrog(&self, plan: &Plan, max_steps: usize, observe: &mut dyn FnMut(usize, [f64; 2]) -> ControlFlow<()>) -> Result<usize>;
}

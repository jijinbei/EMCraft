//! Running the waveguide leapfrog somewhere other than the CPU's threads: the run prepared as
//! plain arrays (a [`Plan`]), and what steps it (a [`Stepper`], such as `emcraft-gpu`'s).
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

/// An open problem's run prepared for a device: the box's uniform grid lined by a CPML on all
/// faces, the lumped ports and current elements, the Huygens box's samples. Lattices and
/// indices as [`Plan`]'s.
pub struct OpenPlan {
    /// Cells along x, y and z, and their size along each (m).
    pub n: [usize; 3],
    pub hs: [f64; 3],
    pub dt: f64,
    /// dt/ε on the E edges of each component, 0 on conductors and the box's walls.
    pub ce: [Vec<f64>; 3],
    /// dt/μ₀ on every H face.
    pub ch: f64,
    /// The CPML's (b, a) along each axis: at the nodes for E (n + 1), at the half cells for H
    /// (n). Component a's two auxiliary variables absorb its derivatives along u and v, the
    /// axes across it in cyclic order.
    pub cpml_e: [Vec<[f64; 2]>; 3],
    pub cpml_h: [Vec<[f64; 2]>; 3],
    pub ports: Vec<PortPlan>,
    pub currents: Vec<CurrentPlan>,
    pub samples: Vec<SamplePlan>,
    pub frequencies: Vec<f64>,
    /// Steps between the device's reports ([`OpenBatch`]).
    pub batch: usize,
    pub(crate) pulse: Pulse,
}

impl OpenPlan {
    /// The sources' amplitude s(t) in step `n` (from 0), at t = (n + ½) Δt.
    pub fn source_at(&self, n: usize) -> f64 {
        self.pulse.at((n as f64 + 0.5) * self.dt)
    }
}

/// A lumped port: a resistive voltage source on a run of E edges along `axis` (Piket-May et
/// al. 1994). After E's plain update, each edge is redone semi-implicitly from E before the
/// step (`old`): curl = (E − old)/c, vs = voltage · s · sign, E = ca old + cb curl − cv vs; with
/// e = (old + E)/2 the port's current sums (vs + h e)/r_edge · sign over its edges (then divided
/// by their number) and its voltage −h e · sign, h the cell along the axis.
pub struct PortPlan {
    pub axis: usize,
    pub edges: Vec<PortEdgePlan>,
    pub r_edge: f64,
    pub sign: f64,
    pub voltage: f64,
}

pub struct PortEdgePlan {
    pub id: usize,
    pub ca: f64,
    pub cb: f64,
    pub cv: f64,
}

/// A current element: after the ports, E[id] −= coefficient · s on component `axis`.
pub struct CurrentPlan {
    pub axis: usize,
    pub id: usize,
    pub coefficient: f64,
}

/// A sample of the Huygens box: the centre of the cell face normal to `axis` at node `plane`,
/// at cells (iu, iv) across (u, v the axes across in cyclic order). Its tangential E is E_u
/// averaged over v and v + 1 and E_v over u and u + 1 at (plane, iu, iv); its H is H_u averaged
/// over u, u + 1 and a = plane − 1, plane, and H_v over v, v + 1 and the same along a.
pub struct SamplePlan {
    pub axis: usize,
    pub plane: usize,
    pub iu: usize,
    pub iv: usize,
}

/// What a device reports after a batch of steps.
pub struct OpenBatch<'a> {
    /// The batch's first step and its number of steps.
    pub first: usize,
    pub steps: usize,
    /// Each step's (voltage, current) of each port, step-major.
    pub ports: &'a [[f64; 2]],
    /// The running transforms on the Huygens box after the batch, Σ field · Δt e^{−jωt} (E at
    /// t = (n + 1) Δt, H at (n + ½) Δt): per frequency, per sample, (re, im) of x, y, z.
    pub e: &'a [f64],
    pub h: &'a [f64],
}

/// What steps an [`OpenPlan`].
pub trait OpenStepper {
    /// Runs up to `max_steps` steps in batches of `plan.batch` (the last one shorter), handing
    /// each to `observe`, which breaks to stop. Returns the steps run.
    fn radiate(&self, plan: &OpenPlan, max_steps: usize, observe: &mut dyn FnMut(OpenBatch<'_>) -> ControlFlow<()>) -> Result<usize>;
}

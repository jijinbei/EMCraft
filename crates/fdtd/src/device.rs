//! Running the waveguide leapfrog somewhere other than the CPU's threads: the run prepared as
//! plain arrays (a [`Plan`]), and what steps it (a [`Stepper`], such as `emcraft-gpu`'s).
//! The CPU prepares, transforms and checks; the device only steps the fields, injects the
//! source and projects E_y on the two planes.

use std::ops::ControlFlow;

pub use crate::dispersion::{NO_SLOT, Poles};
pub use crate::yee::Cpml;
use crate::yee::{Pulse, lattice_lengths};
use crate::{FdtdError, Result, invalid};

/// A grid axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    /// The three axes in order.
    pub const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    /// The axis' component index: 0, 1 or 2.
    pub const fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }

    /// The two axes across this one, in cyclic order: x → (y, z), y → (z, x), z → (x, y).
    pub const fn across(self) -> (Axis, Axis) {
        match self {
            Axis::X => (Axis::Y, Axis::Z),
            Axis::Y => (Axis::Z, Axis::X),
            Axis::Z => (Axis::X, Axis::Y),
        }
    }
}

impl From<Axis> for usize {
    fn from(a: Axis) -> usize {
        a.index()
    }
}

impl From<Axis> for u32 {
    fn from(a: Axis) -> u32 {
        a.index() as u32
    }
}

impl TryFrom<usize> for Axis {
    type Error = FdtdError;

    fn try_from(i: usize) -> Result<Axis> {
        Axis::ALL.get(i).copied().ok_or_else(|| invalid("axis", format!("{i} is not 0, 1 or 2")))
    }
}

/// Whether arrays per component have the lengths `lens`.
fn sized(arrays: &[Vec<f64>; 3], lens: [usize; 3]) -> bool {
    arrays.iter().zip(lens).all(|(a, l)| a.len() == l)
}

/// Whether `h_decay` is empty (no magnetic losses) or one value per H face.
fn decay_fits(h_decay: &[Vec<f64>; 3], lens: [usize; 3]) -> bool {
    h_decay.iter().all(Vec::is_empty) || sized(h_decay, lens)
}

/// A plan whose arrays do not match its grid.
fn mismatch() -> FdtdError {
    invalid("plan", "its arrays do not match its grid")
}

/// A waveguide run prepared for a device, in SI units and f64.
///
/// Each component lives on its own lattice of points (E_a a half cell along a, nodes across;
/// H_a nodes along a, half cells across), indexed i + dims₀ (j + dims₁ k) with x fastest.
pub struct Plan {
    /// Cells along x, y and z.
    pub n: [usize; 3],
    /// The time step (s).
    pub dt: f64,
    /// The dispersive and lossy edges (and the walls' edges with finite conductivity), if any:
    /// such an edge advances its terms with E (the auxiliary differential equations) instead of
    /// E += c ∇×H.
    pub poles: Option<Poles>,
    /// dt/ε on the E edges of each component, 0 on and in perfect conductors (the edge is not
    /// updated). With finite wall conductivity the walls' edges are updated, the H beyond the
    /// grid's outer walls taken as zero.
    pub e: [Vec<f64>; 3],
    /// dt/μ on the H faces of each component, 0 where neither cell exists; with magnetic
    /// losses, (dt/μ)/(1 + σ_m dt/2μ).
    pub h: [Vec<f64>; 3],
    /// With magnetic losses, the decay of H on each face, H ← decay H − h ∇×E; empty (1
    /// everywhere) without.
    pub h_decay: [Vec<f64>; 3],
    /// The cells' steps along each axis (n of them).
    pub primal: [Vec<f64>; 3],
    /// The steps between cell centres, half cells at the ends (n + 1 of them).
    pub dual: [Vec<f64>; 3],
    /// The CPML's (b, a) at the z nodes, for E (nz + 1 of them): ψ ← b ψ + a ∂.
    pub cpml_e: Vec<Cpml>,
    /// The CPML's (b, a) at the cell centres along z, for H (nz of them).
    pub cpml_h: Vec<Cpml>,
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

    /// Refuses a plan whose arrays do not match its grid.
    pub fn validate(&self) -> Result<()> {
        let [nx, ny, nz] = self.n;
        let [lens_e, lens_h] = lattice_lengths(self.n).ok_or_else(mismatch)?;
        let plane = (nx + 1) * ny;
        let agree = sized(&self.e, lens_e)
            && sized(&self.h, lens_h)
            && self.primal.iter().zip(self.n).all(|(d, n)| d.len() == n)
            && self.dual.iter().zip(self.n).all(|(d, n)| d.len() == n + 1)
            && self.cpml_e.len() == nz + 1
            && self.cpml_h.len() == nz
            && self.source.len() == plane
            && self.weights.iter().all(|w| w.len() == plane)
            && self.source_plane <= nz
            && self.planes.iter().all(|&k| k <= nz)
            && self.poles.as_ref().is_none_or(|p| p.fit(lens_e))
            && decay_fits(&self.h_decay, lens_h);
        if agree { Ok(()) } else { Err(mismatch()) }
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
    /// The dispersive and lossy edges, if any, as [`Plan::poles`].
    pub poles: Option<Poles>,
    /// dt/μ on the H faces of each component (with magnetic losses, the exponential update's).
    pub ch: [Vec<f64>; 3],
    /// With magnetic losses, the decay of H on each face, as [`Plan::h_decay`]; empty without.
    pub h_decay: [Vec<f64>; 3],
    /// The CPML's (b, a) along each axis: at the nodes for E (n + 1), at the half cells for H
    /// (n). Component a's two auxiliary variables absorb its derivatives along u and v, the
    /// axes across it in cyclic order.
    pub cpml_e: [Vec<Cpml>; 3],
    pub cpml_h: [Vec<Cpml>; 3],
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

    /// Refuses a plan whose arrays do not match its grid, or whose ports, current elements or
    /// samples lie off it.
    pub fn validate(&self) -> Result<()> {
        let n = self.n;
        let [lens_e, lens_h] = lattice_lengths(n).ok_or_else(mismatch)?;
        let on_edges = |axis: Axis, id: usize| id < lens_e[axis.index()];
        let agree = sized(&self.ce, lens_e)
            && sized(&self.ch, lens_h)
            && self.poles.as_ref().is_none_or(|p| p.fit(lens_e))
            && decay_fits(&self.h_decay, lens_h)
            && self.cpml_e.iter().zip(n).all(|(c, n)| c.len() == n + 1)
            && self.cpml_h.iter().zip(n).all(|(c, n)| c.len() == n)
            && self.ports.iter().all(|q| !q.edges.is_empty() && q.edges.iter().all(|e| on_edges(q.axis, e.id)))
            && self.currents.iter().all(|c| on_edges(c.axis, c.id))
            && self.samples.iter().all(|s| {
                let (u, v) = s.axis.across();
                s.plane >= 1 && s.plane < n[s.axis.index()] && s.iu + 1 < n[u.index()] && s.iv + 1 < n[v.index()]
            })
            && self.batch > 0;
        if agree { Ok(()) } else { Err(mismatch()) }
    }
}

/// A lumped port: a resistive voltage source on a run of E edges along `axis` (Piket-May et
/// al. 1994). After E's plain update, each edge is redone semi-implicitly from E before the
/// step (`old`): curl = (E − old)/c, vs = voltage · s · sign, E = ca old + cb curl − cv vs; with
/// e = (old + E)/2 the port's current sums (vs + h e)/r_edge · sign over its edges (then divided
/// by their number) and its voltage −h e · sign, h the cell along the axis.
pub struct PortPlan {
    pub axis: Axis,
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

/// A current element: after the ports, E[id] −= coefficient · s on component `axis`; on a
/// slotted edge ([`OpenPlan::poles`]) its Debye and surface terms then gain kb times the same
/// change (they took Eⁿ⁺¹).
pub struct CurrentPlan {
    pub axis: Axis,
    pub id: usize,
    pub coefficient: f64,
}

/// A sample of the Huygens box: the centre of the cell face normal to `axis` at node `plane`,
/// at cells (iu, iv) across (u, v the axes across in cyclic order). Its tangential E is E_u
/// averaged over v and v + 1 and E_v over u and u + 1 at (plane, iu, iv); its H is H_u averaged
/// over u, u + 1 and a = plane − 1, plane, and H_v over v, v + 1 and the same along a.
pub struct SamplePlan {
    pub axis: Axis,
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

//! Driven frequency-domain analysis of a waveguide or a body with ports: the vector wave equation
//!
//! ∇ × (μr⁻¹ ∇ × E) − k₀² εr E = 0
//!
//! on Nédélec elements, tangential E = 0 on the perfectly conducting walls, and modal port
//! conditions on the end faces (J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd
//! ed., Wiley 2014, §8.5 and §9.2).
//!
//! **Ports.** On a port face with outward normal n̂, the field is a sum of the guide's modes e_m
//! ([`emcraft_fem::modes`]): an incident amplitude a_m arriving and an amplitude b_m leaving. For
//! each mode, n̂ × ∇ × E_t = κ_m (c_m(E) − 2a_m) e_m, where c_m(E) = ⟨e_m, E⟩/⟨e_m, e_m⟩ =
//! a_m + b_m is the mode's coefficient on the face. Put into the weak form, with
//! v_m,i = ∫_port Nᵢ · e_m dS, this adds the rank-one terms (κ_m/μr) v_m v_mᵀ/‖e_m‖² to the
//! matrix and 2(κ_m/μr) a_m v_m to the right-hand side. The condition is exact for any field
//! made of the modes it carries, propagating or evanescent; a port should be far enough from a
//! discontinuity that the modes it leaves out have decayed.
//!
//! A port whose cross-section is not a whole rectangle or circle of one material — a coaxial
//! line, a ridge, a polygon — takes its modes from [`crate::portmode`] at each frequency, hybrid
//! ones included: H_t = ŝ × (e_t + ∇φ) β/(ωμ) leaving it, so its rank-one terms are
//! (jβ/D) v vᵀ with v_i = ∫ μr⁻¹ Nᵢ · (e_t + ∇φ) dS and D = ∫ μr⁻¹ e_t · (e_t + ∇φ) dS, which
//! for a TE mode are the closed forms' (κ = jβ, D = ‖e‖²/μr).
//!
//! **Symmetry planes.** A model symmetric about x = a/2 or y = b/2 may be solved on the half below
//! the plane ([`emcraft_model::Symmetry`]), closed there by an electric wall — its tangential
//! unknowns eliminated, the essential condition, as on the conducting walls — or a magnetic wall
//! — its unknowns kept with no surface term, the natural condition n̂ × (μr⁻¹ ∇ × E) = 0 (Jin
//! 2014, ch. 8; image theory: R. F. Harrington, *Time-Harmonic Electromagnetic Fields*, 1961,
//! ch. 3). The half carries the fields of one parity about the plane: a closed-form port keeps
//! the modes of that parity (TE_m0 with m odd for a magnetic wall at x = a/2), its vectors
//! integrated over the half face and its norms ‖e‖² over the half (exactly half the whole for a
//! mode of definite parity), so the S-parameters are the whole model's. A numerical port's
//! cross-section is the half, its rim free on a magnetic wall; its line impedance, by power and
//! current, is the half's halved per magnetic plane (both halve). Fields outside the half are
//! the images of those inside.
//!
//! **S-parameters.** Each port is excited in turn by a unit TE₁₀ amplitude, all from one
//! factorization. S_pq = (c₁₀ at p − δ_pq) for excitation at q, normalised to equal power
//! (times √(P_p/P_q), P = ‖e‖² β/μ the power of a unit wave), so S is unitary for a lossless
//! structure even when the ports' media or apertures differ.
//!
//! **Losses.** A lossy dielectric makes the mass matrix complex, εr(1 − j tan δ); a magnetic
//! loss tangent the curl–curl matrix, μr⁻¹ → (μr(1 − j tan δ_m))⁻¹, so K is complex too (the
//! system stays complex symmetric). A port may lie in a lossy or dispersive material that fills
//! its cross-section: its modes then have complex γ = α + jβ and κ (`ports`), and S is read
//! as pseudo-waves referred to each port's own wave impedance (R. B. Marks, D. F. Williams,
//! *A general waveguide circuit theory*, J. Res. NIST 97(5), 1992), normalised by the modes'
//! unconjugated powers so that S stays reciprocal — the power waves when the ports are
//! lossless (`sparams`). De-embedding moves the planes by the complex γ, attenuation included.
//!
//! **Dispersive materials.** A material with Debye, Lorentz or Drude poles
//! ([`emcraft_model::Pole`]) has a permittivity εr(ω) of its own at each frequency. Its
//! tetrahedra are kept out of the assembled mass matrix: each dispersive material has its own
//! (unit-permittivity) mass matrix M_d, and the system at ω is K − k₀² (M + Σ_d εr,d(ω) M_d) —
//! exact at every frequency, the permittivity evaluated in closed form (A. Taflove, S. C. Hagness,
//! *Computational Electrodynamics*, 3rd ed., Artech House 2005, §9.2–9.4, for the pole forms).
//!
//! The parts, each in a module of its own: the unknowns ([`Dofs`]), the materials of the
//! tetrahedra (`media`), the volume and wall matrices and a frequency's system (`assembly`), the
//! perfectly matched layer (`pml`), the ports' modes (`ports`) and their terms at a frequency
//! (`terms`), lumped ports (`lumped`), the system factored and solved for each excitation
//! (`solution`), S read from the solutions (`sparams`), fields (`fields`), losses (`losses`),
//! and the periodic cell: its reduction (`periodic`) and its Floquet ports (`floquet`).

use std::f64::consts::PI;

use emcraft_fem::{Element, Order};
use emcraft_mesh::Topology;
use emcraft_model::units::C0;
use emcraft_model::{Boundary, Hertz, Mesh, Model, Quantity, Symmetry, SymmetryKind};
use num_complex::Complex64 as C;
use rayon::prelude::*;

use crate::ldlt::FactorError;
use crate::smatrix::SMatrix;
use crate::sparse::Csr;

mod assembly;
mod dofs;
mod fields;
mod floquet;
mod losses;
mod lumped;
mod media;
mod periodic;
mod pml;
mod ports;
mod solution;
mod sparams;
mod terminals;
mod terms;

pub(crate) use assembly::{System, rule_points};
pub use dofs::Dofs;
pub use floquet::{FloquetMode, FloquetPort, FloquetTerm};
pub(crate) use losses::wall_factor;
pub use losses::{surface_resistance, wall_impedance};
pub use lumped::LumpedData;
pub use periodic::Tie;
use pml::Layer;
pub use pml::PML_REFLECTION;
pub use ports::{MAX_PATTERN_SIDE, ModeInfo, ModePattern, NumericPort, PortData, PortModes, kept_by, port_mode_pattern, port_modes};
pub use sparams::ModeTerm;
pub use terminals::SPort;
pub use terms::PortTerm;

use assembly::Space;
use losses::WallLoss;
use media::Media;

/// The most unknowns a problem may have.
pub const MAX_UNKNOWNS: usize = 2_000_000;
/// The most frequencies one sweep may solve.
pub const MAX_FREQUENCIES: usize = 2001;
/// The most moments a full solve adds to a fast sweep's basis per excitation
/// ([`Driven::solve_spanning`]).
pub const MAX_MOMENTS: usize = 4;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SolveError {
    #[error("the model is not ready to solve: {0}")]
    NotReady(String),
    #[error("invalid {what}: {why}")]
    Invalid { what: String, why: String },
    #[error("{0}")]
    Element(#[from] emcraft_fem::ElementError),
    #[error(transparent)]
    Model(#[from] emcraft_model::ModelError),
    #[error("{0}")]
    Factor(#[from] FactorError),
    #[error("{unknowns} unknowns exceed the limit of {limit}: use a larger element size or order 1")]
    TooLarge { unknowns: usize, limit: usize },
}

pub type Result<T> = std::result::Result<T, SolveError>;

pub(crate) fn invalid(what: impl Into<String>, why: impl Into<String>) -> SolveError {
    SolveError::Invalid { what: what.into(), why: why.into() }
}

/// An index kept in 32 bits, or none (`u32::MAX`, as the mesh's topology marks a missing
/// tetrahedron): a degree of freedom's unknown, none where it is eliminated.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Slot(u32);

impl Slot {
    pub(crate) const NONE: Slot = Slot(u32::MAX);

    /// Index `i`; none when it does not fit 32 bits (the limits on unknowns keep every index
    /// far below).
    pub(crate) fn new(i: usize) -> Slot {
        u32::try_from(i).map_or(Slot::NONE, Slot)
    }

    /// The index, `None` for none.
    pub(crate) fn get(self) -> Option<usize> {
        (self != Slot::NONE).then_some(self.0 as usize)
    }
}

/// A raw 32-bit index, `u32::MAX` for none.
impl From<u32> for Slot {
    fn from(i: u32) -> Slot {
        Slot(i)
    }
}

/// The (position, index) of each slot of `slots` that is not none, in order.
pub(crate) fn present(slots: &[Slot]) -> impl Iterator<Item = (usize, usize)> + '_ {
    slots.iter().enumerate().filter_map(|(i, s)| Some((i, s.get()?)))
}

/// The free-space wavenumber k₀ = 2πf/c (1/m) at `frequency` (Hz).
pub(crate) fn wavenumber(frequency: f64) -> f64 {
    2.0 * PI * frequency / C0
}

/// Refuses a frequency that breaks [`Hertz`]'s rule (finite, > 0): for the public calls that
/// take a frequency as an `f64` (the model's own frequencies are [`Hertz`] already).
pub(crate) fn check_frequency(frequency: f64) -> Result<()> {
    if Hertz::keeps(frequency) { Ok(()) } else { Err(invalid("frequency", format!("must be a finite number {}, got {frequency}", Hertz::MUST))) }
}

/// A waveguide meshed and assembled up to the frequency: the curl–curl and mass matrices and
/// the port vectors.
#[derive(Clone, Debug)]
pub struct Driven {
    pub order: Order,
    pub mesh: Mesh,
    pub topo: Topology,
    pub dofs: Dofs,
    /// Each tetrahedron's permittivity and permeability.
    pub(crate) media: Media,
    pub(crate) pattern: Csr,
    pub(crate) stiffness: Vec<C>,
    /// The mass matrix of the non-dispersive tetrahedra, weighted by their complex permittivity.
    pub(crate) mass: Vec<C>,
    /// Per dispersive material ([`Media::dispersive_materials`], in that order), its
    /// unit-permittivity mass matrix on the pattern (its tetrahedra are not in `mass`).
    pub(crate) dispersive_mass: Vec<Vec<f64>>,
    /// Walls of finite conductivity.
    pub(crate) wall: Option<WallLoss>,
    /// The boundary faces that are walls (indices into the mesh's).
    walls: Vec<usize>,
    pub ports: Vec<PortData>,
    /// The perfectly matched layer and its tetrahedra, whose matrices depend on the frequency
    /// and are assembled with it.
    pub(crate) pml: Option<Layer>,
    /// Lumped ports: each with its vector cᵢ = ∫ Nᵢ · ŝ dl along its gap.
    pub lumped: Vec<LumpedData>,
    /// The model's symmetry planes: the mesh is the part below them.
    pub symmetry: Vec<Symmetry>,
    /// The model's periodic boundaries and the reduction they make (a unit cell).
    pub periodic: Option<emcraft_model::Periodic>,
    pub tie: Option<Tie>,
    /// The symbolic factorization of the system's pattern, made at the first frequency and
    /// shared by the others (the pattern does not change with the frequency).
    analysis: std::sync::OnceLock<crate::ldlt::Analysis>,
}

/// The solution at one frequency.
#[derive(Clone, Debug, PartialEq)]
pub struct FrequencyResult {
    pub frequency: f64,
    /// S[p][q]: at port p for a unit wave of port q's first mode into it; power waves of the
    /// modes, or referred to the ports' reference impedances where set.
    pub s: SMatrix,
    /// Each port's line impedance (Ω), where its first mode has one.
    pub impedances: Vec<Option<f64>>,
    /// The field coefficients for each excitation (port q).
    pub fields: Vec<Vec<C>>,
    /// The worst backward error of the solves.
    pub backward_error: f64,
    /// Pivots replaced by the factorization's static pivoting.
    pub perturbed_pivots: usize,
    /// Entries of the factor and its largest frontal matrix (rows).
    pub factor_entries: usize,
    pub largest_front: usize,
    /// Where the time went (seconds).
    pub timings: Timings,
    /// Solved in full (the whole system factorized), not in a fast sweep's reduced basis.
    pub full: bool,
}

/// Seconds spent at one frequency, by stage. Not part of a result's value: two results compare
/// equal whatever their timings.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timings {
    /// The system: matrix, layer, port modes and terms.
    pub assembly: f64,
    /// The ordering and symbolic factorization (0 when an earlier frequency's was reused).
    pub analysis: f64,
    /// The numeric factorization.
    pub factor: f64,
    /// The solves with iterative refinement and the S-parameters.
    pub solve: f64,
}

impl PartialEq for Timings {
    fn eq(&self, _: &Timings) -> bool {
        true
    }
}

/// Which field to evaluate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    E,
    H,
}

/// Whether the model's waveguide ports are kept or its guide closed there by conducting walls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ends {
    Ports,
    Closed,
}

/// The walls' conductivity: the model's, or perfectly conducting whatever it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Walls {
    Modelled,
    Perfect,
}

/// The mesh's boundary faces by role.
struct BoundaryFaces {
    /// Each port's faces: an end's tag, or the faces on a port's rectangle.
    ports: Vec<Vec<usize>>,
    /// The rest but the symmetry planes.
    walls: Vec<usize>,
    /// The faces whose tangential unknowns are eliminated: the walls when they conduct
    /// perfectly, and the electric symmetry planes.
    fixed: Vec<usize>,
}

impl BoundaryFaces {
    fn classify(model: &Model, mesh: &Mesh, ends: Ends, perfect_walls: bool) -> Result<BoundaryFaces> {
        // Faces on a periodic boundary are neither walls nor ports: they are tied to the
        // opposite face.
        let tied = |i: usize| model.periodic.as_ref().is_some_and(|p| periodic::on_tied_face(mesh, p, &mesh.boundary[i].nodes));
        let ports: Vec<Vec<usize>> = match ends {
            Ends::Ports => model.ports.iter().map(|p| ports::faces_of_port(mesh, p)).collect::<Result<_>>()?,
            Ends::Closed => Vec::new(),
        };
        let on_port: std::collections::HashSet<usize> = ports.iter().flatten().copied().collect();
        // Symmetry planes are not walls: an electric one is fixed whatever the walls' conductivity,
        // a magnetic one left free.
        let walls: Vec<usize> = (0..mesh.boundary.len())
            .filter(|i| !on_port.contains(i) && !matches!(mesh.boundary[*i].tag, Boundary::Symmetry(_)) && !tied(*i))
            .collect();
        let electric = (0..mesh.boundary.len()).filter(|i| mesh.boundary[*i].tag == Boundary::Symmetry(SymmetryKind::Electric));
        let fixed: Vec<usize> = if perfect_walls { walls.iter().copied().chain(electric).collect() } else { electric.collect() };
        Ok(BoundaryFaces { ports, walls, fixed })
    }
}

impl Driven {
    /// Prepares the model's waveguide (meshed) for solving with elements of `order`.
    pub fn new(model: &Model, order: Order) -> Result<Driven> {
        Driven::build(model, order, Ends::Ports, Walls::Modelled)
    }

    /// The model's waveguide closed at both ends by conducting walls (a cavity), its ports left
    /// out: the matrices of an eigenmode analysis ([`crate::eigen`]).
    pub fn closed(model: &Model, order: Order) -> Result<Driven> {
        refuse_open(model)?;
        Driven::build(model, order, Ends::Closed, Walls::Modelled)
    }

    /// The cavity with perfectly conducting walls whatever their conductivity: the lossless
    /// problem whose modes perturbation theory gives Q factors from.
    pub fn cavity(model: &Model, order: Order) -> Result<Driven> {
        refuse_open(model)?;
        Driven::build(model, order, Ends::Closed, Walls::Perfect)
    }

    /// The model's open problem: its box lined with the perfectly matched layer
    /// ([`emcraft_model::Open`]), driven by sources, lumped ports or its waveguide ports on faces
    /// inside it ([`crate::radiation`]).
    pub fn open(model: &Model, order: Order) -> Result<Driven> {
        if model.open.is_none() {
            return Err(SolveError::NotReady("no open boundary: line the box with model.open".into()));
        }
        let ends = if model.ports.is_empty() { Ends::Closed } else { Ends::Ports };
        Driven::build(model, order, ends, Walls::Modelled)
    }

    fn build(model: &Model, order: Order, ends: Ends, walls: Walls) -> Result<Driven> {
        let issues: Vec<String> =
            model.check().into_iter().filter(|i| ends == Ends::Ports || *i != emcraft_model::Issue::NoPorts).map(|i| i.to_string()).collect();
        if !issues.is_empty() {
            return Err(SolveError::NotReady(issues.join("; ")));
        }
        let mesh = model.mesh.as_deref().cloned().ok_or_else(|| SolveError::NotReady("no mesh".into()))?;
        let topo = Topology::new(&mesh);
        // The conductivity and thickness are checked by their types.
        let sigma = match walls {
            Walls::Modelled => model.wall_conductivity.map(|c| c.get()),
            Walls::Perfect => None,
        };
        let faces = BoundaryFaces::classify(model, &mesh, ends, sigma.is_none())?;
        if let Some(why) = model.symmetry_issue() {
            return Err(invalid("symmetry", why));
        }
        let dofs = Dofs::new(order, &mesh, &topo, &faces.fixed)?;
        let media = Media::new(model, &mesh)?;
        let pml = Layer::of(model, &mesh)?;
        let space = Space { mesh: &mesh, topo: &topo, dofs: &dofs };
        let ports = model
            .ports
            .iter()
            .zip(&faces.ports)
            .map(|(p, f)| match &model.periodic {
                Some(per) => Ok(PortData {
                    name: p.name.clone(),
                    end: p.end,
                    modes: PortModes::Floquet(Box::new(floquet::prepare(model, per, p, f)?)),
                    reference: None,
                    deembed: 0.0,
                    terminals: 0,
                }),
                None => ports::prepare(model, p, &space, f),
            })
            .collect::<Result<Vec<_>>>()?;
        let lumped = model.lumped.iter().map(|lp| lumped::prepare(lp, &space)).collect::<Result<Vec<_>>>()?;
        // (The ports' rank-one terms border the matrix instead of filling it: see `system`.)
        let pattern = space.pattern(lumped.iter().map(|l| l.c.iter().map(|(u, _)| *u).collect()));
        let in_pml = pml.as_ref().map(Layer::tets).unwrap_or_default();
        let volume = space.volume_matrices(&pattern, &media, |t| !in_pml.contains(&t))?;
        let tie = match &model.periodic {
            Some(per) => {
                if !lumped.is_empty() || pml.is_some() {
                    return Err(invalid("periodic boundaries", "not supported yet: lumped ports or a matched layer in a periodic cell"));
                }
                Some(periodic::Tie::new(&mesh, &topo, &dofs, per, &pattern)?)
            }
            None => None,
        };
        let wall = match sigma {
            Some(sigma) => Some(WallLoss { sigma, thickness: model.wall_thickness.map(|t| t.get()), mass: space.wall_mass(&pattern, &faces.walls)? }),
            None => None,
        };
        Ok(Driven {
            order,
            dofs,
            media,
            stiffness: volume.stiffness,
            mass: volume.mass,
            dispersive_mass: volume.dispersive,
            pattern,
            wall,
            walls: faces.walls,
            ports,
            pml,
            lumped,
            symmetry: model.symmetry.clone(),
            periodic: model.periodic,
            tie,
            analysis: std::sync::OnceLock::new(),
            mesh,
            topo,
        })
    }

    /// The finite-element space the problem is posed in.
    pub(crate) fn space(&self) -> Space<'_> {
        Space { mesh: &self.mesh, topo: &self.topo, dofs: &self.dofs }
    }

    /// Solves at one frequency (Hz).
    pub fn solve(&self, frequency: f64) -> Result<FrequencyResult> {
        Ok(self.solve_spanning(frequency, 0)?.0)
    }

    /// Solves at one frequency (Hz) as [`Driven::solve`] does, and from the same factorization
    /// the next `moments` vectors of each excitation's Krylov sequence y_{j+1} = A⁻¹ M y_j,
    /// y₀ = x: the field's derivatives in k₀² up to the ports' (A = K − k₀² M + …, so
    /// ∂x/∂k₀² = A⁻¹ M x when the ports' terms are held), for a reduced basis to span the
    /// frequencies around this one (multi-point moment matching: R. D. Slone, R. Lee, J.-F. Lee,
    /// *Multipoint Galerkin asymptotic waveform evaluation for model order reduction of
    /// frequency domain FEM electromagnetic radiation problems*, IEEE Trans. Antennas Propag.
    /// 49(10):1504–1513, 2001). Each extra vector costs a solve, not a factorization; at most
    /// [`MAX_MOMENTS`]. A periodic cell gives none.
    pub(crate) fn solve_spanning(&self, frequency: f64, moments: usize) -> Result<(FrequencyResult, Vec<Vec<C>>)> {
        check_frequency(frequency)?;
        if self.tie.is_some() {
            return Ok((self.solve_periodic(frequency)?, Vec::new()));
        }
        let t0 = std::time::Instant::now();
        let system = self.system(frequency)?;
        let assembly = t0.elapsed().as_secs_f64();
        let f = self.factor(&system)?;
        let System { a, rhs, terms, border } = system;
        let n = self.dofs.unknowns;
        let mut solved = solution::solve_each(&f, &a, &rhs, n, assembly)?;
        let dispersive = self.dispersive_scales(frequency);
        let k2 = wavenumber(frequency).powi(2);
        let mut extra: Vec<Vec<C>> = Vec::new();
        let mut last: Vec<Vec<C>> = if moments > 0 { solved.fields.clone() } else { Vec::new() };
        for _ in 0..moments.min(MAX_MOMENTS) {
            // M y with the permittivities at this frequency (k₀² εr,d M_d scaled back by k₀²).
            let rhs: Vec<Vec<C>> = last
                .iter()
                .map(|y| {
                    let mut b: Vec<C> = (0..n)
                        .into_par_iter()
                        .map(|i| {
                            self.pattern
                                .row(i)
                                .map(|k| {
                                    let mut m = self.mass[k];
                                    for (c, md) in dispersive.iter().zip(&self.dispersive_mass) {
                                        m += c / k2 * md[k];
                                    }
                                    m * y[self.pattern.col[k]]
                                })
                                .sum()
                        })
                        .collect();
                    b.resize(n + border, C::new(0.0, 0.0));
                    b
                })
                .collect();
            // Unrefined: the vectors only span the basis, whose residual is checked in full.
            last = f
                .solve_refined_many(&a, &rhs, 0)?
                .into_iter()
                .map(|(mut y, _)| {
                    y.truncate(n);
                    y
                })
                .collect();
            extra.extend(last.iter().cloned());
        }
        let (s, impedances) = self.s_parameters(&terms, &mut solved.fields, frequency)?;
        Ok((solved.result(frequency, s, impedances), extra))
    }

    pub(crate) fn element_of(&self, t: usize) -> Result<Element> {
        self.space().element(t)
    }

    pub(crate) fn dofs_of(&self, t: usize) -> impl Iterator<Item = Slot> + '_ {
        self.dofs.of_tet(&self.topo, t)
    }

    /// Tetrahedron `t`'s complex relative permittivity at `frequency` (Hz).
    pub(crate) fn eps_at(&self, t: usize, frequency: f64) -> C {
        self.media.eps_at(t, frequency)
    }

    /// Whether any material's permittivity depends on the frequency.
    pub fn is_dispersive(&self) -> bool {
        !self.media.dispersive_materials().is_empty()
    }

    pub(crate) fn mu_of(&self, t: usize) -> C {
        self.media.mu(t)
    }

    /// The system's matrix at `frequency` without the ports' terms, on the full unknowns (for
    /// checks of the periodic reduction).
    pub fn matrix_without_ports(&self, frequency: f64) -> Result<Csr> {
        check_frequency(frequency)?;
        self.unbordered(frequency)
    }

    /// The curl–curl matrix K (complex permeability), the mass matrix M (complex permittivity)
    /// and their sparsity pattern, for eigenmode analysis.
    pub(crate) fn matrices(&self) -> (&Csr, &[C], &[C]) {
        (&self.pattern, &self.stiffness, &self.mass)
    }
}

/// Cavities are closed: an open model's resonances would be complex.
fn refuse_open(model: &Model) -> Result<()> {
    if model.open.is_some() {
        return Err(invalid(
            "eigenmodes",
            "not supported yet: an open model's resonances (complex frequencies); close the box (model.open without a thickness)",
        ));
    }
    Ok(())
}

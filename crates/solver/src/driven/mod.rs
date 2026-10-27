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
//! (`terms`), lumped ports (`lumped`), S read from the solutions (`sparams`), fields (`fields`)
//! and losses (`losses`).

use std::f64::consts::PI;

use emcraft_fem::{Element, Order};
use emcraft_mesh::Topology;
use emcraft_model::units::C0;
use emcraft_model::{Boundary, Mesh, Model, Point3, Symmetry, SymmetryKind};
use num_complex::Complex64 as C;

use crate::ldlt::{FactorError, Ldlt};
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
mod sparams;
mod terms;

pub(crate) use assembly::{System, rule_points};
pub use dofs::Dofs;
pub use floquet::{FloquetMode, FloquetPort, FloquetTerm};
pub use losses::surface_resistance;
pub(crate) use losses::wall_factor;
pub use lumped::LumpedData;
pub use periodic::Tie;
use pml::Layer;
pub use pml::PML_REFLECTION;
pub use ports::{MAX_PATTERN_SIDE, ModeInfo, ModePattern, NumericPort, PortData, PortModes, kept_by, port_mode_pattern, port_modes};
pub use terms::PortTerm;

use assembly::Space;
use losses::WallLoss;
use media::Media;

/// The most unknowns a problem may have.
pub const MAX_UNKNOWNS: usize = 2_000_000;
/// The most frequencies one sweep may solve.
pub const MAX_FREQUENCIES: usize = 2001;

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

pub(crate) const NONE: u32 = u32::MAX;

/// The free-space wavenumber k₀ = 2πf/c (1/m) at `frequency` (Hz).
pub(crate) fn wavenumber(frequency: f64) -> f64 {
    2.0 * PI * frequency / C0
}

/// Refuses a frequency that is not a finite number > 0.
pub(crate) fn check_frequency(frequency: f64) -> Result<()> {
    if !(frequency.is_finite() && frequency > 0.0) {
        return Err(invalid("frequency", format!("must be a finite number > 0, got {frequency}")));
    }
    Ok(())
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
    pub(crate) stiffness: Vec<f64>,
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
        let issues: Vec<String> = model.check().into_iter().filter(|i| ends == Ends::Ports || !i.starts_with("no ports")).collect();
        if !issues.is_empty() {
            return Err(SolveError::NotReady(issues.join("; ")));
        }
        let mesh = model.mesh.clone().ok_or_else(|| SolveError::NotReady("no mesh".into()))?;
        let topo = Topology::new(&mesh);
        let sigma = match walls {
            Walls::Modelled => model.wall_conductivity,
            Walls::Perfect => None,
        };
        if let Some(c) = sigma
            && !(c.is_finite() && c > 0.0)
        {
            return Err(invalid("wall conductivity", format!("must be a finite number > 0, got {c}")));
        }
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
            Some(sigma) => Some(WallLoss { sigma, mass: space.wall_mass(&pattern, &faces.walls)? }),
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

    /// Factors a [`System`], its border last.
    pub(crate) fn factor(&self, s: &System) -> Result<Ldlt> {
        let budget = crate::ldlt::memory_budget();
        if let Some(an) = self.analysis.get().filter(|an| an.fits(&s.a)) {
            let mut f = Ldlt::factor_analysed(&s.a, an, budget)?;
            f.analysis_seconds = 0.0;
            return Ok(f);
        }
        let points: Vec<Point3> = self.dofs.points.iter().copied().chain(std::iter::repeat_n(Point3::ORIGIN, s.border)).collect();
        let an = crate::ldlt::Analysis::new(&s.a, &points, s.border)?;
        let f = Ldlt::factor_analysed(&s.a, &an, budget)?;
        // Kept for the next frequencies (the first analysis stays when two race).
        let _ = self.analysis.set(an);
        Ok(f)
    }

    /// Solves at one frequency (Hz).
    pub fn solve(&self, frequency: f64) -> Result<FrequencyResult> {
        if self.tie.is_some() {
            return self.solve_periodic(frequency);
        }
        check_frequency(frequency)?;
        let t0 = std::time::Instant::now();
        let system = self.system(frequency)?;
        let assembly = t0.elapsed().as_secs_f64();
        let f = self.factor(&system)?;
        let t1 = std::time::Instant::now();
        let System { a, rhs, terms, .. } = system;
        let mut fields = Vec::with_capacity(rhs.len());
        let mut worst: f64 = 0.0;
        for b in &rhs {
            let (mut x, e) = f.solve_refined(&a, b, 4)?;
            x.truncate(self.dofs.unknowns);
            worst = worst.max(e);
            fields.push(x);
        }
        let (s, impedances) = self.s_parameters(&terms, &fields)?;
        Ok(FrequencyResult {
            frequency,
            s,
            impedances,
            fields,
            backward_error: worst,
            perturbed_pivots: f.perturbed,
            factor_entries: f.entries,
            largest_front: f.largest_front,
            timings: Timings { assembly, analysis: f.analysis_seconds, factor: f.numeric_seconds, solve: t1.elapsed().as_secs_f64() },
        })
    }

    pub(crate) fn element_of(&self, t: usize) -> Result<Element> {
        self.space().element(t)
    }

    pub(crate) fn dofs_of(&self, t: usize) -> impl Iterator<Item = u32> + '_ {
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

    pub(crate) fn mu_of(&self, t: usize) -> f64 {
        self.media.mu(t)
    }

    /// The curl–curl matrix K, the mass matrix M (complex permittivity) and their sparsity
    /// pattern, for eigenmode analysis.
    pub(crate) fn matrices(&self) -> (&Csr, &[f64], &[C]) {
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

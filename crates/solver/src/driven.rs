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
//! **S-parameters.** Each port is excited in turn by a unit TE₁₀ amplitude, all from one
//! factorization. S_pq = (c₁₀ at p − δ_pq) for excitation at q, normalised to equal power
//! (times √(P_p/P_q), P = ‖e‖² β/μ the power of a unit wave), so S is unitary for a lossless
//! structure even when the ports' media or apertures differ.

use emcraft_fem::nedelec::LOCAL_EDGES;
use std::f64::consts::PI;

use emcraft_fem::modes::{self, Mode};
use emcraft_fem::{Element, Order};
use emcraft_mesh::Topology;
use emcraft_model::units::{C0, MU0};
use emcraft_model::{Boundary, End, Mesh, Model};
use emcraft_model::{CVec3, Point3, Vec3};
use num_complex::Complex64 as C;

use crate::ldlt::{FactorError, Ldlt};
use crate::smatrix::SMatrix;
use crate::sparse::Csr;

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

/// Degrees of freedom: per edge and per face, those on conducting walls eliminated.
#[derive(Clone, Debug)]
pub struct Dofs {
    pub order: Order,
    pub edges: usize,
    pub faces: usize,
    /// Global DOF → unknown, [`NONE`] when eliminated.
    unknown: Vec<u32>,
    pub unknowns: usize,
    /// A point per unknown (edge midpoints, face centroids), for the ordering.
    pub points: Vec<Point3>,
}

impl Dofs {
    /// `fixed_faces`: the boundary faces (indices into the mesh's) whose tangential E is zero
    /// (perfectly conducting walls).
    fn new(order: Order, mesh: &Mesh, topo: &Topology, fixed_faces: &[usize]) -> Result<Dofs> {
        let (pe, pf) = (order.per_edge(), order.per_face());
        let (ne, nf) = (topo.edges.len(), topo.faces.len());
        let total = ne * pe + nf * pf;
        let mut fixed = vec![false; total];
        for b in fixed_faces.iter().filter_map(|&i| mesh.boundary.get(i)) {
            let tet = b.tet as usize;
            let Some((local, face)) = topo.find_face(tet, b.nodes) else {
                return Err(invalid("mesh", "a boundary face is not a face of its tetrahedron"));
            };
            for j in 0..pf {
                fixed[ne * pe + face as usize * pf + j] = true;
            }
            let corners = emcraft_fem::nedelec::LOCAL_FACES[local];
            for (e, [a, c]) in emcraft_fem::nedelec::LOCAL_EDGES.iter().enumerate() {
                if corners.contains(a) && corners.contains(c) {
                    let edge = topo.tet_edges[tet][e] as usize;
                    for j in 0..pe {
                        fixed[edge * pe + j] = true;
                    }
                }
            }
        }
        let free = fixed.iter().filter(|f| !**f).count();
        if free > MAX_UNKNOWNS {
            return Err(SolveError::TooLarge { unknowns: free, limit: MAX_UNKNOWNS });
        }
        let mut unknown = vec![NONE; total];
        let mut points = Vec::with_capacity(free);
        let node = |n: u32| mesh.nodes[n as usize];
        for (g, f) in fixed.iter().enumerate() {
            if *f {
                continue;
            }
            unknown[g] = points.len() as u32;
            let centre =
                if g < ne * pe { Point3::centroid(topo.edges[g / pe].map(node)) } else { Point3::centroid(topo.faces[(g - ne * pe) / pf].map(node)) };
            points.push(centre.unwrap_or(Point3::ORIGIN));
        }
        Ok(Dofs { order, edges: ne, faces: nf, unknown, unknowns: free, points })
    }

    /// The unknown of global degree of freedom `slot`, `None` when it is eliminated.
    pub(crate) fn unknown_of(&self, slot: usize) -> Option<usize> {
        self.unknown.get(slot).copied().filter(|u| *u != NONE).map(|u| u as usize)
    }

    /// The global degrees of freedom (slots, eliminated or not) of a tetrahedron's local basis
    /// functions, in the order of [`Dofs::of_tet`].
    pub(crate) fn slots_of_tet(&self, topo: &Topology, tet: usize) -> impl Iterator<Item = usize> + use<> {
        let (pe, pf, edges) = (self.order.per_edge(), self.order.per_face(), self.edges);
        let (te, tf) = (topo.tet_edges[tet], topo.tet_faces[tet]);
        (0..pe)
            .flat_map(move |j| te.into_iter().map(move |e| e as usize * pe + j))
            .chain(tf.into_iter().flat_map(move |f| (0..pf).map(move |j| edges * pe + f as usize * pf + j)))
    }

    /// The unknowns of a tetrahedron's local basis functions ([`NONE`] for eliminated ones).
    fn of_tet<'a>(&'a self, topo: &Topology, tet: usize) -> impl Iterator<Item = u32> + use<'a> {
        self.slots_of_tet(topo, tet).map(|s| self.unknown.get(s).copied().unwrap_or(NONE))
    }
}

/// A port prepared for solving.
#[derive(Clone, Debug)]
pub struct PortData {
    pub name: String,
    pub end: End,
    pub modes: PortModes,
    /// The impedance S is referred to at this port (Ω), if any.
    pub reference: Option<f64>,
    /// How far (m) inside the model the reference plane lies.
    pub deembed: f64,
}

/// A port's modes: closed forms, or the numerical solver's on its cross-section.
#[derive(Clone, Debug)]
pub enum PortModes {
    /// A whole rectangle or circle of one material (εr, μr): each mode with its vector
    /// v_m,i = ∫ Nᵢ · e_m dS (unknown, value).
    Analytic { eps_r: f64, mu_r: f64, modes: Vec<(Mode, Vec<(usize, f64)>)> },
    /// Any other cross-section; its modes depend on the frequency and are solved with it.
    Numeric(Box<NumericPort>),
}

/// A port whose modes come from [`crate::portmode`].
#[derive(Clone, Debug)]
pub struct NumericPort {
    pub section: crate::portmode::CrossSection,
    /// The mesh's boundary face of each of the section's triangles.
    faces: Vec<usize>,
    /// Modes in the port condition.
    count: usize,
    /// The unknowns with a tangential trace on the port, ascending.
    support: Vec<usize>,
}

/// One mode's part of a port condition at one frequency: the matrix gains factor·v vᵀ, a unit
/// incident wave drives rhs·v, and the mode's coefficient in a field x is vᵀx/norm.
#[derive(Clone, Debug)]
pub struct PortTerm<'a> {
    pub name: String,
    pub v: std::borrow::Cow<'a, [(usize, f64)]>,
    pub factor: C,
    pub rhs: C,
    pub norm: f64,
    /// 2ωμ₀ times the power a unit wave of the mode carries (0 for an evanescent one).
    pub proxy: f64,
    pub propagating: bool,
    /// The line's characteristic impedance (Ω) for a port's first mode, where defined.
    pub impedance: Option<f64>,
    /// The mode's propagation constant γ = α + jβ (1/m): it goes as e^{−γz} along the line.
    pub gamma: C,
}

/// A waveguide meshed and assembled up to the frequency: the curl–curl and mass matrices and
/// the port vectors.
#[derive(Clone, Debug)]
pub struct Driven {
    pub order: Order,
    pub mesh: Mesh,
    pub topo: Topology,
    pub dofs: Dofs,
    /// Complex relative permittivity εr(1 − j tan δ) and relative permeability per tetrahedron.
    eps: Vec<C>,
    mu: Vec<f64>,
    pub(crate) pattern: Csr,
    pub(crate) stiffness: Vec<f64>,
    pub(crate) mass: Vec<C>,
    /// Walls of finite conductivity: σ (S/m) and their tangential mass matrix.
    pub(crate) wall: Option<(f64, Vec<f64>)>,
    /// The boundary faces that are walls (indices into the mesh's).
    walls: Vec<usize>,
    pub ports: Vec<PortData>,
    /// The perfectly matched layer: its thickness and, for each tetrahedron in it, the depth
    /// into it along x, y and z as fractions of the thickness (0 where it is not in the layer
    /// along that axis). Their matrices depend on the frequency and are assembled with it.
    /// (thickness, design reflection, and per tetrahedron its depths)
    pub(crate) pml: Option<(f64, f64, Vec<(usize, Vec3)>)>,
    /// Lumped ports: each with its vector cᵢ = ∫ Nᵢ · ŝ dl along its gap.
    pub lumped: Vec<LumpedData>,
    /// The symbolic factorization of the system's pattern, made at the first frequency and
    /// shared by the others (the pattern does not change with the frequency).
    analysis: std::sync::OnceLock<crate::ldlt::Analysis>,
}

/// S read from the solutions of each port's excitation (`fields[q]`): the first mode's
/// coefficient at each port, less the incident wave, as power waves of the modes (scaled by
/// the ports' power proxies).
pub(crate) fn read_s(terms: &[Vec<PortTerm<'_>>], fields: &[Vec<C>]) -> SMatrix {
    let proxy = |r: usize| terms.get(r).and_then(|l| l.first()).map_or(1.0, |t| t.proxy);
    let mut s = SMatrix::zeros(terms.len());
    for (q, x) in fields.iter().enumerate() {
        for (p, list) in terms.iter().enumerate() {
            let Some(t) = list.first() else { continue };
            let c: C = t.v.iter().map(|&(i, vi)| x[i] * vi).sum::<C>() / t.norm;
            let direct = if p == q { c - 1.0 } else { c };
            s[(p, q)] = direct * (proxy(p) / proxy(q)).sqrt();
        }
    }
    s
}

/// Solves a small dense complex system by Gaussian elimination with partial pivoting.
pub(crate) fn complex_solve(mut a: Vec<Vec<C>>, mut b: Vec<C>) -> Option<Vec<C>> {
    let n = b.len();
    for k in 0..n {
        let piv = (k..n).max_by(|&i, &j| a[i][k].norm().total_cmp(&a[j][k].norm()))?;
        if a[piv][k].norm() == 0.0 {
            return None;
        }
        a.swap(k, piv);
        b.swap(k, piv);
        for i in k + 1..n {
            let f = a[i][k] / a[k][k];
            for j in k..n {
                let x = a[k][j];
                a[i][j] -= f * x;
            }
            let x = b[k];
            b[i] -= f * x;
        }
    }
    let mut x = vec![C::new(0.0, 0.0); n];
    for i in (0..n).rev() {
        let s: C = (i + 1..n).map(|j| a[i][j] * x[j]).sum();
        x[i] = (b[i] - s) / a[i][i];
    }
    Some(x)
}

/// A frequency's linear system: the matrix with its border of port modes (the last `border`
/// unknowns), the right-hand side of each port's excitation, and the ports' terms.
pub(crate) struct System<'a> {
    pub a: Csr,
    pub rhs: Vec<Vec<C>>,
    pub terms: Vec<Vec<PortTerm<'a>>>,
    pub border: usize,
}

/// `a` bordered by one unknown per (v, f): its column holds s v and its diagonal −s²/f, with
/// s = √(|f| ‖a‖) so the border's pivots are of the matrix's size (unscaled, −1/f can be far
/// below the factorization's static-pivot threshold). The x part of the solution is the same
/// whatever s.
fn bordered(a: &Csr, borders: &[(&[(usize, f64)], C)]) -> Result<Csr> {
    let (n, m) = (a.n, borders.len());
    let norm = a.max_abs();
    let scale: Vec<f64> = borders.iter().map(|(_, f)| (f.norm() * norm).sqrt()).collect();
    let mut extra: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for (k, (v, _)) in borders.iter().enumerate() {
        for &(i, vi) in v.iter() {
            extra.get_mut(i).ok_or_else(|| invalid("port", "a mode's vector refers to a missing unknown"))?.push((n + k, scale[k] * vi));
        }
    }
    let mut out = Csr { n: n + m, ptr: Vec::with_capacity(n + m + 1), col: Vec::new(), val: Vec::new() };
    out.ptr.push(0);
    for (i, row) in extra.iter().enumerate() {
        let (lo, hi) = (a.ptr[i], a.ptr[i + 1]);
        out.col.extend_from_slice(&a.col[lo..hi]);
        out.val.extend_from_slice(&a.val[lo..hi]);
        out.col.extend(row.iter().map(|(j, _)| *j));
        out.val.extend(row.iter().map(|(_, v)| C::new(*v, 0.0)));
        out.ptr.push(out.col.len());
    }
    for (k, (v, f)) in borders.iter().enumerate() {
        if !(f.norm() > 0.0 && scale[k].is_finite()) {
            return Err(invalid("port", "a mode with no coupling"));
        }
        out.col.extend(v.iter().map(|(i, _)| *i));
        out.val.extend(v.iter().map(|(_, x)| C::new(scale[k] * x, 0.0)));
        out.col.push(n + k);
        out.val.push(-(scale[k] * scale[k]) / f);
        out.ptr.push(out.col.len());
    }
    Ok(out)
}

/// A lumped port prepared for solving.
#[derive(Clone, Debug)]
pub struct LumpedData {
    pub name: String,
    pub impedance: f64,
    pub voltage: C,
    /// (unknown, ∫ Nᵢ · ŝ dl) along the gap from `from` to `to`.
    pub c: Vec<(usize, f64)>,
}

/// The reflection a perfectly matched layer is designed for at normal incidence, before
/// discretization: its stretch is s = 1 − j(σ₀/k₀)(ξ/d)², σ₀ = 3 ln(1/R)/(2d).
pub const PML_REFLECTION: f64 = 1e-6;

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

impl Driven {
    /// Prepares the model's waveguide (meshed) for solving with elements of `order`.
    pub fn new(model: &Model, order: Order) -> Result<Driven> {
        Driven::build(model, order, true, false)
    }

    /// The model's waveguide closed at both ends by conducting walls (a cavity), its ports left
    /// out: the matrices of an eigenmode analysis ([`crate::eigen`]).
    pub fn closed(model: &Model, order: Order) -> Result<Driven> {
        refuse_open(model)?;
        Driven::build(model, order, false, false)
    }

    /// The cavity with perfectly conducting walls whatever their conductivity: the lossless
    /// problem whose modes perturbation theory gives Q factors from.
    pub fn cavity(model: &Model, order: Order) -> Result<Driven> {
        refuse_open(model)?;
        Driven::build(model, order, false, true)
    }

    /// The model's open problem: its box lined with the perfectly matched layer
    /// ([`emcraft_model::Open`]), driven by sources, lumped ports or its waveguide ports on faces
    /// inside it ([`crate::radiation`]).
    pub fn open(model: &Model, order: Order) -> Result<Driven> {
        if model.open.is_none() {
            return Err(SolveError::NotReady("no open boundary: line the box with model.open".into()));
        }
        Driven::build(model, order, !model.ports.is_empty(), false)
    }

    fn build(model: &Model, order: Order, with_ports: bool, lossless_walls: bool) -> Result<Driven> {
        let issues: Vec<String> = model.check().into_iter().filter(|i| with_ports || !i.starts_with("no ports")).collect();
        if !issues.is_empty() {
            return Err(SolveError::NotReady(issues.join("; ")));
        }
        let mesh = model.mesh.clone().ok_or_else(|| SolveError::NotReady("no mesh".into()))?;
        let topo = Topology::new(&mesh);
        let sigma = if lossless_walls { None } else { model.wall_conductivity };
        if let Some(c) = sigma
            && !(c.is_finite() && c > 0.0)
        {
            return Err(invalid("wall conductivity", format!("must be a finite number > 0, got {c}")));
        }
        // Each port's boundary faces: an end's tag, or the faces on a port's rectangle. Walls: the
        // rest.
        let port_faces: Vec<Vec<usize>> =
            if with_ports { model.ports.iter().map(|p| faces_of_port(&mesh, p)).collect::<Result<_>>()? } else { Vec::new() };
        let on_port: std::collections::HashSet<usize> = port_faces.iter().flatten().copied().collect();
        let walls: Vec<usize> = (0..mesh.boundary.len()).filter(|i| !on_port.contains(i)).collect();
        let fixed: &[usize] = if sigma.is_none() { &walls } else { &[] };
        let dofs = Dofs::new(order, &mesh, &topo, fixed)?;
        let material = |t: usize| mesh.tet_material.get(t).and_then(|&m| model.materials.get(m as usize));
        let mut eps = Vec::with_capacity(mesh.tets.len());
        let mut mu = Vec::with_capacity(mesh.tets.len());
        for t in 0..mesh.tets.len() {
            let m = material(t).ok_or_else(|| invalid("mesh", format!("tetrahedron {t} has no material")))?;
            eps.push(C::new(m.eps_r, -m.eps_r * m.loss_tangent));
            mu.push(m.mu_r);
        }

        // The layer (open problems only): tetrahedra by their centroids' depth into it.
        // The layer lines open problems (sources, or ports with an open boundary).
        let pml = match (&model.open, &model.solid, true) {
            (Some(o), Some(emcraft_model::Solid { shape: emcraft_model::Shape::Box { size }, .. }), true) => {
                let d = o.thickness;
                let mut list = Vec::new();
                for t in 0..mesh.tets.len() {
                    let centroid = mesh.centroid(t).ok_or_else(|| invalid("mesh", format!("no tetrahedron {t}")))?;
                    let depth = centroid.to_vec().zip_map(*size, |c, l| ((d - c).max(c - (l - d)).max(0.0) / d).min(1.0));
                    if depth.iter().any(|x| x > 0.0) {
                        list.push((t, depth));
                    }
                }
                Some((d, o.reflection.unwrap_or(PML_REFLECTION), list))
            }
            _ => None,
        };
        let in_pml: std::collections::HashSet<usize> = pml.iter().flat_map(|(_, _, l)| l.iter().map(|(t, _)| *t)).collect();

        // Port vectors: the closed forms' once, the numerical modes' support (their vectors come
        // with each frequency).
        let mut ports = Vec::new();
        for (k, p) in model.ports.iter().enumerate().filter(|_| with_ports) {
            let shape = if p.mode_solver == emcraft_model::ModeSolver::Numeric { None } else { model.port_shape_of(p).ok() };
            let modes = match shape {
                Some(shape) => analytic_port(model, p, shape, &mesh, &topo, &dofs, order, &port_faces[k])?,
                None => PortModes::Numeric(Box::new(numeric_port(model, p, &mesh, &topo, &dofs, order, &port_faces[k])?)),
            };
            ports.push(PortData { name: p.name.clone(), end: p.end, modes, reference: p.reference, deembed: p.deembed });
        }

        // Lumped ports: their gaps' line integrals.
        let mut lumped = Vec::new();
        for lp in &model.lumped {
            let c = band_vector(&mesh, &topo, &dofs, order, lp.from, lp.to, lp.radius)?;
            if c.is_empty() {
                return Err(invalid(format!("lumped port `{}`", lp.name), "its gap lies outside the mesh (inside a conductor?)"));
            }
            lumped.push(LumpedData { name: lp.name.clone(), impedance: lp.impedance, voltage: C::new(lp.voltage[0], lp.voltage[1]), c });
        }

        // Pattern: element couplings and the dense port blocks.
        let mut groups: Vec<Vec<usize>> =
            (0..mesh.tets.len()).map(|t| dofs.of_tet(&topo, t).filter(|&u| u != NONE).map(|u| u as usize).collect()).collect();
        // (The ports' rank-one terms border the matrix instead of filling it: see `system`.)
        for l in &lumped {
            groups.push(l.c.iter().map(|(u, _)| *u).collect());
        }
        let pattern = Csr::from_groups(dofs.unknowns, groups.iter().map(|g| g.as_slice()));
        let mut kk = Csr { val: vec![C::new(0.0, 0.0); pattern.col.len()], ..pattern.clone() };
        let mut mm = kk.clone();
        let mut cache = ElementCache::default();
        for t in (0..mesh.tets.len()).filter(|t| !in_pml.contains(t)) {
            let (s, m) = cache.matrices(&mesh, &topo, t, order)?;
            let unknowns: Vec<u32> = dofs.of_tet(&topo, t).collect();
            let n = unknowns.len();
            for (i, ui) in unknowns.iter().enumerate() {
                if *ui == NONE {
                    continue;
                }
                for (j, uj) in unknowns.iter().enumerate() {
                    if *uj == NONE {
                        continue;
                    }
                    kk.add(*ui as usize, *uj as usize, C::new(s[i * n + j] / mu[t], 0.0));
                    mm.add(*ui as usize, *uj as usize, eps[t] * m[i * n + j]);
                }
            }
        }
        let stiffness = kk.val.iter().map(|v| v.re).collect();
        let mass = mm.val;
        // Walls of finite conductivity: ∫ N_i,t · N_j,t dS over them.
        let mut ww = Csr { val: vec![C::new(0.0, 0.0); pattern.col.len()], ..pattern.clone() };
        if sigma.is_some() {
            let n = if order == Order::One { 3 } else { 4 };
            for b in walls.iter().filter_map(|&i| mesh.boundary.get(i)) {
                let tet = b.tet as usize;
                let el = element(&mesh, &topo, tet, order)?;
                let unknowns: Vec<u32> = dofs.of_tet(&topo, tet).collect();
                for (l, wa, normal) in el.face_rule(local_face(&topo, tet, &b.nodes)?, n) {
                    let t: Vec<Vec3> = el.values(l).into_iter().map(|v| v.tangential(normal)).collect();
                    for (i, ui) in unknowns.iter().enumerate() {
                        if *ui == NONE {
                            continue;
                        }
                        for (j, uj) in unknowns.iter().enumerate() {
                            if *uj != NONE {
                                ww.add(*ui as usize, *uj as usize, C::new(wa * t[i].dot(t[j]), 0.0));
                            }
                        }
                    }
                }
            }
        }
        let wall = sigma.map(|c| (c, ww.val.iter().map(|v| v.re).collect()));
        Ok(Driven {
            order,
            mesh,
            topo,
            dofs,
            eps,
            mu,
            pattern,
            stiffness,
            mass,
            wall,
            walls,
            ports,
            pml,
            lumped,
            analysis: std::sync::OnceLock::new(),
        })
    }

    /// Adds the layer's tetrahedra at wavenumber k₀: with stretches s_d = 1 − j(σ₀/k₀)ξ_d², the
    /// layer is the anisotropic medium εr Λ, μr Λ, Λ = diag(s_y s_z/s_x, s_z s_x/s_y, s_x s_y/s_z)
    /// (Z. S. Sacks, D. M. Kingsland, R. Lee, J.-F. Lee, IEEE Trans. AP 43(12), 1995; Jin 2014,
    /// §9.6), so its element matrices are Σ_d (1/(μr Λ_d)) K_d − k₀² Σ_d εr Λ_d M_d by component.
    pub(crate) fn add_pml(&self, a: &mut Csr, k0: f64) -> Result<()> {
        let Some((d, reflection, list)) = &self.pml else { return Ok(()) };
        let sigma0 = 3.0 * (1.0 / reflection).ln() / (2.0 * d);
        for &(t, depth) in list {
            let s = [depth.x, depth.y, depth.z].map(|x| C::new(1.0, -sigma0 / k0 * x * x));
            let lambda = [s[1] * s[2] / s[0], s[2] * s[0] / s[1], s[0] * s[1] / s[2]];
            let el = element(&self.mesh, &self.topo, t, self.order)?;
            let (kc, mc) = (el.stiffness_components(), el.mass_components());
            let unknowns: Vec<u32> = self.dofs.of_tet(&self.topo, t).collect();
            let n = unknowns.len();
            let (eps, mu) = (self.eps.get(t).copied().unwrap_or(C::new(1.0, 0.0)), self.mu.get(t).copied().unwrap_or(1.0));
            for (i, ui) in unknowns.iter().enumerate() {
                if *ui == NONE {
                    continue;
                }
                for (j, uj) in unknowns.iter().enumerate() {
                    if *uj == NONE {
                        continue;
                    }
                    let mut v = C::new(0.0, 0.0);
                    for k in 0..3 {
                        v += kc[k][i * n + j] / (mu * lambda[k]) - k0 * k0 * eps * lambda[k] * mc[k][i * n + j];
                    }
                    a.add(*ui as usize, *uj as usize, v);
                }
            }
        }
        Ok(())
    }

    /// The system at a frequency: the matrix bordered by the ports' modes, each port's
    /// excitation by its first mode, and the modes' terms.
    pub(crate) fn system(&self, frequency: f64) -> Result<System<'_>> {
        let k0 = 2.0 * PI * frequency / C0;
        let mut a = self.pattern.clone();
        for (x, (s, m)) in a.val.iter_mut().zip(self.stiffness.iter().zip(&self.mass)) {
            *x = C::new(*s, 0.0) - m * (k0 * k0);
        }
        self.add_pml(&mut a, k0)?;
        // Lumped ports: the current I = (V_s + cᵀx)/R through each gap adds (jωμ₀/R) c cᵀ.
        let omega = 2.0 * PI * frequency;
        for l in &self.lumped {
            let f = C::new(0.0, omega * MU0 / l.impedance);
            for &(i, ci) in &l.c {
                for &(j, cj) in &l.c {
                    a.add(i, j, f * (ci * cj));
                }
            }
        }
        // The Leontovich condition E_t = Z_s n̂ × H on the walls (n̂ into the guide), Z_s =
        // (1 + j)R_s, R_s = √(ωμ₀/2σ), adds (jωμ₀/Z_s) ∫ N_i,t · N_j,t dS (T. B. A. Senior, J. L.
        // Volakis, Approximate Boundary Conditions in Electromagnetics, IEE 1995, §2.2; Jin 2014,
        // §9.2).
        if let Some((sigma, w)) = &self.wall {
            let c = wall_factor(frequency, *sigma);
            for (x, wij) in a.val.iter_mut().zip(w) {
                *x += c * *wij;
            }
        }
        let terms = self.checked_terms(frequency)?;
        // Each mode's factor·v vᵀ would couple every unknown on the port with every other; a
        // border unknown y_m = factor·vᵀx per mode keeps the matrix sparse:
        // [A V; Vᵀ −F⁻¹] [x; y] = [b; 0] eliminates to (A + V F Vᵀ) x = b.
        let borders: Vec<(&[(usize, f64)], C)> = terms.iter().flatten().map(|t| (&*t.v, t.factor)).collect();
        let border = borders.len();
        let a = bordered(&a, &borders)?;
        let n = self.dofs.unknowns;
        let rhs = terms
            .iter()
            .map(|list| {
                let mut b = vec![C::new(0.0, 0.0); n + border];
                if let Some(t) = list.first() {
                    for &(i, vi) in t.v.iter() {
                        b[i] = t.rhs * vi;
                    }
                }
                b
            })
            .collect();
        Ok(System { a, rhs, terms, border })
    }

    /// Every port's terms at `frequency`, refusing it where a port's first mode does not
    /// propagate.
    pub(crate) fn checked_terms(&self, frequency: f64) -> Result<Vec<Vec<PortTerm<'_>>>> {
        let k0 = 2.0 * PI * frequency / C0;
        let mut terms = Vec::with_capacity(self.ports.len());
        for p in 0..self.ports.len() {
            // (A closed-form mode is refused before its κ, singular at the cut-off.)
            let below = self.below_cutoff(p, k0);
            let list = if below { Vec::new() } else { self.port_terms(p, frequency)? };
            if below || !list.first().is_some_and(|t| t.propagating) {
                return Err(invalid(
                    "frequency",
                    format!(
                        "{frequency} Hz is below the cut-off of the first mode at port `{}`: no wave can carry power into the port",
                        self.ports[p].name
                    ),
                ));
            }
            terms.push(list);
        }
        Ok(terms)
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
        if !(frequency.is_finite() && frequency > 0.0) {
            return Err(invalid("frequency", format!("must be a finite number > 0, got {frequency}")));
        }
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
        let impedances: Vec<Option<f64>> = terms.iter().map(|l| l.first().and_then(|t| t.impedance)).collect();
        let s = self.deembed(self.refer(read_s(&terms, &fields), &impedances)?, &terms);
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
        element(&self.mesh, &self.topo, t, self.order)
    }

    pub(crate) fn dofs_of(&self, t: usize) -> impl Iterator<Item = u32> + '_ {
        self.dofs.of_tet(&self.topo, t)
    }

    pub(crate) fn eps_of(&self, t: usize) -> C {
        self.eps.get(t).copied().unwrap_or(C::new(1.0, 0.0))
    }

    pub(crate) fn mu_of(&self, t: usize) -> f64 {
        self.mu.get(t).copied().unwrap_or(1.0)
    }

    /// The field (E in V/m or H in A/m) at a point for the coefficients `x` at `frequency`.
    pub fn field(&self, x: &[C], frequency: f64, which: Field, p: Point3) -> Option<CVec3> {
        let (t, _) = emcraft_mesh::locate(&self.mesh, p)?;
        let el = element(&self.mesh, &self.topo, t, self.order).ok()?;
        Some(self.field_in(&el, t, x, frequency, which, el.barycentric(p)))
    }

    /// E and H at a point, `None` outside the mesh.
    pub fn fields(&self, x: &[C], frequency: f64, p: Point3) -> Option<(CVec3, CVec3)> {
        let (t, _) = emcraft_mesh::locate(&self.mesh, p)?;
        let el = element(&self.mesh, &self.topo, t, self.order).ok()?;
        let l = el.barycentric(p);
        Some((self.field_in(&el, t, x, frequency, Field::E, l), self.field_in(&el, t, x, frequency, Field::H, l)))
    }

    pub(crate) fn field_in(&self, el: &Element, t: usize, x: &[C], frequency: f64, which: Field, l: [f64; 4]) -> CVec3 {
        let values = match which {
            Field::E => el.values(l),
            Field::H => el.curl_values(l),
        };
        let field: CVec3 = self
            .dofs
            .of_tet(&self.topo, t)
            .zip(&values)
            .filter(|(u, _)| *u != NONE)
            .map(|(u, v)| *v * x.get(u as usize).copied().unwrap_or_default())
            .sum();
        match which {
            Field::E => field,
            // H = −∇ × E / (jωμ).
            Field::H => -field / C::new(0.0, 2.0 * PI * frequency * MU0 * self.mu_of(t)),
        }
    }

    /// ‖field − exact‖ and ‖exact‖ in L²(Ω), by quadrature on every tetrahedron.
    pub fn l2_error(&self, x: &[C], frequency: f64, which: Field, exact: impl Fn(Point3) -> CVec3) -> Result<(f64, f64)> {
        let n = self.rule_points();
        let (mut err, mut norm) = (0.0, 0.0);
        for t in 0..self.mesh.tets.len() {
            let el = self.element_of(t)?;
            for (l, w) in el.rule(n) {
                let (u, e) = (self.field_in(&el, t, x, frequency, which, l), exact(el.point(l)));
                err += w * (u - e).norm_sqr();
                norm += w * e.norm_sqr();
            }
        }
        Ok((err.sqrt(), norm.sqrt()))
    }

    /// Gauss points per direction for integrals of fields over elements of this order.
    fn rule_points(&self) -> usize {
        if self.order == Order::One { 3 } else { 4 }
    }

    /// ∫ f(t, E) dV over every tetrahedron `keep` admits, E the field at each point of a rule.
    fn integrate_e(&self, x: &[C], frequency: f64, keep: impl Fn(usize) -> bool, f: impl Fn(usize, CVec3) -> f64) -> Result<f64> {
        let n = self.rule_points();
        (0..self.mesh.tets.len()).filter(|t| keep(*t)).try_fold(0.0, |total, t| {
            let el = self.element_of(t)?;
            Ok(total + el.rule(n).into_iter().map(|(l, w)| w * f(t, self.field_in(&el, t, x, frequency, Field::E, l))).sum::<f64>())
        })
    }

    /// ∫ f(field) dS over the walls, the field `which` and the wall's unit normal at each point.
    fn integrate_walls(&self, x: &[C], frequency: f64, which: Field, f: impl Fn(CVec3, Vec3) -> f64) -> Result<f64> {
        let n = if self.order == Order::One { 3 } else { 4 };
        self.walls.iter().filter_map(|&i| self.mesh.boundary.get(i)).try_fold(0.0, |total, b| {
            let tet = b.tet as usize;
            let el = self.element_of(tet)?;
            let rule = el.face_rule(local_face(&self.topo, tet, &b.nodes)?, n);
            Ok(total + rule.into_iter().map(|(l, wa, normal)| wa * f(self.field_in(&el, tet, x, frequency, which, l), normal)).sum::<f64>())
        })
    }

    /// The power (W) the dielectrics absorb for the coefficients `x`: ½ ω ε₀ ∫ εr tan δ |E|² dV
    /// (fields in peak values).
    pub fn dielectric_loss(&self, x: &[C], frequency: f64) -> Result<f64> {
        let omega = 2.0 * PI * frequency;
        let eps0 = 1.0 / (MU0 * C0 * C0);
        let loss = |t: usize| -self.eps_of(t).im;
        let total = self.integrate_e(x, frequency, |t| loss(t) != 0.0, |t, e| loss(t) * e.norm_sqr())?;
        Ok(0.5 * omega * eps0 * total)
    }

    /// The curl–curl matrix K, the mass matrix M (complex permittivity) and their sparsity
    /// pattern, for eigenmode analysis.
    pub(crate) fn matrices(&self) -> (&Csr, &[f64], &[C]) {
        (&self.pattern, &self.stiffness, &self.mass)
    }

    /// The power (W) walls of conductivity `sigma` would absorb for a field computed with
    /// perfectly conducting walls: ½ R_s ∫ |H_t|² dS (the perturbation estimate).
    pub fn wall_loss_perturbation(&self, x: &[C], frequency: f64, sigma: f64) -> Result<f64> {
        let rs = surface_resistance(frequency, sigma);
        let total = self.integrate_walls(x, frequency, Field::H, |h, n| tangential_c(h, n).norm_sqr())?;
        Ok(0.5 * rs * total)
    }

    /// The time-averaged stored energy (J) at resonance, twice the electric: ½ ε₀ ∫ εr |E|² dV.
    pub fn stored_energy(&self, x: &[C], frequency: f64) -> Result<f64> {
        let eps0 = 1.0 / (MU0 * C0 * C0);
        let total = self.integrate_e(x, frequency, |_| true, |t, e| self.eps_of(t).re * e.norm_sqr())?;
        Ok(0.5 * eps0 * total)
    }

    /// The power (W) the walls of finite conductivity absorb: ∫ |E_t|²/(4R_s) dS (= ½ R_s ∫ |H_t|²
    /// dS with the Leontovich condition).
    pub fn wall_loss(&self, x: &[C], frequency: f64) -> Result<f64> {
        let Some((sigma, _)) = &self.wall else { return Ok(0.0) };
        let rs = surface_resistance(frequency, *sigma);
        let total = self.integrate_walls(x, frequency, Field::E, |e, n| tangential_c(e, n).norm_sqr())?;
        Ok(total / (4.0 * rs))
    }

    /// The power (W) a unit TE₁₀ wave carries into port `p`: ½ ‖e‖² β/(ωμ).
    pub fn incident_power(&self, p: usize, frequency: f64) -> Option<f64> {
        let port = self.ports.get(p)?;
        let omega = 2.0 * PI * frequency;
        match &port.modes {
            PortModes::Analytic { eps_r, mu_r, modes } => {
                let (mode, _) = modes.first()?;
                let k = omega / C0 * (eps_r * mu_r).sqrt();
                Some(0.5 * mode.norm() * mode.gamma(k).1 / (omega * MU0 * mu_r))
            }
            PortModes::Numeric(_) => Some(0.5 * self.port_terms(p, frequency).ok()?.first()?.proxy / (omega * MU0)),
        }
    }

    /// Whether a closed-form port's first mode is at or below its cut-off at k₀ (numerical ports
    /// tell by their terms).
    fn below_cutoff(&self, p: usize, k0: f64) -> bool {
        match self.ports.get(p).map(|p| &p.modes) {
            Some(PortModes::Analytic { eps_r, mu_r, modes }) => modes.first().is_none_or(|(m, _)| k0 * (eps_r * mu_r).sqrt() <= m.cutoff()),
            _ => false,
        }
    }

    /// S of the modes' power waves referred to the ports' reference impedances R (those without
    /// one keep their mode's): with each port's line impedance Z, V = √Z (a + b) and
    /// I = (a − b)/√Z, the new waves a′ = P a + Q b, b′ = Q a + P b, P = (Z + R)/(2√(ZR)),
    /// Q = (Z − R)/(2√(ZR)), so S′ = (Q + P S)(P + Q S)⁻¹ (K. Kurokawa, *Power waves and the
    /// scattering matrix*, IEEE Trans. MTT 13(2):194–202, 1965).
    /// S at the ports' reference planes moved `deembed` into the model along their lines: the
    /// waves there are e^{γd} larger, so S′_pq = S_pq e^{γ_p d_p + γ_q d_q} for each port's
    /// first mode (D. M. Pozar, *Microwave Engineering*, 4th ed., §4.3, shifting reference
    /// planes).
    pub(crate) fn deembed(&self, s: SMatrix, terms: &[Vec<PortTerm<'_>>]) -> SMatrix {
        if self.ports.iter().all(|p| p.deembed == 0.0) {
            return s;
        }
        let shift = |p: usize| match (self.ports.get(p), terms.get(p).and_then(|l| l.first())) {
            (Some(port), Some(t)) => (t.gamma * port.deembed).exp(),
            _ => C::new(1.0, 0.0),
        };
        SMatrix::from_fn(s.ports(), |p, q| s[(p, q)] * shift(p) * shift(q))
    }

    pub(crate) fn refer(&self, s: SMatrix, impedances: &[Option<f64>]) -> Result<SMatrix> {
        let n = s.ports();
        let mut p = vec![1.0; n];
        let mut q = vec![0.0; n];
        let mut any = false;
        for (i, port) in self.ports.iter().enumerate() {
            let Some(r) = port.reference else { continue };
            let z = impedances.get(i).copied().flatten().ok_or_else(|| {
                invalid(
                    format!("port `{}`", port.name),
                    "has a reference impedance, but its mode has no line impedance (one conductor inside the ground is needed)",
                )
            })?;
            let root = 2.0 * (z * r).sqrt();
            p[i] = (z + r) / root;
            q[i] = (z - r) / root;
            any = true;
        }
        if !any {
            return Ok(s);
        }
        s.renormalized(&p, &q).ok_or_else(|| invalid("reference impedances", "a singular renormalisation"))
    }

    /// Port `p`'s modes as terms of its condition at `frequency` (the first is the one excited and
    /// read). Closed forms: n̂ × ∇ × E_t = κ E_t, factor κ/(μr‖e‖²); numerical modes: H_t =
    /// ŝ × g β/(ωμ), factor jβ/D, with v_i = ∫ μr⁻¹ Nᵢ · g dS and D = ∫ μr⁻¹ e · g dS
    /// ([`crate::portmode`]).
    pub fn port_terms(&self, p: usize, frequency: f64) -> Result<Vec<PortTerm<'_>>> {
        let port = self.ports.get(p).ok_or_else(|| invalid("port", format!("no port {p}")))?;
        let k0 = 2.0 * PI * frequency / C0;
        match &port.modes {
            PortModes::Analytic { eps_r, mu_r, modes } => {
                let k = k0 * (eps_r * mu_r).sqrt();
                modes
                    .iter()
                    .map(|(mode, v)| {
                        let (kr, ki) = mode.kappa(k).ok_or_else(|| {
                            invalid("frequency", format!("{} Hz is at the cut-off of mode {} at port `{}`", frequency, mode.name(), port.name))
                        })?;
                        let kappa = C::new(kr, ki) / mu_r;
                        let gamma = mode.gamma(k);
                        Ok(PortTerm {
                            name: mode.name(),
                            v: std::borrow::Cow::Borrowed(v.as_slice()),
                            factor: kappa / mode.norm(),
                            rhs: 2.0 * kappa,
                            norm: mode.norm(),
                            proxy: mode.norm() * gamma.1 / mu_r,
                            propagating: gamma.1 > 0.0,
                            impedance: None,
                            gamma: C::new(gamma.0, gamma.1),
                        })
                    })
                    .collect()
            }
            PortModes::Numeric(n) => self.numeric_terms(n, k0),
        }
    }

    fn numeric_terms(&self, n: &NumericPort, k0: f64) -> Result<Vec<PortTerm<'static>>> {
        let modes = n.section.modes(k0, n.count)?;
        let index: std::collections::HashMap<usize, usize> = n.support.iter().enumerate().map(|(k, &u)| (u, k)).collect();
        let mut v = vec![vec![0.0; n.support.len()]; modes.len()];
        let rule = if self.order == Order::One { 6 } else { 8 };
        for (t, &f) in n.faces.iter().enumerate() {
            let b = self.mesh.boundary.get(f).ok_or_else(|| invalid("port", "a face is missing from the mesh"))?;
            let tet = b.tet as usize;
            let el = self.element_of(tet)?;
            let unknowns: Vec<u32> = self.dofs_of(tet).collect();
            let inv_mu = 1.0 / n.section.mu_of(t);
            for (l, wa, _) in el.face_rule(local_face(&self.topo, tet, &b.nodes)?, rule) {
                let x = el.point(l);
                let values = el.values(l);
                let gs = modes.iter().map(|m| n.section.fields(m, t, x).map(|(_, g)| g)).collect::<Result<Vec<_>>>()?;
                for (u, val) in unknowns.iter().zip(&values) {
                    let Some(&k) = index.get(&(*u as usize)) else { continue };
                    for (vm, g) in v.iter_mut().zip(&gs) {
                        vm[k] += wa * inv_mu * val.dot(*g);
                    }
                }
            }
        }
        let impedance = match modes.first() {
            Some(m) => n.section.impedance(m, k0)?,
            None => None,
        };
        Ok(modes
            .iter()
            .zip(v)
            .enumerate()
            .map(|(i, (m, vm))| {
                let beta = m.beta();
                let jb = C::new(0.0, 1.0) * beta;
                PortTerm {
                    impedance: if i == 0 { impedance } else { None },
                    name: format!("mode {}", i + 1),
                    v: std::borrow::Cow::Owned(n.support.iter().copied().zip(vm).collect()),
                    factor: jb / m.d,
                    rhs: 2.0 * jb,
                    norm: m.d,
                    proxy: if m.beta2 > 0.0 { beta.re * m.d } else { 0.0 },
                    propagating: m.beta2 > 0.0,
                    gamma: jb,
                }
            })
            .collect())
    }
}

/// The part of a complex field across the unit normal n: v − (v·n) n.
fn tangential_c(v: CVec3, n: Vec3) -> CVec3 {
    v - n * v.dot_real(n)
}

/// R_s = √(ωμ₀/2σ) (Ω).
pub fn surface_resistance(frequency: f64, sigma: f64) -> f64 {
    (PI * frequency * MU0 / sigma).sqrt()
}

/// jωμ₀/Z_s = ωμ₀(1 + j)/(2R_s) (1/m).
pub(crate) fn wall_factor(frequency: f64, sigma: f64) -> C {
    let omega = 2.0 * PI * frequency;
    C::new(1.0, 1.0) * (omega * MU0 / (2.0 * surface_resistance(frequency, sigma)))
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

/// A port's boundary faces: those tagged for its end, or those on its face's rectangle (all
/// three corners on the plane and inside the rectangle, to a millionth of its size).
fn faces_of_port(mesh: &Mesh, p: &emcraft_model::Port) -> Result<Vec<usize>> {
    let out: Vec<usize> = match &p.face {
        None => {
            let tag = match p.end {
                End::Start => Boundary::Port(0),
                End::End => Boundary::Port(1),
            };
            (0..mesh.boundary.len()).filter(|&i| mesh.boundary[i].tag == tag).collect()
        }
        Some(f) => {
            let tol = 1e-6 * f.a.max(f.b);
            let inside =
                |q: Point3| (q.z - f.z).abs() <= tol && q.x >= f.x0 - tol && q.x <= f.x0 + f.a + tol && q.y >= f.y0 - tol && q.y <= f.y0 + f.b + tol;
            (0..mesh.boundary.len())
                .filter(|&i| mesh.boundary[i].nodes.iter().all(|n| mesh.nodes.get(*n as usize).is_some_and(|q| inside(*q))))
                .collect()
        }
    };
    if out.is_empty() {
        return Err(invalid(format!("port `{}`", p.name), "no boundary face lies there (is the guide closed by a face on that plane?)"));
    }
    Ok(out)
}

/// A port of a whole rectangle or circle filled with one material: its closed-form modes and
/// their vectors.
#[allow(clippy::too_many_arguments)]
fn analytic_port(
    model: &Model,
    p: &emcraft_model::Port,
    shape: emcraft_model::PortShape,
    mesh: &Mesh,
    topo: &Topology,
    dofs: &Dofs,
    order: Order,
    faces: &[usize],
) -> Result<PortModes> {
    let m = model.port_material(p.end)?;
    if m.loss_tangent != 0.0 {
        return Err(invalid(format!("port `{}`", p.name), "not supported yet: a port in a lossy material (end the guide with a lossless section)"));
    }
    let list: Vec<Mode> = match shape {
        emcraft_model::PortShape::Rect { x0, y0, a, b } => modes::lowest(a, b, p.modes as usize).into_iter().map(|md| md.at(x0, y0)).collect(),
        emcraft_model::PortShape::Circle { cx, cy, r } => modes::lowest_circular(r, p.modes as usize).into_iter().map(|md| md.at(cx, cy)).collect(),
    };
    let mut vectors: Vec<Vec<f64>> = vec![vec![0.0; dofs.unknowns]; list.len()];
    let n = if order == Order::One { 6 } else { 8 };
    let mut touched = vec![false; dofs.unknowns];
    for b in faces.iter().filter_map(|&i| mesh.boundary.get(i)) {
        let tet = b.tet as usize;
        let el = element(mesh, topo, tet, order)?;
        let unknowns: Vec<u32> = dofs.of_tet(topo, tet).collect();
        for (l, wa, _) in el.face_rule(local_face(topo, tet, &b.nodes)?, n) {
            let x = el.point(l);
            let values = el.values(l);
            // Only the functions of the face's own edges and the face have a tangential trace on
            // it; the others are normal to it (zero but for rounding), and keeping them would
            // make the dense port block several times larger.
            let scale = values.iter().map(|v| v.norm()).fold(0.0, f64::max);
            for (mi, mode) in list.iter().enumerate() {
                let e = mode.field(x.x, x.y);
                let e = Vec3::new(e[0], e[1], 0.0);
                for (u, v) in unknowns.iter().zip(&values) {
                    if *u != NONE && v.x.hypot(v.y) > 1e-9 * scale {
                        vectors[mi][*u as usize] += wa * v.dot(e);
                        touched[*u as usize] = true;
                    }
                }
            }
        }
    }
    let support: Vec<usize> = (0..dofs.unknowns).filter(|&u| touched[u]).collect();
    let modes = list.into_iter().zip(vectors).map(|(mode, v)| (mode, support.iter().map(|&u| (u, v[u])).collect())).collect();
    Ok(PortModes::Analytic { eps_r: m.eps_r, mu_r: m.mu_r, modes })
}

/// A port of any other cross-section: its faces as the triangles of a [`crate::portmode`]
/// problem, materials from the tetrahedra behind them, and the unknowns with a trace there.
fn numeric_port(
    model: &Model,
    p: &emcraft_model::Port,
    mesh: &Mesh,
    topo: &Topology,
    dofs: &Dofs,
    order: Order,
    faces: &[usize],
) -> Result<NumericPort> {
    let section = cross_section(model, p, mesh, order, faces)?;
    // The unknowns with a tangential trace on the faces (as for the closed forms).
    let mut touched = std::collections::BTreeSet::new();
    let rule = if order == Order::One { 6 } else { 8 };
    for b in faces.iter().filter_map(|&i| mesh.boundary.get(i)) {
        let tet = b.tet as usize;
        let el = element(mesh, topo, tet, order)?;
        let unknowns: Vec<u32> = dofs.of_tet(topo, tet).collect();
        for (l, _, _) in el.face_rule(local_face(topo, tet, &b.nodes)?, rule) {
            let values = el.values(l);
            let scale = values.iter().map(|v| v.norm()).fold(0.0, f64::max);
            touched.extend(unknowns.iter().zip(&values).filter(|(u, v)| **u != NONE && v.x.hypot(v.y) > 1e-9 * scale).map(|(u, _)| *u as usize));
        }
    }
    Ok(NumericPort { section, faces: faces.to_vec(), count: p.modes as usize, support: touched.into_iter().collect() })
}

/// A port's cross-section from its faces: materials from the tetrahedra behind them, conductors
/// where wall faces meet it, edges bent as the volume's.
fn cross_section(model: &Model, p: &emcraft_model::Port, mesh: &Mesh, order: Order, faces: &[usize]) -> Result<crate::portmode::CrossSection> {
    let what = || format!("port `{}`", p.name);
    let mut global: Vec<u32> = faces.iter().filter_map(|&f| mesh.boundary.get(f)).flat_map(|b| b.nodes).collect();
    global.sort_unstable();
    global.dedup();
    let local = |g: u32| global.binary_search(&g).map(|k| k as u32).map_err(|_| invalid(what(), "a face's node is missing"));
    let nodes = global.iter().map(|&g| mesh.node(g).ok_or_else(|| invalid(what(), "a face's node is missing"))).collect::<Result<Vec<_>>>()?;
    let mut tris = Vec::with_capacity(faces.len());
    let (mut eps, mut mu) = (Vec::with_capacity(faces.len()), Vec::with_capacity(faces.len()));
    for &f in faces {
        let b = mesh.boundary.get(f).ok_or_else(|| invalid(what(), "a face is missing from the mesh"))?;
        tris.push([local(b.nodes[0])?, local(b.nodes[1])?, local(b.nodes[2])?]);
        let m = mesh
            .tet_material
            .get(b.tet as usize)
            .and_then(|&m| model.materials.get(m as usize))
            .ok_or_else(|| invalid(what(), "a face has no material behind it"))?;
        if m.loss_tangent != 0.0 {
            return Err(invalid(what(), "not supported yet: a port in a lossy material (end the guide with a lossless section)"));
        }
        eps.push(m.eps_r);
        mu.push(m.mu_r);
    }
    // Edges of the faces bent onto a curved wall, by local nodes.
    let mut curved = std::collections::BTreeMap::new();
    for t in &tris {
        for [i, j] in emcraft_fem::tri::TRI_EDGES {
            let (a, b) = (t[i].min(t[j]), t[i].max(t[j]));
            if let Some(m) = mesh.midpoint(global[a as usize], global[b as usize]) {
                curved.insert([a, b], m);
            }
        }
    }
    // Conductors inside the cross-section (a strip ending on the port): the section's edges that
    // also bound a wall face, by local nodes. (Its rim is a wall anyway.)
    let own: std::collections::HashSet<usize> = faces.iter().copied().collect();
    let wall_edges: std::collections::HashSet<[u32; 2]> = mesh
        .boundary
        .iter()
        .enumerate()
        .filter(|(i, b)| !own.contains(i) && b.tag == Boundary::Pec)
        .flat_map(|(_, b)| emcraft_fem::tri::TRI_EDGES.map(|[i, j]| [b.nodes[i].min(b.nodes[j]), b.nodes[i].max(b.nodes[j])]))
        .collect();
    let conductors: Vec<[u32; 2]> = tris
        .iter()
        .flat_map(|t| emcraft_fem::tri::TRI_EDGES.map(|[i, j]| [t[i].min(t[j]), t[i].max(t[j])]))
        .filter(|[a, b]| wall_edges.contains(&[global[*a as usize], global[*b as usize]]))
        .collect();
    crate::portmode::CrossSection::new(order, nodes, tris, eps, mu, &conductors, &curved)
}

/// A port mode at a frequency: its name and β² (rad²/m²; negative when evanescent).
#[derive(Clone, Debug, PartialEq)]
pub struct ModeInfo {
    pub name: String,
    pub beta2: f64,
    /// Whether the closed forms gave it.
    pub closed_form: bool,
    /// The characteristic impedance by power and current (Ω), for a propagating mode of a
    /// cross-section with one conductor besides the ground (a TEM or quasi-TEM line).
    pub impedance: Option<f64>,
}

/// Each port's modes at `frequency` (Hz), from the meshed model, without assembling the
/// volume: closed forms where they apply, the cross-section solver elsewhere.
pub fn port_modes(model: &Model, order: Order, frequency: f64) -> Result<Vec<(String, Vec<ModeInfo>)>> {
    if !(frequency.is_finite() && frequency > 0.0) {
        return Err(invalid("frequency", format!("must be a finite number > 0, got {frequency}")));
    }
    let mesh = model.mesh.as_ref().ok_or_else(|| SolveError::NotReady("no mesh".into()))?;
    if model.ports.is_empty() {
        return Err(SolveError::NotReady("no ports: create one with model.port.create".into()));
    }
    let k0 = 2.0 * PI * frequency / C0;
    model
        .ports
        .iter()
        .map(|p| {
            let shape = if p.mode_solver == emcraft_model::ModeSolver::Numeric { None } else { model.port_shape_of(p).ok() };
            let list = match shape {
                Some(shape) => {
                    let m = model.port_material(p.end)?;
                    let k = k0 * (m.eps_r * m.mu_r).sqrt();
                    let modes = match shape {
                        emcraft_model::PortShape::Rect { a, b, .. } => modes::lowest(a, b, p.modes as usize),
                        emcraft_model::PortShape::Circle { r, .. } => modes::lowest_circular(r, p.modes as usize),
                    };
                    modes
                        .iter()
                        .map(|md| ModeInfo { name: md.name(), beta2: k * k - md.cutoff().powi(2), closed_form: true, impedance: None })
                        .collect()
                }
                None => {
                    let section = cross_section(model, p, mesh, order, &faces_of_port(mesh, p)?)?;
                    section
                        .modes(k0, p.modes as usize)?
                        .into_iter()
                        .enumerate()
                        .map(|(i, m)| {
                            let impedance = section.impedance(&m, k0)?;
                            Ok(ModeInfo { name: format!("mode {}", i + 1), beta2: m.beta2, closed_form: false, impedance })
                        })
                        .collect::<Result<Vec<_>>>()?
                }
            };
            Ok((p.name.clone(), list))
        })
        .collect()
}

/// The port's vector: the line integral along the gap, or for a band of radius r its average
/// over 16 lines evenly round the band (the trapezoid rule, spectrally accurate round a
/// circle): the voltage is the band's mean line integral and the current spreads evenly round
/// it.
fn band_vector(mesh: &Mesh, topo: &Topology, dofs: &Dofs, order: Order, a: Point3, b: Point3, r: f64) -> Result<Vec<(usize, f64)>> {
    if r == 0.0 {
        return line_vector(mesh, topo, dofs, order, a, b);
    }
    let s = (b - a).normalized().ok_or_else(|| invalid("lumped port", "its two ends coincide"))?;
    // Two unit vectors across the line.
    let helper = Vec3::unit(if s.x.abs() < 0.9 { 0 } else { 1 });
    let u = s.cross(helper).normalized().unwrap_or(Vec3::unit(0));
    let v = s.cross(u);
    let n = 16;
    let mut out: std::collections::BTreeMap<usize, f64> = std::collections::BTreeMap::new();
    for k in 0..n {
        let phi = 2.0 * PI * k as f64 / n as f64;
        let off = (u * phi.cos() + v * phi.sin()) * r;
        for (i, c) in line_vector(mesh, topo, dofs, order, a + off, b + off)? {
            *out.entry(i).or_default() += c / n as f64;
        }
    }
    Ok(out.into_iter().collect())
}

/// ∫ Nᵢ · ŝ dl along the segment from `a` to `b` (ŝ its direction), for every unknown: the
/// segment is clipped to each tetrahedron it crosses (barycentric coordinates are linear along
/// it), and each stretch between the clips' breakpoints is integrated once, in one of the
/// tetrahedra holding it (the tangential component along the line is the same in all of them),
/// by four-point Gauss rules.
fn line_vector(mesh: &Mesh, topo: &Topology, dofs: &Dofs, order: Order, a: Point3, b: Point3) -> Result<Vec<(usize, f64)>> {
    let d = b - a;
    let len = d.norm();
    let s = d / len;
    let mut pieces: Vec<(f64, f64, usize)> = Vec::new();
    // Only the tetrahedra whose boxes meet the segment's.
    for t in emcraft_mesh::tets_near(mesh, emcraft_model::Aabb::new(a.min(b), a.max(b))) {
        let Some(c) = mesh.corners(t) else { continue };
        // Barycentric coordinates of a and b in the straight tetrahedron.
        let e = [1, 2, 3].map(|i| c[i] - c[0]);
        let det = emcraft_model::geom::triple(e[0], e[1], e[2]);
        if det == 0.0 {
            continue;
        }
        let g = [e[1].cross(e[2]), e[2].cross(e[0]), e[0].cross(e[1])];
        let bary = |p: Point3| {
            let l = g.map(|gi| gi.dot(p - c[0]) / det);
            [1.0 - l[0] - l[1] - l[2], l[0], l[1], l[2]]
        };
        let (la, lb) = (bary(a), bary(b));
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let tol = 1e-12;
        for i in 0..4 {
            let (u, v) = (la[i], lb[i]);
            // u + t (v − u) ≥ −tol.
            if (v - u).abs() < 1e-15 {
                if u < -tol {
                    t1 = -1.0;
                }
            } else {
                let t = (-tol - u) / (v - u);
                if v > u { t0 = t0.max(t) } else { t1 = t1.min(t) }
            }
        }
        if t1 - t0 > 1e-12 {
            pieces.push((t0.max(0.0), t1.min(1.0), t));
        }
    }
    let mut cuts: Vec<f64> = pieces.iter().flat_map(|p| [p.0, p.1]).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
    let (x, w) = emcraft_fem::quadrature::gauss_legendre(4);
    let mut out: std::collections::BTreeMap<usize, f64> = std::collections::BTreeMap::new();
    for win in cuts.windows(2) {
        let (t0, t1) = (win[0], win[1]);
        let mid = 0.5 * (t0 + t1);
        let Some(&(_, _, tet)) = pieces.iter().find(|p| p.0 <= mid && mid <= p.1) else { continue };
        let el = element(mesh, topo, tet, order)?;
        let unknowns: Vec<u32> = dofs.of_tet(topo, tet).collect();
        for (xi, wi) in x.iter().zip(&w) {
            let t = t0 + xi * (t1 - t0);
            for (u, v) in unknowns.iter().zip(el.values(el.barycentric(a + d * t))) {
                if *u != NONE {
                    *out.entry(*u as usize).or_default() += wi * (t1 - t0) * len * v.dot(s);
                }
            }
        }
    }
    Ok(out.into_iter().collect())
}

/// The local corners (in the sorted order) of a tetrahedron's boundary face.
fn local_face(topo: &Topology, t: usize, nodes: &[u32; 3]) -> Result<[usize; 3]> {
    let s = topo.sorted.get(t).ok_or_else(|| invalid("mesh", format!("no tetrahedron {t}")))?;
    let mut out = [0usize; 3];
    for (o, n) in out.iter_mut().zip(nodes) {
        *o = s.iter().position(|v| v == n).ok_or_else(|| invalid("mesh", format!("boundary face {nodes:?} is not on tetrahedron {t}")))?;
    }
    Ok(out)
}

/// The element of a tetrahedron, corners sorted by global node.
fn element(mesh: &Mesh, topo: &Topology, t: usize, order: Order) -> Result<Element> {
    let s = topo.sorted.get(t).ok_or_else(|| invalid("mesh", format!("no tetrahedron {t}")))?;
    let corners = s.map(|n| mesh.node(n).unwrap_or(Point3::new(f64::NAN, f64::NAN, f64::NAN)));
    let el = Element::new(order, corners)?;
    if mesh.curved.is_empty() {
        return Ok(el);
    }
    Ok(el.curved(LOCAL_EDGES.map(|[i, j]| mesh.midpoint(s[i], s[j])))?)
}

/// Element matrices of a structured mesh repeat: tetrahedra that are translates of each other
/// (the same edge vectors in the same sorted corner order) share them.
#[derive(Default)]
struct ElementCache {
    map: std::collections::HashMap<[i64; 9], (Vec<f64>, Vec<f64>)>,
}

impl ElementCache {
    fn matrices(&mut self, mesh: &Mesh, topo: &Topology, t: usize, order: Order) -> Result<(Vec<f64>, Vec<f64>)> {
        let s = topo.sorted.get(t).ok_or_else(|| invalid("mesh", format!("no tetrahedron {t}")))?;
        // Curved tetrahedra are their own.
        if LOCAL_EDGES.iter().any(|[i, j]| mesh.midpoint(s[*i], s[*j]).is_some()) {
            let el = element(mesh, topo, t, order)?;
            return Ok((el.stiffness(1.0), el.mass(1.0)));
        }
        let c = s.map(|n| mesh.node(n).unwrap_or(Point3::new(f64::NAN, f64::NAN, f64::NAN)));
        // Edge vectors quantised far below the mesh size (10⁻¹⁵ m): only exact repeats match.
        let mut key = [0i64; 9];
        for (k, e) in [1, 2, 3].map(|k| c[k] - c[0]).iter().enumerate() {
            for (d, v) in e.iter().enumerate() {
                key[3 * k + d] = (v * 1e15).round() as i64;
            }
        }
        if let Some(m) = self.map.get(&key) {
            return Ok(m.clone());
        }
        let el = Element::new(order, c)?;
        let m = (el.stiffness(1.0), el.mass(1.0));
        if self.map.len() < 100_000 {
            self.map.insert(key, m.clone());
        }
        Ok(m)
    }
}

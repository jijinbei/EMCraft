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
//! **S-parameters.** Each port is excited in turn by a unit TE₁₀ amplitude, all from one
//! factorization. S_pq = (c₁₀ at p − δ_pq) for excitation at q, normalised to equal power
//! (times √(P_p/P_q), P = ‖e‖² β/μ the power of a unit wave), so S is unitary for a lossless
//! structure even when the ports' media or apertures differ.

use emcraft_fem::nedelec::LOCAL_EDGES;
use std::f64::consts::PI;

use emcraft_fem::modes::{self, Mode};
use emcraft_fem::poly::{Vec3, dot};
use emcraft_fem::{Element, Order};
use emcraft_mesh::Topology;
use emcraft_model::units::{C0, MU0};
use emcraft_model::{Boundary, End, Mesh, Model};
use num_complex::Complex64 as C;

use crate::ldlt::{FactorError, Ldlt};
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

fn invalid(what: impl Into<String>, why: impl Into<String>) -> SolveError {
    SolveError::Invalid { what: what.into(), why: why.into() }
}

const NONE: u32 = u32::MAX;

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
    pub points: Vec<[f64; 3]>,
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
            let p = if g < ne * pe {
                let [a, b] = topo.edges[g / pe];
                let (p, q) = (node(a), node(b));
                [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0, (p[2] + q[2]) / 2.0]
            } else {
                let [a, b, c] = topo.faces[(g - ne * pe) / pf];
                let (p, q, r) = (node(a), node(b), node(c));
                [(p[0] + q[0] + r[0]) / 3.0, (p[1] + q[1] + r[1]) / 3.0, (p[2] + q[2] + r[2]) / 3.0]
            };
            points.push(p);
        }
        Ok(Dofs { order, edges: ne, faces: nf, unknown, unknowns: free, points })
    }

    /// The unknowns of a tetrahedron's local basis functions ([`NONE`] for eliminated ones).
    fn of_tet(&self, topo: &Topology, tet: usize) -> Vec<u32> {
        let (pe, pf) = (self.order.per_edge(), self.order.per_face());
        let te = topo.tet_edges[tet];
        let tf = topo.tet_faces[tet];
        let mut out = Vec::with_capacity(self.order.dofs());
        for j in 0..pe {
            for e in te {
                out.push(self.unknown[e as usize * pe + j]);
            }
        }
        for f in tf {
            for j in 0..pf {
                out.push(self.unknown[self.edges * pe + f as usize * pf + j]);
            }
        }
        out
    }
}

/// A port prepared for solving.
#[derive(Clone, Debug)]
pub struct PortData {
    pub name: String,
    pub end: End,
    pub eps_r: f64,
    pub mu_r: f64,
    /// Each mode with its vector v_m,i = ∫ Nᵢ · e_m dS (unknown, value).
    pub modes: Vec<(Mode, Vec<(usize, f64)>)>,
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
    pattern: Csr,
    stiffness: Vec<f64>,
    mass: Vec<C>,
    /// Walls of finite conductivity: σ (S/m) and their tangential mass matrix.
    wall: Option<(f64, Vec<f64>)>,
    /// The boundary faces that are walls (indices into the mesh's).
    walls: Vec<usize>,
    pub ports: Vec<PortData>,
}

/// The solution at one frequency.
#[derive(Clone, Debug, PartialEq)]
pub struct FrequencyResult {
    pub frequency: f64,
    /// S[p][q]: at port p for a unit TE₁₀ wave into port q.
    pub s: Vec<Vec<C>>,
    /// The field coefficients for each excitation (port q).
    pub fields: Vec<Vec<C>>,
    /// The worst backward error of the solves.
    pub backward_error: f64,
    /// Pivots replaced by the factorization's static pivoting.
    pub perturbed_pivots: usize,
    /// Entries of the factor and its largest frontal matrix (rows).
    pub factor_entries: usize,
    pub largest_front: usize,
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
        Driven::build(model, order, false, false)
    }

    /// The cavity with perfectly conducting walls whatever their conductivity: the lossless
    /// problem whose modes perturbation theory gives Q factors from.
    pub fn cavity(model: &Model, order: Order) -> Result<Driven> {
        Driven::build(model, order, false, true)
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
        // Walls: the conducting faces and, with no port there, the end faces.
        let port_tags: Vec<Boundary> = if with_ports {
            model
                .ports
                .iter()
                .map(|p| match p.end {
                    End::Start => Boundary::Port(0),
                    End::End => Boundary::Port(1),
                })
                .collect()
        } else {
            Vec::new()
        };
        let walls: Vec<usize> = (0..mesh.boundary.len()).filter(|&i| !port_tags.contains(&mesh.boundary[i].tag)).collect();
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

        // Port vectors.
        let mut ports = Vec::new();
        for p in model.ports.iter().filter(|_| with_ports) {
            let tag = match p.end {
                End::Start => Boundary::Port(0),
                End::End => Boundary::Port(1),
            };
            let m = model.port_material(p.end)?;
            if m.loss_tangent != 0.0 {
                return Err(invalid(
                    format!("port `{}`", p.name),
                    "not supported yet: a port in a lossy material (end the guide with a lossless section)",
                ));
            }
            let list: Vec<Mode> = match model.port_shape(p.end)? {
                emcraft_model::PortShape::Rect { x0, y0, a, b } => {
                    modes::lowest(a, b, p.modes as usize).into_iter().map(|md| md.at(x0, y0)).collect()
                }
                emcraft_model::PortShape::Circle { cx, cy, r } => {
                    modes::lowest_circular(r, p.modes as usize).into_iter().map(|md| md.at(cx, cy)).collect()
                }
            };
            let mut vectors: Vec<Vec<f64>> = vec![vec![0.0; dofs.unknowns]; list.len()];
            let n = if order == Order::One { 6 } else { 8 };
            let mut touched = vec![false; dofs.unknowns];
            for b in mesh.boundary.iter().filter(|b| b.tag == tag) {
                let tet = b.tet as usize;
                let el = element(&mesh, &topo, tet, order)?;
                let unknowns = dofs.of_tet(&topo, tet);
                for (l, wa, _) in el.face_rule(local_face(&topo, tet, &b.nodes)?, n) {
                    let x = el.point(l);
                    let values = el.values(l);
                    // Only the functions of the face's own edges and the face have a tangential
                    // trace on it; the others are normal to it (zero but for rounding), and
                    // keeping them would make the dense port block several times larger.
                    let scale = values.iter().map(|v| norm3(*v)).fold(0.0, f64::max);
                    for (mi, mode) in list.iter().enumerate() {
                        let e = mode.field(x[0], x[1]);
                        let e = [e[0], e[1], 0.0];
                        for (u, v) in unknowns.iter().zip(&values) {
                            if *u != NONE && v[0].hypot(v[1]) > 1e-9 * scale {
                                vectors[mi][*u as usize] += wa * dot(*v, e);
                                touched[*u as usize] = true;
                            }
                        }
                    }
                }
            }
            let support: Vec<usize> = (0..dofs.unknowns).filter(|&u| touched[u]).collect();
            let modes = list.into_iter().zip(vectors).map(|(mode, v)| (mode, support.iter().map(|&u| (u, v[u])).collect())).collect();
            ports.push(PortData { name: p.name.clone(), end: p.end, eps_r: m.eps_r, mu_r: m.mu_r, modes });
        }

        // Pattern: element couplings and the dense port blocks.
        let mut groups: Vec<Vec<usize>> =
            (0..mesh.tets.len()).map(|t| dofs.of_tet(&topo, t).into_iter().filter(|&u| u != NONE).map(|u| u as usize).collect()).collect();
        for p in &ports {
            if let Some((_, v)) = p.modes.first() {
                groups.push(v.iter().map(|(u, _)| *u).collect());
            }
        }
        let pattern = Csr::from_groups(dofs.unknowns, groups.iter().map(|g| g.as_slice()));
        let mut kk = Csr { val: vec![C::new(0.0, 0.0); pattern.col.len()], ..pattern.clone() };
        let mut mm = kk.clone();
        let mut cache = ElementCache::default();
        for t in 0..mesh.tets.len() {
            let (s, m) = cache.matrices(&mesh, &topo, t, order)?;
            let unknowns = dofs.of_tet(&topo, t);
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
                let unknowns = dofs.of_tet(&topo, tet);
                for (l, wa, normal) in el.face_rule(local_face(&topo, tet, &b.nodes)?, n) {
                    let t: Vec<Vec3> = el.values(l).into_iter().map(|v| tangential(v, normal)).collect();
                    for (i, ui) in unknowns.iter().enumerate() {
                        if *ui == NONE {
                            continue;
                        }
                        for (j, uj) in unknowns.iter().enumerate() {
                            if *uj != NONE {
                                ww.add(*ui as usize, *uj as usize, C::new(wa * dot(t[i], t[j]), 0.0));
                            }
                        }
                    }
                }
            }
        }
        let wall = sigma.map(|c| (c, ww.val.iter().map(|v| v.re).collect()));
        Ok(Driven { order, mesh, topo, dofs, eps, mu, pattern, stiffness, mass, wall, walls, ports })
    }

    /// The system matrix at wavenumber k₀ and each port's TE₁₀ excitation vector.
    fn system(&self, frequency: f64) -> Result<(Csr, Vec<Vec<C>>)> {
        let k0 = 2.0 * PI * frequency / C0;
        let mut a = self.pattern.clone();
        for (x, (s, m)) in a.val.iter_mut().zip(self.stiffness.iter().zip(&self.mass)) {
            *x = C::new(*s, 0.0) - m * (k0 * k0);
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
        let mut rhs = Vec::new();
        for p in &self.ports {
            let k = k0 * (p.eps_r * p.mu_r).sqrt();
            let mut b = vec![C::new(0.0, 0.0); self.dofs.unknowns];
            for (mi, (mode, v)) in p.modes.iter().enumerate() {
                let (kr, ki) = mode.kappa(k).ok_or_else(|| {
                    invalid("frequency", format!("{} Hz is at the cut-off of mode {} at port `{}`", frequency, mode.name(), p.name))
                })?;
                let kappa = C::new(kr, ki) / p.mu_r;
                let c = kappa / mode.norm();
                for &(i, vi) in v {
                    for &(j, vj) in v {
                        a.add(i, j, c * (vi * vj));
                    }
                }
                if mi == 0 {
                    for &(i, vi) in v {
                        b[i] = 2.0 * kappa * vi;
                    }
                }
            }
            rhs.push(b);
        }
        Ok((a, rhs))
    }

    /// Solves at one frequency (Hz).
    pub fn solve(&self, frequency: f64) -> Result<FrequencyResult> {
        if !(frequency.is_finite() && frequency > 0.0) {
            return Err(invalid("frequency", format!("must be a finite number > 0, got {frequency}")));
        }
        let k0 = 2.0 * PI * frequency / C0;
        for p in &self.ports {
            let te10 = p.modes.first().map(|m| m.0).ok_or_else(|| invalid(format!("port `{}`", p.name), "has no modes"))?;
            if k0 * (p.eps_r * p.mu_r).sqrt() <= te10.cutoff() {
                return Err(invalid(
                    "frequency",
                    format!("{frequency} Hz is below the TE10 cut-off at port `{}`: no wave can carry power into the port", p.name),
                ));
            }
        }
        let (a, rhs) = self.system(frequency)?;
        let f = Ldlt::factor(&a, &self.dofs.points)?;
        let mut fields = Vec::with_capacity(rhs.len());
        let mut worst: f64 = 0.0;
        for b in &rhs {
            let (x, e) = f.solve_refined(&a, b, 4)?;
            worst = worst.max(e);
            fields.push(x);
        }
        let np = self.ports.len();
        let mut s = vec![vec![C::new(0.0, 0.0); np]; np];
        for (q, x) in fields.iter().enumerate() {
            for (p, port) in self.ports.iter().enumerate() {
                let Some((mode, v)) = port.modes.first() else { continue };
                let c: C = v.iter().map(|&(i, vi)| x[i] * vi).sum::<C>() / mode.norm();
                let direct = if p == q { c - 1.0 } else { c };
                s[p][q] = direct * self.power_factor(p, q, k0);
            }
        }
        Ok(FrequencyResult {
            frequency,
            s,
            fields,
            backward_error: worst,
            perturbed_pivots: f.perturbed,
            factor_entries: f.entries,
            largest_front: f.largest_front,
        })
    }

    /// √(P_p/P_q) for TE₁₀ waves of unit amplitude at ports p and q: a wave of amplitude A
    /// carries ½|A|² ‖e‖² β/(ωμ), so S in power waves is the amplitude ratio times this.
    fn power_factor(&self, p: usize, q: usize, k0: f64) -> f64 {
        let power = |port: &PortData| port.modes.first().map_or(1.0, |(m, _)| m.norm() * m.gamma(k0 * (port.eps_r * port.mu_r).sqrt()).1 / port.mu_r);
        match (self.ports.get(p), self.ports.get(q)) {
            (Some(pp), Some(qq)) => (power(pp) / power(qq)).sqrt(),
            _ => 1.0,
        }
    }

    /// The field (E in V/m or H in A/m) at a point for the coefficients `x` at `frequency`.
    pub fn field(&self, x: &[C], frequency: f64, which: Field, p: [f64; 3]) -> Option<[C; 3]> {
        let (t, _) = emcraft_mesh::locate(&self.mesh, p)?;
        let el = element(&self.mesh, &self.topo, t, self.order).ok()?;
        Some(self.field_in(&el, t, x, frequency, which, el.barycentric(p)))
    }

    /// E and H at a point, `None` outside the mesh.
    pub fn fields(&self, x: &[C], frequency: f64, p: [f64; 3]) -> Option<([C; 3], [C; 3])> {
        let (t, _) = emcraft_mesh::locate(&self.mesh, p)?;
        let el = element(&self.mesh, &self.topo, t, self.order).ok()?;
        let l = el.barycentric(p);
        Some((self.field_in(&el, t, x, frequency, Field::E, l), self.field_in(&el, t, x, frequency, Field::H, l)))
    }

    fn field_in(&self, el: &Element, t: usize, x: &[C], frequency: f64, which: Field, l: [f64; 4]) -> [C; 3] {
        let unknowns = self.dofs.of_tet(&self.topo, t);
        let values = match which {
            Field::E => el.values(l),
            Field::H => el.curl_values(l),
        };
        let mut out = [C::new(0.0, 0.0); 3];
        for (u, v) in unknowns.iter().zip(&values) {
            if *u == NONE {
                continue;
            }
            let c = x.get(*u as usize).copied().unwrap_or_default();
            for d in 0..3 {
                out[d] += c * v[d];
            }
        }
        if which == Field::H {
            // H = −∇ × E / (jωμ).
            let jwmu = C::new(0.0, 2.0 * PI * frequency * MU0 * self.mu.get(t).copied().unwrap_or(1.0));
            out = out.map(|c| -c / jwmu);
        }
        out
    }

    /// ‖field − exact‖ and ‖exact‖ in L²(Ω), by quadrature on every tetrahedron.
    pub fn l2_error(&self, x: &[C], frequency: f64, which: Field, exact: impl Fn([f64; 3]) -> [C; 3]) -> Result<(f64, f64)> {
        let n = if self.order == Order::One { 3 } else { 4 };
        let (mut err, mut norm) = (0.0, 0.0);
        for t in 0..self.mesh.tets.len() {
            let el = element(&self.mesh, &self.topo, t, self.order)?;
            for (l, w) in el.rule(n) {
                let u = self.field_in(&el, t, x, frequency, which, l);
                let e = exact(el.point(l));
                for d in 0..3 {
                    err += w * (u[d] - e[d]).norm_sqr();
                    norm += w * e[d].norm_sqr();
                }
            }
        }
        Ok((err.sqrt(), norm.sqrt()))
    }

    /// The power (W) the dielectrics absorb for the coefficients `x`: ½ ω ε₀ ∫ εr tan δ |E|² dV
    /// (fields in peak values).
    pub fn dielectric_loss(&self, x: &[C], frequency: f64) -> Result<f64> {
        let omega = 2.0 * PI * frequency;
        let eps0 = 1.0 / (MU0 * C0 * C0);
        let n = if self.order == Order::One { 3 } else { 4 };
        let mut total = 0.0;
        for t in 0..self.mesh.tets.len() {
            let loss = -self.eps.get(t).map_or(0.0, |e| e.im);
            if loss == 0.0 {
                continue;
            }
            let el = element(&self.mesh, &self.topo, t, self.order)?;
            for (l, w) in el.rule(n) {
                let e = self.field_in(&el, t, x, frequency, Field::E, l);
                total += w * loss * e.iter().map(|c| c.norm_sqr()).sum::<f64>();
            }
        }
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
        let n = if self.order == Order::One { 3 } else { 4 };
        let mut total = 0.0;
        for b in self.walls.iter().filter_map(|&i| self.mesh.boundary.get(i)) {
            let tet = b.tet as usize;
            let el = element(&self.mesh, &self.topo, tet, self.order)?;
            for (l, wa, normal) in el.face_rule(local_face(&self.topo, tet, &b.nodes)?, n) {
                let h = self.field_in(&el, tet, x, frequency, Field::H, l);
                let hn = h[0] * normal[0] + h[1] * normal[1] + h[2] * normal[2];
                total += wa * (0..3).map(|d| (h[d] - hn * normal[d]).norm_sqr()).sum::<f64>();
            }
        }
        Ok(0.5 * rs * total)
    }

    /// The time-averaged stored energy (J) at resonance, twice the electric: ½ ε₀ ∫ εr |E|² dV.
    pub fn stored_energy(&self, x: &[C], frequency: f64) -> Result<f64> {
        let eps0 = 1.0 / (MU0 * C0 * C0);
        let n = if self.order == Order::One { 3 } else { 4 };
        let mut total = 0.0;
        for t in 0..self.mesh.tets.len() {
            let el = element(&self.mesh, &self.topo, t, self.order)?;
            let er = self.eps.get(t).map_or(1.0, |e| e.re);
            for (l, w) in el.rule(n) {
                let e = self.field_in(&el, t, x, frequency, Field::E, l);
                total += w * er * e.iter().map(|c| c.norm_sqr()).sum::<f64>();
            }
        }
        Ok(0.5 * eps0 * total)
    }

    /// The power (W) the walls of finite conductivity absorb: ∫ |E_t|²/(4R_s) dS (= ½ R_s ∫ |H_t|²
    /// dS with the Leontovich condition).
    pub fn wall_loss(&self, x: &[C], frequency: f64) -> Result<f64> {
        let Some((sigma, _)) = &self.wall else { return Ok(0.0) };
        let rs = surface_resistance(frequency, *sigma);
        let n = if self.order == Order::One { 3 } else { 4 };
        let mut total = 0.0;
        for b in self.walls.iter().filter_map(|&i| self.mesh.boundary.get(i)) {
            let tet = b.tet as usize;
            let el = element(&self.mesh, &self.topo, tet, self.order)?;
            for (l, wa, normal) in el.face_rule(local_face(&self.topo, tet, &b.nodes)?, n) {
                let e = self.field_in(&el, tet, x, frequency, Field::E, l);
                let en = e[0] * normal[0] + e[1] * normal[1] + e[2] * normal[2];
                let et: f64 = (0..3).map(|d| (e[d] - en * normal[d]).norm_sqr()).sum();
                total += wa * et;
            }
        }
        Ok(total / (4.0 * rs))
    }

    /// The power (W) a unit TE₁₀ wave carries into port `p`: ½ ‖e‖² β/(ωμ).
    pub fn incident_power(&self, p: usize, frequency: f64) -> Option<f64> {
        let port = self.ports.get(p)?;
        let (mode, _) = port.modes.first()?;
        let omega = 2.0 * PI * frequency;
        let k = omega / C0 * (port.eps_r * port.mu_r).sqrt();
        Some(0.5 * mode.norm() * mode.gamma(k).1 / (omega * MU0 * port.mu_r))
    }
}

fn norm3(v: Vec3) -> f64 {
    dot(v, v).sqrt()
}

/// R_s = √(ωμ₀/2σ) (Ω).
pub fn surface_resistance(frequency: f64, sigma: f64) -> f64 {
    (PI * frequency * MU0 / sigma).sqrt()
}

/// jωμ₀/Z_s = ωμ₀(1 + j)/(2R_s) (1/m).
fn wall_factor(frequency: f64, sigma: f64) -> C {
    let omega = 2.0 * PI * frequency;
    C::new(1.0, 1.0) * (omega * MU0 / (2.0 * surface_resistance(frequency, sigma)))
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

/// v − (v·n)n.
fn tangential(v: Vec3, n: Vec3) -> Vec3 {
    emcraft_fem::poly::sub(v, emcraft_fem::poly::scale(n, dot(v, n)))
}

/// The element of a tetrahedron, corners sorted by global node.
fn element(mesh: &Mesh, topo: &Topology, t: usize, order: Order) -> Result<Element> {
    let s = topo.sorted.get(t).ok_or_else(|| invalid("mesh", format!("no tetrahedron {t}")))?;
    let corners = s.map(|n| mesh.nodes.get(n as usize).copied().unwrap_or([f64::NAN; 3]));
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
        let c = s.map(|n| mesh.nodes.get(n as usize).copied().unwrap_or([f64::NAN; 3]));
        // Edge vectors quantised far below the mesh size (10⁻¹⁵ m): only exact repeats match.
        let mut key = [0i64; 9];
        for k in 0..3 {
            for d in 0..3 {
                key[3 * k + d] = ((c[k + 1][d] - c[0][d]) * 1e15).round() as i64;
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

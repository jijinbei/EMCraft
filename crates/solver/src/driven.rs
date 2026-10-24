//! Driven frequency-domain analysis of a waveguide: the vector wave equation
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
//! factorization. S_pq = (c₁₀ at p − δ_pq) for excitation at q, normalised to equal power,
//! √(Z_q/Z_p) with Z = ωμ/β, so S is unitary for a lossless structure even when the port media
//! differ.

use std::f64::consts::PI;

use emcraft_fem::modes::{self, Mode};
use emcraft_fem::poly::{Vec3, dot};
use emcraft_fem::quadrature::{tet_rule, triangle_rule};
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
    fn new(order: Order, mesh: &Mesh, topo: &Topology) -> Result<Dofs> {
        let (pe, pf) = (order.per_edge(), order.per_face());
        let (ne, nf) = (topo.edges.len(), topo.faces.len());
        let total = ne * pe + nf * pf;
        let mut fixed = vec![false; total];
        for b in mesh.boundary.iter().filter(|b| b.tag == Boundary::Pec) {
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
    /// Relative permittivity and permeability per tetrahedron.
    eps: Vec<f64>,
    mu: Vec<f64>,
    pattern: Csr,
    stiffness: Vec<f64>,
    mass: Vec<f64>,
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
        let issues = model.check();
        if !issues.is_empty() {
            return Err(SolveError::NotReady(issues.join("; ")));
        }
        let mesh = model.mesh.clone().ok_or_else(|| SolveError::NotReady("no mesh".into()))?;
        let w = model.waveguide.as_ref().ok_or_else(|| SolveError::NotReady("no waveguide".into()))?;
        let topo = Topology::new(&mesh);
        let dofs = Dofs::new(order, &mesh, &topo)?;
        let material = |t: usize| mesh.tet_material.get(t).and_then(|&m| model.materials.get(m as usize));
        let mut eps = Vec::with_capacity(mesh.tets.len());
        let mut mu = Vec::with_capacity(mesh.tets.len());
        for t in 0..mesh.tets.len() {
            let m = material(t).ok_or_else(|| invalid("mesh", format!("tetrahedron {t} has no material")))?;
            eps.push(m.eps_r);
            mu.push(m.mu_r);
        }

        // Port vectors.
        let mut ports = Vec::new();
        for p in &model.ports {
            let tag = match p.end {
                End::Start => Boundary::Port(0),
                End::End => Boundary::Port(1),
            };
            let m = model.port_material(p.end)?;
            let list = modes::lowest(w.width, w.height, p.modes as usize);
            let mut vectors: Vec<Vec<f64>> = vec![vec![0.0; dofs.unknowns]; list.len()];
            let rule = triangle_rule(if order == Order::One { 6 } else { 8 });
            let mut touched = vec![false; dofs.unknowns];
            for b in mesh.boundary.iter().filter(|b| b.tag == tag) {
                let tet = b.tet as usize;
                let el = element(&mesh, &topo, tet, order)?;
                let unknowns = dofs.of_tet(&topo, tet);
                let q = b.nodes.map(|n| mesh.nodes[n as usize]);
                let area = 0.5 * norm3(emcraft_fem::poly::cross(emcraft_fem::poly::sub(q[1], q[0]), emcraft_fem::poly::sub(q[2], q[0])));
                for (l, wq) in &rule {
                    let x = [0, 1, 2].map(|d| l[0] * q[0][d] + l[1] * q[1][d] + l[2] * q[2][d]);
                    let values = el.values(el.barycentric(x));
                    for (mi, mode) in list.iter().enumerate() {
                        let e = mode.field(x[0], x[1]);
                        let e = [e[0], e[1], 0.0];
                        for (u, v) in unknowns.iter().zip(&values) {
                            if *u != NONE {
                                vectors[mi][*u as usize] += wq * area * dot(*v, e);
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
                    mm.add(*ui as usize, *uj as usize, C::new(m[i * n + j] * eps[t], 0.0));
                }
            }
        }
        let stiffness = kk.val.iter().map(|v| v.re).collect();
        let mass = mm.val.iter().map(|v| v.re).collect();
        Ok(Driven { order, mesh, topo, dofs, eps, mu, pattern, stiffness, mass, ports })
    }

    /// The system matrix at wavenumber k₀ and each port's TE₁₀ excitation vector.
    fn system(&self, frequency: f64) -> Result<(Csr, Vec<Vec<C>>)> {
        let k0 = 2.0 * PI * frequency / C0;
        let mut a = self.pattern.clone();
        for (x, (s, m)) in a.val.iter_mut().zip(self.stiffness.iter().zip(&self.mass)) {
            *x = C::new(s - k0 * k0 * m, 0.0);
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

    /// √(Z_q/Z_p) for TE₁₀ (Z = ωμ/β, equal cross-sections).
    fn power_factor(&self, p: usize, q: usize, k0: f64) -> f64 {
        let z = |port: &PortData| {
            let mode = port.modes.first().map(|m| m.0);
            let beta = mode.map_or(1.0, |m| m.gamma(k0 * (port.eps_r * port.mu_r).sqrt()).1);
            port.mu_r / beta
        };
        match (self.ports.get(p), self.ports.get(q)) {
            (Some(pp), Some(qq)) => (z(qq) / z(pp)).sqrt(),
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
            for (l, w) in tet_rule(&el.corners, n) {
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

    /// The relative permittivity of each tetrahedron.
    pub fn permittivity(&self) -> &[f64] {
        &self.eps
    }
}

fn norm3(v: Vec3) -> f64 {
    dot(v, v).sqrt()
}

/// The element of a tetrahedron, corners sorted by global node.
fn element(mesh: &Mesh, topo: &Topology, t: usize, order: Order) -> Result<Element> {
    let s = topo.sorted.get(t).ok_or_else(|| invalid("mesh", format!("no tetrahedron {t}")))?;
    let corners = s.map(|n| mesh.nodes.get(n as usize).copied().unwrap_or([f64::NAN; 3]));
    Ok(Element::new(order, corners)?)
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

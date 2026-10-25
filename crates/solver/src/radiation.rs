//! Open problems: fields radiated by sources or scattered by dielectric bodies into free space,
//! the box lined with a perfectly matched layer ([`Driven::open`]), and their far fields.
//!
//! **Sources.** A current element (the Hertzian dipole of moment I·l along d̂ at r₀) enters the
//! weak form of ∇ × μr⁻¹∇ × E − k₀²εr E = −jωμ₀J as bᵢ = −jωμ₀ I l Nᵢ(r₀)·d̂. A plane wave
//! E_inc = ê e^{−jk₀ k̂·r} is not a source of the total field but of the field the dielectrics
//! scatter, E = E_inc + E_s: ∇ × ∇ × E_s − k₀²εr E_s = k₀²(εr − 1)E_inc (μr = 1), so
//! bᵢ = k₀² ∫ (εr − 1) Nᵢ · E_inc dV over them, and the layer absorbs only what is scattered.
//!
//! **Far field.** On a box (the Huygens surface) between the bodies and the layer, the
//! equivalent currents J = n̂ × H and M = −n̂ × E radiate the far field
//! r e^{jkr} E_θ = −jk/(4π) (L_φ + η N_θ), r e^{jkr} E_φ = jk/(4π) (L_θ − η N_φ), with
//! N = ∮ J e^{jk r̂·r'} dS' and L = ∮ M e^{jk r̂·r'} dS' (C. A. Balanis, *Advanced Engineering
//! Electromagnetics*, 2nd ed., Wiley 2012, §6.8, eqs. 6-122, 6-125), r' from the box's centre.
//! The power through the same box is ½ Re ∮ (E × H*) · n̂ dS.

use std::f64::consts::PI;

use emcraft_model::units::{C0, MU0};
use emcraft_model::{Model, Shape, Source};
use num_complex::Complex64 as C;

use crate::driven::{Driven, Result, SolveError, invalid};
use crate::ldlt::Ldlt;

/// The solution of an open problem at one frequency.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenResult {
    pub frequency: f64,
    /// The field coefficients: the total field for current sources, the scattered field for a
    /// plane wave.
    pub field: Vec<C>,
    pub backward_error: f64,
    /// Each lumped port's terminal quantities.
    pub ports: Vec<LumpedResult>,
}

/// A lumped port's terminals: with every port's source on at once, its current, the voltage
/// across its terminals, their ratio (the input impedance, or the active impedance of an
/// array element), the reflection against the port's impedance and the power it delivers.
#[derive(Clone, Debug, PartialEq)]
pub struct LumpedResult {
    pub name: String,
    pub current: C,
    pub voltage: C,
    pub impedance: C,
    pub s11: C,
    /// ½ Re(V I*), W.
    pub power: f64,
}

/// The far field in one direction: r e^{jkr} E_θ and r e^{jkr} E_φ (V).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FarField {
    pub theta: f64,
    pub phi: f64,
    pub e_theta: C,
    pub e_phi: C,
}

impl FarField {
    pub fn magnitude_squared(&self) -> f64 {
        self.e_theta.norm_sqr() + self.e_phi.norm_sqr()
    }
}

/// The Huygens box: inside the layer by a third of its thickness, so the box lies in ordinary
/// elements.
fn huygens_box(model: &Model) -> Result<([f64; 3], [f64; 3])> {
    let (Some(o), Some(emcraft_model::Solid { shape: Shape::Box { size }, .. })) = (&model.open, &model.solid) else {
        return Err(SolveError::NotReady("no open boundary".into()));
    };
    let g = o.thickness * (1.0 + 1.0 / 3.0);
    let (lo, hi) = ([g; 3], [0, 1, 2].map(|d| size[d] - g));
    // The bodies and sources must be inside it.
    let inside = |a: [f64; 3], b: [f64; 3]| (0..3).all(|d| a[d] > lo[d] && b[d] < hi[d]);
    for inc in &model.inclusions {
        let (a, b) = inc.bounds();
        if !inside(a, b) {
            return Err(invalid(
                "open boundary",
                "an inclusion reaches past the Huygens surface (a third of the layer's thickness inside it); enlarge the box",
            ));
        }
    }
    for f in &model.features {
        let (a, b) = f.bounds();
        if !inside(a, b) {
            return Err(invalid(
                "open boundary",
                "a feature reaches past the Huygens surface (a third of the layer's thickness inside it); enlarge the box",
            ));
        }
    }
    for s in &model.sources {
        if let Source::Dipole { at, .. } = s
            && !inside(*at, *at)
        {
            return Err(invalid("dipole", "must lie inside the Huygens surface (a third of the layer's thickness inside it)"));
        }
    }
    Ok((lo, hi))
}

/// Points and weights on the box's faces, each with its outward normal: patches of about
/// `step` with 3 × 3 Gauss points.
fn box_rule(lo: [f64; 3], hi: [f64; 3], step: f64) -> Vec<([f64; 3], f64, [f64; 3])> {
    let (x, w) = emcraft_fem::quadrature::gauss_legendre(3);
    let mut out = Vec::new();
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let (lu, lv) = (hi[u] - lo[u], hi[v] - lo[v]);
        let (nu, nv) = (((lu / step).ceil() as usize).clamp(2, 400), ((lv / step).ceil() as usize).clamp(2, 400));
        for (side, plane) in [(-1.0, lo[axis]), (1.0, hi[axis])] {
            let mut n = [0.0; 3];
            n[axis] = side;
            for i in 0..nu {
                for j in 0..nv {
                    for (a, wa) in x.iter().zip(&w) {
                        for (b, wb) in x.iter().zip(&w) {
                            let mut p = [0.0; 3];
                            p[axis] = plane;
                            p[u] = lo[u] + (i as f64 + a) * lu / nu as f64;
                            p[v] = lo[v] + (j as f64 + b) * lv / nv as f64;
                            out.push((p, wa * wb * lu * lv / (nu * nv) as f64, n));
                        }
                    }
                }
            }
        }
    }
    out
}

fn cross(a: [C; 3], b: [C; 3]) -> [C; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn real(v: [f64; 3]) -> [C; 3] {
    v.map(|x| C::new(x, 0.0))
}

impl Driven {
    /// Solves the open problem driven by the model's sources at `frequency` (Hz).
    pub fn solve_open(&self, model: &Model, frequency: f64) -> Result<OpenResult> {
        if !(frequency.is_finite() && frequency > 0.0) {
            return Err(invalid("frequency", format!("must be a finite number > 0, got {frequency}")));
        }
        if model.sources.is_empty() && self.lumped.is_empty() {
            return Err(SolveError::NotReady("no sources: add one with source.dipole, source.plane_wave or port.lumped".into()));
        }
        huygens_box(model)?;
        let omega = 2.0 * PI * frequency;
        let k0 = omega / C0;
        let (a, mut rhs) = self.system(frequency)?;
        let mut b = rhs.pop().unwrap_or_else(|| vec![C::new(0.0, 0.0); self.dofs.unknowns]);
        b.iter_mut().for_each(|v| *v = C::new(0.0, 0.0));
        for src in &model.sources {
            match *src {
                Source::Dipole { at, direction, length, current } => {
                    let (t, _) = emcraft_mesh::locate(&self.mesh, at).ok_or_else(|| invalid("dipole", "lies outside the mesh"))?;
                    let el = self.element_of(t)?;
                    let values = el.values(el.barycentric(at));
                    let c = C::new(0.0, -omega * MU0 * current * length);
                    for (u, v) in self.dofs_of(t).iter().zip(&values) {
                        if *u != crate::driven::NONE {
                            b[*u as usize] += c * (v[0] * direction[0] + v[1] * direction[1] + v[2] * direction[2]);
                        }
                    }
                }
                Source::PlaneWave { direction, polarization } => {
                    self.lift_conductors(model, k0, direction, polarization, &mut b)?;
                    let in_pml: std::collections::HashSet<usize> = self.pml.iter().flat_map(|(_, l)| l.iter().map(|(t, _)| *t)).collect();
                    for t in 0..self.mesh.tets.len() {
                        let contrast = self.eps_of(t) - 1.0;
                        if contrast.norm() == 0.0 || in_pml.contains(&t) {
                            continue;
                        }
                        if self.mu_of(t) != 1.0 {
                            return Err(invalid("plane wave", "not supported yet: magnetic scatterers (μr ≠ 1)"));
                        }
                        let el = self.element_of(t)?;
                        let unknowns = self.dofs_of(t);
                        for (l, w) in el.rule(if self.order == emcraft_fem::Order::One { 3 } else { 4 }) {
                            let p = el.point(l);
                            let phase = C::from_polar(1.0, -k0 * (direction[0] * p[0] + direction[1] * p[1] + direction[2] * p[2]));
                            let c = k0 * k0 * contrast * phase * w;
                            for (u, v) in unknowns.iter().zip(el.values(l)) {
                                if *u != crate::driven::NONE {
                                    b[*u as usize] += c * (v[0] * polarization[0] + v[1] * polarization[1] + v[2] * polarization[2]);
                                }
                            }
                        }
                    }
                }
            }
        }
        // Lumped ports: their sources drive −jωμ₀ (V_s/R) c (see the system's rank-one terms).
        for l in &self.lumped {
            let f = C::new(0.0, -omega * MU0) * l.voltage / l.impedance;
            for &(i, ci) in &l.c {
                b[i] += f * ci;
            }
        }
        let f = Ldlt::factor(&a, &self.dofs.points)?;
        let (field, backward_error) = f.solve_refined(&a, &b, 4)?;
        let ports = self
            .lumped
            .iter()
            .map(|l| {
                // ∫ E · ŝ dl across the gap, the current through it and the terminal voltage
                // V = V_s − R I = −∫ E · ŝ dl.
                let line: C = l.c.iter().map(|&(i, ci)| field[i] * ci).sum();
                let current = (l.voltage + line) / l.impedance;
                let voltage = -line;
                let impedance = voltage / current;
                let s11 = (impedance - l.impedance) / (impedance + l.impedance);
                LumpedResult { name: l.name.clone(), current, voltage, impedance, s11, power: 0.5 * (voltage * current.conj()).re }
            })
            .collect();
        Ok(OpenResult { frequency, field, backward_error, ports })
    }

    /// The plane wave's conductors: on them the scattered field is n × E_s = −n × E_inc, so their
    /// eliminated degrees of freedom take the values g of that trace (its L² projection on the
    /// conducting surface: Σ_j ∫ (n × Nᵢ)·(n × Nⱼ) gⱼ = −∫ (n × Nᵢ)·(n × E_inc)), and the free
    /// ones' right-hand side loses A_IB g, element by element. Conductors are the walls not on
    /// the box's own faces (behind the layer, where the scattered field has died away).
    fn lift_conductors(&self, model: &Model, k0: f64, direction: [f64; 3], polarization: [f64; 3], b: &mut [C]) -> Result<()> {
        let Some(emcraft_model::Solid { shape: Shape::Box { size }, .. }) = &model.solid else { return Ok(()) };
        let tol = 1e-9 * size.iter().fold(0.0f64, |m, v| m.max(*v));
        let node = |n: u32| self.mesh.nodes.get(n as usize).copied().unwrap_or([f64::NAN; 3]);
        let on_box = |nodes: &[u32; 3]| (0..3).any(|d| [0.0, size[d]].iter().any(|c| nodes.iter().all(|n| (node(*n)[d] - c).abs() <= tol)));
        let faces: Vec<(usize, usize)> = self
            .mesh
            .boundary
            .iter()
            .filter(|f| !on_box(&f.nodes))
            .filter_map(|f| self.topo.find_face(f.tet as usize, f.nodes).map(|(lf, _)| (f.tet as usize, lf)))
            .collect();
        if faces.is_empty() {
            return Ok(());
        }
        // The local functions with a tangential trace on local face lf.
        let order = self.order;
        let on_face = |lf: usize| -> Vec<usize> {
            let corners = emcraft_fem::nedelec::LOCAL_FACES[lf];
            let mut out = Vec::new();
            for (e, [a, c]) in emcraft_fem::nedelec::LOCAL_EDGES.iter().enumerate() {
                if corners.contains(a) && corners.contains(c) {
                    out.push(e);
                    if order == emcraft_fem::Order::Two {
                        out.push(6 + e);
                    }
                }
            }
            if order == emcraft_fem::Order::Two {
                out.extend([12 + 2 * lf, 13 + 2 * lf]);
            }
            out
        };
        let mut index: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        let mut points = Vec::new();
        let mut groups: Vec<Vec<usize>> = Vec::with_capacity(faces.len());
        for &(t, lf) in &faces {
            let slots = self.dofs.slots_of_tet(&self.topo, t);
            let c = self.mesh.corners(t).unwrap_or([[0.0; 3]; 4]);
            let centroid = [0, 1, 2].map(|d| (c[0][d] + c[1][d] + c[2][d] + c[3][d]) / 4.0);
            groups.push(
                on_face(lf)
                    .into_iter()
                    .map(|li| {
                        *index.entry(slots[li]).or_insert_with(|| {
                            points.push(centroid);
                            points.len() - 1
                        })
                    })
                    .collect(),
            );
        }
        let nb = points.len();
        let mut m = crate::sparse::Csr::from_groups(nb, groups.iter().map(|g| g.as_slice()));
        let mut r = vec![C::new(0.0, 0.0); nb];
        let tangential = |v: [f64; 3], n: [f64; 3]| {
            let d = v[0] * n[0] + v[1] * n[1] + v[2] * n[2];
            [v[0] - d * n[0], v[1] - d * n[1], v[2] - d * n[2]]
        };
        for (&(t, lf), group) in faces.iter().zip(&groups) {
            let el = self.element_of(t)?;
            let local = on_face(lf);
            let corners = emcraft_fem::nedelec::LOCAL_FACES[lf];
            for (l, w, n) in el.face_rule(corners, 4) {
                let v = el.values(l);
                let p = el.point(l);
                let phase = C::from_polar(1.0, -k0 * (direction[0] * p[0] + direction[1] * p[1] + direction[2] * p[2]));
                let g = tangential(polarization, n).map(|c| -phase * c);
                let tv: Vec<[f64; 3]> = local.iter().map(|&li| tangential(v[li], n)).collect();
                for (a, ta) in group.iter().zip(&tv) {
                    r[*a] += w * (ta[0] * g[0] + ta[1] * g[1] + ta[2] * g[2]);
                    for (bb, tb) in group.iter().zip(&tv) {
                        m.add(*a, *bb, C::new(w * (ta[0] * tb[0] + ta[1] * tb[1] + ta[2] * tb[2]), 0.0));
                    }
                }
            }
        }
        let f = Ldlt::factor(&m, &points)?;
        let (g, _) = f.solve_refined(&m, &r, 2)?;
        // Lift: every tetrahedron with a conductor's degree of freedom.
        let in_pml: std::collections::HashSet<usize> = self.pml.iter().flat_map(|(_, l)| l.iter().map(|(t, _)| *t)).collect();
        for t in 0..self.mesh.tets.len() {
            let slots = self.dofs.slots_of_tet(&self.topo, t);
            let known: Vec<(usize, C)> = slots.iter().enumerate().filter_map(|(li, s)| index.get(s).map(|&k| (li, g[k]))).collect();
            if known.is_empty() {
                continue;
            }
            if in_pml.contains(&t) {
                return Err(invalid("plane wave", "a conductor reaches into the perfectly matched layer"));
            }
            let el = self.element_of(t)?;
            let (s, mm) = (el.stiffness(1.0), el.mass(1.0));
            let n = slots.len();
            let (eps, mu) = (self.eps_of(t), self.mu_of(t));
            for (i, u) in self.dofs_of(t).iter().enumerate() {
                if *u == crate::driven::NONE {
                    continue;
                }
                for &(j, gj) in &known {
                    let a = s[i * n + j] / mu - k0 * k0 * eps * mm[i * n + j];
                    b[*u as usize] -= a * gj;
                }
            }
        }
        Ok(())
    }

    /// E and H at the Huygens surface's points, with the points' weights and normals.
    fn on_huygens(&self, model: &Model, x: &[C], frequency: f64) -> Result<Vec<([f64; 3], f64, [f64; 3], [C; 3], [C; 3])>> {
        let (lo, hi) = huygens_box(model)?;
        let lambda = C0 / frequency;
        let mut out = Vec::new();
        for (p, w, n) in box_rule(lo, hi, lambda / 12.0) {
            let (e, h) = self.fields(x, frequency, p).ok_or_else(|| invalid("Huygens surface", "a point of it lies outside the mesh"))?;
            out.push((p, w, n, e, h));
        }
        Ok(out)
    }

    /// The far field in the directions (θ, φ) given, its phase from the Huygens box's centre.
    pub fn far_field(&self, model: &Model, x: &[C], frequency: f64, directions: &[(f64, f64)]) -> Result<Vec<FarField>> {
        let k = 2.0 * PI * frequency / C0;
        let eta = MU0 * C0;
        let (lo, hi) = huygens_box(model)?;
        let centre = [0, 1, 2].map(|d| 0.5 * (lo[d] + hi[d]));
        let surface = self.on_huygens(model, x, frequency)?;
        Ok(directions
            .iter()
            .map(|&(theta, phi)| {
                let (st, ct, sp, cp) = (theta.sin(), theta.cos(), phi.sin(), phi.cos());
                let r = [st * cp, st * sp, ct];
                let (th, ph) = ([ct * cp, ct * sp, -st], [-sp, cp, 0.0]);
                let (mut n, mut l) = ([C::new(0.0, 0.0); 3], [C::new(0.0, 0.0); 3]);
                for (p, w, nrm, e, h) in &surface {
                    let q = [p[0] - centre[0], p[1] - centre[1], p[2] - centre[2]];
                    let ph_ = C::from_polar(*w, k * (r[0] * q[0] + r[1] * q[1] + r[2] * q[2]));
                    let j = cross(real(*nrm), *h);
                    let m = cross(real(*nrm), *e).map(|c| -c);
                    for d in 0..3 {
                        n[d] += j[d] * ph_;
                        l[d] += m[d] * ph_;
                    }
                }
                let dot = |a: [C; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
                let c = C::new(0.0, k / (4.0 * PI));
                FarField { theta, phi, e_theta: -c * (dot(l, ph) + eta * dot(n, th)), e_phi: c * (dot(l, th) - eta * dot(n, ph)) }
            })
            .collect())
    }

    /// The power (W) the field carries out through the Huygens box.
    pub fn radiated_power(&self, model: &Model, x: &[C], frequency: f64) -> Result<f64> {
        Ok(self
            .on_huygens(model, x, frequency)?
            .iter()
            .map(|(_, w, n, e, h)| {
                let s = cross(*e, h.map(|c| c.conj()));
                0.5 * w * (s[0].re * n[0] + s[1].re * n[1] + s[2].re * n[2])
            })
            .sum())
    }
}

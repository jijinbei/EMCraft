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
use emcraft_model::{Combine, Model, Shape, Source};
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
        if model.sources.is_empty() {
            return Err(SolveError::NotReady("no sources: add one with source.dipole or source.plane_wave".into()));
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
                    if model.inclusions.iter().any(|i| i.material.is_none()) || model.features.iter().any(|f| f.combine != Combine::Add) {
                        return Err(invalid(
                            "plane wave",
                            "not supported yet: plane waves on conductors (the scattered field needs n × E_s = −n × E_inc on them)",
                        ));
                    }
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
        let f = Ldlt::factor(&a, &self.dofs.points)?;
        let (field, backward_error) = f.solve_refined(&a, &b, 4)?;
        Ok(OpenResult { frequency, field, backward_error })
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

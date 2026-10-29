//! Open problems: fields radiated by sources or scattered by dielectric bodies into free space,
//! the box lined with a perfectly matched layer or the sphere whose outer shell is one
//! ([`Driven::open`]), and their far fields.
//!
//! **Sources.** A current element (the Hertzian dipole of moment I·l along d̂ at r₀) enters the
//! weak form of ∇ × μr⁻¹∇ × E − k₀²εr E = −jωμ₀J as bᵢ = −jωμ₀ I l Nᵢ(r₀)·d̂. A plane wave
//! E_inc = ê e^{−jk₀ k̂·r} is not a source of the total field but of the field the dielectrics
//! scatter, E = E_inc + E_s: ∇ × ∇ × E_s − k₀²εr E_s = k₀²(εr − 1)E_inc (μr = 1), so
//! bᵢ = k₀² ∫ (εr − 1) Nᵢ · E_inc dV over them, and the layer absorbs only what is scattered.
//!
//! **Far field.** On a box (the Huygens surface; a sphere inside a spherical layer) between the bodies and the layer, the
//! equivalent currents J = n̂ × H and M = −n̂ × E radiate the far field
//! r e^{jkr} E_θ = −jk/(4π) (L_φ + η N_θ), r e^{jkr} E_φ = jk/(4π) (L_θ − η N_φ), with
//! N = ∮ J e^{jk r̂·r'} dS' and L = ∮ M e^{jk r̂·r'} dS' (C. A. Balanis, *Advanced Engineering
//! Electromagnetics*, 2nd ed., Wiley 2012, §6.8, eqs. 6-122, 6-125), r' from the box's (sphere's) centre.
//! The power through the same box is ½ Re ∮ (E × H*) · n̂ dS.

use std::f64::consts::PI;

use emcraft_model::units::{C0, MU0};
use emcraft_model::{Model, Point3, Shape, Source, Vec3};
use emcraft_theory::radiation::{Huygens, HuygensSample};
use num_complex::Complex64 as C;

use crate::driven::{Driven, ModeTerm, Result, SolveError, invalid};
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
    /// A waveguide port feeding the problem (the model's first, its TE10 of unit amplitude
    /// incident): its reflection and the power it delivers, P_inc (1 − |S11|²).
    pub feed: Option<(C, f64)>,
    /// The values of the eliminated degrees of freedom on a plane wave's conductors (global
    /// slot, value), ascending by slot: there the scattered field's trace is −n × E_inc, not
    /// zero (`lift_conductors`). Empty without a plane wave.
    pub lifted: Vec<(usize, C)>,
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

/// The Huygens box: each face a third of the layer's thickness inside the layer's inner face (in
/// ordinary elements, clear of those the layer's profile reaches), or halfway between that face
/// and what radiates (the bodies, features, dipoles and lumped ports' bounding box) when that is
/// nearer, so it stays as far from the sources' near fields as the box allows. Whatever
/// radiates must keep clear of the layer by an element or so.
fn huygens_box(model: &Model) -> Result<(Point3, Point3)> {
    let (Some(o), Some(emcraft_model::Solid { shape: Shape::Box { size }, .. })) = (&model.open, &model.solid) else {
        return Err(SolveError::NotReady("no open boundary".into()));
    };
    if o.shape != emcraft_model::OpenShape::Box {
        return Err(invalid("open boundary", "a spherical layer on a box solid"));
    }
    let d = o.thickness;
    let (inner_lo, inner_hi) = (Point3::new(d, d, d), size.to_point() - Vec3::new(d, d, d));
    // Nothing placed (a plane wave on nothing): the middle of the box.
    let centre = inner_lo.midpoint(inner_hi);
    let emcraft_model::Aabb { min: clo, max: chi } = emcraft_model::Aabb::around(model.radiators()).unwrap_or(emcraft_model::Aabb::point(centre));
    let margin = 0.1 * d;
    if (0..3).any(|k| clo[k] < inner_lo[k] + margin || chi[k] > inner_hi[k] - margin) {
        return Err(invalid("open boundary", "the bodies and sources reach into or up to the perfectly matched layer; enlarge the box"));
    }
    let inset = d / 3.0;
    Ok((inner_lo + (clo - inner_lo).map(|g| inset.min(0.5 * g)), inner_hi - (inner_hi - chi).map(|g| inset.min(0.5 * g))))
}

/// The surface the far field is transformed from.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Surface {
    /// A box between the sources and a box's layer ([`huygens_box`]).
    Box(Point3, Point3),
    /// A sphere of this radius about the origin, inside a spherical layer ([`huygens_sphere`]).
    Sphere(f64),
}

impl Surface {
    fn of(model: &Model) -> Result<Surface> {
        match model.open.map(|o| o.shape) {
            Some(emcraft_model::OpenShape::Sphere) => huygens_sphere(model).map(Surface::Sphere),
            _ => huygens_box(model).map(|(lo, hi)| Surface::Box(lo, hi)),
        }
    }

    fn centre(&self) -> Point3 {
        match *self {
            Surface::Box(lo, hi) => lo.midpoint(hi),
            Surface::Sphere(_) => Point3::ORIGIN,
        }
    }

    fn rule(&self, step: f64) -> Vec<(Point3, f64, Vec3)> {
        match *self {
            Surface::Box(lo, hi) => box_rule(lo, hi, step),
            Surface::Sphere(r) => sphere_rule(r, step),
        }
    }
}

/// The Huygens sphere of a spherical layer (inner radius r₀ = R − d): a third of the layer's
/// thickness inside r₀, or halfway between r₀ and the farthest of what radiates when that is
/// nearer — the box's rule ([`huygens_box`]) in the radius.
fn huygens_sphere(model: &Model) -> Result<f64> {
    let inner = model.spherical_layer().map(|l| l.inner).ok_or_else(|| SolveError::NotReady("no spherical layer".into()))?;
    let d = model.open.map_or(0.0, |o| o.thickness);
    let reach = model.radiators().map(|p| p.to_vec().norm()).fold(0.0f64, f64::max);
    if !(reach < inner - 0.1 * d) {
        return Err(invalid("open boundary", "the bodies and sources reach into or up to the spherical layer; enlarge the sphere"));
    }
    Ok(inner - (d / 3.0).min(0.5 * (inner - reach)))
}

/// Points and weights on a sphere of radius r about the origin, with their outward normals:
/// bands in θ of about `step` with 3 Gauss points each (weights r² sin θ dθ), and in φ a uniform
/// rule of three points per `step` round the equator (exact for the trigonometric polynomials
/// up to that degree).
fn sphere_rule(r: f64, step: f64) -> Vec<(Point3, f64, Vec3)> {
    let (x, w) = emcraft_fem::quadrature::gauss_legendre(3);
    let bands = ((PI * r / step).ceil() as usize).clamp(2, 400);
    let around = (3 * ((2.0 * PI * r / step).ceil() as usize)).clamp(6, 2400);
    let (dtheta, dphi) = (PI / bands as f64, 2.0 * PI / around as f64);
    let mut out = Vec::with_capacity(bands * 3 * around);
    for b in 0..bands {
        for (xa, wa) in x.iter().zip(&w) {
            let theta = (b as f64 + xa) * dtheta;
            let (st, ct) = theta.sin_cos();
            for k in 0..around {
                let (sp, cp) = ((k as f64 + 0.5) * dphi).sin_cos();
                let n = Vec3::new(st * cp, st * sp, ct);
                out.push((Point3::ORIGIN + n * r, r * r * st * wa * dtheta * dphi, n));
            }
        }
    }
    out
}

/// Points and weights on the box's faces, each with its outward normal: patches of about
/// `step` with 3 × 3 Gauss points.
fn box_rule(lo: Point3, hi: Point3, step: f64) -> Vec<(Point3, f64, Vec3)> {
    let (x, w) = emcraft_fem::quadrature::gauss_legendre(3);
    let gauss: Vec<(f64, f64)> = x.into_iter().zip(w).collect();
    let span = hi - lo;
    (0..3)
        .flat_map(|axis| [(axis, -1.0, lo[axis]), (axis, 1.0, hi[axis])])
        .flat_map(|(axis, side, plane)| {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let (nu, nv) = (((span[u] / step).ceil() as usize).clamp(2, 400), ((span[v] / step).ceil() as usize).clamp(2, 400));
            let area = span[u] * span[v] / (nu * nv) as f64;
            let normal = Vec3::unit(axis) * side;
            let gauss = &gauss;
            (0..nu).flat_map(move |i| (0..nv).map(move |j| (i, j))).flat_map(move |(i, j)| {
                gauss.iter().flat_map(move |&(a, wa)| {
                    gauss.iter().map(move |&(b, wb)| {
                        let p = lo
                            .with(axis, plane)
                            .with(u, lo[u] + (i as f64 + a) * span[u] / nu as f64)
                            .with(v, lo[v] + (j as f64 + b) * span[v] / nv as f64);
                        (p, wa * wb * area, normal)
                    })
                })
            })
        })
        .collect()
}

impl Driven {
    /// Solves the open problem driven by the model's sources at `frequency` (Hz).
    pub fn solve_open(&self, model: &Model, frequency: f64) -> Result<OpenResult> {
        if !(frequency.is_finite() && frequency > 0.0) {
            return Err(invalid("frequency", format!("must be a finite number > 0, got {frequency}")));
        }
        if model.sources.is_empty() && self.lumped.is_empty() && self.ports.is_empty() {
            return Err(SolveError::NotReady(
                "no sources: add one with source.dipole, source.plane_wave, model.port.lumped or a waveguide port".into(),
            ));
        }
        Surface::of(model)?;
        let omega = 2.0 * PI * frequency;
        let k0 = omega / C0;
        let system = self.system(frequency)?;
        let f = self.factor(&system)?;
        let crate::driven::System { a, rhs, terms, border } = system;
        let n = self.dofs.unknowns;
        // A waveguide port's first mode drives it (the first port's), else nothing but the sources.
        let mut b = rhs.into_iter().next().unwrap_or_else(|| vec![C::new(0.0, 0.0); n + border]);
        let mut lifted: std::collections::BTreeMap<usize, C> = std::collections::BTreeMap::new();
        for src in &model.sources {
            match *src {
                Source::Dipole { at, direction, length, current } => {
                    let (t, _) = emcraft_mesh::locate(&self.mesh, at).ok_or_else(|| invalid("dipole", "lies outside the mesh"))?;
                    let el = self.element_of(t)?;
                    let values = el.values(el.barycentric(at));
                    let c = C::new(0.0, -omega * MU0 * current * length);
                    for (u, v) in self.dofs_of(t).zip(&values).filter(|(u, _)| *u != crate::driven::NONE) {
                        b[u as usize] += c * v.dot(direction);
                    }
                }
                Source::PlaneWave { direction, polarization } => {
                    for (slot, g) in self.lift_conductors(model, k0, direction, polarization, &mut b)? {
                        *lifted.entry(slot).or_default() += g;
                    }
                    let in_pml = self.pml_tets();
                    for t in 0..self.mesh.tets.len() {
                        let contrast = self.eps_at(t, frequency) - 1.0;
                        if contrast.norm() == 0.0 || in_pml.contains(&t) {
                            continue;
                        }
                        if self.mu_of(t) != C::new(1.0, 0.0) {
                            return Err(invalid("plane wave", "not supported yet: magnetic scatterers (μr ≠ 1)"));
                        }
                        let el = self.element_of(t)?;
                        let unknowns: Vec<u32> = self.dofs_of(t).collect();
                        for (l, w) in el.rule(crate::driven::rule_points(self.order)) {
                            let phase = C::from_polar(1.0, -k0 * direction.dot(el.point(l).to_vec()));
                            let c = k0 * k0 * contrast * phase * w;
                            for (u, v) in unknowns.iter().zip(el.values(l)).filter(|(u, _)| **u != crate::driven::NONE) {
                                b[*u as usize] += c * v.dot(polarization);
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
        let (mut field, backward_error) = f.solve_refined(&a, &b, 4)?;
        field.truncate(n);
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
        let feed = match (terms.first().and_then(|l| l.first()), self.incident_power(0, frequency)) {
            (Some(t), Some(incident)) => {
                let s11 = t.coefficient(&field) - 1.0;
                Some((s11, incident * (1.0 - s11.norm_sqr())))
            }
            _ => None,
        };
        Ok(OpenResult { frequency, field, backward_error, ports, feed, lifted: lifted.into_iter().collect() })
    }

    /// The plane wave's conductors: on them the scattered field is n × E_s = −n × E_inc, so their
    /// eliminated degrees of freedom take the values g of that trace (its L² projection on the
    /// conducting surface: Σ_j ∫ (n × Nᵢ)·(n × Nⱼ) gⱼ = −∫ (n × Nᵢ)·(n × E_inc)), and the free
    /// ones' right-hand side loses A_IB g, element by element. Conductors are the walls not on
    /// the box's own faces or the sphere's (behind the layer, where the scattered field has died
    /// away). Returns the eliminated degrees of freedom's values g (global slot, value).
    fn lift_conductors(&self, model: &Model, k0: f64, direction: Vec3, polarization: Vec3, b: &mut [C]) -> Result<Vec<(usize, C)>> {
        let node = |n: u32| self.mesh.node(n).unwrap_or(Point3::new(f64::NAN, f64::NAN, f64::NAN));
        let on_box = |nodes: &[u32; 3]| match model.solid.as_ref().map(|s| &s.shape) {
            Some(Shape::Box { size }) => {
                let tol = 1e-9 * size.iter().fold(0.0f64, f64::max);
                (0..3).any(|d| [0.0, size[d]].iter().any(|c| nodes.iter().all(|n| (node(*n)[d] - c).abs() <= tol)))
            }
            Some(Shape::Sphere { radius }) => nodes.iter().all(|n| (node(*n).to_vec().norm() - radius).abs() <= 1e-9 * radius),
            _ => true,
        };
        let faces: Vec<(usize, usize)> = self
            .mesh
            .boundary
            .iter()
            .filter(|f| !on_box(&f.nodes))
            .filter_map(|f| self.topo.find_face(f.tet as usize, f.nodes).map(|(lf, _)| (f.tet as usize, lf)))
            .collect();
        if faces.is_empty() {
            return Ok(Vec::new());
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
            let slots: Vec<usize> = self.dofs.slots_of_tet(&self.topo, t).collect();
            let centroid = self.mesh.centroid(t).unwrap_or(Point3::ORIGIN);
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
        for (&(t, lf), group) in faces.iter().zip(&groups) {
            let el = self.element_of(t)?;
            let local = on_face(lf);
            let corners = emcraft_fem::nedelec::LOCAL_FACES[lf];
            for (l, w, n) in el.face_rule(corners, 4) {
                let v = el.values(l);
                let phase = C::from_polar(1.0, -k0 * direction.dot(el.point(l).to_vec()));
                let g = polarization.tangential(n) * -phase;
                let tv: Vec<Vec3> = local.iter().map(|&li| v[li].tangential(n)).collect();
                for (a, ta) in group.iter().zip(&tv) {
                    r[*a] += g.dot_real(*ta) * w;
                    for (bb, tb) in group.iter().zip(&tv) {
                        m.add(*a, *bb, C::new(w * ta.dot(*tb), 0.0));
                    }
                }
            }
        }
        let f = Ldlt::factor(&m, &points)?;
        let (g, _) = f.solve_refined(&m, &r, 2)?;
        // Lift: every tetrahedron with a conductor's degree of freedom.
        let in_pml = self.pml_tets();
        for t in 0..self.mesh.tets.len() {
            let known: Vec<(usize, C)> =
                self.dofs.slots_of_tet(&self.topo, t).enumerate().filter_map(|(li, s)| index.get(&s).map(|&k| (li, g[k]))).collect();
            if known.is_empty() {
                continue;
            }
            if in_pml.contains(&t) {
                return Err(invalid("plane wave", "a conductor reaches into the perfectly matched layer"));
            }
            let el = self.element_of(t)?;
            let (s, mm) = (el.stiffness(1.0), el.mass(1.0));
            let n = self.order.dofs();
            let (eps, mu) = (self.eps_at(t, k0 * C0 / (2.0 * PI)), self.mu_of(t));
            for (i, u) in self.dofs_of(t).enumerate().filter(|(_, u)| *u != crate::driven::NONE) {
                for &(j, gj) in &known {
                    let a = s[i * n + j] / mu - k0 * k0 * eps * mm[i * n + j];
                    b[u as usize] -= a * gj;
                }
            }
        }
        let mut values: Vec<(usize, C)> = index.iter().map(|(&slot, &k)| (slot, g.get(k).copied().unwrap_or_default())).collect();
        values.sort_unstable_by_key(|(slot, _)| *slot);
        Ok(values)
    }

    /// The fields on the Huygens box round the model's sources and bodies, for the far field
    /// and the radiated power. Points sharing a tetrahedron share its element.
    pub fn huygens(&self, model: &Model, x: &[C], frequency: f64) -> Result<Huygens> {
        let surface = Surface::of(model)?;
        let mut elements: std::collections::HashMap<usize, emcraft_fem::Element> = std::collections::HashMap::new();
        let samples = surface
            .rule(C0 / frequency / 12.0)
            .into_iter()
            .map(|(point, weight, normal)| {
                let (t, _) =
                    emcraft_mesh::locate(&self.mesh, point).ok_or_else(|| invalid("Huygens surface", "a point of it lies outside the mesh"))?;
                let el = match elements.entry(t) {
                    std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                    std::collections::hash_map::Entry::Vacant(v) => v.insert(self.element_of(t)?),
                };
                let l = el.barycentric(point);
                let (e, h) =
                    (self.field_in(el, t, x, frequency, crate::driven::Field::E, l), self.field_in(el, t, x, frequency, crate::driven::Field::H, l));
                Ok(HuygensSample { point, weight, normal, e, h })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Huygens { frequency, centre: surface.centre(), samples })
    }

    /// The far field in the directions (θ, φ) given, its phase from the Huygens box's centre.
    pub fn far_field(&self, model: &Model, x: &[C], frequency: f64, directions: &[(f64, f64)]) -> Result<Vec<FarField>> {
        let h = self.huygens(model, x, frequency)?;
        Ok(far_fields(&h, directions))
    }

    /// The power (W) the field carries out through the Huygens box.
    pub fn radiated_power(&self, model: &Model, x: &[C], frequency: f64) -> Result<f64> {
        Ok(self.huygens(model, x, frequency)?.power())
    }
}

/// The far field of a Huygens surface in the directions (θ, φ) given.
pub fn far_fields(h: &Huygens, directions: &[(f64, f64)]) -> Vec<FarField> {
    directions
        .iter()
        .map(|&(theta, phi)| {
            let (e_theta, e_phi) = h.far_field(theta, phi);
            FarField { theta, phi, e_theta, e_phi }
        })
        .collect()
}

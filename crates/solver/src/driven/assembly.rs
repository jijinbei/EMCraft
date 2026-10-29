//! Assembly: the elements of the mesh, the frequency-independent volume and wall matrices, and
//! a frequency's system bordered by the ports' modes.

use emcraft_fem::nedelec::LOCAL_EDGES;
use emcraft_fem::{Element, Order};
use emcraft_mesh::Topology;
use emcraft_model::{BoundaryFace, Mesh, Point3, Vec3};
use num_complex::Complex64 as C;

use super::media::{Media, per_mu};
use super::terms::PortTerm;
use super::{Dofs, Driven, NONE, Result, invalid, wall_factor, wavenumber};
use crate::sparse::Csr;

/// Gauss points per direction for integrals of fields or of two basis functions over elements
/// and wall faces.
pub(crate) fn rule_points(order: Order) -> usize {
    if order == Order::One { 3 } else { 4 }
}

/// Gauss points per direction on port faces, for the modes' fields against the basis.
pub(super) fn port_rule_points(order: Order) -> usize {
    if order == Order::One { 6 } else { 8 }
}

/// The finite-element space: the mesh, its topology and the unknowns on it.
#[derive(Clone, Copy)]
pub(crate) struct Space<'a> {
    pub mesh: &'a Mesh,
    pub topo: &'a Topology,
    pub dofs: &'a Dofs,
}

/// A boundary face's quadrature: the tetrahedron behind it, its element and unknowns, and the
/// rule's points (barycentric coordinates, weight, outward unit normal).
pub(super) struct FaceRule {
    pub tet: usize,
    pub el: Element,
    pub unknowns: Vec<u32>,
    pub points: Vec<([f64; 4], f64, Vec3)>,
}

/// One coupling of a tetrahedron's element matrix: local functions `i` and `j`, both unknowns
/// (`row` and `col`).
#[derive(Clone, Copy)]
pub(super) struct Coupling {
    pub i: usize,
    pub j: usize,
    pub row: usize,
    pub col: usize,
}

/// Every pair of a tetrahedron's local functions that are both unknowns, row by row.
pub(super) fn couplings(unknowns: &[u32]) -> impl Iterator<Item = Coupling> + '_ {
    let free = || unknowns.iter().enumerate().filter(|(_, u)| **u != NONE).map(|(i, u)| (i, *u as usize));
    free().flat_map(move |(i, row)| free().map(move |(j, col)| Coupling { i, j, row, col }))
}

/// The matrices that do not depend on the frequency, on the pattern.
pub(super) struct VolumeMatrices {
    /// The curl–curl matrix K (μ⁻¹, complex with magnetic losses).
    pub stiffness: Vec<C>,
    /// The mass matrix of the non-dispersive tetrahedra, weighted by their εr(1 − j tan δ).
    pub mass: Vec<C>,
    /// Per dispersive material, the unit-permittivity mass matrix of its tetrahedra.
    pub dispersive: Vec<Vec<f64>>,
}

impl<'a> Space<'a> {
    pub fn order(&self) -> Order {
        self.dofs.order
    }

    /// The element of tetrahedron `t`.
    pub fn element(&self, t: usize) -> Result<Element> {
        element(self.mesh, self.topo, t, self.order())
    }

    /// The unknowns of tetrahedron `t`'s local functions ([`NONE`] for eliminated ones).
    pub fn unknowns(&self, t: usize) -> Vec<u32> {
        self.dofs.of_tet(self.topo, t).collect()
    }

    /// The quadrature of boundary face `b` with `n` points per direction.
    pub(super) fn face_rule(&self, b: &BoundaryFace, n: usize) -> Result<FaceRule> {
        let tet = b.tet as usize;
        let el = self.element(tet)?;
        let unknowns = self.unknowns(tet);
        let points = el.face_rule(local_face(self.topo, tet, &b.nodes)?, n);
        Ok(FaceRule { tet, el, unknowns, points })
    }

    /// The matrices' pattern: the element couplings and the `extra` groups of unknowns (the
    /// lumped ports' dense blocks).
    pub(super) fn pattern(&self, extra: impl Iterator<Item = Vec<usize>>) -> Csr {
        let mut groups: Vec<Vec<usize>> =
            (0..self.mesh.tets.len()).map(|t| self.dofs.of_tet(self.topo, t).filter(|&u| u != NONE).map(|u| u as usize).collect()).collect();
        groups.extend(extra);
        Csr::from_groups(self.dofs.unknowns, groups.iter().map(|g| g.as_slice()))
    }

    /// K, M and each dispersive material's M_d over the tetrahedra `keep` admits (those of the
    /// layer are assembled with each frequency).
    pub(super) fn volume_matrices(&self, pattern: &Csr, media: &Media, keep: impl Fn(usize) -> bool) -> Result<VolumeMatrices> {
        let mut kk = pattern.zeroed();
        let mut mm = kk.clone();
        let mut md: Vec<Csr> = media.dispersive_materials().iter().map(|_| kk.clone()).collect();
        let mut cache = ElementCache::default();
        for t in (0..self.mesh.tets.len()).filter(|t| keep(*t)) {
            let (s, m) = cache.matrices(self, t)?;
            let unknowns = self.unknowns(t);
            let n = unknowns.len();
            let (eps, mu, dispersive) = (media.eps(t), media.mu(t), media.dispersive_of(t));
            for Coupling { i, j, row, col } in couplings(&unknowns) {
                kk.add(row, col, per_mu(s[i * n + j], mu));
                let (target, e) = match dispersive.and_then(|d| md.get_mut(d)) {
                    Some(d) => (d, C::new(1.0, 0.0)),
                    None => (&mut mm, eps),
                };
                target.add(row, col, e * m[i * n + j]);
            }
        }
        Ok(VolumeMatrices { stiffness: kk.val, mass: mm.val, dispersive: md.iter().map(real_parts).collect() })
    }

    /// The walls' tangential mass matrix ∫ N_i,t · N_j,t dS over the boundary faces `walls`.
    pub(super) fn wall_mass(&self, pattern: &Csr, walls: &[usize]) -> Result<Vec<f64>> {
        let mut ww = pattern.zeroed();
        for b in walls.iter().filter_map(|&i| self.mesh.boundary.get(i)) {
            let face = self.face_rule(b, rule_points(self.order()))?;
            for (l, wa, normal) in face.points {
                let t: Vec<Vec3> = face.el.values(l).into_iter().map(|v| v.tangential(normal)).collect();
                for Coupling { i, j, row, col } in couplings(&face.unknowns) {
                    ww.add(row, col, C::new(wa * t[i].dot(t[j]), 0.0));
                }
            }
        }
        Ok(real_parts(&ww))
    }
}

fn real_parts(a: &Csr) -> Vec<f64> {
    a.val.iter().map(|v| v.re).collect()
}

/// A frequency's linear system: the matrix with its border of port modes (the last `border`
/// unknowns), the right-hand side of each port's excitation, and the ports' terms.
pub(crate) struct System<'a> {
    pub a: Csr,
    pub rhs: Vec<Vec<C>>,
    pub terms: Vec<Vec<PortTerm<'a>>>,
    pub border: usize,
}

/// The rows of `a` bordered by one unknown per (column v, factor f): each row of `a` followed
/// by its entries s v_i of the border's columns (column n + k for the k-th), with
/// s = √(|f| ‖a‖) so the border's pivots are of the matrix's size (unscaled, −1/f can be far
/// below the factorization's static-pivot threshold); and each border's s. The x part of the
/// solution is the same whatever s. The border's own rows are the caller's to add.
pub(super) fn border_columns<V: IntoIterator<Item = (usize, C)>>(a: &Csr, borders: impl IntoIterator<Item = (V, C)>) -> Result<(Csr, Vec<f64>)> {
    let n = a.n;
    let norm = a.max_abs();
    let mut scale = Vec::new();
    let mut extra: Vec<Vec<(usize, C)>> = vec![Vec::new(); n];
    for (k, (v, f)) in borders.into_iter().enumerate() {
        let s = (f.norm() * norm).sqrt();
        for (i, vi) in v {
            extra.get_mut(i).ok_or_else(|| invalid("port", "a mode's vector refers to a missing unknown"))?.push((n + k, vi * s));
        }
        if !(f.norm() > 0.0 && s.is_finite()) {
            return Err(invalid("port", "a mode with no coupling"));
        }
        scale.push(s);
    }
    let mut out = Csr { n: n + scale.len(), ptr: Vec::with_capacity(n + scale.len() + 1), col: Vec::new(), val: Vec::new() };
    out.ptr.push(0);
    for (i, row) in extra.iter().enumerate() {
        let (lo, hi) = (a.ptr[i], a.ptr[i + 1]);
        out.col.extend_from_slice(&a.col[lo..hi]);
        out.val.extend_from_slice(&a.val[lo..hi]);
        out.col.extend(row.iter().map(|(j, _)| *j));
        out.val.extend(row.iter().map(|(_, v)| *v));
        out.ptr.push(out.col.len());
    }
    Ok((out, scale))
}

/// `a` bordered by one unknown per (v, f) ([`border_columns`]): its column and row hold s v
/// (the matrix stays complex symmetric) and its diagonal −s²/f.
fn bordered(a: &Csr, borders: &[(&[(usize, C)], C)]) -> Result<Csr> {
    let n = a.n;
    let columns = borders.iter().map(|(v, f)| (v.iter().copied(), *f));
    let (mut out, scale) = border_columns(a, columns)?;
    for (k, ((v, f), s)) in borders.iter().zip(scale).enumerate() {
        out.col.extend(v.iter().map(|(i, _)| *i));
        out.val.extend(v.iter().map(|(_, x)| x * s));
        out.col.push(n + k);
        out.val.push(-(s * s) / f);
        out.ptr.push(out.col.len());
    }
    Ok(out)
}

impl Driven {
    /// k₀² εr,d(ω) of each dispersive material at `frequency`, in their order.
    pub(crate) fn dispersive_scales(&self, frequency: f64) -> Vec<C> {
        let k0 = wavenumber(frequency);
        self.media.dispersive_materials().iter().map(|m| m.eps_at(frequency) * (k0 * k0)).collect()
    }

    /// The system at a frequency: the matrix bordered by the ports' modes, each excitation
    /// ([`Driven::excitations`]: a port's first mode, each conductor's of a terminal port), and
    /// the modes' terms.
    pub(crate) fn system(&self, frequency: f64) -> Result<System<'_>> {
        let a = self.unbordered(frequency)?;
        let terms = self.checked_terms(frequency)?;
        // Each mode's factor·v vᵀ would couple every unknown on the port with every other; a
        // border unknown y_m = factor·vᵀx per mode keeps the matrix sparse:
        // [A V; Vᵀ −F⁻¹] [x; y] = [b; 0] eliminates to (A + V F Vᵀ) x = b.
        let borders: Vec<(&[(usize, C)], C)> = terms.iter().flatten().map(|t| (&*t.v, t.factor)).collect();
        let border = borders.len();
        let a = bordered(&a, &borders)?;
        let n = self.dofs.unknowns;
        let rhs = self
            .excitations()
            .into_iter()
            .map(|(p, m)| {
                let mut b = vec![C::new(0.0, 0.0); n + border];
                if let Some(t) = terms.get(p).and_then(|l| l.get(m)) {
                    for (i, bi) in t.excitation() {
                        b[i] = bi;
                    }
                }
                b
            })
            .collect();
        Ok(System { a, rhs, terms, border })
    }

    /// The matrix at a frequency without the ports' terms: K − k₀²M, the dispersive materials,
    /// the layer, the lumped ports and the walls' losses.
    pub(crate) fn unbordered(&self, frequency: f64) -> Result<Csr> {
        let k0 = wavenumber(frequency);
        let mut a = self.pattern.clone();
        for (x, (s, m)) in a.val.iter_mut().zip(self.stiffness.iter().zip(&self.mass)) {
            *x = *s - m * (k0 * k0);
        }
        for (c, md) in self.dispersive_scales(frequency).into_iter().zip(&self.dispersive_mass) {
            for (x, m) in a.val.iter_mut().zip(md) {
                *x -= c * *m;
            }
        }
        self.add_pml(&mut a, k0)?;
        // Lumped ports: the current I = (V_s + cᵀx)/R through each gap adds (jωμ₀/R) c cᵀ.
        for l in &self.lumped {
            let f = l.factor(frequency);
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
        if let Some(w) = &self.wall {
            let c = wall_factor(frequency, w.sigma);
            for (x, wij) in a.val.iter_mut().zip(&w.mass) {
                *x += c * *wij;
            }
        }
        Ok(a)
    }
}

/// The local corners (in the sorted order) of a tetrahedron's boundary face.
pub(super) fn local_face(topo: &Topology, t: usize, nodes: &[u32; 3]) -> Result<[usize; 3]> {
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
    /// Tetrahedron `t`'s unit stiffness and mass matrices.
    fn matrices(&mut self, space: &Space, t: usize) -> Result<(Vec<f64>, Vec<f64>)> {
        let mesh = space.mesh;
        let s = space.topo.sorted.get(t).ok_or_else(|| invalid("mesh", format!("no tetrahedron {t}")))?;
        // Curved tetrahedra are their own.
        if LOCAL_EDGES.iter().any(|[i, j]| mesh.midpoint(s[*i], s[*j]).is_some()) {
            let el = space.element(t)?;
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
        let el = Element::new(space.order(), c)?;
        let m = (el.stiffness(1.0), el.mass(1.0));
        if self.map.len() < 100_000 {
            self.map.insert(key, m.clone());
        }
        Ok(m)
    }
}

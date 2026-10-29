//! The modes of a waveguide port of any cross-section, solved on its triangles: conducting walls
//! round it and on any holes in it (inner conductors), materials per triangle, so coaxial lines,
//! ridged and partly filled guides and other shapes without closed forms get their modes, hybrid
//! ones (with an axial E) included.
//!
//! **Formulation** (J.-F. Lee, D.-K. Sun, Z. J. Cendes, IEEE Trans. MTT 39(8):1262–1271, 1991;
//! J.-M. Jin, *The Finite Element Method in Electromagnetics*, 3rd ed., Wiley 2014, §8.4). A mode
//! E = (e_t + ŝ e_z) e^{−jβs} travelling along ŝ, with e_z = jβφ, satisfies for every test pair
//! (f, ξ)
//!
//! ∫ μr⁻¹ (∇×e)(∇×f) − k₀² εr e·f = −β² ∫ μr⁻¹ (e + ∇φ)·(f + ∇ξ) − k₀² εr φ ξ,
//!
//! the generalised eigenproblem A x = −β² B x on edge functions for e and nodal ones for φ
//! ([`emcraft_fem::tri`]), tangential e and φ zero on the walls. Its eigenvalues λ = −β² lie
//! above −k₀² max(εr μr); with the shift σ just below that bound, subspace iteration on
//! (A − σB)⁻¹B finds them nearest σ first — the modes with the largest β², propagating then
//! evanescent, in the order the port needs them. B is indefinite, so each projected problem is a
//! small nonsymmetric eigenproblem ([`crate::dense`]).
//!
//! **Fields.** H_t = ŝ × (e_t + ∇φ) β/(ωμ). With g = e_t + ∇φ, the non-conjugate power of a mode
//! is ∫ (e × h) · ŝ dS = (β/ωμ₀) D, D = ∫ μr⁻¹ e · g dS, by which [`crate::driven`] projects the
//! port's field onto it.

use emcraft_fem::Order;
use emcraft_fem::tri::{TRI_EDGES, Triangle};
use emcraft_model::{Point3, Vec3};
use num_complex::Complex64 as C;

use crate::driven::{Result, Slot, SolveError, invalid, present};
use crate::ldlt::Ldlt;
use crate::sparse::Csr;

mod coupled;
mod lossy;

pub use lossy::LossyPortMode;

/// Iteration cap of the subspace iteration.
const MAX_ITERATIONS: usize = 300;
/// The most unknowns a cross-section may have.
pub const MAX_UNKNOWNS: usize = 400_000;

/// A port's cross-section in triangles with its matrices assembled up to the frequency.
#[derive(Clone, Debug)]
pub struct CrossSection {
    pub order: Order,
    pub nodes: Vec<Point3>,
    /// Corners ascending by node.
    pub tris: Vec<[u32; 3]>,
    eps: Vec<f64>,
    mu: Vec<f64>,
    tri_edges: Vec<[u32; 3]>,
    /// Each triangle's moved edge midpoints (in its local edge order).
    mids: Vec<[Option<Point3>; 3]>,
    /// The edges on conductors (the rim and any inside), by nodes ascending.
    walls: Vec<[u32; 2]>,
    /// Unknown of each transverse slot (edge functions, then triangle functions), then of each
    /// scalar slot (nodes, then edges); none on the walls.
    t_unknown: Vec<Slot>,
    z_unknown: Vec<Slot>,
    edges: usize,
    pub unknowns: usize,
    /// Transverse unknowns come first, scalar ones after: 0..transverse, transverse..unknowns.
    transverse: usize,
    /// A point per unknown, for the ordering.
    points: Vec<Point3>,
    /// ∫ μ⁻¹ curl curl, ∫ εr e·f, the k-free part of B and ∫ εr φ ξ, on one pattern.
    pattern: Csr,
    curl: Vec<f64>,
    mass: Vec<f64>,
    b: Vec<f64>,
    t: Vec<f64>,
    max_index2: f64,
}

/// One mode at a frequency.
#[derive(Clone, Debug, PartialEq)]
pub struct PortMode {
    /// β² (rad²/m²): positive for a propagating mode, negative for an evanescent one.
    pub beta2: f64,
    /// The coefficients: transverse unknowns, then scalar ones, scaled so ∫ |e_t|² dS = 1.
    pub x: Vec<f64>,
    /// D = ∫ μr⁻¹ e_t · (e_t + ∇φ) dS.
    pub d: f64,
}

impl PortMode {
    /// β, or −jα for an evanescent mode (decaying as e^{−αs}).
    pub fn beta(&self) -> C {
        if self.beta2 >= 0.0 { C::new(self.beta2.sqrt(), 0.0) } else { C::new(0.0, -(-self.beta2).sqrt()) }
    }
}

/// The edges of a cross-section with conditions of their own, each by its nodes: conductors
/// inside it, boundary edges on a magnetic wall (`free`) and edges whose midpoints lie on a
/// curved wall (ascending nodes → the midpoint).
#[derive(Clone, Copy, Debug)]
pub struct Walls<'a> {
    pub conductors: &'a [[u32; 2]],
    pub free: &'a [[u32; 2]],
    pub curved: &'a std::collections::BTreeMap<[u32; 2], Point3>,
}

impl CrossSection {
    /// A cross-section of triangles on `nodes` (their z ignored) with εr and μr per triangle;
    /// its boundary edges and the `conductors` edges inside it are conducting walls. Edges in
    /// `curved` (by their nodes, ascending) have their midpoints moved there, onto a curved wall:
    /// their triangles are curved ([`Triangle::curved`]).
    pub fn new(
        order: Order,
        nodes: Vec<Point3>,
        tris: Vec<[u32; 3]>,
        eps: Vec<f64>,
        mu: Vec<f64>,
        conductors: &[[u32; 2]],
        curved: &std::collections::BTreeMap<[u32; 2], Point3>,
    ) -> Result<CrossSection> {
        CrossSection::with_free_rim(order, nodes, tris, eps, mu, Walls { conductors, free: &[], curved })
    }

    /// As [`CrossSection::new`] with the conductors and curved edges of `walls`, but its boundary
    /// edges in `walls.free` are a magnetic wall — a symmetry plane's — rather than a conductor:
    /// their tangential e and their φ are left free, the natural condition (tangential H = 0)
    /// holding weakly.
    pub fn with_free_rim(
        order: Order,
        nodes: Vec<Point3>,
        tris: Vec<[u32; 3]>,
        eps: Vec<f64>,
        mu: Vec<f64>,
        walls: Walls<'_>,
    ) -> Result<CrossSection> {
        let Walls { conductors, free, curved } = walls;
        if tris.is_empty() || eps.len() != tris.len() || mu.len() != tris.len() {
            return Err(invalid("port cross-section", "needs triangles, each with its materials"));
        }
        if !eps.iter().chain(&mu).all(|v| v.is_finite() && *v > 0.0) {
            return Err(invalid("port cross-section", "materials must be finite and positive (lossless)"));
        }
        let nn = nodes.len();
        if tris.iter().flatten().any(|&v| v as usize >= nn) || !nodes.iter().all(|p| p.is_finite()) {
            return Err(invalid("port cross-section", "a triangle refers to a missing node, or a node is not finite"));
        }
        let tris: Vec<[u32; 3]> = tris
            .into_iter()
            .map(|mut t| {
                t.sort_unstable();
                t
            })
            .collect();
        // Edges numbered in ascending order of their nodes.
        let mut edge_set: std::collections::BTreeMap<[u32; 2], (usize, u32)> = std::collections::BTreeMap::new();
        for t in &tris {
            for [i, j] in TRI_EDGES {
                edge_set.entry([t[i], t[j]]).or_insert((0, 0)).1 += 1;
            }
        }
        for (k, v) in edge_set.values_mut().enumerate() {
            v.0 = k;
        }
        let edge_list: Vec<[u32; 2]> = edge_set.keys().copied().collect();
        let tri_edges: Vec<[u32; 3]> = tris.iter().map(|t| TRI_EDGES.map(|[i, j]| edge_set.get(&[t[i], t[j]]).map_or(0, |e| e.0 as u32))).collect();
        let mut wall_edge: Vec<bool> = edge_set.values().map(|&(_, uses)| uses == 1).collect();
        for c in free {
            let key = [c[0].min(c[1]), c[0].max(c[1])];
            if let Some(&(k, _)) = edge_set.get(&key) {
                wall_edge[k] = false;
            }
        }
        for c in conductors {
            let key = [c[0].min(c[1]), c[0].max(c[1])];
            if let Some(&(k, _)) = edge_set.get(&key) {
                wall_edge[k] = true;
            }
        }
        let mut wall_node = vec![false; nn];
        for (e, _) in edge_list.iter().zip(&wall_edge).filter(|(_, w)| **w) {
            wall_node[e[0] as usize] = true;
            wall_node[e[1] as usize] = true;
        }
        let ne = edge_list.len();
        let (pe, pf) = (order.per_edge(), order.per_face());
        let mid = |e: usize| edge_list.get(e).map_or(Point3::ORIGIN, |[a, b]| nodes[*a as usize].midpoint(nodes[*b as usize]));
        let centre = |t: usize| Point3::centroid(tris[t].map(|v| nodes[v as usize])).unwrap_or(Point3::ORIGIN);
        let mut points = Vec::new();
        let mut number = |free: bool, p: Point3| {
            if free {
                points.push(p);
                Slot::new(points.len() - 1)
            } else {
                Slot::NONE
            }
        };
        let mut t_unknown: Vec<Slot> = (0..ne * pe).map(|s| number(!wall_edge[s / pe], mid(s / pe))).collect();
        t_unknown.extend((0..tris.len() * pf).map(|s| number(true, centre(s / pf))));
        let scalar_slots = nn + if order == Order::Two { ne } else { 0 };
        let z_unknown: Vec<Slot> =
            (0..scalar_slots).map(|s| if s < nn { number(!wall_node[s], nodes[s]) } else { number(!wall_edge[s - nn], mid(s - nn)) }).collect();
        let transverse = t_unknown.iter().filter(|u| u.get().is_some()).count();
        let unknowns = points.len();
        if unknowns == 0 {
            return Err(invalid("port cross-section", "has no unknowns (every edge is on a wall)"));
        }
        if unknowns > MAX_UNKNOWNS {
            return Err(SolveError::TooLarge { unknowns, limit: MAX_UNKNOWNS });
        }
        let mids = tris.iter().map(|t| TRI_EDGES.map(|[i, j]| curved.get(&[t[i], t[j]]).copied())).collect();
        let walls: Vec<[u32; 2]> = edge_list.iter().zip(&wall_edge).filter(|(_, w)| **w).map(|(e, _)| *e).collect();
        let mut cs = CrossSection {
            order,
            nodes,
            tris,
            eps,
            mu,
            tri_edges,
            mids,
            walls,
            t_unknown,
            z_unknown,
            edges: ne,
            unknowns,
            transverse,
            points,
            pattern: Csr::default(),
            curl: Vec::new(),
            mass: Vec::new(),
            b: Vec::new(),
            t: Vec::new(),
            max_index2: 1.0,
        };
        cs.max_index2 = cs.eps.iter().zip(&cs.mu).map(|(e, m)| e * m).fold(1.0, f64::max);
        cs.assemble()?;
        Ok(cs)
    }

    fn triangle(&self, t: usize) -> Result<Triangle> {
        let corners = self.tris[t].map(|v| self.nodes[v as usize]);
        let mids = self.mids.get(t).copied().unwrap_or([None; 3]);
        Triangle::new(self.order, corners)
            .and_then(|tri| tri.curved(mids))
            .ok_or_else(|| invalid("port cross-section", format!("triangle {t} is degenerate or folds over")))
    }

    /// The unknowns of a triangle's transverse and scalar functions (none on walls).
    fn unknowns_of(&self, t: usize) -> (Vec<Slot>, Vec<Slot>) {
        let get = |list: &[Slot], s: usize| list.get(s).copied().unwrap_or(Slot::NONE);
        let te = self.tri_edges[t];
        let (pe, pf) = (self.order.per_edge(), self.order.per_face());
        let vt: Vec<Slot> = (0..pe)
            .flat_map(|j| te.map(|e| e as usize * pe + j))
            .chain((0..pf).map(|j| self.edges * pe + t * pf + j))
            .map(|s| get(&self.t_unknown, s))
            .collect();
        let nn = self.nodes.len();
        let corners = self.tris[t].map(|v| v as usize);
        let vz: Vec<Slot> = match self.order {
            Order::One => corners.iter().map(|&s| get(&self.z_unknown, s)).collect(),
            Order::Two => corners.iter().copied().chain(te.map(|e| nn + e as usize)).map(|s| get(&self.z_unknown, s)).collect(),
        };
        (vt, vz)
    }

    fn assemble(&mut self) -> Result<()> {
        let n = self.unknowns;
        let groups: Vec<Vec<usize>> = (0..self.tris.len())
            .map(|t| {
                let (vt, vz) = self.unknowns_of(t);
                vt.into_iter().chain(vz).filter_map(Slot::get).collect()
            })
            .collect();
        self.pattern = Csr::from_groups(n, groups.iter().map(|g| g.as_slice()));
        let mats = self.assemble_with(&|t| C::new(self.eps[t], 0.0), &|t| C::new(1.0 / self.mu[t], 0.0))?;
        let [curl, mass, b, tz] = mats.map(|m| m.into_iter().map(|c| c.re).collect::<Vec<f64>>());
        (self.curl, self.mass, self.b, self.t) = (curl, mass, b, tz);
        Ok(())
    }

    /// ∫ μr⁻¹ curl curl, ∫ εr e·f, the k-free part of B and ∫ εr φ ξ on the pattern, with εr
    /// and μr⁻¹ of each triangle from `eps` and `inv_mu` (complex for lossy materials).
    fn assemble_with(&self, eps: &dyn Fn(usize) -> C, inv_mu: &dyn Fn(usize) -> C) -> Result<[Vec<C>; 4]> {
        let mut mats: [Csr; 4] = std::array::from_fn(|_| self.pattern.zeroed());
        for t in 0..self.tris.len() {
            let tri = self.triangle(t)?;
            let (vt, vz) = self.unknowns_of(t);
            let (eps, inv_mu) = (eps(t), inv_mu(t));
            for (l, w) in tri.rule() {
                let (v, c, s, g) = (tri.values(l), tri.curls(l), tri.scalars(l), tri.scalar_gradients(l));
                for (i, ui) in present(&vt) {
                    for (j, uj) in present(&vt) {
                        mats[0].add(ui, uj, w * inv_mu * c[i] * c[j]);
                        mats[1].add(ui, uj, w * eps * v[i].dot(v[j]));
                        mats[2].add(ui, uj, w * inv_mu * v[i].dot(v[j]));
                    }
                    for (k, uk) in present(&vz) {
                        let x = w * inv_mu * v[i].dot(g[k]);
                        mats[2].add(ui, uk, x);
                        mats[2].add(uk, ui, x);
                    }
                }
                for (k, uk) in present(&vz) {
                    for (m, um) in present(&vz) {
                        mats[2].add(uk, um, w * inv_mu * g[k].dot(g[m]));
                        mats[3].add(uk, um, w * eps * s[k] * s[m]);
                    }
                }
            }
        }
        Ok(mats.map(|m| m.val))
    }

    /// y = M x for values `val` on the pattern.
    fn mul(&self, val: impl Fn(usize) -> f64, x: &[f64]) -> Vec<f64> {
        let p = &self.pattern;
        (0..p.n).map(|i| p.row(i).map(|k| val(k) * x[p.col[k]]).sum()).collect()
    }

    /// The `count` modes with the largest β² at free-space wavenumber `k0` (rad/m), β² descending.
    pub fn modes(&self, k0: f64, count: usize) -> Result<Vec<PortMode>> {
        if !(k0.is_finite() && k0 > 0.0) {
            return Err(invalid("port modes", format!("the wavenumber must be finite and positive, got {k0}")));
        }
        let n = self.unknowns;
        let count = count.clamp(1, n);
        let k2 = k0 * k0;
        let sigma = -1.05 * k2 * self.max_index2;
        // A = curl − k² mass (transverse block); B = b − k² t.
        let a_val = |k: usize| self.curl[k] - k2 * self.mass[k];
        let b_val = |k: usize| self.b[k] - k2 * self.t[k];
        let mut shifted = self.pattern.clone();
        for (k, v) in shifted.val.iter_mut().enumerate() {
            *v = C::new(a_val(k) - sigma * b_val(k), 0.0);
        }
        let factor = Ldlt::factor(&shifted, &self.points)?;
        let project = self.constraint(k2)?;
        let q = (count + count.max(6)).min(n);
        let mut rand = crate::subspace::uniform(0x9e37_79b9_7f4a_7c15);
        let mut x: Vec<Vec<f64>> = (0..q).map(|_| (0..n).map(|_| rand()).collect()).collect();
        for v in &mut x {
            project(v)?;
        }
        let mut previous: Vec<f64> = Vec::new();
        let mut result: Option<(Vec<f64>, Vec<Vec<f64>>)> = None;
        let mut complex: Option<(f64, f64)> = None;
        for _ in 0..MAX_ITERATIONS {
            // W = (A − σB)⁻¹ B X, orthonormalised.
            let mut w: Vec<Vec<C>> = x.iter().map(|v| self.mul(b_val, v).into_iter().map(|r| C::new(r, 0.0)).collect()).collect();
            factor.solve_many(&mut w)?;
            let mut w: Vec<Vec<f64>> = w.into_iter().map(|v| v.into_iter().map(|c| c.re).collect()).collect();
            for v in &mut w {
                project(v)?;
            }
            let qb = orthonormal(w).ok_or_else(|| invalid("port modes", "the subspace lost its independence"))?;
            // The projections of A − σB and B; the Ritz values are the eigenvalues θ of
            // G⁻¹ Bq, λ = σ + 1/θ.
            let kq: Vec<Vec<f64>> = qb.iter().map(|v| self.mul(|k| a_val(k) - sigma * b_val(k), v)).collect();
            let bq: Vec<Vec<f64>> = qb.iter().map(|v| self.mul(b_val, v)).collect();
            let m = qb.len();
            let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(p, r)| p * r).sum::<f64>();
            let g: Vec<f64> = (0..m * m).map(|ij| dot(&qb[ij / m], &kq[ij % m])).collect();
            let bm: Vec<f64> = (0..m * m).map(|ij| dot(&qb[ij / m], &bq[ij % m])).collect();
            let scale = g.iter().fold(0.0f64, |s, v| s.max(v.abs()));
            let lu = crate::dense::Lu::new(g, m, 1e-14 * scale).ok_or_else(|| invalid("port modes", "a singular projection"))?;
            let cols: Vec<Vec<f64>> = (0..m).map(|j| lu.solve(&(0..m).map(|i| bm[i * m + j]).collect::<Vec<_>>())).collect();
            let op: Vec<f64> = (0..m * m).map(|ij| cols[ij % m][ij / m]).collect();
            let mut ritz = crate::dense::eigen(&op, m).ok_or_else(|| invalid("port modes", "the projected eigenproblem did not converge"))?;
            ritz.sort_by(|p, r| r.re.total_cmp(&p.re));
            // Complex Ritz pairs come and go while a degenerate pair of modes settles; the
            // subspace keeps its size with the basis vectors.
            complex = ritz.iter().take(count).find(|e| e.im != 0.0).map(|e| (e.re, e.im.abs()));
            let lambdas: Vec<f64> =
                ritz.iter().filter(|e| e.im == 0.0).map(|e| if e.re != 0.0 { sigma + 1.0 / e.re } else { f64::INFINITY }).collect();
            let vectors: Vec<Vec<f64>> =
                ritz.iter().filter(|e| e.im == 0.0).map(|e| (0..n).map(|r| (0..m).map(|k| qb[k][r] * e.vector[k]).sum()).collect()).collect();
            if complex.is_some() {
                x = qb;
                previous.clear();
                continue;
            }
            let wanted: Vec<f64> = lambdas.iter().take(count).copied().collect();
            let done = previous.len() == count && wanted.iter().zip(&previous).all(|(a, b)| (a - b).abs() <= 1e-11 * k2 * self.max_index2);
            previous = wanted;
            if done {
                result = Some((previous.clone(), vectors.into_iter().take(count).collect()));
                break;
            }
            x = if vectors.len() == m { vectors } else { qb };
        }
        if let (None, Some((re, im))) = (&result, complex) {
            return Err(invalid(
                "port modes",
                format!("not supported yet: complex modes (θ = {re} ± {im}j); refine the mesh or change the frequency"),
            ));
        }
        let (lambdas, vectors) = result.ok_or_else(|| invalid("port modes", format!("no convergence in {MAX_ITERATIONS} iterations")))?;
        lambdas.into_iter().zip(vectors).map(|(l, v)| self.finish(-l, v)).collect()
    }

    /// The projection onto the modes' constraint B_zt e + B_zz φ = 0 (the scalar rows of the
    /// eigenproblem, which every mode with β ≠ 0 satisfies): φ ← −B_zz⁻¹ B_zt e. Without it, every
    /// (0, φ) is an eigenvector with β = 0, a family that crowds out the evanescent modes;
    /// (A − σB)⁻¹B keeps vectors on the constraint, so the projection only removes rounding.
    fn constraint(&self, k2: f64) -> Result<impl Fn(&mut Vec<f64>) -> Result<()> + '_> {
        let (nt, n) = (self.transverse, self.unknowns);
        let p = &self.pattern;
        let nz = n - nt;
        let groups: Vec<Vec<usize>> =
            (nt..n).map(|i| p.row(i).map(|k| p.col[k]).filter(|&j| j >= nt).map(|j| j - nt).chain([i - nt]).collect()).collect();
        let mut zz = Csr::from_groups(nz, groups.iter().map(|g| g.as_slice()));
        for i in nt..n {
            for k in p.row(i).filter(|&k| p.col[k] >= nt) {
                zz.add(i - nt, p.col[k] - nt, C::new(self.b[k] - k2 * self.t[k], 0.0));
            }
        }
        let factor = if nz > 0 { Some(Ldlt::factor(&zz, &self.points[nt..])?) } else { None };
        Ok(move |v: &mut Vec<f64>| -> Result<()> {
            let Some(f) = &factor else { return Ok(()) };
            // r = −B_zt e.
            let mut r: Vec<C> =
                (nt..n).map(|i| C::new(-p.row(i).filter(|&k| p.col[k] < nt).map(|k| self.b[k] * v[p.col[k]]).sum::<f64>(), 0.0)).collect();
            f.solve(&mut r)?;
            v[nt..].iter_mut().zip(&r).for_each(|(x, c)| *x = c.re);
            Ok(())
        })
    }

    /// Scales a mode to ∫ |e_t|² = 1, signs it, and computes D.
    fn finish(&self, beta2: f64, mut x: Vec<f64>) -> Result<PortMode> {
        let (mut e2, mut d, mut sy, mut sx, mut sr, mut area) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let centre = Point3::centroid(self.nodes.iter().copied()).unwrap_or(Point3::ORIGIN);
        for t in 0..self.tris.len() {
            let tri = self.triangle(t)?;
            area += tri.area;
            for (l, w) in tri.rule() {
                let (e, g) = self.at(&tri, t, &x, l);
                e2 += w * e.norm_sqr();
                d += w * e.dot(g) / self.mu[t];
                sy += w * e.y;
                sx += w * e.x;
                sr += w * e.dot(tri.point(l) - centre);
            }
        }
        if !(e2 > 0.0 && e2.is_finite()) {
            return Err(invalid("port modes", "a mode without a transverse field"));
        }
        let len = area.sqrt();
        // The sign: ∫ e_y > 0, else ∫ e_x > 0, else outward from the centre (a coaxial TEM).
        let (unit, tol) = (e2.sqrt(), 1e-6 * len);
        let sign = [sy, sx, sr / len].into_iter().find(|s| s.abs() > tol * unit).map_or(1.0, f64::signum);
        let scale = sign / unit;
        x.iter_mut().for_each(|c| *c *= scale);
        Ok(PortMode { beta2, x, d: d / e2 })
    }

    /// e_t and g = e_t + ∇φ at barycentric `l` of triangle `t` for coefficients `x`.
    fn at(&self, tri: &Triangle, t: usize, x: &[f64], l: [f64; 3]) -> (Vec3, Vec3) {
        let (vt, vz) = self.unknowns_of(t);
        let coef = |u: Slot| u.get().map_or(0.0, |u| x.get(u).copied().unwrap_or(0.0));
        let e = tri.values(l).into_iter().zip(&vt).fold(Vec3::ZERO, |s, (v, u)| s + v * coef(*u));
        let grad = tri.scalar_gradients(l).into_iter().zip(&vz).fold(Vec3::ZERO, |s, (g, u)| s + g * coef(*u));
        (e, e + grad)
    }

    /// e_t and g = e_t + ∇φ of `mode` at a point of triangle `t` (its z ignored).
    pub fn fields(&self, mode: &PortMode, t: usize, p: Point3) -> Result<(Vec3, Vec3)> {
        if t >= self.tris.len() {
            return Err(invalid("port modes", format!("no triangle {t}")));
        }
        let tri = self.triangle(t)?;
        Ok(self.at(&tri, t, &mode.x, tri.barycentric(p)))
    }

    /// The triangle holding point `p` (its z ignored; straight sides), if any.
    pub fn locate(&self, p: Point3) -> Option<usize> {
        self.tris.iter().position(|t| {
            let [a, b, c] = t.map(|v| self.nodes.get(v as usize).copied().unwrap_or(Point3::ORIGIN));
            let d = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
            if d == 0.0 {
                return false;
            }
            let l1 = ((p.x - a.x) * (c.y - a.y) - (c.x - a.x) * (p.y - a.y)) / d;
            let l2 = ((b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y)) / d;
            let tol = -1e-9;
            l1 >= tol && l2 >= tol && 1.0 - l1 - l2 >= tol
        })
    }

    /// The conductors other than the ground: each a set of nodes, ascending. Conductors are the
    /// connected sets of wall edges; the ground is the one holding the leftmost node (the rim).
    pub fn conductors(&self) -> Vec<Vec<u32>> {
        let n = self.nodes.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn root(parent: &mut [usize], mut v: usize) -> usize {
            while parent[v] != v {
                parent[v] = parent[parent[v]];
                v = parent[v];
            }
            v
        }
        for [a, b] in &self.walls {
            let (ra, rb) = (root(&mut parent, *a as usize), root(&mut parent, *b as usize));
            if ra != rb {
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
        let mut on_wall: Vec<u32> = self.walls.iter().flatten().copied().collect();
        on_wall.sort_unstable();
        on_wall.dedup();
        let Some(&leftmost) = on_wall.iter().min_by(|a, b| self.nodes[**a as usize].x.total_cmp(&self.nodes[**b as usize].x).then(a.cmp(b))) else {
            return Vec::new();
        };
        let ground = root(&mut parent, leftmost as usize);
        let mut groups: std::collections::BTreeMap<usize, Vec<u32>> = std::collections::BTreeMap::new();
        for v in on_wall {
            let r = root(&mut parent, v as usize);
            if r != ground {
                groups.entry(r).or_default().push(v);
            }
        }
        groups.into_values().collect()
    }

    /// The current I = ∮ H · dl round each conductor other than the ground (A, for the mode at
    /// unit amplitude, along +ŝ), and the power P = ½ Re ∫ (e × h*) · ŝ dS (W) the mode carries.
    ///
    /// The contour integral is taken in its weak form, which needs no trace of H on element
    /// edges (its normal part jumps there): with w = 1 on the conductor and 0 a triangle away
    /// (the sum of its nodes' hat functions), Stokes' theorem and ∇ × H = jωεE give
    /// I = −∫ ∇w · H̃ dA − ∫ w jωε E_z dA, H̃ = μr⁻¹ g β/(ωμ₀) (H = ŝ × H̃), E_z = jβφ.
    pub fn currents(&self, mode: &PortMode, k0: f64) -> Result<(Vec<C>, f64)> {
        let eta0 = emcraft_model::units::MU0 * emcraft_model::units::C0;
        let beta = mode.beta();
        // β/(ωμ₀) = β/(k₀η₀); jωε₀ · jβ = −k₀β/η₀.
        let scale = beta / (k0 * eta0);
        let axial = -(k0 / eta0) * beta;
        let power = 0.5 * (scale * mode.d).re;
        let currents = self
            .conductors()
            .iter()
            .map(|nodes| {
                let mut sum = C::new(0.0, 0.0);
                for t in (0..self.tris.len()).filter(|&t| self.tris[t].iter().any(|v| nodes.binary_search(v).is_ok())) {
                    let tri = self.triangle(t)?;
                    let on: [bool; 3] = self.tris[t].map(|v| nodes.binary_search(&v).is_ok());
                    let (_, vz) = self.unknowns_of(t);
                    for (l, wt) in tri.rule() {
                        let (_, g) = self.at(&tri, t, &mode.x, l);
                        let grads = tri.scalar_gradients(l);
                        let w: f64 = (0..3).filter(|k| on[*k]).map(|k| l[k]).sum();
                        let dw = (0..3).filter(|k| on[*k]).fold(Vec3::ZERO, |a, k| a + grads[k]);
                        let phi: f64 = tri
                            .scalars(l)
                            .into_iter()
                            .zip(&vz)
                            .map(|(f, u)| u.get().map_or(0.0, |u| f * mode.x.get(u).copied().unwrap_or(0.0)))
                            .sum();
                        sum += wt * (-scale * (dw.dot(g) / self.mu[t]) - axial * (self.eps[t] * w * phi));
                    }
                }
                Ok(sum)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((currents, power))
    }

    /// The mode's characteristic impedance by power and current, Z = 2P/|I|² (Ω), when the
    /// cross-section has exactly one conductor besides the ground; `None` otherwise.
    pub fn impedance(&self, mode: &PortMode, k0: f64) -> Result<Option<f64>> {
        if mode.beta2 <= 0.0 {
            return Ok(None);
        }
        let (currents, power) = self.currents(mode, k0)?;
        Ok(match currents[..] {
            [i] if i.norm() > 0.0 => Some(2.0 * power / i.norm_sqr()),
            _ => None,
        })
    }

    /// μr of triangle `t`.
    pub fn mu_of(&self, t: usize) -> f64 {
        self.mu.get(t).copied().unwrap_or(1.0)
    }

    /// e_z/(jβ) = φ of `mode` at barycentric `l` of triangle `t`.
    pub fn phi(&self, mode: &PortMode, t: usize, l: [f64; 3]) -> Result<f64> {
        let tri = self.triangle(t)?;
        let (_, vz) = self.unknowns_of(t);
        Ok(tri.scalars(l).into_iter().zip(&vz).map(|(s, u)| u.get().map_or(0.0, |u| s * mode.x.get(u).copied().unwrap_or(0.0))).sum())
    }
}

/// Orthonormal vectors spanning `v` (modified Gram–Schmidt, twice); `None` when they are
/// dependent.
fn orthonormal(mut v: Vec<Vec<f64>>) -> Option<Vec<Vec<f64>>> {
    let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(p, q)| p * q).sum::<f64>();
    for i in 0..v.len() {
        let start = dot(&v[i], &v[i]).sqrt();
        for _ in 0..2 {
            for j in 0..i {
                let d = dot(&v[i], &v[j]);
                let (head, tail) = v.split_at_mut(i);
                tail[0].iter_mut().zip(&head[j]).for_each(|(a, b)| *a -= d * b);
            }
        }
        let len = dot(&v[i], &v[i]).sqrt();
        if !(len > 1e-10 * start && len.is_finite()) {
            return None;
        }
        v[i].iter_mut().for_each(|c| *c /= len);
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A structured mesh of [0, a] × [0, b], nx × ny cells each cut into two triangles along
    /// alternating diagonals, εr from `eps(x, y)` at each triangle's centroid.
    pub(crate) fn rectangle(order: Order, a: f64, b: f64, nx: usize, ny: usize, eps: impl Fn(f64, f64) -> f64) -> CrossSection {
        let nodes: Vec<Point3> =
            (0..=ny).flat_map(|j| (0..=nx).map(move |i| Point3::new(a * i as f64 / nx as f64, b * j as f64 / ny as f64, 0.0))).collect();
        let id = |i: usize, j: usize| (j * (nx + 1) + i) as u32;
        let mut tris = Vec::new();
        for j in 0..ny {
            for i in 0..nx {
                let (p, q, r, s) = (id(i, j), id(i + 1, j), id(i + 1, j + 1), id(i, j + 1));
                if (i + j) % 2 == 0 {
                    tris.push([p, q, r]);
                    tris.push([p, r, s]);
                } else {
                    tris.push([p, q, s]);
                    tris.push([q, r, s]);
                }
            }
        }
        let e: Vec<f64> = tris.iter().map(|t| Point3::centroid(t.map(|v| nodes[v as usize])).map_or(1.0, |c| eps(c.x, c.y))).collect();
        let mu = vec![1.0; tris.len()];
        CrossSection::new(order, nodes, tris, e, mu, &[], &Default::default()).unwrap()
    }

    #[test]
    fn an_empty_rectangle_has_the_te_and_tm_modes() {
        // WR-90 at 12 GHz: β² = k² − k_c², TE10, TE20, TE01, TE11/TM11 (degenerate), TE30: one
        // propagating, five evanescent. Order 2 on 12 × 6 cells: within 4e-4 k² (order 1 on
        // 24 × 12: 1.5e-2); the TE10 field within 1e-3 (3e-2).
        let (a, b) = (0.02286, 0.01016);
        let k0 = 2.0 * std::f64::consts::PI * 12e9 / emcraft_model::units::C0;
        let mut exact: Vec<f64> = emcraft_fem::modes::lowest(a, b, 8).iter().map(|m| k0 * k0 - m.cutoff().powi(2)).collect();
        exact.sort_by(|p, q| q.total_cmp(p));
        for (order, n, tol, field_tol) in [(Order::One, 24, 2.5e-2, 5e-2), (Order::Two, 12, 6e-4, 2e-3)] {
            let cs = rectangle(order, a, b, n, n / 2, |_, _| 1.0);
            let modes = cs.modes(k0, 6).unwrap();
            for (m, e) in modes.iter().zip(&exact) {
                // Relative to k² (β² changes sign through the cut-offs).
                assert!((m.beta2 - e).abs() < tol * k0 * k0, "{order:?}: {} {e}", m.beta2);
            }
            // TE10: e = ŷ √(2/ab) sin(πx/a) (∫|e|² = 1, ∫ e_y > 0), no axial field (g = e), so
            // D = ∫|e|² = 1.
            let te10 = &modes[0];
            assert!((te10.d - 1.0).abs() < 1e-6, "{}", te10.d);
            let amp = (2.0 / (a * b)).sqrt();
            let worst = (0..cs.tris.len())
                .map(|t| {
                    let c = Point3::centroid(cs.tris[t].map(|v| cs.nodes[v as usize])).unwrap();
                    let (e, g) = cs.fields(te10, t, c).unwrap();
                    let exact = Vec3::unit(1) * (amp * (std::f64::consts::PI * c.x / a).sin());
                    ((e - exact).norm() / amp).max((g - e).norm() / amp)
                })
                .fold(0.0, f64::max);
            assert!(worst < field_tol, "{order:?}: {worst:e}");
        }
    }

    #[test]
    fn a_dielectric_slab_gives_the_hybrid_lse_and_lsm_modes() {
        // WR-90 with εr = 2.25 filling its lower half (0 < y < b/2) at 10 GHz: the LSM₁₀
        // (TE10-like, with an axial E) and the modes after it, against Collin's transcendental
        // equations. The interface is a grid line.
        let (a, b, er) = (0.02286, 0.01016, 2.25);
        let k0 = 2.0 * std::f64::consts::PI * 10e9 / emcraft_model::units::C0;
        let exact = emcraft_theory::slab::slab_loaded_modes(a, b, b / 2.0, er, k0, -3.0 * k0 * k0, 6);
        let cs = rectangle(Order::Two, a, b, 16, 8, |_, y| if y < b / 2.0 { er } else { 1.0 });
        let modes = cs.modes(k0, 6).unwrap();
        for (m, e) in modes.iter().zip(&exact) {
            // The dominant mode within 2e-6 k², the sixth (deeply evanescent) within 6e-4 of
            // |β²| + k².
            assert!((m.beta2 - e).abs() < 1e-3 * (e.abs() + k0 * k0), "{} {e}", m.beta2);
        }
        // The dominant mode is hybrid: its axial field φ is not zero.
        let te = &modes[0];
        let phi: f64 = (0..cs.tris.len()).map(|t| cs.phi(te, t, [1.0 / 3.0; 3]).unwrap().abs()).fold(0.0, f64::max);
        assert!(phi > 1e-3, "{phi}");
        assert!((modes[0].beta2 - exact[0]).abs() < 1e-5 * k0 * k0);
    }

    /// An annulus r_in < r < r_out in `rings` × `sectors` cells, two triangles each.
    fn annulus(order: Order, r_in: f64, r_out: f64, rings: usize, sectors: usize, eps: f64) -> CrossSection {
        let nodes: Vec<Point3> = (0..=rings)
            .flat_map(|i| {
                let r = r_in + (r_out - r_in) * i as f64 / rings as f64;
                (0..sectors).map(move |j| {
                    let a = 2.0 * std::f64::consts::PI * j as f64 / sectors as f64;
                    Point3::new(r * a.cos(), r * a.sin(), 0.0)
                })
            })
            .collect();
        let id = |i: usize, j: usize| (i * sectors + j % sectors) as u32;
        let tris: Vec<[u32; 3]> = (0..rings)
            .flat_map(|i| (0..sectors).flat_map(move |j| [[id(i, j), id(i + 1, j), id(i + 1, j + 1)], [id(i, j), id(i + 1, j + 1), id(i, j + 1)]]))
            .collect();
        let n = tris.len();
        CrossSection::new(order, nodes, tris, vec![eps; n], vec![1.0; n], &[], &Default::default()).unwrap()
    }

    #[test]
    fn a_coaxial_line_carries_its_tem_mode_at_the_speed_of_light_in_it() {
        // A two-conductor line of one dielectric carries TEM at β = k √εr whatever its section's
        // shape, so the polygonal one here too: exact but for rounding. Its field is radial,
        // outward by the sign convention.
        let k0 = 2.0 * std::f64::consts::PI * 3e9 / emcraft_model::units::C0;
        for order in [Order::One, Order::Two] {
            let cs = annulus(order, 0.001, 0.0035, 4, 24, 2.1);
            let modes = cs.modes(k0, 2).unwrap();
            assert!((modes[0].beta2 / (k0 * k0 * 2.1) - 1.0).abs() < 1e-9, "{order:?} {}", modes[0].beta2);
            let p = Point3::new(0.002, 0.0002, 0.0);
            let t = (0..cs.tris.len())
                .find(|&t| {
                    let tri = cs.triangle(t).unwrap();
                    tri.barycentric(p).iter().all(|l| *l >= 0.0)
                })
                .unwrap();
            let (e, g) = cs.fields(&modes[0], t, p).unwrap();
            assert!(e.dot(p.to_vec()) > 0.0 && e.cross(p.to_vec()).z.abs() < 0.2 * e.norm() * p.to_vec().norm(), "{e:?}");
            // TEM: no axial field, so g = e and D = ∫|e|² = 1.
            assert!((g - e).norm() < 1e-6 * e.norm() && (modes[0].d - 1.0).abs() < 1e-9, "{}", modes[0].d);
            // The next mode (TE11-like) is below its cut-off at 3 GHz: evanescent.
            assert!(modes[1].beta2 < 0.0);
        }
    }

    #[test]
    fn a_coaxial_line_has_its_closed_form_impedance() {
        // Z₀ = η₀ ln(b/a) / (2π √εr), by power and the inner conductor's current, the rings'
        // edges curved onto the circles.
        let (r_in, r_out, er, rings, sectors) = (0.001, 0.0035, 2.1, 4, 24);
        let nodes: Vec<Point3> = (0..=rings)
            .flat_map(|i| {
                let r = r_in + (r_out - r_in) * i as f64 / rings as f64;
                (0..sectors).map(move |j| {
                    let a = 2.0 * std::f64::consts::PI * j as f64 / sectors as f64;
                    Point3::new(r * a.cos(), r * a.sin(), 0.0)
                })
            })
            .collect();
        let id = |i: usize, j: usize| (i * sectors + j % sectors) as u32;
        let tris: Vec<[u32; 3]> = (0..rings)
            .flat_map(|i| (0..sectors).flat_map(move |j| [[id(i, j), id(i + 1, j), id(i + 1, j + 1)], [id(i, j), id(i + 1, j + 1), id(i, j + 1)]]))
            .collect();
        let mut curved = std::collections::BTreeMap::new();
        for (i, r) in [(0, r_in), (rings, r_out)] {
            for j in 0..sectors {
                let (a, b) = (id(i, j), id(i, j + 1));
                let angle = 2.0 * std::f64::consts::PI * (j as f64 + 0.5) / sectors as f64;
                curved.insert([a.min(b), a.max(b)], Point3::new(r * angle.cos(), r * angle.sin(), 0.0));
            }
        }
        let n = tris.len();
        let cs = CrossSection::new(Order::Two, nodes, tris, vec![er; n], vec![1.0; n], &[], &curved).unwrap();
        assert_eq!(cs.conductors().len(), 1);
        let k0 = 2.0 * std::f64::consts::PI * 3e9 / emcraft_model::units::C0;
        let modes = cs.modes(k0, 1).unwrap();
        let z = cs.impedance(&modes[0], k0).unwrap().unwrap();
        let eta0 = emcraft_model::units::MU0 * emcraft_model::units::C0;
        let exact = eta0 * (r_out / r_in).ln() / (2.0 * std::f64::consts::PI * er.sqrt());
        // Measured 2.6e-4 (the current taken on the edges of the first ring, where H's normal part
        // jumps: 6.6e-2).
        assert!((z / exact - 1.0).abs() < 1e-3, "{z} {exact}");
        // The current flows along +ŝ for a field pointing outwards (positive inner conductor).
        let (i, _) = cs.currents(&modes[0], k0).unwrap();
        assert!(i[0].re.abs() > 0.0);
    }
}

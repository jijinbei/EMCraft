//! Nédélec edge elements of the first kind on tetrahedra (J.-C. Nédélec, *Mixed finite elements
//! in R³*, Numer. Math. 35:315–341, 1980), with the hierarchical basis of J. P. Webb,
//! *Hierarchal vector basis functions of arbitrary order for triangular and tetrahedral finite
//! elements*, IEEE Trans. Antennas Propag. 47(8):1244–1253, 1999:
//!
//! - **order 1** (6 functions): the Whitney edge functions w_ij = λ_i∇λ_j − λ_j∇λ_i (H. Whitney,
//!   *Geometric Integration Theory*, Princeton 1957), one per edge, whose tangential component
//!   integrates to 1 along their edge and 0 along the others;
//! - **order 2** (20 functions, Webb's "order 1.5"): the Whitney functions, the gradients
//!   ∇(λ_iλ_j) of the edges, and two functions per face (i < j < k), λ_i w_jk and λ_j w_ki.
//!
//! The corners must be given **sorted by global node number**: local edges and faces are then
//! oriented by ascending global nodes, the same from every tetrahedron that shares them, so
//! tangential continuity holds with no sign bookkeeping. Edge i of [`LOCAL_EDGES`] joins
//! corners `LOCAL_EDGES[i]`; face f of [`LOCAL_FACES`] has corners `LOCAL_FACES[f]`.
//!
//! Local degrees of freedom, order 2: the six Whitney functions (edge order), the six edge
//! gradients (edge order), then the two functions of each face (face order).
//!
//! **Curved elements** ([`Element::curved`]): a tetrahedron whose edges' midpoints move onto a
//! curved surface is the image of the straight one under the quadratic map
//! Φ(x̂) = x̂ + Σ_edges 4λ_iλ_j d_ij (d_ij the midpoint's displacement), the isoparametric map of
//! the ten-node tetrahedron. Its functions are the straight ones carried by the covariant Piola
//! transform, N = DΦ^{−T} N̂ and curl N = DΦ curl N̂ / det DΦ (P. Monk, *Finite Element Methods
//! for Maxwell's Equations*, Oxford 2003, §3.9), which keeps tangential continuity: on a face
//! the map depends only on that face's edges, the same from both sides. Its matrices are
//! integrated by quadrature.

use crate::poly::{Point3, Symbolic, Vec3, VecPoly, realise, realise_curl};
use crate::quadrature::tet_rule;

pub const LOCAL_EDGES: [[usize; 2]; 6] = [[0, 1], [0, 2], [0, 3], [1, 2], [1, 3], [2, 3]];
pub const LOCAL_FACES: [[usize; 3]; 4] = [[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]];

/// The element order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Order {
    One,
    Two,
}

impl Order {
    pub fn from_u8(n: u8) -> Option<Order> {
        match n {
            1 => Some(Order::One),
            2 => Some(Order::Two),
            _ => None,
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            Order::One => 1,
            Order::Two => 2,
        }
    }

    /// Basis functions per tetrahedron.
    pub fn dofs(self) -> usize {
        match self {
            Order::One => 6,
            Order::Two => 20,
        }
    }

    /// Degrees of freedom per edge and per face.
    pub fn per_edge(self) -> usize {
        match self {
            Order::One => 1,
            Order::Two => 2,
        }
    }

    pub fn per_face(self) -> usize {
        match self {
            Order::One => 0,
            Order::Two => 2,
        }
    }
}

fn unit(i: usize) -> [u8; 4] {
    let mut e = [0; 4];
    e[i] = 1;
    e
}

fn pair(i: usize, j: usize) -> [u8; 4] {
    let mut e = [0; 4];
    e[i] += 1;
    e[j] += 1;
    e
}

/// The basis functions of an order as symbolic terms (independent of the element's shape).
pub fn symbolic(order: Order) -> Vec<Vec<Symbolic>> {
    let s = |coef: f64, exp: [u8; 4], grad: usize| Symbolic { coef, exp, grad };
    let mut out = Vec::with_capacity(order.dofs());
    // Whitney: λ_i∇λ_j − λ_j∇λ_i.
    for [i, j] in LOCAL_EDGES {
        out.push(vec![s(1.0, unit(i), j), s(-1.0, unit(j), i)]);
    }
    if order == Order::Two {
        // ∇(λ_iλ_j) = λ_i∇λ_j + λ_j∇λ_i.
        for [i, j] in LOCAL_EDGES {
            out.push(vec![s(1.0, unit(i), j), s(1.0, unit(j), i)]);
        }
        for [i, j, k] in LOCAL_FACES {
            // λ_i w_jk = λ_iλ_j∇λ_k − λ_iλ_k∇λ_j.
            out.push(vec![s(1.0, pair(i, j), k), s(-1.0, pair(i, k), j)]);
            // λ_j w_ki = λ_jλ_k∇λ_i − λ_jλ_i∇λ_k.
            out.push(vec![s(1.0, pair(j, k), i), s(-1.0, pair(j, i), k)]);
        }
    }
    out
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ElementError {
    #[error("degenerate tetrahedron (volume {0:e})")]
    Degenerate(f64),
}

/// A tetrahedron with its basis functions and their curls realised.
#[derive(Clone, Debug)]
pub struct Element {
    pub order: Order,
    pub corners: [Point3; 4],
    /// Unsigned volume.
    pub volume: f64,
    /// ∇λᵢ.
    pub grads: [Vec3; 4],
    pub basis: Vec<VecPoly>,
    pub curls: Vec<VecPoly>,
    /// The displacements of the edges' midpoints (local edge order) of a curved element.
    pub curve: Option<[Vec3; 6]>,
}

/// A 3 × 3 matrix by rows (the Jacobian DΦ of a curved element's map).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat3 {
    pub rows: [Vec3; 3],
}

impl Mat3 {
    pub const IDENTITY: Mat3 = Mat3 { rows: [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)] };

    pub fn det(&self) -> f64 {
        emcraft_model::geom::triple(self.rows[0], self.rows[1], self.rows[2])
    }

    /// m v.
    pub fn mul(&self, v: Vec3) -> Vec3 {
        Vec3::new(self.rows[0].dot(v), self.rows[1].dot(v), self.rows[2].dot(v))
    }

    /// The columns.
    fn columns(&self) -> [Vec3; 3] {
        let [a, b, c] = self.rows;
        [Vec3::new(a.x, b.x, c.x), Vec3::new(a.y, b.y, c.y), Vec3::new(a.z, b.z, c.z)]
    }

    /// x = adj(m) v / det, adj's columns the cross products of `vs` (Cramer): `vs` the rows to
    /// solve m x = v, the columns to solve mᵀ x = v.
    fn cramer([r0, r1, r2]: [Vec3; 3], det: f64, v: Vec3) -> Vec3 {
        (r1.cross(r2) * v.x + r2.cross(r0) * v.y + r0.cross(r1) * v.z) / det
    }

    /// The solution of m x = v, det m given.
    pub fn solve(&self, det: f64, v: Vec3) -> Vec3 {
        Mat3::cramer(self.rows, det, v)
    }

    /// The solution of mᵀ x = v, det m given.
    pub fn solve_transposed(&self, det: f64, v: Vec3) -> Vec3 {
        Mat3::cramer(self.columns(), det, v)
    }
}

/// The smallest det DΦ of the quadratic map of a tetrahedron (any corner order) whose edges'
/// midpoints move to `mids` (local edge order): how close it comes to folding.
pub fn least_jacobian(corners: [Point3; 4], mids: [Option<Point3>; 6]) -> Result<f64, ElementError> {
    Ok(Element::new(Order::One, corners)?.bent(mids)?.least_jacobian())
}

/// Where a curved element's map is checked not to fold: the corners, the edges' midpoints, the
/// faces' centroids and the centroid (the mesher bends edges only where det DΦ stays well above
/// zero at these).
pub fn check_points() -> Vec<[f64; 4]> {
    let mut out = Vec::with_capacity(15);
    for i in 0..4 {
        let mut l = [0.0; 4];
        l[i] = 1.0;
        out.push(l);
    }
    for [i, j] in LOCAL_EDGES {
        let mut l = [0.0; 4];
        l[i] = 0.5;
        l[j] = 0.5;
        out.push(l);
    }
    for f in LOCAL_FACES {
        let mut l = [0.0; 4];
        for i in f {
            l[i] = 1.0 / 3.0;
        }
        out.push(l);
    }
    out.push([0.25; 4]);
    out
}

impl Element {
    /// The element on these corners (sorted by global node, see the module notes).
    pub fn new(order: Order, corners: [Point3; 4]) -> Result<Element, ElementError> {
        let [e1, e2, e3] = [1, 2, 3].map(|k| corners[k] - corners[0]);
        let det = emcraft_model::geom::triple(e1, e2, e3);
        let scale_len = [e1, e2, e3].iter().map(|e| e.norm_sqr()).fold(0.0, f64::max);
        if !(det.abs() > 1e-14 * scale_len.powf(1.5)) || !det.is_finite() {
            return Err(ElementError::Degenerate(det / 6.0));
        }
        // ∇λ₁ = (e2 × e3)/det, ∇λ₂ = (e3 × e1)/det, ∇λ₃ = (e1 × e2)/det, ∇λ₀ = −Σ.
        let (g1, g2, g3) = (e2.cross(e3) / det, e3.cross(e1) / det, e1.cross(e2) / det);
        let g0 = -(g1 + g2 + g3);
        let grads = [g0, g1, g2, g3];
        let sym = symbolic(order);
        let basis = sym.iter().map(|t| realise(t, &grads)).collect();
        let curls = sym.iter().map(|t| realise_curl(t, &grads)).collect();
        Ok(Element { order, corners, volume: det.abs() / 6.0, grads, basis, curls, curve: None })
    }

    /// The element bent so that the midpoints of the edges given (local edge order) lie at those
    /// points; the others stay straight. Refused when the map folds (det DΦ ≤ 0 somewhere).
    pub fn curved(self, mids: [Option<Point3>; 6]) -> Result<Element, ElementError> {
        if mids.iter().all(Option::is_none) {
            return Ok(self);
        }
        let e = self.bent(mids)?;
        let least = e.least_jacobian();
        if !(least > 1e-3) {
            return Err(ElementError::Degenerate(least * e.volume));
        }
        Ok(e)
    }

    /// The element with its edges' midpoints moved, unchecked.
    fn bent(mut self, mids: [Option<Point3>; 6]) -> Result<Element, ElementError> {
        let mut d = [Vec3::ZERO; 6];
        for ((k, [i, j]), m) in LOCAL_EDGES.iter().enumerate().zip(mids) {
            if let Some(m) = m {
                d[k] = m - self.corners[*i].midpoint(self.corners[*j]);
                if !d[k].is_finite() {
                    return Err(ElementError::Degenerate(f64::NAN));
                }
            }
        }
        self.curve = Some(d);
        Ok(self)
    }

    /// The smallest det DΦ over [`check_points`] (1 for a straight element).
    pub fn least_jacobian(&self) -> f64 {
        check_points().into_iter().map(|l| self.jacobian(l).det()).fold(f64::INFINITY, f64::min)
    }

    /// DΦ at barycentric coordinates `l` (the identity for a straight element).
    pub fn jacobian(&self, l: [f64; 4]) -> Mat3 {
        let Some(d) = &self.curve else { return Mat3::IDENTITY };
        // DΦ = I + Σ_edges d ⊗ ∇(4λ_aλ_b), ∇(4λ_aλ_b) = 4(λ_a∇λ_b + λ_b∇λ_a): row r gains d_r g.
        LOCAL_EDGES.iter().zip(d).fold(Mat3::IDENTITY, |mut j, ([a, b], dk)| {
            let g = (self.grads[*b] * l[*a] + self.grads[*a] * l[*b]) * 4.0;
            for (r, row) in j.rows.iter_mut().enumerate() {
                *row += g * dk[r];
            }
            j
        })
    }

    /// The curl–curl matrix ∫ ν curl Nᵢ · curl Nⱼ dV (row-major, n × n) for a constant ν.
    pub fn stiffness(&self, nu: f64) -> Vec<f64> {
        if self.curve.is_some() {
            return self.gram_by_quadrature(nu, true);
        }
        gram(&self.curls, self.volume, nu)
    }

    /// The mass matrix ∫ ε Nᵢ · Nⱼ dV for a constant ε.
    pub fn mass(&self, eps: f64) -> Vec<f64> {
        if self.curve.is_some() {
            return self.gram_by_quadrature(eps, false);
        }
        gram(&self.basis, self.volume, eps)
    }

    /// The curl–curl matrix split by component, ∫ (curl Nᵢ)_d (curl Nⱼ)_d dV for d = x, y, z:
    /// for a diagonal tensor ν the matrix is Σ_d ν_d of these (perfectly matched layers).
    pub fn stiffness_components(&self) -> [Vec<f64>; 3] {
        [0, 1, 2].map(|d| self.component_gram(d, true))
    }

    /// The mass matrix split by component, ∫ (Nᵢ)_d (Nⱼ)_d dV.
    pub fn mass_components(&self) -> [Vec<f64>; 3] {
        [0, 1, 2].map(|d| self.component_gram(d, false))
    }

    fn component_gram(&self, d: usize, curl: bool) -> Vec<f64> {
        let n = self.basis.len();
        let mut m = vec![0.0; n * n];
        if self.curve.is_some() {
            for (l, w) in self.rule(5) {
                let v = if curl { self.curl_values(l) } else { self.values(l) };
                for i in 0..n {
                    for j in i..n {
                        m[i * n + j] += w * v[i][d] * v[j][d];
                    }
                }
            }
        } else {
            let p = if curl { &self.curls } else { &self.basis };
            for i in 0..n {
                for j in i..n {
                    m[i * n + j] = p[i].inner_component(&p[j], self.volume, d);
                }
            }
        }
        for i in 0..n {
            for j in 0..i {
                m[i * n + j] = m[j * n + i];
            }
        }
        m
    }

    fn gram_by_quadrature(&self, factor: f64, curl: bool) -> Vec<f64> {
        let n = self.basis.len();
        let mut m = vec![0.0; n * n];
        // The integrands are rational in λ; five points per direction hold the curvature of a
        // mesh fine enough to resolve its surfaces to well below the discretization error.
        for (l, w) in self.rule(5) {
            let v = if curl { self.curl_values(l) } else { self.values(l) };
            for i in 0..n {
                for j in i..n {
                    m[i * n + j] += factor * w * v[i].dot(v[j]);
                }
            }
        }
        for i in 0..n {
            for j in 0..i {
                m[i * n + j] = m[j * n + i];
            }
        }
        m
    }

    /// A quadrature rule over the (curved) element: barycentric coordinates of the straight
    /// element and weights summing to the volume, n points per direction.
    pub fn rule(&self, n: usize) -> Vec<([f64; 4], f64)> {
        let mut r = tet_rule(&self.corners, n);
        if self.curve.is_some() {
            for (l, w) in &mut r {
                *w *= self.jacobian(*l).det();
            }
        }
        r
    }

    /// A rule over face `face` (local corners): barycentric coordinates in the element, weights
    /// summing to the face's area, and the unit normal (by the right hand along the corners).
    pub fn face_rule(&self, face: [usize; 3], n: usize) -> Vec<([f64; 4], f64, Vec3)> {
        let (t1, t2) = (self.corners[face[1]] - self.corners[face[0]], self.corners[face[2]] - self.corners[face[0]]);
        crate::quadrature::triangle_rule(n)
            .into_iter()
            .map(|(b, w)| {
                let mut l = [0.0; 4];
                for (k, c) in face.iter().enumerate() {
                    l[*c] = b[k];
                }
                let j = self.jacobian(l);
                let c = j.mul(t1).cross(j.mul(t2));
                let len = c.norm();
                (l, 0.5 * len * w, c / len.max(f64::MIN_POSITIVE))
            })
            .collect()
    }

    /// The basis functions at barycentric coordinates `l`.
    pub fn values(&self, l: [f64; 4]) -> Vec<Vec3> {
        let v = self.basis.iter().map(|p| p.eval(l));
        if self.curve.is_some() {
            let j = self.jacobian(l);
            let det = j.det();
            return v.map(|v| j.solve_transposed(det, v)).collect();
        }
        v.collect()
    }

    pub fn curl_values(&self, l: [f64; 4]) -> Vec<Vec3> {
        let c = self.curls.iter().map(|p| p.eval(l));
        if self.curve.is_some() {
            let j = self.jacobian(l);
            let det = j.det();
            return c.map(|c| j.mul(c) / det).collect();
        }
        c.collect()
    }

    fn straight_barycentric(&self, p: Point3) -> [f64; 4] {
        let d = p - self.corners[0];
        let (l1, l2, l3) = (self.grads[1].dot(d), self.grads[2].dot(d), self.grads[3].dot(d));
        [1.0 - l1 - l2 - l3, l1, l2, l3]
    }

    fn straight_point(&self, l: [f64; 4]) -> Point3 {
        Point3::ORIGIN + self.corners.iter().zip(l).map(|(c, w)| c.to_vec() * w).sum::<Vec3>()
    }

    /// The barycentric coordinates of a point (for a curved element, of its preimage under Φ,
    /// by Newton's method from the straight element's).
    pub fn barycentric(&self, p: Point3) -> [f64; 4] {
        let mut l = self.straight_barycentric(p);
        if self.curve.is_some() {
            let mut x = self.straight_point(l);
            for _ in 0..20 {
                let r = self.point(l) - p;
                let j = self.jacobian(l);
                let dx = j.solve(j.det(), r);
                x -= dx;
                l = self.straight_barycentric(x);
                if dx.norm_sqr() <= 1e-30 * x.to_vec().norm_sqr().max(1e-300) {
                    break;
                }
            }
        }
        l
    }

    /// The point at barycentric coordinates `l`.
    pub fn point(&self, l: [f64; 4]) -> Point3 {
        let bend: Vec3 = match &self.curve {
            Some(d) => LOCAL_EDGES.iter().zip(d).map(|([a, b], dk)| *dk * (4.0 * l[*a] * l[*b])).sum(),
            None => Vec3::ZERO,
        };
        self.straight_point(l) + bend
    }
}

fn gram(p: &[VecPoly], volume: f64, factor: f64) -> Vec<f64> {
    let n = p.len();
    let mut m = vec![0.0; n * n];
    for i in 0..n {
        for j in i..n {
            let v = factor * p[i].inner(&p[j], volume);
            m[i * n + j] = v;
            m[j * n + i] = v;
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quadrature::{gauss_legendre, tet_rule};

    fn tet() -> [Point3; 4] {
        [[0.1, 0.0, 0.2], [1.3, 0.1, -0.1], [0.2, 0.9, 0.3], [0.4, 0.3, 1.1]].map(Point3::from)
    }

    fn bary_of(e: &Element, p: Point3) -> [f64; 4] {
        let d = p - e.corners[0];
        let l1 = e.grads[1].dot(d);
        let l2 = e.grads[2].dot(d);
        let l3 = e.grads[3].dot(d);
        [1.0 - l1 - l2 - l3, l1, l2, l3]
    }

    #[test]
    fn whitney_functions_are_dual_to_the_edges() {
        let e = Element::new(Order::One, tet()).unwrap();
        let (x, w) = gauss_legendre(4);
        for (f, basis) in e.basis.iter().enumerate() {
            for (k, [a, b]) in LOCAL_EDGES.iter().enumerate() {
                let t = e.corners[*b] - e.corners[*a];
                let mut s = 0.0;
                for (xi, wi) in x.iter().zip(&w) {
                    let p = e.corners[*a].lerp(e.corners[*b], *xi);
                    s += wi * basis.eval(bary_of(&e, p)).dot(t);
                }
                let expect = if f == k { 1.0 } else { 0.0 };
                assert!((s - expect).abs() < 1e-13, "function {f} on edge {k}: {s}");
            }
        }
    }

    #[test]
    fn order_one_matrices_match_the_closed_form() {
        // curl w_ij = 2 ∇λ_i × ∇λ_j; ∫ λ_a λ_b = V (1 + δ_ab) / 20.
        let e = Element::new(Order::One, tet()).unwrap();
        let (s, m) = (e.stiffness(1.0), e.mass(1.0));
        let g = e.grads;
        let ll = |a: usize, b: usize| e.volume * if a == b { 2.0 } else { 1.0 } / 20.0;
        for (p, [i, j]) in LOCAL_EDGES.iter().enumerate() {
            for (q, [k, l]) in LOCAL_EDGES.iter().enumerate() {
                let cs = 4.0 * e.volume * g[*i].cross(g[*j]).dot(g[*k].cross(g[*l]));
                let cm =
                    ll(*i, *k) * g[*j].dot(g[*l]) - ll(*i, *l) * g[*j].dot(g[*k]) - ll(*j, *k) * g[*i].dot(g[*l]) + ll(*j, *l) * g[*i].dot(g[*k]);
                assert!((s[p * 6 + q] - cs).abs() < 1e-12 * cs.abs().max(1.0));
                assert!((m[p * 6 + q] - cm).abs() < 1e-14);
            }
        }
    }

    #[test]
    fn matrices_agree_with_quadrature() {
        let e = Element::new(Order::Two, tet()).unwrap();
        let (s, m) = (e.stiffness(1.0), e.mass(1.0));
        let n = e.basis.len();
        let rule = tet_rule(&e.corners, 5);
        let mut sq = vec![0.0; n * n];
        let mut mq = vec![0.0; n * n];
        for (l, w) in &rule {
            let (v, c) = (e.values(*l), e.curl_values(*l));
            for i in 0..n {
                for j in 0..n {
                    mq[i * n + j] += w * v[i].dot(v[j]);
                    sq[i * n + j] += w * c[i].dot(c[j]);
                }
            }
        }
        for k in 0..n * n {
            assert!((s[k] - sq[k]).abs() < 1e-11, "S[{k}] {} {}", s[k], sq[k]);
            assert!((m[k] - mq[k]).abs() < 1e-13, "M[{k}] {} {}", m[k], mq[k]);
        }
    }

    #[test]
    fn curls_match_finite_differences() {
        let e = Element::new(Order::Two, tet()).unwrap();
        let p = e.point([0.2, 0.3, 0.1, 0.4]);
        let h = 1e-6;
        for f in 0..e.basis.len() {
            let at = |q: Point3| e.basis[f].eval(bary_of(&e, q));
            let d = |axis: usize, comp: usize| {
                let mut a = p;
                let mut b = p;
                a[axis] += h;
                b[axis] -= h;
                (at(a)[comp] - at(b)[comp]) / (2.0 * h)
            };
            let curl = Vec3::new(d(1, 2) - d(2, 1), d(2, 0) - d(0, 2), d(0, 1) - d(1, 0));
            let c = e.curls[f].eval(bary_of(&e, p));
            for k in 0..3 {
                assert!((curl[k] - c[k]).abs() < 1e-6, "function {f}: {curl:?} vs {c:?}");
            }
        }
    }

    /// Solves the L2 projection of a polynomial field onto the element's span and returns the
    /// largest error at sample points.
    fn projection_error(order: Order, field: impl Fn(Point3) -> Vec3) -> f64 {
        let e = Element::new(order, tet()).unwrap();
        let n = e.basis.len();
        let rule = tet_rule(&e.corners, 6);
        let m = e.mass(1.0);
        let mut rhs = vec![0.0; n];
        for (l, w) in &rule {
            let f = field(e.point(*l));
            for (r, v) in rhs.iter_mut().zip(e.values(*l)) {
                *r += w * v.dot(f);
            }
        }
        let x = crate::dense_solve(m, rhs).unwrap();
        let mut worst: f64 = 0.0;
        for l in [[0.25; 4], [0.7, 0.1, 0.1, 0.1], [0.0, 0.5, 0.5, 0.0], [0.1, 0.2, 0.3, 0.4]] {
            let v = e.values(l);
            let u: Vec3 = x.iter().zip(&v).map(|(c, vi)| *vi * *c).sum();
            let f = field(e.point(l));
            worst = worst.max((u - f).iter().fold(0.0f64, |a, b| a.max(b.abs())));
        }
        worst
    }

    #[test]
    fn spans_constants_and_order_two_spans_linear_fields() {
        let constant = |_: Point3| Vec3::new(0.3, -1.2, 0.7);
        assert!(projection_error(Order::One, constant) < 1e-12);
        // Whitney functions span a + b × x.
        let rot = |x: Point3| Vec3::new(0.3, -1.2, 0.7) + Vec3::new(0.5, 0.1, -0.4).cross(x.to_vec());
        assert!(projection_error(Order::One, rot) < 1e-12);
        let linear = |x: Point3| Vec3::new(1.0 + 2.0 * x.x - x.z, 0.5 * x.y + 3.0 * x.z, x.x - x.y + 0.2);
        assert!(projection_error(Order::One, linear) > 1e-2);
        assert!(projection_error(Order::Two, linear) < 1e-11);
    }

    #[test]
    fn gradients_are_in_the_kernel_of_the_curl() {
        // Order 2 holds the gradients of quadratics: λ-linear ones through the Whitney functions
        // (coefficients φ_j − φ_i), λ_iλ_j through the edge gradients.
        let e = Element::new(Order::Two, tet()).unwrap();
        let s = e.stiffness(1.0);
        let n = 20;
        let phi = [0.3, -1.0, 2.0, 0.5];
        let psi = [0.7, -0.2, 0.1, 1.5, -0.9, 0.4];
        let mut x = vec![0.0; n];
        for (k, [i, j]) in LOCAL_EDGES.iter().enumerate() {
            x[k] = phi[*j] - phi[*i];
            x[6 + k] = psi[k];
        }
        let norm = s.iter().fold(0.0f64, |a, b| a.max(b.abs()));
        for i in 0..n {
            let r: f64 = (0..n).map(|j| s[i * n + j] * x[j]).sum();
            assert!(r.abs() < 1e-13 * norm, "row {i}: {r}");
        }
    }

    #[test]
    fn tangential_components_agree_across_a_shared_face() {
        // Two tetrahedra share the face (1, 2, 3) of global nodes; their fourth corners are on
        // either side. Corners are sorted by global node in each element.
        let p = [[0.0, 0.0, 0.0], [1.0, 0.1, 0.0], [0.1, 1.0, 0.2], [0.2, 0.3, 1.0], [1.0, 1.0, 1.0]].map(Point3::from);
        let a = Element::new(Order::Two, [p[0], p[1], p[2], p[3]]).unwrap();
        let b = Element::new(Order::Two, [p[1], p[2], p[3], p[4]]).unwrap();
        // Global DOFs on the shared face: edges (1,2), (1,3), (2,3) and the face's two functions.
        // In a they are local edges 3, 4, 5 and face 3; in b local edges 0, 1, 3 and face 0.
        let pairs: Vec<(usize, usize)> =
            vec![(3, 0), (4, 1), (5, 3), (6 + 3, 6), (6 + 4, 6 + 1), (6 + 5, 6 + 3), (12 + 2 * 3, 12), (12 + 2 * 3 + 1, 12 + 1)];
        let normal = (p[2] - p[1]).cross(p[3] - p[1]).normalized().unwrap();
        for (u, v, w) in [(0.2, 0.3, 0.5), (0.6, 0.2, 0.2), (1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0)] {
            let x = Point3::ORIGIN + p[1].to_vec() * u + p[2].to_vec() * v + p[3].to_vec() * w;
            let (va, vb) = (a.values(bary_of(&a, x)), b.values(bary_of(&b, x)));
            for (i, j) in &pairs {
                let ta = normal.cross(va[*i]);
                let tb = normal.cross(vb[*j]);
                assert!((ta - tb).iter().all(|d| d.abs() < 1e-12), "{i}/{j}: {ta:?} {tb:?}");
            }
            // DOFs not on the face have no tangential component there.
            for (k, f) in vb.iter().enumerate() {
                if !pairs.iter().any(|(_, j)| *j == k) {
                    assert!(normal.cross(*f).iter().all(|d| d.abs() < 1e-12), "b {k}");
                }
            }
        }
    }

    /// The test tetrahedron with edges (0, 1) and (1, 3) bowed out.
    fn bent(order: Order) -> Element {
        let c = tet();
        let mut mids = [None; 6];
        mids[0] = Some(c[0].midpoint(c[1]) + Vec3::new(0.0, -0.08, -0.05));
        mids[4] = Some(c[1].midpoint(c[3]) + Vec3::new(0.07, 0.0, 0.03));
        Element::new(order, c).unwrap().curved(mids).unwrap()
    }

    #[test]
    fn curved_elements_keep_their_identities() {
        let e = bent(Order::Two);
        // det DΦ is a cubic in λ: rules of 3 and 6 points per direction agree on the volume, and
        // it differs from the straight one.
        let (v3, v6): (f64, f64) = (e.rule(3).iter().map(|r| r.1).sum(), e.rule(6).iter().map(|r| r.1).sum());
        assert!((v3 - v6).abs() < 1e-14 && (v3 - e.volume).abs() > 1e-3, "{v3} {v6} {}", e.volume);
        // Newton inverts the map.
        for l in [[0.25; 4], [0.7, 0.1, 0.1, 0.1], [0.05, 0.5, 0.05, 0.4]] {
            let p = e.point(l);
            let back = e.barycentric(p);
            assert!(l.iter().zip(&back).all(|(a, b)| (a - b).abs() < 1e-12), "{l:?} {back:?}");
        }
        // The Whitney functions stay dual to the (now curved) edges: ∫ N · dx along edge (0, 1).
        let (x, w) = gauss_legendre(8);
        for f in 0..6 {
            let mut s = 0.0;
            for (t, wt) in x.iter().zip(&w) {
                let l = [1.0 - t, *t, 0.0, 0.0];
                // dx/dt = DΦ (c1 − c0).
                let tangent = e.jacobian(l).mul(e.corners[1] - e.corners[0]);
                s += wt * (e.values(l)[f]).dot(tangent);
            }
            assert!((s - if f == 0 { 1.0 } else { 0.0 }).abs() < 1e-12, "{f}: {s}");
        }
        // Curls by finite differences in physical coordinates.
        let p = e.point([0.2, 0.3, 0.1, 0.4]);
        let h = 1e-6;
        for f in 0..e.basis.len() {
            let at = |q: Point3| e.values(e.barycentric(q))[f];
            let d = |axis: usize, comp: usize| {
                let (mut a, mut b) = (p, p);
                a[axis] += h;
                b[axis] -= h;
                (at(a)[comp] - at(b)[comp]) / (2.0 * h)
            };
            let curl = Vec3::new(d(1, 2) - d(2, 1), d(2, 0) - d(0, 2), d(0, 1) - d(1, 0));
            let c = e.curl_values(e.barycentric(p))[f];
            assert!((0..3).all(|k| (curl[k] - c[k]).abs() < 1e-5), "function {f}: {curl:?} vs {c:?}");
        }
        // Gradients stay in the kernel of the curl (the Piola transform maps ∇φ̂ to ∇φ).
        let s = e.stiffness(1.0);
        let phi = [0.3, -1.0, 2.0, 0.5];
        let psi = [0.7, -0.2, 0.1, 1.5, -0.9, 0.4];
        let mut x = [0.0; 20];
        for (k, [i, j]) in LOCAL_EDGES.iter().enumerate() {
            x[k] = phi[*j] - phi[*i];
            x[6 + k] = psi[k];
        }
        let norm = s.iter().fold(0.0f64, |a, b| a.max(b.abs()));
        for i in 0..20 {
            let r: f64 = (0..20).map(|j| s[i * 20 + j] * x[j]).sum();
            assert!(r.abs() < 1e-12 * norm, "row {i}: {r}");
        }
        // Symmetric positive mass matrix.
        let m = e.mass(1.0);
        assert!((0..20).all(|i| m[i * 21] > 0.0 && (0..20).all(|j| (m[i * 20 + j] - m[j * 20 + i]).abs() < 1e-15)));
        // A map that folds is refused.
        let c = tet();
        let mut mids = [None; 6];
        mids[0] = Some(c[2] + Vec3::new(0.0, 0.5, 0.0));
        assert!(Element::new(Order::One, c).unwrap().curved(mids).is_err());
    }

    #[test]
    fn component_matrices_add_up() {
        for e in [Element::new(Order::Two, tet()).unwrap(), bent(Order::Two)] {
            let (s, m) = (e.stiffness(1.0), e.mass(1.0));
            let (sc, mc) = (e.stiffness_components(), e.mass_components());
            for k in 0..s.len() {
                assert!((sc[0][k] + sc[1][k] + sc[2][k] - s[k]).abs() < 1e-11 * s[k].abs().max(1.0));
                assert!((mc[0][k] + mc[1][k] + mc[2][k] - m[k]).abs() < 1e-14);
            }
        }
    }

    #[test]
    fn curved_faces_agree_from_both_sides() {
        // As for straight elements, with the shared face's edge (1, 2) bowed: both elements bend
        // it the same way.
        let p = [[0.0, 0.0, 0.0], [1.0, 0.1, 0.0], [0.1, 1.0, 0.2], [0.2, 0.3, 1.0], [1.0, 1.0, 1.0]].map(Point3::from);
        let mid = Point3::new(0.62, 0.62, 0.25);
        let mut ma = [None; 6];
        ma[3] = Some(mid);
        let mut mb = [None; 6];
        mb[0] = Some(mid);
        let a = Element::new(Order::Two, [p[0], p[1], p[2], p[3]]).unwrap().curved(ma).unwrap();
        let b = Element::new(Order::Two, [p[1], p[2], p[3], p[4]]).unwrap().curved(mb).unwrap();
        let pairs = [(3, 0), (4, 1), (5, 3), (9, 6), (10, 7), (11, 9), (18, 12), (19, 13)];
        for (u, v, w) in [(0.2, 0.3, 0.5), (0.6, 0.2, 0.2), (1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0)] {
            let la = [0.0, u, v, w];
            let lb = [u, v, w, 0.0];
            let (xa, xb) = (a.point(la), b.point(lb));
            assert!((xa - xb).iter().all(|d| d.abs() < 1e-14));
            // The surface normal at this point, from the face's tangents.
            let ja = a.jacobian(la);
            let n = ja.mul(p[2] - p[1]).cross(ja.mul(p[3] - p[1]));
            let (va, vb) = (a.values(la), b.values(lb));
            for (i, j) in pairs {
                let (ta, tb) = (n.cross(va[i]), n.cross(vb[j]));
                assert!((ta - tb).iter().all(|d| d.abs() < 1e-11), "{i}/{j}: {ta:?} {tb:?}");
            }
        }
        // The face rule integrates the curved face's area: the same from both elements.
        let area = |e: &Element, f: [usize; 3]| e.face_rule(f, 6).iter().map(|r| r.1).sum::<f64>();
        assert!((area(&a, [1, 2, 3]) - area(&b, [0, 1, 2])).abs() < 1e-14);
    }

    #[test]
    fn degenerate_tets_are_refused() {
        let flat = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]].map(Point3::from);
        assert!(Element::new(Order::One, flat).is_err());
        let nan = [[f64::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].map(Point3::from);
        assert!(Element::new(Order::One, nan).is_err());
    }
}

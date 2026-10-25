//! Elements on triangles of a plane cross-section z = const, for the modes of waveguide ports
//! of any shape: the traces of the tetrahedra's Nédélec functions ([`crate::nedelec`]) for the
//! transverse field and hierarchical Lagrange functions for the axial one — the mixed pair of
//! J.-F. Lee, D.-K. Sun, Z. J. Cendes, *Full-wave analysis of dielectric waveguides using
//! tangential vector finite elements*, IEEE Trans. MTT 39(8):1262–1271, 1991 (CT/LN at order 1,
//! LT/QN at order 2), whose discrete gradients of the scalar space lie in the vector space.
//!
//! - **vector, order 1** (3 functions): the Whitney functions w_ij = λ_i∇λ_j − λ_j∇λ_i of the
//!   edges [`TRI_EDGES`];
//! - **vector, order 2** (8): those, the edge gradients ∇(λ_iλ_j), and λ₀w₁₂, λ₁w₂₀ (Webb's
//!   face functions, J. P. Webb, IEEE Trans. AP 47(8), 1999);
//! - **scalar, order 1** (3): λ₀, λ₁, λ₂; **order 2** (6): those and λ_iλ_j per edge.
//!
//! As on tetrahedra, the corners are given sorted by global node number, so shared edges are
//! oriented alike from both sides. Points and vectors are in the xy plane (z = 0); the curl of a
//! transverse field is its z component, a scalar.
//!
//! **Curved triangles** ([`Triangle::curved`]): edges whose midpoints move onto a curved wall
//! make the quadratic map Φ(x̂) = x̂ + Σ 4λ_iλ_j d_ij, the functions carried by the covariant Piola
//! transform N = DΦ^{−T} N̂, curl N = curl N̂ / det DΦ, ∇φ = DΦ^{−T} ∇̂φ (P. Monk, *Finite Element
//! Methods for Maxwell's Equations*, Oxford 2003, §3.9) — the same map as the tetrahedra's
//! ([`crate::nedelec`]) on their faces, so a port's modes and the volume's traces share one space.

use crate::Order;
use crate::poly::{Point3, Vec3};

/// The local edges, by corner.
pub const TRI_EDGES: [[usize; 2]; 3] = [[0, 1], [0, 2], [1, 2]];

/// A straight triangle with its barycentric gradients.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Triangle {
    pub order: Order,
    pub corners: [Point3; 3],
    /// ∇λ_i (in the plane).
    grads: [Vec3; 3],
    pub area: f64,
    /// The edges' midpoint displacements (in [`TRI_EDGES`] order) of a curved triangle.
    bend: Option<[Vec3; 3]>,
}

/// A 2 × 2 Jacobian [[∂x/∂x̂, ∂x/∂ŷ], [∂y/∂x̂, ∂y/∂ŷ]].
#[derive(Clone, Copy, Debug)]
struct Jac {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

impl Jac {
    fn det(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    /// J^{−T} v (v in the plane).
    fn inv_t(&self, v: Vec3) -> Vec3 {
        let det = self.det();
        Vec3::new((self.d * v.x - self.c * v.y) / det, (-self.b * v.x + self.a * v.y) / det, 0.0)
    }

    /// J^{−1} v.
    fn inv(&self, v: Vec3) -> Vec3 {
        let det = self.det();
        Vec3::new((self.d * v.x - self.b * v.y) / det, (-self.c * v.x + self.a * v.y) / det, 0.0)
    }
}

impl Triangle {
    /// The triangle on `corners` (their z ignored); `None` when it is degenerate.
    pub fn new(order: Order, corners: [Point3; 3]) -> Option<Triangle> {
        let flat = corners.map(|p| p.with(2, 0.0));
        let twice = (flat[1] - flat[0]).cross(flat[2] - flat[0]).z;
        if !(twice.abs() > 0.0 && twice.is_finite()) {
            return None;
        }
        // ∇λ_i = ẑ × (p_{i+2} − p_{i+1}) / 2A, indices cyclic.
        let grads = [0, 1, 2].map(|i| Vec3::unit(2).cross(flat[(i + 2) % 3] - flat[(i + 1) % 3]) / twice);
        Some(Triangle { order, corners: flat, grads, area: 0.5 * twice.abs(), bend: None })
    }

    /// The triangle with its edges' midpoints moved to `mids` (in [`TRI_EDGES`] order, `None` for
    /// a straight edge); `None` when the map folds (its Jacobian changes sign inside).
    pub fn curved(self, mids: [Option<Point3>; 3]) -> Option<Triangle> {
        if mids.iter().all(Option::is_none) {
            return Some(self);
        }
        let mut d = [Vec3::ZERO; 3];
        for ((k, [i, j]), m) in TRI_EDGES.iter().enumerate().zip(mids) {
            if let Some(m) = m {
                d[k] = (m.with(2, 0.0) - self.corners[*i].midpoint(self.corners[*j])).with(2, 0.0);
            }
        }
        if !d.iter().all(|v| v.is_finite()) {
            return None;
        }
        let t = Triangle { bend: Some(d), ..self };
        // det DΦ > 0 at the corners, the midpoints and the centroid: no fold.
        let probes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.5, 0.5, 0.0], [0.5, 0.0, 0.5], [0.0, 0.5, 0.5], [1.0 / 3.0; 3]];
        probes.iter().all(|l| t.jacobian(*l).det() > 1e-3).then_some(t)
    }

    /// DΦ = I + Σ_edges d ⊗ ∇(4λ_aλ_b).
    fn jacobian(&self, l: [f64; 3]) -> Jac {
        let Some(d) = &self.bend else { return Jac { a: 1.0, b: 0.0, c: 0.0, d: 1.0 } };
        TRI_EDGES.iter().zip(d).fold(Jac { a: 1.0, b: 0.0, c: 0.0, d: 1.0 }, |j, ([p, q], dk)| {
            let g = (self.grads[*q] * l[*p] + self.grads[*p] * l[*q]) * 4.0;
            Jac { a: j.a + dk.x * g.x, b: j.b + dk.x * g.y, c: j.c + dk.y * g.x, d: j.d + dk.y * g.y }
        })
    }

    /// Vector functions per triangle.
    pub fn vector_count(&self) -> usize {
        match self.order {
            Order::One => 3,
            Order::Two => 8,
        }
    }

    /// Scalar functions per triangle.
    pub fn scalar_count(&self) -> usize {
        match self.order {
            Order::One => 3,
            Order::Two => 6,
        }
    }

    /// The point at barycentric coordinates `l` (through the map of a curved triangle).
    pub fn point(&self, l: [f64; 3]) -> Point3 {
        let straight = self.straight_point(l);
        match &self.bend {
            None => straight,
            Some(d) => straight + TRI_EDGES.iter().zip(d).fold(Vec3::ZERO, |s, ([i, j], dk)| s + *dk * (4.0 * l[*i] * l[*j])),
        }
    }

    fn straight_point(&self, l: [f64; 3]) -> Point3 {
        Point3::ORIGIN + (0..3).map(|i| self.corners[i].to_vec() * l[i]).fold(Vec3::ZERO, |a, b| a + b)
    }

    fn straight_barycentric(&self, p: Point3) -> [f64; 3] {
        let d = p.with(2, 0.0) - self.corners[0];
        let (l1, l2) = (self.grads[1].dot(d), self.grads[2].dot(d));
        [1.0 - l1 - l2, l1, l2]
    }

    /// The barycentric coordinates of a point of the plane (of its preimage, for a curved
    /// triangle: Newton's method on the map, a few steps from the straight guess).
    pub fn barycentric(&self, p: Point3) -> [f64; 3] {
        let mut l = self.straight_barycentric(p);
        if self.bend.is_some() {
            let target = p.with(2, 0.0);
            for _ in 0..8 {
                let r = self.point(l) - target;
                if r.norm() <= 1e-14 * self.area.sqrt() {
                    break;
                }
                // x̂ ← x̂ − DΦ⁻¹ (Φ(x̂) − p), in barycentric terms through the straight triangle.
                let step = self.jacobian(l).inv(r);
                let x = self.straight_point(l) - step;
                l = self.straight_barycentric(x);
            }
        }
        l
    }

    fn whitney(&self, l: [f64; 3], [i, j]: [usize; 2]) -> Vec3 {
        self.grads[j] * l[i] - self.grads[i] * l[j]
    }

    /// The vector functions at `l`.
    pub fn values(&self, l: [f64; 3]) -> Vec<Vec3> {
        let v = self.straight_values(l);
        if self.bend.is_none() {
            return v;
        }
        let j = self.jacobian(l);
        v.into_iter().map(|x| j.inv_t(x)).collect()
    }

    fn straight_values(&self, l: [f64; 3]) -> Vec<Vec3> {
        let w = TRI_EDGES.map(|e| self.whitney(l, e));
        match self.order {
            Order::One => w.to_vec(),
            Order::Two => {
                let g = TRI_EDGES.map(|[i, j]| self.grads[j] * l[i] + self.grads[i] * l[j]);
                // λ₀ w₁₂ and λ₁ w₂₀ (w₂₀ = −w₀₂).
                let faces = [w[2] * l[0], -w[1] * l[1]];
                w.into_iter().chain(g).chain(faces).collect()
            }
        }
    }

    /// Their curls (z components).
    pub fn curls(&self, l: [f64; 3]) -> Vec<f64> {
        let c = self.straight_curls(l);
        if self.bend.is_none() {
            return c;
        }
        let det = self.jacobian(l).det();
        c.into_iter().map(|x| x / det).collect()
    }

    fn straight_curls(&self, l: [f64; 3]) -> Vec<f64> {
        let cross = |a: Vec3, b: Vec3| a.cross(b).z;
        // curl w_ij = 2 ∇λ_i × ∇λ_j; curl(λ w) = ∇λ × w + λ curl w.
        let cw = TRI_EDGES.map(|[i, j]| 2.0 * cross(self.grads[i], self.grads[j]));
        match self.order {
            Order::One => cw.to_vec(),
            Order::Two => {
                let w = TRI_EDGES.map(|e| self.whitney(l, e));
                let faces = [cross(self.grads[0], w[2]) + l[0] * cw[2], -(cross(self.grads[1], w[1]) + l[1] * cw[1])];
                cw.into_iter().chain([0.0; 3]).chain(faces).collect()
            }
        }
    }

    /// The scalar functions at `l`.
    pub fn scalars(&self, l: [f64; 3]) -> Vec<f64> {
        let edges = TRI_EDGES.map(|[i, j]| l[i] * l[j]);
        match self.order {
            Order::One => l.to_vec(),
            Order::Two => l.into_iter().chain(edges).collect(),
        }
    }

    /// Their gradients.
    pub fn scalar_gradients(&self, l: [f64; 3]) -> Vec<Vec3> {
        let g = self.straight_gradients(l);
        if self.bend.is_none() {
            return g;
        }
        let j = self.jacobian(l);
        g.into_iter().map(|x| j.inv_t(x)).collect()
    }

    fn straight_gradients(&self, l: [f64; 3]) -> Vec<Vec3> {
        match self.order {
            Order::One => self.grads.to_vec(),
            Order::Two => self.grads.into_iter().chain(TRI_EDGES.map(|[i, j]| self.grads[j] * l[i] + self.grads[i] * l[j])).collect(),
        }
    }

    /// A quadrature rule on the triangle: barycentric points and weights (summing to the area),
    /// exact for the products of two functions of its order with a constant coefficient.
    /// (A curved triangle's integrands are rational: two more points per direction.)
    pub fn rule(&self) -> Vec<([f64; 3], f64)> {
        let n = match self.order {
            Order::One => 2,
            Order::Two => 3,
        } + if self.bend.is_some() { 2 } else { 0 };
        crate::quadrature::triangle_rule(n).into_iter().map(|(l, w)| (l, w * self.area * self.jacobian(l).det())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri(order: Order) -> Triangle {
        Triangle::new(order, [Point3::new(0.1, -0.2, 0.0), Point3::new(1.3, 0.1, 0.0), Point3::new(0.4, 0.9, 0.0)]).unwrap()
    }

    #[test]
    fn whitney_functions_are_dual_to_the_edges() {
        let t = tri(Order::One);
        for (k, [a, b]) in TRI_EDGES.iter().enumerate() {
            let tangent = t.corners[*b] - t.corners[*a];
            for m in 0..3 {
                // ∫ w_m · t̂ along edge k by Simpson (w is linear along it).
                let at = |s: f64| {
                    let mut l = [0.0; 3];
                    l[*a] = 1.0 - s;
                    l[*b] = s;
                    t.values(l)[m].dot(tangent)
                };
                let line = (at(0.0) + 4.0 * at(0.5) + at(1.0)) / 6.0;
                assert!((line - if k == m { 1.0 } else { 0.0 }).abs() < 1e-14, "{k} {m}: {line}");
            }
        }
    }

    #[test]
    fn curls_and_gradients_match_finite_differences() {
        for order in [Order::One, Order::Two] {
            let t = tri(order);
            let l = [0.2, 0.5, 0.3];
            let p = t.point(l);
            let h = 1e-6;
            let at = |q: Point3| t.values(t.barycentric(q));
            let (px, mx, py, my) = (at(p + Vec3::unit(0) * h), at(p - Vec3::unit(0) * h), at(p + Vec3::unit(1) * h), at(p - Vec3::unit(1) * h));
            for (i, c) in t.curls(l).iter().enumerate() {
                // curl = ∂v_y/∂x − ∂v_x/∂y.
                let fd = (px[i].y - mx[i].y - py[i].x + my[i].x) / (2.0 * h);
                assert!((fd - c).abs() < 1e-7 * (1.0 + c.abs()), "{order:?} {i}: {fd} {c}");
            }
            let s = |q: Point3| t.scalars(t.barycentric(q));
            let (sx, sxm, sy, sym) = (s(p + Vec3::unit(0) * h), s(p - Vec3::unit(0) * h), s(p + Vec3::unit(1) * h), s(p - Vec3::unit(1) * h));
            for (i, g) in t.scalar_gradients(l).iter().enumerate() {
                let fd = Vec3::new((sx[i] - sxm[i]) / (2.0 * h), (sy[i] - sym[i]) / (2.0 * h), 0.0);
                assert!((fd - *g).norm() < 1e-7, "{order:?} {i}");
            }
            // The gradients of the scalar space lie in the vector space: at order 2 the edge
            // gradients are its functions 3..6; at order 1, ∇λ₀ = −(w₀₁ + w₀₂).
            let v = t.values(l);
            let g = t.scalar_gradients(l);
            match order {
                Order::One => assert!((g[0] + v[0] + v[1]).norm() < 1e-14),
                Order::Two => (0..3).for_each(|k| assert!((g[3 + k] - v[3 + k]).norm() < 1e-14)),
            }
            assert!((t.rule().iter().map(|r| r.1).sum::<f64>() - t.area).abs() < 1e-14);
        }
        assert!(Triangle::new(Order::One, [Point3::ORIGIN; 3]).is_none());
    }

    #[test]
    fn a_curved_triangle_maps_its_functions_by_piola() {
        // One edge bowed outwards: the map's points and its inverse agree, and the functions'
        // curls and gradients still match finite differences in the physical plane.
        let straight = tri(Order::Two);
        let bowed = straight.corners[1].midpoint(straight.corners[2]) + Vec3::new(0.08, 0.05, 0.0);
        let t = straight.curved([None, None, Some(bowed)]).unwrap();
        assert!((t.point([0.0, 0.5, 0.5]) - bowed).norm() < 1e-15);
        let l = [0.2, 0.5, 0.3];
        let p = t.point(l);
        let back = t.barycentric(p);
        assert!(back.iter().zip(&l).all(|(a, b)| (a - b).abs() < 1e-12), "{back:?}");
        let h = 1e-6;
        let at = |q: Point3| t.values(t.barycentric(q));
        let (px, mx, py, my) = (at(p + Vec3::unit(0) * h), at(p - Vec3::unit(0) * h), at(p + Vec3::unit(1) * h), at(p - Vec3::unit(1) * h));
        for (i, c) in t.curls(l).iter().enumerate() {
            let fd = (px[i].y - mx[i].y - py[i].x + my[i].x) / (2.0 * h);
            assert!((fd - c).abs() < 1e-6 * (1.0 + c.abs()), "{i}: {fd} {c}");
        }
        let s = |q: Point3| t.scalars(t.barycentric(q));
        let (sx, sxm, sy, sym) = (s(p + Vec3::unit(0) * h), s(p - Vec3::unit(0) * h), s(p + Vec3::unit(1) * h), s(p - Vec3::unit(1) * h));
        for (i, g) in t.scalar_gradients(l).iter().enumerate() {
            let fd = Vec3::new((sx[i] - sxm[i]) / (2.0 * h), (sy[i] - sym[i]) / (2.0 * h), 0.0);
            assert!((fd - *g).norm() < 1e-6, "{i}");
        }
        // The area grows by the bowed edge's parabolic segment, (2/3) |chord| × the bow normal
        // to it (the straight triangle's corners run anticlockwise, the bow is outwards).
        let chord = straight.corners[2] - straight.corners[1];
        let bow = Vec3::new(0.08, 0.05, 0.0).dot(Vec3::new(chord.y, -chord.x, 0.0) / chord.norm());
        let area: f64 = t.rule().iter().map(|r| r.1).sum();
        assert!((area - (straight.area + 2.0 / 3.0 * chord.norm() * bow)).abs() < 1e-12, "{area}");
        // A bow folding the triangle over is refused.
        assert!(straight.curved([None, None, Some(straight.corners[0])]).is_none());
    }
}

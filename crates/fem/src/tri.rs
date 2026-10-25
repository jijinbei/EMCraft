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
        Some(Triangle { order, corners: flat, grads, area: 0.5 * twice.abs() })
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

    /// The point at barycentric coordinates `l`.
    pub fn point(&self, l: [f64; 3]) -> Point3 {
        Point3::ORIGIN + (0..3).map(|i| self.corners[i].to_vec() * l[i]).fold(Vec3::ZERO, |a, b| a + b)
    }

    /// The barycentric coordinates of a point of the plane.
    pub fn barycentric(&self, p: Point3) -> [f64; 3] {
        let d = p.with(2, 0.0) - self.corners[0];
        let (l1, l2) = (self.grads[1].dot(d), self.grads[2].dot(d));
        [1.0 - l1 - l2, l1, l2]
    }

    fn whitney(&self, l: [f64; 3], [i, j]: [usize; 2]) -> Vec3 {
        self.grads[j] * l[i] - self.grads[i] * l[j]
    }

    /// The vector functions at `l`.
    pub fn values(&self, l: [f64; 3]) -> Vec<Vec3> {
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
        match self.order {
            Order::One => self.grads.to_vec(),
            Order::Two => self.grads.into_iter().chain(TRI_EDGES.map(|[i, j]| self.grads[j] * l[i] + self.grads[i] * l[j])).collect(),
        }
    }

    /// A quadrature rule on the triangle: barycentric points and weights (summing to the area),
    /// exact for the products of two functions of its order with a constant coefficient.
    pub fn rule(&self) -> Vec<([f64; 3], f64)> {
        let n = match self.order {
            Order::One => 2,
            Order::Two => 3,
        };
        crate::quadrature::triangle_rule(n).into_iter().map(|(l, w)| (l, w * self.area)).collect()
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
}

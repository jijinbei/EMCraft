//! The trilinear hexahedron's certificate and metrics without allocation.
//!
//! The same computation as the generic path ([`super::bernstein`], [`super::jacobian`]) for the
//! one domain it is the bottleneck on (a projected block mesh is a million hex8 elements): three
//! segment factors of degree at most 2, so every polynomial fits in 27 coefficients on the
//! stack. Every operation is the generic one in the same order (the same products, sums and
//! skipped zero terms), so the results are the same bit for bit; a test compares the two on
//! valid, curved, inverted and degenerate elements. Only elements the first bounds settle are
//! decided here (every element of a smooth mesh): an element that needs subdivision returns
//! `None` and goes through the generic certificate.

use super::jacobian::{HEX_CORNERS, JacobianCheck, RELATIVE_TOLERANCE, Validity};
use super::shape;

/// Coefficients of a polynomial of degree at most 2 in each of three variables.
const LEN: usize = 27;

fn factorial(n: usize) -> f64 {
    (2..=n).map(|k| k as f64).product()
}

/// A polynomial in Bernstein form on the unit cube, degree `deg[f]` along factor `f`, stored as
/// [`super::bernstein::Bernstein`] stores it (`((m0 · (d1 + 1)) + m1) · (d2 + 1) + m2`).
#[derive(Clone, Copy)]
struct Poly {
    deg: [usize; 3],
    coef: [f64; LEN],
}

impl Poly {
    fn zero(deg: [usize; 3]) -> Self {
        Poly { deg, coef: [0.0; LEN] }
    }

    fn slot(&self, m: [usize; 3]) -> usize {
        (m[0] * (self.deg[1] + 1) + m[1]) * (self.deg[2] + 1) + m[2]
    }

    fn get(&self, m: [usize; 3]) -> f64 {
        self.coef.get(self.slot(m)).copied().unwrap_or(0.0)
    }

    /// Every multi-index, in storage order.
    fn multis(&self) -> impl Iterator<Item = [usize; 3]> + use<> {
        let d = self.deg;
        (0..=d[0]).flat_map(move |a| (0..=d[1]).flat_map(move |b| (0..=d[2]).map(move |c| [a, b, c])))
    }

    /// The multinomial weight of `m`, computed as the generic path computes it.
    fn weight(&self, m: [usize; 3]) -> f64 {
        let mut w = 1.0;
        for (&d, k) in self.deg.iter().zip(m) {
            w *= factorial(d);
            w /= factorial(d - k);
            w /= factorial(k);
        }
        w
    }

    fn mul(&self, o: &Poly) -> Poly {
        let mut out = Poly::zero([0, 1, 2].map(|f| self.deg[f] + o.deg[f]));
        let mut out_w = [0.0; LEN];
        for m in out.multis() {
            let s = out.slot(m);
            if let Some(x) = out_w.get_mut(s) {
                *x = out.weight(m);
            }
        }
        let terms = |p: &Poly| {
            let mut t = [([0usize; 3], 0.0f64); LEN];
            let mut n = 0;
            for m in p.multis() {
                let c = p.get(m) * p.weight(m);
                if c != 0.0
                    && let Some(slot) = t.get_mut(n)
                {
                    *slot = (m, c);
                    n += 1;
                }
            }
            (t, n)
        };
        let (left, nl) = terms(self);
        let (right, nr) = terms(o);
        for (a, ca) in left.iter().take(nl) {
            for (b, cb) in right.iter().take(nr) {
                let s = out.slot([a[0] + b[0], a[1] + b[1], a[2] + b[2]]);
                if let (Some(c), Some(w)) = (out.coef.get_mut(s), out_w.get(s)) {
                    *c += ca * cb / w;
                }
            }
        }
        out
    }

    fn axpy(&self, s: f64, o: &Poly) -> Poly {
        let mut out = *self;
        for (c, b) in out.coef.iter_mut().zip(&o.coef) {
            *c += s * b;
        }
        out
    }

    /// The derivative along factor `f`.
    fn derivative(&self, f: usize) -> Poly {
        let mut deg = self.deg;
        deg[f] -= 1;
        let mut out = Poly::zero(deg);
        for b in out.multis() {
            let mut hi = b;
            hi[f] += 1;
            let v = self.deg[f] as f64 * (self.get(hi) - self.get(b));
            let s = out.slot(b);
            if let Some(c) = out.coef.get_mut(s) {
                *c += v;
            }
        }
        out
    }

    fn bounds(&self) -> (f64, f64) {
        self.multis().map(|m| self.get(m)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| (lo.min(c), hi.max(c)))
    }

    /// The value at a corner of the cube (0 or 1 per factor).
    fn at_vertex(&self, v: [usize; 3]) -> f64 {
        self.get([0, 1, 2].map(|f| if v[f] > 0 { self.deg[f] } else { 0 }))
    }

    fn integral(&self) -> f64 {
        let per: f64 = self.deg.iter().map(|&d| factorial(d) / factorial(d + 1)).product();
        self.multis().map(|m| self.get(m)).sum::<f64>() * per
    }

    /// The value at the centre of the cube.
    fn centre(&self) -> f64 {
        self.multis()
            .map(|m| {
                let mut t = self.get(m) * self.weight(m);
                for (&d, k) in self.deg.iter().zip(m) {
                    for c in [d - k, k] {
                        t *= 0.5f64.powi(c as i32);
                    }
                }
                t
            })
            .sum()
    }
}

/// The certificate and Knupp's scaled Jacobian and shape of a hex8 with corners `p` (already
/// translated so the first is at the origin, as the generic path does), when the first bounds of
/// its determinant settle it; `None` when it needs subdivision or a value is not finite.
pub(crate) fn hex8(p: &[crate::Point3; 8]) -> Option<(JacobianCheck, f64, f64)> {
    let mut g = [Poly::zero([1, 1, 1]); 3];
    for (corner, x) in HEX_CORNERS.iter().zip(p) {
        for (poly, c) in g.iter_mut().zip(x.iter()) {
            let s = poly.slot(*corner);
            *poly.coef.get_mut(s)? = c;
        }
    }
    let columns: [[Poly; 3]; 3] = [0, 1, 2].map(|f| g.map(|q| q.derivative(f)));
    let [a, b, c] = &columns;
    let cross = |i: usize, j: usize| b[i].mul(&c[j]).axpy(-1.0, &b[j].mul(&c[i]));
    let det = a[0].mul(&cross(1, 2)).axpy(1.0, &a[1].mul(&cross(2, 0))).axpy(1.0, &a[2].mul(&cross(0, 1)));

    let norm_bound = |col: &[Poly; 3]| {
        let [x, y, z] = col;
        x.multis().map(|m| (x.get(m), y.get(m), z.get(m))).map(|(a, b, c)| (a * a + b * b + c * c).sqrt()).fold(0.0, f64::max)
    };
    let n: f64 = columns.iter().map(norm_bound).product();
    let floor = 2.0 * crate::compat::MIN_NORMALIZED_JACOBIAN * n;

    let (lo, hi) = det.bounds();
    let volume = det.integral();
    let scale = lo.abs().max(hi.abs());
    if !scale.is_finite() || !volume.is_finite() || !floor.is_finite() {
        return None;
    }
    let tol = (RELATIVE_TOLERANCE * scale).max(floor);
    let ratio_of = |v: f64| if scale > 0.0 { v / scale } else { 0.0 };
    let check = if HEX_CORNERS.iter().any(|v| !(det.at_vertex(*v) > tol)) {
        JacobianCheck { validity: Validity::Invalid, ratio: ratio_of(lo), volume }
    } else if lo > tol {
        JacobianCheck { validity: Validity::Valid, ratio: ratio_of(f64::INFINITY.min(lo)), volume }
    } else {
        return None;
    };

    let at = |value: &dyn Fn(&Poly) -> f64| {
        let [a, b, c] = columns.each_ref().map(|col| col.each_ref().map(value).into());
        shape::jacobian_metrics(a, b, c)
    };
    let centre = at(&|q| q.centre());
    let (scaled, knupp) = HEX_CORNERS.iter().map(|corner| at(&|q| q.at_vertex(*corner))).fold(centre, |(s, k), (s2, k2)| (s.min(s2), k.min(k2)));
    Some((check, scaled, knupp))
}

#[cfg(test)]
mod tests {
    use crate::compat::ElementKind;

    use super::super::{element_validity, generic_quality, local};
    use super::*;

    /// A pseudo-random number in [-1, 1) (deterministic).
    fn noise(seed: &mut u64) -> f64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*seed >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
    }

    /// Hexahedra of every kind the certificate meets: near-cubes, sheared, curved (twisted top),
    /// inverted, collapsed, far from the origin and tiny.
    fn samples() -> Vec<[[f64; 3]; 8]> {
        let mut seed = 409;
        let mut out = Vec::new();
        for k in 0..4000 {
            let jitter = [0.02, 0.2, 0.45, 0.9][k % 4];
            let (offset, size) = match k % 5 {
                0 => (0.0, 1.0),
                1 => (1e6, 1.0),
                2 => (-3.5, 1e-6),
                3 => (12.0, 250.0),
                _ => (0.25, 0.1),
            };
            let mut x = HEX_CORNERS.map(|c| c.map(|v| offset + size * (v as f64 + jitter * 0.5 * noise(&mut seed))));
            match k % 7 {
                // A twisted top face: positive corners, possibly negative inside.
                0 => {
                    for q in x.iter_mut().skip(4) {
                        let (s, t) = (q[0] - offset, q[1] - offset);
                        let a = 1.2 * noise(&mut seed).abs();
                        q[0] = offset + s * a.cos() - t * a.sin();
                        q[1] = offset + s * a.sin() + t * a.cos();
                    }
                }
                // Inverted.
                1 => x.swap(0, 4),
                // A collapsed edge.
                2 => x[1] = x[0],
                _ => {}
            }
            out.push(x);
        }
        out
    }

    /// The fast path agrees with the generic Bernstein path bit for bit wherever it decides,
    /// and decides valid and invalid elements alike.
    #[test]
    fn matches_the_generic_certificate_bit_for_bit() {
        let (mut valid, mut invalid, mut deferred) = (0, 0, 0);
        for x in samples() {
            let x = crate::vec3::pts(x);
            let fast = super::super::element_quality(ElementKind::Hex8, &x);
            let p = local(ElementKind::Hex8, &x).unwrap();
            let generic = generic_quality(ElementKind::Hex8, &x, &p);
            assert_eq!(format!("{fast:?}"), format!("{generic:?}"), "{x:?}");
            let checks = (element_validity(ElementKind::Hex8, &x).ok(), generic.as_ref().ok().and_then(|q| q.jacobian));
            assert_eq!(format!("{:?}", checks.0), format!("{:?}", checks.1), "{x:?}");
            let c: [crate::Point3; 8] = p.try_into().unwrap();
            match hex8(&c).map(|r| r.0.validity) {
                Some(Validity::Valid) => valid += 1,
                Some(_) => invalid += 1,
                None => deferred += 1,
            }
        }
        assert!(valid > 1000 && invalid > 500 && deferred > 0, "valid {valid}, invalid {invalid}, deferred {deferred}");
    }
}

//! Vector polynomials in the barycentric coordinates of a tetrahedron: sums of terms
//! `c · λ₀^a λ₁^b λ₂^c λ₃^d` with a vector coefficient `c`. Products of two of them integrate
//! exactly over the tetrahedron by the formula
//!
//! ∫_T λ₀^a λ₁^b λ₂^c λ₃^d dV = 3! V a! b! c! d! / (a + b + c + d + 3)!
//!
//! (e.g. O. C. Zienkiewicz, R. L. Taylor, *The Finite Element Method*, vol. 1, 5th ed. 2000,
//! §8.8; Z. J. Cendes, *Vector finite elements for electromagnetic field computation*, IEEE
//! Trans. Magn. 27(5):3958–3966, 1991), so element matrices need no quadrature.

pub type Vec3 = [f64; 3];

pub fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn scale(a: Vec3, s: f64) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// One term: `coef · Π λᵢ^exp[i]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Term {
    pub exp: [u8; 4],
    pub coef: Vec3,
}

/// A vector polynomial in barycentric coordinates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VecPoly {
    pub terms: Vec<Term>,
}

const FACTORIAL: [f64; 13] = {
    let mut f = [1.0; 13];
    let mut i = 1;
    while i < 13 {
        f[i] = f[i - 1] * i as f64;
        i += 1;
    }
    f
};

fn factorial(n: usize) -> f64 {
    FACTORIAL.get(n).copied().unwrap_or(f64::INFINITY)
}

/// ∫_T Π λᵢ^αᵢ dV over a tetrahedron of volume `volume`.
pub fn monomial_integral(alpha: [u8; 4], volume: f64) -> f64 {
    let total: usize = alpha.iter().map(|&a| a as usize).sum();
    6.0 * volume * alpha.iter().map(|&a| factorial(a as usize)).product::<f64>() / factorial(total + 3)
}

/// `Π λᵢ^exp[i]`.
pub fn monomial(exp: [u8; 4], l: [f64; 4]) -> f64 {
    let mut v = 1.0;
    for (e, x) in exp.iter().zip(l) {
        for _ in 0..*e {
            v *= x;
        }
    }
    v
}

impl VecPoly {
    /// Adds a term, merging it into one with the same exponents.
    pub fn push(&mut self, exp: [u8; 4], coef: Vec3) {
        if let Some(t) = self.terms.iter_mut().find(|t| t.exp == exp) {
            t.coef = add(t.coef, coef);
        } else {
            self.terms.push(Term { exp, coef });
        }
    }

    pub fn eval(&self, l: [f64; 4]) -> Vec3 {
        self.terms.iter().fold([0.0; 3], |acc, t| add(acc, scale(t.coef, monomial(t.exp, l))))
    }

    /// ∫_T self · other dV.
    pub fn inner(&self, other: &VecPoly, volume: f64) -> f64 {
        let mut s = 0.0;
        for a in &self.terms {
            for b in &other.terms {
                let c = dot(a.coef, b.coef);
                if c != 0.0 {
                    let mut e = a.exp;
                    for (x, y) in e.iter_mut().zip(b.exp) {
                        *x += y;
                    }
                    s += c * monomial_integral(e, volume);
                }
            }
        }
        s
    }

    /// ∫_T self_d other_d dV: one component of the inner product.
    pub fn inner_component(&self, other: &VecPoly, volume: f64, d: usize) -> f64 {
        let mut s = 0.0;
        for a in &self.terms {
            for b in &other.terms {
                let c = a.coef[d] * b.coef[d];
                if c != 0.0 {
                    let mut e = a.exp;
                    for (x, y) in e.iter_mut().zip(b.exp) {
                        *x += y;
                    }
                    s += c * monomial_integral(e, volume);
                }
            }
        }
        s
    }

    /// ∫_T self · f dV for a field `f` given at the corners and varying linearly between them.
    pub fn inner_linear(&self, corners: &[Vec3; 4], volume: f64) -> f64 {
        let mut s = 0.0;
        for a in &self.terms {
            for (k, f) in corners.iter().enumerate() {
                let mut e = a.exp;
                e[k] += 1;
                s += dot(a.coef, *f) * monomial_integral(e, volume);
            }
        }
        s
    }
}

/// A term of a basis function before the gradients are known: `c · Π λᵢ^exp[i] · ∇λ_grad`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Symbolic {
    pub coef: f64,
    pub exp: [u8; 4],
    pub grad: usize,
}

/// The vector polynomial of symbolic terms on a tetrahedron with barycentric gradients `g`.
pub fn realise(terms: &[Symbolic], g: &[Vec3; 4]) -> VecPoly {
    let mut p = VecPoly::default();
    for t in terms {
        p.push(t.exp, scale(g[t.grad], t.coef));
    }
    p
}

/// The curl of symbolic terms: curl(m(λ) ∇λ_k) = Σ_p ∂m/∂λ_p ∇λ_p × ∇λ_k.
pub fn realise_curl(terms: &[Symbolic], g: &[Vec3; 4]) -> VecPoly {
    let mut p = VecPoly::default();
    for t in terms {
        for q in 0..4 {
            if t.exp[q] == 0 || q == t.grad {
                continue;
            }
            let mut e = t.exp;
            e[q] -= 1;
            p.push(e, scale(cross(g[q], g[t.grad]), t.coef * t.exp[q] as f64));
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monomial_integrals() {
        // ∫ 1 = V, ∫ λ = V/4, ∫ λ² = V/10, ∫ λᵢλⱼ = V/20.
        assert!((monomial_integral([0; 4], 2.0) - 2.0).abs() < 1e-15);
        assert!((monomial_integral([1, 0, 0, 0], 1.0) - 0.25).abs() < 1e-15);
        assert!((monomial_integral([0, 2, 0, 0], 1.0) - 0.1).abs() < 1e-15);
        assert!((monomial_integral([0, 1, 0, 1], 1.0) - 0.05).abs() < 1e-15);
    }

    #[test]
    fn merging_and_evaluation() {
        let mut p = VecPoly::default();
        p.push([1, 0, 0, 0], [1.0, 0.0, 0.0]);
        p.push([1, 0, 0, 0], [0.0, 2.0, 0.0]);
        p.push([0, 1, 1, 0], [0.0, 0.0, 3.0]);
        assert_eq!(p.terms.len(), 2);
        assert_eq!(p.eval([0.5, 0.2, 0.3, 0.0]), [0.5, 1.0, 3.0 * 0.06]);
    }
}

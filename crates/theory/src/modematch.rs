//! Mode matching for H-plane discontinuities: a waveguide of one height b whose broad-wall
//! aperture changes from section to section (steps, inductive irises), each section filled with
//! one material. Every field is y-independent, so only the TE_m0 modes take part, with
//! E_y = sin(mπ(x − x_i)/a_i) on section i's aperture [x_i, x_i + a_i].
//!
//! **Junction** (A. Wexler, *Solution of waveguide discontinuities by modal analysis*, IEEE
//! Trans. MTT 15(9):508–517, 1967; T. Itoh (ed.), *Numerical Techniques for Microwave and
//! Millimeter-Wave Passive Structures*, Wiley 1989, ch. 9; J. Uher, J. Bornemann, U. Rosenberg,
//! *Waveguide Components for Antenna Feed Systems: Theory and CAD*, Artech House 1993). Between
//! a guide 1 and a guide 2 whose aperture lies within guide 1's, with incident amplitudes a
//! (in 1) and d (in 2) and outgoing b and c: E_y is continuous on aperture 2 and zero on the
//! rest of aperture 1 (the step's metal); H_x is continuous on aperture 2. Projected on the
//! modes of guide 1 and guide 2:
//!
//! D₁(a + b) = M(c + d),   Mᵀ Y₁(a − b) = D₂ Y₂(c − d),
//!
//! with M_mn = ∫ e_m⁽¹⁾ e_n⁽²⁾ dx over aperture 2, D_i = (a_i/2) I and Y = diag(β/μr) (the
//! modal admittances up to the common 1/ω). With W = D₂Y₂ + MᵀY₁D₁⁻¹M:
//!
//! S₂₁ = 2W⁻¹MᵀY₁, S₂₂ = 2W⁻¹D₂Y₂ − I, S₁₁ = D₁⁻¹M S₂₁ − I, S₁₂ = D₁⁻¹M(S₂₂ + I).
//!
//! The modes kept in each guide are in proportion to its width (relative convergence: R.
//! Mittra, T. Itoh, T.-S. Li, *Analytical and numerical studies of the relative convergence
//! phenomenon arising in the solution of an integral equation by the moment method*, IEEE Trans.
//! MTT 20(2):96–104, 1972).
//!
//! **Cascade.** Each section is a diagonal transmission e^{−jβ_m l} (only decaying exponentials,
//! so evanescent sections stay stable); junctions and sections are combined by the Redheffer
//! star product of generalised scattering matrices.

use std::f64::consts::PI;

use emcraft_model::units::C0;
use num_complex::Complex64 as C;

use crate::TheoryError;

/// A section of an H-plane line: its length, material and aperture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HSection {
    pub length: f64,
    pub eps_r: f64,
    pub mu_r: f64,
    /// The aperture's start and width along x (m).
    pub x0: f64,
    pub width: f64,
}

/// A waveguide of sections whose apertures nest from one to the next.
#[derive(Clone, Debug, PartialEq)]
pub struct HPlaneLine {
    pub sections: Vec<HSection>,
}

/// A dense complex matrix, by rows.
#[derive(Clone, Debug, PartialEq)]
struct Mat {
    r: usize,
    c: usize,
    v: Vec<C>,
}

const ZERO: C = C::new(0.0, 0.0);
const ONE: C = C::new(1.0, 0.0);

impl Mat {
    fn zeros(r: usize, c: usize) -> Mat {
        Mat { r, c, v: vec![ZERO; r * c] }
    }
    fn identity(n: usize) -> Mat {
        let mut m = Mat::zeros(n, n);
        for i in 0..n {
            m.v[i * n + i] = ONE;
        }
        m
    }
    fn diag(d: &[C]) -> Mat {
        let mut m = Mat::zeros(d.len(), d.len());
        for (i, x) in d.iter().enumerate() {
            m.v[i * d.len() + i] = *x;
        }
        m
    }
    fn at(&self, i: usize, j: usize) -> C {
        self.v[i * self.c + j]
    }
    fn mul(&self, o: &Mat) -> Mat {
        let mut out = Mat::zeros(self.r, o.c);
        for i in 0..self.r {
            for k in 0..self.c {
                let a = self.v[i * self.c + k];
                if a == ZERO {
                    continue;
                }
                for j in 0..o.c {
                    out.v[i * o.c + j] += a * o.v[k * o.c + j];
                }
            }
        }
        out
    }
    fn add(&self, o: &Mat, s: C) -> Mat {
        Mat { r: self.r, c: self.c, v: self.v.iter().zip(&o.v).map(|(a, b)| a + s * b).collect() }
    }
    fn transpose(&self) -> Mat {
        let mut out = Mat::zeros(self.c, self.r);
        for i in 0..self.r {
            for j in 0..self.c {
                out.v[j * self.r + i] = self.v[i * self.c + j];
            }
        }
        out
    }
    fn scale(&self, s: C) -> Mat {
        Mat { r: self.r, c: self.c, v: self.v.iter().map(|a| a * s).collect() }
    }
    /// Solves self · X = b by Gaussian elimination with partial pivoting.
    fn solve(&self, b: &Mat) -> Result<Mat, TheoryError> {
        let n = self.r;
        let (mut a, mut x) = (self.clone(), b.clone());
        for k in 0..n {
            let p = (k..n).max_by(|&i, &j| a.at(i, k).norm().total_cmp(&a.at(j, k).norm())).unwrap_or(k);
            if !(a.at(p, k).norm() > 0.0) {
                return Err(TheoryError::Invalid("singular mode-matching system".into()));
            }
            if p != k {
                for j in 0..n {
                    a.v.swap(k * n + j, p * n + j);
                }
                for j in 0..x.c {
                    x.v.swap(k * x.c + j, p * x.c + j);
                }
            }
            let piv = a.at(k, k);
            for i in k + 1..n {
                let f = a.at(i, k) / piv;
                if f == ZERO {
                    continue;
                }
                for j in k..n {
                    let t = a.v[k * n + j];
                    a.v[i * n + j] -= f * t;
                }
                for j in 0..x.c {
                    let t = x.v[k * x.c + j];
                    x.v[i * x.c + j] -= f * t;
                }
            }
        }
        for k in (0..n).rev() {
            for j in 0..x.c {
                let mut s = x.v[k * x.c + j];
                for i in k + 1..n {
                    s -= a.v[k * n + i] * x.v[i * x.c + j];
                }
                x.v[k * x.c + j] = s / a.v[k * n + k];
            }
        }
        Ok(x)
    }
}

/// A generalised scattering matrix between a left and a right port group.
#[derive(Clone, Debug)]
struct Gsm {
    ll: Mat,
    lr: Mat,
    rl: Mat,
    rr: Mat,
}

impl Gsm {
    /// A uniform section: transmission `p` both ways.
    fn line(p: &[C]) -> Gsm {
        let n = p.len();
        Gsm { ll: Mat::zeros(n, n), lr: Mat::diag(p), rl: Mat::diag(p), rr: Mat::zeros(n, n) }
    }

    /// The Redheffer star product: self on the left, `b` on the right.
    fn star(&self, b: &Gsm) -> Result<Gsm, TheoryError> {
        let n = self.rr.r;
        // (I − B_ll A_rr)⁻¹ and (I − A_rr B_ll)⁻¹.
        let i_n = Mat::identity(n);
        let u = i_n.add(&b.ll.mul(&self.rr), -ONE);
        let v = i_n.add(&self.rr.mul(&b.ll), -ONE);
        let ub = u.solve(&b.ll.mul(&self.rl))?; // (I − B_ll A_rr)⁻¹ B_ll A_rl
        let ul = u.solve(&b.lr)?; // (I − B_ll A_rr)⁻¹ B_lr
        let va = v.solve(&self.rl)?; // (I − A_rr B_ll)⁻¹ A_rl
        let vr = v.solve(&self.rr.mul(&b.lr))?; // (I − A_rr B_ll)⁻¹ A_rr B_lr
        Ok(Gsm { ll: self.ll.add(&self.lr.mul(&ub), ONE), lr: self.lr.mul(&ul), rl: b.rl.mul(&va), rr: b.rr.add(&b.rl.mul(&vr), ONE) })
    }
}

/// ∫_u^v cos(p x + q) dx.
fn int_cos(p: f64, q: f64, u: f64, v: f64) -> f64 {
    if p.abs() * (v - u).abs() < 1e-12 { (v - u) * q.cos() } else { ((p * v + q).sin() - (p * u + q).sin()) / p }
}

/// M_mn = ∫ sin(mπ(x − x1)/a1) sin(nπ(x − x2)/a2) dx over aperture 2 (within aperture 1).
fn coupling(n1: usize, x1: f64, a1: f64, n2: usize, x2: f64, a2: f64) -> Mat {
    let mut m = Mat::zeros(n1, n2);
    for i in 0..n1 {
        let k1 = (i + 1) as f64 * PI / a1;
        for j in 0..n2 {
            let k2 = (j + 1) as f64 * PI / a2;
            // sin A sin B = ½[cos(A − B) − cos(A + B)], A = k1(x − x1), B = k2(x − x2).
            let d = int_cos(k1 - k2, -k1 * x1 + k2 * x2, x2, x2 + a2);
            let s = int_cos(k1 + k2, -k1 * x1 - k2 * x2, x2, x2 + a2);
            m.v[i * n2 + j] = C::new(0.5 * (d - s), 0.0);
        }
    }
    m
}

/// The junction of a wide guide (1) and a guide (2) whose aperture lies within it: the GSM with
/// guide 1 on the left.
fn junction(n1: usize, s1: &HSection, y1: &[C], n2: usize, s2: &HSection, y2: &[C]) -> Result<Gsm, TheoryError> {
    let m = coupling(n1, s1.x0, s1.width, n2, s2.x0, s2.width);
    let d1 = C::new(s1.width / 2.0, 0.0);
    let d2y2 = Mat::diag(&y2.iter().map(|y| y * (s2.width / 2.0)).collect::<Vec<_>>());
    let mt_y1 = m.transpose().mul(&Mat::diag(y1));
    let w = d2y2.add(&mt_y1.mul(&m).scale(d1.inv()), ONE);
    let s21 = w.solve(&mt_y1.scale(C::new(2.0, 0.0)))?;
    let s22 = w.solve(&d2y2.scale(C::new(2.0, 0.0)))?.add(&Mat::identity(n2), -ONE);
    let md = m.scale(d1.inv());
    let s11 = md.mul(&s21).add(&Mat::identity(n1), -ONE);
    let s12 = md.mul(&s22.add(&Mat::identity(n2), ONE));
    Ok(Gsm { ll: s11, lr: s12, rl: s21, rr: s22 })
}

impl HPlaneLine {
    /// S₁₁ and S₂₁ of the TE₁₀ wave (power waves, referred to the end faces) with `modes` TE_m0
    /// modes in the widest section and proportionally fewer in narrower ones.
    pub fn solve(&self, frequency: f64, modes: usize) -> Result<(C, C), TheoryError> {
        if !(frequency.is_finite() && frequency > 0.0) {
            return Err(TheoryError::Invalid(format!("frequency {frequency}")));
        }
        if self.sections.is_empty() || !(1..=2000).contains(&modes) {
            return Err(TheoryError::Invalid("line or mode count".into()));
        }
        for s in &self.sections {
            if ![s.length, s.eps_r, s.mu_r, s.width].iter().all(|v| v.is_finite() && *v > 0.0) || !s.x0.is_finite() {
                return Err(TheoryError::Invalid(format!("section {s:?}")));
            }
        }
        let widest = self.sections.iter().map(|s| s.width).fold(0.0, f64::max);
        let k0 = 2.0 * PI * frequency / C0;
        let count = |s: &HSection| ((modes as f64 * s.width / widest).round() as usize).max(1);
        // β (Im ≤ 0) and Y = β/μr of each section's modes.
        let props: Vec<(Vec<C>, Vec<C>)> = self
            .sections
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let k2 = k0 * k0 * s.eps_r * s.mu_r;
                let beta: Vec<C> = (1..=count(s))
                    .map(|m| {
                        let d = k2 - (m as f64 * PI / s.width).powi(2);
                        if d >= 0.0 { C::new(d.sqrt(), 0.0) } else { C::new(0.0, -(-d).sqrt()) }
                    })
                    .collect();
                if beta.first().is_some_and(|b| b.norm() < 1e-9 * PI / s.width) {
                    return Err(TheoryError::Cutoff(i));
                }
                let y = beta.iter().map(|b| b / s.mu_r).collect();
                Ok((beta, y))
            })
            .collect::<Result<_, _>>()?;
        let travel = |i: usize| -> Vec<C> { props[i].0.iter().map(|b| (C::new(0.0, -1.0) * b * self.sections[i].length).exp()).collect() };
        let mut total = Gsm::line(&travel(0));
        let tol = 1e-9 * widest;
        for i in 1..self.sections.len() {
            let (a, b) = (&self.sections[i - 1], &self.sections[i]);
            let (na, nb) = (count(a), count(b));
            let holds = |p: &HSection, q: &HSection| p.x0 <= q.x0 + tol && q.x0 + q.width <= p.x0 + p.width + tol;
            let j = if holds(a, b) {
                junction(na, a, &props[i - 1].1, nb, b, &props[i].1)?
            } else if holds(b, a) {
                // The wide guide on the right: the same junction seen from the other side.
                let g = junction(nb, b, &props[i].1, na, a, &props[i - 1].1)?;
                Gsm { ll: g.rr, lr: g.rl, rl: g.lr, rr: g.ll }
            } else {
                return Err(TheoryError::Invalid(format!("sections {i} and {}: apertures overlap only in part", i + 1)));
            };
            total = total.star(&j)?.star(&Gsm::line(&travel(i)))?;
        }
        let (first, last) = (0, self.sections.len() - 1);
        let power = |i: usize| self.sections[i].width * props[i].1[0];
        let factor = (power(last) / power(first)).sqrt();
        Ok((total.ll.at(0, 0), total.rl.at(0, 0) * factor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Layer, Te10Line};

    const A: f64 = 0.02286;

    fn full(length: f64, eps_r: f64) -> HSection {
        HSection { length, eps_r, mu_r: 1.0, x0: 0.0, width: A }
    }

    fn iris(length: f64, width: f64) -> HSection {
        HSection { length, eps_r: 1.0, mu_r: 1.0, x0: (A - width) / 2.0, width }
    }

    #[test]
    fn equal_apertures_reduce_to_the_te10_cascade() {
        let line = HPlaneLine { sections: vec![full(0.015, 2.0), full(0.02, 1.0), full(0.015, 2.0)] };
        let exact = Te10Line {
            a: A,
            layers: vec![
                Layer { length: 0.015, eps_r: 2.0, mu_r: 1.0 },
                Layer { length: 0.02, eps_r: 1.0, mu_r: 1.0 },
                Layer { length: 0.015, eps_r: 2.0, mu_r: 1.0 },
            ],
        };
        for f in [5.0e9, 8.0e9] {
            let (s11, s21) = line.solve(f, 12).unwrap();
            let (e11, e21) = exact.solve(f).unwrap().s_power();
            assert!((s11 - e11).norm() < 1e-12 && (s21 - e21).norm() < 1e-12, "{f}");
        }
    }

    #[test]
    fn coupling_integrals_match_quadrature() {
        let (x1, a1, x2, a2) = (0.0, A, 0.004, 0.012);
        let m = coupling(5, x1, a1, 3, x2, a2);
        let (x, w) = (0..2000).map(|i| (x2 + a2 * (i as f64 + 0.5) / 2000.0, a2 / 2000.0)).unzip::<f64, f64, Vec<_>, Vec<_>>();
        for i in 0..5 {
            for j in 0..3 {
                let q: f64 = x
                    .iter()
                    .zip(&w)
                    .map(|(x, w)| w * ((i + 1) as f64 * PI * (x - x1) / a1).sin() * ((j + 1) as f64 * PI * (x - x2) / a2).sin())
                    .sum();
                assert!((m.at(i, j).re - q).abs() < 1e-8, "{i} {j}");
            }
        }
    }

    #[test]
    fn an_iris_is_lossless_reciprocal_and_converges() {
        let fwd = HPlaneLine { sections: vec![full(0.01, 1.0), iris(0.002, 0.012), full(0.012, 1.0)] };
        let mut rev = fwd.clone();
        rev.sections.reverse();
        let f = 10.0e9;
        let (s11, s21) = fwd.solve(f, 60).unwrap();
        assert!((s11.norm_sqr() + s21.norm_sqr() - 1.0).abs() < 1e-10);
        let (_, s12) = rev.solve(f, 60).unwrap();
        assert!((s21 - s12).norm() < 1e-10);
        // An inductive iris reflects, and more for a narrower one.
        let narrow = HPlaneLine { sections: vec![full(0.01, 1.0), iris(0.002, 0.008), full(0.012, 1.0)] };
        assert!(narrow.solve(f, 60).unwrap().0.norm() > s11.norm() && s11.norm() > 0.3);
        // Converged in the mode count.
        let (c11, _) = fwd.solve(f, 120).unwrap();
        assert!((c11 - s11).norm() < 2e-3, "{}", (c11 - s11).norm());
    }

    #[test]
    fn a_step_and_its_mirror_agree() {
        // A wide-to-narrow step at the guide's side, seen from both ends.
        let side = HSection { length: 0.015, eps_r: 1.0, mu_r: 1.0, x0: 0.0, width: 0.016 };
        let fwd = HPlaneLine { sections: vec![full(0.01, 1.0), side] };
        let rev = HPlaneLine { sections: vec![side, full(0.01, 1.0)] };
        let f = 12.0e9;
        let (s11, s21) = fwd.solve(f, 60).unwrap();
        let (_, s12) = rev.solve(f, 60).unwrap();
        assert!((s21 - s12).norm() < 1e-10);
        assert!((s11.norm_sqr() + s21.norm_sqr() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn hostile_inputs_are_refused() {
        let line = HPlaneLine { sections: vec![full(0.01, 1.0), iris(0.002, 0.01)] };
        assert!(line.solve(f64::NAN, 10).is_err());
        assert!(line.solve(1e10, 0).is_err());
        assert!(HPlaneLine { sections: vec![] }.solve(1e10, 10).is_err());
        let skew = HPlaneLine {
            sections: vec![HSection { x0: 0.0, width: 0.01, ..full(0.01, 1.0) }, HSection { x0: 0.005, width: 0.01, ..full(0.01, 1.0) }],
        };
        assert!(skew.solve(2e10, 10).is_err());
    }
}

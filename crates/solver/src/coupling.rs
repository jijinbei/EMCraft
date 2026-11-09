//! Coupling matrices of coupled-resonator filters: synthesis from a generalized Chebyshev
//! response and extraction from S-parameters.
//!
//! - **Synthesis** (R. J. Cameron, *General coupling matrix synthesis methods for Chebyshev
//!   filtering functions*, IEEE Trans. MTT 47(4):433–442, 1999; *Advanced coupling matrix
//!   synthesis techniques for microwave filters*, IEEE Trans. MTT 51(1):1–10, 2003). The
//!   filtering function's numerator F(ω) by the recursion of its U and V polynomials over the
//!   transmission zeros, P(ω) from the zeros, ε from the return loss at the band edge ω = ±1,
//!   E(s) by the Feldtkeller equation |E|² = |F/ε_R|² + |P/ε|² (its left-half-plane roots). The
//!   admittances y21, y22 from E and F, their residues at the eigenvalues give the N + 2
//!   transversal matrix; plane rotations reduce it to the folded form (main-line couplings and
//!   the cross couplings of the anti-diagonal).
//! - **Response** of an N + 2 matrix: [A] = [M + ωW − jR], S11 = 1 + 2j [A⁻¹]₀₀,
//!   S21 = −2j [A⁻¹]_{N+1,0} (Cameron 2003, eq. 7).
//! - **Extraction** by the Cauchy method (G. Macchiarella, D. Traina, *A formulation of the
//!   Cauchy method suitable for the synthesis of lossless circuit models of microwave filters
//!   from lossy measurements*, IEEE MWCL 16(5):243–245, 2006): S11 E − F = 0 and S21 E − P = 0
//!   at the samples, solved for the polynomials' coefficients by least squares (Householder
//!   QR); F's and P's roots taken onto the imaginary axis and the lossless E rebuilt from them,
//!   so a lossy measurement still gives a realisable matrix. The ports' phase loading (feed
//!   lines between the ports and the first resonators) is a linear phase in ω, read from the
//!   stop bands where |S11| is near one and refined against the model.
//!
//! Frequencies are the low-pass prototype's ω (the band edges at ±1); a band-pass filter's
//! f maps to ω = (f₀/Δf)(f/f₀ − f₀/f).

use crate::Complex64 as C;
use crate::dense::Lu;

/// The highest filter order synthesised or extracted.
pub const MAX_ORDER: usize = 16;

const ZERO: C = C::new(0.0, 0.0);
const ONE: C = C::new(1.0, 0.0);
const J: C = C::new(0.0, 1.0);

/// A polynomial's coefficients, constant first, from its roots (monic).
fn from_roots(roots: &[C]) -> Vec<C> {
    let mut p = vec![ONE];
    for r in roots {
        let mut q = vec![ZERO; p.len() + 1];
        for (i, c) in p.iter().enumerate() {
            q[i + 1] += c;
            q[i] -= c * r;
        }
        p = q;
    }
    p
}

/// p(x) by Horner's rule.
fn eval(p: &[C], x: C) -> C {
    p.iter().rev().fold(ZERO, |acc, c| acc * x + c)
}

/// p′.
fn derivative(p: &[C]) -> Vec<C> {
    p.iter().enumerate().skip(1).map(|(i, c)| c * i as f64).collect()
}

/// p q.
fn multiply(p: &[C], q: &[C]) -> Vec<C> {
    let mut out = vec![ZERO; (p.len() + q.len()).saturating_sub(1)];
    for (i, a) in p.iter().enumerate() {
        for (j, b) in q.iter().enumerate() {
            out[i + j] += a * b;
        }
    }
    out
}

/// The roots of `p` (its last coefficient non-zero): the eigenvalues of its companion matrix,
/// each polished by Newton's method. `None` when they cannot be found.
fn roots(p: &[C]) -> Option<Vec<C>> {
    let d = p.len().checked_sub(1)?;
    let lead = *p.last()?;
    if lead.norm() == 0.0 || !lead.norm().is_finite() {
        return None;
    }
    if d == 0 {
        return Some(Vec::new());
    }
    let mut a = vec![ZERO; d * d];
    for j in 0..d {
        a[j] = -p[d - 1 - j] / lead;
    }
    for i in 1..d {
        a[i * d + i - 1] = ONE;
    }
    let mut r = crate::zdense::values(&a, d)?;
    let dp = derivative(p);
    for x in &mut r {
        for _ in 0..3 {
            let (v, dv) = (eval(p, *x), eval(&dp, *x));
            if dv.norm() == 0.0 {
                break;
            }
            let next = *x - v / dv;
            if eval(p, next).norm() < v.norm() {
                *x = next;
            } else {
                break;
            }
        }
    }
    r.iter().all(|x| x.re.is_finite() && x.im.is_finite()).then_some(r)
}

/// A filter's characteristic polynomials in s = jω: S11 = F(s)/E(s), S21 = P(s)/E(s), E monic
/// and Hurwitz, F and P scaled by ε_R and ε; the reflection zeros (F's roots jω) and the finite
/// transmission zeros (P's) as ω.
#[derive(Clone, Debug, PartialEq)]
pub struct Characteristic {
    pub order: usize,
    pub reflection_zeros: Vec<f64>,
    pub transmission_zeros: Vec<f64>,
    pub eps: f64,
    pub eps_r: f64,
    /// Coefficients, constant first.
    pub e: Vec<C>,
    pub f: Vec<C>,
    pub p: Vec<C>,
}

impl Characteristic {
    /// S11 and S21 at the low-pass frequency ω.
    pub fn s(&self, omega: f64) -> (C, C) {
        let s = C::new(0.0, omega);
        let e = eval(&self.e, s);
        (eval(&self.f, s) / e, eval(&self.p, s) / e)
    }
}

/// The characteristic of `order` resonators whose reflection zeros are `reflection` and finite
/// transmission zeros `transmission` (ω), with ε: E from the Feldtkeller equation. An error for
/// sizes or numbers that do not make a filter.
pub fn characteristic(order: usize, reflection: &[f64], transmission: &[f64], eps: f64) -> Result<Characteristic, String> {
    if order == 0 || order > MAX_ORDER {
        return Err(format!("the order is from 1 to {MAX_ORDER}"));
    }
    if reflection.len() != order || transmission.len() > order {
        return Err(format!("{order} reflection zeros and at most {order} transmission zeros"));
    }
    if !reflection.iter().chain(transmission).all(|w| w.is_finite()) || !(eps.is_finite() && eps > 0.0) {
        return Err("zeros and ε must be finite, ε positive".into());
    }
    let nfz = transmission.len();
    let eps_r = if nfz == order {
        if eps <= 1.0 {
            return Err("a fully canonical filter needs ε > 1".into());
        }
        eps / (eps * eps - 1.0).sqrt()
    } else {
        1.0
    };
    let real = |w: &[f64]| w.iter().map(|x| C::new(*x, 0.0)).collect::<Vec<_>>();
    let (fw, pw) = (from_roots(&real(reflection)), from_roots(&real(transmission)));
    // Q(ω) = F(ω)²/ε_R² + P(ω)²/ε², real, its roots in conjugate pairs.
    let mut q = multiply(&fw, &fw).into_iter().map(|c| c / (eps_r * eps_r)).collect::<Vec<_>>();
    for (i, c) in multiply(&pw, &pw).into_iter().enumerate() {
        q[i] += c / (eps * eps);
    }
    let lead = q[2 * order].re;
    let mut r = roots(&q).ok_or("the Feldtkeller equation's roots were not found")?;
    // E's roots s = jω in the left half plane: Im ω > 0.
    r.sort_by(|a, b| b.im.total_cmp(&a.im));
    let upper: Vec<C> = r.into_iter().take(order).collect();
    let scale = upper.iter().fold(1.0f64, |m, x| m.max(x.norm()));
    if upper.iter().any(|x| x.im <= 1e-12 * scale) || !(lead > 0.0) {
        return Err("the response is not realisable (E has roots on the axis)".into());
    }
    let e = from_roots(&upper.iter().map(|w| J * w).collect::<Vec<_>>());
    let root = lead.sqrt();
    let jw = |w: &[f64]| w.iter().map(|x| C::new(0.0, *x)).collect::<Vec<_>>();
    let f: Vec<C> = from_roots(&jw(reflection)).into_iter().map(|c| c / (eps_r * root)).collect();
    // P multiplied by j when N − nfz is even (Cameron 1999), so S11 and S21 are orthogonal.
    let turn = if (order - nfz).is_multiple_of(2) { J } else { ONE };
    let p: Vec<C> = from_roots(&jw(transmission)).into_iter().map(|c| c * turn / (eps * root)).collect();
    Ok(Characteristic { order, reflection_zeros: reflection.to_vec(), transmission_zeros: transmission.to_vec(), eps, eps_r, e, f, p })
}

/// The generalized Chebyshev characteristic of `order` resonators, return loss `return_loss`
/// (dB, equiripple over |ω| ≤ 1) and finite transmission zeros `zeros` (ω, each |ω| > 1; the
/// rest at infinity).
///
/// ```
/// use emcraft_solver::coupling::chebyshev;
///
/// // Fourth order, 22 dB, zeros at ω = ±1.8 (a symmetric quasi-elliptic filter).
/// let c = chebyshev(4, 22.0, &[-1.8, 1.8]).unwrap();
/// // The return loss at the band edge, the zeros of transmission where asked.
/// let (s11, _) = c.s(1.0);
/// assert!((20.0 * s11.norm().log10() + 22.0).abs() < 1e-6);
/// assert!(c.s(1.8).1.norm() < 1e-12);
/// // Lossless at every frequency.
/// let (a, b) = c.s(0.37);
/// assert!((a.norm_sqr() + b.norm_sqr() - 1.0).abs() < 1e-9);
/// assert!(chebyshev(4, 22.0, &[0.5]).is_err());
/// ```
pub fn chebyshev(order: usize, return_loss: f64, zeros: &[f64]) -> Result<Characteristic, String> {
    if order == 0 || order > MAX_ORDER {
        return Err(format!("the order is from 1 to {MAX_ORDER}"));
    }
    if !(return_loss.is_finite() && return_loss > 0.0 && return_loss <= 80.0) {
        return Err("the return loss is from 0 to 80 dB".into());
    }
    if zeros.len() > order || !zeros.iter().all(|w| w.is_finite() && w.abs() > 1.0) {
        return Err(format!("at most {order} transmission zeros, each outside the band (|ω| > 1)"));
    }
    // U and V/ω′ (Cameron 1999, eq. 8): Uₙ = (ω − 1/ωₙ) U + cₙ (ω² − 1) V, Vₙ = (ω − 1/ωₙ) V + cₙ U.
    let mut u = vec![1.0];
    let mut v = vec![0.0];
    let mul_x = |p: &[f64], a: f64| {
        let mut out = vec![0.0; p.len() + 1];
        for (i, c) in p.iter().enumerate() {
            out[i + 1] += c;
            out[i] -= a * c;
        }
        out
    };
    let add = |a: &[f64], b: &[f64]| {
        let mut out = vec![0.0; a.len().max(b.len())];
        for (i, c) in a.iter().enumerate() {
            out[i] += c;
        }
        for (i, c) in b.iter().enumerate() {
            out[i] += c;
        }
        out
    };
    for n in 0..order {
        let inv = zeros.get(n).map_or(0.0, |w| 1.0 / w);
        let c = (1.0 - inv * inv).sqrt();
        let vx2 = add(&mul_x(&mul_x(&v, 1.0), -1.0), &[]); // (ω − 1)(ω + 1) V
        let nu = add(&mul_x(&u, inv), &vx2.iter().map(|x| c * x).collect::<Vec<_>>());
        let nv = add(&mul_x(&v, inv), &u.iter().map(|x| c * x).collect::<Vec<_>>());
        u = nu;
        v = nv;
    }
    u.truncate(order + 1);
    let r = roots(&u.iter().map(|x| C::new(*x, 0.0)).collect::<Vec<_>>()).ok_or("the reflection zeros were not found")?;
    let mut reflection: Vec<f64> = r.iter().map(|x| x.re).collect();
    reflection.sort_by(f64::total_cmp);
    // ε from the return loss at ω = 1: |S11|² = r² there.
    let fm: f64 = reflection.iter().map(|w| 1.0 - w).product();
    let pm: f64 = zeros.iter().map(|w| 1.0 - w).product();
    let r2 = 10f64.powf(-return_loss / 10.0);
    let k = 1.0 / r2 - 1.0;
    let eps = if zeros.len() < order { (pm / fm).abs() / k.sqrt() } else { (1.0 + pm * pm / (fm * fm * k)).sqrt() };
    let mut zeros = zeros.to_vec();
    zeros.sort_by(f64::total_cmp);
    characteristic(order, &reflection, &zeros, eps)
}

/// An N + 2 coupling matrix (source, N resonators, load), real and symmetric, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Coupling {
    pub order: usize,
    pub m: Vec<f64>,
}

impl Coupling {
    /// The size N + 2.
    pub fn size(&self) -> usize {
        self.order + 2
    }

    /// M\[i\]\[j\] (0 the source, N + 1 the load).
    pub fn get(&self, i: usize, j: usize) -> f64 {
        self.m.get(i * self.size() + j).copied().unwrap_or(0.0)
    }

    /// S11 and S21 at the low-pass frequency ω: [A] = [M + ωW − jR] (Cameron 2003, eq. 7).
    /// `None` when A is singular there.
    pub fn s(&self, omega: f64) -> Option<(C, C)> {
        let n = self.size();
        let a: Vec<C> = (0..n * n)
            .map(|k| {
                let (i, j) = (k / n, k % n);
                let mut v = C::new(self.m[k], 0.0);
                if i == j {
                    v += if i == 0 || i == n - 1 { -J } else { C::new(omega, 0.0) };
                }
                v
            })
            .collect();
        let lu = Lu::new(a, n)?;
        let mut e0 = vec![ZERO; n];
        e0[0] = ONE;
        let x = lu.solve(&e0);
        Some((ONE + 2.0 * J * x[0], -2.0 * J * x[n - 1]))
    }

    /// The matrix turned by a plane rotation at pivot (p, q) by θ: M′ = R M Rᵀ.
    fn rotate(&mut self, p: usize, q: usize, theta: f64) {
        let n = self.size();
        let (c, s) = (theta.cos(), theta.sin());
        // Rows p and q, then columns p and q.
        for j in 0..n {
            let (a, b) = (self.m[p * n + j], self.m[q * n + j]);
            self.m[p * n + j] = c * a - s * b;
            self.m[q * n + j] = s * a + c * b;
        }
        for i in 0..n {
            let (a, b) = (self.m[i * n + p], self.m[i * n + q]);
            self.m[i * n + p] = c * a - s * b;
            self.m[i * n + q] = s * a + c * b;
        }
    }

    /// The folded form (Cameron 2003, §IV): rows from the source and columns from the load
    /// cleared in turn by rotations, leaving the main line, the diagonal, the anti-diagonal's
    /// cross couplings and an asymmetric response's diagonal ones beside them ([`folded_place`]). The response is unchanged (each rotation is a similarity).
    pub fn folded(&self) -> Coupling {
        let mut m = self.clone();
        let n = m.size();
        let last = n - 1;
        let mut k = 0;
        while k + 2 <= last {
            // Row k: clear M[k][j] for j from the column left of the anti-diagonal's down to k + 2.
            let col_end = last - k;
            for j in (k + 2..col_end).rev() {
                let theta = (-m.get(k, j)).atan2(m.get(k, j - 1));
                if m.get(k, j) != 0.0 {
                    m.rotate(j - 1, j, theta);
                }
            }
            // Column last − k: clear M[i][c] for i from k + 2 up to c − 2. Row k + 1's entry
            // there is zero already: the rotations keep the rows' inner products, and row k's
            // vector is along its main-line coupling alone (Cameron 1999, §IV).
            let c = last - k;
            for i in k + 2..c.saturating_sub(1) {
                let theta = m.get(i, c).atan2(m.get(i + 1, c));
                if m.get(i, c) != 0.0 {
                    m.rotate(i, i + 1, theta);
                }
            }
            k += 1;
        }
        // Clean the round-off left where the form has zeros.
        for i in 0..n {
            for j in 0..n {
                if !folded_place(i, j, last) && m.m[i * n + j].abs() < 1e-12 {
                    m.m[i * n + j] = 0.0;
                }
            }
        }
        m
    }
}

/// Whether (i, j) may be non-zero in the folded form of an N + 2 matrix whose last index is
/// `last`: the diagonal, the main line, the anti-diagonal's cross couplings and, for an
/// asymmetric response, the diagonal ones beside them (i + j = last + 1: M₂₄ of a fourth-order
/// filter with one zero, a trisection's).
pub fn folded_place(i: usize, j: usize, last: usize) -> bool {
    i == j || i.abs_diff(j) == 1 || i + j == last || i + j == last + 1
}

/// The N + 2 transversal matrix of a characteristic: each resonator coupled to the source and
/// the load alone, from the residues of y21 and y22 at their poles (Cameron 2003, §II–III).
pub fn transversal(c: &Characteristic) -> Result<Coupling, String> {
    let n = c.order;
    let at = |v: &[C], i: usize| v.get(i).copied().unwrap_or(ZERO);
    let sum: Vec<C> = (0..=n).map(|i| at(&c.e, i) + at(&c.f, i)).collect();
    let m1: Vec<C> = sum.iter().enumerate().map(|(i, x)| if i % 2 == 0 { C::new(x.re, 0.0) } else { C::new(0.0, x.im) }).collect();
    let n1: Vec<C> = sum.iter().enumerate().map(|(i, x)| if i % 2 == 0 { C::new(0.0, x.im) } else { C::new(x.re, 0.0) }).collect();
    let (mut yd, mut y22) = if n.is_multiple_of(2) { (m1, n1) } else { (n1, m1) };
    let mut y21: Vec<C> = (0..=n).map(|i| at(&c.p, i)).collect();
    let lead = yd[n];
    if lead.norm() == 0.0 {
        return Err("the admittances have no poles".into());
    }
    for v in [&mut yd, &mut y22, &mut y21] {
        v.iter_mut().for_each(|x| *x /= lead);
    }
    yd.truncate(n + 1);
    // A fully canonical filter's y21 keeps a constant at infinity: the source–load coupling.
    let k_inf = y21[n];
    for (i, x) in y21.iter_mut().enumerate() {
        *x -= k_inf * yd[i];
    }
    let poles = roots(&yd).ok_or("the admittances' poles were not found")?;
    let dyd = derivative(&yd);
    let mut lines: Vec<(f64, f64, f64)> = Vec::with_capacity(n);
    for s in &poles {
        let d = eval(&dyd, *s);
        let (r22, r21) = (eval(&y22, *s) / d, eval(&y21, *s) / d);
        if !(r22.re > 0.0) {
            return Err("a residue of y22 is not positive: the response is not a filter's".into());
        }
        let root = r22.re.sqrt();
        lines.push((-s.im, root, r21.re / root));
    }
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    let size = n + 2;
    let mut m = vec![0.0; size * size];
    for (k, (diag, ms, ml)) in lines.into_iter().enumerate() {
        let r = k + 1;
        m[r * size + r] = diag;
        m[r] = ms;
        m[r * size] = ms;
        m[r * size + size - 1] = ml;
        m[(size - 1) * size + r] = ml;
    }
    let msl = (k_inf / J).re;
    m[size - 1] = msl;
    m[(size - 1) * size] = msl;
    Ok(Coupling { order: n, m })
}

/// The folded coupling matrix of a characteristic.
///
/// ```
/// use emcraft_solver::coupling::{chebyshev, synthesize};
///
/// let c = chebyshev(4, 20.0, &[]).unwrap();
/// let m = synthesize(&c).unwrap();
/// // An all-pole filter folds to a line: source, four resonators, load.
/// assert!(m.get(0, 2).abs() < 1e-9 && m.get(1, 4).abs() < 1e-9);
/// // Its response is the characteristic's.
/// for w in [-1.3, -0.5, 0.2, 0.9, 2.0] {
///     let ((a, b), (x, y)) = (m.s(w).unwrap(), c.s(w));
///     assert!((a.norm() - x.norm()).abs() < 1e-9 && (b.norm() - y.norm()).abs() < 1e-9);
/// }
/// ```
pub fn synthesize(c: &Characteristic) -> Result<Coupling, String> {
    Ok(transversal(c)?.folded())
}

/// The least-squares solution of A x = b (A `rows` × `cols`, row-major) by Householder QR on
/// columns scaled to unit length; `None` when A is rank deficient.
fn least_squares(mut a: Vec<C>, rows: usize, cols: usize, mut b: Vec<C>) -> Option<Vec<C>> {
    if rows < cols || a.len() != rows * cols || b.len() != rows {
        return None;
    }
    let scale: Vec<f64> = (0..cols).map(|j| (0..rows).map(|i| a[i * cols + j].norm_sqr()).sum::<f64>().sqrt()).collect();
    if scale.iter().any(|s| !(*s > 0.0 && s.is_finite())) {
        return None;
    }
    for i in 0..rows {
        for j in 0..cols {
            a[i * cols + j] /= scale[j];
        }
    }
    for k in 0..cols {
        let alpha = (k..rows).map(|i| a[i * cols + k].norm_sqr()).sum::<f64>().sqrt();
        if alpha <= 1e-14 {
            return None;
        }
        let x0 = a[k * cols + k];
        let phase = if x0.norm() > 0.0 { x0 / x0.norm() } else { ONE };
        let mut v: Vec<C> = (k..rows).map(|i| a[i * cols + k]).collect();
        v[0] += phase * alpha;
        let vv: f64 = v.iter().map(C::norm_sqr).sum();
        if vv == 0.0 {
            continue;
        }
        for j in k..cols {
            let s: C = v.iter().enumerate().map(|(t, vi)| vi.conj() * a[(k + t) * cols + j]).sum();
            for (t, vi) in v.iter().enumerate() {
                a[(k + t) * cols + j] -= 2.0 * vi * s / vv;
            }
        }
        let s: C = v.iter().enumerate().map(|(t, vi)| vi.conj() * b[k + t]).sum();
        for (t, vi) in v.iter().enumerate() {
            b[k + t] -= 2.0 * vi * s / vv;
        }
    }
    let mut x = vec![ZERO; cols];
    for i in (0..cols).rev() {
        let s: C = (i + 1..cols).map(|j| a[i * cols + j] * x[j]).sum();
        let d = a[i * cols + i];
        if d.norm() == 0.0 {
            return None;
        }
        x[i] = (b[i] - s) / d;
    }
    let x: Vec<C> = x.into_iter().zip(&scale).map(|(v, s)| v / s).collect();
    x.iter().all(|c| c.re.is_finite() && c.im.is_finite()).then_some(x)
}

/// The characteristic of `order` resonators and `zeros` finite transmission zeros that best
/// fits S11 and S21 at the low-pass frequencies `omega` (the Cauchy method, see the module
/// docs): its reflection and transmission zeros on the axis, E rebuilt lossless.
///
/// ```
/// use emcraft_solver::coupling::{chebyshev, fit};
///
/// // A response sampled and fitted again gives its own zeros back.
/// let c = chebyshev(5, 25.0, &[1.6]).unwrap();
/// let w: Vec<f64> = (0..81).map(|k| -2.5 + 5.0 * f64::from(k) / 80.0).collect();
/// let (s11, s21): (Vec<_>, Vec<_>) = w.iter().map(|x| c.s(*x)).unzip();
/// let back = fit(&w, &s11, &s21, 5, 1).unwrap();
/// assert!((back.transmission_zeros[0] - 1.6).abs() < 1e-6);
/// assert!((back.eps - c.eps).abs() < 1e-6 * c.eps);
/// ```
pub fn fit(omega: &[f64], s11: &[C], s21: &[C], order: usize, zeros: usize) -> Result<Characteristic, String> {
    let (_, f, p) = cauchy(omega, s11, s21, order, zeros)?;
    lossless(&f, &p, order, zeros)
}

/// The rational fit itself: E (monic), F and P's coefficients.
fn cauchy(omega: &[f64], s11: &[C], s21: &[C], order: usize, zeros: usize) -> Result<(Vec<C>, Vec<C>, Vec<C>), String> {
    if order == 0 || order > MAX_ORDER || zeros > order {
        return Err(format!("the order is from 1 to {MAX_ORDER}, with at most as many transmission zeros"));
    }
    let k = omega.len();
    if s11.len() != k || s21.len() != k {
        return Err("one S11 and one S21 per frequency".into());
    }
    let cols = order + (order + 1) + (zeros + 1);
    if 2 * k < cols + 2 {
        return Err(format!("{order} resonators need at least {} frequencies", cols / 2 + 2));
    }
    if !omega.iter().all(|w| w.is_finite()) || !s11.iter().chain(s21).all(|c| c.re.is_finite() && c.im.is_finite()) {
        return Err("frequencies and S must be finite".into());
    }
    let mut a = vec![ZERO; 2 * k * cols];
    let mut b = vec![ZERO; 2 * k];
    for (r, ((w, x11), x21)) in omega.iter().zip(s11).zip(s21).enumerate() {
        let s = C::new(0.0, *w);
        let pw: Vec<C> = std::iter::successors(Some(ONE), |x| Some(x * s)).take(order + 1).collect();
        let (r1, r2) = (2 * r, 2 * r + 1);
        for i in 0..order {
            a[r1 * cols + i] = x11 * pw[i];
            a[r2 * cols + i] = x21 * pw[i];
        }
        for i in 0..=order {
            a[r1 * cols + order + i] = -pw[i];
        }
        for i in 0..=zeros {
            a[r2 * cols + 2 * order + 1 + i] = -pw[i];
        }
        b[r1] = -x11 * pw[order];
        b[r2] = -x21 * pw[order];
    }
    let x = least_squares(a, 2 * k, cols, b).ok_or("the fit's equations are singular: more frequencies, or a lower order")?;
    let mut e = x[..order].to_vec();
    e.push(ONE);
    Ok((e, x[order..2 * order + 1].to_vec(), x[2 * order + 1..].to_vec()))
}

/// The lossless characteristic whose zeros are F's and P's roots taken onto the axis, ε from
/// their leading coefficients.
fn lossless(f: &[C], p: &[C], order: usize, zeros: usize) -> Result<Characteristic, String> {
    let fr = roots(f).ok_or("F's roots were not found")?;
    let pr = roots(p).ok_or("P's roots were not found")?;
    let mut reflection: Vec<f64> = fr.iter().map(|z| z.im).collect();
    let mut transmission: Vec<f64> = pr.iter().map(|z| z.im).collect();
    reflection.sort_by(f64::total_cmp);
    transmission.sort_by(f64::total_cmp);
    let ratio = f[order].norm() / p[zeros].norm();
    let eps = if zeros < order { ratio } else { (ratio * ratio + 1.0).sqrt() };
    characteristic(order, &reflection, &transmission, eps)
}

/// The roots of a polynomial that is a square (each root twice, split by round-off or a fit
/// that is not quite one), one of each pair: sorted along the real axis and averaged two by two.
fn halved(roots: &[C]) -> Vec<f64> {
    let mut r: Vec<C> = roots.to_vec();
    r.sort_by(|a, b| a.re.total_cmp(&b.re));
    r.chunks(2).map(|pair| pair.iter().map(|z| z.re).sum::<f64>() / pair.len() as f64).collect()
}

/// The characteristic of `order` resonators and `zeros` finite transmission zeros whose
/// magnitudes best fit the data's, phase-free: |S11|²/|S21|² = ε² F(ω)²/P(ω)² for a lossless
/// filter, so |S21|² A(ω) − |S11|² B(ω) = 0 with A = ε²F² (degree 2N) and B = P² (monic,
/// degree 2 nfz) solved by least squares; F's and P's roots the double roots of A and B. The
/// ports' feed lines change only the phase, so they need no removal.
///
/// ```
/// use emcraft_solver::coupling::{chebyshev, fit_magnitude};
///
/// let c = chebyshev(4, 20.0, &[-1.6, 1.9]).unwrap();
/// let w: Vec<f64> = (0..121).map(|k| -3.0 + 6.0 * f64::from(k) / 120.0).collect();
/// // Any phase at all on the data: only |S| is read.
/// let (s11, s21): (Vec<_>, Vec<_>) = w.iter().map(|x| { let (a, b) = c.s(*x); (a * emcraft_solver::Complex64::from_polar(1.0, 3.0 * x), b) }).unzip();
/// let back = fit_magnitude(&w, &s11, &s21, 4, 2).unwrap();
/// assert!((back.transmission_zeros[0] + 1.6).abs() < 1e-5 && (back.transmission_zeros[1] - 1.9).abs() < 1e-5);
/// assert!((back.eps - c.eps).abs() < 1e-6 * c.eps);
/// ```
pub fn fit_magnitude(omega: &[f64], s11: &[C], s21: &[C], order: usize, zeros: usize) -> Result<Characteristic, String> {
    if order == 0 || order > MAX_ORDER || zeros > order {
        return Err(format!("the order is from 1 to {MAX_ORDER}, with at most as many transmission zeros"));
    }
    let k = omega.len();
    if s11.len() != k || s21.len() != k {
        return Err("one S11 and one S21 per frequency".into());
    }
    let cols = (2 * order + 1) + 2 * zeros;
    if k < cols + 2 {
        return Err(format!("{order} resonators need at least {} frequencies", cols + 2));
    }
    if !omega.iter().all(|w| w.is_finite()) || !s11.iter().chain(s21).all(|c| c.re.is_finite() && c.im.is_finite()) {
        return Err("frequencies and S must be finite".into());
    }
    // Sanathanan–Koerner: each row weighted by 1/(A + B) of the last fit, so the residual is
    // the error in |S|² (not in |S11|²/|S21|², which the stop bands would rule); first by
    // |S21|² (A + B = B/|S21|², B taken as one).
    let mut weights: Vec<f64> = s21.iter().map(|x| x.norm_sqr().max(1e-12)).collect();
    let mut big_a = Vec::new();
    let mut big_b = Vec::new();
    for _ in 0..6 {
        let mut a = vec![ZERO; k * cols];
        let mut b = vec![ZERO; k];
        for (r, ((w, x11), x21)) in omega.iter().zip(s11).zip(s21).enumerate() {
            let (m11, m21) = (x11.norm_sqr(), x21.norm_sqr());
            let weight = weights[r];
            let pw: Vec<f64> = std::iter::successors(Some(1.0), |x| Some(x * w)).take(2 * order + 1).collect();
            for i in 0..=2 * order {
                a[r * cols + i] = C::new(weight * m21 * pw[i], 0.0);
            }
            for i in 0..2 * zeros {
                a[r * cols + 2 * order + 1 + i] = C::new(-weight * m11 * pw[i], 0.0);
            }
            b[r] = C::new(weight * m11 * pw[2 * zeros], 0.0);
        }
        let x = least_squares(a, k, cols, b).ok_or("the fit's equations are singular: more frequencies, or a lower order")?;
        big_a = x[..=2 * order].iter().map(|c| C::new(c.re, 0.0)).collect();
        big_b = x[2 * order + 1..].iter().map(|c| C::new(c.re, 0.0)).collect();
        big_b.push(ONE);
        for (wt, w) in weights.iter_mut().zip(omega) {
            let s = eval(&big_a, C::new(*w, 0.0)).re.abs() + eval(&big_b, C::new(*w, 0.0)).re.abs();
            *wt = if s > 0.0 && s.is_finite() { 1.0 / s } else { 1.0 };
        }
    }
    let lead = big_a[2 * order].re;
    if !(lead > 0.0) {
        return Err("the fitted |S11|²/|S21|² does not grow as a filter's: check the band and the order".into());
    }
    let reflection = halved(&roots(&big_a).ok_or("F's roots were not found")?);
    let transmission = halved(&roots(&big_b).ok_or("P's roots were not found")?);
    // The linear fit need not keep A a square; it starts the zeros' own fit to |S|, as does the
    // all-pole Chebyshev's (zeros at cos((2k − 1)π/2N)), the better of the two kept.
    let ratio = lead.sqrt();
    let m11: Vec<f64> = s11.iter().map(|x| x.norm()).collect();
    let m21: Vec<f64> = s21.iter().map(|x| x.norm()).collect();
    let chebyshev_zeros: Vec<f64> = (1..=order).map(|k| ((2 * k - 1) as f64 * std::f64::consts::PI / (2 * order) as f64).cos()).collect();
    let starts = [reflection, chebyshev_zeros];
    let mut best: Option<(f64, Vec<f64>)> = None;
    for r0 in starts {
        let mut x0: Vec<f64> = r0.into_iter().chain(transmission.iter().copied()).collect();
        x0.push(ratio.max(1e-6).ln());
        let x = levenberg_marquardt(&x0, |x| magnitude_residuals(omega, &m11, &m21, x, order, zeros));
        let cost: f64 = magnitude_residuals(omega, &m11, &m21, &x, order, zeros).iter().map(|r| r * r).sum();
        if best.as_ref().is_none_or(|(c, _)| cost < *c) {
            best = Some((cost, x));
        }
    }
    let (_, x) = best.ok_or("no fit")?;
    let mut reflection = x[..order].to_vec();
    let mut transmission = x[order..order + zeros].to_vec();
    reflection.sort_by(f64::total_cmp);
    transmission.sort_by(f64::total_cmp);
    let ratio = x[order + zeros].exp();
    let eps = if zeros < order { ratio } else { (ratio * ratio + 1.0).sqrt() };
    characteristic(order, &reflection, &transmission, eps)
}

/// |S11| and |S21| of a lossless filter with reflection zeros `x[..N]`, transmission zeros
/// `x[N..N + nfz]` and ln(ε/ε_R) last, less the data's: |S11|² = K²/(1 + K²) with
/// K = (ε/ε_R) F(ω)/P(ω).
fn magnitude_residuals(omega: &[f64], m11: &[f64], m21: &[f64], x: &[f64], order: usize, zeros: usize) -> Vec<f64> {
    let ratio = x[order + zeros].exp();
    let mut out = Vec::with_capacity(2 * omega.len());
    for ((w, a), b) in omega.iter().zip(m11).zip(m21) {
        let f: f64 = x[..order].iter().map(|z| w - z).product();
        let p: f64 = x[order..order + zeros].iter().map(|z| w - z).product();
        let (ff, pp) = ((ratio * f).powi(2), p * p);
        let den = ff + pp;
        let (s11, s21) = if den > 0.0 { ((ff / den).sqrt(), (pp / den).sqrt()) } else { (1.0, 0.0) };
        out.push(s11 - a);
        out.push(s21 - b);
    }
    out
}

/// The minimum of Σ r(x)² near `x0` by Levenberg–Marquardt (K. Madsen, H. B. Nielsen,
/// O. Tingleff, *Methods for Non-Linear Least Squares Problems*, 2nd ed., DTU 2004, §3.2): the
/// Jacobian by central differences, at most 200 steps.
fn levenberg_marquardt(x0: &[f64], r: impl Fn(&[f64]) -> Vec<f64>) -> Vec<f64> {
    let n = x0.len();
    let mut x = x0.to_vec();
    let mut res = r(&x);
    let cost = |v: &[f64]| v.iter().map(|e| e * e).sum::<f64>();
    let mut c = cost(&res);
    let mut mu = 1e-3;
    for _ in 0..200 {
        if !c.is_finite() {
            break;
        }
        let m = res.len();
        let mut jac = vec![0.0; m * n];
        for j in 0..n {
            let h = 1e-7 * x[j].abs().max(1e-3);
            let mut xp = x.clone();
            let mut xm = x.clone();
            xp[j] += h;
            xm[j] -= h;
            let (rp, rm) = (r(&xp), r(&xm));
            for i in 0..m {
                jac[i * n + j] = (rp[i] - rm[i]) / (2.0 * h);
            }
        }
        // (JᵀJ + μ diag(JᵀJ)) δ = −Jᵀ r
        let mut jtj = vec![0.0; n * n];
        let mut g = vec![0.0; n];
        for i in 0..m {
            for a in 0..n {
                g[a] += jac[i * n + a] * res[i];
                for b in 0..n {
                    jtj[a * n + b] += jac[i * n + a] * jac[i * n + b];
                }
            }
        }
        let mut improved = false;
        for _ in 0..12 {
            let mut lhs = jtj.clone();
            for a in 0..n {
                lhs[a * n + a] += mu * jtj[a * n + a].max(1e-12);
            }
            let Some(lu) = Lu::new(lhs, n) else {
                mu *= 10.0;
                continue;
            };
            let delta = lu.solve(&g.iter().map(|v| -v).collect::<Vec<_>>());
            let xn: Vec<f64> = x.iter().zip(&delta).map(|(a, d)| a + d).collect();
            let rn = r(&xn);
            let cn = cost(&rn);
            if cn < c {
                let gain = c - cn;
                x = xn;
                res = rn;
                c = cn;
                mu = (mu / 3.0).max(1e-12);
                improved = gain > 1e-15 * c.max(1e-30);
                break;
            }
            mu *= 4.0;
        }
        if !improved {
            break;
        }
    }
    x
}

/// A port's phase loading: θ(ω) = a + b ω (radians), S11 = S11′ e^{−jθ}.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Loading {
    pub a: f64,
    pub b: f64,
}

impl Loading {
    /// The loading θ at ω (radians).
    pub fn at(self, w: f64) -> f64 {
        self.a + self.b * w
    }
}

/// A line θ = a + b ω fitted to phases `(ω, θ)` given in runs (each run's phases unwrapped
/// along it), sharing the slope; the offset from the longest run. `None` with fewer than two
/// points in all.
fn phase_line(runs: &[Vec<(f64, f64)>]) -> Option<Loading> {
    let mut num = 0.0;
    let mut den = 0.0;
    for run in runs.iter().filter(|r| r.len() >= 2) {
        let n = run.len() as f64;
        let (mw, mt) = (run.iter().map(|p| p.0).sum::<f64>() / n, run.iter().map(|p| p.1).sum::<f64>() / n);
        num += run.iter().map(|p| (p.0 - mw) * (p.1 - mt)).sum::<f64>();
        den += run.iter().map(|p| (p.0 - mw).powi(2)).sum::<f64>();
    }
    if den <= 0.0 {
        return None;
    }
    let b = num / den;
    let longest = runs.iter().max_by_key(|r| r.len())?;
    let n = longest.len() as f64;
    let a = longest.iter().map(|p| p.1 - b * p.0).sum::<f64>() / n;
    Some(Loading { a, b })
}

/// Unwraps phases along a run.
fn unwrap(values: &mut [(f64, f64)]) {
    let mut offset = 0.0;
    for k in 1..values.len() {
        let step = values[k].1 + offset - values[k - 1].1;
        offset -= std::f64::consts::TAU * (step / std::f64::consts::TAU).round();
        values[k].1 += offset;
    }
}

/// What [`fit_loaded`] found: the characteristic and each port's phase loading.
#[derive(Clone, Debug, PartialEq)]
pub struct Extracted {
    pub characteristic: Characteristic,
    pub loading: [Loading; 2],
}

/// The ports' doubled phase loading 2θ read from `s` (S11 or S22) over the stop bands
/// (|ω| > 1, |S| ≥ 0.9), less the model's phase `model` there; zero without enough samples.
fn loading_of(omega: &[f64], s: &[C], model: &[C]) -> Loading {
    let mut lower = Vec::new();
    let mut upper = Vec::new();
    for ((w, x), m) in omega.iter().zip(s).zip(model) {
        if w.abs() > 1.0 && x.norm() >= 0.9 && m.norm() > 0.0 {
            // Doubled, so a reflection near −1 reads as near +1: the sign is not the loading's.
            let r = (x / m) * (x / m);
            let point = (*w, -r.arg());
            if *w < 0.0 { lower.push(point) } else { upper.push(point) }
        }
    }
    lower.sort_by(|a, b| a.0.total_cmp(&b.0));
    upper.sort_by(|a, b| a.0.total_cmp(&b.0));
    unwrap(&mut lower);
    unwrap(&mut upper);
    phase_line(&[lower, upper]).map_or_else(Loading::default, |l| Loading { a: l.a / 2.0, b: l.b / 2.0 })
}

/// The RMS misfit of the rational fit to S11 and S21 with the ports' phase slopes `b` removed.
fn misfit(omega: &[f64], s11: &[C], s21: &[C], b: [f64; 2], order: usize, zeros: usize) -> f64 {
    let a: Vec<C> = s11.iter().zip(omega).map(|(x, w)| x * C::from_polar(1.0, b[0] * w)).collect();
    let c: Vec<C> = s21.iter().zip(omega).map(|(x, w)| x * C::from_polar(1.0, 0.5 * (b[0] + b[1]) * w)).collect();
    let Ok((e, f, p)) = cauchy(omega, &a, &c, order, zeros) else { return f64::INFINITY };
    let mut sum = 0.0;
    for ((w, x), y) in omega.iter().zip(&a).zip(&c) {
        let s = C::new(0.0, *w);
        let d = eval(&e, s);
        sum += (x - eval(&f, s) / d).norm_sqr() + (y - eval(&p, s) / d).norm_sqr();
    }
    (sum / omega.len().max(1) as f64).sqrt()
}

/// The minimum of `g` over [lo, hi] by golden-section search, `steps` narrowings.
fn golden(mut lo: f64, mut hi: f64, steps: usize, g: impl Fn(f64) -> f64) -> f64 {
    let r = 0.5 * (5f64.sqrt() - 1.0);
    let (mut x1, mut x2) = (hi - r * (hi - lo), lo + r * (hi - lo));
    let (mut g1, mut g2) = (g(x1), g(x2));
    for _ in 0..steps {
        if g1 <= g2 {
            hi = x2;
            (x2, g2) = (x1, g1);
            x1 = hi - r * (hi - lo);
            g1 = g(x1);
        } else {
            lo = x1;
            (x1, g1) = (x2, g2);
            x2 = lo + r * (hi - lo);
            g2 = g(x2);
        }
    }
    if g1 <= g2 { x1 } else { x2 }
}

/// The characteristic fitted to a filter's S11, S21 and S22 with each port's phase loading
/// removed: the slopes first read from the stop bands' phase (S11's and S22's), then each
/// refined in turn as the one that fits the rational model best (the misfit of a linear phase
/// left in is large: a rational function of order N cannot follow it), three rounds.
pub fn fit_loaded(omega: &[f64], s11: &[C], s21: &[C], s22: &[C], order: usize, zeros: usize) -> Result<Extracted, String> {
    if s22.len() != omega.len() {
        return Err("one S22 per frequency".into());
    }
    let ones = vec![ONE; omega.len()];
    let start = [loading_of(omega, s11, &ones), loading_of(omega, s22, &ones)];
    let mut b = [start[0].b, start[1].b];
    if !b.iter().all(|x| x.is_finite()) {
        b = [0.0, 0.0];
    }
    // A coarse grid first (the misfit has other dips a phase turn away), then golden sections.
    let span = b.iter().fold(2.0f64, |m, x| m.max(1.5 * x.abs()));
    let grid = 32;
    let step = 2.0 * span / grid as f64;
    let centre = b;
    let mut best = f64::INFINITY;
    for i in 0..=grid {
        for k in 0..=grid {
            let t = [centre[0] - span + step * i as f64, centre[1] - span + step * k as f64];
            let g = misfit(omega, s11, s21, t, order, zeros);
            if g < best {
                best = g;
                b = t;
            }
        }
    }
    let mut width = 2.0 * step;
    for _ in 0..3 {
        for k in 0..2 {
            let at = |x: f64| {
                let mut t = b;
                t[k] = x;
                misfit(omega, s11, s21, t, order, zeros)
            };
            b[k] = golden(b[k] - width, b[k] + width, 40, at);
        }
        width *= 0.25;
    }
    let a: Vec<C> = s11.iter().zip(omega).map(|(x, w)| x * C::from_polar(1.0, b[0] * w)).collect();
    let c: Vec<C> = s21.iter().zip(omega).map(|(x, w)| x * C::from_polar(1.0, 0.5 * (b[0] + b[1]) * w)).collect();
    let characteristic = fit(omega, &a, &c, order, zeros)?;
    Ok(Extracted { characteristic, loading: [Loading { a: start[0].a, b: b[0] }, Loading { a: start[1].a, b: b[1] }] })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chebyshev low-pass prototype values g₀ … g_{N+1} (Matthaei, Young & Jones, *Microwave
    /// Filters, Impedance-Matching Networks, and Coupling Structures*, 1964, §4.05), from the
    /// ripple of return loss `rl` dB.
    fn g_values(n: usize, rl: f64) -> Vec<f64> {
        let ripple = -10.0 * (1.0 - 10f64.powf(-rl / 10.0)).log10();
        let beta = (1.0 / (ripple * std::f64::consts::LN_10 / 40.0).tanh()).ln();
        let gamma = (beta / (2.0 * n as f64)).sinh();
        let a: Vec<f64> = (1..=n).map(|k| ((2 * k - 1) as f64 * std::f64::consts::PI / (2.0 * n as f64)).sin()).collect();
        let b: Vec<f64> = (1..=n).map(|k| gamma.powi(2) + (k as f64 * std::f64::consts::PI / n as f64).sin().powi(2)).collect();
        let mut g = vec![1.0, 2.0 * a[0] / gamma];
        for k in 2..=n {
            let next = 4.0 * a[k - 2] * a[k - 1] / (b[k - 2] * g[k - 1]);
            g.push(next);
        }
        g.push(if n % 2 == 1 { 1.0 } else { 1.0 / (beta / 4.0).tanh().powi(2) });
        g
    }

    #[test]
    fn an_all_pole_filter_folds_to_the_couplings_of_its_g_values() {
        // M_{k,k+1} = 1/√(g_k g_{k+1}), the source's and load's 1/√(g₀g₁), 1/√(g_N g_{N+1}).
        for (n, rl) in [(3, 20.0), (4, 20.0), (6, 26.0)] {
            let m = synthesize(&chebyshev(n, rl, &[]).unwrap()).unwrap();
            let g = g_values(n, rl);
            for k in 0..=n {
                let want = 1.0 / (g[k] * g[k + 1]).sqrt();
                assert!((m.get(k, k + 1).abs() - want).abs() < 1e-6, "n {n} k {k}: {} vs {want}", m.get(k, k + 1));
            }
            for k in 1..=n {
                assert!(m.get(k, k).abs() < 1e-9, "a synchronous filter's resonators are tuned to the centre");
            }
        }
    }

    #[test]
    fn transmission_zeros_fold_into_cross_couplings_and_keep_the_response() {
        // Asymmetric (one zero above) and symmetric (a pair), and fully canonical (N zeros).
        for (n, rl, zeros) in [(4, 22.0, vec![1.5]), (6, 23.0, vec![-1.4, 1.4]), (3, 20.0, vec![-2.0, 1.7, 3.0])] {
            let c = chebyshev(n, rl, &zeros).unwrap();
            let t = transversal(&c).unwrap();
            let m = t.folded();
            let last = n + 1;
            for i in 0..=last {
                for j in 0..=last {
                    if !folded_place(i, j, last) {
                        assert!(m.get(i, j).abs() < 1e-9, "{zeros:?}: M[{i}][{j}] = {}", m.get(i, j));
                    }
                    assert!((m.get(i, j) - m.get(j, i)).abs() < 1e-9);
                }
            }
            for w in (0..41).map(|k| -3.0 + 6.0 * f64::from(k) / 40.0) {
                let (a, b) = c.s(w);
                for x in [&t, &m] {
                    let (p, q) = x.s(w).unwrap();
                    assert!((p.norm() - a.norm()).abs() < 1e-8 && (q.norm() - b.norm()).abs() < 1e-8, "{zeros:?} at {w}");
                }
            }
            for z in &zeros {
                assert!(m.s(*z).unwrap().1.norm() < 1e-8);
            }
        }
    }

    #[test]
    fn a_loaded_lossy_response_still_fits_its_zeros() {
        const LOSS: f64 = 0.98;
        // Feed lines of different length on each port, and a little loss (S scaled by 0.98).
        let c = chebyshev(4, 20.0, &[-1.7]).unwrap();
        let w: Vec<f64> = (0..161).map(|k| -3.0 + 6.0 * f64::from(k) / 160.0).collect();
        let (l1, l2) = (Loading { a: 0.4, b: 1.3 }, Loading { a: -1.1, b: 0.7 });
        let mut s11 = Vec::new();
        let mut s21 = Vec::new();
        let mut s22 = Vec::new();
        for x in &w {
            let (a, b) = c.s(*x);
            let s = C::new(0.0, *x);
            let f22 = eval(&c.f.iter().map(C::conj).collect::<Vec<_>>(), -s.conj()).conj() / eval(&c.e, s);
            s11.push(LOSS * a * C::from_polar(1.0, -l1.at(*x)));
            s22.push(LOSS * f22 * C::from_polar(1.0, -l2.at(*x)));
            s21.push(LOSS * b * C::from_polar(1.0, -0.5 * (l1.at(*x) + l2.at(*x))));
        }
        let x = fit_loaded(&w, &s11, &s21, &s22, 4, 1).unwrap();
        assert!((x.characteristic.transmission_zeros[0] + 1.7).abs() < 0.02, "{:?} {:?}", x.characteristic.transmission_zeros, x.loading);
        for (got, want) in x.characteristic.reflection_zeros.iter().zip(&c.reflection_zeros) {
            assert!((got - want).abs() < 0.02, "{:?}", x.characteristic.reflection_zeros);
        }
        assert!((x.loading[0].b - 1.3).abs() < 0.05, "{:?}", x.loading);
    }

    #[test]
    fn hostile_requests_are_refused() {
        assert!(chebyshev(0, 20.0, &[]).is_err());
        assert!(chebyshev(MAX_ORDER + 1, 20.0, &[]).is_err());
        assert!(chebyshev(3, -1.0, &[]).is_err());
        assert!(chebyshev(3, f64::NAN, &[]).is_err());
        assert!(chebyshev(2, 20.0, &[1.5, 2.0, 3.0]).is_err());
        assert!(characteristic(2, &[0.1], &[], 0.1).is_err());
        assert!(fit(&[0.0, 1.0], &[ONE; 2], &[ONE; 2], 4, 0).is_err());
        assert!(fit(&[0.0; 30], &[ONE; 30], &[ONE; 30], 3, 0).is_err());
        assert!(least_squares(vec![ONE, ONE], 1, 2, vec![ONE]).is_none());
    }
}

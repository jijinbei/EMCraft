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
    /// Dielectric loss tangent.
    pub loss_tangent: f64,
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

/// The modes of a line along its varying axis.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Basis {
    /// H-plane: TE_m0, e = sin(mπ(t − t0)/w), m ≥ 1.
    Sin,
    /// E-plane in a guide of width `a`: LSE^x_1n, e = cos(nπ(t − t0)/h), n ≥ 0, times sin(πx/a).
    Cos { a: f64 },
}

impl Basis {
    /// The mode index of the k-th mode.
    fn index(self, k: usize) -> f64 {
        match self {
            Basis::Sin => (k + 1) as f64,
            Basis::Cos { .. } => k as f64,
        }
    }

    /// ∫ e_k² over the aperture.
    fn norm(self, k: usize, span: f64) -> f64 {
        match self {
            Basis::Cos { .. } if k == 0 => span,
            _ => 0.5 * span,
        }
    }
}

/// A segment of a line along its varying axis.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Seg {
    length: f64,
    eps_r: f64,
    mu_r: f64,
    loss_tangent: f64,
    t0: f64,
    span: f64,
}

/// M_mn = ∫ e_m⁽¹⁾ e_n⁽²⁾ dt over aperture 2 (within aperture 1), each side's `n` modes over its
/// aperture from `x`, `a` wide.
fn coupling(basis: Basis, (n1, x1, a1): (usize, f64, f64), (n2, x2, a2): (usize, f64, f64)) -> Mat {
    let mut m = Mat::zeros(n1, n2);
    for i in 0..n1 {
        let k1 = basis.index(i) * PI / a1;
        for j in 0..n2 {
            let k2 = basis.index(j) * PI / a2;
            // A = k1(t − x1), B = k2(t − x2): sin A sin B = ½[cos(A − B) − cos(A + B)],
            // cos A cos B = ½[cos(A − B) + cos(A + B)].
            let d = int_cos(k1 - k2, -k1 * x1 + k2 * x2, x2, x2 + a2);
            let s = int_cos(k1 + k2, -k1 * x1 - k2 * x2, x2, x2 + a2);
            let v = match basis {
                Basis::Sin => 0.5 * (d - s),
                Basis::Cos { .. } => 0.5 * (d + s),
            };
            m.v[i * n2 + j] = C::new(v, 0.0);
        }
    }
    m
}

/// The junction of a wide guide (1) and a guide (2) whose aperture lies within it: the GSM with
/// guide 1 on the left (D_i = diag ∫ e² dt, Y_i the modal admittances, one per mode).
fn junction(basis: Basis, s1: &Seg, y1: &[C], s2: &Seg, y2: &[C]) -> Result<Gsm, TheoryError> {
    let (n1, n2) = (y1.len(), y2.len());
    let m = coupling(basis, (n1, s1.t0, s1.span), (n2, s2.t0, s2.span));
    let d1inv = Mat::diag(&(0..n1).map(|k| C::new(1.0 / basis.norm(k, s1.span), 0.0)).collect::<Vec<_>>());
    let d2y2 = Mat::diag(&y2.iter().enumerate().map(|(k, y)| y * basis.norm(k, s2.span)).collect::<Vec<_>>());
    let mt_y1 = m.transpose().mul(&Mat::diag(y1));
    let w = d2y2.add(&mt_y1.mul(&d1inv.mul(&m)), ONE);
    let s21 = w.solve(&mt_y1.scale(C::new(2.0, 0.0)))?;
    let s22 = w.solve(&d2y2.scale(C::new(2.0, 0.0)))?.add(&Mat::identity(n2), -ONE);
    let md = d1inv.mul(&m);
    let s11 = md.mul(&s21).add(&Mat::identity(n1), -ONE);
    let s12 = md.mul(&s22.add(&Mat::identity(n2), ONE));
    Ok(Gsm { ll: s11, lr: s12, rl: s21, rr: s22 })
}

/// S₁₁ and S₂₁ of the fundamental mode through segments (power waves, referred to the ends).
fn solve_line(basis: Basis, segs: &[Seg], frequency: f64, modes: usize) -> Result<(C, C), TheoryError> {
    if !(frequency.is_finite() && frequency > 0.0) {
        return Err(TheoryError::Invalid(format!("frequency {frequency}")));
    }
    if segs.is_empty() || !(1..=2000).contains(&modes) {
        return Err(TheoryError::Invalid("line or mode count".into()));
    }
    for s in segs {
        if ![s.length, s.eps_r, s.mu_r, s.span].iter().all(|v| v.is_finite() && *v > 0.0)
            || !s.t0.is_finite()
            || !(s.loss_tangent.is_finite() && s.loss_tangent >= 0.0)
        {
            return Err(TheoryError::Invalid(format!("section {s:?}")));
        }
    }
    let widest = segs.iter().map(|s| s.span).fold(0.0, f64::max);
    let k0 = 2.0 * PI * frequency / C0;
    let count = |s: &Seg| ((modes as f64 * s.span / widest).round() as usize).max(1);
    let kx2 = match basis {
        Basis::Sin => 0.0,
        Basis::Cos { a } => (PI / a).powi(2),
    };
    // β (Im ≤ 0) and the admittance Y of each segment's modes: β/μr (TE_m0), (k² − kx²)/(μr β)
    // (LSE^x_1n, from H_x/E_y = −(k² − kx²)/(ωμβ)); both β/μr for the fundamental.
    let props: Vec<(Vec<C>, Vec<C>)> = segs
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let k2 = C::new(1.0, -s.loss_tangent) * (k0 * k0 * s.eps_r * s.mu_r);
            let beta: Vec<C> = (0..count(s)).map(|k| crate::decaying_sqrt(k2 - kx2 - (basis.index(k) * PI / s.span).powi(2))).collect();
            if beta.first().is_some_and(|b| b.norm() < 1e-9 * k0.max(1.0)) {
                return Err(TheoryError::Cutoff(i));
            }
            let y = match basis {
                Basis::Sin => beta.iter().map(|b| b / s.mu_r).collect(),
                Basis::Cos { .. } => beta.iter().map(|b| (k2 - kx2) / (b * s.mu_r)).collect(),
            };
            Ok((beta, y))
        })
        .collect::<Result<_, _>>()?;
    let travel = |i: usize| -> Vec<C> { props[i].0.iter().map(|b| (C::new(0.0, -1.0) * b * segs[i].length).exp()).collect() };
    let mut total = Gsm::line(&travel(0));
    let tol = 1e-9 * widest;
    for i in 1..segs.len() {
        let (a, b) = (&segs[i - 1], &segs[i]);
        let holds = |p: &Seg, q: &Seg| p.t0 <= q.t0 + tol && q.t0 + q.span <= p.t0 + p.span + tol;
        let j = if holds(a, b) {
            junction(basis, a, &props[i - 1].1, b, &props[i].1)?
        } else if holds(b, a) {
            // The wide guide on the right: the same junction seen from the other side.
            let g = junction(basis, b, &props[i].1, a, &props[i - 1].1)?;
            Gsm { ll: g.rr, lr: g.rl, rl: g.lr, rr: g.ll }
        } else {
            return Err(TheoryError::Invalid(format!("sections {i} and {}: apertures overlap only in part", i + 1)));
        };
        total = total.star(&j)?.star(&Gsm::line(&travel(i)))?;
    }
    let (first, last) = (0, segs.len() - 1);
    let power = |i: usize| basis.norm(0, segs[i].span) * props[i].1[0];
    let factor = (power(last) / power(first)).sqrt();
    Ok((total.ll.at(0, 0), total.rl.at(0, 0) * factor))
}

impl HPlaneLine {
    /// S₁₁ and S₂₁ of the TE₁₀ wave (power waves, referred to the end faces) with `modes` TE_m0
    /// modes in the widest section and proportionally fewer in narrower ones.
    pub fn solve(&self, frequency: f64, modes: usize) -> Result<(C, C), TheoryError> {
        let segs: Vec<Seg> = self
            .sections
            .iter()
            .map(|s| Seg { length: s.length, eps_r: s.eps_r, mu_r: s.mu_r, loss_tangent: s.loss_tangent, t0: s.x0, span: s.width })
            .collect();
        solve_line(Basis::Sin, &segs, frequency, modes)
    }
}

/// A section of an E-plane line: its length, material and aperture across the narrow wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ESection {
    pub length: f64,
    pub eps_r: f64,
    pub mu_r: f64,
    pub loss_tangent: f64,
    /// The aperture's start and height along y (m).
    pub y0: f64,
    pub height: f64,
}

/// A waveguide of width `a` whose narrow wall changes from section to section (E-plane steps,
/// capacitive irises), all sections of one material. With the TE₁₀ field's sin(πx/a) common to
/// every section, the fields are TE to x (E_x = 0, the LSE^x_1n modes, from the potential
/// ψ = sin(πx/a) cos(nπ(y − y_i)/b_i) e^{−γz}); matching E_y and H_x on the aperture also matches
/// H_y when the material on both sides is the same (H_x and H_y both follow ψ). Mode matching and
/// the cascade are as for the H-plane, with the cos basis (Uher, Bornemann, Rosenberg 1993, §2.2).
#[derive(Clone, Debug, PartialEq)]
pub struct EPlaneLine {
    pub a: f64,
    pub sections: Vec<ESection>,
}

impl EPlaneLine {
    /// S₁₁ and S₂₁ of the TE₁₀ wave (power waves) with `modes` LSE modes in the tallest section.
    pub fn solve(&self, frequency: f64, modes: usize) -> Result<(C, C), TheoryError> {
        if !(self.a.is_finite() && self.a > 0.0) {
            return Err(TheoryError::Invalid("width".into()));
        }
        if self.sections.windows(2).any(|w| w[0].eps_r != w[1].eps_r || w[0].mu_r != w[1].mu_r || w[0].loss_tangent != w[1].loss_tangent) {
            return Err(TheoryError::Invalid("an E-plane line of more than one material (LSM modes would couple)".into()));
        }
        let segs: Vec<Seg> = self
            .sections
            .iter()
            .map(|s| Seg { length: s.length, eps_r: s.eps_r, mu_r: s.mu_r, loss_tangent: s.loss_tangent, t0: s.y0, span: s.height })
            .collect();
        solve_line(Basis::Cos { a: self.a }, &segs, frequency, modes)
    }
}

/// A section of a rectangular line whose aperture may change in both width and height: its
/// length, material and aperture [x0, x0 + a] × [y0, y0 + b] (m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectSection {
    pub length: f64,
    pub eps_r: f64,
    pub mu_r: f64,
    pub loss_tangent: f64,
    pub x0: f64,
    pub a: f64,
    pub y0: f64,
    pub b: f64,
}

/// A rectangular line of sections whose apertures change across x, y or both, nesting or
/// overlapping in part: mode matching of the vector TE_mn and TM_mn modes (R. R. Mansour,
/// R. H. MacPhie, *An improved transmission matrix formulation of cascaded discontinuities and
/// its application to E-plane circuits*, IEEE Trans. MTT 34(12):1490–1498, 1986; H. Patzelt,
/// F. Arndt, *Double-plane steps in rectangular waveguides and their application for
/// transformers, irises, and filters*, IEEE Trans. MTT 30(5):771–776, 1982).
///
/// Each section's modes are those whose cut-off lies below one wavenumber, so a larger aperture
/// keeps more (relative convergence). The field on a junction's plane lies on the two
/// apertures' intersection: a junction is two nested ones through a guide of that intersection
/// of no length (one of them alone when an aperture holds the other).
#[derive(Clone, Debug, PartialEq)]
pub struct RectLine {
    pub sections: Vec<RectSection>,
}

/// A rectangular aperture: [x0, x0 + a] × [y0, y0 + b].
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x0: f64,
    a: f64,
    y0: f64,
    b: f64,
}

impl Rect {
    fn of(s: &RectSection) -> Rect {
        Rect { x0: s.x0, a: s.a, y0: s.y0, b: s.b }
    }

    fn holds(&self, o: &Rect, tol: f64) -> bool {
        self.x0 <= o.x0 + tol && o.x0 + o.a <= self.x0 + self.a + tol && self.y0 <= o.y0 + tol && o.y0 + o.b <= self.y0 + self.b + tol
    }

    /// The intersection, `None` when the two do not overlap with area.
    fn meet(&self, o: &Rect) -> Option<Rect> {
        let (x0, x1) = (self.x0.max(o.x0), (self.x0 + self.a).min(o.x0 + o.a));
        let (y0, y1) = (self.y0.max(o.y0), (self.y0 + self.b).min(o.y0 + o.b));
        (x1 > x0 && y1 > y0).then_some(Rect { x0, a: x1 - x0, y0, b: y1 - y0 })
    }
}

/// A vector mode of a rectangular guide: TE_mn (m + n ≥ 1) or TM_mn (m, n ≥ 1). Its transverse
/// E is (cx cos(kx u) sin(ky v), cy sin(kx u) cos(ky v)), u and v from the aperture's corner:
/// TE (ky, −kx), TM (kx, ky).
#[derive(Clone, Copy, Debug, PartialEq)]
struct VMode {
    te: bool,
    kx: f64,
    ky: f64,
}

impl VMode {
    fn coefficients(&self) -> (f64, f64) {
        if self.te { (self.ky, -self.kx) } else { (self.kx, self.ky) }
    }

    fn kc2(&self) -> f64 {
        self.kx * self.kx + self.ky * self.ky
    }
}

/// The modes of aperture `r` with cut-off wavenumber up to `kc` (at least the fundamental),
/// lowest first; at most `cap`.
fn vmodes(r: &Rect, kc: f64, cap: usize) -> Vec<VMode> {
    let (mmax, nmax) = ((kc * r.a / PI).floor() as usize, (kc * r.b / PI).floor() as usize);
    let mut out = Vec::new();
    for m in 0..=mmax.min(4000) {
        for n in 0..=nmax.min(4000) {
            let (kx, ky) = (m as f64 * PI / r.a, n as f64 * PI / r.b);
            if kx * kx + ky * ky > kc * kc * (1.0 + 1e-12) {
                continue;
            }
            if m + n >= 1 {
                out.push(VMode { te: true, kx, ky });
            }
            if m >= 1 && n >= 1 {
                out.push(VMode { te: false, kx, ky });
            }
        }
    }
    out.sort_by(|p, q| p.kc2().total_cmp(&q.kc2()).then(q.te.cmp(&p.te)));
    if out.is_empty() {
        let (kx, ky) = if r.a >= r.b { (PI / r.a, 0.0) } else { (0.0, PI / r.b) };
        out.push(VMode { te: true, kx, ky });
    }
    out.truncate(cap.max(1));
    out
}

/// ∫ over [lo, hi] of f(k1 (t − s1)) f(k2 (t − s2)), f cos (`cos`) or sin.
fn int_product(cos: bool, (k1, s1): (f64, f64), (k2, s2): (f64, f64), (lo, hi): (f64, f64)) -> f64 {
    // A = k1 t − k1 s1, B = k2 t − k2 s2.
    let d = int_cos(k1 - k2, -k1 * s1 + k2 * s2, lo, hi);
    let s = int_cos(k1 + k2, -k1 * s1 - k2 * s2, lo, hi);
    if cos { 0.5 * (d + s) } else { 0.5 * (d - s) }
}

/// ∫ e_p⁽¹⁾ · e_q⁽²⁾ over `region` of mode `p` of aperture `r1` and `q` of `r2`.
fn vproduct(p: &VMode, r1: &Rect, q: &VMode, r2: &Rect, region: &Rect) -> f64 {
    let (cx1, cy1) = p.coefficients();
    let (cx2, cy2) = q.coefficients();
    let xs = (region.x0, region.x0 + region.a);
    let ys = (region.y0, region.y0 + region.b);
    let ex = cx1 * cx2 * int_product(true, (p.kx, r1.x0), (q.kx, r2.x0), xs) * int_product(false, (p.ky, r1.y0), (q.ky, r2.y0), ys);
    let ey = cy1 * cy2 * int_product(false, (p.kx, r1.x0), (q.kx, r2.x0), xs) * int_product(true, (p.ky, r1.y0), (q.ky, r2.y0), ys);
    ex + ey
}

/// A guide's modes with their propagation constants and admittances (per 1/(ωμ₀)): TE β/μr,
/// TM k²/(μr β), k² = k₀² εr μr (1 − j tan δ).
struct Guide {
    rect: Rect,
    modes: Vec<VMode>,
    beta: Vec<C>,
    y: Vec<C>,
    norm: Vec<f64>,
}

impl Guide {
    fn new(rect: Rect, s: &RectSection, k0: f64, kc: f64, cap: usize) -> Guide {
        let modes = vmodes(&rect, kc, cap);
        let k2 = C::new(1.0, -s.loss_tangent) * (k0 * k0 * s.eps_r * s.mu_r);
        let beta: Vec<C> = modes.iter().map(|m| crate::decaying_sqrt(k2 - m.kc2())).collect();
        let y = modes.iter().zip(&beta).map(|(m, b)| if m.te { b / s.mu_r } else { k2 / (b * s.mu_r) }).collect();
        let norm = modes.iter().map(|m| vproduct(m, &rect, m, &rect, &rect)).collect();
        Guide { rect, modes, beta, y, norm }
    }
}

/// The nested junction of guide `wide` and guide `narrow` whose aperture it holds: the GSM with
/// `wide` on the left (as [`junction`], with vector modes).
fn vjunction(wide: &Guide, narrow: &Guide) -> Result<Gsm, TheoryError> {
    let (n1, n2) = (wide.modes.len(), narrow.modes.len());
    let mut m = Mat::zeros(n1, n2);
    for (i, p) in wide.modes.iter().enumerate() {
        for (j, q) in narrow.modes.iter().enumerate() {
            m.v[i * n2 + j] = C::new(vproduct(p, &wide.rect, q, &narrow.rect, &narrow.rect), 0.0);
        }
    }
    let d1inv = Mat::diag(&wide.norm.iter().map(|d| C::new(1.0 / d, 0.0)).collect::<Vec<_>>());
    let d2y2 = Mat::diag(&narrow.y.iter().zip(&narrow.norm).map(|(y, d)| y * *d).collect::<Vec<_>>());
    let mt_y1 = m.transpose().mul(&Mat::diag(&wide.y));
    let w = d2y2.add(&mt_y1.mul(&d1inv.mul(&m)), ONE);
    let s21 = w.solve(&mt_y1.scale(C::new(2.0, 0.0)))?;
    let s22 = w.solve(&d2y2.scale(C::new(2.0, 0.0)))?.add(&Mat::identity(n2), -ONE);
    let md = d1inv.mul(&m);
    let s11 = md.mul(&s21).add(&Mat::identity(n1), -ONE);
    let s12 = md.mul(&s22.add(&Mat::identity(n2), ONE));
    Ok(Gsm { ll: s11, lr: s12, rl: s21, rr: s22 })
}

/// A GSM seen from its other side.
fn flipped(g: Gsm) -> Gsm {
    Gsm { ll: g.rr, lr: g.rl, rl: g.lr, rr: g.ll }
}

impl RectLine {
    /// S₁₁ and S₂₁ of the fundamental mode (TE₁₀ for a wider than high guide; power waves,
    /// referred to the end faces), the modes of each section up to the cut-off the largest
    /// aperture's `modes`-th has.
    ///
    /// ```
    /// use emcraft_theory::modematch::{RectLine, RectSection};
    /// // WR-90, a step to a smaller guide centred in both width and height, and back.
    /// let s = |length: f64, a: f64, b: f64| RectSection { length, eps_r: 1.0, mu_r: 1.0, loss_tangent: 0.0, x0: (0.02286 - a) / 2.0, a, y0: (0.01016 - b) / 2.0, b };
    /// let line = RectLine { sections: vec![s(0.01, 0.02286, 0.01016), s(0.005, 0.018, 0.006), s(0.01, 0.02286, 0.01016)] };
    /// let (s11, s21) = line.solve(10e9, 120)?;
    /// assert!((s11.norm_sqr() + s21.norm_sqr() - 1.0).abs() < 1e-9); // lossless
    /// // A refused line: an aperture of no width.
    /// assert!(RectLine { sections: vec![s(0.01, 0.0, 0.01)] }.solve(10e9, 50).is_err());
    /// # Ok::<(), emcraft_theory::TheoryError>(())
    /// ```
    pub fn solve(&self, frequency: f64, modes: usize) -> Result<(C, C), TheoryError> {
        let secs = &self.sections;
        if !(frequency.is_finite() && frequency > 0.0) {
            return Err(TheoryError::Invalid(format!("frequency {frequency}")));
        }
        if secs.is_empty() || !(1..=1500).contains(&modes) {
            return Err(TheoryError::Invalid("line or mode count".into()));
        }
        for s in secs {
            if ![s.length, s.eps_r, s.mu_r, s.a, s.b].iter().all(|v| v.is_finite() && *v > 0.0)
                || !(s.x0.is_finite() && s.y0.is_finite())
                || !(s.loss_tangent.is_finite() && s.loss_tangent >= 0.0)
            {
                return Err(TheoryError::Invalid(format!("section {s:?}")));
            }
        }
        let k0 = 2.0 * PI * frequency / C0;
        // The cut-off wavenumber of the largest aperture's `modes`-th mode: every guide's
        // modes up to it.
        let largest =
            secs.iter().map(Rect::of).max_by(|p, q| (p.a * p.b).total_cmp(&(q.a * q.b))).ok_or_else(|| TheoryError::Invalid("no sections".into()))?;
        let mut kc = PI / largest.a.min(largest.b);
        while vmodes(&largest, kc, usize::MAX).len() < modes && kc < 1e9 {
            kc *= 1.05;
        }
        let cap = 3 * modes;
        let guides: Vec<Guide> = secs.iter().map(|s| Guide::new(Rect::of(s), s, k0, kc, cap)).collect();
        for (i, g) in guides.iter().enumerate() {
            if g.beta.first().is_some_and(|b| b.norm() < 1e-9 * k0.max(1.0)) {
                return Err(TheoryError::Cutoff(i));
            }
        }
        let travel = |i: usize| -> Vec<C> { guides[i].beta.iter().map(|b| (C::new(0.0, -1.0) * b * secs[i].length).exp()).collect() };
        let mut total = Gsm::line(&travel(0));
        let tol = 1e-9 * largest.a.max(largest.b);
        for i in 1..secs.len() {
            let (a, b) = (&guides[i - 1], &guides[i]);
            let j = if a.rect.holds(&b.rect, tol) {
                vjunction(a, b)?
            } else if b.rect.holds(&a.rect, tol) {
                flipped(vjunction(b, a)?)
            } else {
                // The field on the plane lies on the apertures' intersection: through a guide
                // of it, of no length.
                let o = a.rect.meet(&b.rect).ok_or_else(|| TheoryError::Invalid(format!("sections {i} and {}: apertures apart", i + 1)))?;
                let mid = Guide::new(o, &secs[i - 1], k0, kc, cap);
                vjunction(a, &mid)?.star(&flipped(vjunction(b, &mid)?))?
            };
            total = total.star(&j)?.star(&Gsm::line(&travel(i)))?;
        }
        let power = |g: &Guide| g.norm[0] * g.y[0];
        let factor = (power(&guides[secs.len() - 1]) / power(&guides[0])).sqrt();
        Ok((total.ll.at(0, 0), total.rl.at(0, 0) * factor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Layer, Te10Line};

    const A: f64 = 0.02286;

    fn full(length: f64, eps_r: f64) -> HSection {
        HSection { length, eps_r, mu_r: 1.0, x0: 0.0, width: A, loss_tangent: 0.0 }
    }

    fn iris(length: f64, width: f64) -> HSection {
        HSection { length, eps_r: 1.0, mu_r: 1.0, x0: (A - width) / 2.0, width, loss_tangent: 0.0 }
    }

    #[test]
    fn equal_apertures_reduce_to_the_te10_cascade() {
        let line = HPlaneLine { sections: vec![full(0.015, 2.0), full(0.02, 1.0), full(0.015, 2.0)] };
        let exact =
            Te10Line { a: A, layers: vec![Layer::lossless(0.015, 2.0, 1.0), Layer::lossless(0.02, 1.0, 1.0), Layer::lossless(0.015, 2.0, 1.0)] };
        for f in [5.0e9, 8.0e9] {
            let (s11, s21) = line.solve(f, 12).unwrap();
            let (e11, e21) = exact.solve(f).unwrap().s_power();
            assert!((s11 - e11).norm() < 1e-12 && (s21 - e21).norm() < 1e-12, "{f}");
        }
    }

    #[test]
    fn coupling_integrals_match_quadrature() {
        let (x1, a1, x2, a2) = (0.0, A, 0.004, 0.012);
        let m = coupling(Basis::Sin, (5, x1, a1), (3, x2, a2));
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
        approx::assert_abs_diff_eq!(s11.norm_sqr() + s21.norm_sqr(), 1.0, epsilon = 1e-10);
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
        let side = HSection { length: 0.015, eps_r: 1.0, mu_r: 1.0, x0: 0.0, width: 0.016, loss_tangent: 0.0 };
        let fwd = HPlaneLine { sections: vec![full(0.01, 1.0), side] };
        let rev = HPlaneLine { sections: vec![side, full(0.01, 1.0)] };
        let f = 12.0e9;
        let (s11, s21) = fwd.solve(f, 60).unwrap();
        let (_, s12) = rev.solve(f, 60).unwrap();
        assert!((s21 - s12).norm() < 1e-10);
        approx::assert_abs_diff_eq!(s11.norm_sqr() + s21.norm_sqr(), 1.0, epsilon = 1e-10);
    }

    #[test]
    fn e_plane_lines_reduce_conserve_and_converge() {
        let b = 0.01016;
        let full = |l: f64| ESection { length: l, eps_r: 1.0, mu_r: 1.0, loss_tangent: 0.0, y0: 0.0, height: b };
        // Equal heights: the empty guide's exact phase.
        let plain = EPlaneLine { a: A, sections: vec![full(0.01), full(0.02)] };
        let (r, t) = plain.solve(10.0e9, 20).unwrap();
        let exact = Te10Line { a: A, layers: vec![Layer::lossless(0.03, 1.0, 1.0)] }.solve(10.0e9).unwrap();
        assert!(r.norm() < 1e-12 && (t - exact.s21).norm() < 1e-12);
        // A capacitive iris (the narrow wall closed to 4 mm, centred, 1 mm thick).
        let iris = ESection { length: 0.001, y0: (b - 0.004) / 2.0, height: 0.004, ..full(0.0) };
        let line = EPlaneLine { a: A, sections: vec![full(0.01), iris, full(0.01)] };
        let (r, t) = line.solve(10.0e9, 80).unwrap();
        approx::assert_abs_diff_eq!(r.norm_sqr() + t.norm_sqr(), 1.0, epsilon = 1e-10);
        assert!(r.norm() > 0.1, "{r}");
        let (r2, _) = line.solve(10.0e9, 160).unwrap();
        assert!((r2 - r).norm() < 5e-3, "{}", (r2 - r).norm());
        // A step down to half the height, from both ends: reciprocal.
        let low = ESection { length: 0.015, y0: 0.0, height: b / 2.0, ..full(0.0) };
        let (_, f) = EPlaneLine { a: A, sections: vec![full(0.01), low] }.solve(10.0e9, 80).unwrap();
        let (_, g) = EPlaneLine { a: A, sections: vec![low, full(0.01)] }.solve(10.0e9, 80).unwrap();
        assert!((f - g).norm() < 1e-10);
        // Two materials are refused.
        assert!(EPlaneLine { a: A, sections: vec![full(0.01), ESection { eps_r: 2.0, ..low }] }.solve(10.0e9, 20).is_err());
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

    /// The vector mode matching reduces to the scalar lines it holds: an H-plane iris (only
    /// TE_m0 take part) and an E-plane iris (LSE^x_1n, made of TE and TM_1n) agree with the
    /// H-plane and E-plane solutions as the modes grow, and a double-plane step is lossless and
    /// reciprocal.
    #[test]
    fn vector_mode_matching_holds_the_scalar_lines() {
        let b = 0.01016;
        let sec = |length: f64, x0: f64, a: f64, y0: f64, h: f64| RectSection { length, eps_r: 1.0, mu_r: 1.0, loss_tangent: 0.0, x0, a, y0, b: h };
        // H-plane: an inductive iris 10 mm wide, 2 mm thick, off centre.
        let rect = RectLine { sections: vec![sec(0.01, 0.0, A, 0.0, b), sec(0.002, 0.004, 0.010, 0.0, b), sec(0.01, 0.0, A, 0.0, b)] };
        let h = HPlaneLine { sections: vec![full(0.01, 1.0), HSection { x0: 0.004, ..iris(0.002, 0.010) }, full(0.01, 1.0)] };
        // Of the vector modes, only the TE_m0 couple to an H-plane iris: with as many TE_m0 the
        // two agree to rounding (some count of the scalar line's is the vector set's).
        let (r, t) = rect.solve(10e9, 300).unwrap();
        let closest = (10..40).map(|n| h.solve(10e9, n).unwrap()).map(|(rh, th)| (r - rh).norm() + (t - th).norm()).fold(f64::INFINITY, f64::min);
        assert!(closest < 1e-9, "{closest}");
        // E-plane: a capacitive iris 4 mm high, 1 mm thick, centred.
        let rect = RectLine { sections: vec![sec(0.01, 0.0, A, 0.0, b), sec(0.001, 0.0, A, (b - 0.004) / 2.0, 0.004), sec(0.01, 0.0, A, 0.0, b)] };
        let full_e = |l: f64| ESection { length: l, eps_r: 1.0, mu_r: 1.0, loss_tangent: 0.0, y0: 0.0, height: b };
        let e = EPlaneLine {
            a: A,
            sections: vec![full_e(0.01), ESection { length: 0.001, y0: (b - 0.004) / 2.0, height: 0.004, ..full_e(0.0) }, full_e(0.01)],
        };
        // Here TE_1n and TM_1n (n ≥ 0) make the LSE^x_1n: the two agree as both converge.
        let (r, t) = rect.solve(10e9, 600).unwrap();
        let (re, te) = e.solve(10e9, 160).unwrap();
        assert!((r - re).norm() < 5e-3 && (t - te).norm() < 5e-3, "{r} {re} {t} {te}");
        // A double-plane step down and its mirror: lossless and reciprocal.
        let big = sec(0.01, 0.0, A, 0.0, b);
        let small = sec(0.012, 0.002, 0.019, 0.001, 0.0086);
        let (r, f) = RectLine { sections: vec![big, small] }.solve(10e9, 200).unwrap();
        let (_, g) = RectLine { sections: vec![small, big] }.solve(10e9, 200).unwrap();
        approx::assert_abs_diff_eq!(r.norm_sqr() + f.norm_sqr(), 1.0, epsilon = 1e-9);
        assert!((f - g).norm() < 1e-9, "{f} {g}");
    }
}

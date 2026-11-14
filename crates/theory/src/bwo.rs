//! The backward-wave oscillator's start of oscillation (docs/adr/0012-particles.md, stage D).
//!
//! A beam of velocity v₀ in step with a backward space harmonic of a slow-wave structure
//! (H. R. Johnson, *Backward-wave oscillators*, Proc. IRE 43:684–697, 1955; J. R. Pierce,
//! *Traveling-Wave Tubes*, Van Nostrand 1950, chs. II and VII): with the circuit field ∝
//! e^{−jβₑz + βₑCδz}, βₑ = ω/v₀, and a lossless circuit, the three waves' δ solve
//!
//! (δ² + 4QC)(δ + jb) = j,
//!
//! b the velocity parameter, (v₀ − v_p)/(C v_p), QC Pierce's space-charge parameter. The beam
//! enters unmodulated: its current and velocity, ∝ Σ Aᵢ/(δᵢ² + 4QC) and Σ Aᵢδᵢ/(δᵢ² + 4QC),
//! that is Σ Aᵢ(δᵢ + jb) and Σ Aᵢδᵢ(δᵢ + jb), vanish at z = 0; nothing comes in at the collector
//! end (Σ Aᵢ e^{2πCNδᵢ} = 0 at z = L, N = L/λₑ beam wavelengths). The three conditions'
//! determinant vanishes at the start of oscillation: CN = 0.314, b = 1.522 without space charge
//! (Johnson), CN rising with QC. Solved here by Newton's method on (CN, b), the cubic's roots by
//! the Durand–Kerner iteration.
//!
//! **Pierce's gain parameter** C³ = K I₀/(4V₀), K = |E|²/(2β²P) the coupling impedance of the
//! harmonic. A relativistic beam bunches under its longitudinal mass γ³m: C³ = K I₀ q/(2 γ³ m
//! v₀²) (B. Levush, T. M. Antonsen, A. Bromborsky, W. R. Lou, Y. Carmel, *Theory of relativistic
//! backward-wave oscillators with end reflections*, IEEE Trans. Plasma Sci. 20(3):263–280,
//! 1992), which is Pierce's at v₀ ≪ c. So the start current is I_st = (CN/N)³ · 2γ³mv₀²/(qK).
//!
//! **Space charge** 4QC = (ω_q/(ωC))², ω_q = R ω_p/γ^{3/2} the reduced plasma frequency
//! (ω_p² = n q²/(ε₀m)). For a thin ring beam of radius b and thickness t inside a conducting
//! tube of radius a, the ring's own field of a modulation e^{−jβₑz} decays as
//! I₀(κr), K₀(κr), κ = βₑ/γ, and against that of an unbounded beam reduces it by
//! R² = βₑ² t b G/γ², G = I₀(κb)[K₀(κb) − K₀(κa) I₀(κb)/I₀(κa)] (G. M. Branch, T. G. Mihran,
//! *Plasma frequency reduction factors in electron beams*, IRE Trans. Electron Devices ED-2:3–11,
//! 1955, here for a thin ring and relativistic κ).

use emcraft_model::units::{C0, EPS0};
use num_complex::Complex64 as Z;

use crate::bessel::{i0, k0};

/// Johnson's start condition: CN and the velocity parameter b.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Start {
    pub cn: f64,
    pub b: f64,
}

/// The roots of (δ² + 4QC)(δ + jb) − j by the Durand–Kerner iteration, bounded.
fn roots(b: f64, qc: f64) -> [Z; 3] {
    let jb = Z::new(0.0, b);
    let p = |z: Z| (z * z + 4.0 * qc) * (z + jb) - Z::new(0.0, 1.0);
    let scale = 1.0 + b.abs() + qc.abs().sqrt();
    let mut r: [Z; 3] = std::array::from_fn(|i| Z::from_polar(scale, 0.4 + 2.1 * i as f64));
    for _ in 0..500 {
        let mut moved = 0.0f64;
        for i in 0..3 {
            let denom = (0..3).filter(|j| *j != i).fold(Z::new(1.0, 0.0), |acc, j| acc * (r[i] - r[j]));
            if denom.norm() == 0.0 {
                continue;
            }
            let step = p(r[i]) / denom;
            r[i] -= step;
            moved = moved.max(step.norm() / scale);
        }
        if moved < 1e-15 {
            break;
        }
    }
    r
}

/// The boundary conditions' determinant at (CN, b).
fn determinant(cn: f64, b: f64, qc: f64) -> Z {
    let d = roots(b, qc);
    let jb = Z::new(0.0, b);
    let rows: [[Z; 3]; 3] = [d.map(|x| x + jb), d.map(|x| x * (x + jb)), d.map(|x| (2.0 * std::f64::consts::PI * cn * x).exp())];
    let m = |i: usize, j: usize| rows[1][i] * rows[2][j] - rows[1][j] * rows[2][i];
    rows[0][0] * m(1, 2) - rows[0][1] * m(0, 2) + rows[0][2] * m(0, 1)
}

/// Johnson's start of oscillation of a lossless BWO at space-charge parameter `qc` (0 to 1):
/// CN = 0.314, b = 1.522 without space charge. None if Newton's method does not settle.
///
/// ```
/// use emcraft_theory::bwo::johnson_start;
/// let s = johnson_start(0.0).unwrap();
/// assert!((s.cn - 0.314).abs() < 5e-4 && (s.b - 1.522).abs() < 5e-4);
/// // Space charge raises the start.
/// assert!(johnson_start(0.25).unwrap().cn > 0.34);
/// assert!(johnson_start(f64::NAN).is_none());
/// ```
pub fn johnson_start(qc: f64) -> Option<Start> {
    if !(qc.is_finite() && (0.0..=1.0).contains(&qc)) {
        return None;
    }
    let (mut cn, mut b) = (0.3, 1.5);
    let h = 1e-7;
    // From no space charge to qc in steps, each Newton's start the last step's root.
    let stages = (qc / 0.05).ceil().max(1.0) as usize;
    for stage in 1..=stages {
        let q = qc * stage as f64 / stages as f64;
        let mut settled = false;
        for _ in 0..100 {
            let f = determinant(cn, b, q);
            if f.norm() < 1e-13 {
                settled = true;
                break;
            }
            let (fa, fb) = ((determinant(cn + h, b, q) - f) / h, (determinant(cn, b + h, q) - f) / h);
            let det = fa.re * fb.im - fb.re * fa.im;
            if det == 0.0 || !det.is_finite() {
                return None;
            }
            cn -= (f.re * fb.im - f.im * fb.re) / det;
            b -= (fa.re * f.im - fa.im * f.re) / det;
        }
        if !settled {
            return None;
        }
    }
    Some(Start { cn, b })
}

/// A thin ring beam: its radius and thickness, and the radius of the conducting tube around it
/// (m), for its space charge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub radius: f64,
    pub thickness: f64,
    pub tube: f64,
}

/// A backward-wave oscillator: the coupling impedance on the beam (Ω), the length (m) and the
/// frequency (Hz); the beam at `gamma0`, of particles of `charge` (C) and `mass` (kg), its space
/// charge counted if it is a `ring`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oscillator {
    pub impedance: f64,
    pub length: f64,
    pub frequency: f64,
    pub gamma0: f64,
    pub charge: f64,
    pub mass: f64,
    pub ring: Option<Ring>,
}

impl Oscillator {
    fn valid(&self) -> bool {
        let ring = self
            .ring
            .is_none_or(|r| [r.radius, r.thickness, r.tube].iter().all(|x| x.is_finite() && *x > 0.0) && r.radius + 0.5 * r.thickness < r.tube);
        [self.impedance, self.length, self.frequency, self.gamma0, self.charge, self.mass].iter().all(|x| x.is_finite())
            && self.impedance > 0.0
            && self.length > 0.0
            && self.frequency > 0.0
            && self.gamma0 > 1.0
            && self.charge != 0.0
            && self.mass > 0.0
            && ring
    }

    fn speed(&self) -> f64 {
        C0 * (1.0 - 1.0 / (self.gamma0 * self.gamma0)).sqrt()
    }

    /// 2γ³mv₀²/(qK): the current (A) at C = 1.
    fn scale(&self) -> f64 {
        let v0 = self.speed();
        2.0 * self.gamma0.powi(3) * self.mass * v0 * v0 / (self.charge.abs() * self.impedance)
    }

    /// Pierce's QC at `current` (A); 0 without a ring.
    ///
    /// ```
    /// use emcraft_theory::bwo::{Oscillator, Ring};
    /// let ring = Ring { radius: 8e-3, thickness: 1e-3, tube: 10e-3 };
    /// let o = Oscillator { impedance: 0.6, length: 0.24, frequency: 11.8e9, gamma0: 1.0 + 1e5 / 510_998.95, charge: -1.602_176_634e-19, mass: 9.109_383_701_5e-31, ring: Some(ring) };
    /// let qc = o.space_charge(5.0).unwrap();
    /// assert!(qc > 0.05 && qc < 0.3);
    /// // QC grows as the cube root of the current.
    /// assert!((o.space_charge(40.0).unwrap() / qc - 2.0).abs() < 1e-9);
    /// ```
    pub fn space_charge(&self, current: f64) -> Option<f64> {
        if !(self.valid() && current.is_finite() && current > 0.0) {
            return None;
        }
        let Some(ring) = self.ring else { return Some(0.0) };
        let v0 = self.speed();
        let w = 2.0 * std::f64::consts::PI * self.frequency;
        let beta = w / v0;
        let kappa = beta / self.gamma0;
        let (ib, ia) = (i0(kappa * ring.radius)?, i0(kappa * ring.tube)?);
        let g = ib * (k0(kappa * ring.radius)? - k0(kappa * ring.tube)? * ib / ia);
        let r2 = beta * beta * ring.thickness * ring.radius * g / (self.gamma0 * self.gamma0);
        let density = current / (self.charge.abs() * v0 * 2.0 * std::f64::consts::PI * ring.radius * ring.thickness);
        let wq2 = r2 * density * self.charge * self.charge / (EPS0 * self.mass) / self.gamma0.powi(3);
        let c = (current / self.scale()).cbrt();
        Some(wq2 / (w * c).powi(2) / 4.0)
    }

    /// The start current (A): (CN/N)³ times 2γ³mv₀²/(qK), CN Johnson's at the space charge of
    /// that current (a fixed point, bounded). None for hostile input.
    ///
    /// ```
    /// use emcraft_theory::bwo::{Oscillator, Ring};
    /// // 100 kV electrons (γ₀ = 1.1957, β₀ = 0.5482), 1 Ω, 12 GHz, 120 mm: N = 8.762, 24.18 A.
    /// let mut o = Oscillator { impedance: 1.0, length: 0.12, frequency: 12e9, gamma0: 1.0 + 1e5 / 510_998.95, charge: -1.602_176_634e-19, mass: 9.109_383_701_5e-31, ring: None };
    /// let i = o.start_current().unwrap();
    /// assert!((i / 24.18 - 1.0).abs() < 2e-3);
    /// // Twice as long: an eighth of it (Johnson's L⁻³).
    /// o.length = 0.24;
    /// assert!((i / o.start_current().unwrap() - 8.0).abs() < 1e-9);
    /// // A ring's space charge raises it.
    /// o.ring = Some(Ring { radius: 8e-3, thickness: 1e-3, tube: 10e-3 });
    /// assert!(o.start_current().unwrap() > i / 8.0);
    /// o.impedance = 0.0;
    /// assert!(o.start_current().is_none());
    /// ```
    pub fn start_current(&self) -> Option<f64> {
        if !self.valid() {
            return None;
        }
        let n = self.length * self.frequency / self.speed();
        let at = |cn: f64| (cn / n).powi(3) * self.scale();
        let mut current = at(johnson_start(0.0)?.cn);
        for _ in 0..100 {
            let qc = self.space_charge(current)?;
            let next = at(johnson_start(qc.min(1.0))?.cn);
            let settled = (next - current).abs() < 1e-12 * current;
            current = next;
            if settled {
                return Some(current);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_condition_is_johnsons() {
        // Johnson (1955), Fig. 6 at QC = 0, d = 0: CN = 0.314, b = 1.522; the waves then cancel at
        // the gun to the iteration's tolerance.
        let s = johnson_start(0.0).unwrap();
        approx::assert_abs_diff_eq!(s.cn, 0.314, epsilon = 2e-4);
        approx::assert_abs_diff_eq!(s.b, 1.522, epsilon = 3e-4);
        assert!(determinant(s.cn, s.b, 0.0).norm() < 1e-12);
        // Johnson's Fig. 6: CN rises with QC, slowly at first.
        let cns: Vec<f64> = [0.1, 0.2, 0.3, 0.4].iter().map(|q| johnson_start(*q).unwrap().cn).collect();
        assert!(cns.windows(2).all(|w| w[1] > w[0]) && cns[0] > 0.314 && cns[0] < 0.33);
    }

    #[test]
    fn a_slow_beams_start_current_is_pierces() {
        // At v₀ ≪ c the relativistic C³ is Pierce's K I/(4V₀) (Pierce 1950, ch. II), V₀ = m v₀²/2q.
        let (q, m) = (-1.602_176_634e-19f64, 9.109_383_701_5e-31);
        let v0 = 1e-3 * C0;
        let gamma0 = 1.0 / (1.0 - 1e-6f64).sqrt();
        let o = Oscillator { impedance: 5.0, length: 0.1, frequency: 3e9, gamma0, charge: q, mass: m, ring: None };
        let v = 0.5 * m * v0 * v0 / q.abs();
        let pierce = 4.0 * v * (0.314_055 / (0.1 * 3e9 / v0)).powi(3) / 5.0;
        approx::assert_abs_diff_eq!(o.start_current().unwrap() / pierce, 1.0, epsilon = 1e-5);
    }

    #[test]
    fn a_thin_rings_space_charge_is_reduced_as_branch_and_mihran() {
        // A ring far from the wall and thin against 1/κ: G → I₀K₀(κb), and at κb ≫ 1 that tends to
        // 1/(2κb) (Abramowitz & Stegun 9.7.1–9.7.2), so R² → βₑ² t/(2κγ²) = βₑ t/(2γ).
        let ring = Ring { radius: 0.05, thickness: 1e-4, tube: 0.3 };
        let o = Oscillator {
            impedance: 1.0,
            length: 0.1,
            frequency: 30e9,
            gamma0: 1.1,
            charge: -1.602_176_634e-19,
            mass: 9.109_383_701_5e-31,
            ring: Some(ring),
        };
        let (v0, w) = (o.speed(), 2.0 * std::f64::consts::PI * o.frequency);
        let current = 3.0;
        let density = current / (o.charge.abs() * v0 * 2.0 * std::f64::consts::PI * ring.radius * ring.thickness);
        let wp2 = density * o.charge * o.charge / (EPS0 * o.mass);
        let r2 = (w / v0) * ring.thickness / (2.0 * o.gamma0);
        let c = (current / o.scale()).cbrt();
        let want = r2 * wp2 / o.gamma0.powi(3) / (w * c).powi(2) / 4.0;
        // κb = 69: the asymptote's next term is 1/(8(κb)²) relative, the wall's e^{−2κ(a−b)}.
        approx::assert_abs_diff_eq!(o.space_charge(current).unwrap() / want, 1.0, epsilon = 1e-3);
    }
}

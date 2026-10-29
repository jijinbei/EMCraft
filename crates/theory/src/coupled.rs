//! Edge-coupled stripline: two strips of no thickness, width W, a gap S apart, centred between
//! ground planes b apart in εr, and the circuits made of them — the parallel-coupled section and
//! the parallel-coupled band-pass filter.
//!
//! **Even and odd impedances** by Cohn's exact conformal mapping (S. B. Cohn, *Shielded
//! coupled-strip transmission line*, IRE Trans. MTT 3(5):29–38, 1955):
//! Z₀ₑ = η₀/(4√εr) · K(kₑ′)/K(kₑ), kₑ = tanh(πW/2b) tanh(π(W + S)/2b), and Z₀ₒ alike with
//! kₒ = tanh(πW/2b) coth(π(W + S)/2b). The mapping inverts in closed form: kₑkₒ = tanh²(πW/2b),
//! kₑ/kₒ = tanh²(π(W + S)/2b).
//!
//! **The coupled section** (two lines coupled over a length, one end of each open: the
//! band-pass section) has, with θ the electrical length, Z₁₁ = Z₂₂ = −j(Z₀ₑ + Z₀ₒ)/2 · cot θ and
//! Z₁₂ = −j(Z₀ₑ − Z₀ₒ)/2 · csc θ (E. M. T. Jones, J. T. Bolljahn, *Coupled-strip-transmission-line
//! filters and directional couplers*, IRE Trans. MTT 4(2):75–81, 1956; D. M. Pozar, *Microwave
//! Engineering*, 4th ed., Wiley 2012, §8.7, Table 8.8). In homogeneous stripline both modes are
//! TEM with the same β = k₀√εr, so θ is one number. Its chain (ABCD) matrix:
//! A = D = (Z₀ₑ + Z₀ₒ)/(Z₀ₑ − Z₀ₒ) cos θ, C = 2j sin θ/(Z₀ₑ − Z₀ₒ),
//! B = j[(Z₀ₑ − Z₀ₒ)² − (Z₀ₑ + Z₀ₒ)² cos²θ]/(2(Z₀ₑ − Z₀ₒ) sin θ).
//!
//! **The directional coupler** (two lines coupled over their whole length, every end a port):
//! by symmetry its four ports split into the even mode (both lines alike: a line of Z₀ₑ) and the
//! odd mode (opposite: Z₀ₒ), each a two-port between the ports' impedance R with reflection Γ
//! and transmission T. Port 1 driven, the reflection is (Γₑ + Γₒ)/2, the coupled port (the other
//! line, same end) (Γₑ − Γₒ)/2, the through port (Tₑ + Tₒ)/2 and the isolated port
//! (Tₑ − Tₒ)/2. With R = √(Z₀ₑZ₀ₒ) it is matched and isolated at every frequency, the coupled
//! wave jC sin θ/(√(1 − C²) cos θ + j sin θ), the through √(1 − C²)/(√(1 − C²) cos θ + j sin θ),
//! 90° apart (D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley 2012, §7.6).
//!
//! **The filter** (S. B. Cohn, *Parallel-coupled transmission-line-resonator filters*, IRE Trans.
//! MTT 6(2):223–231, 1958; G. L. Matthaei, L. Young, E. M. T. Jones, *Microwave Filters,
//! Impedance-Matching Networks, and Coupling Structures*, McGraw-Hill 1964, §§4.05, 10.02):
//! n half-wave resonators coupled by n + 1 quarter-wave sections, each an admittance inverter
//! J with J₀₁Z₀ = √(πw/2g₀g₁), J_{j,j+1}Z₀ = πw/(2√(g_j g_{j+1})), J_{n,n+1}Z₀ = √(πw/2g_n g_{n+1})
//! for the fractional bandwidth w, and Z₀ₑ = Z₀[1 + JZ₀ + (JZ₀)²], Z₀ₒ = Z₀[1 − JZ₀ + (JZ₀)²];
//! g the Chebyshev low-pass prototype (Matthaei–Young–Jones §4.05).

use std::f64::consts::PI;

use emcraft_model::units::{C0, MU0};
use num_complex::Complex64 as C;

use crate::TheoryError;

/// Iteration cap of the arithmetic–geometric mean and of the bisections.
const MAX_STEPS: usize = 200;

/// The arithmetic–geometric mean of a, b > 0.
fn agm(mut a: f64, mut b: f64) -> f64 {
    for _ in 0..64 {
        if (a - b).abs() <= 1e-16 * a {
            break;
        }
        (a, b) = (0.5 * (a + b), (a * b).sqrt());
    }
    a
}

/// K(k′)/K(k) for 0 < k < 1: AGM(1, k′)/AGM(1, k), k′ = √((1 − k)(1 + k)).
fn ratio(k: f64) -> f64 {
    agm(1.0, ((1.0 - k) * (1.0 + k)).sqrt()) / agm(1.0, k)
}

/// η₀/4 = 30π (Ω).
fn quarter_eta0() -> f64 {
    MU0 * C0 / 4.0
}

fn bad(what: &str) -> TheoryError {
    TheoryError::Invalid(what.into())
}

/// The even and odd impedances (Ω) of strips `w` wide, a gap `s` apart, centred between planes
/// `b` apart in `eps_r` (Cohn 1955).
pub fn z0_even_odd(w: f64, s: f64, b: f64, eps_r: f64) -> Result<(f64, f64), TheoryError> {
    if ![w, s, b, eps_r].iter().all(|v| v.is_finite() && *v > 0.0) {
        return Err(bad("coupled stripline: W, S, b and εr must be finite and positive"));
    }
    let (x, y) = (PI * w / (2.0 * b), PI * (w + s) / (2.0 * b));
    let (ke, ko) = (x.tanh() * y.tanh(), x.tanh() / y.tanh());
    let z = quarter_eta0() / eps_r.sqrt();
    Ok((z * ratio(ke), z * ratio(ko)))
}

/// The modulus k whose K(k′)/K(k) is `r` > 0 (it falls as k rises), by bisection on k.
fn modulus(r: f64) -> f64 {
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..MAX_STEPS {
        let mid = 0.5 * (lo + hi);
        if mid <= lo || mid >= hi {
            break;
        }
        if ratio(mid) > r {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// The width W and gap S (m) of coupled strips between planes `b` apart in `eps_r` with even
/// and odd impedances `z0e` > `z0o` (Ω): Cohn's mapping inverted.
pub fn dimensions(z0e: f64, z0o: f64, b: f64, eps_r: f64) -> Result<(f64, f64), TheoryError> {
    if ![z0e, z0o, b, eps_r].iter().all(|v| v.is_finite() && *v > 0.0) || z0e <= z0o {
        return Err(bad("coupled stripline: impedances must be finite, positive, Z0e > Z0o"));
    }
    let z = quarter_eta0() / eps_r.sqrt();
    let (ke, ko) = (modulus(z0e / z), modulus(z0o / z));
    let (tx, ty) = ((ke * ko).sqrt(), (ke / ko).sqrt());
    if !(tx > 0.0 && tx < 1.0 && ty > 0.0 && ty < 1.0) {
        return Err(bad("coupled stripline: no strips have these impedances"));
    }
    let w = 2.0 * b * tx.atanh() / PI;
    let s = 2.0 * b * ty.atanh() / PI - w;
    Ok((w, s))
}

/// The coupling C = (Z₀ₑ − Z₀ₒ)/(Z₀ₑ + Z₀ₒ) of a quarter-wave coupled section.
pub fn coupling(z0e: f64, z0o: f64) -> f64 {
    (z0e - z0o) / (z0e + z0o)
}

/// A two-port's chain matrix [[A, B], [C, D]].
pub type Abcd = [[C; 2]; 2];

fn product(p: &Abcd, q: &Abcd) -> Abcd {
    let mut r = [[C::new(0.0, 0.0); 2]; 2];
    for i in 0..2 {
        for j in 0..2 {
            r[i][j] = p[i][0] * q[0][j] + p[i][1] * q[1][j];
        }
    }
    r
}

/// A uniform line of impedance `z` (Ω) and electrical length `theta` (rad).
pub fn line(z: f64, theta: f64) -> Abcd {
    let j = C::new(0.0, 1.0);
    let (c, s) = (C::new(theta.cos(), 0.0), theta.sin());
    [[c, j * z * s], [j * s / z, c]]
}

/// The coupled section with one end of each line open (the band-pass section), even and odd
/// impedances `z0e` > `z0o` (Ω), electrical length `theta` (rad, not a multiple of π).
pub fn section(z0e: f64, z0o: f64, theta: f64) -> Abcd {
    let j = C::new(0.0, 1.0);
    let (d, s) = (z0e - z0o, z0e + z0o);
    let (cos, sin) = (theta.cos(), theta.sin());
    let a = C::new(s / d * cos, 0.0);
    let b = j * ((d * d - s * s * cos * cos) / (2.0 * d * sin));
    let c = j * (2.0 * sin / d);
    [[a, b], [c, a]]
}

/// S11 and S21 of a two-port's chain matrix between ports referred to `z0` (Ω).
pub fn s_params(m: &Abcd, z0: f64) -> (C, C) {
    let [[a, b], [c, d]] = *m;
    let den = a + b / z0 + c * z0 + d;
    ((a + b / z0 - c * z0 - d) / den, C::new(2.0, 0.0) / den)
}

/// The waves out of a coupled-line directional coupler (even and odd impedances `z0e`, `z0o`
/// (Ω), electrical length `theta` (rad), every port referred to `r` (Ω)) for a unit wave into
/// port 1: [reflected, coupled (other line, same end), through (same line, far end), isolated
/// (other line, far end)], by the even and odd modes (module docs).
pub fn coupler(z0e: f64, z0o: f64, theta: f64, r: f64) -> [C; 4] {
    let (ge, te) = s_params(&line(z0e, theta), r);
    let (go, to) = s_params(&line(z0o, theta), r);
    [(ge + go) * 0.5, (ge - go) * 0.5, (te + to) * 0.5, (te - to) * 0.5]
}

/// The g values g₀ … g_{n+1} of the Chebyshev low-pass prototype of order `n` with passband
/// ripple `ripple_db` (dB) (Matthaei–Young–Jones §4.05, eqs. 4.05-5).
pub fn chebyshev(n: usize, ripple_db: f64) -> Result<Vec<f64>, TheoryError> {
    if !(1..=20).contains(&n) || !(ripple_db.is_finite() && ripple_db > 0.0 && ripple_db < 10.0) {
        return Err(bad("Chebyshev prototype: order 1 to 20, ripple between 0 and 10 dB"));
    }
    let beta = (1.0 / (ripple_db / 17.37).tanh()).ln();
    let gamma = (beta / (2.0 * n as f64)).sinh();
    let a = |k: usize| ((2 * k - 1) as f64 * PI / (2.0 * n as f64)).sin();
    let bk = |k: usize| gamma * gamma + (k as f64 * PI / n as f64).sin().powi(2);
    let mut g = vec![1.0, 2.0 * a(1) / gamma];
    for k in 2..=n {
        let prev = g[k - 1];
        g.push(4.0 * a(k - 1) * a(k) / (bk(k - 1) * prev));
    }
    g.push(if n % 2 == 1 { 1.0 } else { (1.0 / (beta / 4.0).tanh()).powi(2) });
    Ok(g)
}

/// One coupled section of a filter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoupledSection {
    pub z0e: f64,
    pub z0o: f64,
    /// The coupled length (m).
    pub length: f64,
}

/// A parallel-coupled band-pass filter in stripline: its sections in order between feed lines
/// of `z0` (Ω).
#[derive(Clone, Debug, PartialEq)]
pub struct CoupledFilter {
    pub z0: f64,
    pub eps_r: f64,
    pub sections: Vec<CoupledSection>,
}

/// What a coupled filter is designed for: its order, Chebyshev ripple (dB), centre `f0` (Hz),
/// fractional bandwidth, feeds of `z0` (Ω) in `eps_r`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoupledSpec {
    pub order: usize,
    pub ripple_db: f64,
    pub f0: f64,
    pub fbw: f64,
    pub z0: f64,
    pub eps_r: f64,
}

impl CoupledFilter {
    /// Cohn's design: order `n`, Chebyshev ripple `ripple_db`, centre `f0` (Hz), fractional
    /// bandwidth `fbw`, feeds of `z0` (Ω) in `eps_r`; every section a quarter wavelength at f0.
    pub fn design(spec: &CoupledSpec) -> Result<CoupledFilter, TheoryError> {
        let CoupledSpec { order: n, ripple_db, f0, fbw, z0, eps_r } = *spec;
        if ![f0, fbw, z0, eps_r].iter().all(|v| v.is_finite() && *v > 0.0) || fbw >= 1.0 {
            return Err(bad("coupled filter: f0, z0 and εr finite and positive, 0 < fbw < 1"));
        }
        let g = chebyshev(n, ripple_db)?;
        let quarter = C0 / (f0 * eps_r.sqrt()) / 4.0;
        let sections = (0..=n)
            .map(|j| {
                let jz = if j == 0 || j == n { (PI * fbw / (2.0 * g[j] * g[j + 1])).sqrt() } else { PI * fbw / (2.0 * (g[j] * g[j + 1]).sqrt()) };
                CoupledSection { z0e: z0 * (1.0 + jz + jz * jz), z0o: z0 * (1.0 - jz + jz * jz), length: quarter }
            })
            .collect();
        Ok(CoupledFilter { z0, eps_r, sections })
    }

    /// The ideal TEM cascade's chain matrix at `f` (Hz), each section `extra` (m) longer than
    /// its coupled length.
    pub fn abcd(&self, f: f64, extra: f64) -> Abcd {
        let beta = 2.0 * PI * f * self.eps_r.sqrt() / C0;
        let one = [[C::new(1.0, 0.0), C::new(0.0, 0.0)], [C::new(0.0, 0.0), C::new(1.0, 0.0)]];
        self.sections.iter().fold(one, |m, s| product(&m, &section(s.z0e, s.z0o, beta * (s.length + extra))))
    }

    /// S11 and S21 of the ideal cascade at `f` (Hz) between ports referred to the feeds' z0.
    pub fn s(&self, f: f64, extra: f64) -> (C, C) {
        s_params(&self.abcd(f, extra), self.z0)
    }
}

/// The x in [`lo`, `hi`] where `g` crosses zero, by bisection (`g(lo)` and `g(hi)` of opposite
/// signs), or `None`.
pub fn crossing(g: impl Fn(f64) -> f64, mut lo: f64, mut hi: f64) -> Option<f64> {
    let (mut glo, ghi) = (g(lo), g(hi));
    if !(glo * ghi <= 0.0) {
        return None;
    }
    for _ in 0..MAX_STEPS {
        let mid = 0.5 * (lo + hi);
        if mid <= lo || mid >= hi {
            break;
        }
        let gm = g(mid);
        if (gm <= 0.0) == (glo <= 0.0) {
            (lo, glo) = (mid, gm);
        } else {
            hi = mid;
        }
    }
    Some(0.5 * (lo + hi))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stripline;

    #[test]
    fn far_apart_strips_are_two_single_striplines() {
        let (w, b) = (2.4e-3, 3e-3);
        let (ze, zo) = z0_even_odd(w, 20.0 * b, b, 2.2).unwrap();
        let z = stripline::z0(w, b, 2.2);
        assert!((ze / z - 1.0).abs() < 1e-9 && (zo / z - 1.0).abs() < 1e-9, "{ze} {zo} {z}");
        // Close strips: the odd mode's impedance falls, the even's rises.
        let (ze, zo) = z0_even_odd(w, 0.2e-3, b, 2.2).unwrap();
        assert!(ze > z && zo < z);
    }

    #[test]
    fn cohns_mapping_inverts() {
        let (w, s, b) = (1.3e-3, 0.37e-3, 2e-3);
        let (ze, zo) = z0_even_odd(w, s, b, 2.2).unwrap();
        let (w2, s2) = dimensions(ze, zo, b, 2.2).unwrap();
        assert!((w2 - w).abs() < 1e-12 && (s2 - s).abs() < 1e-12, "{w2} {s2}");
        assert!(dimensions(40.0, 50.0, b, 2.2).is_err() && z0_even_odd(f64::NAN, s, b, 1.0).is_err());
    }

    #[test]
    fn chebyshev_prototypes_match_the_tables() {
        // Matthaei–Young–Jones Table 4.05-2(a), 0.1 dB ripple, n = 3: 1.0315, 1.1474, 1.0315, 1.
        let g = chebyshev(3, 0.1).unwrap();
        for (a, b) in g.iter().zip([1.0, 1.0315, 1.1474, 1.0315, 1.0]) {
            assert!((a - b).abs() < 1e-4, "{g:?}");
        }
        // 0.5 dB, n = 2: 1.4029, 0.7071, g3 = 1.9841.
        let g = chebyshev(2, 0.5).unwrap();
        for (a, b) in g.iter().zip([1.0, 1.4029, std::f64::consts::FRAC_1_SQRT_2, 1.9841]) {
            assert!((a - b).abs() < 1e-4, "{g:?}");
        }
        assert!(chebyshev(0, 0.1).is_err() && chebyshev(3, f64::NAN).is_err());
    }

    #[test]
    fn a_quarter_wave_section_is_an_inverter() {
        // At θ = π/2 the section is an inverter K = (Z0e − Z0o)/2: |S21| = 2K z0/(K² + z0²),
        // with z0 = √(Z0e Z0o), 2C√(1 − C²).
        let (ze, zo) = (70.0, 35.0);
        let m = section(ze, zo, PI / 2.0);
        let (s11, s21) = s_params(&m, (ze * zo).sqrt());
        let c = coupling(ze, zo);
        assert!((s21.norm() - 2.0 * c * (1.0 - c * c).sqrt()).abs() < 1e-12, "{s21}");
        assert!((s11.norm_sqr() + s21.norm_sqr() - 1.0).abs() < 1e-12);
        // The chain matrix of a reciprocal two-port: AD − BC = 1.
        let [[a, b], [cc, d]] = section(ze, zo, 0.7);
        assert!((a * d - b * cc - 1.0).norm() < 1e-12);
        let [[a, b], [cc, d]] = line(50.0, 0.3);
        assert!((a * d - b * cc - 1.0).norm() < 1e-12);
    }

    #[test]
    fn the_matched_coupler_is_pozars() {
        let (ze, zo, theta): (f64, f64, f64) = (69.37, 36.04, 1.1);
        let z0 = (ze * zo).sqrt();
        let [r, cpl, thr, iso] = coupler(ze, zo, theta, z0);
        let c = coupling(ze, zo);
        let j = C::new(0.0, 1.0);
        let root = (1.0 - c * c).sqrt();
        let den = root * theta.cos() + j * theta.sin();
        assert!(r.norm() < 1e-12 && iso.norm() < 1e-12, "{r} {iso}");
        assert!((cpl - j * c * theta.sin() / den).norm() < 1e-12 && (thr - root / den).norm() < 1e-12);
        // |coupled| = C sin θ/√(1 − C² cos²θ); lossless.
        assert!((cpl.norm() - c * theta.sin() / (1.0 - c * c * theta.cos().powi(2)).sqrt()).abs() < 1e-12);
        assert!((cpl.norm_sqr() + thr.norm_sqr() - 1.0).abs() < 1e-12);
        // Mismatched ports: still lossless, reflection and isolation no longer zero.
        let s = coupler(ze, zo, theta, 50.0 * 1.1);
        assert!((s.iter().map(|x| x.norm_sqr()).sum::<f64>() - 1.0).abs() < 1e-12 && s[0].norm() > 1e-3);
    }

    #[test]
    fn the_designed_filter_passes_its_band() {
        let f0 = 5e9;
        let filter = CoupledFilter::design(&CoupledSpec { order: 3, ripple_db: 0.1, f0, fbw: 0.1, z0: 50.0, eps_r: 2.2 }).unwrap();
        assert_eq!(filter.sections.len(), 4);
        // Symmetric: the outer sections alike, the inner ones alike.
        assert!((filter.sections[0].z0e - filter.sections[3].z0e).abs() < 1e-12);
        // Matched at f0 within the ripple; ripple 0.1 dB is |S21|² ≥ 0.9772 in the band.
        let (_, s21) = filter.s(f0, 0.0);
        assert!(s21.norm_sqr() > 0.97, "{s21}");
        for f in [0.97 * f0, 1.03 * f0] {
            assert!(filter.s(f, 0.0).1.norm_sqr() > 0.97);
        }
        // Far outside the band, little gets through.
        assert!(filter.s(0.8 * f0, 0.0).1.norm() < 0.1 && filter.s(1.2 * f0, 0.0).1.norm() < 0.1);
        // Lossless: |S11|² + |S21|² = 1.
        let (s11, s21) = filter.s(1.07 * f0, 0.0);
        assert!((s11.norm_sqr() + s21.norm_sqr() - 1.0).abs() < 1e-12);
        let edge = crossing(|f| filter.s(f, 0.0).1.norm_sqr() - 0.5, f0, 1.3 * f0).unwrap();
        assert!(edge > 1.04 * f0 && edge < 1.1 * f0, "{edge}");
    }

    /// The references of the oracle cases `coupled-stripline-impedance`, `coupled-line-coupler`
    /// and `coupled-line-bandpass` (their expected.json), from the strips as their recipes build
    /// them (mm).
    #[test]
    fn the_oracles_references() {
        let (mm, er) = (1e-3, 2.2);
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * b.abs().max(1.0);
        // Coupled stripline: W = 2.4, S = 0.6, b = 3.
        let (ze, zo) = z0_even_odd(2.4 * mm, 0.6 * mm, 3.0 * mm, er).unwrap();
        assert!(close(ze, 57.46078003811662) && close(zo, 42.824609251107255), "{ze} {zo}");
        // The coupled section: W = 2.3258, S = 0.1763, b = 4, coupled over 8.7217 between feeds
        // of 4, each grown by Δ at both ends, between ports of the feeds' impedance.
        let (b, w, s) = (4.0 * mm, 2.3258 * mm, 0.1763 * mm);
        let (ze, zo) = z0_even_odd(w, s, b, er).unwrap();
        let z = crate::stripline::z0(w, b, er);
        assert!(close(z, 62.21392318257151), "{z}");
        let (_, delta, _) = crate::stripline::open_end(w, b);
        let beta = |f: f64| 2.0 * PI * f * er.sqrt() / C0;
        let coupler = |f: f64| {
            let feed = line(z, beta(f) * (4.0 * mm - delta));
            s_params(&product(&product(&feed, &section(ze, zo, beta(f) * (8.7217 * mm + 2.0 * delta))), &feed), z).1
        };
        let s21 = coupler(5e9);
        assert!((s21.norm() - 0.5710836976449547).abs() < 2e-6 && (s21.arg().to_degrees() - -148.91680578559536).abs() < 1e-3, "{s21}");
        assert!((coupler(4e9).norm() - 0.5576297562569794).abs() < 2e-6 && (coupler(6e9).norm() - 0.5576297562569795).abs() < 2e-6);
        // At the quarter wave proper, between ports of √(Z0e Z0o): 2C√(1 − C²).
        let c = coupling(ze, zo);
        let (_, at_quarter) = s_params(&section(ze, zo, PI / 2.0), (ze * zo).sqrt());
        assert!((at_quarter.norm() - 2.0 * c * (1.0 - c * c).sqrt()).abs() < 1e-12);
        // The filter: (W, S, coupled length) per section, the feeds 3.3165 wide.
        let built = [(2.3258, 0.1763, 9.4139), (3.1217, 0.8514, 9.3828), (3.1217, 0.8514, 9.3828), (2.3258, 0.1763, 9.4139)];
        let filter = CoupledFilter {
            z0: crate::stripline::z0(3.3165 * mm, b, er),
            eps_r: er,
            sections: built
                .iter()
                .map(|&(w, s, l)| {
                    let (z0e, z0o) = z0_even_odd(w * mm, s * mm, b, er).unwrap();
                    CoupledSection { z0e, z0o, length: l * mm + crate::stripline::open_end(w * mm, b).1 }
                })
                .collect(),
        };
        // Built as designed: the design's impedances and quarter waves to rounding.
        let design = CoupledFilter::design(&CoupledSpec { order: 3, ripple_db: 0.1, f0: 5e9, fbw: 0.1, z0: 50.0, eps_r: er }).unwrap();
        for (a, d) in filter.sections.iter().zip(&design.sections) {
            assert!((a.z0e / d.z0e - 1.0).abs() < 1e-4 && (a.z0o / d.z0o - 1.0).abs() < 1e-4 && (a.length / d.length - 1.0).abs() < 1e-5);
        }
        let edge = |lo: f64, hi: f64| crossing(|f| filter.s(f, 0.0).1.norm() - 0.5f64.sqrt(), lo, hi).unwrap();
        assert!((edge(4e9, 5e9) - 4655558405.811216).abs() < 1e3 && (edge(5e9, 6e9) - 5344466478.938534).abs() < 1e3);
        let db = |f: f64| 20.0 * filter.s(f, 0.0).1.norm().log10();
        assert!((filter.s(5e9, 0.0).1.norm() - 0.9999999986744627).abs() < 1e-9);
        assert!((db(4.4e9) - -17.61693271520208).abs() < 1e-6 && (db(5.6e9) - -17.61614833364449).abs() < 1e-6);
    }

    /// The references of the oracle case `coupled-line-coupler-4port` (its expected.json): the
    /// strips as its recipe builds them (mm), 50 Ω terminals.
    #[test]
    fn the_four_port_couplers_references() {
        let (mm, er): (f64, f64) = (1e-3, 2.2);
        // Designed for 10 dB at Z0 = 50 Ω: Z0e,o = Z0 √((1 ± C)/(1 ∓ C)), W and S by Cohn
        // inverted, rounded to 0.1 µm as built.
        let c = 10f64.powf(-0.5);
        let (de, d_o) = (50.0 * ((1.0 + c) / (1.0 - c)).sqrt(), 50.0 * ((1.0 - c) / (1.0 + c)).sqrt());
        let (w, s) = dimensions(de, d_o, 4.0 * mm, er).unwrap();
        assert!((w / mm - 2.6938).abs() < 1e-4 && (s / mm - 0.1762).abs() < 1e-4, "{w} {s}");
        let (ze, zo) = z0_even_odd(2.6938 * mm, 0.1762 * mm, 4.0 * mm, er).unwrap();
        let theta = |f: f64| 2.0 * PI * f * er.sqrt() / C0 * 10.106 * mm;
        let db = |c: C| 20.0 * c.norm().log10();
        let at = |f: f64| coupler(ze, zo, theta(f), 50.0);
        let [r, cpl, thr, iso] = at(5e9);
        assert!((db(cpl) - -10.000081297631308).abs() < 1e-9 && (db(thr) - -0.4575658735703948).abs() < 1e-9);
        assert!((cpl.arg().to_degrees() - 0.000014341973769460684).abs() < 1e-6 && (thr.arg().to_degrees() - -89.99998565802623).abs() < 1e-6);
        assert!((r.norm() - 0.000013239767164191834).abs() < 1e-12 && (iso.norm() - 0.000004413209825693818).abs() < 1e-12);
        assert!(((cpl / thr).arg().to_degrees() - 90.0).abs() < 1e-9);
        assert!((db(at(4e9)[1]) - -10.394285400787203).abs() < 1e-9 && (db(at(6e9)[1]) - -10.394284047483557).abs() < 1e-9);
    }
}

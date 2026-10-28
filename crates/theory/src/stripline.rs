//! The stripline: a strip of no thickness, width W, centred between two infinite ground planes
//! b apart in a dielectric εr. Its TEM impedance by conformal mapping is exact: Z₀ = η₀/(4√εr) ·
//! K(k)/K(k′), k = sech(πW/2b), k′ = tanh(πW/2b) (S. B. Cohn, *Characteristic impedance of the
//! shielded-strip transmission line*, IRE Trans. MTT 2(2):52–57, 1954; D. M. Pozar, *Microwave
//! Engineering*, 4th ed., Wiley 2012, §3.7). K is the complete elliptic integral of the first
//! kind, by the arithmetic–geometric mean: K(k) = π / (2 AGM(1, √(1 − k²))) (Abramowitz &
//! Stegun 17.6).

use std::f64::consts::PI;

use emcraft_model::units::{C0, MU0};

/// The arithmetic–geometric mean of a and b > 0.
fn agm(mut a: f64, mut b: f64) -> f64 {
    for _ in 0..64 {
        if (a - b).abs() <= 1e-16 * a {
            break;
        }
        (a, b) = (0.5 * (a + b), (a * b).sqrt());
    }
    a
}

/// K(k), 0 ≤ k < 1.
pub fn elliptic_k(k: f64) -> f64 {
    PI / (2.0 * agm(1.0, (1.0 - k * k).sqrt()))
}

/// Z₀ (Ω) of a centred stripline of width `w` between planes `b` apart in εr.
pub fn z0(w: f64, b: f64, eps_r: f64) -> f64 {
    let x = PI * w / (2.0 * b);
    // K(k)/K(k′) = AGM(1, k)/AGM(1, k′): each modulus enters as the other's complement, so a
    // wide strip's k′ = tanh x ≈ 1 does not round √(1 − k′²) to zero.
    let (k, kp) = (1.0 / x.cosh(), x.tanh());
    MU0 * C0 / (4.0 * eps_r.sqrt()) * agm(1.0, k) / agm(1.0, kp)
}

/// The open end's extension Δ (m) of a strip `w` wide between planes `b` apart: the middle of
/// its bracket, between Δ₀ W/(W + 2Δ₀) (the end's fringing over the strip's width only, the
/// line's charge over its effective width W + 2Δ₀) and the wide-strip limit Δ₀ = b ln 2/π (the
/// excess width of an edge in Cohn's mapping, which the end of a wide strip shares in the
/// quasi-static limit: A. G. Altschuler, A. A. Oliner, IRE Trans. MTT 8(3):328–339, 1960).
/// Returns (low, middle, high).
pub fn open_end(w: f64, b: f64) -> (f64, f64, f64) {
    let hi = b * 2f64.ln() / PI;
    let lo = hi * w / (w + 2.0 * hi);
    (lo, 0.5 * (lo + hi), hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elliptic_integrals_and_limits() {
        // K(0) = π/2; K(1/√2) = Γ(1/4)²/(4√π) = 1.8540746773013719.
        assert!((elliptic_k(0.0) - PI / 2.0).abs() < 1e-15);
        assert!((elliptic_k(0.5f64.sqrt()) - 1.854_074_677_301_371_9).abs() < 1e-14);
        // A wide strip is two parallel-plate lines in parallel: Z → η₀ b / (4 W √εr).
        let (w, b) = (0.05, 0.002);
        assert!((z0(w, b, 1.0) / (MU0 * C0 * b / (4.0 * w)) - 1.0).abs() < 0.05);
        // The 50 Ω stripline on PTFE-like εr = 2.2 is about W = 0.8 b.
        assert!((z0(0.8 * 0.003, 0.003, 2.2) - 50.0).abs() < 3.0, "{}", z0(0.0024, 0.003, 2.2));
    }

    #[test]
    fn the_open_ends_bracket() {
        // The oracle's stripline-ends: 4 mm strip, planes 2 mm apart: 0.3615–0.4413, 0.4014 mm.
        let (lo, mid, hi) = open_end(0.004, 0.002);
        assert!((lo - 0.3615e-3).abs() < 1e-7 && (mid - 0.4014e-3).abs() < 1e-7 && (hi - 0.4413e-3).abs() < 1e-7, "{lo} {mid} {hi}");
    }
}

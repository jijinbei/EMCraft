//! Right-angle corners of rectangular waveguide (the square-cornered 90° bend, H-plane or
//! E-plane) by field superposition: the corner is a w × w square joining two guides of width w,
//! closed by conducting walls on its two outer sides.
//!
//! **Both corners are two-dimensional.** In the H-plane corner (the bend in the plane of the
//! broad dimension a = w) the field is E = E_z(x, y) ẑ across the height b, with
//! (∇² + k²) E_z = 0 and E_z = 0 on the walls: modes sin(mπx/w), m ≥ 1. In the E-plane corner
//! (the bend in the plane of the narrow dimension b = w) every field goes as sin(πz/a) across the
//! broad one; from the magnetic Hertz potential ψ ẑ, ψ = f(x, y) sin(πz/a), E = −jωμ ∇ × (ψ ẑ)
//! lies in the plane of the bend, (∇² + k_c²) f = 0 with k_c² = k² − (π/a)², and the walls ask
//! ∂f/∂n = 0: modes cos(mπx/w), m ≥ 0 (the LSE modes; R. E. Collin, *Field Theory of Guided
//! Waves*, 2nd ed., IEEE Press 1991, on LSE and LSM modes; N. Marcuvitz, *Waveguide Handbook*, McGraw-Hill
//! 1951, ch. 5, for these corners' equivalent circuits).
//!
//! **Superposition** (A. Sieverding, F. Arndt, *Field theoretic CAD of open or aperture matched
//! T-junction coupled rectangular waveguide structures*, IEEE Trans. MTT 40(2):353–362, 1992;
//! J. Uher, J. Bornemann, U. Rosenberg, *Waveguide Components for Antenna Feed Systems*, Artech
//! House 1993). With the square 0 < x, y < w, guide 1 below it (y < 0), guide 2 beyond it
//! (x > w) and walls on x = 0 and y = w, the field inside is the sum of two expansions, each
//! meeting both walls by itself:
//!
//! f = Σ_m e_m(x) p_m s_m(w − y) + Σ_n e_n(y) q_n s_n(x),
//!
//! with e the guides' modes and s_m(t) = sinh(Γ_m t)/sinh(Γ_m w) (H-plane) or
//! cosh(Γ_m t)/cosh(Γ_m w) (E-plane), Γ_m = √((mπ/w)² − k²). Matching f and its normal derivative
//! on the two open sides to the guides' modes, a_m + b_m and c_n + d_n incident and outgoing,
//! leaves 2N equations in p and q whose couplings are integrals of e_m against sinh or cosh in
//! closed form. The representation is singular where an expansion's own cavity resonates
//! (sinh(Γ_m w) = 0 in the H-plane corner, cosh(Γ_m w) = 0 in the E-plane one): at those
//! frequencies the result is refused. S is read in the dominant mode, both guides alike, as the
//! ratios of transverse E: for the H-plane corner E_z = f; for the E-plane corner E is the
//! derivative of f, E_x ∝ −∂_y f in guide 1 and E_y ∝ ∂_x f in guide 2, so S = −b₀/a₀, −c₀/a₀.
//! Reference planes: the square's open sides.
//!
//! ```
//! use emcraft_theory::bend::{Corner, right_angle};
//! // WR-90 at 10.5 GHz: a lossless, reciprocal corner.
//! let (s11, s21) = right_angle(Corner::HPlane, 22.86e-3, 10.16e-3, 10.5e9, 60)?;
//! assert!((s11.norm_sqr() + s21.norm_sqr() - 1.0).abs() < 1e-9);
//! let (s11, s21) = right_angle(Corner::EPlane, 22.86e-3, 10.16e-3, 10.5e9, 60)?;
//! assert!((s11.norm_sqr() + s21.norm_sqr() - 1.0).abs() < 1e-9);
//! assert!(right_angle(Corner::HPlane, 22.86e-3, 10.16e-3, -1.0, 60).is_err());
//! # Ok::<(), emcraft_theory::TheoryError>(())
//! ```

use std::f64::consts::PI;

use emcraft_model::units::C0;
use num_complex::Complex64 as C;

use crate::TheoryError;
use crate::post::dense_solve;

/// Which right-angle corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    /// The bend in the plane of the broad dimension a.
    HPlane,
    /// The bend in the plane of the narrow dimension b.
    EPlane,
}

/// The most modes per guide.
pub const MAX_MODES: usize = 2000;

/// Γ = √((mπ/w)² − k²) with Re Γ ≥ 0 (Im Γ ≥ 0 for a propagating mode: e^{−Γt} = e^{−jβt}).
fn gamma(m: usize, w: f64, k2: f64) -> C {
    let g = C::new((m as f64 * PI / w).powi(2) - k2, 0.0).sqrt();
    if g.re < 0.0 { -g } else { g }
}

/// The square's modes for a corner or a junction (here and in [`crate::tee`]): the inputs
/// checked (at most `max` modes), then the width w of the square, the modes' indices (from 1 for
/// the H-plane, 0 for the E-plane) and their Γ, the dominant one propagating.
pub(crate) fn square_modes(
    corner: Corner,
    a: f64,
    b: f64,
    frequency: f64,
    modes: usize,
    max: usize,
) -> Result<(f64, Vec<usize>, Vec<C>), TheoryError> {
    if !(a.is_finite() && b.is_finite() && a > 0.0 && b > 0.0) {
        return Err(TheoryError::Invalid("guide dimensions".into()));
    }
    if !(frequency.is_finite() && frequency > 0.0) {
        return Err(TheoryError::Invalid("frequency".into()));
    }
    if modes == 0 || modes > max {
        return Err(TheoryError::Invalid(format!("mode count {modes} (1 to {max})")));
    }
    let k = 2.0 * PI * frequency / C0;
    // The width of the square and the 2D wavenumber, and the first mode's index.
    let (w, k2, first) = match corner {
        Corner::HPlane => (a, k * k, 1),
        Corner::EPlane => (b, k * k - (PI / a).powi(2), 0),
    };
    let index: Vec<usize> = (first..first + modes).collect();
    let g: Vec<C> = index.iter().map(|&m| gamma(m, w, k2)).collect();
    if !(g[0].im > 0.0 && g[0].re.abs() < 1e-12 * g[0].im) {
        return Err(TheoryError::Invalid("frequency: the dominant mode does not propagate".into()));
    }
    Ok((w, index, g))
}

/// The square-cornered right-angle bend of a guide of broad dimension `a` and narrow `b` (m),
/// empty, at `frequency` (Hz), with `modes` modes per guide: (S11, S21) of the dominant mode at
/// the square's open sides.
pub fn right_angle(corner: Corner, a: f64, b: f64, frequency: f64, modes: usize) -> Result<(C, C), TheoryError> {
    let (w, index, g) = square_modes(corner, a, b, frequency, modes, MAX_MODES)?;
    let alpha = |m: usize| m as f64 * PI / w;
    let sign = |m: usize| if m.is_multiple_of(2) { 1.0 } else { -1.0 };
    // e^{−2Γw}, |·| ≤ 1: the hyperbolic functions of Γw without overflow.
    let e2: Vec<C> = g.iter().map(|gm| (-2.0 * gm * w).exp()).collect();
    let n = modes;
    let mut mat = vec![C::new(0.0, 0.0); 4 * n * n];
    let mut rhs = vec![C::new(0.0, 0.0); 2 * n];
    let dim = 2 * n;
    // Each expansion's diagonal, refused where its cavity resonates.
    let diag: Vec<C> = match corner {
        // Γ(1 + coth Γw) = 2Γ/(1 − e^{−2Γw}).
        Corner::HPlane => g.iter().zip(&e2).map(|(gm, e)| 2.0 * gm / (1.0 - e)).collect(),
        // 1 + tanh Γw = 2/(1 + e^{−2Γw}).
        Corner::EPlane => e2.iter().map(|e| 2.0 / (1.0 + e)).collect(),
    };
    if diag.iter().any(|d| !(d.re.is_finite() && d.im.is_finite()) || d.norm() > 1e12) {
        return Err(TheoryError::Invalid(format!("frequency {frequency} Hz: a resonance of the corner's expansion (choose another)")));
    }
    for (i, &m) in index.iter().enumerate() {
        mat[i * dim + i] = diag[i];
        mat[(n + i) * dim + n + i] = diag[i];
        for (j, &l) in index.iter().enumerate() {
            match corner {
                Corner::HPlane => {
                    // Row m (side y = 0) against q_l, row l (side x = w) against p_m.
                    let c = (2.0 / w) * alpha(l) * sign(m) * alpha(m) / (alpha(m).powi(2) + g[j] * g[j]);
                    mat[i * dim + n + j] = c;
                    let c = (2.0 / w) * alpha(m) * sign(m) * alpha(l) / (alpha(l).powi(2) + g[i] * g[i]);
                    mat[(n + j) * dim + i] = c;
                }
                Corner::EPlane => {
                    // K_rc = (ε_r/w)(−1)^r Γ_c tanh(Γ_c w)/(α_r² + Γ_c²), mode r of one
                    // guide against expansion term c (by its position in `index`).
                    let k_of = |r: usize, c: usize| {
                        let eps = if r == 0 { 1.0 } else { 2.0 };
                        let tanh = (1.0 - e2[c]) / (1.0 + e2[c]);
                        (eps / w) * sign(r) * g[c] * tanh / (alpha(r).powi(2) + g[c] * g[c])
                    };
                    // Row m: K_ml q_l; row l: (−1)^{m+l} K_lm p_m.
                    mat[i * dim + n + j] = k_of(m, j);
                    mat[(n + j) * dim + i] = sign(m) * sign(l) * k_of(l, i);
                }
            }
        }
    }
    // A unit dominant wave into guide 1.
    rhs[0] = match corner {
        Corner::HPlane => 2.0 * g[0],
        Corner::EPlane => C::new(2.0, 0.0),
    };
    let x = dense_solve(mat, rhs, dim).ok_or_else(|| TheoryError::Invalid(format!("frequency {frequency} Hz: a singular corner system")))?;
    let (p0, q0) = (x[0], x[n]);
    Ok(match corner {
        Corner::HPlane => (p0 - 1.0, q0),
        Corner::EPlane => {
            let tanh = (1.0 - e2[0]) / (1.0 + e2[0]);
            (-(1.0 - tanh * p0), tanh * q0)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_lossless_and_converge() {
        let (a, b) = (22.86e-3, 10.16e-3);
        for corner in [Corner::HPlane, Corner::EPlane] {
            for f in [8.5e9, 10.5e9, 11.5e9] {
                let coarse = right_angle(corner, a, b, f, 100).unwrap();
                let fine = right_angle(corner, a, b, f, 400).unwrap();
                for (s11, s21) in [coarse, fine] {
                    assert!((s11.norm_sqr() + s21.norm_sqr() - 1.0).abs() < 1e-9, "{corner:?} {f}: {s11} {s21}");
                }
                assert!((coarse.0 - fine.0).norm() < 2e-3 && (coarse.1 - fine.1).norm() < 2e-3, "{corner:?} {f}: {coarse:?} {fine:?}");
            }
        }
        assert!(right_angle(Corner::HPlane, a, b, 1e3, 10).is_err(), "below cut-off");
        assert!(right_angle(Corner::HPlane, a, b, 10e9, 0).is_err());
    }

    #[test]
    fn the_oracle_references_are_reproduced() {
        // tests/oracle/{h,e}plane-corner: 2000 modes, the ports 20 mm out; (|S11|, ∠S11, |S21|, ∠S21).
        let (a, b, l) = (22.86e-3, 10.16e-3, 20e-3);
        let cases = [
            (Corner::HPlane, 8e9, [0.404913697518, -64.4998144854, 0.914354907879, 25.5001855146]),
            (Corner::HPlane, 10e9, [0.432383640382, 81.0527101881, 0.901689740171, 171.0527101881]),
            (Corner::EPlane, 8e9, [0.239350982827, -176.1737173348, 0.970933111506, -86.1737173348]),
            (Corner::EPlane, 10e9, [0.476748239446, 2.7100553108, 0.879039883160, 92.7100553108]),
        ];
        for (corner, f, want) in cases {
            // 400 modes are within 2e-5 of 2000 (the series converge as N^{−4/3}).
            let (s11, s21) = right_angle(corner, a, b, f, 400).unwrap();
            let k = 2.0 * PI * f / C0;
            let moved = C::new(0.0, -2.0 * (k * k - (PI / a).powi(2)).sqrt() * l).exp();
            let (s11, s21) = (s11 * moved, s21 * moved);
            let at = |m: f64, deg: f64| C::from_polar(m, deg.to_radians());
            assert!((s11 - at(want[0], want[1])).norm() < 2e-5 && (s21 - at(want[2], want[3])).norm() < 2e-5, "{corner:?} {f}: {s11} {s21}");
        }
    }
}

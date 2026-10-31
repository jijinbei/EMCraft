//! Symmetric T-junctions of rectangular waveguide (H-plane and E-plane) by field superposition,
//! and with them any junction of guides of one width on a square: the square w × w region whose
//! sides are each open to a guide or closed by a conducting wall.
//!
//! **Two-dimensional, as the corners of [`crate::bend`].** In the H-plane junction (all guides in
//! the plane of the broad dimension, w = a) E = E_z(x, y) ẑ, (∇² + k²) f = 0 with f = 0 on the
//! walls: modes sin(mπτ/w), m ≥ 1. In the E-plane junction (the plane of the narrow dimension,
//! w = b) every field goes as sin(πz/a), from the magnetic Hertz potential ψ ẑ,
//! ψ = f(x, y) sin(πz/a): (∇² + k² − (π/a)²) f = 0 with ∂f/∂n = 0 on the walls, the LSE modes
//! cos(mπτ/w), m ≥ 0.
//!
//! **Superposition** (A. Sieverding, F. Arndt, *Field theoretic CAD of open or aperture matched
//! T-junction coupled rectangular waveguide structures*, IEEE Trans. MTT 40(2):353–362, 1992,
//! where the T-junction is the building block; J. Uher, J. Bornemann, U. Rosenberg, *Waveguide
//! Components for Antenna Feed Systems*, Artech House 1993). The square is 0 < x, y < w; its
//! sides Left (x = 0), Right (x = w), Bottom (y = 0) and Top (y = w). Each open side s carries
//! one expansion Σ_m e_m(τ_s) p^s_m s_m(d_s), τ_s the coordinate along the side (y on Left and
//! Right, x on Bottom and Top), d_s the distance from the opposite side, s_m(d) =
//! sinh(Γ_m d)/sinh(Γ_m w) (H-plane) or cosh(Γ_m d)/cosh(Γ_m w) (E-plane),
//! Γ_m = √((mπ/w)² − k²). Each expansion vanishes (H-plane), or has no normal derivative
//! (E-plane), on every side but its own, so it meets all the walls by itself and the open sides
//! couple only through it. Matching f and its normal derivative on each open side to the guide's
//! modes, a^s incident and b^s outgoing,
//!
//! - H-plane: p^s = a^s + b^s, and Γ_m(1 + coth Γ_m w) p^s_m − Γ_m csch(Γ_m w) p^{s̄}_m +
//!   Σ_{s'} Σ_l C^{ss'}_{ml} p^{s'}_l = 2Γ_m a^s_m, with s̄ the opposite side and s' the
//!   adjacent ones;
//! - E-plane: a^s − b^s = tanh(Γw) p^s, and (1 + tanh Γ_m w) p^s_m + sech(Γ_m w) p^{s̄}_m +
//!   Σ_{s'} Σ_l K^{ss'}_{ml} p^{s'}_l = 2a^s_m;
//!
//! the couplings of adjacent sides are the integrals of e_m against sinh or cosh in closed form,
//! C = (2/w) α_m α_l σ/(α_m² + Γ_l²) and K = (ε_m/w) Γ_l tanh(Γ_l w) σ'/(α_m² + Γ_l²),
//! α = mπ/w, ε₀ = 1, ε_m = 2, the signs σ, σ' = ±1, (−1)^m, (−1)^l by which ends of each other's
//! sides the two meet at. Two open sides adjacent give the corner of [`crate::bend`]; Left and
//! Right alone, a straight guide of length w; three, the T-junction. The representation is
//! singular where an expansion's own cavity resonates (sinh(Γ_m w) = 0 in the H-plane,
//! cosh(Γ_m w) = 0 in the E-plane: for WR-90 at 9.27 GHz and 9.87 GHz): those frequencies are
//! refused.
//!
//! S is read in the dominant mode, as the ratio of transverse E, every port's mode pointing
//! along +x or +y (as EMCraft's ports: E_z > 0 for the H-plane, E along the narrow dimension's +
//! axis for the E-plane). For the H-plane E_z = f, so S_ij = b^i/a^j. For the E-plane E is the
//! derivative of f (E_x = −jωμ ∂_y f, E_y = jωμ ∂_x f), so S_ij = −σ_i σ_j b^i/a^j with σ = −1 on
//! Left and Top, +1 on Right and Bottom. Reference planes: the square's open sides; moving them
//! out along each arm is the caller's (e^{−jβ(l_i + l_j)}).
//!
//! The T-junction ([`h_plane`], [`e_plane`]): the main guide along x through the square, the side
//! arm off its Top, the wall on its Bottom; ports 1 (Left), 2 (Right), 3 (Top). Symmetric about
//! x = w/2: S11 = S22, S13 = S23 (H-plane), S13 = −S23 (E-plane).
//!
//! ```
//! use emcraft_theory::tee;
//! // WR-90 at 10.5 GHz: lossless and reciprocal.
//! let s = tee::h_plane(22.86e-3, 10.5e9, 60)?;
//! let power = |j: usize| (0..3).map(|i| s[i][j].norm_sqr()).sum::<f64>();
//! assert!((power(0) - 1.0).abs() < 1e-9 && (s[0][2] - s[2][0]).norm() < 1e-9);
//! assert!((s[0][2] - s[1][2]).norm() < 1e-9, "H-plane: S13 = S23");
//! let s = tee::e_plane(10.16e-3, 22.86e-3, 10.5e9, 60)?;
//! assert!((s[0][2] + s[1][2]).norm() < 1e-9, "E-plane: S13 = −S23");
//! // Refused: below cut-off, a resonance of the expansions, too many modes.
//! assert!(tee::h_plane(22.86e-3, 5e9, 60).is_err());
//! assert!(tee::h_plane(22.86e-3, 2f64.sqrt() * 299_792_458.0 / (2.0 * 22.86e-3), 60).is_err());
//! assert!(tee::h_plane(22.86e-3, 10.5e9, 100_000).is_err());
//! # Ok::<(), emcraft_theory::TheoryError>(())
//! ```

use std::f64::consts::PI;

use num_complex::Complex64 as C;

use crate::TheoryError;
use crate::bend::{Corner, square_modes};
use crate::post::dense_solve_many;

/// A side of the junction's square 0 < x, y < w.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// x = 0.
    Left,
    /// x = w.
    Right,
    /// y = 0.
    Bottom,
    /// y = w.
    Top,
}

impl Side {
    /// Along x (Bottom, Top) rather than y (Left, Right).
    fn horizontal(self) -> bool {
        matches!(self, Side::Bottom | Side::Top)
    }

    /// At the far end (x = w or y = w) of the coordinate across it.
    fn far(self) -> bool {
        matches!(self, Side::Right | Side::Top)
    }

    /// The sign of transverse E against f's amplitudes in the E-plane: −1 on Left and Top.
    fn sigma(self) -> f64 {
        match self {
            Side::Left | Side::Top => -1.0,
            Side::Right | Side::Bottom => 1.0,
        }
    }
}

/// The most modes per side.
pub const MAX_MODES: usize = 1500;

/// The most unknowns (sides × modes): a dense complex system of 4500² is 324 MB.
pub const MAX_UNKNOWNS: usize = 4500;

/// The junction square of a guide of broad dimension `a` and narrow `b` (m), its sides `open`
/// to guides (the others walls), in the `plane` of the broad (H) or the narrow (E) dimension, at
/// `frequency` (Hz), with `modes` modes per side: S of the dominant mode, ports in the order of
/// `open`, at the square's sides. The H-plane problem does not depend on `b`.
pub fn square(plane: Corner, a: f64, b: f64, frequency: f64, open: &[Side], modes: usize) -> Result<Vec<Vec<C>>, TheoryError> {
    if open.is_empty() || open.iter().enumerate().any(|(i, s)| open[..i].contains(s)) {
        return Err(TheoryError::Invalid("open sides: one to four distinct sides".into()));
    }
    let max = MAX_MODES.min(MAX_UNKNOWNS / open.len());
    let (w, index, g) = square_modes(plane, a, b, frequency, modes, max)?;
    let alpha = |m: usize| m as f64 * PI / w;
    let sign = |m: usize| if m.is_multiple_of(2) { 1.0 } else { -1.0 };
    // e^{−Γw} and e^{−2Γw}, |·| ≤ 1: the hyperbolic functions of Γw without overflow.
    let e1: Vec<C> = g.iter().map(|gm| (-gm * w).exp()).collect();
    let e2: Vec<C> = e1.iter().map(|e| e * e).collect();
    let tanh: Vec<C> = e2.iter().map(|e| (1.0 - e) / (1.0 + e)).collect();
    // Each expansion on its own side, and on the opposite one.
    let (own, opposite): (Vec<C>, Vec<C>) = match plane {
        // Γ(1 + coth Γw) = 2Γ/(1 − e^{−2Γw}); −Γ csch Γw = −2Γe^{−Γw}/(1 − e^{−2Γw}).
        Corner::HPlane => g.iter().zip(e1.iter().zip(&e2)).map(|(gm, (e, ee))| (2.0 * gm / (1.0 - ee), -2.0 * gm * e / (1.0 - ee))).unzip(),
        // 1 + tanh Γw = 2/(1 + e^{−2Γw}); sech Γw = 2e^{−Γw}/(1 + e^{−2Γw}).
        Corner::EPlane => e1.iter().zip(&e2).map(|(e, ee)| (2.0 / (1.0 + ee), 2.0 * e / (1.0 + ee))).unzip(),
    };
    let bad = |d: &C| !(d.re.is_finite() && d.im.is_finite()) || d.norm() > 1e12;
    if own.iter().chain(&opposite).any(bad) {
        return Err(TheoryError::Invalid(format!("frequency {frequency} Hz: a resonance of the junction's expansion (choose another)")));
    }
    // The coupling of side s's row m to the expansion of the adjacent side t, term l.
    let adjacent = |s: Side, t: Side, i: usize, j: usize| -> C {
        let (m, l) = (index[i], index[j]);
        let d = alpha(m).powi(2) + g[j] * g[j];
        match plane {
            Corner::HPlane => {
                // ∂_n of t's mode at s, and s's mode against sinh along s.
                let dn = if s.far() { alpha(l) * sign(l) } else { -alpha(l) };
                let proj = if t.far() { -(2.0 / w) * alpha(m) * sign(m) / d } else { (2.0 / w) * alpha(m) / d };
                dn * proj
            }
            Corner::EPlane => {
                // t's mode at s, and s's mode against cosh along s.
                let at = if s.far() { sign(l) } else { 1.0 };
                let eps = if m == 0 { 1.0 } else { 2.0 };
                let proj = (eps / w) * g[j] * tanh[j] / d;
                at * if t.far() { sign(m) * proj } else { proj }
            }
        }
    };
    let (ports, n) = (open.len(), modes);
    let dim = ports * n;
    let mut mat = vec![C::new(0.0, 0.0); dim * dim];
    for (si, &s) in open.iter().enumerate() {
        for (ti, &t) in open.iter().enumerate() {
            for i in 0..n {
                let row = (si * n + i) * dim + ti * n;
                if si == ti {
                    mat[row + i] = own[i];
                } else if s.horizontal() == t.horizontal() {
                    mat[row + i] = opposite[i];
                } else {
                    for j in 0..n {
                        mat[row + j] = adjacent(s, t, i, j);
                    }
                }
            }
        }
    }
    // A unit dominant wave into each port in turn.
    let mut rhs = vec![C::new(0.0, 0.0); dim * ports];
    for p in 0..ports {
        rhs[p * n * ports + p] = match plane {
            Corner::HPlane => 2.0 * g[0],
            Corner::EPlane => C::new(2.0, 0.0),
        };
    }
    let x = dense_solve_many(mat, rhs, dim, ports)
        .ok_or_else(|| TheoryError::Invalid(format!("frequency {frequency} Hz: a singular junction system")))?;
    let mut s = vec![vec![C::new(0.0, 0.0); ports]; ports];
    for (i, row) in s.iter_mut().enumerate() {
        for (j, sij) in row.iter_mut().enumerate() {
            let p = x[i * n * ports + j];
            let incident = if i == j { 1.0 } else { 0.0 };
            *sij = match plane {
                Corner::HPlane => p - incident,
                Corner::EPlane => -open[i].sigma() * open[j].sigma() * (incident - tanh[0] * p),
            };
        }
    }
    Ok(s)
}

/// The three ports of a tee from [`square`]'s rows.
fn three(s: Vec<Vec<C>>) -> Result<[[C; 3]; 3], TheoryError> {
    let rows: Vec<[C; 3]> = s.into_iter().filter_map(|r| r.try_into().ok()).collect();
    rows.try_into().map_err(|_| TheoryError::Invalid("tee: not a three-port".into()))
}

/// The symmetric H-plane T-junction of guides of broad dimension `a` (m) at `frequency` (Hz),
/// `modes` modes per side: the 3 × 3 S of TE10 at the junction square's sides, ports 1 and 2 the
/// main guide along x, 3 the side arm along y.
pub fn h_plane(a: f64, frequency: f64, modes: usize) -> Result<[[C; 3]; 3], TheoryError> {
    three(square(Corner::HPlane, a, a, frequency, &[Side::Left, Side::Right, Side::Top], modes)?)
}

/// The symmetric E-plane T-junction of guides of narrow dimension `b` and broad `a` (m) at
/// `frequency` (Hz), `modes` LSE modes per side: the 3 × 3 S of TE10 at the junction square's
/// sides, ports 1 and 2 the main guide along x, 3 the side arm along y.
pub fn e_plane(b: f64, a: f64, frequency: f64, modes: usize) -> Result<[[C; 3]; 3], TheoryError> {
    three(square(Corner::EPlane, a, b, frequency, &[Side::Left, Side::Right, Side::Top], modes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bend::right_angle;
    use emcraft_model::units::C0;

    const A: f64 = 22.86e-3;
    const B: f64 = 10.16e-3;

    fn tee(plane: Corner, f: f64, modes: usize) -> [[C; 3]; 3] {
        match plane {
            Corner::HPlane => h_plane(A, f, modes).unwrap(),
            Corner::EPlane => e_plane(B, A, f, modes).unwrap(),
        }
    }

    #[test]
    fn tees_are_lossless_reciprocal_and_symmetric() {
        for plane in [Corner::HPlane, Corner::EPlane] {
            for f in [8.5e9, 10.5e9, 12e9] {
                let s = tee(plane, f, 120);
                for j in 0..3 {
                    let power: f64 = (0..3).map(|i| s[i][j].norm_sqr()).sum();
                    assert!((power - 1.0).abs() < 1e-9, "{plane:?} {f}: column {j} {power}");
                    for i in 0..3 {
                        assert!((s[i][j] - s[j][i]).norm() < 1e-9, "{plane:?} {f}: S{i}{j}");
                        // Unitary: the columns orthogonal.
                        let dot: C = (0..3).map(|r| s[r][i].conj() * s[r][j]).sum();
                        assert!(i == j || dot.norm() < 1e-9, "{plane:?} {f}: columns {i} {j}");
                    }
                }
                let parity = if plane == Corner::HPlane { 1.0 } else { -1.0 };
                assert!((s[0][0] - s[1][1]).norm() < 1e-9 && (s[0][2] - parity * s[1][2]).norm() < 1e-9, "{plane:?} {f}");
            }
        }
    }

    #[test]
    fn tees_converge_in_the_number_of_modes() {
        // The series converge as N^{−4/3} (the re-entrant corners' r^{2/3}, H-plane; r^{−1/3} in
        // E, E-plane): N to 2N moves S by well under 1e-3 from 100 modes.
        for plane in [Corner::HPlane, Corner::EPlane] {
            for f in [8.5e9, 11e9] {
                let (coarse, fine) = (tee(plane, f, 100), tee(plane, f, 200));
                let finer = tee(plane, f, 400);
                let diff = |p: &[[C; 3]; 3], q: &[[C; 3]; 3]| (0..9).map(|e| (p[e / 3][e % 3] - q[e / 3][e % 3]).norm()).fold(0.0, f64::max);
                let (d1, d2) = (diff(&coarse, &fine), diff(&fine, &finer));
                assert!(d1 < 1e-3 && d2 < d1 / 1.8, "{plane:?} {f}: {d1} {d2}");
            }
        }
    }

    #[test]
    fn the_oracle_references_are_reproduced() {
        // tests/oracle/{h,e}plane-tee at 10.5 GHz: 1500 modes, the ports 20 mm out;
        // (|S11|, ∠S11, |S31|, ∠S31).
        let (f, l) = (10.5e9, 20e-3);
        let cases = [
            (Corner::HPlane, [0.211665532741, -122.1883958153, 0.549146704269, 132.1191909674]),
            (Corner::EPlane, [0.251452670843, -123.4777304041, 0.613432748568, 85.4375185584]),
        ];
        for (plane, want) in cases {
            // 400 modes are within 1e-4 of 1500 (N^{−4/3}).
            let s = tee(plane, f, 400);
            let k = 2.0 * PI * f / C0;
            let moved = C::new(0.0, -2.0 * (k * k - (PI / A).powi(2)).sqrt() * l).exp();
            let at = |m: f64, deg: f64| C::from_polar(m, deg.to_radians());
            let (s11, s31) = (s[0][0] * moved, s[2][0] * moved);
            assert!((s11 - at(want[0], want[1])).norm() < 1e-4 && (s31 - at(want[2], want[3])).norm() < 1e-4, "{plane:?}: {s11} {s31}");
        }
    }

    #[test]
    fn two_adjacent_sides_are_the_corner_of_bend() {
        // bend.rs: guide 1 below the square, guide 2 beyond its Right side.
        for plane in [Corner::HPlane, Corner::EPlane] {
            for f in [8.5e9, 10.5e9] {
                let s = square(plane, A, B, f, &[Side::Bottom, Side::Right], 80).unwrap();
                let (s11, s21) = right_angle(plane, A, B, f, 80).unwrap();
                assert!((s[0][0] - s11).norm() < 1e-10 && (s[1][0] - s21).norm() < 1e-10, "{plane:?} {f}: {s:?} {s11} {s21}");
            }
        }
    }

    #[test]
    fn two_opposite_sides_are_a_straight_guide() {
        // Left and Right open, walls above and below: a guide of length w, matched.
        for plane in [Corner::HPlane, Corner::EPlane] {
            let (w, kc) = if plane == Corner::HPlane { (A, PI / A) } else { (B, PI / A) };
            for f in [8.5e9, 12e9] {
                let k = 2.0 * PI * f / C0;
                let through = C::new(0.0, -(k * k - kc * kc).sqrt() * w).exp();
                let s = square(plane, A, B, f, &[Side::Left, Side::Right], 40).unwrap();
                assert!(s[0][0].norm() < 1e-12 && (s[1][0] - through).norm() < 1e-12, "{plane:?} {f}: {s:?}");
                // Across the other way (Bottom and Top), the same.
                let s = square(plane, A, B, f, &[Side::Bottom, Side::Top], 40).unwrap();
                assert!(s[0][0].norm() < 1e-12 && (s[1][0] - through).norm() < 1e-12, "{plane:?} {f}: {s:?}");
            }
        }
    }

    #[test]
    fn a_cross_is_lossless_and_symmetric() {
        let all = [Side::Left, Side::Right, Side::Bottom, Side::Top];
        for plane in [Corner::HPlane, Corner::EPlane] {
            let s = square(plane, A, B, 10.5e9, &all, 60).unwrap();
            for j in 0..4 {
                let power: f64 = (0..4).map(|i| s[i][j].norm_sqr()).sum();
                assert!((power - 1.0).abs() < 1e-9, "{plane:?}: {power}");
                for i in 0..4 {
                    assert!((s[i][j] - s[j][i]).norm() < 1e-9);
                }
                assert!((s[j][j] - s[0][0]).norm() < 1e-9, "{plane:?}: every side alike");
            }
        }
    }

    #[test]
    fn hostile_inputs_are_refused() {
        let f = 10.5e9;
        assert!(square(Corner::HPlane, f64::NAN, B, f, &[Side::Left], 10).is_err());
        assert!(square(Corner::HPlane, A, -1.0, f, &[Side::Left], 10).is_err());
        assert!(square(Corner::HPlane, A, B, f64::INFINITY, &[Side::Left], 10).is_err());
        assert!(square(Corner::HPlane, A, B, f, &[], 10).is_err());
        assert!(square(Corner::HPlane, A, B, f, &[Side::Left, Side::Left], 10).is_err());
        assert!(square(Corner::HPlane, A, B, f, &[Side::Left], 0).is_err());
        assert!(square(Corner::HPlane, A, B, f, &[Side::Left, Side::Right, Side::Top, Side::Bottom], MAX_MODES).is_err());
        assert!(h_plane(A, 1e3, 10).is_err(), "below cut-off");
        // The expansions' resonances: βw = π (H-plane), βw = π/2 (E-plane).
        let k = |beta: f64, kc: f64| (beta * beta + kc * kc).sqrt() * C0 / (2.0 * PI);
        assert!(h_plane(A, k(PI / A, PI / A), 40).is_err());
        assert!(e_plane(B, A, k(PI / (2.0 * B), PI / A), 40).is_err());
        // One port, all walls but one: a short, |S11| = 1.
        let s = square(Corner::EPlane, A, B, f, &[Side::Top], 20).unwrap();
        assert!((s[0][0].norm() - 1.0).abs() < 1e-12);
    }
}

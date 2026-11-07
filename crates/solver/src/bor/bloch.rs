//! A periodic body of revolution's Bloch wave: the propagation constant γ = α + jβ of the
//! least-attenuated wave going +z through an infinite repetition of one cell, from the cell's
//! generalized scattering matrix between its two end planes (the same circular guide on both,
//! [`super::generalized`]).
//!
//! With a and b the waves into and out of the cell (N modes a port), a Bloch wave repeats
//! itself one period on, scaled by λ = e^{−γ p}: the waves going +z at the far end are λ times
//! those at the near end, (b₂, a₂) = λ (a₁, b₁). With b₁ = S₁₁a₁ + S₁₂a₂ and
//! b₂ = S₂₁a₁ + S₂₂a₂ this is the eigenproblem of the transfer matrix
//! T = [[S₂₁ − S₂₂S₁₂⁻¹S₁₁, S₂₂S₁₂⁻¹], [−S₁₂⁻¹S₁₁, S₁₂⁻¹]] acting on (a₁, b₁)
//! (R. E. Collin, *Foundations for Microwave Engineering*, 2nd ed., McGraw-Hill 1992, §9.2,
//! periodic structures by the transfer matrix; T. Itoh (ed.), *Numerical Techniques for
//! Microwave and Millimeter-Wave Passive Structures*, Wiley 1989, ch. 9, with the evanescent
//! modes kept). Its 2N eigenvalues come in pairs λ, 1/λ (the waves either way); the wave taken
//! is the one with |λ| ≤ 1 nearest the unit circle — in a pass band one of the two of modulus 1,
//! the one whose first mode carries power +z (|a₁|² > |b₁|²); in a stop band the one decaying.
//! β is −arg λ / p up to a multiple of 2π/p, which the caller chooses (the branch nearest the
//! smooth guide's).

use num_complex::Complex64 as C;

use super::Profile;
use super::ports::{BorPort, generalized};
use crate::driven::{Result, invalid};

/// The Bloch wave of the cell between its ports `near` and `far` (one period apart) at
/// `frequency` in order `m`: λ = e^{−γ p}, the one taken as the module says.
pub fn bloch_lambda(p: &Profile, m: u32, near: BorPort, far: BorPort, frequency: f64) -> Result<C> {
    if near.modes != far.modes {
        return Err(invalid("Bloch wave", "the cell's two ends need as many modes"));
    }
    let n = near.modes;
    let (_, s) = generalized(p, m, &[near, far], frequency)?;
    let w = 2 * n;
    let block = |r0: usize, c0: usize| -> Vec<C> { (0..n * n).map(|ij| s[(r0 + ij / n) * w + c0 + ij % n]).collect() };
    let (s11, s12, s21, s22) = (block(0, 0), block(0, n), block(n, 0), block(n, n));
    let mul = |a: &[C], b: &[C]| -> Vec<C> { (0..n * n).map(|ij| (0..n).map(|k| a[(ij / n) * n + k] * b[k * n + ij % n]).sum()).collect() };
    // S₁₂⁻¹ by columns.
    let lu = crate::dense::Lu::new(s12.clone(), n).ok_or_else(|| invalid("Bloch wave", "the cell transmits nothing: no wave crosses it"))?;
    let mut inv = vec![C::new(0.0, 0.0); n * n];
    for j in 0..n {
        let e: Vec<C> = (0..n).map(|i| if i == j { C::new(1.0, 0.0) } else { C::new(0.0, 0.0) }).collect();
        let col = lu.solve(&e);
        for i in 0..n {
            inv[i * n + j] = col[i];
        }
    }
    let inv_s11 = mul(&inv, &s11);
    let s22_inv = mul(&s22, &inv);
    let s22_inv_s11 = mul(&s22_inv, &s11);
    let mut t = vec![C::new(0.0, 0.0); w * w];
    for i in 0..n {
        for j in 0..n {
            t[i * w + j] = s21[i * n + j] - s22_inv_s11[i * n + j];
            t[i * w + n + j] = s22_inv[i * n + j];
            t[(n + i) * w + j] = -inv_s11[i * n + j];
            t[(n + i) * w + n + j] = inv[i * n + j];
        }
    }
    let pairs = crate::zdense::eigen(&t, w).ok_or_else(|| invalid("Bloch wave", "the transfer matrix's eigenproblem did not converge"))?;
    // Forward or decaying: |λ| ≤ 1, the first mode carrying power +z when |λ| is 1.
    let unit = |l: &C| (l.norm() - 1.0).abs() < 1e-6;
    let forward = |v: &[C]| v[0].norm_sqr() > v[n].norm_sqr();
    pairs
        .iter()
        .filter(|(l, v)| l.norm() <= 1.0 + 1e-6 && (!unit(l) || forward(v)))
        .max_by(|a, b| a.0.norm().total_cmp(&b.0.norm()))
        .map(|(l, _)| *l)
        .ok_or_else(|| invalid("Bloch wave", "no wave goes through the cell"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_model::units::C0;
    use std::f64::consts::PI;

    /// The first zero of J₁′.
    const X11P: f64 = 1.841_183_781_340_659;

    /// A circular guide's cell, radius `r` and `p` long, its wall waving by `h` sin(2πz/p).
    fn cell(r: f64, p: f64, h: f64, size: f64) -> Profile {
        let steps = 64;
        let mut ring = vec![[0.0, 0.0]];
        ring.extend((0..=steps).map(|k| {
            let z = p * k as f64 / steps as f64;
            [r + h * (2.0 * PI * z / p).sin(), z]
        }));
        ring.push([0.0, p]);
        // The outline counter-clockwise in (r, z): along the axis, up the wall, back.
        let mut outline = vec![[0.0, 0.0]];
        outline.extend(ring[1..ring.len() - 1].iter().copied());
        outline.push([0.0, p]);
        let (points, triangles) = emcraft_mesh::planar::polygon(&outline, size).unwrap_or_default();
        let material = vec![(1.0, 1.0); triangles.len()];
        Profile { points, triangles, material }
    }

    /// A straight guide's cell carries TE11 as it is: λ = e^{−jβp}, β = √(k² − (x′₁₁/r)²).
    #[test]
    fn a_straight_cell_carries_the_guides_wave() {
        let (r, p, f) = (0.012, 0.016, 10e9);
        let prof = cell(r, p, 0.0, 0.6e-3);
        let ports = |z: f64| BorPort { z, modes: 3 };
        let lambda = bloch_lambda(&prof, 1, ports(0.0), ports(p), f).unwrap_or_else(|e| panic!("{e}"));
        let k = 2.0 * PI * f / C0;
        let beta = (k * k - (X11P / r).powi(2)).sqrt();
        assert!((lambda - C::from_polar(1.0, -beta * p)).norm() < 5e-3, "{lambda} against {}", C::from_polar(1.0, -beta * p));
    }

    /// A waving wall opens a stop band where β p = π (the Bragg condition for TE11 reflected
    /// into itself): there the wave decays, |λ| < 1, and below and above it passes.
    #[test]
    fn a_waving_wall_stops_the_wave_at_the_bragg_frequency() {
        let (r, p, h) = (0.012, 0.016, 0.0015);
        let prof = cell(r, p, h, 0.6e-3);
        let ports = |z: f64| BorPort { z, modes: 3 };
        let fb = C0 / (2.0 * PI) * ((X11P / r).powi(2) + (PI / p).powi(2)).sqrt();
        let at = |f: f64| bloch_lambda(&prof, 1, ports(0.0), ports(p), f).map_or(f64::NAN, C::norm);
        assert!(at(fb) < 0.99, "{} at {fb:e} Hz", at(fb));
        assert!((at(0.85 * fb) - 1.0).abs() < 1e-4, "{}", at(0.85 * fb));
    }
}

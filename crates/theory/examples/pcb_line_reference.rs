//! The reference of the oracle's `pcb-route-line` case, evaluated from the published formulas on
//! their own (nothing of `emcraft_theory::pcb` is called): a zero-thickness microstrip `W` wide
//! over `h` of εr (E. Hammerstad, Ø. Jensen, "Accurate models for microstrip computer-aided
//! design", IEEE MTT-S Int. Microwave Symp. Dig., 1980, pp. 407–409), a lossless line `L` long
//! between `Z_ref` terminations, its ABCD matrix (D. M. Pozar, *Microwave Engineering*, 4th ed.,
//! Wiley 2012, §4.4).
//! `pcb_line_reference W h er L Z_ref f` (mm, mm, —, mm, Ω, GHz) prints Z0, the delay and |S11|,
//! |S21| at f.
use num_complex::Complex64 as C;

/// Speed of light (m/s, exact) and free-space impedance μ₀c (Ω, μ₀ of CODATA 2018), written out
/// here rather than taken from the crates under test.
const C0: f64 = 299_792_458.0;
const ETA0: f64 = 1.256_637_062_12e-6 * C0;

fn main() {
    let numbers: Vec<f64> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let [w, h, er, l, z_ref, f] = numbers[..] else {
        return println!("usage: pcb_line_reference W h er L Z_ref f   (mm, mm, -, mm, ohm, GHz)");
    };
    let u = w / h;
    // Hammerstad & Jensen, eqs. (2)–(4): the effective permittivity of a zero-thickness strip.
    let a = 1.0 + ((u.powi(4) + (u / 52.0).powi(2)) / (u.powi(4) + 0.432)).ln() / 49.0 + (1.0 + (u / 18.1).powi(3)).ln() / 18.7;
    let b = 0.564 * ((er - 0.9) / (er + 3.0)).powf(0.053);
    let eps_eff = (er + 1.0) / 2.0 + (er - 1.0) / 2.0 * (1.0 + 10.0 / u).powf(-a * b);
    // Eq. (1): the strip's impedance in air, then in the dielectric.
    let fu = 6.0 + (2.0 * std::f64::consts::PI - 6.0) * (-(30.666 / u).powf(0.7528)).exp();
    let z0 = ETA0 / (2.0 * std::f64::consts::PI) * (fu / u + (1.0 + (2.0 / u).powi(2)).sqrt()).ln() / eps_eff.sqrt();
    let delay = l * 1e-3 * eps_eff.sqrt() / C0;
    // The line's ABCD matrix, then S between Z_ref terminations.
    let theta = 2.0 * std::f64::consts::PI * f * 1e9 * delay;
    let (cos, sin) = (C::new(theta.cos(), 0.0), C::new(0.0, theta.sin()));
    let (aa, bb, cc, dd) = (cos, sin * z0, sin / z0, cos);
    let den = aa + bb / z_ref + cc * z_ref + dd;
    let s11 = (aa + bb / z_ref - cc * z_ref - dd) / den;
    let s21 = 2.0 / den;
    println!("Z0 {z0:.17} ohm, eps_eff {eps_eff:.17}, delay {delay:.17e} s");
    println!("{f} GHz: |S11| {:.17}, |S21| {:.17}", s11.norm(), s21.norm());
}

//! The reference of the oracle's `pcb-via-short` case, evaluated from the published formulas on
//! their own (nothing of the crates under test is called): a zero-thickness microstrip `W` wide
//! over `h` of εr (E. Hammerstad, Ø. Jensen, "Accurate models for microstrip computer-aided
//! design", IEEE MTT-S Int. Microwave Symp. Dig., 1980, pp. 407–409), `L` long from its port to a
//! via of diameter `d` down to the ground, the via an inductance (M. E. Goldfarb, R. A. Pucel,
//! "Modeling via hole grounds in microstrip", IEEE Microwave Guided Wave Lett. 1(6):135–137,
//! 1991): L_via = μ₀/2π [h ln((h + √(r² + h²))/r) + 3/2 (r − √(r² + h²))]. The line referred to
//! its own impedance: S11 = Γ e^{−2jβL}, Γ = (jωL_via − Z0)/(jωL_via + Z0).
//! `pcb_via_reference W h er L d f` (mm, mm, —, mm, mm, GHz) prints Z0, L_via and S11 at f.
use num_complex::Complex64 as C;

/// Speed of light (m/s, exact) and μ₀ (H/m, CODATA 2018), written out here rather than taken
/// from the crates under test.
const C0: f64 = 299_792_458.0;
const MU0: f64 = 1.256_637_062_12e-6;

fn main() {
    let numbers: Vec<f64> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let [w, h, er, l, d, f] = numbers[..] else {
        return println!("usage: pcb_via_reference W h er L d f   (mm, mm, -, mm, mm, GHz)");
    };
    let u = w / h;
    let a = 1.0 + ((u.powi(4) + (u / 52.0).powi(2)) / (u.powi(4) + 0.432)).ln() / 49.0 + (1.0 + (u / 18.1).powi(3)).ln() / 18.7;
    let b = 0.564 * ((er - 0.9) / (er + 3.0)).powf(0.053);
    let eps_eff = (er + 1.0) / 2.0 + (er - 1.0) / 2.0 * (1.0 + 10.0 / u).powf(-a * b);
    let fu = 6.0 + (2.0 * std::f64::consts::PI - 6.0) * (-(30.666 / u).powf(0.7528)).exp();
    let eta0 = MU0 * C0;
    let z0 = eta0 / (2.0 * std::f64::consts::PI) * (fu / u + (1.0 + (2.0 / u).powi(2)).sqrt()).ln() / eps_eff.sqrt();
    let (r, hm) = (d * 0.5e-3, h * 1e-3);
    let s = (r * r + hm * hm).sqrt();
    let l_via = MU0 / (2.0 * std::f64::consts::PI) * (hm * ((hm + s) / r).ln() + 1.5 * (r - s));
    let omega = 2.0 * std::f64::consts::PI * f * 1e9;
    let zl = C::new(0.0, omega * l_via);
    let gamma = (zl - z0) / (zl + z0);
    let beta = omega / C0 * eps_eff.sqrt();
    let s11 = gamma * C::from_polar(1.0, -2.0 * beta * l * 1e-3);
    println!("Z0 {z0:.17} ohm, eps_eff {eps_eff:.17}, L_via {l_via:.17e} H");
    println!("{f} GHz: |S11| {:.17}, arg S11 {:.17} deg", s11.norm(), s11.arg().to_degrees());
}

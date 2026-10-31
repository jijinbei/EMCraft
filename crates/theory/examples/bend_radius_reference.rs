//! The oracle references of `hplane-bend-radius` and `eplane-bend-radius`: 90° bends of WR-90,
//! inner radius 28.6 mm (H-plane) and 35 mm (E-plane), straight arms of 20 mm, reference planes
//! at the arms' ends. For each frequency: ν₀ (Chebyshev collocation) and its order, the leading
//! phase −ν₀π/2 − 2βl, the bend by mode matching (40 and 80 modes), a junction's reflection.
//! `cargo run -p emcraft-theory --example bend_radius_reference`
use std::f64::consts::{FRAC_PI_2, PI};

use emcraft_model::units::C0;
use emcraft_theory::curved::{CurvedGuide, Plane, bend, dominant, junction};
use num_complex::Complex64 as C;

fn main() {
    let (a, b, l) = (22.86e-3, 10.16e-3, 20e-3);
    for (plane, r) in [(Plane::H, 28.6e-3), (Plane::E, 35e-3)] {
        let g = CurvedGuide { plane, a, b, radius: r };
        println!("{plane:?}-plane bend, r = {} mm", r * 1e3);
        for f in [8.2e9, 9.25e9, 10.3e9, 11.35e9, 12.4e9] {
            let k = 2.0 * PI * f / C0;
            let beta = (k * k - (PI / a).powi(2)).sqrt();
            let Ok(nu) = dominant(&g, f) else {
                println!("  {f:e}: no ν₀");
                continue;
            };
            // Arms, and the E-plane's ports keeping +x in and +y out (the radial direction turns).
            let turn = if plane == Plane::E { -1.0 } else { 1.0 };
            let arms = C::from_polar(turn, -2.0 * beta * l);
            let lead = C::from_polar(1.0, -nu.nu * FRAC_PI_2) * arms;
            let (Ok((s11, s21)), Ok((s11f, s21f)), Ok(j)) = (bend(&g, FRAC_PI_2, f, 40), bend(&g, FRAC_PI_2, f, 80), junction(&g, f, 80)) else {
                println!("  {f:e}: no mode matching");
                continue;
            };
            let s21f = s21f * arms;
            println!(
                "  {:.2} GHz: nu0 {:.12} (order {}, change {:.1e}), lead {:.6} deg; mode matching S21 {:.6} deg |S21| {:.8} |S11| {:.6} (40-80 modes {:.1e}); |junction| {:.6}; lead - MM {:.4} deg",
                f * 1e-9,
                nu.nu,
                nu.order,
                nu.change,
                lead.arg().to_degrees(),
                s21f.arg().to_degrees(),
                s21f.norm(),
                s11f.norm(),
                (s11 - s11f).norm() + (s21 * arms - s21f).norm(),
                j.norm(),
                (lead / s21f).arg().to_degrees(),
            );
        }
    }
}

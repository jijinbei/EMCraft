//! The references of the oracle's T-junction cases (`hplane-tee`, `eplane-tee`): S of the
//! symmetric H-plane or E-plane tee of WR-90 by field superposition (`emcraft_theory::tee`),
//! moved out to ports `l` mm along each arm by e^{−jβ(l_i + l_j)}.
//! `tee_reference h|e MODES L f…` (frequencies in GHz) prints |S|, ∠S (degrees) of S11, S21,
//! S31, S32 and S33 at each frequency.
use std::f64::consts::PI;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let numbers: Vec<f64> = args.iter().skip(1).filter_map(|a| a.parse().ok()).collect();
    let (a, b) = (22.86e-3, 10.16e-3);
    let (plane, modes, l, freqs) = match (args.first().map(String::as_str), numbers.as_slice()) {
        (Some(p @ ("h" | "e")), [modes, l, freqs @ ..]) if *modes >= 1.0 => (p, *modes as usize, l * 1e-3, freqs),
        _ => return println!("usage: tee_reference h|e MODES L f…"),
    };
    for f in freqs {
        let s = match plane {
            "h" => emcraft_theory::tee::h_plane(a, f * 1e9, modes),
            _ => emcraft_theory::tee::e_plane(b, a, f * 1e9, modes),
        };
        let s = match s {
            Ok(s) => s,
            Err(e) => {
                println!("{f} GHz: {e}");
                continue;
            }
        };
        let k = 2.0 * PI * f * 1e9 / emcraft_model::units::C0;
        let moved = num_complex::Complex64::new(0.0, -2.0 * (k * k - (PI / a).powi(2)).sqrt() * l).exp();
        print!("{f} GHz");
        for (name, i, j) in [("s11", 0, 0), ("s21", 1, 0), ("s31", 2, 0), ("s32", 2, 1), ("s33", 2, 2)] {
            let v = s[i][j] * moved;
            print!("  {name} {:.12} {:.10}", v.norm(), v.arg().to_degrees());
        }
        println!();
    }
}

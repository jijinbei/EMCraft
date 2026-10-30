//! The references of the design templates' oracle cases: |Γ| of the WR-90 to WR-75 linear
//! taper by the theory of small reflections (`taper L f1 f2 ...`: length in mm, frequencies in
//! GHz), and the pyramidal horn's directivity by Balanis' formula (`horn A B h f ...`: mouth
//! and flare length in mm, frequencies in GHz; fed by WR-90).
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let numbers: Vec<f64> = args.iter().skip(1).filter_map(|a| a.parse().ok()).collect();
    match (args.first().map(String::as_str), numbers.as_slice()) {
        (Some("taper"), [l, freqs @ ..]) => {
            let t = emcraft_theory::taper::LinearTaper { a1: 22.86e-3, b1: 10.16e-3, a2: 19.05e-3, b2: 9.525e-3, length: l * 1e-3 };
            for f in freqs {
                match t.reflection(f * 1e9) {
                    Ok(g) => println!("{f} GHz |Γ| = {}", g.norm()),
                    Err(e) => println!("{f} GHz: {e}"),
                }
            }
        }
        (Some("horn"), [a1, b1, h, freqs @ ..]) => {
            let horn = emcraft_theory::horn::PyramidalHorn { a: 22.86e-3, b: 10.16e-3, a1: a1 * 1e-3, b1: b1 * 1e-3, length: h * 1e-3 };
            for f in freqs {
                match horn.directivity(f * 1e9) {
                    Ok(d) => println!("{f} GHz D = {d} ({} dBi)", 10.0 * d.log10()),
                    Err(e) => println!("{f} GHz: {e}"),
                }
            }
        }
        _ => println!("usage: design_references taper L f… | horn A B h f…"),
    }
}

//! S11 of the centred full-height post across WR-90 (30 mm of guide) at 10 GHz for
//! the diameters given (mm), from the closed-form reference.
fn main() {
    for d in std::env::args().skip(1).filter_map(|a| a.parse::<f64>().ok()) {
        let post = emcraft_theory::post::InductivePost { width: 22.86e-3, length: 30e-3, x: 11.43e-3, z: 15e-3, radius: d * 0.5e-3 };
        match post.solve(10e9, 128) {
            Ok(s) => println!("{d} {} {}", s.s11.norm(), s.s11.arg().to_degrees()),
            Err(e) => println!("{d} error {e}"),
        }
    }
}

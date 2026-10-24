//! Colour maps for scalar fields: a diverging map for signed values (a field component at one
//! phase) and a sequential one for magnitudes. Both are our own control points, interpolated
//! linearly in sRGB.

use crate::Rgb;

const DIVERGING: [Rgb; 5] = [Rgb(40, 80, 170), Rgb(120, 170, 220), Rgb(245, 245, 245), Rgb(230, 140, 100), Rgb(170, 30, 40)];
const SEQUENTIAL: [Rgb; 5] = [Rgb(20, 15, 60), Rgb(90, 30, 120), Rgb(190, 60, 90), Rgb(245, 140, 50), Rgb(250, 235, 120)];

fn sample(stops: &[Rgb; 5], t: f64) -> Rgb {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.5 };
    let x = t * (stops.len() - 1) as f64;
    let i = (x.floor() as usize).min(stops.len() - 2);
    stops[i].mix(stops[i + 1], x - i as f64)
}

/// A signed value in [−1, 1] (clamped): blue through white to red.
pub fn diverging(v: f64) -> Rgb {
    sample(&DIVERGING, 0.5 * (v + 1.0))
}

/// A magnitude in [0, 1] (clamped): dark to bright.
pub fn sequential(v: f64) -> Rgb {
    sample(&SEQUENTIAL, v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ends_middle_and_hostile_values() {
        assert_eq!(diverging(-1.0), DIVERGING[0]);
        assert_eq!(diverging(0.0), Rgb(245, 245, 245));
        assert_eq!(diverging(7.0), DIVERGING[4]);
        assert_eq!(sequential(0.0), SEQUENTIAL[0]);
        assert_eq!(sequential(1.0), SEQUENTIAL[4]);
        assert_eq!(diverging(f64::NAN), Rgb(245, 245, 245));
    }
}

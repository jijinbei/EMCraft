//! The time-domain reflectometry chart.

use emcraft_render::chart::{Chart, Series, Style};
use emcraft_render::{Axis, Pen};

use crate::tdr::{Mode, Tdr};

/// A TDR's chart: ρ(t) (low pass, a step per change of impedance) or |h(t)| (band pass, a peak
/// per reflection) against time in ns.
pub fn tdr_chart(t: &Tdr, port: &str, z0: Option<f64>) -> Chart {
    let points: Vec<[f64; 2]> = t.times.iter().zip(&t.values).map(|(time, v)| [time * 1e9, *v]).collect();
    let (lo, hi) = t.values.iter().fold((0.0f64, 0.0f64), |(a, b), v| (a.min(*v), b.max(*v)));
    let pad = 0.1 * (hi - lo).max(1e-3);
    let end = t.times.last().map_or(1.0, |x| x * 1e9);
    // ρ and |h| are ratios of waves: no unit, which the axis says.
    let (title, label, axis) = match t.mode {
        Mode::LowPass => (
            format!("TDR at port {port}, low pass: step response{}", z0.map_or(String::new(), |z| format!(" (Z = {z} Ω · (1 + ρ)/(1 − ρ))"))),
            "ρ (step)",
            "reflection ρ, step response (unitless)",
        ),
        Mode::BandPass => {
            (format!("TDR at port {port}, band pass: where reflections come from"), "|h| (impulse)", "|h|, impulse response (unitless)")
        }
    };
    Chart {
        title,
        x: Axis::new("round-trip time (ns)", 0.0, end),
        y: Axis::new(axis, lo - pad, hi + pad),
        series: vec![Series { label: label.into(), pen: Pen::Series(0), style: Style::Line, points }],
        marks: Vec::new(),
    }
}

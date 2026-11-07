//! The pulse chart: a pulse through the line (`results.pulse`), its input and output against
//! time — their power, or their instantaneous frequency (the chirp the input carries and what
//! the line leaves of it).

use emcraft_render::chart::{Chart, Series, Style};
use emcraft_render::{Axis, Pen};
use serde_json::Value;

/// The chart of `results.pulse`'s reply `r`: power (`frequency` false) or instantaneous
/// frequency in `unit` against time in ns, the output moved back by its delay so the two lie
/// together.
pub fn chart(r: &Value, frequency: bool, unit: &str) -> Chart {
    let points = r["points"].as_array().cloned().unwrap_or_default();
    let shift = r["output_time"].as_f64().unwrap_or(0.0);
    let series_of = |key: &str, moved: f64| -> Vec<[f64; 2]> {
        points.iter().filter_map(|q| Some([q["t"].as_f64()? - moved, q[key].as_f64()?])).filter(|p| p[1].is_finite()).collect()
    };
    let compression = r["compression"].as_f64().unwrap_or(f64::NAN);
    let (out_w, in_w) = (r["output_width"].as_f64().unwrap_or(f64::NAN), r["input_width"].as_f64().unwrap_or(f64::NAN));
    let chirp = r["chirp"].as_str().unwrap_or("");
    let title = format!("Pulse: ×{compression:.2} peak power, {in_w:.2} → {out_w:.2} ns ({chirp} chirp)");
    let (input, output, axis) = if frequency {
        (series_of("input_frequency", 0.0), series_of("output_frequency", shift), format!("instantaneous frequency ({unit})"))
    } else {
        (series_of("input", 0.0), series_of("output", shift), "power (input's peak 1)".to_owned())
    };
    let all = input.iter().chain(&output);
    let (lo, hi) = all.clone().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[1]), b.max(p[1])));
    let (lo, hi) = if frequency { (lo, hi) } else { (0.0, hi) };
    let pad = 0.08 * (hi - lo).max(1e-9);
    // The time the two take, padded a little.
    let (t0, t1) = all.filter(|p| frequency || p[1] > 1e-3 * hi).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[0]), b.max(p[0])));
    let (t0, t1) = if t1 > t0 { (t0 - 0.1 * (t1 - t0), t1 + 0.1 * (t1 - t0)) } else { (-1.0, 1.0) };
    Chart::new(
        title,
        Axis::new("time (ns; the output moved back by its delay)", t0, t1),
        Axis::new(&axis, if frequency { lo - pad } else { 0.0 }, hi + pad),
        vec![
            Series::new("pulse.input", "input", Pen::Series(4), Style::Line, input),
            Series::new("pulse.output", "output", Pen::Series(0), Style::Line, output),
        ],
    )
}

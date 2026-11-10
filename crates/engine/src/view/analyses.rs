//! The charts of the analyses read from a sweep: a sample's material constants (`results.material`),
//! a passive two-port's noise figure (`results.noise`), an array's active reflections against
//! the scan (`results.active`) and the power a structure handles (`results.breakdown`), each
//! from its command's reply. A temperature (`results.thermal`) is a few numbers, not a chart.

use emcraft_render::chart::{Chart, Series, Style};
use emcraft_render::{Axis, Pen};
use serde_json::Value;

/// Values spread less than this share of their size (or of 1) are drawn as one flat line.
const FLAT: f64 = 1e-9;

/// The commands whose replies chart here.
pub const CHARTED: [&str; 4] = ["results.material", "results.noise", "results.active", "results.breakdown"];

/// The chart of command `command`'s reply `r` (frequencies in `unit`); `None` for a reply that
/// is not one of [`CHARTED`] or holds nothing to draw.
pub fn chart(command: &str, r: &Value, unit: &str) -> Option<Chart> {
    let chart = match command {
        "results.material" => material(r, unit),
        "results.noise" => noise(r, unit),
        "results.active" => active(r),
        "results.breakdown" => breakdown(r, unit),
        _ => return None,
    };
    chart.filter(|c| c.series.iter().any(|s| !s.points.is_empty()))
}

/// The points' `x` and `y` read by JSON pointers, the finite ones.
fn curve(points: &[Value], x: &str, y: &str, scale: f64) -> Vec<[f64; 2]> {
    points
        .iter()
        .filter_map(|p| Some([p.pointer(x)?.as_f64()?, scale * p.pointer(y)?.as_f64()?]))
        .filter(|q| q[0].is_finite() && q[1].is_finite())
        .collect()
}

/// An axis over the series' y values, padded by a tenth (one unit either side when flat).
fn y_axis(label: &str, series: &[Series]) -> Axis {
    let (lo, hi) =
        series.iter().flat_map(|s| s.points.iter().map(|p| p[1])).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), y| (a.min(y), b.max(y)));
    if !(lo.is_finite() && hi.is_finite()) {
        return Axis::new(label, 0.0, 1.0);
    }
    // Flat to rounding (a lossless line's 0 dB noise figure): a unit either side.
    let flat = hi - lo <= FLAT * hi.abs().max(lo.abs()).max(1.0);
    let pad = if flat { 1.0 } else { 0.1 * (hi - lo) };
    Axis::new(label, lo - pad, hi + pad)
}

/// An axis over the series' x values.
fn x_axis(label: &str, series: &[Series]) -> Axis {
    let (lo, hi) =
        series.iter().flat_map(|s| s.points.iter().map(|p| p[0])).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
    if lo.is_finite() && hi > lo { Axis::new(label, lo, hi) } else { Axis::new(label, lo - 1.0, lo + 1.0) }
}

/// εr′, εr″, μr′ and μr″ against frequency (ε = ε′ − jε″: the losses drawn positive).
fn material(r: &Value, unit: &str) -> Option<Chart> {
    let points = r["points"].as_array()?;
    let series = vec![
        Series::new("eps_re", "εr′", Pen::Series(0), Style::Joined, curve(points, "/frequency", "/eps_r/re", 1.0)),
        Series::new("eps_im", "εr″", Pen::Series(0), Style::Dashed, curve(points, "/frequency", "/eps_r/im", -1.0)),
        Series::new("mu_re", "μr′", Pen::Series(1), Style::Joined, curve(points, "/frequency", "/mu_r/re", 1.0)),
        Series::new("mu_im", "μr″", Pen::Series(1), Style::Dashed, curve(points, "/frequency", "/mu_r/im", -1.0)),
    ];
    let read = r["read"].as_u64().unwrap_or(0);
    let title = format!("Material from S (Nicolson–Ross–Weir): {read} of {} frequencies read", points.len());
    Some(Chart::new(title, x_axis(&format!("frequency ({unit})"), &series), y_axis("relative permittivity, permeability", &series), series))
}

/// The noise figure from a matched source and the least noise figure against frequency.
fn noise(r: &Value, unit: &str) -> Option<Chart> {
    let points = r["points"].as_array()?;
    let series = vec![
        Series::new("nf", "NF", Pen::Series(0), Style::Joined, curve(points, "/frequency", "/nf_db", 1.0)),
        Series::new("nf_min", "NF_min", Pen::Series(1), Style::Dashed, curve(points, "/frequency", "/nf_min_db", 1.0)),
    ];
    let t = r["temperature"].as_f64().unwrap_or(f64::NAN);
    Some(Chart::new(format!("Noise figure at {t} K"), x_axis(&format!("frequency ({unit})"), &series), y_axis("noise figure (dB)", &series), series))
}

/// Each element's |Γ| (dB) against the scan angle θ at the first frequency.
fn active(r: &Value) -> Option<Chart> {
    let first = r["points"].as_array()?.first()?;
    let scan = first["scan"].as_array()?;
    let entries = r["entries"].as_array().map_or(0, Vec::len);
    let series: Vec<Series> = (0..entries)
        .map(|k| {
            let name = r["entries"][k].as_str().unwrap_or("?");
            let pts = curve(scan, "/theta", &format!("/elements/{k}/db"), 1.0);
            Series::new(&format!("element{}", k + 1), format!("|Γ| {name}"), Pen::Series(k.min(255) as u8), Style::Joined, pts)
        })
        .collect();
    let f = first["frequency"].as_f64().unwrap_or(f64::NAN);
    let phi = scan.first().and_then(|s| s["phi"].as_f64()).unwrap_or(0.0);
    Some(Chart::new(format!("Active reflection at {f}, φ = {phi}°"), x_axis("scan angle θ (°)", &series), y_axis("|Γ| (dB)", &series), series))
}

/// The power at breakdown against frequency, in kW.
fn breakdown(r: &Value, unit: &str) -> Option<Chart> {
    let points = r["points"].as_array()?;
    let series = vec![Series::new("power", "power at breakdown", Pen::Series(0), Style::Joined, curve(points, "/frequency", "/max_power", 1e-3))];
    let eb = r["breakdown"].as_f64().unwrap_or(f64::NAN);
    let title = format!("Power handling: breakdown at {eb} MV/m, driven at {}", r["excitation"].as_str().unwrap_or("?"));
    Some(Chart::new(title, x_axis(&format!("frequency ({unit})"), &series), y_axis("power (kW)", &series), series))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn each_reply_charts_its_numbers_and_an_empty_one_none() {
        let material = json!({"read": 2, "points": [
            {"frequency": 8.0, "eps_r": {"re": 2.2, "im": -0.02}, "mu_r": {"re": 1.0, "im": 0.0}},
            {"frequency": 9.0, "why": "S11 vanishes"},
            {"frequency": 10.0, "eps_r": {"re": 2.21, "im": -0.021}, "mu_r": {"re": 1.0, "im": 0.0}}]});
        let c = chart("results.material", &material, "GHz").unwrap();
        assert_eq!(c.series[0].points, vec![[8.0, 2.2], [10.0, 2.21]]);
        assert_eq!(c.series[1].points[0], [8.0, 0.02], "the loss drawn positive");
        let active = json!({"entries": ["1", "2"], "points": [{"frequency": 10.0, "scan": [
            {"theta": 0.0, "phi": 0.0, "elements": [{"db": -10.0}, {"db": -12.0}]},
            {"theta": 30.0, "phi": 0.0, "elements": [{"db": -8.0}, {"db": -9.0}]}]}]});
        let c = chart("results.active", &active, "GHz").unwrap();
        assert_eq!((c.series.len(), c.series[1].points.clone()), (2, vec![[0.0, -12.0], [30.0, -9.0]]));
        assert!(chart("results.noise", &json!({"points": []}), "GHz").is_none());
        assert!(chart("results.thermal", &json!({}), "GHz").is_none());
    }
}

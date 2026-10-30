//! The far-field pattern of an open problem.

use emcraft_render::Pen;
use emcraft_render::axes::Axis;
use emcraft_render::chart::{Chart, Series, Style};
use serde_json::Value;

use crate::open::{FarRows, FarSolver, far_rows, plane_wave};
use crate::{Result, Session};

/// The far-field pattern of the last radiation solve against θ at azimuth `phi` (degrees),
/// `points` angles from 0 to 180°: directivity for sources, radar cross-section for a plane
/// wave, over the closed form where it applies.
pub fn pattern_chart(s: &Session, phi: f64, points: usize) -> Result<Chart> {
    pattern_chart_of(s, phi, points, None, None)
}

/// [`pattern_chart`] from a chosen solver and frequency (see `results.far_field`).
pub fn pattern_chart_of(s: &Session, phi: f64, points: usize, solver: Option<FarSolver>, frequency: Option<f64>) -> Result<Chart> {
    let n = points.clamp(13, 721);
    let dirs: Vec<(f64, f64)> = (0..n).map(|i| ((180.0 * i as f64 / (n - 1) as f64).to_radians(), phi.to_radians())).collect();
    let FarRows { frequency: f, rows, .. } = far_rows(s, &dirs, solver, frequency)?;
    let scattering = plane_wave(s).is_some();
    let (key, exact_key, label) =
        if scattering { ("rcs_dbsm", "exact_rcs", "RCS (dBsm)") } else { ("directivity_dbi", "exact_directivity", "directivity (dBi)") };
    let l = s.display.length.from_si(1.0);
    let db = |v: &Value| v.as_f64().map(|x| if scattering { 10.0 * (x / (l * l)).log10() } else { 10.0 * x.log10() });
    let ours: Vec<[f64; 2]> = rows.iter().filter_map(|row| Some([row["theta"].as_f64()?, row[key].as_f64()?])).collect();
    let exact: Vec<[f64; 2]> = rows.iter().filter_map(|row| Some([row["theta"].as_f64()?, db(&row[exact_key])?])).collect();
    let top = ours.iter().chain(&exact).map(|q| q[1]).filter(|v| v.is_finite()).fold(f64::NEG_INFINITY, f64::max);
    let top = if top.is_finite() { (top / 5.0).ceil() * 5.0 } else { 0.0 };
    let mut series = Vec::new();
    if !exact.is_empty() {
        let name = if scattering { "Mie series" } else { "Hertzian dipole" };
        series.push(Series { label: name.into(), pen: Pen::Muted, style: Style::Line, points: exact });
    }
    series.push(Series { label: "FEM".into(), pen: Pen::Series(1), style: Style::Markers, points: ours });
    let title = format!("Far field at {} {}, φ = {phi}°", s.show_frequency(f), s.display.frequency.symbol());
    Ok(Chart { title, x: Axis::new("θ (degrees)", 0.0, 180.0), y: Axis::new(label, top - 40.0, top), series, marks: Vec::new() })
}

//! The dispersion diagram of a periodic cell.

use std::f64::consts::PI;

use emcraft_model::units::C0;
use emcraft_render::chart::{Chart, Series, Style};
use emcraft_render::{Axis, Pen};
use emcraft_solver::bloch::Dispersion;

use super::chart::span;
use crate::periodic::{exact_bands, solved};
use crate::{Result, Session};

/// Band `b`'s series colour (the palette's, cycled).
fn band(b: usize) -> u8 {
    (b % emcraft_render::palette::SERIES) as u8
}

/// The dispersion diagram: frequency against the phase per period, each band's solved points,
/// the closed form's curves where they apply and the light line f = c ψ/(2πL).
pub fn dispersion_chart(s: &Session) -> Result<Chart> {
    let d = solved(s)?;
    let fu = s.display.frequency;
    let (p0, p1) = span(d.points.iter().map(|p| p.psi.to_degrees()));
    let (p0, p1) = if p1 > p0 { (p0, p1) } else { (p0 - 10.0, p0 + 10.0) };
    let bands = d.points.iter().map(|p| p.frequencies.len()).max().unwrap_or(0);
    let f_top = d.points.iter().flat_map(|p| p.frequencies.iter().copied()).fold(0.0, f64::max);
    let mut series = exact_curves(s, d, (p0, p1), bands, f_top);
    // The light line of the cell's slowest medium is the reference a slow-wave structure is read
    // against: here vacuum's, f = c ψ/(2πL).
    if d.period > 0.0 {
        let light = |deg: f64| fu.from_si(C0 * deg.to_radians() / (2.0 * PI * d.period));
        series.push(Series::new("light", "light line", Pen::Muted, Style::Line, vec![[p0.max(0.0), light(p0.max(0.0))], [p1, light(p1)]]));
    }
    for b in 0..bands {
        let pts: Vec<[f64; 2]> = d.points.iter().filter_map(|p| p.frequencies.get(b).map(|f| [p.psi.to_degrees(), fu.from_si(*f)])).collect();
        series.push(Series::new(&format!("band{}", b + 1), format!("band {} (FEM)", b + 1), Pen::Series(band(b)), Style::Markers, pts));
    }
    let y1 = fu.from_si(f_top) * 1.1;
    Ok(Chart::new(
        format!("Dispersion along {}", emcraft_model::Axis::name_of(d.axis)),
        Axis::new("phase per period ψ (°)", p0, p1),
        Axis::new(&format!("frequency ({})", fu.symbol()), 0.0, if y1 > 0.0 { y1 } else { 1.0 }),
        series,
    ))
}

/// The closed form's bands from phase `p0` to `p1` (degrees), finely, each in its band's colour
/// paled; none where no closed form applies.
fn exact_curves(s: &Session, d: &Dispersion, (p0, p1): (f64, f64), bands: usize, f_top: f64) -> Vec<Series> {
    let fu = s.display.frequency;
    let mut exact: Vec<Vec<[f64; 2]>> = vec![Vec::new(); bands];
    let mut label = None;
    for deg in crate::numbers::evenly_spaced(p0, p1, 121) {
        if let Some((list, what)) = exact_bands(s, d, deg.to_radians(), bands, f_top * 1.5 + 1.0) {
            label = Some(what);
            for (b, f) in list.iter().take(bands).enumerate() {
                exact[b].push([deg, fu.from_si(*f)]);
            }
        }
    }
    let Some(what) = label else { return Vec::new() };
    exact
        .into_iter()
        .enumerate()
        .filter(|(_, p)| !p.is_empty())
        .map(|(b, pts)| Series::new("exact", if b == 0 { what.to_owned() } else { String::new() }, Pen::Faded(band(b)), Style::Line, pts))
        .collect()
}

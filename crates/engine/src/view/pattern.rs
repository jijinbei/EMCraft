//! The far-field pattern of an open problem: a cut through it (θ varying at an azimuth φ, or φ
//! varying at a polar angle θ), its total or one polarization's share.

use emcraft_render::Pen;
use emcraft_render::axes::Axis;
use emcraft_render::chart::{Chart, Series, Style};

use crate::open::{Antenna, FarAsk, FarRow, FarRows, FarSolver, Measure, far_rows, plane_wave};
use crate::{Result, Session};

emcraft_macros::named_enum! {
    /// Which cut of the far field a pattern shows (`cut`).
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub enum Cut {
        /// θ from 0 to 180° at an azimuth φ.
        #[default]
        Phi = "phi" => "φ cut",
        /// φ from 0 to 360° at a polar angle θ.
        Theta = "theta" => "θ cut",
    }
}

emcraft_macros::named_enum! {
    /// What a pattern shows of the far field (`component`): all of it, or one polarization's
    /// share (the total times its power over |E_θ|² + |E_φ|²): E_θ, E_φ, right- or left-hand
    /// circular, or Ludwig-3 co- or cross-polar about the co-polar direction.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub enum FarComponent {
        #[default]
        Total = "total" => "total",
        Theta = "theta" => "Eθ",
        Phi = "phi" => "Eφ",
        Right = "rhcp" => "RHCP",
        Left = "lhcp" => "LHCP",
        Co = "co" => "co-polar",
        Cross = "cross" => "cross-polar",
    }
}

emcraft_macros::named_enum! {
    /// What a source's pattern measures (`quantity`): directivity, gain (directivity times the
    /// radiation efficiency) or realized gain (with the feed's mismatch counted too).
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub enum FarQuantity {
        #[default]
        Directivity = "directivity" => "directivity",
        Gain = "gain" => "gain",
        Realized = "realized_gain" => "realized gain",
    }
}

impl FarComponent {
    /// An antenna row's dBi of this share of the directivity; `None` for the total and for E_θ's
    /// and E_φ's, read from the field itself.
    fn dbi(self, a: &Antenna) -> Option<f64> {
        match self {
            FarComponent::Total | FarComponent::Theta | FarComponent::Phi => None,
            FarComponent::Right => Some(a.directivity_rhcp_dbi),
            FarComponent::Left => Some(a.directivity_lhcp_dbi),
            FarComponent::Co => Some(a.directivity_co_dbi),
            FarComponent::Cross => Some(a.directivity_cross_dbi),
        }
    }
}

/// A far-field pattern's settings: the cut and its angle (degrees: φ for a φ cut, θ for a θ
/// cut), the quantity and component, the co-polar direction (degrees from x̂), the angles
/// drawn, and whose far field at which frequency.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatternView {
    pub cut: Cut,
    pub angle: f64,
    pub quantity: FarQuantity,
    pub component: FarComponent,
    pub co_polar: f64,
    pub points: usize,
    pub solver: Option<FarSolver>,
    /// The time domain's frequency (Hz; the nearest solved).
    pub frequency: Option<f64>,
    /// The dB below the pattern's top drawn ([`RANGES`]).
    pub range: f64,
    /// Drawn round a centre (the cut's angle around it, the dB outwards) rather than along an
    /// axis.
    pub polar: bool,
}

/// The dynamic ranges offered (dB below a pattern's top), the default first.
pub const RANGES: [f64; 4] = [40.0, 20.0, 30.0, 60.0];

/// The rings of a polar pattern: this many, evenly in dB from its floor to its top.
const RINGS: usize = 4;

/// A polar pattern's spokes, every this many degrees.
const SPOKE: f64 = 30.0;

impl Default for PatternView {
    fn default() -> PatternView {
        PatternView {
            cut: Cut::Phi,
            angle: 0.0,
            quantity: FarQuantity::Directivity,
            component: FarComponent::Total,
            co_polar: 0.0,
            points: 91,
            solver: None,
            frequency: None,
            range: RANGES[0],
            polar: false,
        }
    }
}

/// The far-field pattern of the last radiation solve against θ at azimuth `phi` (degrees),
/// `points` angles from 0 to 180°: directivity for sources, radar cross-section for a plane
/// wave, over the closed form where it applies.
pub fn pattern_chart(s: &Session, phi: f64, points: usize) -> Result<Chart> {
    pattern_chart_in(s, &PatternView { angle: phi, points, ..PatternView::default() })
}

/// [`pattern_chart`] from a chosen solver and frequency (see `results.far_field`).
pub fn pattern_chart_of(s: &Session, phi: f64, points: usize, solver: Option<FarSolver>, frequency: Option<f64>) -> Result<Chart> {
    pattern_chart_in(s, &PatternView { angle: phi, points, solver, frequency, ..PatternView::default() })
}

/// The pattern `view` asks for: a φ cut (θ from 0 to 180°) or a θ cut (φ from 0 to 360°), the
/// total or one polarization's share in dB, over the closed form (the total's) where it
/// applies.
///
/// ```
/// use emcraft_engine::view::{Cut, FarComponent, PatternView, pattern_chart_in};
///
/// let mut s = emcraft_engine::Session::new();
/// // No radiation solved: no pattern, and why.
/// assert!(pattern_chart_in(&s, &PatternView::default()).is_err());
/// let view = PatternView { cut: Cut::Theta, angle: 90.0, component: FarComponent::Phi, ..PatternView::default() };
/// assert_eq!(view.cut.id(), "theta");
/// ```
pub fn pattern_chart_in(s: &Session, view: &PatternView) -> Result<Chart> {
    let n = view.points.clamp(13, 721);
    let angle = if view.angle.is_finite() { view.angle } else { 0.0 };
    let (span, along) = match view.cut {
        Cut::Phi => (180.0, "θ"),
        Cut::Theta => (360.0, "φ"),
    };
    let sweep: Vec<f64> = (0..n).map(|i| span * i as f64 / (n - 1) as f64).collect();
    let dirs: Vec<(f64, f64)> = sweep
        .iter()
        .map(|a| match view.cut {
            Cut::Phi => (a.to_radians(), angle.to_radians()),
            Cut::Theta => (angle.to_radians(), a.to_radians()),
        })
        .collect();
    let co_polar = if view.co_polar.is_finite() { view.co_polar.to_radians() } else { 0.0 };
    let FarRows { frequency: f, rows, .. } = far_rows(s, &dirs, &FarAsk { solver: view.solver, frequency: view.frequency, co_polar })?;
    let scattering = plane_wave(s).is_some();
    let label = match (scattering, view.quantity) {
        (true, _) => "RCS (dBsm)",
        (false, FarQuantity::Directivity) => "directivity (dBi)",
        (false, FarQuantity::Gain) => "gain (dBi)",
        (false, FarQuantity::Realized) => "realized gain (dBi)",
    };
    // The quantity asked in a direction (dB), and the closed form's.
    let value = |row: &FarRow| match (&row.measure, view.quantity) {
        (Measure::Rcs(r), _) => Some(r.rcs_dbsm),
        (Measure::Antenna(a), FarQuantity::Directivity) => Some(a.directivity_dbi),
        (Measure::Antenna(a), FarQuantity::Gain) => a.gain.map(|g| g.gain_dbi),
        (Measure::Antenna(a), FarQuantity::Realized) => a.gain.map(|g| g.realized_gain_dbi),
    };
    let l = s.display.length.from_si(1.0);
    let exact_of = |row: &FarRow| match (&row.measure, view.quantity) {
        (Measure::Rcs(r), _) => r.exact_rcs.map(|x| 10.0 * (x / (l * l)).log10()),
        (Measure::Antenna(a), FarQuantity::Directivity) => a.exact_directivity.map(|x| 10.0 * x.log10()),
        (Measure::Antenna(_), _) => None,
    };
    if !scattering && view.quantity != FarQuantity::Directivity && rows.first().is_some_and(|r| value(r).is_none()) {
        return Err(crate::EngineError::failed(format!("no {}: the model has no feed (a lumped or waveguide port)", view.quantity.label())));
    }
    // A polarization's share of the power in a direction, in dB.
    let share = |row: &FarRow| -> f64 {
        if view.component == FarComponent::Total {
            return 0.0;
        }
        if !matches!(view.component, FarComponent::Theta | FarComponent::Phi) {
            return match &row.measure {
                Measure::Antenna(a) => view.component.dbi(a).map_or(f64::NAN, |part| part - a.directivity_dbi),
                // A plane wave's rows have no such share: not drawn.
                Measure::Rcs(_) => f64::NAN,
            };
        }
        let (t, p) = (row.e_theta.norm().powi(2), row.e_phi.norm().powi(2));
        let part = if view.component == FarComponent::Theta { t } else { p };
        if t + p > 0.0 { 10.0 * (part / (t + p)).log10() } else { f64::NEG_INFINITY }
    };
    let ours: Vec<[f64; 2]> =
        rows.iter().zip(&sweep).filter_map(|(row, a)| Some([*a, value(row)? + share(row)])).filter(|q| !q[1].is_nan()).collect();
    let exact: Vec<[f64; 2]> = if view.component == FarComponent::Total {
        rows.iter().zip(&sweep).filter_map(|(row, a)| Some([*a, exact_of(row)?])).collect()
    } else {
        Vec::new()
    };
    let top = ours.iter().chain(&exact).map(|q| q[1]).filter(|v| v.is_finite()).fold(f64::NEG_INFINITY, f64::max);
    let top = if top.is_finite() { (top / 5.0).ceil() * 5.0 } else { 0.0 };
    let mut series = Vec::new();
    if !exact.is_empty() {
        let name = if scattering { "Mie series" } else { "Hertzian dipole" };
        series.push(Series::new("exact", name, Pen::Muted, Style::Line, exact));
    }
    let fem = match view.component {
        FarComponent::Total => "FEM".to_owned(),
        c => format!("FEM, {}", c.label()),
    };
    series.push(Series::new("fem", fem, Pen::Series(1), Style::Markers, ours));
    let held = match view.cut {
        Cut::Phi => format!("φ = {angle}°"),
        Cut::Theta => format!("θ = {angle}°"),
    };
    let title = format!("Far field at {} {}, {held}", s.show_frequency(f), s.display.frequency.symbol());
    let range = if view.range.is_finite() { view.range.clamp(10.0, 100.0) } else { RANGES[0] };
    if view.polar {
        return Ok(polar(title, label, series, view.cut, top, range));
    }
    Ok(Chart::new(title, Axis::new(&format!("{along} (degrees)"), 0.0, span), Axis::new(label, top - range, top), series))
}

/// A cut drawn round a centre: each point at its angle (θ from the vertical for a φ cut, φ from
/// the right for a θ cut), `range` dB below `top` at the centre and `top` at the outer ring;
/// rings every quarter of the range and spokes every 30° under it, its label naming the rings.
fn polar(title: String, label: &str, series: Vec<Series>, cut: Cut, top: f64, range: f64) -> Chart {
    let place = |angle: f64, db: f64| {
        let r = ((db - (top - range)) / range).clamp(0.0, 1.0);
        let a = angle.to_radians();
        match cut {
            Cut::Phi => [r * a.sin(), r * a.cos()],
            Cut::Theta => [r * a.cos(), r * a.sin()],
        }
    };
    let grid = |points: Vec<[f64; 2]>| Series { width: Some(0.8), ..Series::new("grid", "", Pen::Muted, Style::Line, points) };
    let circle = |r: f64| grid((0..=72).map(|k| f64::from(k) * 5.0_f64.to_radians()).map(|a| [r * a.cos(), r * a.sin()]).collect());
    let rings = (1..=RINGS).map(|k| circle(k as f64 / RINGS as f64));
    let spokes = (0..(360.0 / SPOKE) as usize).map(|k| {
        let a = (k as f64 * SPOKE).to_radians();
        grid(vec![[0.0, 0.0], [a.cos(), a.sin()]])
    });
    let placed = series.into_iter().map(|s| Series { points: s.points.iter().map(|p| place(p[0], p[1])).collect(), ..s });
    let step = range / RINGS as f64;
    let named = format!("{label}: rings every {step} dB, outer {top}");
    let mut chart = Chart::new(title, Axis::new("", -1.08, 1.08), Axis::new(&named, -1.08, 1.08), rings.chain(spokes).chain(placed).collect());
    chart.look.grid = false;
    chart.look.equal = true;
    chart.look.frame = false;
    chart
}

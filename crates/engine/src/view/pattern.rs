//! The far-field pattern of an open problem: a cut through it (θ varying at an azimuth φ, or φ
//! varying at a polar angle θ), its total or one polarization's share.

use emcraft_render::Pen;
use emcraft_render::axes::Axis;
use emcraft_render::chart::{Chart, Series, Style};
use serde_json::Value;

use crate::open::{FarAsk, FarRows, FarSolver, far_rows, plane_wave};
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
    /// The far-field row's dBi of this share of the directivity; `None` for E_θ's and E_φ's,
    /// read from the field itself.
    fn key(self) -> Option<&'static str> {
        match self {
            FarComponent::Total => Some("directivity_dbi"),
            FarComponent::Theta | FarComponent::Phi => None,
            FarComponent::Right => Some("directivity_rhcp_dbi"),
            FarComponent::Left => Some("directivity_lhcp_dbi"),
            FarComponent::Co => Some("directivity_co_dbi"),
            FarComponent::Cross => Some("directivity_cross_dbi"),
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
}

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
    let (key, exact_key, label) = match (scattering, view.quantity) {
        (true, _) => ("rcs_dbsm", "exact_rcs", "RCS (dBsm)"),
        (false, FarQuantity::Directivity) => ("directivity_dbi", "exact_directivity", "directivity (dBi)"),
        (false, FarQuantity::Gain) => ("gain_dbi", "", "gain (dBi)"),
        (false, FarQuantity::Realized) => ("realized_gain_dbi", "", "realized gain (dBi)"),
    };
    if !scattering && view.quantity != FarQuantity::Directivity && rows.first().is_some_and(|r| r.get(key).is_none()) {
        return Err(crate::EngineError::failed(format!("no {}: the model has no feed (a lumped or waveguide port)", view.quantity.label())));
    }
    let l = s.display.length.from_si(1.0);
    let db = |v: &Value| v.as_f64().map(|x| if scattering { 10.0 * (x / (l * l)).log10() } else { 10.0 * x.log10() });
    // A polarization's share of the power in a direction, in dB.
    let share = |row: &Value| -> f64 {
        if view.component == FarComponent::Total {
            return 0.0;
        }
        if let Some(k) = view.component.key() {
            return match (row[k].as_f64(), row["directivity_dbi"].as_f64()) {
                (Some(part), Some(all)) => part - all,
                // No such share (a part of nothing, or a plane wave's rows): not drawn.
                _ => f64::NAN,
            };
        }
        let mag = |k: &str| row[k]["mag"].as_f64().unwrap_or(0.0);
        let (t, p) = (mag("e_theta").powi(2), mag("e_phi").powi(2));
        let part = if view.component == FarComponent::Theta { t } else { p };
        if t + p > 0.0 { 10.0 * (part / (t + p)).log10() } else { f64::NEG_INFINITY }
    };
    let ours: Vec<[f64; 2]> =
        rows.iter().zip(&sweep).filter_map(|(row, a)| Some([*a, row[key].as_f64()? + share(row)])).filter(|q| !q[1].is_nan()).collect();
    let exact: Vec<[f64; 2]> = if view.component == FarComponent::Total && !exact_key.is_empty() {
        rows.iter().zip(&sweep).filter_map(|(row, a)| Some([*a, db(&row[exact_key])?])).collect()
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
    Ok(Chart::new(title, Axis::new(&format!("{along} (degrees)"), 0.0, span), Axis::new(label, top - 40.0, top), series))
}

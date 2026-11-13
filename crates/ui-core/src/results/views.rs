//! The other result pages' choices as every window shows them, each made into its chart by the
//! engine: the TDR's port and transform, the far-field cut (its angle, quantity and
//! polarization), the pulse sent through (`results.pulse`'s settings as typed, and whether its
//! power or its instantaneous frequency is charted), and the dispersion diagram's table.

use emcraft_engine::Session;
use emcraft_engine::tdr::Mode;
use emcraft_engine::view::{Cut, FarComponent, FarQuantity, PatternView};
use serde_json::{Value, json};

use super::Table;
use crate::lang::{Text, args, trf};

/// The TDR's port (the first entry of S when none) and transform (the engine's pick when none).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TdrView {
    pub port: Option<String>,
    pub mode: Option<Mode>,
}

/// The transforms offered, with their names and tips (`None`: the engine picks).
pub const TDR_MODES: [(Option<Mode>, Text, Text); 3] = [
    (None, t!("Auto"), t!("Low pass on a harmonic sweep (f = k Δf), else band pass")),
    (Some(Mode::LowPass), t!("Low pass"), t!("The step response: a step at each change of impedance (lines without a cut-off)")),
    (Some(Mode::BandPass), t!("Band pass"), t!("|h(t)|: a peak where each reflection comes from (guides with a cut-off)")),
];

impl TdrView {
    /// `results.tdr`'s parameters.
    pub fn params(&self) -> Value {
        let mut p = json!({});
        if let Some(port) = &self.port {
            p["port"] = json!(port);
        }
        if let Some(mode) = self.mode {
            p["mode"] = json!(mode.id());
        }
        p
    }

    /// The chart `session` gives for it.
    pub fn chart(&self, s: &Session) -> emcraft_engine::Result<emcraft_render::chart::Chart> {
        emcraft_engine::results_tdr_chart(s, &emcraft_engine::Params::new(&self.params(), s.display()))
    }
}

/// The far-field cut: θ varying at an azimuth φ, or φ varying at a polar angle θ; the angle
/// (degrees); what it measures; the total or one polarization's share; the dB drawn below its
/// top, and whether it is drawn round a centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatternCut {
    pub cut: Cut,
    pub angle: f64,
    pub quantity: FarQuantity,
    pub component: FarComponent,
    pub range: f64,
    pub polar: bool,
}

impl Default for PatternCut {
    fn default() -> Self {
        PatternCut {
            cut: Cut::Phi,
            angle: 0.0,
            quantity: FarQuantity::Directivity,
            component: FarComponent::Total,
            range: emcraft_engine::view::RANGES[0],
            polar: false,
        }
    }
}

impl PatternCut {
    /// The cut `cut`, its angle where it starts (φ 0°, θ 90°) when it changes.
    pub fn with_cut(self, cut: Cut) -> PatternCut {
        if cut == self.cut { self } else { PatternCut { cut, angle: if cut == Cut::Theta { 90.0 } else { 0.0 }, ..self } }
    }

    /// The angle's range (degrees) and its name for the cut.
    pub fn range(&self) -> (f64, f64, &'static str) {
        match self.cut {
            Cut::Phi => (0.0, 360.0, "φ (°)"),
            Cut::Theta => (0.0, 180.0, "θ (°)"),
        }
    }

    /// The chart `session` gives for it.
    pub fn chart(&self, s: &Session) -> emcraft_engine::Result<emcraft_render::chart::Chart> {
        let (lo, hi, _) = self.range();
        let angle = if self.angle.is_finite() { self.angle.clamp(lo, hi) } else { lo };
        let view = PatternView {
            cut: self.cut,
            angle,
            quantity: self.quantity,
            component: self.component,
            range: self.range,
            polar: self.polar,
            ..PatternView::default()
        };
        emcraft_engine::view::pattern_chart_in(s, &view)
    }
}

/// The chirps offered: their ids, names and tips.
pub const CHIRPS: [(&str, Text, Text); 4] = [
    ("optimized", t!("Optimized"), t!("The better of the best linear and the best matched chirp")),
    ("matched", t!("Matched"), t!("Its frequency follows the line's group delay")),
    ("linear", t!("Linear"), t!("A straight sweep: the band given, else −1/D")),
    ("none", t!("None"), t!("No chirp")),
];

/// The pulse sent through, as typed: the carrier (display units; the sweep's middle when empty),
/// the width (ns), a cell's guide length (display units), the chirp and a linear chirp's band,
/// which of a cell's Bloch waves (from 1); whether the chart shows the instantaneous frequency
/// rather than the power; and whether the project's own pulse was sent once.
#[derive(Clone, Debug, PartialEq)]
pub struct PulseForm {
    pub frequency: String,
    pub width: String,
    pub length: String,
    pub bandwidth: String,
    pub chirp: Option<String>,
    pub wave: Option<usize>,
    pub show_frequency: bool,
    pub restored: bool,
}

impl Default for PulseForm {
    fn default() -> Self {
        PulseForm {
            frequency: String::new(),
            width: crate::dialogs::short_number(emcraft_engine::pulse::DEFAULT_WIDTH_NS),
            length: String::new(),
            bandwidth: String::new(),
            chirp: None,
            wave: None,
            show_frequency: false,
            restored: false,
        }
    }
}

impl PulseForm {
    /// The chirp chosen: as set, else linear when a band is typed, else optimized.
    pub fn chirp(&self) -> &str {
        self.chirp.as_deref().unwrap_or_else(|| if self.bandwidth.trim().is_empty() { "optimized" } else { "linear" })
    }

    /// `results.pulse`'s parameters (a number that does not read is sent as typed, for the
    /// engine to refuse with its reason).
    ///
    /// ```
    /// use emcraft_ui_core::results::views::PulseForm;
    /// let f = PulseForm { frequency: "9.5".into(), width: "2".into(), ..PulseForm::default() };
    /// assert_eq!(f.params(), serde_json::json!({"frequency": 9.5, "width": 2.0, "chirp": "optimized"}));
    /// ```
    pub fn params(&self) -> Value {
        let number = |s: &str| s.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map_or_else(|| json!(s.trim()), |v| json!(v));
        let mut p = json!({"width": number(&self.width), "chirp": self.chirp()});
        for (key, text) in [("frequency", &self.frequency), ("length", &self.length)] {
            if !text.trim().is_empty() {
                p[key] = number(text);
            }
        }
        if self.chirp() == "linear" && !self.bandwidth.trim().is_empty() {
            p["bandwidth"] = number(&self.bandwidth);
        }
        if let Some(w) = self.wave {
            p["wave"] = json!(w);
        }
        p
    }

    /// The form filled from settings `results.pulse` was run with (a project's own pulse).
    pub fn of(v: &Value) -> PulseForm {
        let text = |k: &str| v.get(k).map_or_else(String::new, crate::dialogs::value_text);
        let width = Some(text("width")).filter(|w| !w.is_empty()).unwrap_or_else(|| PulseForm::default().width);
        PulseForm {
            frequency: text("frequency"),
            width,
            length: text("length"),
            bandwidth: text("bandwidth"),
            chirp: v.get("chirp").and_then(Value::as_str).map(str::to_owned),
            wave: v.get("wave").and_then(Value::as_u64).map(|w| w as usize),
            ..PulseForm::default()
        }
    }
}

/// The dispersion diagram's table: the phase per period, β, and each band's frequency (display
/// units), with the unknowns after the periodic reduction and the period in its caption.
pub fn dispersion(s: &Session) -> Option<Table> {
    let d = s.results().dispersion.as_ref()?;
    let (fu, lu) = (s.display().frequency, s.display().length);
    let bands = d.points.iter().map(|p| p.frequencies.len()).max().unwrap_or(0);
    let period = format!("{:.4} {}", lu.from_si(d.period), lu.symbol());
    let caption = trf(t!("{n} unknowns after the periodic reduction; period {period}"), args!(n = d.unknowns, period = period));
    let headers = ["ψ (°)".to_owned(), format!("β (rad/{})", lu.symbol())]
        .into_iter()
        .chain((0..bands).map(|b| trf(t!("band {n} ({unit})"), args!(n = (b + 1), unit = fu.symbol()))))
        .collect();
    let rows = d
        .points
        .iter()
        .map(|p| {
            let bands = (0..bands).map(|b| p.frequencies.get(b).map_or(String::new(), |f| format!("{:.4}", fu.from_si(*f))));
            [format!("{:.1}", p.psi.to_degrees()), format!("{:.4}", p.beta * lu.to_si(1.0))].into_iter().chain(bands).collect()
        })
        .collect();
    Some(Table { caption: Some(caption), headers, rows, marks: None })
}

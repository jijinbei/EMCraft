//! The viewport's state beside the session: the field view's settings and samples, and what
//! the result views keep (their charts and tables, the settings they were drawn with).

use emcraft_engine::view::{FieldView, Plane, PlaneSamples, Probe};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::picture::{Cached, PictureKey};
use crate::ui_kit;

/// The field view's state.
pub struct FieldState {
    pub view: FieldView,
    /// The plane's position in display length units; the middle when `None`.
    pub at: Option<f64>,
    pub samples: Option<PlaneSamples>,
    pub picture: Cached<PictureKey<FieldView>>,
    /// Where the picture puts the plane, for reading the pointer's position.
    pub layout: Option<emcraft_render::plot::Layout>,
    /// The phase animation, played like a video.
    pub player: ui_kit::Player,
    /// The fields under the pointer (last frame), and the point clicked on the plane (display
    /// units along the picture's axes), read again whenever the view changes.
    pub hover: Option<Probe>,
    pub pinned: Option<[f64; 2]>,
    /// Planes kept for the 3D view beside the field view's own.
    pub kept: Vec<KeptPlane>,
    /// The model in 3D beside the picture.
    pub beside_3d: bool,
    /// The fields at the project's probes for the view's solution, and what they were read for.
    pub probe_values: Option<(ProbeKey, Vec<Option<(emcraft_model::CVec3, emcraft_model::CVec3)>>)>,
    /// A probe being dragged on the picture: its index and where it is now (picture axes).
    pub dragging: Option<(usize, [f64; 2])>,
}

/// What the probes' values were read for: the session's revision, the solution (frequency,
/// mode, excitation) and the probes' points.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeKey {
    pub revision: u64,
    pub frequency: f64,
    pub mode: Option<usize>,
    pub excitation: usize,
    pub points: Vec<[f64; 3]>,
}

impl Default for FieldState {
    fn default() -> Self {
        FieldState {
            view: FieldView { show_h: false, ..FieldView::new(10.0e9, Plane::H) },
            at: None,
            samples: None,
            picture: Cached::default(),
            layout: None,
            player: ui_kit::Player::default(),
            hover: None,
            pinned: None,
            kept: Vec::new(),
            beside_3d: false,
            probe_values: None,
            dragging: None,
        }
    }
}

impl FieldState {
    /// Lets the samples and the picture go after the session changed (the settings stay).
    pub fn forget_samples(&mut self) {
        self.samples = None;
        self.picture.clear();
        self.pinned = None;
        for k in &mut self.kept {
            k.samples = None;
        }
    }
}

/// The TDR view's settings as `results.tdr`'s parameters (and `view.tdr`'s): the port and the
/// transform, the engine's defaults when `None`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TdrParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<emcraft_engine::tdr::Mode>,
}

impl TdrParams {
    /// The command's JSON parameters.
    pub fn to_params(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }

    /// The settings a run of `results.tdr` asked for (the defaults where it could not say).
    pub fn of(params: &Value) -> TdrParams {
        serde_json::from_value(params.clone()).unwrap_or_default()
    }
}

/// The TDR view's settings and its chart.
#[derive(Default)]
pub struct TdrState {
    pub params: TdrParams,
    pub chart: Cached<PictureKey<TdrParams>>,
}

/// The study view's chart, the S-parameter it shows (a sweep's; the first when `None`) and, for
/// a grid sweep, the slice: the variable charted along and the value each other one is held at.
#[derive(Default)]
pub struct StudyState {
    pub chart: Cached<PictureKey<(Option<String>, emcraft_engine::study::Slice)>>,
    pub parameter: Option<String>,
    pub slice: emcraft_engine::study::Slice,
}

/// Which chart the shared chart slot of [`ViewState`] holds.
#[derive(Clone, Debug, PartialEq)]
pub enum SharedChart {
    /// The S-parameters, in dB, as magnitudes or as phases, with the kept solves drawn over them.
    SParameters(emcraft_engine::view::SDisplay, Vec<u32>),
    Dispersion,
    /// A cut through the far-field pattern.
    Pattern(PatternSettings),
}

/// The far-field view's cut, its angle (degrees: φ of a φ cut, θ of a θ cut) and component.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatternSettings {
    pub cut: emcraft_engine::view::Cut,
    pub angle: f64,
    pub component: emcraft_engine::view::FarComponent,
}

impl Default for PatternSettings {
    fn default() -> Self {
        PatternSettings { cut: emcraft_engine::view::Cut::Phi, angle: 0.0, component: emcraft_engine::view::FarComponent::Total }
    }
}

/// The chart style card: which chart's is open, its text fields as typed, and the style (as
/// `view.chart.style` shows it) they were filled from.
#[derive(Clone, Debug, Default)]
pub struct StyleEdit {
    pub open: Option<emcraft_engine::views::ChartId>,
    pub title: String,
    /// Each axis' fields, in [`emcraft_engine::views::AxisId::ALL`]'s order.
    pub axes: [AxisText; 2],
    pub seen: Option<Value>,
}

/// An axis' label, low end and high end as typed in the chart style card.
#[derive(Clone, Debug, Default)]
pub struct AxisText {
    pub label: String,
    pub min: String,
    pub max: String,
}

/// A band of the S-parameter chart picked by dragging across it, to solve more points in
/// (`solve.refine`): its ends (Hz), where a drag began, the points asked.
#[derive(Clone, Debug, PartialEq)]
pub struct Refine {
    pub band: Option<(f64, f64)>,
    pub from: Option<f64>,
    pub points: usize,
}

impl Default for Refine {
    fn default() -> Self {
        Refine { band: None, from: None, points: 11 }
    }
}

/// A port mode's picture: the port, the mode's index and the frequency (Hz).
#[derive(Clone, Debug, PartialEq)]
pub struct ModePicture {
    pub port: String,
    pub mode: usize,
    pub frequency: f64,
}

/// The eigenmode table (`results.eigenmodes`' reply, or why it failed) and the session revision
/// it was read at.
#[derive(Clone, Debug, PartialEq)]
pub struct EigenTable {
    pub revision: u64,
    pub reply: Result<Value, String>,
}

/// A plane kept for the 3D view beside the field view's own: which, its position (display
/// units), and its samples once taken.
#[derive(Clone, Debug)]
pub struct KeptPlane {
    pub plane: Plane,
    pub at: f64,
    pub samples: Option<PlaneSamples>,
}

/// What the result views keep between frames.
#[derive(Default)]
pub struct ViewState {
    /// The chart of the S-parameter, far-field or dispersion view (one at a time).
    pub chart: Cached<PictureKey<SharedChart>>,
    /// Where the chart last drawn put its plot area, and its curves (keys and plain labels,
    /// those hidden too).
    pub layout: Option<emcraft_render::axes::Layout>,
    pub curves: Vec<(String, String)>,
    /// The chart style card.
    pub style: StyleEdit,
    /// The S-parameter chart's band to refine, and each port's reference plane as edited.
    pub refine: Refine,
    pub deembed: Vec<f64>,
    /// The far-field pattern's cut.
    pub pattern: PatternSettings,
    /// What the S-parameter chart shows: |S| in dB, magnitude or phase.
    pub sdisplay: emcraft_engine::view::SDisplay,
    /// The kept solves (`results.history`) drawn over the S-parameter chart.
    pub compare: Vec<u32>,
    pub tdr: TdrState,
    pub study: StudyState,
    /// The picked port mode's picture.
    pub mode_picture: Cached<PictureKey<ModePicture>>,
    /// The eigenmode table and the session revision it was read at.
    pub eigen_table: Option<EigenTable>,
}

/// The replies of the commands whose results the views show as they came: the last port-mode
/// solve's, the last comparison with theory's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Replies {
    pub port_modes: Option<Value>,
    pub compare: Option<Value>,
}

#[cfg(test)]
mod tests {
    use emcraft_engine::tdr::Mode;
    use serde_json::json;

    use super::*;

    #[test]
    fn the_tdr_settings_are_results_tdr_s_parameters() {
        let all = TdrParams { port: Some("2".into()), mode: Some(Mode::BandPass) };
        assert_eq!(all.to_params(), json!({"port": "2", "mode": "bandpass"}));
        assert_eq!(TdrParams::default().to_params(), json!({}));
        for p in [all, TdrParams::default(), TdrParams { port: None, mode: Some(Mode::LowPass) }] {
            assert_eq!(TdrParams::of(&p.to_params()), p);
        }
        // What the view does not hold is left out; what it cannot read leaves the defaults.
        assert_eq!(TdrParams::of(&json!({"port": "1", "window": "hann"})).port.as_deref(), Some("1"));
        assert_eq!(TdrParams::of(&json!({"mode": "sideways"})), TdrParams::default());
    }
}

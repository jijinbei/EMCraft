//! The viewport's state beside the session: the field view's settings and samples, and what
//! the result views keep (their charts and tables, the settings they were drawn with).

use emcraft_engine::view::{FieldView, Plane, PlaneSamples, Probe};
use serde_json::Value;

use crate::picture::Cached;
use crate::ui_kit;

/// The field view's state.
pub struct FieldState {
    pub view: FieldView,
    /// The plane's position in display length units; the middle when `None`.
    pub at: Option<f64>,
    pub samples: Option<PlaneSamples>,
    pub picture: Cached,
    /// Where the picture puts the plane, for reading the pointer's position.
    pub layout: Option<emcraft_render::plot::Layout>,
    /// The phase animation, played like a video.
    pub player: ui_kit::Player,
    /// The fields under the pointer (last frame), and the point clicked on the plane (display
    /// units along the picture's axes), read again whenever the view changes.
    pub hover: Option<Probe>,
    pub pinned: Option<[f64; 2]>,
    /// Planes kept for the 3D view beside the field view's own (plane, position in display
    /// units), and their samples.
    pub kept: Vec<(Plane, f64)>,
    pub kept_samples: Vec<Option<PlaneSamples>>,
    /// The model in 3D beside the picture.
    pub beside_3d: bool,
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
            kept_samples: Vec::new(),
            beside_3d: false,
        }
    }
}

impl FieldState {
    /// Lets the samples and the picture go after the session changed (the settings stay).
    pub fn forget_samples(&mut self) {
        self.samples = None;
        self.picture.clear();
        self.pinned = None;
        self.kept_samples.clear();
    }
}

/// The TDR view's settings (the port and the transform, defaults when `None`) and its chart.
#[derive(Default)]
pub struct TdrState {
    pub port: Option<String>,
    pub mode: Option<emcraft_engine::tdr::Mode>,
    pub chart: Cached,
}

/// The study view's chart, the S-parameter it shows (a sweep's; the first when `None`) and, for
/// a grid sweep, the slice: the variable charted along and the value each other one is held at.
#[derive(Default)]
pub struct StudyState {
    pub chart: Cached,
    pub parameter: Option<String>,
    pub slice: emcraft_engine::study::Slice,
}

/// What the result views keep between frames.
#[derive(Default)]
pub struct ViewState {
    /// The chart of the S-parameter, far-field or dispersion view (one at a time).
    pub chart: Cached,
    /// The far-field pattern's azimuth (degrees).
    pub pattern_phi: f64,
    pub tdr: TdrState,
    pub study: StudyState,
    /// The last port-mode solve's reply, and the picked mode's picture.
    pub port_modes: Option<Value>,
    pub mode_picture: Cached,
    /// The last comparison with theory's reply.
    pub compare: Option<Value>,
    /// The eigenmode table and the session revision it was read at.
    pub eigen_table: Option<(u64, Value)>,
}

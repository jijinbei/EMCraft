//! The field on its plane in the 3D view: the field view's plane sampled once per solution and
//! plane (kept until the project or what is sampled changes), plotted at its component and
//! phase, and its colour bar as a picture with its title and ticks.

use std::sync::Arc;

use emcraft_engine::Session;
use emcraft_engine::view::{FieldView, Plane, PlaneSamples};
use emcraft_render::plot::FieldPlot;
use emcraft_ui_core::scene3d::ColourBar;
use iced::widget::image::Handle;

/// Seconds a played phase takes to go round once.
pub const PERIOD: f64 = 2.0;

/// Strips in the colour bar's gradient.
const BAR_STRIPS: u32 = 90;

/// The field view the window starts from: the H-plane's E field.
pub fn start() -> FieldView {
    FieldView { show_h: false, ..FieldView::new(10.0e9, Plane::H) }
}

/// The plane's samples for the window's generation and the view (its phase aside: a phase
/// changes the plot, not the samples).
#[derive(Clone, Debug)]
pub struct Sampled {
    generation: u64,
    view: FieldView,
    samples: Arc<PlaneSamples>,
}

/// What is sampled of `view`: all of it but its phase, component and colours.
fn sampled_of(view: &FieldView) -> FieldView {
    FieldView { phase: 0.0, component: Default::default(), db: None, contours: false, clamp: None, ..view.clone() }
}

/// `view`'s plot from `old`'s samples when they serve it, else from the plane sampled again;
/// the samples kept for next time.
pub fn plot(session: &Session, generation: u64, view: &FieldView, old: Option<Sampled>) -> (Option<Sampled>, Result<FieldPlot, String>) {
    let wanted = sampled_of(view);
    let kept = old.filter(|s| s.generation == generation && s.view == wanted);
    let sampled = match kept {
        Some(s) => Ok(s),
        None => emcraft_engine::view::sample_plane(session, view)
            .map(|samples| Sampled { generation, view: wanted, samples: Arc::new(samples) })
            .map_err(|e| e.to_string()),
    };
    match sampled {
        Ok(s) => {
            let plot = s.samples.plot(view).map_err(|e| e.to_string());
            (Some(s), plot)
        }
        Err(e) => (None, Err(e)),
    }
}

impl Sampled {
    /// The samples themselves.
    pub fn samples(&self) -> &Arc<PlaneSamples> {
        &self.samples
    }

    /// The plane and its position along its normal (m from the model's low side).
    pub fn place(&self) -> (Plane, f64) {
        (self.samples.plane, self.samples.at)
    }
}

/// The colour bar as the window shows it: its title in plain Unicode, its gradient (a picture a
/// pixel high, stretched), its ticks' texts, low to high (an end the field goes past marked ≥
/// or ≤), and the field's peak over the period with where it is on the plane (display units).
#[derive(Clone, Debug)]
pub struct Bar {
    pub title: String,
    pub gradient: Handle,
    pub ticks: Vec<String>,
    pub peak: Option<(String, [f64; 2])>,
}

impl Bar {
    /// The bar of `b`, the field strongest (`strongest`: where, and its amplitude) as it says.
    pub fn of(b: &ColourBar, strongest: Option<([f64; 2], f64)>) -> Bar {
        let label = emcraft_render::math::to_unicode(&b.label);
        let title = if b.scale.db.is_some() { format!("{label} · dB") } else { label };
        let rgba = (0..BAR_STRIPS)
            .flat_map(|k| {
                let c = b.scale.at((f64::from(k) + 0.5) / f64::from(BAR_STRIPS));
                [c.0, c.1, c.2, 255]
            })
            .collect::<Vec<u8>>();
        // Past the ends: the peak beyond the full colour (a hair's room for rounding).
        let past = strongest.is_some_and(|(_, a)| a > b.scale.peak * (1.0 + 1e-9));
        let ticks = b.scale.ticks();
        let last = ticks.len().saturating_sub(1);
        let ticks = ticks
            .into_iter()
            .enumerate()
            .map(|(k, (at, _))| {
                let mark = match (past, k) {
                    (true, 0) if b.scale.signed => "≤",
                    (true, k) if k == last => "≥",
                    _ => "",
                };
                format!("{mark}{}", b.scale.tick_text(at))
            })
            .collect();
        let peak = strongest.map(|(at, a)| (emcraft_render::axes::format(a), at));
        Bar { title, gradient: Handle::from_rgba(BAR_STRIPS, 1, rgba), ticks, peak }
    }
}

/// The axis across a plane (0 x, 1 y, 2 z): the H-plane lies across y, the E-plane across x, a
/// cross-section across z.
pub fn normal(plane: Plane) -> usize {
    match plane {
        Plane::H => 1,
        Plane::E => 0,
        Plane::Cross => 2,
    }
}

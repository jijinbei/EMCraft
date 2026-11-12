//! The field on its plane in the 3D view: the field view's plane sampled once per solution and
//! plane (kept until the project or what is sampled changes), plotted at its component and
//! phase, and its colour bar as a picture with its title and ticks.

use std::sync::Arc;

use emcraft_engine::Session;
use emcraft_engine::view::{FieldView, Plane, PlaneSamples};
use emcraft_render::plot::FieldPlot;
use emcraft_ui_core::scene3d::ColourBar;
use iced::widget::image::Handle;

/// Strips in the colour bar's gradient.
const BAR_STRIPS: u32 = 90;

/// The field view the window starts from: the H-plane's E field (the egui window's).
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
    FieldView { phase: 0.0, component: Default::default(), db: None, contours: false, ..view.clone() }
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
/// pixel high, stretched) and its ticks' texts, low to high.
#[derive(Clone, Debug)]
pub struct Bar {
    pub title: String,
    pub gradient: Handle,
    pub ticks: Vec<String>,
}

impl Bar {
    pub fn of(b: &ColourBar) -> Bar {
        let label = emcraft_render::math::to_unicode(&b.label);
        let title = if b.scale.db.is_some() { format!("{label} · dB") } else { label };
        let rgba = (0..BAR_STRIPS)
            .flat_map(|k| {
                let c = b.scale.at((f64::from(k) + 0.5) / f64::from(BAR_STRIPS));
                [c.0, c.1, c.2, 255]
            })
            .collect::<Vec<u8>>();
        let ticks = b.scale.ticks().into_iter().map(|(at, _)| b.scale.tick_text(at)).collect();
        Bar { title, gradient: Handle::from_rgba(BAR_STRIPS, 1, rgba), ticks }
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

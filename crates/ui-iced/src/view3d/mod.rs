//! The 3D view: the session's scene as the shared rules build it ([`scene3d::build`], in the
//! theme's colours) drawn by the GPU in iced's shader widget ([`shader`]), navigated by the mouse
//! mapping (orbit, pan, zoom), fitted to the model and turned to the standard views. Its state is
//! a value: each [`Nav`] gives the next ([`View3d::navigated`]), each change to the project the
//! scene built again when what it shows changed ([`View3d::refreshed`]).

pub mod labels;
pub mod shader;

use std::sync::Arc;

use emcraft_engine::Session;
use emcraft_gpu3d::Packed;
use emcraft_render::Rgb;
use emcraft_render::view3d::{Camera, StandardView};
use emcraft_ui_core::scene3d::{self, Colours, Overlay, Pick};
use emcraft_ui_core::theme::Theme;

/// A box (m): its low and high corners.
pub type Bounds = ([f64; 3], [f64; 3]);

/// What changes the view's camera or what it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Nav {
    /// Turned by points dragged across and down.
    Orbit(f32, f32),
    /// Moved with the picture by points dragged.
    Pan(f32, f32),
    /// Closer by steps (each 10 %).
    Zoom(f32),
    /// The model fitted in the view.
    Fit,
    /// Turned to a standard view, the model fitted.
    Standard(StandardView),
    /// What the view puts forward.
    Overlay(Overlay),
    /// The view's size (points), as laid out.
    Size(f32, f32),
}

/// A click on the view: where (points from its corner), whether Shift or Ctrl was held, whether
/// it was the second of a double click.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Click {
    pub at: emcraft_ui_core::geom::Point,
    pub add: bool,
    pub double: bool,
}

/// What the scene was built from: the window's generation of the project, the overlay, what is
/// selected (lit), the theme and the language its labels and legend are in.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    generation: u64,
    overlay: Overlay,
    selected: Vec<Pick>,
    theme: String,
    language: &'static str,
}

/// The scene as built (what a click picks from), packed for the GPU, the model's box, the labels
/// and the legend.
#[derive(Clone, Debug)]
pub struct Shown {
    key: Key,
    pub built: Arc<scene3d::Built>,
    pub packed: Arc<Packed>,
    pub bounds: Option<Bounds>,
    pub labels: Vec<scene3d::Label>,
    pub legend: Vec<(String, Rgb)>,
}

/// The 3D view's state.
#[derive(Clone, Debug)]
pub struct View3d {
    pub overlay: Overlay,
    pub camera: Camera,
    /// The box last fitted and the camera the fit left: a camera not moved since is fitted again
    /// when the view's shape changes.
    fitted: Option<(Bounds, Camera)>,
    /// The view's size (points).
    pub size: (f32, f32),
    pub shown: Option<Shown>,
}

impl Default for View3d {
    fn default() -> Self {
        View3d { overlay: Overlay::Plain, camera: Camera::default(), fitted: None, size: (800.0, 600.0), shown: None }
    }
}

/// The scene of `session` for `overlay` in `theme`'s colours, packed.
fn built(session: &Session, theme: &Theme, key: Key) -> Shown {
    let b = scene3d::build_with(session, key.overlay, &Colours::of(theme), &[], &key.selected);
    let packed = Packed::with_arrows(b.gpu_scene.as_ref().unwrap_or(&b.scene), emcraft_gpu3d::next_id(), None, &b.arrows);
    let bounds = b.bounds.or_else(|| b.scene.bounds());
    let (labels, legend) = (b.labels.clone(), b.legend.clone());
    Shown { key, built: Arc::new(b), packed: Arc::new(packed), bounds, labels, legend }
}

impl View3d {
    /// The view after the project or the theme changed: the scene built again when what it shows
    /// changed, a new box fitted.
    /// `generation`: the window's, raised by every command that ends well; `selected`: what is
    /// lit.
    pub fn refreshed(self, session: &Session, generation: u64, selected: &[Pick], theme: &Theme) -> View3d {
        let key = Key {
            generation,
            selected: selected.to_vec(),
            overlay: self.overlay,
            theme: theme.name.clone(),
            language: emcraft_ui_core::lang::current(),
        };
        if self.shown.as_ref().is_some_and(|s| s.key == key) {
            return self;
        }
        let shown = built(session, theme, key);
        let view = View3d { shown: Some(shown), ..self };
        match view.bounds() {
            Some(b) if view.fitted.is_none_or(|(f, _)| f != b) => view.fit(),
            _ => view,
        }
    }

    /// What is under `at` (points from the view's corner): the part its nearest triangle is.
    pub fn pick(&self, at: emcraft_ui_core::geom::Point) -> Option<Pick> {
        let shown = self.shown.as_ref()?;
        let (w, h) = (f64::from(self.size.0), f64::from(self.size.1));
        let pixel = emcraft_render::view3d::Pixel::new(f64::from(at.x), f64::from(at.y), w, h);
        shown.built.pick(&self.camera, pixel).and_then(Pick::from_tag)
    }

    /// The model's box, once there is one.
    pub fn bounds(&self) -> Option<Bounds> {
        self.shown.as_ref()?.bounds
    }

    /// The view's width over its height.
    fn aspect(&self) -> f64 {
        f64::from(self.size.0) / f64::from(self.size.1.max(1.0))
    }

    /// The camera fitted to the model's box (as it was without one).
    fn fit(self) -> View3d {
        let Some(b) = self.bounds() else { return self };
        let mut camera = self.camera;
        camera.fit(b.0, b.1, self.aspect());
        View3d { camera, fitted: Some((b, camera)), ..self }
    }

    /// The camera changed by `change`; the fit forgotten (a moved camera keeps its place when
    /// the view changes shape).
    fn moved(self, change: impl FnOnce(&mut Camera)) -> View3d {
        let mut camera = self.camera;
        change(&mut camera);
        View3d { camera, ..self }
    }

    /// The view after `nav`.
    ///
    /// ```
    /// use emcraft_ui_iced::view3d::{Nav, View3d};
    /// let v = View3d::default().navigated(Nav::Zoom(1.0));
    /// assert!(v.camera.distance < View3d::default().camera.distance);
    /// // Not a number: the camera stays.
    /// assert_eq!(v.clone().navigated(Nav::Orbit(f32::NAN, 0.0)).camera, v.camera);
    /// ```
    pub fn navigated(self, nav: Nav) -> View3d {
        let height = f64::from(self.size.1);
        match nav {
            Nav::Orbit(dx, dy) => self.moved(|c| c.orbit(f64::from(dx), f64::from(dy))),
            Nav::Pan(dx, dy) => self.moved(|c| c.pan(f64::from(dx), f64::from(dy), height)),
            Nav::Zoom(steps) => self.moved(|c| c.zoom(f64::from(steps))),
            Nav::Fit => self.fit(),
            Nav::Standard(v) => self.moved(|c| c.standard(v)).fit(),
            Nav::Overlay(overlay) => View3d { overlay, ..self },
            Nav::Size(w, h) if w.is_finite() && h.is_finite() && w >= 1.0 && h >= 1.0 => {
                let unmoved = self.fitted.is_some_and(|(_, c)| c == self.camera);
                let view = View3d { size: (w, h), ..self };
                if unmoved { view.fit() } else { view }
            }
            Nav::Size(..) => self,
        }
    }
}

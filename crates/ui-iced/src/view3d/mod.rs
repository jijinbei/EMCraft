//! The 3D view: the session's scene as the shared rules build it ([`scene3d::build`], in the
//! theme's colours) drawn by the GPU in iced's shader widget ([`shader`]), navigated by the mouse
//! mapping (orbit, pan, zoom), fitted to the model and turned to the standard views. Its state is
//! a value: each [`Nav`] gives the next ([`View3d::navigated`]), each change to the project the
//! scene built again when what it shows changed ([`View3d::refreshed`]). On the field overlay
//! the field view's plane is drawn in it, coloured by the field ([`field`]).

pub mod field;
pub mod labels;
pub mod shader;

use std::sync::Arc;

use emcraft_engine::Session;
use emcraft_engine::view::{Component, FieldView, Plane};
use emcraft_gpu3d::{FieldGrid, Packed};
use emcraft_render::Rgb;
use emcraft_render::colormap::ColourScale;
use emcraft_render::view3d::{Camera, StandardView};
use emcraft_ui_core::scene3d::{self, Colours, Cut, Overlay, Pick};
use emcraft_ui_core::theme::Theme;

/// Most triangles a click under a section looks through for one on the side kept.
const MOST_HITS: usize = 256;

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
    /// The field overlay's plane, component, frequency (Hz) and the plane's place along its
    /// normal (m from the model's low side).
    Plane(Plane),
    Component(Component),
    Frequency(f64),
    At(f64),
    /// The field's phase ωt (rad): the picture shows Re(F e^{jωt}).
    Phase(f64),
    /// The section view's cut, or none (the whole model).
    Section(Option<Cut>),
    /// The field's arrows shown: E's, H's.
    Arrows(bool, bool),
    /// An eigenmode's field (by its index) instead of the sweep's, or the sweep's.
    Mode(Option<usize>),
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
    /// The field view, on the field overlay (its phase left out when the GPU colours the plane:
    /// a phase then changes only the frame).
    field: Option<FieldView>,
    gpu_field: bool,
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
    /// The field's arrows: what each set shows and its scale (plain Unicode), in its colour.
    pub arrow_legend: Vec<(String, Rgb)>,
    /// The field's colour bar, on the field overlay.
    pub bar: Option<field::Bar>,
    /// The field plane's phasors and colour scale when the GPU colours it.
    pub field: Option<(Arc<FieldGrid>, ColourScale)>,
}

/// What the scene is built from besides the view's own state: the project and the window's
/// generation of it (raised by every command that ends well), what is selected (lit in the scene
/// when `lit`, else ringed by the GPU), the theme, and whether the GPU colours the field's plane
/// (the preferences').
pub struct Source<'a> {
    pub session: &'a Session,
    pub generation: u64,
    pub selected: &'a [Pick],
    pub theme: &'a Theme,
    pub gpu_field: bool,
    pub lit: bool,
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
    /// What the field overlay shows, its plane's samples and why it could not be drawn.
    pub field: FieldView,
    /// The section view's cut: the GPU clips the scene there.
    pub section: Option<Cut>,
    pub(crate) sampled: Option<field::Sampled>,
    /// The plane's phasors as uploaded, for the generation and the view they were made from.
    grid: Option<((u64, FieldView), Arc<FieldGrid>)>,
    pub field_error: Option<String>,
}

impl Default for View3d {
    fn default() -> Self {
        View3d {
            overlay: Overlay::Plain,
            camera: Camera::default(),
            fitted: None,
            size: (800.0, 600.0),
            shown: None,
            field: field::start(),
            section: None,
            sampled: None,
            grid: None,
            field_error: None,
        }
    }
}

/// The field's arrows for the GPU to turn with the phase, and their legend.
type Arrows = (Vec<scene3d::Arrow>, Vec<(String, Rgb)>);

/// The scene of `session` for `key` in `theme`'s colours with the field's `layers`, packed, its
/// plane marked for the GPU to colour from `grid` when it does (`view` its scale's), the field's
/// `arrows` with it.
fn built(
    session: &Session,
    theme: &Theme,
    key: Key,
    layers: &[scene3d::FieldLayer],
    grid: Option<(Arc<FieldGrid>, &FieldView)>,
    arrows: Arrows,
) -> Shown {
    let b = scene3d::build_with(session, key.overlay, &Colours::of(theme), layers, &key.selected);
    let (more, said) = arrows;
    let b =
        scene3d::Built { arrows: b.arrows.into_iter().chain(more).collect(), arrow_legend: b.arrow_legend.into_iter().chain(said).collect(), ..b };
    let mark = grid.as_ref().map(|(g, _)| g.mark());
    let packed = Packed::with_arrows(b.gpu_scene.as_ref().unwrap_or(&b.scene), emcraft_gpu3d::next_id(), mark, &b.arrows);
    let bounds = b.bounds.or_else(|| b.scene.bounds());
    let (labels, bar) = (b.labels.clone(), b.colour_bar.as_ref().map(field::Bar::of));
    let legend = b.legend.clone();
    let arrow_legend = b.arrow_legend.iter().map(|(s, c)| (emcraft_render::math::to_unicode(s), *c)).collect();
    // The scale the colour bar shows, so the plane and the bar agree.
    let field = grid.map(|(g, v)| {
        let scale = b.colour_bar.as_ref().map_or_else(|| g.scale(v), |c| c.scale);
        (g, scale)
    });
    Shown { key, built: Arc::new(b), packed: Arc::new(packed), bounds, labels, legend, arrow_legend, bar, field }
}

impl View3d {
    /// The view after the project or the theme changed: the scene built again when what it shows
    /// changed, a new box fitted.
    pub fn refreshed(self, src: &Source) -> View3d {
        let Source { session, generation, selected, theme, gpu_field, lit } = *src;
        let selected = if lit { selected } else { &[] };
        let shown_field = |v: &FieldView| if gpu_field { FieldView { phase: 0.0, ..v.clone() } } else { v.clone() };
        let key = Key {
            generation,
            selected: selected.to_vec(),
            overlay: self.overlay,
            field: (self.overlay == Overlay::Field).then(|| shown_field(&self.field)),
            gpu_field,
            theme: theme.name.clone(),
            language: emcraft_ui_core::lang::current(),
        };
        if self.shown.as_ref().is_some_and(|s| s.key == key) {
            return self;
        }
        let (sampled, plot) = match key.field.is_some() {
            true => {
                let (sampled, plot) = field::plot(session, generation, &self.field, self.sampled.clone());
                (sampled, Some(plot))
            }
            false => (self.sampled.clone(), None),
        };
        let place = sampled.as_ref().map(field::Sampled::place);
        // The plane's phasors for the GPU: made again only for new samples or another component.
        let lo = session.model().bounds().map(|b| [b.min.x, b.min.y, b.min.z]);
        let grid_key = (generation, FieldView { phase: 0.0, db: None, contours: false, ..self.field.clone() });
        let grid = match (gpu_field, &plot, &sampled, lo) {
            (true, Some(Ok(_)), Some(s), Some(lo)) => Some(match &self.grid {
                Some((k, g)) if *k == grid_key => g.clone(),
                _ => Arc::new(FieldGrid::of(session, s.samples(), &self.field, lo, emcraft_gpu3d::next_id())),
            }),
            _ => None,
        };
        let layers: Vec<scene3d::FieldLayer> = match (&plot, place) {
            (Some(Ok(plot)), Some((plane, at))) => vec![scene3d::FieldLayer { plot, plane, at, gpu: grid.is_some() }],
            _ => Vec::new(),
        };
        // With the GPU's colouring its arrows turn on the GPU too: their phasors go with the scene.
        let arrows = match (&grid, &sampled, lo) {
            (Some(_), Some(s), Some(lo)) => scene3d::phase_arrows(session, s.samples(), &self.field, lo, &Colours::of(theme)),
            _ => (Vec::new(), Vec::new()),
        };
        let shown = built(session, theme, key, &layers, grid.clone().map(|g| (g, &self.field)), arrows);
        let field_error = plot.and_then(Result::err);
        let grid = grid.map(|g| (grid_key, g)).or_else(|| self.grid.clone());
        let view = View3d { shown: Some(shown), sampled, grid, field_error, ..self };
        match view.bounds() {
            Some(b) if view.fitted.is_none_or(|(f, _)| f != b) => view.fit(),
            _ => view,
        }
    }

    /// What is under `at` (points from the view's corner): the part its nearest triangle is
    /// (the nearest solid one first); with a section, the nearest on the side kept.
    pub fn pick(&self, at: emcraft_ui_core::geom::Point) -> Option<Pick> {
        let shown = self.shown.as_ref()?;
        let (w, h) = (f64::from(self.size.0), f64::from(self.size.1));
        let pixel = emcraft_render::view3d::Pixel::new(f64::from(at.x), f64::from(at.y), w, h);
        let tag = match self.section {
            None => shown.built.pick(&self.camera, pixel),
            Some(cut) => {
                let scene = &shown.built.scene;
                let kept: Vec<_> = emcraft_render::view3d::hits_in_order(scene, self.camera.ray(pixel)?, MOST_HITS)
                    .into_iter()
                    .filter(|h| cut.keeps(h.at))
                    .collect();
                let solid = kept.iter().find(|h| scene.tris.get(h.index).is_some_and(|t| t.alpha >= 200));
                solid.or_else(|| kept.first()).map(|h| h.tag)
            }
        };
        tag.and_then(Pick::from_tag)
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
            Nav::Plane(plane) => View3d { field: FieldView { plane, at: None, ..self.field.clone() }, ..self },
            // The arrows follow the field coloured: E's for an electric component, H's for a
            // magnetic one.
            Nav::Component(component) => {
                let (show_e, show_h) = (!component.is_magnetic(), component.is_magnetic());
                View3d { field: FieldView { component, show_e, show_h, ..self.field.clone() }, ..self }
            }
            Nav::Mode(mode) => View3d { field: FieldView { mode, ..self.field.clone() }, ..self },
            Nav::Arrows(show_e, show_h) => View3d { field: FieldView { show_e, show_h, ..self.field.clone() }, ..self },
            Nav::Frequency(frequency) => View3d { field: FieldView { frequency, ..self.field.clone() }, ..self },
            Nav::At(at) => View3d { field: FieldView { at: Some(at), ..self.field.clone() }, ..self },
            Nav::Phase(phase) if phase.is_finite() => {
                View3d { field: FieldView { phase: phase.rem_euclid(std::f64::consts::TAU), ..self.field.clone() }, ..self }
            }
            Nav::Phase(_) => self,
            Nav::Section(cut) => View3d { section: cut.filter(|c| c.at.is_finite()), ..self },
            Nav::Size(w, h) if w.is_finite() && h.is_finite() && w >= 1.0 && h >= 1.0 => {
                let unmoved = self.fitted.is_some_and(|(_, c)| c == self.camera);
                let view = View3d { size: (w, h), ..self };
                if unmoved { view.fit() } else { view }
            }
            Nav::Size(..) => self,
        }
    }
}

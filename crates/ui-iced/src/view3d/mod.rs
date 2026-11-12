//! The 3D view: the session's scene as the shared rules build it ([`scene3d::build`], in the
//! theme's colours) drawn by the GPU in iced's shader widget ([`shader`]), navigated by the mouse
//! mapping (orbit, pan, zoom), fitted to the model and turned to the standard views. Its state is
//! a value: each [`Nav`] gives the next ([`View3d::navigated`]), each change to the project the
//! scene built again when what it shows changed ([`View3d::refreshed`]). View changes (fit, the
//! standard views, the arrow keys' turns) are animated by the frames' clock ([`Nav::Tick`]), as
//! the egui window's; a held arrow turns the view steadily. On the field overlay the field view's
//! plane is drawn in it, coloured by the field ([`field`]).

pub mod field;
pub mod labels;
pub mod marks;
pub mod navigator;
pub mod shader;

use std::sync::Arc;

use emcraft_engine::Session;
use emcraft_engine::view::{Component, FieldView, Plane};
use emcraft_gpu3d::{FieldGrid, Packed};
use emcraft_render::Rgb;
use emcraft_render::colormap::ColourScale;
use emcraft_render::view3d::{Camera, CameraAnim};
use emcraft_ui_core::navigation::{Goal, HOLD_DEGREES_PER_SECOND, KEY_PAN_SHARE, KEY_STEP_DEGREES, TAP_SECONDS, zoom_limits};
use emcraft_ui_core::scene3d::{self, Colours, Cut, Overlay, Pick};
use emcraft_ui_core::selection::BoxSel;
use emcraft_ui_core::theme::Theme;

/// Most triangles a click under a section looks through for one on the side kept.
const MOST_HITS: usize = 256;
/// A zoom step's share of the camera's distance, in e's power (as [`Camera::zoom`]'s).
const ZOOM_STEP: f64 = -0.1;

pub use emcraft_ui_core::navigation::Bounds;

/// What changes the view's camera or what it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Nav {
    /// Turned by points dragged across and down.
    Orbit(f32, f32),
    /// Moved with the picture by points dragged.
    Pan(f32, f32),
    /// Closer by steps (each 10 %).
    Zoom(f32),
    /// Closer by steps toward the model's point under the pointer (points from the view's
    /// corner), within the model's limits.
    ZoomAt(f32, emcraft_ui_core::geom::Point),
    /// A view change, animated from where one under way is going.
    Goal(Goal),
    /// An arrow pressed: the view turned toward the screen's right and up (Shift: a quarter
    /// turn), and turning steadily while held; with Ctrl the picture moved a tenth of the view.
    Arrow {
        right: f64,
        up: f64,
        shift: bool,
        ctrl: bool,
    },
    /// The arrows let go.
    Released,
    /// A frame's time: the view change under way and the held arrow go on.
    Tick(std::time::Instant),
    /// The selection box being dragged, or none.
    Box(Option<BoxSel>),
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
    /// The section's dimensions and port marks shown, or not.
    Dims(bool),
    /// The camera turned straight at the cut, orthographic, its outline framed.
    LookAtCut,
    /// The field's arrows shown: E's, H's.
    Arrows(bool, bool),
    /// An eigenmode's field (by its index) instead of the sweep's, or the sweep's.
    Mode(Option<usize>),
}

/// A grip dragged (a body's, or the selected port's reference plane's): pressed where (points
/// from the view's corner), moved by points, let go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GripMove {
    Start(emcraft_ui_core::geom::Point),
    By(f32, f32),
    End,
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

/// An arrow held: its turn, whether Shift doubled it, when it was pressed and the frame last
/// turned at (seconds on the view's clock).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hold {
    right: f64,
    up: f64,
    shift: bool,
    since: f64,
    last: f64,
}

/// The 3D view's state.
#[derive(Clone, Debug)]
pub struct View3d {
    pub overlay: Overlay,
    pub camera: Camera,
    /// The view change asked for (begun with the next frame) and the one under way.
    goal: Option<Goal>,
    anim: Option<CameraAnim>,
    /// The clock's start: view changes are timed in seconds from it.
    epoch: std::time::Instant,
    hold: Option<Hold>,
    /// The selection box while it is dragged.
    pub boxsel: Option<BoxSel>,
    /// The box last fitted and the camera the fit left: a camera not moved since is fitted again
    /// when the view's shape changes.
    fitted: Option<(Bounds, Camera)>,
    /// The view's size (points).
    pub size: (f32, f32),
    pub shown: Option<Shown>,
    /// What the field overlay shows, its plane's samples and why it could not be drawn.
    pub field: FieldView,
    /// The section view's cut: the GPU clips the scene there; whether its dimensions show.
    pub section: Option<Cut>,
    pub section_dims: bool,
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
            goal: None,
            anim: None,
            epoch: std::time::Instant::now(),
            hold: None,
            boxsel: None,
            fitted: None,
            size: (800.0, 600.0),
            shown: None,
            field: field::start(),
            section: None,
            section_dims: true,
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

    /// The parts the box `b` (points over the view) takes.
    pub fn boxed(&self, b: BoxSel) -> Vec<Pick> {
        let (w, h) = (f64::from(self.size.0), f64::from(self.size.1));
        self.shown.as_ref().map(|s| emcraft_ui_core::selection::boxed(&s.built.scene, &self.camera, w, h, b)).unwrap_or_default()
    }

    /// The cut's outline (m): what lies on its plane, else the model's box flattened onto it.
    pub fn cut_outline(&self) -> Option<Bounds> {
        let (cut, b) = (self.section?, self.bounds()?);
        Some(match &self.shown {
            Some(s) => emcraft_ui_core::scene3d::section::outline(&s.built.scene, cut, b),
            None => emcraft_ui_core::scene3d::section::flat(cut, b),
        })
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
        View3d { camera, fitted: Some((b, camera)), goal: None, anim: None, ..self }
    }

    /// Whether the view changes by itself: a change asked for or under way, an arrow held (the
    /// window then draws every frame).
    pub fn animating(&self) -> bool {
        self.goal.is_some() || self.anim.is_some() || self.hold.is_some()
    }

    /// Seconds on the view's clock at `now`.
    fn seconds(&self, now: std::time::Instant) -> f64 {
        now.saturating_duration_since(self.epoch).as_secs_f64()
    }

    /// The camera a view change starts from: where one under way is going, so quick clicks and
    /// key taps add up.
    fn heading(&self) -> Camera {
        self.anim.map_or(self.camera, |a| a.to)
    }

    /// The view at `now`: the change asked for begun (a fit remembered as the fit), the one
    /// under way followed, a held arrow turning past a tap.
    fn ticked(self, now: f64) -> View3d {
        let aspect = self.aspect();
        let view = match (self.goal, self.bounds()) {
            (Some(goal), Some(b)) => {
                let to = goal.camera(self.heading(), b, aspect);
                let fitted = matches!(goal, Goal::Fit | Goal::Home).then_some((b, to)).or(self.fitted);
                let anim = (to != self.camera).then(|| CameraAnim::new(self.camera, to, now));
                View3d { goal: None, anim, fitted, ..self }
            }
            (Some(_), None) => View3d { goal: None, ..self },
            (None, _) => self,
        };
        let view = match view.anim.map(|a| a.sample(now)) {
            Some((camera, done)) => View3d { camera, anim: (!done).then_some(view.anim).flatten(), ..view },
            None => view,
        };
        match view.hold {
            Some(h) if now - h.since >= TAP_SECONDS => {
                let rate = if h.shift { 2.0 * HOLD_DEGREES_PER_SECOND } else { HOLD_DEGREES_PER_SECOND };
                let degrees = rate * (now - h.last).clamp(0.0, 0.1);
                let camera = view.camera.stepped(h.right, h.up, degrees);
                View3d { camera, anim: None, hold: Some(Hold { last: now, ..h }), ..view }
            }
            _ => view,
        }
    }

    /// The model point under `at` (points from the view's corner): the surface there, else where
    /// its ray meets the plane through the target facing the camera.
    fn anchor(&self, at: emcraft_ui_core::geom::Point) -> Option<[f64; 3]> {
        let pixel = emcraft_render::view3d::Pixel::new(f64::from(at.x), f64::from(at.y), f64::from(self.size.0), f64::from(self.size.1));
        self.shown
            .as_ref()
            .and_then(|s| emcraft_render::view3d::surface_at(&s.built.scene, &self.camera, pixel))
            .or_else(|| self.camera.on_target_plane(pixel))
    }

    /// The camera changed by `change`, a view change under way stopped where it is; the fit
    /// forgotten (a moved camera keeps its place when the view changes shape).
    fn moved(self, change: impl FnOnce(&mut Camera)) -> View3d {
        let mut camera = self.camera;
        change(&mut camera);
        View3d { camera, goal: None, anim: None, ..self }
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
            Nav::ZoomAt(steps, at) => {
                let (anchor, limits) = (self.anchor(at), self.bounds().map_or([1e-4, 1e3], zoom_limits));
                self.moved(|c| c.zoom_at(anchor, (ZOOM_STEP * f64::from(steps).clamp(-100.0, 100.0)).exp(), limits))
            }
            Nav::Goal(goal) => View3d { goal: Some(goal), ..self },
            Nav::Arrow { right, up, ctrl: true, .. } => {
                let step = height * f64::from(KEY_PAN_SHARE);
                self.moved(|c| c.pan(right * step, -up * step, height))
            }
            Nav::Arrow { right, up, shift, ctrl: false } => {
                let goal = Goal::Turn { right, up, degrees: KEY_STEP_DEGREES[usize::from(shift)] };
                // The press's time is the next frame's: a hold counts from then.
                View3d { goal: Some(goal), hold: Some(Hold { right, up, shift, since: f64::INFINITY, last: f64::INFINITY }), ..self }
            }
            Nav::Released => View3d { hold: None, ..self },
            Nav::Tick(now) => {
                let now = self.seconds(now);
                let hold = self.hold.map(|h| if h.since.is_finite() { h } else { Hold { since: now, last: now, ..h } });
                View3d { hold, ..self }.ticked(now)
            }
            Nav::Box(b) => View3d { boxsel: b.filter(|b| b.rect.is_finite()), ..self },
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
            Nav::Dims(on) => View3d { section_dims: on, ..self },
            Nav::LookAtCut => {
                let goal = self.section.zip(self.cut_outline()).and_then(|(c, o)| emcraft_ui_core::scene3d::section::look(c, o));
                let camera = Camera { projection: emcraft_render::view3d::Projection::Orthographic, ..self.camera };
                match goal {
                    Some(goal) => View3d { camera, goal: Some(goal), ..self },
                    None => self,
                }
            }
            Nav::Size(w, h) if w.is_finite() && h.is_finite() && w >= 1.0 && h >= 1.0 => {
                let unmoved = self.fitted.is_some_and(|(_, c)| c == self.camera);
                let view = View3d { size: (w, h), ..self };
                if unmoved { view.fit() } else { view }
            }
            Nav::Size(..) => self,
        }
    }
}

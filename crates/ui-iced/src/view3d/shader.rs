//! The 3D view in iced's shader widget: the scene's frame drawn by [`emcraft_gpu3d`] on the
//! window's own device (offscreen in `prepare`, shown in the widget's place in `draw`), and the
//! pointer's drags and the wheel read as the mouse mapping says ([`NavPreset`]), each reported as
//! a [`Nav`] for the window to apply: Ctrl+left drag orbits in every mapping, a left drag the
//! mapping leaves free draws a selection box (Shift adds to the selection), the wheel zooms
//! toward the pointer.

use std::sync::Arc;

use emcraft_gpu3d::{Caps, Frame, Gpu3dError, Packed, Renderer, Uniforms};
use emcraft_render::Rgb;
use emcraft_render::view3d::Camera;
use emcraft_ui_core::navigation::{NavButton, NavKeys, NavMode, NavPreset};
use emcraft_ui_core::selection::BoxSel;
use iced::widget::shader::{self, Action, Viewport};
use iced::{Point, Rectangle, Size, keyboard, mouse};

use super::{Click, GripMove, Nav};
use crate::app::Message;

/// Supersampled at rest up to this many output pixels.
const SUPERSAMPLE_PIXELS: f64 = 1.3e6;

/// Zoom steps a point dragged with the zoom mode makes, and a pixel scrolled.
const DRAG_ZOOM: f32 = 0.05;
const PIXEL_ZOOM: f32 = 1.0 / 40.0;

/// What the view draws: the scene packed, through the camera, on its ground (top, bottom), with
/// the per-pixel drawing asked for; whether it is moving (dragged).
#[derive(Clone, Debug)]
pub struct Shot {
    pub packed: Arc<Packed>,
    pub camera: Camera,
    pub ground: (Rgb, Rgb),
    pub look: emcraft_gpu3d::Look,
    /// The field's plane for the GPU to colour, with this frame's phase.
    pub field: Option<emcraft_gpu3d::FieldFrame>,
    /// The section view's cut, which the GPU clips the scene at.
    pub cut: Option<emcraft_ui_core::scene3d::Cut>,
    /// The tags the GPU rings (what is selected) and the ring's colour (0–1).
    pub selected: Vec<u32>,
    pub outline: [f32; 4],
    pub moving: bool,
}

/// The frame of `shot` for a view `out` pixels big at `ppp` pixels a point, supersampled 2× at
/// rest when small enough.
///
/// ```
/// # use std::sync::Arc;
/// use emcraft_ui_iced::view3d::shader::{Shot, frame};
/// let packed = Arc::new(emcraft_gpu3d::Packed::new(&emcraft_render::view3d::Scene::default(), 1));
/// let shot = Shot { packed, camera: Default::default(), ground: Default::default(), look: Default::default(), field: None, cut: None, selected: Vec::new(), outline: [0.0; 4], moving: false };
/// let f = frame(&shot, [800, 600], 1.0);
/// assert_eq!((f.out, f.size, f.supersample), ([800, 600], [1600, 1200], 2));
/// // Moving, the picture is drawn at its own size.
/// assert_eq!(frame(&Shot { moving: true, ..shot }, [800, 600], 1.0).supersample, 1);
/// ```
pub fn frame(shot: &Shot, out: [u32; 2], ppp: f32) -> Frame {
    let out = out.map(|s| s.clamp(1, 4096));
    let small = f64::from(out[0]) * f64::from(out[1]) <= SUPERSAMPLE_PIXELS && out[0].max(out[1]) <= 2048;
    let supersample = if !shot.moving && small { 2 } else { 1 };
    let size = out.map(|s| s * supersample);
    let line_width = f64::from(ppp.max(1.0)) * f64::from(supersample);
    let plain = Uniforms::new(&shot.packed, &shot.camera, size, line_width, shot.ground);
    let uniforms = match shot.cut {
        Some(c) => Uniforms { clip: c.clip(shot.packed.origin), ..plain },
        None => plain,
    };
    Frame {
        look: shot.look,
        ppp,
        field: shot.field.clone(),
        selected: shot.selected.clone(),
        outline: shot.outline,
        ..Frame::plain(shot.packed.clone(), uniforms, size, out, supersample)
    }
}

/// The view's program: what it draws, the mouse mapping its drags follow, and whether the
/// pointer's place is reported (over the field's plane, for its values).
pub struct Viewer {
    pub shot: Shot,
    pub nav: &'static NavPreset,
    pub hover: bool,
    /// A tool is on: a free left drag draws no box.
    pub tool: bool,
    /// The grips a plain left press takes (points from the view's corner): the selected body's,
    /// the selected port's reference plane's.
    pub grips: Vec<Point>,
    /// What an orbit turns about (the preference).
    pub around: emcraft_ui_core::navigation::OrbitAround,
    /// The scene, while the field's plane in it may be dragged (the field overlay).
    pub plane: Option<Arc<emcraft_ui_core::scene3d::Built>>,
}

/// The pointer between events: the drag in progress (its mode and where it was last), where the
/// left button went down, the last click (when and where, for a double click), the modifier keys
/// held and the view's size as last reported.
#[derive(Default)]
pub struct Grip {
    drag: Option<(NavMode, Point)>,
    /// The drag's orbit snaps with Shift (Shift was not needed to start it, and the mapping
    /// snaps).
    snaps: bool,
    /// The field's plane dragged: where the pointer was last.
    planed: Option<Point>,
    /// The touchpad mapping's move with no button down: where the pointer was last.
    motion: Option<Point>,
    /// A selection box: where it began, whether it adds to the selection, whether it has left
    /// the click's slop.
    boxing: Option<(Point, bool, bool)>,
    /// The reference plane's grip dragged: where the pointer was last.
    gripped: Option<Point>,
    pressed: Option<Point>,
    clicked: Option<(std::time::Instant, Point)>,
    keys: NavKeys,
    size: Option<Size>,
    /// The pointer was last reported over the view.
    over: bool,
}

/// How far the pointer may move between press and release for a click, and between two clicks
/// of a double click (points); how soon the second must follow.
const CLICK_SLOP: f32 = 4.0;
const DOUBLE_SLOP: f32 = 6.0;
const DOUBLE_WITHIN: std::time::Duration = std::time::Duration::from_millis(400);

fn button(b: mouse::Button) -> Option<NavButton> {
    match b {
        mouse::Button::Left => Some(NavButton::Left),
        mouse::Button::Middle => Some(NavButton::Middle),
        mouse::Button::Right => Some(NavButton::Right),
        _ => None,
    }
}

fn held(m: keyboard::Modifiers) -> NavKeys {
    NavKeys { shift: m.shift(), ctrl: m.command(), alt: m.alt() }
}

/// What a left drag pressed with `k` held does in `nav`: the mapping's navigation, else Ctrl
/// orbits.
fn left_mode(nav: &NavPreset, k: NavKeys) -> Option<NavMode> {
    nav.mode(NavButton::Left, k).or_else(|| (k.ctrl && !k.shift && !k.alt).then_some(NavMode::Orbit))
}

impl Viewer {
    /// Whether the field's plane is under `at` (the field overlay).
    fn on_plane(&self, at: Point, bounds: Rectangle) -> bool {
        self.plane.as_ref().is_some_and(|built| {
            let pixel = emcraft_render::view3d::Pixel::new(f64::from(at.x), f64::from(at.y), f64::from(bounds.width), f64::from(bounds.height));
            built.pick(&self.shot.camera, pixel).and_then(emcraft_ui_core::scene3d::Pick::from_tag) == Some(emcraft_ui_core::scene3d::Pick::Plane(0))
        })
    }
}

/// What a drag by `d` (points) in `mode` does to the camera.
fn dragged(mode: NavMode, d: iced::Vector) -> Nav {
    match mode {
        NavMode::Orbit => Nav::Orbit(d.x, d.y),
        NavMode::Pan => Nav::Pan(d.x, d.y),
        NavMode::Zoom => Nav::Zoom(-d.y * DRAG_ZOOM),
    }
}

/// The click at `at`, a double click when the last one was near and just before; the click kept
/// for the next (a double click's second is not the first of another).
fn clicked(grip: &mut Grip, at: Point) -> Click {
    let now = std::time::Instant::now();
    let double = grip.clicked.is_some_and(|(t, p)| now.duration_since(t) <= DOUBLE_WITHIN && p.distance(at) <= DOUBLE_SLOP);
    grip.clicked = (!double).then_some((now, at));
    Click { at: crate::geom::shared(at), add: grip.keys.shift || grip.keys.ctrl, double }
}

fn publish(nav: Nav) -> Option<Action<Message>> {
    Some(Action::publish(Message::View3d(nav)).and_capture())
}

impl shader::Program<Message> for Viewer {
    type State = Grip;
    type Primitive = Shot;

    fn update(&self, grip: &mut Grip, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<Action<Message>> {
        // The view's size first, whatever the event: the camera fits its shape.
        if grip.size != Some(bounds.size()) {
            grip.size = Some(bounds.size());
            return Some(Action::publish(Message::View3d(Nav::Size(bounds.width, bounds.height))));
        }
        match event {
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                grip.keys = held(*m);
                None
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(b)) => {
                let at = cursor.position_in(bounds)?;
                let left = *b == mouse::Button::Left;
                let plain = !grip.keys.shift && !grip.keys.ctrl && !grip.keys.alt;
                if left && plain && self.grips.iter().any(|g| g.distance(at) <= emcraft_ui_core::tools::GRIP_PX) {
                    grip.gripped = Some(at);
                    return Some(Action::publish(Message::Grip(GripMove::Start(crate::geom::shared(at)))).and_capture());
                }
                if left && plain && self.on_plane(at, bounds) {
                    grip.planed = Some(at);
                    return Some(Action::capture());
                }
                grip.pressed = left.then_some(at);
                let mode_with = |k: NavKeys| match button(*b) {
                    Some(NavButton::Left) => left_mode(self.nav, k),
                    Some(other) => self.nav.mode(other, k),
                    None => None,
                };
                let mode = mode_with(grip.keys);
                grip.snaps = mode_with(NavKeys { shift: false, ..grip.keys }) == mode && self.nav.id != emcraft_ui_core::navigation::OPENSCAD;
                grip.drag = mode.map(|mode| (mode, at));
                grip.boxing = (left && grip.drag.is_none() && !self.tool).then_some((at, grip.keys.shift, false));
                match mode {
                    // The orbit's pivot is chosen where it begins.
                    Some(NavMode::Orbit) => {
                        Some(Action::publish(Message::View3d(Nav::OrbitFrom(crate::geom::shared(at), self.around))).and_capture())
                    }
                    _ => Some(Action::capture()),
                }
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) if grip.planed.is_some() => {
                let last = grip.planed?;
                let at = *position - iced::Vector::new(bounds.x, bounds.y);
                grip.planed = Some(at);
                Some(Action::publish(Message::View3d(Nav::DragPlane(at.x - last.x, at.y - last.y))).and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if grip.planed.is_some() => {
                grip.planed = None;
                Some(Action::request_redraw().and_capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) if grip.gripped.is_some() => {
                let last = grip.gripped?;
                let at = *position - iced::Vector::new(bounds.x, bounds.y);
                grip.gripped = Some(at);
                Some(Action::publish(Message::Grip(GripMove::By(at.x - last.x, at.y - last.y))).and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if grip.gripped.is_some() => {
                grip.gripped = None;
                Some(Action::publish(Message::Grip(GripMove::End)).and_capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) if grip.boxing.is_some() => {
                let (start, add, out) = grip.boxing?;
                let at = *position - iced::Vector::new(bounds.x, bounds.y);
                let out = out || start.distance(at) > CLICK_SLOP;
                grip.boxing = Some((start, add, out));
                out.then(|| Action::publish(Message::View3d(Nav::Box(Some(BoxSel::from_drag(crate::geom::shared(start), crate::geom::shared(at)))))))
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) if grip.drag.is_none() && self.nav.motion_mode(grip.keys).is_some() => {
                // The touchpad mapping: Shift or Alt held, the pointer's move pans or orbits.
                let (mode, at) = self.nav.motion_mode(grip.keys).zip(cursor.position_in(bounds))?;
                let last = grip.motion.replace(at);
                let d = last.map_or(iced::Vector::ZERO, |l| at - l);
                (d != iced::Vector::ZERO && d.x.hypot(d.y) <= 1e4).then(|| Action::publish(Message::View3d(dragged(mode, d))))
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) if grip.drag.is_none() => {
                grip.motion = None;
                // Not dragging: where the pointer is, while asked and inside; once that it left.
                let at = cursor.position_in(bounds).filter(|_| self.hover);
                let was = std::mem::replace(&mut grip.over, at.is_some());
                (at.is_some() || was).then(|| Action::publish(Message::Hover(at.map(crate::geom::shared))))
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let (mode, last) = grip.drag?;
                let at = *position - iced::Vector::new(bounds.x, bounds.y);
                grip.drag = Some((mode, at));
                let d = at - last;
                match mode {
                    NavMode::Orbit if grip.snaps && grip.keys.shift => publish(Nav::OrbitSnapped(d.x, d.y)),
                    _ => publish(dragged(mode, d)),
                }
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if grip.boxing.is_some_and(|(_, _, out)| out) => {
                let (_, add, _) = grip.boxing.take()?;
                grip.pressed = None;
                Some(Action::publish(Message::Boxed(add)).and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(b)) => {
                grip.drag = None;
                grip.boxing = None;
                let at = cursor.position_in(bounds);
                let pressed = grip.pressed.take().filter(|_| *b == mouse::Button::Left);
                match (pressed, at) {
                    (Some(p), Some(at)) if p.distance(at) <= CLICK_SLOP => Some(Action::publish(Message::Click(clicked(grip, at))).and_capture()),
                    _ => Some(Action::request_redraw()),
                }
            }
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let at = cursor.position_in(bounds)?;
                let zoom = |steps: f32| publish(Nav::ZoomAt(if self.nav.reverse_zoom { -steps } else { steps }, crate::geom::shared(at)));
                let k = grip.keys;
                match delta {
                    mouse::ScrollDelta::Lines { y, .. } => zoom(*y),
                    // A trackpad's two-finger scroll pans (with Shift orbits, where the mapping
                    // says so); with Ctrl, or in the touchpad mapping, it zooms.
                    mouse::ScrollDelta::Pixels { y, .. } if k.ctrl || !self.nav.scroll_pans() => zoom(y * PIXEL_ZOOM),
                    mouse::ScrollDelta::Pixels { x, y } if k.shift && self.nav.shift_scroll_orbits() => publish(Nav::Orbit(*x, *y)),
                    mouse::ScrollDelta::Pixels { x, y } => publish(Nav::Pan(*x, *y)),
                }
            }
            _ => None,
        }
    }

    fn draw(&self, grip: &Grip, _cursor: mouse::Cursor, _bounds: Rectangle) -> Shot {
        Shot { moving: grip.drag.is_some(), ..self.shot.clone() }
    }

    fn mouse_interaction(&self, grip: &Grip, _bounds: Rectangle, _cursor: mouse::Cursor) -> mouse::Interaction {
        match grip.drag {
            None if grip.gripped.is_some() || grip.planed.is_some() => mouse::Interaction::Grabbing,
            None if grip.boxing.is_some() => mouse::Interaction::Crosshair,
            Some((NavMode::Pan, _)) => mouse::Interaction::Grabbing,
            Some(_) => mouse::Interaction::Move,
            None => mouse::Interaction::default(),
        }
    }
}

/// The renderer on the window's device, made with the first frame; or why it could not be, or
/// why it stopped (the view then shows nothing more).
pub struct Gpu {
    renderer: Result<Renderer, Gpu3dError>,
}

impl shader::Pipeline for Gpu {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Gpu {
        let renderer = Renderer::new(device, Caps::guaranteed(device), format);
        if let Err(e) = &renderer {
            eprintln!("emcraft: {e}");
        }
        Gpu { renderer }
    }
}

use emcraft_gpu3d::wgpu;

impl shader::Primitive for Shot {
    type Pipeline = Gpu;

    fn prepare(&self, gpu: &mut Gpu, device: &wgpu::Device, queue: &wgpu::Queue, bounds: &Rectangle, viewport: &Viewport) {
        let Ok(r) = gpu.renderer.as_mut() else { return };
        let ppp = viewport.scale_factor();
        let out = [bounds.width, bounds.height].map(|s| (s * ppp).round().max(1.0) as u32);
        let f = frame(self, out, ppp);
        // Its own encoder, submitted before the window's: the frame is drawn when shown.
        let drawn = emcraft_gpu3d::scoped(device, || {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("emcraft_view3d") });
            r.prepare(device, queue, &mut encoder, &f).map(|()| queue.submit([encoder.finish()]))
        });
        if let Err(e) = drawn.map_err(Gpu3dError::Frame).and_then(|r| r.map(|_| ())) {
            eprintln!("emcraft: {e}; the 3D view stops");
            gpu.renderer = Err(e);
        }
    }

    fn draw(&self, gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) -> bool {
        if let Ok(r) = &gpu.renderer {
            r.show(pass);
        }
        true
    }
}

/// The view's keys (when no field takes them): the arrows turn the view
/// 15° (Shift: a quarter turn) and keep turning while held, Ctrl+arrows move the picture, and
/// Ctrl+1…7 turn to Front, Back, Left, Right, Top, Bottom and the isometric view.
pub fn keys(event: iced::Event, status: iced::event::Status, _: iced::window::Id) -> Option<Message> {
    use emcraft_ui_core::navigation::{DIGIT_VIEWS, Goal};
    use keyboard::key::Named;
    use keyboard::{Event, Key};
    let arrow = |k: &Key| match k {
        Key::Named(Named::ArrowLeft) => Some((-1.0, 0.0)),
        Key::Named(Named::ArrowRight) => Some((1.0, 0.0)),
        Key::Named(Named::ArrowUp) => Some((0.0, 1.0)),
        Key::Named(Named::ArrowDown) => Some((0.0, -1.0)),
        _ => None,
    };
    let iced::Event::Keyboard(event) = event else { return None };
    match event {
        Event::KeyReleased { key, .. } if arrow(&key).is_some() => Some(Message::Key3d(Nav::Released)),
        // Escape whoever took it: a field lets go of it, and the layer that owns the field closes.
        Event::KeyPressed { key: Key::Named(Named::Escape), .. } => Some(Message::Escape(status == iced::event::Status::Captured)),
        _ if status == iced::event::Status::Captured => None,
        Event::KeyPressed { key: Key::Named(Named::Delete), .. } => Some(Message::DeleteKey),
        // `/` searches the commands.
        Event::KeyPressed { key: Key::Character(c), modifiers, .. } if c.as_str() == "/" && !modifiers.command() => Some(Message::FocusSearch),
        // Digits typed while a grip is held (the window takes them only then), Enter as '\n'.
        Event::KeyPressed { key: Key::Named(Named::Backspace), .. } => Some(Message::GripKey(None)),
        Event::KeyPressed { key: Key::Named(Named::Enter), .. } => Some(Message::GripKey(Some('\n'))),
        Event::KeyPressed { key: Key::Character(c), modifiers, .. } if !modifiers.command() && !modifiers.alt() && c.len() == 1 => {
            c.chars().next().filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | 'e')).map(|c| Message::GripKey(Some(c)))
        }
        Event::KeyPressed { key, modifiers, repeat, .. } if !modifiers.alt() => match (arrow(&key), &key) {
            (Some((right, up)), _) if !repeat || modifiers.command() => {
                Some(Message::Key3d(Nav::Arrow { right, up, shift: modifiers.shift(), ctrl: modifiers.command() }))
            }
            (None, Key::Character(c)) if modifiers.command() && !modifiers.shift() => {
                let digit = c.parse::<usize>().ok().filter(|d| *d >= 1)?;
                DIGIT_VIEWS.get(digit - 1).map(|v| Message::Key3d(Nav::Goal(Goal::standard(*v))))
            }
            _ => None,
        },
        _ => None,
    }
}

/// The 3D view `shot` drawn offscreen on the process's own device into a picture of the size it
/// is laid out at (the window drawn headless), or why not, in its place.
pub fn picture<'a>(shot: Shot, tokens: crate::look::Tokens) -> iced::Element<'a, Message> {
    use iced::Widget;
    iced::widget::responsive(move |size| {
        let out = [size.width, size.height].map(|s| (s * crate::headless::SCALE).round().clamp(1.0, 4096.0) as u32);
        match emcraft_gpu3d::picture(0, &frame(&shot, out, crate::headless::SCALE)) {
            Ok(c) => iced::widget::image(iced::widget::image::Handle::from_rgba(c.w as u32, c.h as u32, c.rgba))
                .width(iced::Fill)
                .height(iced::Fill)
                .boxed(),
            Err(e) => {
                iced::widget::container(iced::widget::text(e.to_string()).size(tokens.font_small).color(tokens.text_dim)).center(iced::Fill).boxed()
            }
        }
    })
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::mouse::Cursor;
    use iced::widget::shader::Program;

    fn viewer(nav: &'static NavPreset) -> Viewer {
        let packed = Arc::new(Packed::new(&emcraft_render::view3d::Scene::default(), 1));
        let shot = Shot {
            packed,
            camera: Camera::default(),
            ground: Default::default(),
            look: Default::default(),
            field: None,
            cut: None,
            selected: Vec::new(),
            outline: [0.0; 4],
            moving: false,
        };
        Viewer { shot, nav, hover: false, tool: false, grips: Vec::new(), around: Default::default(), plane: None }
    }

    fn moved(x: f32, y: f32) -> iced::Event {
        iced::Event::Mouse(mouse::Event::CursorMoved { position: Point::new(x, y) })
    }

    /// The touchpad mapping orbits with Alt held and no button down; a trackpad's two-finger
    /// scroll pans in the default mapping and zooms in the touchpad's.
    #[test]
    fn the_touchpad_and_the_trackpad() {
        use emcraft_ui_core::navigation::{FUSION, TOUCHPAD, preset};
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
        let mut grip = Grip { size: Some(bounds.size()), ..Grip::default() };
        let touch = viewer(preset(TOUCHPAD));
        let alt = iced::Event::Keyboard(keyboard::Event::ModifiersChanged(keyboard::Modifiers::ALT));
        touch.update(&mut grip, &alt, bounds, Cursor::Available(Point::new(100.0, 100.0)));
        assert!(
            touch.update(&mut grip, &moved(100.0, 100.0), bounds, Cursor::Available(Point::new(100.0, 100.0))).is_none(),
            "the first move only marks where"
        );
        assert!(touch.update(&mut grip, &moved(110.0, 100.0), bounds, Cursor::Available(Point::new(110.0, 100.0))).is_some(), "the next orbits");
        let scroll = iced::Event::Mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Pixels { x: 3.0, y: 4.0 } });
        let mut plain = Grip { size: Some(bounds.size()), ..Grip::default() };
        assert!(viewer(preset(FUSION)).update(&mut plain, &scroll, bounds, Cursor::Available(Point::new(10.0, 10.0))).is_some());
        assert!(touch.update(&mut plain, &scroll, bounds, Cursor::Available(Point::new(10.0, 10.0))).is_some());
    }
}

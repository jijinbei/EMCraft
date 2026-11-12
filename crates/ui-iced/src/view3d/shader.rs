//! The 3D view in iced's shader widget: the scene's frame drawn by [`emcraft_gpu3d`] on the
//! window's own device (offscreen in `prepare`, shown in the widget's place in `draw`), and the
//! pointer's drags and the wheel read as the mouse mapping says ([`NavPreset`]), each reported as
//! a [`Nav`] for the window to apply.

use std::sync::Arc;

use emcraft_gpu3d::{Caps, Frame, Gpu3dError, Packed, Renderer, Uniforms};
use emcraft_render::Rgb;
use emcraft_render::view3d::Camera;
use emcraft_ui_core::navigation::{NavButton, NavKeys, NavMode, NavPreset};
use iced::widget::shader::{self, Action, Viewport};
use iced::{Point, Rectangle, Size, keyboard, mouse};

use super::{Click, Nav};
use crate::app::Message;

/// Supersampled at rest up to this many output pixels (as the egui window's).
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
/// let shot = Shot { packed, camera: Default::default(), ground: Default::default(), look: Default::default(), field: None, selected: Vec::new(), outline: [0.0; 4], moving: false };
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
    let uniforms = Uniforms::new(&shot.packed, &shot.camera, size, line_width, shot.ground);
    Frame {
        look: shot.look,
        ppp,
        field: shot.field.clone(),
        selected: shot.selected.clone(),
        outline: shot.outline,
        ..Frame::plain(shot.packed.clone(), uniforms, size, out, supersample)
    }
}

/// The view's program: what it draws and the mouse mapping its drags follow.
pub struct Viewer {
    pub shot: Shot,
    pub nav: &'static NavPreset,
}

/// The pointer between events: the drag in progress (its mode and where it was last), where the
/// left button went down, the last click (when and where, for a double click), the modifier keys
/// held and the view's size as last reported.
#[derive(Default)]
pub struct Grip {
    drag: Option<(NavMode, Point)>,
    pressed: Option<Point>,
    clicked: Option<(std::time::Instant, Point)>,
    keys: NavKeys,
    size: Option<Size>,
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

fn keys(m: keyboard::Modifiers) -> NavKeys {
    NavKeys { shift: m.shift(), ctrl: m.command(), alt: m.alt() }
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
                grip.keys = keys(*m);
                None
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(b)) => {
                let at = cursor.position_in(bounds)?;
                grip.pressed = (*b == mouse::Button::Left).then_some(at);
                grip.drag = button(*b).and_then(|b| self.nav.mode(b, grip.keys)).map(|mode| (mode, at));
                Some(Action::capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let (mode, last) = grip.drag?;
                let at = *position - iced::Vector::new(bounds.x, bounds.y);
                grip.drag = Some((mode, at));
                publish(dragged(mode, at - last))
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(b)) => {
                grip.drag = None;
                let at = cursor.position_in(bounds);
                let pressed = grip.pressed.take().filter(|_| *b == mouse::Button::Left);
                match (pressed, at) {
                    (Some(p), Some(at)) if p.distance(at) <= CLICK_SLOP => Some(Action::publish(Message::Click(clicked(grip, at))).and_capture()),
                    _ => Some(Action::request_redraw()),
                }
            }
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_in(bounds)?;
                let steps = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => y * PIXEL_ZOOM,
                };
                publish(Nav::Zoom(if self.nav.reverse_zoom { -steps } else { steps }))
            }
            _ => None,
        }
    }

    fn draw(&self, grip: &Grip, _cursor: mouse::Cursor, _bounds: Rectangle) -> Shot {
        Shot { moving: grip.drag.is_some(), ..self.shot.clone() }
    }

    fn mouse_interaction(&self, grip: &Grip, _bounds: Rectangle, _cursor: mouse::Cursor) -> mouse::Interaction {
        match grip.drag {
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
            eprintln!("emcraft-iced: {e}");
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
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("emcraft_iced_view3d") });
            r.prepare(device, queue, &mut encoder, &f).map(|()| queue.submit([encoder.finish()]))
        });
        if let Err(e) = drawn.map_err(Gpu3dError::Frame).and_then(|r| r.map(|_| ())) {
            eprintln!("emcraft-iced: {e}; the 3D view stops");
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

//! The 3D view's input: zoom and its limits, the pivot, the wheel, the mouse's gestures (orbit,
//! pan, zoom, the planes' and the grips' drags), box selection and the keys.

use super::*;

/// The distances (m) a zoom keeps within, for a model of box `bounds`.
pub(super) fn zoom_limits((lo, hi): ([f64; 3], [f64; 3])) -> [f64; 2] {
    let size = (0..3).map(|i| (hi[i] - lo[i]).powi(2)).sum::<f64>().sqrt();
    let size = if size.is_finite() && size > 0.0 { size } else { 1.0 };
    [size * 1e-4, size * 1e3]
}

/// The model point under `pos` on the view drawn into `rect`: the surface there, else where the
/// pointer's ray meets the plane through the target facing the camera.
pub(super) fn zoom_anchor(v: &View3d, rect: Rect, pos: Option<egui::Pos2>) -> Option<[f64; 3]> {
    let at = pos? - rect.min;
    let (x, y, w, h) = (f64::from(at.x), f64::from(at.y), f64::from(rect.width()), f64::from(rect.height()));
    v.shown().and_then(|b| view3d::surface_at(&b.scene, &v.camera, x, y, w, h)).or_else(|| v.camera.on_target_plane(x, y, w, h))
}

/// What an orbit beginning with the pointer at `pos` turns about (Settings ▸ 3D view ▸ Orbit
/// around): the target (none), the origin, or the model point under the cursor (over empty
/// space, the middle of what a fit frames), chosen once when the orbit begins.
pub(super) fn pivot(app: &EmcraftApp, v: &View3d, rect: Rect, pos: Option<egui::Pos2>, frame: ([f64; 3], [f64; 3])) -> Option<[f64; 3]> {
    match app.prefs.orbit_around() {
        OrbitAround::ViewCentre => None,
        OrbitAround::Origin => Some([0.0; 3]),
        OrbitAround::Cursor => Some(
            pos.and_then(|p| {
                let at = p - rect.min;
                v.shown().and_then(|b| {
                    view3d::surface_at(&b.scene, &v.camera, f64::from(at.x), f64::from(at.y), f64::from(rect.width()), f64::from(rect.height()))
                })
            })
            .unwrap_or_else(|| [0, 1, 2].map(|i| 0.5 * (frame.0[i] + frame.1[i]))),
        ),
    }
}

/// A wheel event's move in points, as egui counts them: lines at its line speed, pages at
/// `page` points.
pub(super) fn wheel_points(unit: egui::MouseWheelUnit, delta: egui::Vec2, o: &egui::InputOptions, page: f32) -> egui::Vec2 {
    match unit {
        egui::MouseWheelUnit::Point => delta,
        egui::MouseWheelUnit::Line => delta * o.line_scroll_speed,
        egui::MouseWheelUnit::Page => delta * page,
    }
}

/// The wheel's vertical share, with egui's modifiers that turn it sideways or upright.
pub(super) fn wheel_vertical(points: egui::Vec2, m: egui::Modifiers, o: &egui::InputOptions) -> f32 {
    let (sideways, upright) = (m.matches_any(o.horizontal_scroll_modifier), m.matches_any(o.vertical_scroll_modifier));
    if sideways && !upright {
        0.0
    } else if upright && !sideways {
        points.x + points.y
    } else {
        points.y
    }
}

/// This frame's wheel and trackpad: wheel zoom (points), pinch factor, two-finger pan and
/// Shift + two-finger move.
#[derive(Default)]
pub(super) struct Wheel {
    zoom: f32,
    pinch: f32,
    pan: egui::Vec2,
    shift_pan: egui::Vec2,
}

/// Reads the wheel events: a mouse wheel's lines and pages zoom; a trackpad's two-finger scroll
/// (point moves between its start and end) pans, with Shift orbits where the mapping says so;
/// bare point moves are a mouse with fine steps; the zoom modifier (Ctrl) and pinch zoom.
pub(super) fn wheel(v: &mut View3d, ui: &egui::Ui, nav: &crate::navigation::NavPreset) -> Wheel {
    let o = ui.ctx().options(|o| o.input_options);
    let page = ui.ctx().viewport_rect().height();
    let mut w = Wheel { pinch: 1.0, ..Wheel::default() };
    ui.input(|i| {
        for e in &i.events {
            match e {
                egui::Event::Zoom(f) if f.is_finite() && *f > 0.0 => w.pinch *= f,
                egui::Event::MouseWheel { unit, delta, modifiers, phase } => {
                    let point = *unit == egui::MouseWheelUnit::Point;
                    match phase {
                        egui::TouchPhase::Start => {
                            v.gesture |= point;
                            continue;
                        }
                        egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                            v.gesture &= !point;
                            continue;
                        }
                        egui::TouchPhase::Move => {}
                    }
                    let pts = wheel_points(*unit, *delta, &o, page);
                    if !pts.is_finite() {
                        continue;
                    }
                    if modifiers.matches_any(o.zoom_modifier) {
                        let f = (o.scroll_zoom_speed * (pts.x + pts.y)).exp();
                        if f.is_finite() && f > 0.0 {
                            w.pinch *= f;
                        }
                        continue;
                    }
                    let shift = modifiers.shift && !modifiers.ctrl && !modifiers.command && !modifiers.alt;
                    if point && v.gesture && nav.scroll_pans() {
                        if shift && nav.shift_scroll_orbits() {
                            w.shift_pan += pts;
                        } else {
                            w.pan += pts;
                        }
                    } else if point {
                        // Bare point moves are a mouse: up and down zoom, sideways pans.
                        w.zoom += wheel_vertical(pts, *modifiers, &o);
                        w.pan.x -= pts.x;
                    } else {
                        w.zoom += wheel_vertical(pts, *modifiers, &o);
                    }
                }
                _ => {}
            }
        }
    });
    w
}

/// Orbit, pan and zoom by the mouse mapping (Settings ▸ 3D view), the wheel and the trackpad; a
/// click picks what is under it (empty space clears), a double-click on nothing fits the view, a
/// middle double-click goes home (fits, in the mapping that does). A plain left press on the
/// reference plane's `grip` or on the field view's plane drags that instead, as SolidCraft's
/// in-view handles take a plain press on them: the mapping's left-button navigation is for
/// empty space or with its keys held, and Ctrl+left drag orbits in every mapping. With a tool
/// on, a click is the tool's.
pub(super) fn navigate(
    app: &EmcraftApp,
    v: &mut View3d,
    ui: &mut egui::Ui,
    rect: Rect,
    frame: ([f64; 3], [f64; 3]),
    grip: Option<egui::Pos2>,
) -> Gesture {
    let resp = ui.hit(rect, Fixed::View3d.into(), Sense::click_and_drag());
    let (mods, hover, press, latest, space, any_down) = ui.input(|i| {
        (i.modifiers, i.pointer.hover_pos(), i.pointer.press_origin(), i.pointer.latest_pos(), i.key_down(egui::Key::Space), i.pointer.any_down())
    });
    let n = Nav { app, nav: app.prefs.nav(), rect, frame, mods, ctrl: mods.ctrl || mods.command, hover, press, latest };
    let space = space && !ui.ctx().egui_wants_keyboard_input();
    // The left button: chosen once, when the drag begins.
    let grip_started = resp.drag_started_by(egui::PointerButton::Primary);
    if grip_started {
        v.left = n.left_drag(v, ui, &resp, space, grip);
    }
    let on_grip = v.left == Some(LeftDrag::Grip);
    let select = n.box_drag(v, &resp);
    let boxing = matches!(v.left, Some(LeftDrag::Box { .. }));
    let d = resp.drag_delta();
    let plane_drag = (v.left == Some(LeftDrag::Plane) && resp.dragged()).then_some(d);
    let grip_drag = match (on_grip, grip_started, resp.dragged()) {
        (false, _, _) => GripDrag::Idle,
        (true, _, false) => GripDrag::Released,
        (true, true, true) => GripDrag::Started(d),
        (true, false, true) => GripDrag::Moved(d),
    };
    let inside = resp.hovered() || resp.contains_pointer();
    let mut orbiting = n.button_drag(v, &resp);
    orbiting |= n.touchpad(v, ui, inside && !any_down);
    orbiting |= n.wheel(v, ui, inside);
    if !orbiting && !(v.gesture && v.orbit.is_some()) {
        v.orbit = None;
    }
    // A box leaves the picture as it is.
    v.dragging = (resp.dragged() && !boxing) || orbiting;
    if resp.double_clicked_by(egui::PointerButton::Middle) && inside {
        v.goal = Some(if n.nav.middle_double_fits { Goal::Fit } else { Goal::Home });
    }
    if v.tools.tool.is_some() {
        return Gesture { select: false, double: false, plane_drag, grip: grip_drag, tool_click: resp.clicked() };
    }
    let double = n.click(v, ui, &resp);
    Gesture { select: select || resp.clicked(), double, plane_drag, grip: grip_drag, tool_click: false }
}

/// What a frame's gestures in the view read: the mapping, the view's rectangle and the model's
/// box, the keys held and the pointer.
struct Nav<'a> {
    app: &'a EmcraftApp,
    nav: &'static crate::navigation::NavPreset,
    rect: Rect,
    frame: ([f64; 3], [f64; 3]),
    mods: egui::Modifiers,
    /// Ctrl, or Cmd on a Mac.
    ctrl: bool,
    hover: Option<egui::Pos2>,
    press: Option<egui::Pos2>,
    latest: Option<egui::Pos2>,
}

impl Nav<'_> {
    /// An orbit by `d` points about the pivot under `at` (kept while the orbit goes on).
    fn orbit(&self, v: &mut View3d, at: Option<egui::Pos2>, d: egui::Vec2, snap: bool) {
        let mut o = v.orbit.unwrap_or_else(|| Drag::new(v.camera, pivot(self.app, v, self.rect, at, self.frame)));
        o.orbit(&mut v.camera, f64::from(d.x), f64::from(d.y), snap);
        v.orbit = Some(o);
    }

    fn pan(&self, v: &mut View3d, d: egui::Vec2) {
        v.camera.pan(f64::from(d.x), f64::from(d.y), f64::from(self.rect.height()));
    }

    fn zoom(&self, v: &mut View3d, at: Option<[f64; 3]>, factor: f64) {
        v.camera.zoom_at(at, factor, zoom_limits(self.frame));
    }

    /// What a left drag beginning now does: Space held pans; a plain press on the grip or on the
    /// field's plane drags it; else the mapping's navigation, Ctrl orbits, or (no tool on) a box.
    fn left_drag(&self, v: &mut View3d, ui: &egui::Ui, resp: &egui::Response, space: bool, grip: Option<egui::Pos2>) -> Option<LeftDrag> {
        let mods = self.mods;
        let at = self.press.or_else(|| resp.interact_pointer_pos());
        let plain = !mods.shift && !self.ctrl && !mods.alt;
        if space {
            Some(LeftDrag::Nav(NavMode::Pan, false))
        } else if plain && grip.zip(at).is_some_and(|(g, p)| g.distance(p) <= crate::tools3d::GRIP_PX) {
            Some(LeftDrag::Grip)
        } else if plain && at.is_some_and(|p| v.pick_at(ui.ctx(), self.rect, p) == Some(scene3d::Pick::Plane(0))) {
            Some(LeftDrag::Plane)
        } else if let Some(m) = self.nav.mode(crate::navigation::NavButton::Left, mods) {
            Some(LeftDrag::Nav(m, mods.shift))
        } else if self.ctrl && !mods.shift && !mods.alt {
            Some(LeftDrag::Nav(NavMode::Orbit, false))
        } else if v.tools.tool.is_none() {
            Some(LeftDrag::Box { add: mods.shift })
        } else {
            None
        }
    }

    /// The box follows the pointer from the press; let go, it selects (`true`). The left drag
    /// ends with the button.
    fn box_drag(&self, v: &mut View3d, resp: &egui::Response) -> bool {
        let mut select = false;
        match (v.left, resp.dragged()) {
            (Some(LeftDrag::Box { .. }), true) => {
                v.boxsel = self.press.zip(self.latest).map(|(a, b)| BoxSel::from_drag(a, b)).filter(|b| b.rect.is_finite());
            }
            (Some(LeftDrag::Box { add }), false) => {
                if let Some(b) = v.boxsel.take() {
                    box_select(v, self.rect, b, add);
                    select = true;
                }
            }
            _ => {}
        }
        if !resp.dragged() {
            v.left = None;
            v.boxsel = None;
        }
        select
    }

    /// The middle and right buttons by the mapping, the left as chosen at its press; whether it
    /// orbits.
    fn button_drag(&self, v: &mut View3d, resp: &egui::Response) -> bool {
        use crate::navigation::{DRAG_ZOOM_PER_POINT, NavButton, OPENSCAD};
        let mods = self.mods;
        let button = if resp.dragged_by(egui::PointerButton::Middle) {
            Some(NavButton::Middle)
        } else if resp.dragged_by(egui::PointerButton::Secondary) {
            Some(NavButton::Right)
        } else {
            None
        };
        let (mode, needs_shift) = match (button, v.left) {
            (Some(b), _) => (self.nav.mode(b, mods), self.nav.mode(b, egui::Modifiers { shift: false, ..mods }) != self.nav.mode(b, mods)),
            (None, Some(LeftDrag::Nav(m, shift))) if resp.dragged_by(egui::PointerButton::Primary) => (Some(m), shift),
            _ => (None, false),
        };
        // The drag's first frame catches up the way from the press, so none of it is lost.
        let d = resp.drag_delta();
        let d = if resp.drag_started() { self.press.zip(self.latest).map_or(d, |(a, b)| b - a) } else { d };
        let d = if d.is_finite() { d } else { egui::Vec2::ZERO };
        match mode {
            Some(NavMode::Orbit) => {
                v.stop_animation();
                let snap = mods.shift && !needs_shift && self.nav.id != OPENSCAD;
                self.orbit(v, self.press.or(self.hover), d, snap);
                return true;
            }
            Some(NavMode::Pan) if d != egui::Vec2::ZERO => {
                v.stop_animation();
                self.pan(v, d);
            }
            Some(NavMode::Zoom) if d != egui::Vec2::ZERO => {
                v.stop_animation();
                self.zoom(v, None, (f64::from(d.y) * DRAG_ZOOM_PER_POINT).exp());
            }
            _ => {}
        }
        false
    }

    /// The touchpad mapping: Shift or Alt held and the pointer moved over the view (`over`, no
    /// button down); whether it orbits.
    fn touchpad(&self, v: &mut View3d, ui: &egui::Ui, over: bool) -> bool {
        let motion = self.nav.motion_mode(self.mods).zip(self.hover).filter(|_| over && !ui.ctx().egui_wants_keyboard_input());
        let before = v.motion;
        v.motion = motion;
        let Some((m, at)) = motion else { return false };
        let step = before.filter(|(b, _)| *b == m).map_or(egui::Vec2::ZERO, |(_, last)| at - last);
        let step = if step.is_finite() && step.length() <= 1e4 { step } else { egui::Vec2::ZERO };
        if step != egui::Vec2::ZERO {
            v.stop_animation();
            match m {
                NavMode::Orbit => self.orbit(v, Some(at), step, false),
                NavMode::Pan | NavMode::Zoom => self.pan(v, step),
            }
        }
        m == NavMode::Orbit
    }

    /// The wheel and the trackpad, toward the point under the cursor, with the pointer over the
    /// view (`inside`; their gestures are followed even when not); whether they orbit.
    fn wheel(&self, v: &mut View3d, ui: &egui::Ui, inside: bool) -> bool {
        use crate::navigation::WHEEL_ZOOM_PER_POINT;
        let sign = if self.nav.reverse_zoom { -1.0 } else { 1.0 };
        let w = wheel(v, ui, self.nav);
        if !inside {
            return false;
        }
        if w.pinch != 1.0 && w.pinch.is_finite() {
            v.stop_animation();
            self.zoom(v, zoom_anchor(v, self.rect, self.hover), f64::from(w.pinch).powf(-sign));
        }
        if w.zoom != 0.0 && w.zoom.is_finite() {
            v.stop_animation();
            self.zoom(v, zoom_anchor(v, self.rect, self.hover), (-f64::from(w.zoom) * WHEEL_ZOOM_PER_POINT * sign).exp());
        }
        if w.pan != egui::Vec2::ZERO && w.pan.is_finite() {
            v.stop_animation();
            self.pan(v, w.pan);
        }
        let orbits = w.shift_pan != egui::Vec2::ZERO && w.shift_pan.is_finite();
        if orbits {
            v.stop_animation();
            self.orbit(v, self.hover, w.shift_pan, false);
        }
        orbits
    }

    /// A click picks what is under it (Shift or Ctrl adds or takes it away; empty space
    /// clears); a double-click on a part picks it alone (`true`), on nothing fits the view.
    fn click(&self, v: &mut View3d, ui: &egui::Ui, resp: &egui::Response) -> bool {
        if !(resp.clicked() || resp.double_clicked()) {
            return false;
        }
        let Some(pos) = resp.interact_pointer_pos() else { return false };
        let pick = v.pick_at(ui.ctx(), self.rect, pos);
        let click = if resp.double_clicked() {
            selection::Click::Double { pick }
        } else {
            selection::Click::Single { pick, add: self.mods.shift || self.ctrl }
        };
        match selection::step(&mut v.selected, click) {
            selection::Then::Edit => true,
            selection::Then::Fit => {
                v.goal = Some(Goal::Fit);
                false
            }
            selection::Then::Nothing => false,
        }
    }
}

/// Selects the parts the box `b` (screen points over the view in `rect`) takes, added to what is
/// selected when `add` (Shift held), else instead of it.
pub(super) fn box_select(v: &mut View3d, rect: Rect, b: BoxSel, add: bool) {
    let local = BoxSel { rect: b.rect.translate(-rect.min.to_vec2()), crossing: b.crossing };
    let picks =
        v.shown().map(|s| selection::boxed(&s.scene, &v.camera, f64::from(rect.width()), f64::from(rect.height()), local)).unwrap_or_default();
    if !add {
        v.selected.clear();
    }
    v.selected.add(&picks);
}

/// The selection box: by window a solid outline, by crossing a dashed one in another colour, as
/// SolidCraft draws them.
pub(super) fn paint_box(p: &egui::Painter, b: BoxSel) {
    let t = &Tokens::get();
    let ink = if b.crossing { t.success } else { t.accent };
    p.rect_filled(b.rect, 0.0, ink.gamma_multiply(0.12));
    if b.crossing {
        let c = [b.rect.left_top(), b.rect.right_top(), b.rect.right_bottom(), b.rect.left_bottom()];
        for i in 0..4 {
            p.extend(egui::Shape::dashed_line(&[c[i], c[(i + 1) % 4]], Stroke::new(1.2, ink), 6.0, 4.0));
        }
    } else {
        p.rect_stroke(b.rect, 0.0, Stroke::new(1.2, ink), egui::StrokeKind::Inside);
    }
}

/// Whether the keys are the 3D view's: no field, menu, dialog card's field, search or page
/// has them.
pub(super) fn keys_free(app: &EmcraftApp, ui: &egui::Ui) -> bool {
    app.ui_state.page.settings_shown().is_none()
        && app.ui_state.ask().is_none()
        && !app.ui_state.searching()
        && ui.ctx().memory(|m| m.focused().is_none())
        && !ui.ctx().egui_wants_keyboard_input()
        && !egui::Popup::is_any_open(ui.ctx())
}

/// The view keys, as SolidCraft's: the arrows turn the view 15° (with Shift a quarter turn),
/// animated, and held past a tap keep turning it at a steady rate; Ctrl+arrows pan a tenth of the
/// view; Ctrl+1…7 turn to Front, Back, Left, Right, Top, Bottom and the isometric view.
pub(super) fn keys(app: &EmcraftApp, v: &mut View3d, ui: &egui::Ui, rect: Rect, now: f64) {
    use crate::navigation::{ARROWS, DIGIT_VIEWS, HOLD_DEGREES_PER_SECOND, KEY_PAN_SHARE, KEY_STEP_DEGREES, TAP_SECONDS};
    if !keys_free(app, ui) {
        v.key_hold = None;
        return;
    }
    let presses: Vec<(egui::Key, egui::Modifiers, bool)> = ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, repeat, modifiers, .. } => Some((*key, *modifiers, *repeat)),
                _ => None,
            })
            .collect()
    });
    for (key, m, repeat) in presses {
        let ctrl = m.ctrl || m.command;
        if m.alt {
            continue;
        }
        if let Some(&(_, right, up)) = ARROWS.iter().find(|(k, _, _)| *k == key) {
            if ctrl {
                // The model moves with the arrow, as under a pan drag.
                let step = f64::from(rect.height() * KEY_PAN_SHARE);
                v.stop_animation();
                v.camera.pan(right * step, -up * step, f64::from(rect.height()));
            } else if !repeat {
                v.key_hold = Some(KeyHold { key, right, up, shift: m.shift, since: now });
                v.goal = Some(Goal::Turn { right, up, degrees: KEY_STEP_DEGREES[usize::from(m.shift)] });
            }
        } else if ctrl
            && !m.shift
            && let Some(&(_, view)) = DIGIT_VIEWS.iter().find(|(k, _)| *k == key)
        {
            v.goal = Some(Goal::View(view));
        }
    }
    let Some(hold) = v.key_hold else { return };
    let (down, dt) = ui.input(|i| (i.key_down(hold.key), f64::from(i.stable_dt)));
    if !down {
        v.key_hold = None;
        return;
    }
    ui.ctx().request_repaint();
    if now - hold.since < TAP_SECONDS {
        return;
    }
    let rate = if hold.shift { 2.0 * HOLD_DEGREES_PER_SECOND } else { HOLD_DEGREES_PER_SECOND };
    let degrees = rate * if dt.is_finite() { dt.clamp(0.0, 0.1) } else { 0.0 };
    if degrees > 1e-6 {
        v.stop_animation();
        v.camera = v.camera.stepped(hold.right, hold.up, degrees);
    }
}

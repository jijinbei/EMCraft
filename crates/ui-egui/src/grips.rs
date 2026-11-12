//! A selected body's grips in the egui window: what the shared rules find and decide
//! ([`emcraft_ui_core::tools::grips`]: the grips, the drag snapping to planes or the grid, the
//! digits typed, the `geometry.body.edit` let go) drawn over the 3D view — squares on its faces,
//! arrows from its middle that move it, a ring that turns it, the dragged one's value beside it.

use egui::{FontId, Pos2, Rect, Stroke, vec2};
use emcraft_ui_core::tools::grips::{self, Grip};

use crate::handle::Handle;
use crate::theme::Tokens;
use crate::tools3d::screen;
use crate::view3d::View3d;
use crate::{Action, Actions, EmcraftApp, scenario};

pub use emcraft_ui_core::tools::grips::BodyDrag;

/// `rect`'s size as the shared rules take it.
fn size(rect: Rect) -> emcraft_ui_core::geom::Vector {
    crate::geom::vector(rect.size())
}

/// Starts dragging the grip under `at`.
fn start(app: &EmcraftApp, v: &mut View3d, rect: Rect, at: Pos2) -> bool {
    let at = crate::geom::point(at - rect.min.to_vec2());
    v.body_drag = grips::start(app.session.model(), app.ui_state.selection(), &v.camera, size(rect), at);
    v.body_drag.is_some()
}

/// The drag let go: one `geometry.body.edit` where it was left.
fn end(app: &EmcraftApp, v: &mut View3d, out: &mut Actions) {
    let Some(bd) = v.body_drag.take() else { return };
    if let Some(params) = grips::let_go(&bd, app.session.display().length) {
        out.push(Action::run("geometry.body.edit", params));
    }
}

/// Digits typed while a grip is held: the value it sets, exactly (display units; a move grip's
/// distance, signed). Enter lets go with it, Escape drops the drag, Backspace takes a digit back.
/// The keys are taken from the frame, so nothing else reads them.
pub fn keys(app: &EmcraftApp, v: &mut View3d, ui: &egui::Ui, out: &mut Actions) {
    if v.body_drag.is_none() {
        return;
    }
    let (mut enter, mut escape) = (false, false);
    let mut keys = Vec::new();
    ui.input_mut(|i| {
        i.events.retain(|e| match e {
            egui::Event::Text(t) => {
                keys.extend(t.chars().map(Some));
                false
            }
            egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                keys.push(None);
                false
            }
            egui::Event::Key { key: egui::Key::Enter, pressed: true, .. } => {
                enter = true;
                false
            }
            egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => {
                escape = true;
                false
            }
            _ => true,
        });
    });
    v.body_drag = v.body_drag.take().map(|bd| keys.into_iter().fold(bd, grips::typed));
    if escape {
        v.body_drag = None;
    } else if enter {
        end(app, v, out);
    }
}

/// Draws the selected body's grips (the dragged one where it is, with its value).
pub fn draw(app: &EmcraftApp, v: &View3d, p: &egui::Painter, rect: Rect) {
    let t = &Tokens::get();
    let axis_colour = |k: usize| [t.axis_x, t.axis_y, t.axis_z][k.min(2)];
    for mark in grips::marks(app.session.model(), app.ui_state.selection(), v.body_drag.as_ref(), app.session.display().length) {
        let Some(s) = screen(&v.camera, rect, mark.at) else { continue };
        match mark.grip {
            Grip::Move { axis } => {
                let c = axis_colour(axis);
                // From the middle, carried as far as the arrow.
                if let Some(m) = mark.from.and_then(|f| screen(&v.camera, rect, f)) {
                    p.line_segment([m, s], Stroke::new(2.0, c));
                }
                p.circle_filled(s, 5.5, c);
                p.circle_stroke(s, 5.5, Stroke::new(1.5, t.panel));
            }
            Grip::Turn => {
                p.circle_stroke(s, 9.0, Stroke::new(2.0, t.accent));
                p.circle_filled(s, 3.0, t.accent);
            }
            _ => {
                let r = Rect::from_center_size(s, vec2(9.0, 9.0));
                p.rect_filled(r, 2.0, t.panel);
                p.rect_stroke(r, 2.0, Stroke::new(2.0, t.accent), egui::StrokeKind::Middle);
            }
        }
        scenario::publish_handle(Handle::BodyGrip(&mark.grip.id()), s);
        if let Some(text) = mark.label {
            let gal = p.layout_no_wrap(text, FontId::proportional(t.font_small), t.text);
            let at = Rect::from_min_size(s + vec2(10.0, -gal.size().y - 8.0), gal.size() + vec2(10.0, 4.0));
            p.rect_filled(at, 4.0, t.panel.gamma_multiply(0.9));
            p.galley(at.min + vec2(5.0, 2.0), gal, t.text);
        }
    }
}

/// Starts a drag on whatever grip is under `at`: a body's, else the port's reference plane.
pub fn start_any(app: &EmcraftApp, v: &mut View3d, rect: Rect, at: Option<Pos2>) {
    if let Some(at) = at
        && start(app, v, rect, at)
    {
        return;
    }
    crate::tools3d::start_drag(app, v);
}

/// The nearest grip to the pointer: a body's, or the reference plane's.
pub fn nearest(app: &EmcraftApp, v: &View3d, rect: Rect, at: Option<Pos2>) -> Option<Pos2> {
    let body = at
        .and_then(|a| grips::under(app.session.model(), app.ui_state.selection(), &v.camera, size(rect), crate::geom::point(a - rect.min.to_vec2())));
    body.map(|(_, s)| rect.min + vec2(s.x, s.y)).or_else(|| crate::tools3d::grip(app, v, rect))
}

/// The actions a drag ends in.
pub fn released(app: &EmcraftApp, v: &mut View3d, out: &mut Actions) {
    if v.body_drag.is_some() {
        end(app, v, out);
    } else {
        crate::tools3d::end_drag(app, v, out);
    }
}

/// Moves whichever grip is dragged.
pub fn moved(app: &EmcraftApp, v: &mut View3d, rect: Rect, d: egui::Vec2) {
    match v.body_drag.take() {
        Some(bd) => {
            let (m, l) = (app.session.model(), app.session.display().length);
            v.body_drag = Some(grips::dragged(bd, m, l, &v.camera, size(rect), crate::geom::vector(d)));
        }
        None => crate::tools3d::drag(app, v, rect, d),
    }
}

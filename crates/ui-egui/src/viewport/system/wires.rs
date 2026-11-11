//! The canvas under the cards: its ground and grid, the links as curves (each with the mark that
//! takes it away) and the link being dragged.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};
use serde_json::json;

use super::bridge::pos;
use crate::handle::Handle;
use crate::theme::Tokens;
use crate::{Action, Actions, Hint, UiExt};
use emcraft_ui_core::system::layout::Layout;

/// The grid's spacing (points).
const GRID: f32 = 24.0;

/// A cubic link from `a` (an output) to `b` (an input), bowed out horizontally.
fn curve(a: Pos2, b: Pos2, stroke: Stroke) -> egui::Shape {
    let bow = ((b.x - a.x).abs() * 0.5).max(40.0);
    egui::epaint::CubicBezierShape::from_points_stroke([a, a + vec2(bow, 0.0), b - vec2(bow, 0.0), b], false, Color32::TRANSPARENT, stroke).into()
}

/// The ground of `area` and a grid of dots that moves with `pan`.
pub(super) fn ground(p: &egui::Painter, area: Rect, pan: Vec2) {
    let t = &Tokens::get();
    p.rect_filled(area, 8.0, t.panel);
    p.rect_stroke(area, 8.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    // Every GRID points from the first line inside the area, moved with the pan.
    let lines = |from: f32, shift: f32, to: f32| (0..).map(move |k| from + shift.rem_euclid(GRID) + k as f32 * GRID).take_while(move |x| *x < to);
    for y in lines(area.top(), pan.y, area.bottom()) {
        for x in lines(area.left(), pan.x, area.right()) {
            p.circle_filled(pos2(x, y), 0.8, t.border);
        }
    }
}

/// The links (those the chain reads in the accent) and, while one is dragged, the link from
/// `dragging`'s start to the pointer.
pub(super) fn links(p: &egui::Painter, layout: &Layout, dragging: Option<(Pos2, Pos2)>) {
    let t = &Tokens::get();
    for &(a, b) in &layout.links {
        let (from, to) = (&layout.nodes[a], &layout.nodes[b]);
        let ink = if from.on_chain && to.on_chain { t.accent } else { t.text_dim };
        p.add(curve(pos(from.output()), pos(to.input()), Stroke::new(2.0, ink)));
    }
    if let Some((a, b)) = dragging {
        p.add(curve(a, b, Stroke::new(2.0, t.accent)));
    }
}

/// Each link's mark at its middle: a click takes the link away.
pub(super) fn unlink_marks(ui: &mut egui::Ui, layout: &Layout, out: &mut Actions) {
    let t = &Tokens::get();
    for &(a, b) in &layout.links {
        let (from, to) = (&layout.nodes[a], &layout.nodes[b]);
        let mid = pos(from.output().lerp(to.input(), 0.5));
        let resp = ui.hit(Rect::from_center_size(mid, vec2(16.0, 16.0)), Handle::SystemUnlink(&to.name), Sense::click());
        let lit = resp.hovered();
        let p = ui.painter();
        p.circle(mid, 6.0, if lit { t.hover } else { t.panel }, Stroke::new(1.0, if lit { t.accent } else { t.border }));
        p.text(mid, Align2::CENTER_CENTER, "×", FontId::proportional(t.font_small - 1.0), if lit { t.text } else { t.text_dim });
        if resp.hint(t!("Take this link away")).clicked() {
            out.push(Action::run("system.unlink", json!({"from": from.name, "to": to.name})));
        }
    }
}

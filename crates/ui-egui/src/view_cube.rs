//! The view cube in the 3D view's corner in the egui window, drawn as SolidCraft draws its
//! navigation cube (`assets/icons/ui/source/navigation.rs`, MIT OR Apache-2.0; ATTRIBUTION.md)
//! from the shared rules ([`emcraft_ui_core::cube`]): the patches as one opaque surface shaded
//! by their tilt, only the cube's own edges outlined, the target under the pointer lit and
//! ringed in the accent, each face's name turned along it. Around it, while the pointer is near:
//! the quarter-turn arrows, Home and Fit, and the axes' triad. A drag on the cube orbits the
//! view (Shift snaps), the views animate, and clicks in quick succession add up.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use emcraft_render::view3d::Drag;
use emcraft_ui_core::cube::{self, DESIGN, Facet, HALF, Kind};

use crate::handle::{AXES, Fixed, Handle};
use crate::lang::{args, tr, trf};
use crate::theme::Tokens;
use crate::view3d::View3d;
use crate::{Actions, UiExt, scenario};

pub use emcraft_ui_core::cube::SIZE;

/// The faces' names, as their labels read (translated).
pub const FACE_NAMES: [&str; 6] = ["RIGHT", "LEFT", "TOP", "BOTTOM", "FRONT", "BACK"];

/// The cube's colours from the theme: faded toward the view's ground while the pointer is away.
#[derive(Clone, Copy)]
struct Ink {
    face: Color32,
    edge: Color32,
    text: Color32,
    hover: Color32,
    accent: Color32,
    axes: [Color32; 3],
}

fn ink(active: bool) -> Ink {
    let t = &Tokens::get();
    let fade = |c: Color32| if active { c } else { c.lerp_to_gamma(t.viewport_top, 0.55) };
    Ink {
        face: fade(t.panel),
        edge: fade(t.border),
        text: fade(t.text),
        hover: t.accent_soft,
        accent: t.accent,
        axes: [fade(t.axis_x), fade(t.axis_y), fade(t.axis_z)],
    }
}

fn pos(p: emcraft_ui_core::geom::Point) -> Pos2 {
    pos2(p.x, p.y)
}

/// The patches as one opaque mesh (no fringes between them), the cube's own edges, the lit
/// target's rim in the accent, and the faces' names turned along them.
fn paint(p: &egui::Painter, faces: &[Facet], lit: Option<[i8; 3]>, k: Ink, scale: f32) {
    let mut mesh = egui::Mesh::default();
    let mut edges: Vec<(Pos2, Pos2, usize, bool)> = Vec::new();
    for f in faces {
        let pts: Vec<Pos2> = f.points.iter().copied().map(pos).collect();
        let on = lit == Some(f.dir);
        let s = cube::shade(f.depth);
        let fill = if on {
            k.hover
        } else {
            Color32::from_rgb((f32::from(k.face.r()) * s) as u8, (f32::from(k.face.g()) * s) as u8, (f32::from(k.face.b()) * s) as u8)
        };
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos(f.centre), fill);
        for q in &pts {
            mesh.colored_vertex(*q, fill);
        }
        let n = pts.len() as u32;
        for j in 0..n {
            mesh.add_triangle(base, base + 1 + j, base + 1 + (j + 1) % n);
        }
        for (j, outer) in f.outer.iter().enumerate() {
            if !on && !outer {
                continue;
            }
            let (a, b) = (pts[j], pts[(j + 1) % pts.len()]);
            let same = |x: Pos2, y: Pos2| x.distance_sq(y) < 1e-6;
            match edges.iter_mut().find(|(x, y, _, _)| (same(*x, a) && same(*y, b)) || (same(*x, b) && same(*y, a))) {
                Some(e) => {
                    e.2 += usize::from(on);
                    e.3 |= outer;
                }
                None => edges.push((a, b, usize::from(on), *outer)),
            }
        }
    }
    p.add(egui::Shape::mesh(mesh));
    // Two lit patches side by side share a seam, not the target's rim.
    for (a, b, lit, outer) in edges.into_iter().filter(|(_, _, lit, outer)| *outer || *lit == 1) {
        let ink = if lit > 0 && (lit == 1 || outer) { k.accent } else { k.edge };
        p.line_segment([a, b], Stroke::new(1.0, ink));
    }
    let t = &Tokens::get();
    for (f, (name, r, _)) in faces.iter().filter_map(|f| f.label.map(|l| (f, l))) {
        let opacity = cube::label_opacity(f.depth);
        if opacity <= 0.0 || r.x.hypot(r.y) < 1e-3 {
            continue;
        }
        let galley =
            p.layout_no_wrap(crate::lang::lookup(name).to_owned(), FontId::proportional(t.font_heading * scale), k.text.gamma_multiply(opacity));
        let angle = r.y.atan2(r.x);
        // The galley turned about its middle, which sits on the face's middle.
        let half = galley.size() / 2.0;
        let (sin, cos) = angle.sin_cos();
        let corner = pos(f.centre) - vec2(half.x * cos - half.y * sin, half.x * sin + half.y * cos);
        p.add(egui::epaint::TextShape::new(corner, galley, k.text).with_angle(angle));
    }
}

/// A triangular arrow at `at` pointing along `dir` (unit).
fn arrow(p: &egui::Painter, at: Pos2, dir: egui::Vec2, hover: bool, k: Ink, scale: f32) {
    let side = vec2(-dir.y, dir.x);
    let pts = vec![at + dir * 7.0 * scale, at - dir * 5.0 * scale + side * 7.0 * scale, at - dir * 5.0 * scale - side * 7.0 * scale];
    p.add(egui::Shape::convex_polygon(pts, if hover { k.hover } else { k.face }, Stroke::new(1.0, if hover { k.accent } else { k.edge })));
}

/// Draws the view cube in the bottom-left corner of `rect` and takes its clicks and drags.
pub fn show(v: &mut View3d, ui: &mut egui::Ui, rect: Rect, _out: &mut Actions) {
    let t = &Tokens::get();
    let outer = Rect::from_min_size(pos2(rect.left() + 8.0, rect.bottom() - SIZE - 8.0), vec2(SIZE, SIZE));
    if !rect.contains_rect(outer) {
        return;
    }
    let scale = outer.width() / DESIGN;
    let c = outer.center();
    let cam = v.camera;
    let faces = cube::facets(&cam, emcraft_ui_core::geom::Point::new(c.x, c.y), HALF * scale);
    let hit = |q: Pos2| cube::hit(&faces, emcraft_ui_core::geom::Point::new(q.x, q.y)).and_then(|i| faces.get(i));
    let resp = ui.hit(outer, Fixed::View3dCube.into(), Sense::click_and_drag());
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let active = resp.dragged() || pointer.is_some_and(|q| outer.expand(SIZE * 0.1).contains(q));
    let k = ink(active);
    let hovered = if resp.dragged() { None } else { pointer.filter(|q| outer.contains(*q)).and_then(hit) };
    let painter = ui.painter().with_clip_rect(rect);
    paint(&painter, &faces, hovered.map(|f| f.dir), k, scale);
    // Every view shown gets a handle at its patches' middle (a face's middle; a corner's three
    // squares meet at the corner); the faces keep the standard views' names too.
    let mut seen: Vec<[i8; 3]> = Vec::new();
    for f in &faces {
        if seen.contains(&f.dir) {
            continue;
        }
        seen.push(f.dir);
        let pieces: Vec<&Facet> = faces.iter().filter(|g| g.dir == f.dir).collect();
        let at = if f.dir.iter().filter(|s| **s != 0).count() == 1 {
            pieces.iter().find(|g| g.label.is_some()).map_or(f.centre, |g| g.centre)
        } else {
            pieces.last().map_or(f.centre, |g| g.centre)
        };
        scenario::publish_handle(Handle::CubeFacet(f.dir), pos(at));
        if let Some(view) = standard_of(f.dir) {
            scenario::publish_handle(Handle::Standard(view), pos(at));
        }
    }
    if let Some(f) = hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        resp.clone().on_hover_text(trf(t!("{view} — drag to orbit, Shift snaps"), args!(view = cube::view_name(f.dir))));
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "view cube"));
    if resp.clicked()
        && let Some(f) = resp.interact_pointer_pos().and_then(hit)
    {
        v.goal = Some(cube::goal(f.dir));
    }
    // A drag on the cube orbits the view about its centre.
    if resp.dragged() {
        let d = resp.drag_delta();
        let snap = ui.input(|i| i.modifiers.shift);
        v.anim = None;
        v.goal = None;
        let mut drag = v.cube_drag.unwrap_or_else(|| Drag::new(v.camera, None));
        drag.orbit(&mut v.camera, f64::from(d.x), f64::from(d.y), snap);
        v.cube_drag = Some(drag);
        v.dragging = true;
    } else {
        v.cube_drag = None;
    }
    // The controls round it, shown while the pointer is near: four quarter turns, Home and Fit.
    for ctl in cube::controls(&faces) {
        let at = c + vec2(ctl.at.x, ctl.at.y) * scale;
        let r = Rect::from_center_size(at, vec2(ctl.size, ctl.size) * scale);
        let b = ui.hit(r, Handle::CubeButton(ctl.name), Sense::click());
        let tip = tr(ctl.tip);
        b.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
        let hover = b.hovered();
        if active || hover {
            match ctl.kind {
                Kind::Arrow(d) => arrow(&painter, at, vec2(d.x, d.y), hover, k, scale),
                Kind::Home => {
                    crate::glyphs::paint(&painter, r.shrink(3.0 * scale), crate::glyphs::Glyph::Home, if hover { k.accent } else { k.text }, k.accent)
                }
                Kind::Fit => {
                    painter.circle(at, 6.0 * scale, if hover { k.hover } else { k.face }, Stroke::new(1.0, if hover { k.accent } else { k.edge }));
                }
            }
        }
        if b.on_hover_text(tip).clicked() {
            v.goal = Some(ctl.goal);
        }
    }
    // The axes' triad below the cube.
    let (origin, tips) = cube::triad(&cam);
    let origin = c + vec2(origin.x, origin.y) * scale;
    let font = FontId::proportional(t.font_small * 0.85);
    for ((tip, color), axis) in tips.iter().zip(k.axes).zip(0..) {
        let end = origin + vec2(tip.x, tip.y) * scale;
        painter.line_segment([origin, end], Stroke::new(1.5, color));
        let label = AXES.get(axis).copied().unwrap_or("?").to_uppercase();
        painter.text(end + (end - origin).normalized() * 5.0, Align2::CENTER_CENTER, label, font.clone(), color);
    }
}

/// The standard view looking from face direction `d`.
fn standard_of(d: [i8; 3]) -> Option<emcraft_render::view3d::StandardView> {
    use emcraft_render::view3d::StandardView as S;
    Some(match d {
        [1, 0, 0] => S::Right,
        [-1, 0, 0] => S::Left,
        [0, 1, 0] => S::Top,
        [0, -1, 0] => S::Bottom,
        [0, 0, 1] => S::Front,
        [0, 0, -1] => S::Back,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_render::view3d::{Camera, StandardView};

    /// Each face's view looks at that face, as its standard view does.
    #[test]
    fn a_faces_view_is_its_standard_view() {
        for d in [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] {
            let mut want = Camera::default();
            want.standard(standard_of(d).unwrap_or(StandardView::Iso));
            let got = Camera::default().looking_from(d.map(f64::from));
            assert!(want.back().iter().zip(got.back()).all(|(a, b)| (a - b).abs() < 1e-9), "{d:?}");
        }
    }
}

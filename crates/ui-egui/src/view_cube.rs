//! The view cube in the 3D view's corner, after SolidCraft's navigation cube
//! (`crates/ui-egui/src/navigation_cube.rs` and its painter, MIT OR Apache-2.0; ATTRIBUTION.md):
//! a cube turned with the camera, each face cut into nine patches. A face's middle and its four
//! side strips turn the view to look at that face; its corner squares to look from that corner
//! (an isometric view), so a click reaches one of 14 views. Around it, while the pointer is near:
//! four arrows turning the view a quarter turn, Home (the isometric view, framed) and Fit, and a
//! small triad of the axes. A drag on the cube orbits the view (Shift snaps), the views animate,
//! and clicks in quick succession add up (each from where the last is going). Drawn in the
//! theme's colours.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use emcraft_render::view3d::{Camera, Drag};

use crate::handle::{AXES, Fixed, Handle};
use crate::lang::{args, tr, trf};
use crate::theme::Tokens;
use crate::view3d::{Goal, View3d};
use crate::{Actions, UiExt, scenario};

/// The cube's square, in points.
pub const SIZE: f32 = 112.0;
/// The design's coordinates: the square is this many units across.
const DESIGN: f32 = 164.0;
/// Half the cube's side, in design units.
const HALF: f32 = 40.0;
/// Where a face's corner squares begin (a share of the half side).
const INSET: f64 = 0.68;

/// One patch of a face as drawn: the direction it turns the view to look from, its corners on
/// screen, its middle, the face's tilt toward the eye (1 square on) and whether it is a face's
/// middle (where the label goes).
struct Facet {
    dir: [i8; 3],
    points: Vec<Pos2>,
    centre: Pos2,
    depth: f64,
    middle: bool,
    /// On a face's middle: the face's four corners on screen.
    outline: Option<Vec<Pos2>>,
}

/// The faces: the outward normal and two axes across it.
const FACES: [([f64; 3], [f64; 3], [f64; 3]); 6] = [
    ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
    ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
    ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
];

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A model direction on screen (x right, y down), `scale` points a unit.
fn project(cam: &Camera, v: [f64; 3], scale: f32) -> egui::Vec2 {
    let b = cam.basis();
    vec2(dot(v, b.right) as f32 * scale, -(dot(v, b.up) as f32) * scale)
}

/// The patches facing the eye, far first (hit-tested near first).
fn facets(cam: &Camera, c: Pos2, half: f32) -> Vec<Facet> {
    let back = cam.back();
    let mut out = Vec::new();
    for (n, r, u) in FACES {
        let depth = dot(n, back);
        if depth <= 1e-5 {
            continue;
        }
        let bands = [(-1.0, -INSET, -1.0), (-INSET, INSET, 0.0), (INSET, 1.0, 1.0)];
        for (x0, x1, sx) in bands {
            for (y0, y1, sy) in bands {
                let corner = sx != 0.0 && sy != 0.0;
                let d = if corner { [0, 1, 2].map(|i| n[i] + r[i] * sx + u[i] * sy) } else { n };
                let at = |x: f64, y: f64| c + project(cam, [0, 1, 2].map(|i| n[i] + r[i] * x + u[i] * y), half);
                let points = vec![at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)];
                let centre = at(0.5 * (x0 + x1), 0.5 * (y0 + y1));
                let middle = sx == 0.0 && sy == 0.0;
                let outline = middle.then(|| vec![at(-1.0, -1.0), at(1.0, -1.0), at(1.0, 1.0), at(-1.0, 1.0)]);
                out.push(Facet { dir: d.map(|v| v.round().clamp(-1.0, 1.0) as i8), points, centre, depth, middle, outline });
            }
        }
    }
    out.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    out
}

/// Whether `p` lies inside the convex polygon `points`.
fn contains(points: &[Pos2], p: Pos2) -> bool {
    if !p.is_finite() || points.len() < 3 {
        return false;
    }
    let mut sign = 0.0f32;
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        let cross = (b - *a).x * (p - *a).y - (b - *a).y * (p - *a).x;
        if cross.abs() > 1e-3 {
            if sign != 0.0 && sign.signum() != cross.signum() {
                return false;
            }
            sign = cross;
        }
    }
    sign != 0.0
}

/// The nearest patch under `p`.
fn hit(faces: &[Facet], p: Pos2) -> Option<usize> {
    faces.iter().enumerate().rev().find(|(_, f)| contains(&f.points, p)).map(|(i, _)| i)
}

/// The faces' names, as their labels read (translated).
pub const FACE_NAMES: [&str; 6] = ["RIGHT", "LEFT", "TOP", "BOTTOM", "FRONT", "BACK"];

/// A face's name, as its label reads (in the current language).
fn face_name(d: [i8; 3]) -> &'static str {
    match d {
        [1, 0, 0] => "RIGHT",
        [-1, 0, 0] => "LEFT",
        [0, 1, 0] => "TOP",
        [0, -1, 0] => "BOTTOM",
        [0, 0, 1] => "FRONT",
        _ => "BACK",
    }
}

/// A patch's view, in words: a face's name, or a corner's three (`TOP · FRONT · RIGHT`).
fn view_name(d: [i8; 3]) -> String {
    [1usize, 2, 0]
        .into_iter()
        .filter(|&k| d[k] != 0)
        .map(|k| {
            let mut e = [0i8; 3];
            e[k] = d[k];
            tr(face_name(e))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The cube's colours from the theme: the panel's face, the border's edges, the text's labels,
/// the accent where the pointer is; faded toward the view's ground while the pointer is away.
#[derive(Clone, Copy)]
struct Ink {
    face: Color32,
    edge: Color32,
    text: Color32,
    hover: Color32,
    accent: Color32,
    axes: [Color32; 3],
}

fn ink(t: &Tokens, active: bool) -> Ink {
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

/// Draws the patches: each filled (shaded by its tilt), the hovered view's patches in the hover
/// colour and outlined, the cube's own edges, and the faces' names on their middles.
fn paint(p: &egui::Painter, faces: &[Facet], hovered: Option<[i8; 3]>, k: Ink, t: &Tokens, scale: f32) {
    let mut mesh = egui::Mesh::default();
    for f in faces {
        let lit = hovered == Some(f.dir);
        let fill = if lit { k.hover } else { k.face.lerp_to_gamma(k.edge, ((1.0 - f.depth.min(1.0)) * 0.45) as f32) };
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(f.centre, fill);
        for q in &f.points {
            mesh.colored_vertex(*q, fill);
        }
        let n = f.points.len() as u32;
        for j in 0..n {
            mesh.add_triangle(base, base + 1 + j, base + 1 + (j + 1) % n);
        }
    }
    p.add(egui::Shape::mesh(mesh));
    // Each face's outline round its nine patches; the hovered view's patches in the accent.
    for o in faces.iter().filter_map(|f| f.outline.clone()) {
        p.add(egui::Shape::closed_line(o, Stroke::new(1.0, k.edge)));
    }
    if let Some(d) = hovered {
        for f in faces.iter().filter(|f| f.dir == d) {
            p.add(egui::Shape::closed_line(f.points.clone(), Stroke::new(1.0, k.accent)));
        }
    }
    for f in faces.iter().filter(|f| f.middle) {
        let opacity = ((f.depth - 0.18) / 0.17).clamp(0.0, 1.0) as f32;
        if opacity <= 0.0 {
            continue;
        }
        let size = (t.font_small * scale * 1.25 * (0.6 + 0.4 * f.depth as f32)).max(6.0);
        p.text(f.centre, Align2::CENTER_CENTER, tr(face_name(f.dir)), FontId::proportional(size), k.text.gamma_multiply(opacity));
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
    let faces = facets(&cam, c, HALF * scale);
    let resp = ui.hit(outer, Fixed::View3dCube.into(), Sense::click_and_drag());
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let active = resp.dragged() || pointer.is_some_and(|q| outer.contains(q));
    let k = ink(t, active);
    let hovered = if resp.dragged() { None } else { pointer.filter(|q| outer.contains(*q)).and_then(|q| hit(&faces, q)).and_then(|i| faces.get(i)) };
    let painter = ui.painter().with_clip_rect(rect);
    paint(&painter, &faces, hovered.map(|f| f.dir), k, t, scale);
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
            pieces.iter().find(|g| g.middle).map_or(f.centre, |g| g.centre)
        } else {
            // The corner's squares, nearest first: the middle of the nearest one.
            pieces.last().map_or(f.centre, |g| g.centre)
        };
        scenario::publish_handle(Handle::CubeFacet(f.dir), at);
        if let Some(view) = standard_of(f.dir) {
            scenario::publish_handle(Handle::Standard(view), at);
        }
    }
    if let Some(f) = hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        resp.clone().on_hover_text(trf("{view} — drag to orbit, Shift snaps", args!(view = view_name(f.dir))));
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "view cube"));
    if resp.clicked()
        && let Some(f) = resp.interact_pointer_pos().and_then(|q| hit(&faces, q)).and_then(|i| faces.get(i))
    {
        v.goal = Some(Goal::From(f.dir.map(f64::from)));
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
    // The buttons round it, shown while the pointer is near: four quarter turns, Home and Fit.
    let square_on = faces.iter().filter(|f| f.middle).count() == 1;
    let (distance, button) = if square_on { (54.0, 20.0) } else { (74.0, 16.0) };
    let buttons: [(&'static str, egui::Vec2, egui::Vec2, Goal, &str); 4] = [
        ("left", vec2(-distance, 0.0), vec2(1.0, 0.0), Goal::Turn { right: -1.0, up: 0.0, degrees: 90.0 }, "Orbit left 90°"),
        ("right", vec2(distance, 0.0), vec2(-1.0, 0.0), Goal::Turn { right: 1.0, up: 0.0, degrees: 90.0 }, "Orbit right 90°"),
        ("up", vec2(0.0, -distance), vec2(0.0, 1.0), Goal::Turn { right: 0.0, up: 1.0, degrees: 90.0 }, "Orbit up 90°"),
        ("down", vec2(0.0, distance), vec2(0.0, -1.0), Goal::Turn { right: 0.0, up: -1.0, degrees: 90.0 }, "Orbit down 90°"),
    ];
    for (name, offset, dir, goal, tip) in buttons {
        let at = c + offset * scale;
        let r = Rect::from_center_size(at, vec2(button, button) * scale);
        let b = ui.hit(r, Handle::CubeButton(name), Sense::click());
        let tip = tr(tip);
        b.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
        if active || b.hovered() {
            arrow(&painter, at, dir, b.hovered(), k, scale);
        }
        if b.on_hover_text(tip).clicked() {
            v.goal = Some(goal);
        }
    }
    let home = Rect::from_center_size(c + vec2(-65.0, -65.0) * scale, vec2(26.0, 26.0) * scale);
    let fit = Rect::from_center_size(c + vec2(67.0, 67.0) * scale, vec2(22.0, 22.0) * scale);
    for (name, r, goal, glyph, tip) in [
        ("home", home, Goal::Home, crate::glyphs::Glyph::Home, "Home view (isometric)"),
        ("fit", fit, Goal::Fit, crate::glyphs::Glyph::Fit, "Fit the model to the view (double-click the view)"),
    ] {
        let b = ui.hit(r, Handle::CubeButton(name), Sense::click());
        let tip = tr(tip);
        b.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
        if active || b.hovered() {
            if b.hovered() {
                painter.rect_filled(r, 4.0, k.hover);
            }
            crate::glyphs::paint(&painter, r.shrink(3.0 * scale), glyph, if b.hovered() { k.accent } else { k.text }, k.accent);
        }
        if b.on_hover_text(tip).clicked() {
            v.goal = Some(goal);
        }
    }
    // The axes' triad below the cube.
    let origin = c + vec2(-60.0, 52.0) * scale;
    let font = FontId::proportional(t.font_small * 0.85);
    for (axis, color) in k.axes.into_iter().enumerate() {
        let mut d = [0.0; 3];
        d[axis] = 1.0;
        let tip = origin + project(&cam, d, 17.0 * scale);
        painter.line_segment([origin, tip], Stroke::new(1.5, color));
        let label = AXES.get(axis).copied().unwrap_or("?").to_uppercase();
        painter.text(tip + (tip - origin).normalized() * 5.0, Align2::CENTER_CENTER, label, font.clone(), color);
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
    use emcraft_render::view3d::StandardView;

    #[test]
    fn the_cube_reaches_six_faces_and_eight_corners() {
        let mut dirs: Vec<[i8; 3]> = Vec::new();
        for view in StandardView::ALL {
            let mut cam = Camera::default();
            cam.standard(view);
            for f in facets(&cam, Pos2::ZERO, HALF) {
                let axes = f.dir.iter().filter(|s| **s != 0).count();
                assert!(axes == 1 || axes == 3, "{:?}", f.dir);
                if !dirs.contains(&f.dir) {
                    dirs.push(f.dir);
                }
            }
        }
        assert_eq!(dirs.len(), 14);
        // Each face's view looks at that face, as its standard view does.
        for d in dirs.iter().filter(|d| d.iter().filter(|s| **s != 0).count() == 1) {
            let mut want = Camera::default();
            want.standard(standard_of(*d).unwrap());
            let got = Camera::default().looking_from(d.map(f64::from));
            assert!(want.back().iter().zip(got.back()).all(|(a, b)| (a - b).abs() < 1e-9), "{d:?}");
        }
        // Square on, only the front face shows, and its middle is the cube's middle.
        let mut front = Camera::default();
        front.standard(StandardView::Front);
        let f = facets(&front, pos2(50.0, 50.0), HALF);
        assert!(f.iter().all(|g| g.dir == [0, 0, 1] || g.dir.iter().filter(|s| **s != 0).count() == 3));
        assert_eq!(hit(&f, pos2(50.0, 50.0)).map(|i| f[i].dir), Some([0, 0, 1]));
        assert_eq!(view_name([1, 1, 1]), "TOP · FRONT · RIGHT");
    }
}

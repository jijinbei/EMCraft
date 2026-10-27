//! EMCraft's command glyphs: small original line drawings painted with egui shapes (no image
//! files). Each is drawn in a square, in an ink colour with an accent for the part that matters.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

/// Paints the glyph `name` in `r`; an unknown name gets a plain rounded square.
pub fn paint(p: &Painter, r: Rect, name: &str, ink: Color32, accent: Color32) {
    let s = r.width().min(r.height());
    let c = r.center();
    // Coordinates in a unit square centred on `c`, y down.
    let at = |x: f32, y: f32| pos2(c.x + (x - 0.5) * s, c.y + (y - 0.5) * s);
    let w = (s / 16.0).max(1.0);
    let line = Stroke::new(w, ink);
    let bold = Stroke::new(w * 1.4, accent);
    let poly = |pts: &[(f32, f32)], closed: bool, stroke: Stroke| {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| at(x, y)).collect();
        if closed {
            p.add(Shape::closed_line(pts, stroke))
        } else {
            p.add(Shape::line(pts, stroke))
        };
    };
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_two_pos(at(x0, y0), at(x1, y1));
    match name {
        // A rectangular guide in perspective with its TE10 field line.
        "waveguide" => {
            poly(&[(0.12, 0.38), (0.62, 0.38), (0.62, 0.82), (0.12, 0.82)], true, line);
            poly(&[(0.12, 0.38), (0.38, 0.18), (0.88, 0.18), (0.62, 0.38)], false, line);
            poly(&[(0.88, 0.18), (0.88, 0.62), (0.62, 0.82)], false, line);
            poly(&[(0.37, 0.44), (0.37, 0.76)], false, bold);
        }
        "box" | "solid" => {
            poly(&[(0.15, 0.35), (0.6, 0.35), (0.6, 0.85), (0.15, 0.85)], true, line);
            poly(&[(0.15, 0.35), (0.4, 0.15), (0.85, 0.15), (0.6, 0.35)], true, bold);
            poly(&[(0.85, 0.15), (0.85, 0.65), (0.6, 0.85)], false, line);
        }
        "cylinder" => {
            p.add(Shape::ellipse_stroke(at(0.5, 0.25), vec2(0.3 * s, 0.1 * s), bold));
            poly(&[(0.2, 0.25), (0.2, 0.75)], false, line);
            poly(&[(0.8, 0.25), (0.8, 0.75)], false, line);
            p.add(Shape::ellipse_stroke(at(0.5, 0.75), vec2(0.3 * s, 0.1 * s), line));
        }
        "sphere" => {
            p.circle_stroke(c, 0.34 * s, line);
            p.add(Shape::ellipse_stroke(c, vec2(0.34 * s, 0.12 * s), bold));
        }
        // Two overlapping bodies: a Boolean combination.
        "feature" => {
            p.rect_stroke(rect(0.12, 0.3, 0.6, 0.82), 1.0, line, egui::StrokeKind::Middle);
            p.circle_stroke(at(0.64, 0.4), 0.22 * s, bold);
        }
        // A body inside a body.
        "inclusion" => {
            p.rect_stroke(rect(0.12, 0.15, 0.88, 0.85), 1.0, line, egui::StrokeKind::Middle);
            p.circle_filled(c, 0.17 * s, accent);
        }
        // A surface file coming in.
        "import" => {
            poly(&[(0.2, 0.55), (0.2, 0.85), (0.8, 0.85), (0.8, 0.55)], false, line);
            poly(&[(0.5, 0.12), (0.5, 0.68)], false, bold);
            poly(&[(0.32, 0.5), (0.5, 0.68), (0.68, 0.5)], false, bold);
        }
        // A material swatch: a slab with dots (polarised molecules).
        "material" => {
            p.rect_stroke(rect(0.14, 0.3, 0.86, 0.75), 2.0, line, egui::StrokeKind::Middle);
            p.rect_filled(rect(0.14, 0.52, 0.86, 0.75), 2.0, accent.gamma_multiply(0.6));
            for x in [0.3, 0.5, 0.7] {
                p.circle_filled(at(x, 0.41), 0.045 * s, ink);
            }
        }
        // A wall with hatching (finite conductivity).
        "walls" => {
            poly(&[(0.25, 0.12), (0.25, 0.88)], false, bold);
            for k in 0..5 {
                let y = 0.18 + 0.16 * k as f32;
                poly(&[(0.25, y + 0.1), (0.1, y + 0.2)], false, line);
            }
            poly(&[(0.42, 0.5), (0.86, 0.5)], false, line);
            poly(&[(0.42, 0.32), (0.86, 0.32)], false, Stroke::new(w * 0.7, ink));
            poly(&[(0.42, 0.68), (0.86, 0.68)], false, Stroke::new(w * 0.7, ink));
        }
        // A wave into an end face.
        "port" => {
            p.rect_stroke(rect(0.58, 0.18, 0.86, 0.82), 1.0, line, egui::StrokeKind::Middle);
            poly(&[(0.1, 0.5), (0.5, 0.5)], false, bold);
            poly(&[(0.38, 0.38), (0.5, 0.5), (0.38, 0.62)], false, bold);
        }
        // A gap with a source and a resistor.
        "lumped" => {
            poly(&[(0.5, 0.08), (0.5, 0.25)], false, line);
            poly(&[(0.5, 0.25), (0.62, 0.31), (0.38, 0.39), (0.62, 0.47), (0.38, 0.55), (0.5, 0.6)], false, bold);
            p.circle_stroke(at(0.5, 0.74), 0.1 * s, line);
            poly(&[(0.5, 0.84), (0.5, 0.94)], false, line);
        }
        // Two arms and radiation.
        "dipole" => {
            poly(&[(0.5, 0.12), (0.5, 0.44)], false, bold);
            poly(&[(0.5, 0.56), (0.5, 0.88)], false, bold);
            for r in [0.2, 0.32] {
                p.add(Shape::line(arc(at(0.5, 0.5), r * s, -0.7, 0.7), line));
                p.add(Shape::line(arc(at(0.5, 0.5), r * s, std::f32::consts::PI - 0.7, std::f32::consts::PI + 0.7), line));
            }
        }
        // Parallel wave fronts and a direction.
        "plane_wave" => {
            for x in [0.25, 0.45, 0.65] {
                poly(&[(x, 0.15), (x, 0.85)], false, line);
            }
            poly(&[(0.12, 0.5), (0.88, 0.5)], false, bold);
            poly(&[(0.76, 0.4), (0.88, 0.5), (0.76, 0.6)], false, bold);
        }
        // A box with an absorbing (dashed) layer.
        "open" => {
            p.rect_stroke(rect(0.3, 0.3, 0.7, 0.7), 1.0, line, egui::StrokeKind::Middle);
            let r = rect(0.1, 0.1, 0.9, 0.9);
            for (a, b) in [
                (r.left_top(), r.right_top()),
                (r.right_top(), r.right_bottom()),
                (r.right_bottom(), r.left_bottom()),
                (r.left_bottom(), r.left_top()),
            ] {
                p.extend(Shape::dashed_line(&[a, b], bold, 0.08 * s, 0.06 * s));
            }
        }
        // Triangles.
        "mesh" => {
            let v = [(0.12, 0.85), (0.5, 0.85), (0.88, 0.85), (0.3, 0.5), (0.7, 0.5), (0.5, 0.15)];
            for (a, b) in [(0, 1), (1, 2), (0, 3), (3, 1), (1, 4), (4, 2), (3, 4), (3, 5), (5, 4)] {
                poly(&[v[a], v[b]], false, line);
            }
            for &(x, y) in &v {
                p.circle_filled(at(x, y), 0.05 * s, accent);
            }
        }
        // A sweep: a play triangle over a frequency axis.
        "sweep" => {
            poly(&[(0.3, 0.18), (0.78, 0.45), (0.3, 0.72)], true, bold);
            poly(&[(0.1, 0.88), (0.9, 0.88)], false, line);
        }
        // A resonance peak.
        "eigen" => {
            poly(&[(0.08, 0.85), (0.3, 0.8), (0.42, 0.65), (0.5, 0.15), (0.58, 0.65), (0.7, 0.8), (0.92, 0.85)], false, bold);
            poly(&[(0.08, 0.9), (0.92, 0.9)], false, line);
        }
        // Radiated arcs from an antenna.
        "radiation" => {
            poly(&[(0.5, 0.55), (0.5, 0.9)], false, line);
            poly(&[(0.38, 0.9), (0.62, 0.9)], false, line);
            for r in [0.14, 0.26, 0.38] {
                p.add(Shape::line(arc(at(0.5, 0.5), r * s, -2.4, -0.74), bold));
            }
        }
        // A pulse in time.
        "time" => {
            poly(&[(0.08, 0.7), (0.3, 0.7), (0.4, 0.2), (0.5, 0.85), (0.6, 0.45), (0.68, 0.7), (0.92, 0.7)], false, bold);
            poly(&[(0.08, 0.88), (0.92, 0.88)], false, line);
        }
        // A cross-section with its mode pattern.
        "port_modes" => {
            p.rect_stroke(rect(0.12, 0.25, 0.88, 0.75), 1.0, line, egui::StrokeKind::Middle);
            for (x, h) in [(0.3, 0.12), (0.5, 0.2), (0.7, 0.12)] {
                poly(&[(x, 0.5 - h), (x, 0.5 + h)], false, bold);
            }
        }
        // A chart.
        "chart" => {
            poly(&[(0.12, 0.12), (0.12, 0.88), (0.9, 0.88)], false, line);
            poly(&[(0.16, 0.25), (0.35, 0.3), (0.5, 0.7), (0.62, 0.75), (0.75, 0.4), (0.88, 0.35)], false, bold);
        }
        // A colour map with arrows.
        "field" => {
            let steps = 5;
            for k in 0..steps {
                let t = k as f32 / (steps - 1) as f32;
                let col = accent.gamma_multiply(0.25 + 0.75 * (1.0 - (2.0 * t - 1.0).abs()));
                p.rect_filled(rect(0.1 + 0.16 * k as f32, 0.2, 0.26 + 0.16 * k as f32, 0.8), 0.0, col);
            }
            poly(&[(0.25, 0.5), (0.75, 0.5)], false, Stroke::new(w * 1.2, ink));
            poly(&[(0.65, 0.42), (0.75, 0.5), (0.65, 0.58)], false, Stroke::new(w * 1.2, ink));
        }
        // A table.
        "table" => {
            p.rect_stroke(rect(0.12, 0.18, 0.88, 0.82), 1.0, line, egui::StrokeKind::Middle);
            p.rect_filled(rect(0.12, 0.18, 0.88, 0.36), 0.0, accent.gamma_multiply(0.7));
            for y in [0.5, 0.66] {
                poly(&[(0.12, y), (0.88, y)], false, line);
            }
            poly(&[(0.42, 0.18), (0.42, 0.82)], false, line);
        }
        // Two curves on one axis.
        "compare" => {
            poly(&[(0.12, 0.12), (0.12, 0.88), (0.9, 0.88)], false, line);
            poly(&[(0.16, 0.3), (0.4, 0.5), (0.6, 0.62), (0.88, 0.7)], false, line);
            for (x, y) in [(0.25, 0.37), (0.5, 0.56), (0.75, 0.66)] {
                p.circle_filled(at(x, y), 0.06 * s, accent);
            }
        }
        // A polar lobe.
        "pattern" => {
            p.circle_stroke(c, 0.38 * s, Stroke::new(w * 0.6, ink));
            let pts: Vec<Pos2> = (0..=48)
                .map(|k| {
                    let t = k as f32 / 48.0 * std::f32::consts::TAU;
                    let r = 0.36 * s * t.sin().abs();
                    pos2(c.x + r * t.sin(), c.y - r * t.cos())
                })
                .collect();
            p.add(Shape::closed_line(pts, bold));
        }
        // A folder.
        "sample" => {
            poly(&[(0.1, 0.3), (0.38, 0.3), (0.45, 0.22), (0.9, 0.22), (0.9, 0.8), (0.1, 0.8)], true, line);
            poly(&[(0.1, 0.4), (0.9, 0.4)], false, bold);
        }
        "units" => {
            poly(&[(0.1, 0.62), (0.9, 0.62)], false, line);
            for k in 0..9 {
                let x = 0.1 + 0.1 * k as f32;
                poly(&[(x, 0.62), (x, if k % 4 == 0 { 0.36 } else { 0.5 })], false, if k % 4 == 0 { bold } else { line });
            }
        }
        "clear" => {
            poly(&[(0.22, 0.22), (0.78, 0.78)], false, bold);
            poly(&[(0.78, 0.22), (0.22, 0.78)], false, bold);
        }
        "summary" => {
            for (k, y) in [0.25, 0.45, 0.65].into_iter().enumerate() {
                p.circle_filled(at(0.18, y), 0.05 * s, accent);
                poly(&[(0.3, y), (if k == 2 { 0.65 } else { 0.85 }, y)], false, line);
            }
        }
        "probe" => {
            poly(&[(0.2, 0.8), (0.62, 0.38)], false, line);
            p.circle_stroke(at(0.68, 0.32), 0.14 * s, bold);
        }
        "script" => {
            p.rect_stroke(rect(0.18, 0.12, 0.82, 0.88), 2.0, line, egui::StrokeKind::Middle);
            for (y, x1) in [(0.3, 0.7), (0.45, 0.6), (0.6, 0.72), (0.75, 0.5)] {
                poly(&[(0.3, y), (x1, y)], false, bold);
            }
        }
        // Four corners closing in on a box: fit the model to the view.
        "fit" => {
            for (x, y, dx, dy) in [(0.12, 0.12, 1.0, 1.0), (0.88, 0.12, -1.0, 1.0), (0.88, 0.88, -1.0, -1.0), (0.12, 0.88, 1.0, -1.0)] {
                poly(&[(x, y + 0.22 * dy), (x, y), (x + 0.22 * dx, y)], false, line);
            }
            p.rect_filled(rect(0.36, 0.36, 0.64, 0.64), 1.5, accent);
        }
        // A cube seen from a corner: the home (isometric) view.
        "home" | "iso" => {
            let top = [(0.5, 0.14), (0.84, 0.32), (0.5, 0.5), (0.16, 0.32)];
            p.add(Shape::convex_polygon(top.iter().map(|&(x, y)| at(x, y)).collect(), accent.gamma_multiply(0.35), bold));
            poly(&[(0.16, 0.32), (0.16, 0.7), (0.5, 0.88), (0.84, 0.7), (0.84, 0.32)], false, line);
            poly(&[(0.5, 0.5), (0.5, 0.88)], false, line);
        }
        // An eye on a plane seen square on: face the field's plane.
        "lookat" => {
            p.add(Shape::convex_polygon(
                [(0.42, 0.18), (0.88, 0.18), (0.88, 0.82), (0.42, 0.82)].iter().map(|&(x, y)| at(x, y)).collect(),
                accent.gamma_multiply(0.3),
                bold,
            ));
            poly(&[(0.08, 0.5), (0.36, 0.5)], false, line);
            poly(&[(0.26, 0.4), (0.36, 0.5), (0.26, 0.6)], false, line);
        }
        // Converging (perspective) or parallel (orthographic) edges of a box.
        "perspective" => {
            poly(&[(0.3, 0.3), (0.7, 0.3), (0.7, 0.7), (0.3, 0.7)], true, bold);
            poly(&[(0.1, 0.12), (0.3, 0.3)], false, line);
            poly(&[(0.9, 0.12), (0.7, 0.3)], false, line);
            poly(&[(0.9, 0.88), (0.7, 0.7)], false, line);
            poly(&[(0.1, 0.88), (0.3, 0.7)], false, line);
        }
        "orthographic" => {
            poly(&[(0.12, 0.3), (0.62, 0.3), (0.62, 0.84), (0.12, 0.84)], true, line);
            poly(&[(0.38, 0.14), (0.88, 0.14), (0.88, 0.68), (0.38, 0.68)], true, bold);
            for (x, y) in [(0.12, 0.3), (0.62, 0.3), (0.62, 0.84), (0.12, 0.84)] {
                poly(&[(x, y), (x + 0.26, y - 0.16)], false, line);
            }
        }
        // A page with a folded corner and a plus.
        "new" => {
            poly(&[(0.22, 0.1), (0.6, 0.1), (0.78, 0.28), (0.78, 0.9), (0.22, 0.9)], true, line);
            poly(&[(0.6, 0.1), (0.6, 0.28), (0.78, 0.28)], false, line);
            poly(&[(0.5, 0.44), (0.5, 0.76)], false, bold);
            poly(&[(0.34, 0.6), (0.66, 0.6)], false, bold);
        }
        // An open folder.
        "open_file" => {
            poly(&[(0.08, 0.82), (0.08, 0.22), (0.36, 0.22), (0.44, 0.3), (0.82, 0.3), (0.82, 0.42)], false, line);
            p.add(Shape::convex_polygon(
                [(0.08, 0.82), (0.24, 0.42), (0.94, 0.42), (0.78, 0.82)].iter().map(|&(x, y)| at(x, y)).collect(),
                accent.gamma_multiply(0.3),
                bold,
            ));
        }
        // A floppy disk.
        "save" => {
            poly(&[(0.14, 0.14), (0.74, 0.14), (0.86, 0.26), (0.86, 0.86), (0.14, 0.86)], true, line);
            p.rect_stroke(rect(0.3, 0.14, 0.66, 0.36), 1.0, line, egui::StrokeKind::Middle);
            p.rect_filled(rect(0.28, 0.54, 0.72, 0.86), 1.5, accent);
        }
        // A gear.
        "settings" => {
            let teeth: Vec<Pos2> = (0..48)
                .map(|k| {
                    let a = k as f32 / 48.0 * std::f32::consts::TAU;
                    let r = if (k / 3) % 2 == 0 { 0.42 } else { 0.32 } * s;
                    pos2(c.x + r * a.cos(), c.y + r * a.sin())
                })
                .collect();
            p.add(Shape::closed_line(teeth, line));
            p.circle_stroke(c, 0.13 * s, bold);
        }
        // A band marked on a frequency axis.
        "band" => {
            poly(&[(0.08, 0.78), (0.92, 0.78)], false, line);
            p.rect_filled(rect(0.3, 0.3, 0.7, 0.72), 1.5, accent.gamma_multiply(0.35));
            poly(&[(0.3, 0.86), (0.3, 0.26)], false, bold);
            poly(&[(0.7, 0.86), (0.7, 0.26)], false, bold);
        }
        // A tick in a circle: the results' checks.
        "check" => {
            p.circle_stroke(c, 0.38 * s, line);
            poly(&[(0.3, 0.52), (0.45, 0.66), (0.72, 0.36)], false, bold);
        }
        // A page of numbers: a Touchstone file.
        "touchstone" => {
            poly(&[(0.2, 0.1), (0.62, 0.1), (0.8, 0.28), (0.8, 0.9), (0.2, 0.9)], true, line);
            for y in [0.38, 0.52, 0.66, 0.8] {
                poly(&[(0.3, y), (0.46, y)], false, bold);
                poly(&[(0.54, y), (0.7, y)], false, line);
            }
        }
        // A plane with a plus: keep it.
        "keep_plane" => {
            p.add(Shape::convex_polygon(
                [(0.1, 0.35), (0.6, 0.2), (0.6, 0.8), (0.1, 0.95)].iter().map(|&(x, y)| at(x, y)).collect(),
                accent.gamma_multiply(0.3),
                line,
            ));
            poly(&[(0.78, 0.08), (0.78, 0.44)], false, bold);
            poly(&[(0.6, 0.26), (0.96, 0.26)], false, bold);
        }
        // A shape and its mirror image across a dashed plane.
        "symmetry" => {
            for k in 0..5 {
                let y0 = 0.08 + 0.18 * k as f32;
                poly(&[(0.5, y0), (0.5, y0 + 0.1)], false, bold);
            }
            poly(&[(0.42, 0.2), (0.12, 0.5), (0.42, 0.8)], true, line);
            p.add(Shape::convex_polygon(
                [(0.58, 0.2), (0.88, 0.5), (0.58, 0.8)].iter().map(|&(x, y)| at(x, y)).collect(),
                accent.gamma_multiply(0.3),
                line,
            ));
        }
        // A house: the start page.
        "start" => {
            poly(&[(0.1, 0.5), (0.5, 0.14), (0.9, 0.5)], false, bold);
            poly(&[(0.2, 0.42), (0.2, 0.86), (0.8, 0.86), (0.8, 0.42)], false, line);
            p.rect_filled(rect(0.42, 0.6, 0.58, 0.86), 1.0, accent);
        }
        _ => {
            p.rect_stroke(rect(0.2, 0.2, 0.8, 0.8), 3.0, line, egui::StrokeKind::Middle);
        }
    }
}

/// Points along an arc about `c` of radius `r` from angle `a0` to `a1` (radians, 0 along +x,
/// growing clockwise on screen).
fn arc(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    (0..=16).map(|k| a0 + (a1 - a0) * k as f32 / 16.0).map(|a| pos2(c.x + r * a.cos(), c.y + r * a.sin())).collect()
}

/// The glyph of a command.
pub fn for_command(id: &str) -> &'static str {
    match id {
        "geometry.waveguide" => "waveguide",
        "geometry.solid" => "box",
        "geometry.feature" => "feature",
        "geometry.inclusion" => "inclusion",
        "geometry.import" => "import",
        "model.material.create" => "material",
        "model.walls" => "walls",
        "model.symmetry" => "symmetry",
        "model.port.create" => "port",
        "model.port.lumped" => "lumped",
        "source.dipole" => "dipole",
        "source.plane_wave" => "plane_wave",
        "source.clear" => "clear",
        "model.open" => "open",
        "mesh.generate" => "mesh",
        "solve.frequency" => "sweep",
        "solve.eigenmodes" | "results.eigenmodes" => "eigen",
        "solve.radiation" => "radiation",
        "solve.time_domain" => "time",
        "solve.port_modes" => "port_modes",
        "results.sparameters" | "view.sparameters" => "chart",
        "results.compare" | "results.field_error" => "compare",
        "results.far_field" | "view.pattern" => "pattern",
        "results.probe" => "probe",
        "view.field" => "field",
        "file.sample" => "sample",
        "units.set" => "units",
        "project.frequency" => "band",
        "results.check" => "check",
        "results.touchstone" => "touchstone",
        "file.new" => "new",
        "file.open" => "open_file",
        "file.save" => "save",
        "model.summary" => "summary",
        _ => "",
    }
}

//! EMCraft's command glyphs: small original line drawings painted with egui shapes (no image
//! files). Each is drawn in a square, in an ink colour with an accent for the part that matters.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

emcraft_macros::named_enum! {
    /// A glyph; its id names it (a picture sheet, a test).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Glyph {
        Waveguide = "waveguide",
        Box = "box",
        Cylinder = "cylinder",
        Sphere = "sphere",
        Feature = "feature",
        Inclusion = "inclusion",
        Import = "import",
        Material = "material",
        Walls = "walls",
        Port = "port",
        Lumped = "lumped",
        Dipole = "dipole",
        PlaneWave = "plane_wave",
        Open = "open",
        Mesh = "mesh",
        Sweep = "sweep",
        Variable = "variable",
        ParamSweep = "param_sweep",
        Optimize = "optimize",
        Eigen = "eigen",
        Radiation = "radiation",
        Time = "time",
        PortModes = "port_modes",
        Chart = "chart",
        Field = "field",
        Table = "table",
        Compare = "compare",
        Pattern = "pattern",
        Sample = "sample",
        Units = "units",
        Clear = "clear",
        Summary = "summary",
        Probe = "probe",
        Script = "script",
        Fit = "fit",
        Home = "home",
        Lookat = "lookat",
        Perspective = "perspective",
        Orthographic = "orthographic",
        New = "new",
        OpenFile = "open_file",
        Save = "save",
        Settings = "settings",
        Band = "band",
        Check = "check",
        Touchstone = "touchstone",
        KeepPlane = "keep_plane",
        Symmetry = "symmetry",
        Periodic = "periodic",
        Dispersion = "dispersion",
        Start = "start",
        Play = "play",
        Pause = "pause",
        /// A box cut open, its cut face filled: the section view.
        Section = "section",
        /// Two arrows turning back on each other: the other side.
        Flip = "flip",
        /// A ruler with its ticks: measure.
        Ruler = "ruler",
        /// A pencil: edit.
        Edit = "edit",
        /// A bin: delete.
        Delete = "delete",
        /// A face picked by a pointer: a port on a face.
        PickFace = "pick_face",
        /// Two points joined across a gap: a lumped port by two points.
        PickGap = "pick_gap",
    /// A plain rounded square: a command without a glyph of its own.
        Blank = "blank",
    }
}

/// Paints `glyph` in `r`.
pub fn paint(p: &Painter, r: Rect, glyph: Glyph, ink: Color32, accent: Color32) {
    let pen = Pen::new(p, r);
    let (s, c) = (pen.s, pen.c);
    let w = pen.w();
    let line = Stroke::new(w, ink);
    let bold = Stroke::new(w * 1.4, accent);
    match glyph {
        // A rectangular guide in perspective with its TE10 field line.
        Glyph::Waveguide => {
            pen.closed(&[(0.12, 0.38), (0.62, 0.38), (0.62, 0.82), (0.12, 0.82)], line);
            pen.open(&[(0.12, 0.38), (0.38, 0.18), (0.88, 0.18), (0.62, 0.38)], line);
            pen.open(&[(0.88, 0.18), (0.88, 0.62), (0.62, 0.82)], line);
            pen.open(&[(0.37, 0.44), (0.37, 0.76)], bold);
        }
        Glyph::Box => {
            pen.closed(&[(0.15, 0.35), (0.6, 0.35), (0.6, 0.85), (0.15, 0.85)], line);
            pen.closed(&[(0.15, 0.35), (0.4, 0.15), (0.85, 0.15), (0.6, 0.35)], bold);
            pen.open(&[(0.85, 0.15), (0.85, 0.65), (0.6, 0.85)], line);
        }
        Glyph::Cylinder => {
            p.add(Shape::ellipse_stroke(pen.at(0.5, 0.25), vec2(0.3 * s, 0.1 * s), bold));
            pen.open(&[(0.2, 0.25), (0.2, 0.75)], line);
            pen.open(&[(0.8, 0.25), (0.8, 0.75)], line);
            p.add(Shape::ellipse_stroke(pen.at(0.5, 0.75), vec2(0.3 * s, 0.1 * s), line));
        }
        Glyph::Sphere => {
            p.circle_stroke(c, 0.34 * s, line);
            p.add(Shape::ellipse_stroke(c, vec2(0.34 * s, 0.12 * s), bold));
        }
        // Two overlapping bodies: a Boolean combination.
        Glyph::Feature => {
            pen.frame(pen.rect(0.12, 0.3, 0.6, 0.82), 1.0, line);
            p.circle_stroke(pen.at(0.64, 0.4), 0.22 * s, bold);
        }
        // A body inside a body.
        Glyph::Inclusion => {
            pen.frame(pen.rect(0.12, 0.15, 0.88, 0.85), 1.0, line);
            p.circle_filled(c, 0.17 * s, accent);
        }
        // A surface file coming in.
        Glyph::Import => {
            pen.open(&[(0.2, 0.55), (0.2, 0.85), (0.8, 0.85), (0.8, 0.55)], line);
            pen.open(&[(0.5, 0.12), (0.5, 0.68)], bold);
            pen.open(&[(0.32, 0.5), (0.5, 0.68), (0.68, 0.5)], bold);
        }
        // A material swatch: a slab with dots (polarised molecules).
        Glyph::Material => {
            pen.frame(pen.rect(0.14, 0.3, 0.86, 0.75), 2.0, line);
            p.rect_filled(pen.rect(0.14, 0.52, 0.86, 0.75), 2.0, accent.gamma_multiply(0.6));
            for x in [0.3, 0.5, 0.7] {
                p.circle_filled(pen.at(x, 0.41), 0.045 * s, ink);
            }
        }
        // A wall with hatching (finite conductivity).
        Glyph::Walls => {
            pen.open(&[(0.25, 0.12), (0.25, 0.88)], bold);
            for k in 0..5 {
                let y = 0.18 + 0.16 * k as f32;
                pen.open(&[(0.25, y + 0.1), (0.1, y + 0.2)], line);
            }
            pen.open(&[(0.42, 0.5), (0.86, 0.5)], line);
            pen.open(&[(0.42, 0.32), (0.86, 0.32)], Stroke::new(w * 0.7, ink));
            pen.open(&[(0.42, 0.68), (0.86, 0.68)], Stroke::new(w * 0.7, ink));
        }
        // A wave into an end face.
        Glyph::Port => {
            pen.frame(pen.rect(0.58, 0.18, 0.86, 0.82), 1.0, line);
            pen.open(&[(0.1, 0.5), (0.5, 0.5)], bold);
            pen.open(&[(0.38, 0.38), (0.5, 0.5), (0.38, 0.62)], bold);
        }
        // A gap with a source and a resistor.
        Glyph::Lumped => {
            pen.open(&[(0.5, 0.08), (0.5, 0.25)], line);
            pen.open(&[(0.5, 0.25), (0.62, 0.31), (0.38, 0.39), (0.62, 0.47), (0.38, 0.55), (0.5, 0.6)], bold);
            p.circle_stroke(pen.at(0.5, 0.74), 0.1 * s, line);
            pen.open(&[(0.5, 0.84), (0.5, 0.94)], line);
        }
        // Two arms and radiation.
        Glyph::Dipole => {
            pen.open(&[(0.5, 0.12), (0.5, 0.44)], bold);
            pen.open(&[(0.5, 0.56), (0.5, 0.88)], bold);
            for r in [0.2, 0.32] {
                p.add(Shape::line(arc(pen.at(0.5, 0.5), r * s, -0.7, 0.7), line));
                p.add(Shape::line(arc(pen.at(0.5, 0.5), r * s, std::f32::consts::PI - 0.7, std::f32::consts::PI + 0.7), line));
            }
        }
        // Parallel wave fronts and a direction.
        Glyph::PlaneWave => {
            for x in [0.25, 0.45, 0.65] {
                pen.open(&[(x, 0.15), (x, 0.85)], line);
            }
            pen.open(&[(0.12, 0.5), (0.88, 0.5)], bold);
            pen.open(&[(0.76, 0.4), (0.88, 0.5), (0.76, 0.6)], bold);
        }
        // A box with an absorbing (dashed) layer.
        Glyph::Open => {
            pen.frame(pen.rect(0.3, 0.3, 0.7, 0.7), 1.0, line);
            let r = pen.rect(0.1, 0.1, 0.9, 0.9);
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
        Glyph::Mesh => {
            let v = [(0.12, 0.85), (0.5, 0.85), (0.88, 0.85), (0.3, 0.5), (0.7, 0.5), (0.5, 0.15)];
            for (a, b) in [(0, 1), (1, 2), (0, 3), (3, 1), (1, 4), (4, 2), (3, 4), (3, 5), (5, 4)] {
                pen.open(&[v[a], v[b]], line);
            }
            for &(x, y) in &v {
                p.circle_filled(pen.at(x, y), 0.05 * s, accent);
            }
        }
        // A sweep: a play triangle over a frequency axis.
        Glyph::Sweep => {
            pen.closed(&[(0.3, 0.18), (0.78, 0.45), (0.3, 0.72)], bold);
            pen.open(&[(0.1, 0.88), (0.9, 0.88)], line);
        }
        // A named value: a dimension line between two ticks, its value a slider's knob.
        Glyph::Variable => {
            pen.open(&[(0.14, 0.3), (0.14, 0.7)], line);
            pen.open(&[(0.86, 0.3), (0.86, 0.7)], line);
            pen.open(&[(0.14, 0.5), (0.86, 0.5)], line);
            p.circle_filled(pen.at(0.58, 0.5), 0.1 * s, accent);
            pen.open(&[(0.3, 0.82), (0.7, 0.82)], bold);
        }
        // A family of curves, one per value of a variable.
        Glyph::ParamSweep => {
            pen.open(&[(0.1, 0.9), (0.9, 0.9)], line);
            pen.open(&[(0.1, 0.9), (0.1, 0.1)], line);
            pen.open(&[(0.18, 0.75), (0.4, 0.6), (0.62, 0.55), (0.88, 0.25)], line);
            pen.open(&[(0.18, 0.62), (0.4, 0.45), (0.62, 0.42), (0.88, 0.15)], bold);
            pen.open(&[(0.18, 0.82), (0.4, 0.74), (0.62, 0.7), (0.88, 0.45)], line);
        }
        // A valley with the point found at its bottom.
        Glyph::Optimize => {
            pen.open(&[(0.08, 0.2), (0.3, 0.62), (0.5, 0.78), (0.7, 0.62), (0.92, 0.2)], line);
            p.circle_filled(pen.at(0.5, 0.78), 0.09 * s, accent);
            pen.open(&[(0.2, 0.92), (0.8, 0.92)], bold);
        }
        // A resonance peak.
        Glyph::Eigen => {
            pen.open(&[(0.08, 0.85), (0.3, 0.8), (0.42, 0.65), (0.5, 0.15), (0.58, 0.65), (0.7, 0.8), (0.92, 0.85)], bold);
            pen.open(&[(0.08, 0.9), (0.92, 0.9)], line);
        }
        // Radiated arcs from an antenna.
        Glyph::Radiation => {
            pen.open(&[(0.5, 0.55), (0.5, 0.9)], line);
            pen.open(&[(0.38, 0.9), (0.62, 0.9)], line);
            for r in [0.14, 0.26, 0.38] {
                p.add(Shape::line(arc(pen.at(0.5, 0.5), r * s, -2.4, -0.74), bold));
            }
        }
        // A pulse in time.
        Glyph::Time => {
            pen.open(&[(0.08, 0.7), (0.3, 0.7), (0.4, 0.2), (0.5, 0.85), (0.6, 0.45), (0.68, 0.7), (0.92, 0.7)], bold);
            pen.open(&[(0.08, 0.88), (0.92, 0.88)], line);
        }
        // A cross-section with its mode pattern.
        Glyph::PortModes => {
            pen.frame(pen.rect(0.12, 0.25, 0.88, 0.75), 1.0, line);
            for (x, h) in [(0.3, 0.12), (0.5, 0.2), (0.7, 0.12)] {
                pen.open(&[(x, 0.5 - h), (x, 0.5 + h)], bold);
            }
        }
        // A chart.
        Glyph::Chart => {
            pen.open(&[(0.12, 0.12), (0.12, 0.88), (0.9, 0.88)], line);
            pen.open(&[(0.16, 0.25), (0.35, 0.3), (0.5, 0.7), (0.62, 0.75), (0.75, 0.4), (0.88, 0.35)], bold);
        }
        // A colour map with arrows.
        Glyph::Field => {
            let steps = 5;
            for k in 0..steps {
                let t = k as f32 / (steps - 1) as f32;
                let col = accent.gamma_multiply(0.25 + 0.75 * (1.0 - (2.0 * t - 1.0).abs()));
                p.rect_filled(pen.rect(0.1 + 0.16 * k as f32, 0.2, 0.26 + 0.16 * k as f32, 0.8), 0.0, col);
            }
            pen.open(&[(0.25, 0.5), (0.75, 0.5)], Stroke::new(w * 1.2, ink));
            pen.open(&[(0.65, 0.42), (0.75, 0.5), (0.65, 0.58)], Stroke::new(w * 1.2, ink));
        }
        // A table.
        Glyph::Table => {
            pen.frame(pen.rect(0.12, 0.18, 0.88, 0.82), 1.0, line);
            p.rect_filled(pen.rect(0.12, 0.18, 0.88, 0.36), 0.0, accent.gamma_multiply(0.7));
            for y in [0.5, 0.66] {
                pen.open(&[(0.12, y), (0.88, y)], line);
            }
            pen.open(&[(0.42, 0.18), (0.42, 0.82)], line);
        }
        // Two curves on one axis.
        Glyph::Compare => {
            pen.open(&[(0.12, 0.12), (0.12, 0.88), (0.9, 0.88)], line);
            pen.open(&[(0.16, 0.3), (0.4, 0.5), (0.6, 0.62), (0.88, 0.7)], line);
            for (x, y) in [(0.25, 0.37), (0.5, 0.56), (0.75, 0.66)] {
                p.circle_filled(pen.at(x, y), 0.06 * s, accent);
            }
        }
        // A polar lobe.
        Glyph::Pattern => {
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
        Glyph::Sample => {
            pen.closed(&[(0.1, 0.3), (0.38, 0.3), (0.45, 0.22), (0.9, 0.22), (0.9, 0.8), (0.1, 0.8)], line);
            pen.open(&[(0.1, 0.4), (0.9, 0.4)], bold);
        }
        Glyph::Units => {
            pen.open(&[(0.1, 0.62), (0.9, 0.62)], line);
            for k in 0..9 {
                let x = 0.1 + 0.1 * k as f32;
                pen.open(&[(x, 0.62), (x, if k % 4 == 0 { 0.36 } else { 0.5 })], if k % 4 == 0 { bold } else { line });
            }
        }
        Glyph::Clear => {
            pen.open(&[(0.22, 0.22), (0.78, 0.78)], bold);
            pen.open(&[(0.78, 0.22), (0.22, 0.78)], bold);
        }
        Glyph::Summary => {
            for (k, y) in [0.25, 0.45, 0.65].into_iter().enumerate() {
                p.circle_filled(pen.at(0.18, y), 0.05 * s, accent);
                pen.open(&[(0.3, y), (if k == 2 { 0.65 } else { 0.85 }, y)], line);
            }
        }
        Glyph::Probe => {
            pen.open(&[(0.2, 0.8), (0.62, 0.38)], line);
            p.circle_stroke(pen.at(0.68, 0.32), 0.14 * s, bold);
        }
        Glyph::Script => {
            pen.frame(pen.rect(0.18, 0.12, 0.82, 0.88), 2.0, line);
            for (y, x1) in [(0.3, 0.7), (0.45, 0.6), (0.6, 0.72), (0.75, 0.5)] {
                pen.open(&[(0.3, y), (x1, y)], bold);
            }
        }
        // Four corners closing in on a box: fit the model to the view.
        Glyph::Fit => {
            for (x, y, dx, dy) in [(0.12, 0.12, 1.0, 1.0), (0.88, 0.12, -1.0, 1.0), (0.88, 0.88, -1.0, -1.0), (0.12, 0.88, 1.0, -1.0)] {
                pen.open(&[(x, y + 0.22 * dy), (x, y), (x + 0.22 * dx, y)], line);
            }
            p.rect_filled(pen.rect(0.36, 0.36, 0.64, 0.64), 1.5, accent);
        }
        // A cube seen from a corner: the home (isometric) view.
        Glyph::Home => {
            let top = [(0.5, 0.14), (0.84, 0.32), (0.5, 0.5), (0.16, 0.32)];
            pen.fill(&top, accent.gamma_multiply(0.35), bold);
            pen.open(&[(0.16, 0.32), (0.16, 0.7), (0.5, 0.88), (0.84, 0.7), (0.84, 0.32)], line);
            pen.open(&[(0.5, 0.5), (0.5, 0.88)], line);
        }
        // An eye on a plane seen square on: face the field's plane.
        Glyph::Lookat => {
            pen.fill(&[(0.42, 0.18), (0.88, 0.18), (0.88, 0.82), (0.42, 0.82)], accent.gamma_multiply(0.3), bold);
            pen.open(&[(0.08, 0.5), (0.36, 0.5)], line);
            pen.open(&[(0.26, 0.4), (0.36, 0.5), (0.26, 0.6)], line);
        }
        // Converging (perspective) or parallel (orthographic) edges of a box.
        Glyph::Perspective => {
            pen.closed(&[(0.3, 0.3), (0.7, 0.3), (0.7, 0.7), (0.3, 0.7)], bold);
            pen.open(&[(0.1, 0.12), (0.3, 0.3)], line);
            pen.open(&[(0.9, 0.12), (0.7, 0.3)], line);
            pen.open(&[(0.9, 0.88), (0.7, 0.7)], line);
            pen.open(&[(0.1, 0.88), (0.3, 0.7)], line);
        }
        Glyph::Orthographic => {
            pen.closed(&[(0.12, 0.3), (0.62, 0.3), (0.62, 0.84), (0.12, 0.84)], line);
            pen.closed(&[(0.38, 0.14), (0.88, 0.14), (0.88, 0.68), (0.38, 0.68)], bold);
            for (x, y) in [(0.12, 0.3), (0.62, 0.3), (0.62, 0.84), (0.12, 0.84)] {
                pen.open(&[(x, y), (x + 0.26, y - 0.16)], line);
            }
        }
        // A page with a folded corner and a plus.
        Glyph::New => {
            pen.closed(&[(0.22, 0.1), (0.6, 0.1), (0.78, 0.28), (0.78, 0.9), (0.22, 0.9)], line);
            pen.open(&[(0.6, 0.1), (0.6, 0.28), (0.78, 0.28)], line);
            pen.open(&[(0.5, 0.44), (0.5, 0.76)], bold);
            pen.open(&[(0.34, 0.6), (0.66, 0.6)], bold);
        }
        // An open folder.
        Glyph::OpenFile => {
            pen.open(&[(0.08, 0.82), (0.08, 0.22), (0.36, 0.22), (0.44, 0.3), (0.82, 0.3), (0.82, 0.42)], line);
            pen.fill(&[(0.08, 0.82), (0.24, 0.42), (0.94, 0.42), (0.78, 0.82)], accent.gamma_multiply(0.3), bold);
        }
        // A floppy disk.
        Glyph::Save => {
            pen.closed(&[(0.14, 0.14), (0.74, 0.14), (0.86, 0.26), (0.86, 0.86), (0.14, 0.86)], line);
            pen.frame(pen.rect(0.3, 0.14, 0.66, 0.36), 1.0, line);
            p.rect_filled(pen.rect(0.28, 0.54, 0.72, 0.86), 1.5, accent);
        }
        // A gear.
        Glyph::Settings => {
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
        Glyph::Band => {
            pen.open(&[(0.08, 0.78), (0.92, 0.78)], line);
            p.rect_filled(pen.rect(0.3, 0.3, 0.7, 0.72), 1.5, accent.gamma_multiply(0.35));
            pen.open(&[(0.3, 0.86), (0.3, 0.26)], bold);
            pen.open(&[(0.7, 0.86), (0.7, 0.26)], bold);
        }
        // A tick in a circle: the results' checks.
        Glyph::Check => {
            p.circle_stroke(c, 0.38 * s, line);
            pen.open(&[(0.3, 0.52), (0.45, 0.66), (0.72, 0.36)], bold);
        }
        // A page of numbers: a Touchstone file.
        Glyph::Touchstone => {
            pen.closed(&[(0.2, 0.1), (0.62, 0.1), (0.8, 0.28), (0.8, 0.9), (0.2, 0.9)], line);
            for y in [0.38, 0.52, 0.66, 0.8] {
                pen.open(&[(0.3, y), (0.46, y)], bold);
                pen.open(&[(0.54, y), (0.7, y)], line);
            }
        }
        // A plane with a plus: keep it.
        Glyph::KeepPlane => {
            pen.fill(&[(0.1, 0.35), (0.6, 0.2), (0.6, 0.8), (0.1, 0.95)], accent.gamma_multiply(0.3), line);
            pen.open(&[(0.78, 0.08), (0.78, 0.44)], bold);
            pen.open(&[(0.6, 0.26), (0.96, 0.26)], bold);
        }
        // A shape and its mirror image across a dashed plane.
        Glyph::Symmetry => {
            for k in 0..5 {
                let y0 = 0.08 + 0.18 * k as f32;
                pen.open(&[(0.5, y0), (0.5, y0 + 0.1)], bold);
            }
            pen.closed(&[(0.42, 0.2), (0.12, 0.5), (0.42, 0.8)], line);
            pen.fill(&[(0.58, 0.2), (0.88, 0.5), (0.58, 0.8)], accent.gamma_multiply(0.3), line);
        }
        // Three cells of a lattice, the middle one's sides dashed: periodic boundaries.
        Glyph::Periodic => {
            for (x0, x1) in [(0.06, 0.34), (0.66, 0.94)] {
                pen.closed(&[(x0, 0.3), (x1, 0.3), (x1, 0.7), (x0, 0.7)], line);
            }
            pen.fill(&[(0.36, 0.3), (0.64, 0.3), (0.64, 0.7), (0.36, 0.7)], accent.gamma_multiply(0.3), line);
            for x in [0.36, 0.64] {
                for k in 0..3 {
                    let y0 = 0.16 + 0.26 * k as f32;
                    pen.open(&[(x, y0), (x, y0 + 0.12)], bold);
                }
            }
        }
        // A dispersion diagram: two bands over the phase, and the light line.
        Glyph::Dispersion => {
            pen.open(&[(0.12, 0.1), (0.12, 0.88), (0.9, 0.88)], line);
            pen.open(&[(0.12, 0.88), (0.86, 0.14)], line);
            let lower: Vec<(f32, f32)> = (0..=8).map(|k| k as f32 / 8.0).map(|u| (0.12 + 0.76 * u, 0.72 - 0.3 * u * u)).collect();
            let upper: Vec<(f32, f32)> = (0..=8).map(|k| k as f32 / 8.0).map(|u| (0.12 + 0.76 * u, 0.3 + 0.12 * u * u)).collect();
            pen.open(&lower, Stroke::new(w * 1.4, accent));
            pen.open(&upper, Stroke::new(w * 1.4, accent));
        }
        // A house: the start page.
        Glyph::Start => {
            pen.open(&[(0.1, 0.5), (0.5, 0.14), (0.9, 0.5)], bold);
            pen.open(&[(0.2, 0.42), (0.2, 0.86), (0.8, 0.86), (0.8, 0.42)], line);
            p.rect_filled(pen.rect(0.42, 0.6, 0.58, 0.86), 1.0, accent);
        }
        // A video's play and pause.
        Glyph::Play => pen.fill(&[(0.3, 0.18), (0.82, 0.5), (0.3, 0.82)], accent, Stroke::new(w, accent)),
        Glyph::Pause => {
            p.rect_filled(pen.rect(0.26, 0.2, 0.42, 0.8), 1.0, accent);
            p.rect_filled(pen.rect(0.58, 0.2, 0.74, 0.8), 1.0, accent);
        }
        Glyph::Section => {
            pen.open(&[(0.5, 0.35), (0.15, 0.35), (0.15, 0.85), (0.5, 0.85)], line);
            pen.open(&[(0.15, 0.35), (0.4, 0.15), (0.75, 0.15)], line);
            pen.fill(&[(0.5, 0.35), (0.75, 0.15), (0.75, 0.65), (0.5, 0.85)], accent.gamma_multiply(0.45), bold);
        }
        Glyph::Flip => {
            pen.open(&[(0.15, 0.35), (0.85, 0.35)], line);
            pen.open(&[(0.68, 0.2), (0.85, 0.35), (0.68, 0.5)], line);
            pen.open(&[(0.85, 0.65), (0.15, 0.65)], bold);
            pen.open(&[(0.32, 0.5), (0.15, 0.65), (0.32, 0.8)], bold);
        }
        Glyph::Ruler => {
            pen.closed(&[(0.08, 0.62), (0.62, 0.08), (0.92, 0.38), (0.38, 0.92)], line);
            for k in 0..5 {
                let (x, y) = (0.22 + 0.11 * k as f32, 0.48 - 0.11 * k as f32);
                let len = if k % 2 == 0 { 0.16 } else { 0.09 };
                pen.open(&[(x, y), (x + len, y + len)], bold);
            }
        }
        Glyph::Edit => {
            pen.closed(&[(0.18, 0.66), (0.62, 0.22), (0.78, 0.38), (0.34, 0.82)], line);
            pen.fill(&[(0.18, 0.66), (0.34, 0.82), (0.12, 0.88)], accent, bold);
            pen.open(&[(0.54, 0.3), (0.7, 0.46)], line);
        }
        Glyph::Delete => {
            pen.open(&[(0.16, 0.26), (0.84, 0.26)], bold);
            pen.open(&[(0.4, 0.26), (0.42, 0.14), (0.58, 0.14), (0.6, 0.26)], line);
            pen.open(&[(0.24, 0.26), (0.3, 0.88), (0.7, 0.88), (0.76, 0.26)], line);
            pen.open(&[(0.42, 0.4), (0.43, 0.76)], line);
            pen.open(&[(0.58, 0.4), (0.57, 0.76)], line);
        }
        // A face in perspective, a pointer on it.
        Glyph::PickFace => {
            pen.fill(&[(0.1, 0.42), (0.52, 0.42), (0.78, 0.16), (0.36, 0.16)], accent.gamma_multiply(0.45), line);
            pen.closed(&[(0.1, 0.42), (0.1, 0.84), (0.52, 0.84), (0.52, 0.42)], line);
            pen.fill(&[(0.5, 0.3), (0.5, 0.86), (0.86, 0.66)], ink, line);
            pen.open(&[(0.66, 0.72), (0.8, 0.94)], line);
        }
        // Two dots and the gap's line between them.
        Glyph::PickGap => {
            pen.open(&[(0.22, 0.78), (0.78, 0.22)], line);
            p.circle_filled(pen.at(0.22, 0.78), 0.11 * s, accent);
            p.circle_filled(pen.at(0.78, 0.22), 0.11 * s, accent);
            pen.open(&[(0.42, 0.46), (0.54, 0.58)], bold);
        }
        Glyph::Blank => {
            pen.frame(pen.rect(0.2, 0.2, 0.8, 0.8), 3.0, line);
        }
    }
}

/// Draws in a unit square centred on the glyph's rectangle, y down.
struct Pen<'a> {
    p: &'a Painter,
    c: Pos2,
    s: f32,
}

impl<'a> Pen<'a> {
    fn new(p: &'a Painter, r: Rect) -> Pen<'a> {
        Pen { p, c: r.center(), s: r.width().min(r.height()) }
    }

    /// The stroke width for the glyph's size.
    fn w(&self) -> f32 {
        (self.s / 16.0).max(1.0)
    }

    fn at(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.c.x + (x - 0.5) * self.s, self.c.y + (y - 0.5) * self.s)
    }

    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_two_pos(self.at(x0, y0), self.at(x1, y1))
    }

    fn points(&self, pts: &[(f32, f32)]) -> Vec<Pos2> {
        pts.iter().map(|&(x, y)| self.at(x, y)).collect()
    }

    /// A polyline through `pts`.
    fn open(&self, pts: &[(f32, f32)], stroke: Stroke) {
        self.p.add(Shape::line(self.points(pts), stroke));
    }

    /// A closed polygon outline through `pts`.
    fn closed(&self, pts: &[(f32, f32)], stroke: Stroke) {
        self.p.add(Shape::closed_line(self.points(pts), stroke));
    }

    /// A filled convex polygon with an outline.
    fn fill(&self, pts: &[(f32, f32)], fill: Color32, stroke: Stroke) {
        self.p.add(Shape::convex_polygon(self.points(pts), fill, stroke));
    }

    /// A rectangle's outline, its corners rounded by `rounding`.
    fn frame(&self, r: Rect, rounding: f32, stroke: Stroke) {
        self.p.rect_stroke(r, rounding, stroke, egui::StrokeKind::Middle);
    }
}

/// Points along an arc about `c` of radius `r` from angle `a0` to `a1` (radians, 0 along +x,
/// growing clockwise on screen).
fn arc(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    (0..=16).map(|k| a0 + (a1 - a0) * k as f32 / 16.0).map(|a| pos2(c.x + r * a.cos(), c.y + r * a.sin())).collect()
}

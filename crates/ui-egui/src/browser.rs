//! The browser: the session's model and results as a tree (Geometry, Materials, Ports, Sources,
//! Boundaries, Mesh, Results) on a floating card at the left, after SolidCraft's browser. Rows
//! are read from the session every frame; clicking a result shows it, double-clicking a
//! geometry, mesh or boundary row opens its command's dialog on the model's values.

use egui::{Align2, FontId, Rect, Sense, Stroke, pos2, vec2};

use crate::glyphs::{self, Glyph};
use std::fmt::Display;

use crate::handle::{Fixed, Handle};
use crate::lang::{self, Both};
use crate::scene3d::Pick;
use crate::theme::Tokens;
use crate::{Action, Actions, EmcraftApp, Style, UiExt, View, scenario};

/// Folded sections and the selected row.
#[derive(Clone, Debug, Default)]
pub struct Browser {
    pub folded: Vec<String>,
    pub selected: Option<String>,
}

/// What a row does.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    None,
    Show(View),
    Edit(&'static str),
    /// Opens a command's dialog with these values over its usual ones (a body's material).
    EditWith(&'static str, serde_json::Value),
}

impl Act {
    /// Whether a double-click edits.
    pub fn edits(&self) -> bool {
        matches!(self, Act::Edit(_) | Act::EditWith(..))
    }

    /// What a double-click asks for: the dialog the row edits with.
    pub fn edit(self) -> Option<Action> {
        match self {
            Act::Edit(id) => Some(Action::dialog(id)),
            Act::EditWith(id, values) => Some(Action::dialog_with(id, values)),
            _ => None,
        }
    }
}

/// A row: its text (in English: its handle and key) as shown, glyph and action.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub text: String,
    /// The text in the current language.
    pub shown: String,
    pub glyph: Glyph,
    pub act: Act,
    /// What the row is in the 3D view (selecting one selects the other).
    pub pick: Option<Pick>,
}

/// A row of `template` (looked up in the current language) with its values.
fn row(template: &str, args: &[(&str, &dyn Display)], glyph: Glyph, act: Act) -> Row {
    let Both { en, shown } = lang::both(template, args);
    Row { text: en, shown, glyph, act, pick: None }
}

impl Row {
    fn picks(self, pick: Pick) -> Row {
        Row { pick: Some(pick), ..self }
    }

    /// The row indented under the one before.
    fn sub(self) -> Row {
        Row { text: format!("  {}", self.text), shown: format!("  {}", self.shown), ..self }
    }
}

/// The row of what `pick` is: its key (`section/text`) and action.
pub fn row_of(app: &EmcraftApp, pick: Pick) -> Option<(String, Row)> {
    tree(app).into_iter().find_map(|(section, rows)| rows.into_iter().find(|r| r.pick == Some(pick)).map(|r| (format!("{section}/{}", r.text), r)))
}

/// The tree's sections and rows for a session.
pub fn tree(app: &EmcraftApp) -> Vec<(&'static str, Vec<Row>)> {
    let f = Fmt::new(&app.session);
    let m = app.session.model();
    let mut sections = Vec::new();
    // A project's variables, when it has any: double-click to change one.
    if !app.session.variables().is_empty() {
        sections.push(("Variables", variables(&app.session)));
    }
    sections.extend([
        ("Geometry", geometry(m, &f)),
        (
            "Materials",
            m.materials
                .iter()
                .map(|mat| row("{name}: {summary}", &[("name", &mat.name), ("summary", &mat.summary())], Glyph::Material, Act::None))
                .collect(),
        ),
        ("Ports", ports(m)),
        ("Sources", sources(m, &f)),
        ("Boundaries", boundaries(m, &f)),
        ("Mesh", mesh(m)),
        ("Results", results(app)),
    ]);
    sections
}

fn variables(s: &emcraft_engine::Session) -> Vec<Row> {
    s.variables()
        .iter()
        .map(|(name, value)| {
            let given = s.history().iter().rev().find(|st| st.defines(name)).and_then(|st| st.param("value").cloned());
            let expression = given.as_ref().and_then(serde_json::Value::as_str).map(|e| format!(" ({e})")).unwrap_or_default();
            let edit = serde_json::json!({"name": name, "value": given.unwrap_or(serde_json::json!(value))});
            row(
                "{name} = {value}{expression}",
                &[("name", name), ("value", value), ("expression", &expression)],
                Glyph::Variable,
                Act::EditWith("project.variable", edit),
            )
        })
        .collect()
}

/// Lengths in the session's display unit, trailing zeros trimmed.
struct Fmt {
    length: emcraft_model::units::Length,
}

impl Fmt {
    fn new(s: &emcraft_engine::Session) -> Fmt {
        Fmt { length: s.display().length }
    }

    fn len(&self, x: f64) -> String {
        format!("{:.4}", self.length.from_si(x)).trim_end_matches('0').trim_end_matches('.').to_owned()
    }

    fn unit(&self) -> &'static str {
        self.length.symbol()
    }
}

fn geometry(m: &emcraft_model::Model, f: &Fmt) -> Vec<Row> {
    let (mm, lu) = (|x| f.len(x), f.unit());
    let mut rows = Vec::new();
    if let Some(w) = &m.waveguide {
        let edit = || Act::Edit("geometry.waveguide");
        let size: [(&str, &dyn Display); 4] = [("w", &mm(w.width)), ("h", &mm(w.height)), ("l", &mm(w.length())), ("unit", &lu)];
        rows.push(row("Waveguide {w} × {h} × {l} {unit}", &size, Glyph::Waveguide, edit()).picks(Pick::Guide));
        for (i, sec) in w.sections.iter().enumerate() {
            let (n, length) = (i + 1, mm(sec.length));
            let args: [(&str, &dyn Display); 4] = [("n", &n), ("length", &length), ("unit", &lu), ("material", &sec.material)];
            let r = match sec.width {
                Some(a) => row(
                    "Section {n}: {length} {unit}, {material}, aperture {aperture}",
                    &[&args[..], &[("aperture", &mm(a))]].concat(),
                    Glyph::Box,
                    edit(),
                ),
                None => row("Section {n}: {length} {unit}, {material}", &args, Glyph::Box, edit()),
            };
            rows.push(r.sub().picks(Pick::Section(i)));
        }
        for (i, layer) in w.layers.iter().enumerate() {
            let args: [(&str, &dyn Display); 4] =
                [("n", &(i + 1)), ("thickness", &mm(layer.thickness)), ("unit", &lu), ("material", &layer.material)];
            rows.push(row("Layer {n}: {thickness} {unit}, {material}", &args, Glyph::Material, edit()).sub().picks(Pick::Layer(i)));
        }
        for (i, st) in w.strips.iter().enumerate() {
            let args: [(&str, &dyn Display); 3] = [("n", &(i + 1)), ("y", &mm(st.y)), ("width", &mm(st.width))];
            rows.push(row("Strip {n}: y {y}, width {width}", &args, Glyph::PortModes, edit()).sub().picks(Pick::Strip(i)));
        }
        for (i, v) in w.vias.iter().enumerate() {
            let args: [(&str, &dyn Display); 6] =
                [("n", &(i + 1)), ("wx", &mm(v.wx)), ("wz", &mm(v.wz)), ("unit", &lu), ("y0", &mm(v.y0)), ("y1", &mm(v.y1))];
            rows.push(row("Via {n}: {wx} × {wz} {unit}, y {y0} to {y1}", &args, Glyph::Box, edit()).sub().picks(Pick::Via(i)));
        }
    }
    if let Some(solid) = &m.solid {
        let edit = Act::Edit("geometry.solid");
        let r = match &solid.shape {
            emcraft_model::Shape::Surface { name, .. } => {
                row("Region: {name} (imported), {material}", &[("name", name), ("material", &solid.material)], Glyph::Box, edit)
            }
            other => row("Region: {shape}, {material}", &[("shape", &other.kind().to_lowercase()), ("material", &solid.material)], Glyph::Box, edit),
        };
        rows.push(r.picks(Pick::Solid));
    }
    // Bodies by what they are made of; a double-click changes it.
    for b in emcraft_engine::bodies::bodies(m) {
        let pick = match b.at {
            emcraft_engine::bodies::BodyRef::Feature(i) => Pick::Feature(i),
            emcraft_engine::bodies::BodyRef::Inclusion(i) => Pick::Inclusion(i),
        };
        let glyph = if b.material == emcraft_engine::bodies::METAL { Glyph::Feature } else { Glyph::Inclusion };
        let edit = Act::EditWith("geometry.body.material", serde_json::json!({"body": b.name, "material": b.material}));
        let args: [(&str, &dyn Display); 3] = [("name", &b.name), ("material", &b.material), ("shape", &b.shape.to_lowercase())];
        rows.push(row("{name}: {material} {shape}", &args, glyph, edit).sub().picks(pick));
    }
    rows
}

fn ports(m: &emcraft_model::Model) -> Vec<Row> {
    let waveguide = m.ports.iter().enumerate().map(|(i, p)| {
        let z = p.reference.map(|z| format!(", {z} Ω")).unwrap_or_default();
        let template =
            if p.end == emcraft_model::End::Start { "Port {name}: Start, {n} modes{impedance}" } else { "Port {name}: End, {n} modes{impedance}" };
        let args: [(&str, &dyn Display); 3] = [("name", &p.name), ("n", &p.modes), ("impedance", &z)];
        row(template, &args, Glyph::Port, Act::Show(View::PortModes)).picks(Pick::Port(i))
    });
    let lumped =
        m.lumped.iter().map(|p| row("Lumped {name}: {impedance} Ω", &[("name", &p.name), ("impedance", &p.impedance)], Glyph::Lumped, Act::None));
    waveguide.chain(lumped).collect()
}

fn sources(m: &emcraft_model::Model, f: &Fmt) -> Vec<Row> {
    m.sources
        .iter()
        .map(|src| match src {
            emcraft_model::Source::Dipole { length, current, .. } => row(
                "Dipole: {length} {unit}, {current} A",
                &[("length", &f.len(*length)), ("unit", &f.unit()), ("current", current)],
                Glyph::Dipole,
                Act::None,
            ),
            emcraft_model::Source::PlaneWave { direction: emcraft_model::Vec3 { x, y, z }, .. } => {
                row("Plane wave along {direction}", &[("direction", &format!("[{x:.2}, {y:.2}, {z:.2}]"))], Glyph::PlaneWave, Act::None)
            }
        })
        .collect()
}

fn boundaries(m: &emcraft_model::Model, f: &Fmt) -> Vec<Row> {
    let walls = match m.wall_conductivity {
        Some(sigma) => row("Walls: σ = {sigma} S/m", &[("sigma", &format!("{sigma:.3e}"))], Glyph::Walls, Act::Edit("model.walls")),
        None => row("Walls: perfect conductor", &[], Glyph::Walls, Act::Edit("model.walls")),
    };
    let mut rows = vec![walls];
    if let Some(open) = &m.open {
        let template =
            if open.shape == emcraft_model::OpenShape::Sphere { "Open: spherical PML {thickness} {unit}" } else { "Open: PML {thickness} {unit}" };
        rows.push(row(template, &[("thickness", &f.len(open.thickness)), ("unit", &f.unit())], Glyph::Open, Act::Edit("model.open")));
    }
    for plane in &m.symmetry {
        let axis = if plane.axis == emcraft_model::Axis::X { "x" } else { "y" };
        let template = if plane.kind == emcraft_model::SymmetryKind::Electric {
            "Symmetry: {axis} = {at} {unit}, electric wall"
        } else {
            "Symmetry: {axis} = {at} {unit}, magnetic wall"
        };
        let args: [(&str, &dyn Display); 3] = [("axis", &axis), ("at", &f.len(plane.at)), ("unit", &f.unit())];
        rows.push(row(template, &args, Glyph::Walls, Act::Edit("model.symmetry")));
    }
    if let Some(p) = &m.periodic {
        rows.push(row("Periodic: {cell}", &[("cell", &periodic_text(p))], Glyph::Periodic, Act::Edit("model.periodic")));
    }
    rows
}

/// The periodic boundaries in brief: the axes tied and, across x or y, the incident wave.
pub(crate) fn periodic_text(p: &emcraft_model::Periodic) -> String {
    let axes: Vec<&str> = p.axes().into_iter().map(|d| ["x", "y", "z"][d.min(2)]).collect();
    if !(p.x || p.y) {
        return axes.join(", ");
    }
    let pol = if p.polarization == emcraft_model::Polarization::Te { "TE" } else { "TM" };
    format!("{}, θ = {:.1}°, φ = {:.1}°, {pol}", axes.join(", "), p.theta.to_degrees(), p.phi.to_degrees())
}

fn mesh(m: &emcraft_model::Model) -> Vec<Row> {
    let r = match &m.mesh {
        Some(mesh) => row(
            "{nodes} nodes, {tets} tetrahedra",
            &[("nodes", &mesh.nodes.len()), ("tets", &mesh.tets.len())],
            Glyph::Mesh,
            Act::Edit("mesh.generate"),
        ),
        None => row("Not meshed", &[], Glyph::Mesh, Act::Edit("mesh.generate")),
    };
    vec![r]
}

fn results(app: &EmcraftApp) -> Vec<Row> {
    let r = app.session.results();
    let fu = app.session.display().frequency;
    let mut rows = Vec::new();
    if let Some(sw) = &r.sweep {
        let args: [(&str, &dyn Display); 2] = [("n", &sw.results.len()), ("unknowns", &sw.driven.dofs.unknowns)];
        rows.push(row("Sweep: {n} frequencies, {unknowns} unknowns", &args, Glyph::Chart, Act::Show(View::SParameters)));
        rows.push(row("Fields", &[], Glyph::Field, Act::Show(View::Field)));
    }
    if let Some(e) = &r.eigen {
        rows.push(row("Eigenmodes: {n}", &[("n", &e.modes.len())], Glyph::Eigen, Act::Show(View::Eigenmodes)));
    }
    if let Some(d) = &r.dispersion {
        rows.push(row("Dispersion: {n} phases", &[("n", &d.points.len())], Glyph::Dispersion, Act::Show(View::Dispersion)));
    }
    if let Some(td) = &r.time_domain {
        rows.push(row("Time domain: {n} frequencies", &[("n", &td.frequencies.len())], Glyph::Time, Act::Show(View::SParameters)));
    }
    if let Some(rad) = &r.radiation {
        let at = format!("{:.4} {}", fu.from_si(rad.result.frequency), fu.symbol());
        rows.push(row("Radiation at {frequency}", &[("frequency", &at)], Glyph::Radiation, Act::Show(View::FarField)));
    }
    if r.td_radiation.is_some() {
        rows.push(row("Time-domain radiation", &[], Glyph::Radiation, Act::Show(View::FarField)));
    }
    if app.replies.port_modes.is_some() {
        rows.push(row("Port modes", &[], Glyph::PortModes, Act::Show(View::PortModes)));
    }
    if app.replies.compare.is_some() {
        rows.push(row("Comparison with theory", &[], Glyph::Compare, Act::Show(View::Compare)));
    }
    if let Some(study) = &r.study {
        let template = match study.kind {
            emcraft_engine::study::Kind::Sweep if study.is_grid() => "Sweep of {names}: {n} points",
            emcraft_engine::study::Kind::Sweep => "Sweep of {names}: {n} values",
            emcraft_engine::study::Kind::Optimize => "Optimization of {names}: {n} solves",
        };
        let glyph = if study.kind == emcraft_engine::study::Kind::Sweep { Glyph::ParamSweep } else { Glyph::Optimize };
        rows.push(row(template, &[("names", &study.variables.join(", ")), ("n", &study.rows.len())], glyph, Act::Show(View::Study)));
    }
    rows
}

/// The browser card.
pub fn show(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    egui::Panel::left("emcraft_browser")
        .default_size(280.0)
        .size_range(200.0..=480.0)
        .resizable(true)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(egui::Margin { left: 6, right: 0, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            t.card(t.panel).inner_margin(egui::Margin::symmetric(6, 6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height());
                scenario::publish_rect(Fixed::Browser, ui.max_rect());
                ui.text(Style::SmallDim, "BROWSER");
                let tree = tree(app);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for (section, rows) in &tree {
                        let folded = app.browser.folded.iter().any(|f| f == section);
                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, *section));
                        if resp.hovered() {
                            ui.painter().rect_filled(r, t.radius_sm, t.hover);
                        }
                        chevron(ui, pos2(r.left() + 9.0, r.center().y), !folded, t.text_dim);
                        ui.painter().text(
                            pos2(r.left() + 20.0, r.center().y),
                            Align2::LEFT_CENTER,
                            lang::tr(section),
                            FontId::proportional(t.font_label),
                            t.text,
                        );
                        ui.painter().text(
                            pos2(r.right() - 6.0, r.center().y),
                            Align2::RIGHT_CENTER,
                            rows.len().to_string(),
                            FontId::proportional(t.font_small),
                            t.text_dim,
                        );
                        scenario::publish_target(Handle::Fold(section), r);
                        if resp.clicked() {
                            out.push(Action::Fold((*section).to_owned()));
                        }
                        if folded {
                            continue;
                        }
                        if rows.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(26.0);
                                ui.text(Style::SmallDim, "none");
                            });
                        }
                        for r in rows {
                            let key = format!("{section}/{}", r.text);
                            let selected = app.browser.selected.as_deref() == Some(key.as_str());
                            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                            let text = r.text.trim_start();
                            let shown = r.shown.trim_start();
                            let indent = 20.0 + 12.0 * (r.text.len() - text.len()) as f32 / 2.0;
                            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, shown));
                            if selected {
                                ui.painter().rect_filled(rect, t.radius_sm, t.selected_bg);
                            } else if resp.hovered() {
                                ui.painter().rect_filled(rect, t.radius_sm, t.hover);
                            }
                            glyphs::paint(
                                ui.painter(),
                                Rect::from_center_size(pos2(rect.left() + indent + 7.0, rect.center().y), vec2(15.0, 15.0)),
                                r.glyph,
                                t.icon,
                                t.accent,
                            );
                            let ink = if selected { t.selected_ink } else { t.text };
                            ui.painter().with_clip_rect(rect).text(
                                pos2(rect.left() + indent + 19.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                shown,
                                FontId::proportional(t.font_body),
                                ink,
                            );
                            scenario::publish_target(Handle::Row(text), rect);
                            let resp = resp.on_hover_text(shown);
                            if resp.clicked() {
                                out.push(Action::Select { row: Some(key.clone()), pick: r.pick });
                                if let Act::Show(v) = r.act {
                                    out.push(Action::ShowView(v));
                                }
                            }
                            if resp.double_clicked() && !app.busy() {
                                out.extend(r.act.clone().edit());
                            }
                        }
                    }
                });
            });
        });
}

/// A disclosure chevron: down when open, right when closed.
fn chevron(ui: &egui::Ui, c: egui::Pos2, open: bool, ink: egui::Color32) {
    let d = 3.5;
    let pts = if open {
        [c + vec2(-d, -d / 2.0), c + vec2(0.0, d / 2.0), c + vec2(d, -d / 2.0)]
    } else {
        [c + vec2(-d / 2.0, -d), c + vec2(d / 2.0, 0.0), c + vec2(-d / 2.0, d)]
    };
    ui.painter().add(egui::Shape::line(pts.to_vec(), Stroke::new(1.4, ink)));
}

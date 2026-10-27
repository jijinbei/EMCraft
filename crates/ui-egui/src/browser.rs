//! The browser: the session's model and results as a tree (Geometry, Materials, Ports, Sources,
//! Boundaries, Mesh, Results) on a floating card at the left, after SolidCraft's browser. Rows
//! are read from the session every frame; clicking a result shows it, double-clicking a
//! geometry, mesh or boundary row opens its command's dialog on the model's values.

use egui::{Align2, FontId, Rect, Sense, Stroke, pos2, vec2};

use crate::scene3d::Pick;
use crate::theme::Tokens;
use crate::{EmcraftApp, View, glyphs, scenario};

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

    /// Opens the dialog the row edits with.
    pub fn open(self, app: &mut EmcraftApp) {
        let r = match self {
            Act::Edit(id) => app.open_dialog(id),
            Act::EditWith(id, values) => app.open_dialog_with(id, values),
            _ => Ok(()),
        };
        if let Err(e) = r {
            app.message(e, true);
        }
    }
}

/// A row: its text, glyph and action.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub text: String,
    pub glyph: &'static str,
    pub act: Act,
    /// What the row is in the 3D view (selecting one selects the other).
    pub pick: Option<Pick>,
}

fn row(text: impl Into<String>, glyph: &'static str, act: Act) -> Row {
    Row { text: text.into(), glyph, act, pick: None }
}

impl Row {
    fn picks(self, pick: Pick) -> Row {
        Row { pick: Some(pick), ..self }
    }
}

/// The row of what `pick` is: its key (`section/text`) and action.
pub fn row_of(app: &EmcraftApp, pick: Pick) -> Option<(String, Row)> {
    tree(app).into_iter().find_map(|(section, rows)| rows.into_iter().find(|r| r.pick == Some(pick)).map(|r| (format!("{section}/{}", r.text), r)))
}

/// The tree's sections and rows for a session.
pub fn tree(app: &EmcraftApp) -> Vec<(&'static str, Vec<Row>)> {
    let f = Fmt::new(&app.session);
    let m = &app.session.model;
    vec![
        ("Geometry", geometry(m, &f)),
        ("Materials", m.materials.iter().map(|mat| row(format!("{}: {}", mat.name, mat.summary()), "material", Act::None)).collect()),
        ("Ports", ports(m)),
        ("Sources", sources(m, &f)),
        ("Boundaries", boundaries(m, &f)),
        ("Mesh", mesh(m)),
        ("Results", results(app)),
    ]
}

/// Lengths in the session's display unit, trailing zeros trimmed.
struct Fmt {
    length: emcraft_model::units::Length,
}

impl Fmt {
    fn new(s: &emcraft_engine::Session) -> Fmt {
        Fmt { length: s.display.length }
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
        rows.push(row(format!("Waveguide {} × {} × {} {lu}", mm(w.width), mm(w.height), mm(w.length())), "waveguide", edit()).picks(Pick::Guide));
        for (i, sec) in w.sections.iter().enumerate() {
            let ap = sec.width.map(|a| format!(", aperture {}", mm(a))).unwrap_or_default();
            rows.push(row(format!("  Section {}: {} {lu}, {}{ap}", i + 1, mm(sec.length), sec.material), "box", edit()).picks(Pick::Section(i)));
        }
        for (i, layer) in w.layers.iter().enumerate() {
            rows.push(row(format!("  Layer {}: {} {lu}, {}", i + 1, mm(layer.thickness), layer.material), "material", edit()).picks(Pick::Layer(i)));
        }
        for (i, st) in w.strips.iter().enumerate() {
            rows.push(row(format!("  Strip {}: y {}, width {}", i + 1, mm(st.y), mm(st.width)), "port_modes", edit()).picks(Pick::Strip(i)));
        }
        for (i, v) in w.vias.iter().enumerate() {
            let text = format!("  Via {}: {} × {} {lu}, y {} to {}", i + 1, mm(v.wx), mm(v.wz), mm(v.y0), mm(v.y1));
            rows.push(row(text, "box", edit()).picks(Pick::Via(i)));
        }
    }
    if let Some(solid) = &m.solid {
        let what = match &solid.shape {
            emcraft_model::Shape::Surface { name, .. } => format!("{name} (imported)"),
            other => other.kind().to_lowercase(),
        };
        rows.push(row(format!("Region: {what}, {}", solid.material), "box", Act::Edit("geometry.solid")).picks(Pick::Solid));
    }
    // Bodies by what they are made of; a double-click changes it.
    for b in emcraft_engine::bodies::bodies(m) {
        let pick = match b.at {
            emcraft_engine::bodies::BodyRef::Feature(i) => Pick::Feature(i),
            emcraft_engine::bodies::BodyRef::Inclusion(i) => Pick::Inclusion(i),
        };
        let glyph = if b.material == emcraft_engine::bodies::METAL { "feature" } else { "inclusion" };
        let text = format!("  {}: {} {}", b.name, b.material, b.shape.to_lowercase());
        rows.push(row(text, glyph, Act::EditWith("geometry.body.material", serde_json::json!({"body": b.name, "material": b.material}))).picks(pick));
    }
    rows
}

fn ports(m: &emcraft_model::Model) -> Vec<Row> {
    let waveguide = m.ports.iter().enumerate().map(|(i, p)| {
        let z = p.reference.map(|z| format!(", {z} Ω")).unwrap_or_default();
        row(format!("Port {}: {:?}, {} modes{z}", p.name, p.end, p.modes), "port", Act::Show(View::PortModes)).picks(Pick::Port(i))
    });
    let lumped = m.lumped.iter().map(|p| row(format!("Lumped {}: {} Ω", p.name, p.impedance), "lumped", Act::None));
    waveguide.chain(lumped).collect()
}

fn sources(m: &emcraft_model::Model, f: &Fmt) -> Vec<Row> {
    m.sources
        .iter()
        .map(|src| match src {
            emcraft_model::Source::Dipole { length, current, .. } => {
                row(format!("Dipole: {} {}, {current} A", f.len(*length), f.unit()), "dipole", Act::None)
            }
            emcraft_model::Source::PlaneWave { direction: emcraft_model::Vec3 { x, y, z }, .. } => {
                row(format!("Plane wave along [{x:.2}, {y:.2}, {z:.2}]"), "plane_wave", Act::None)
            }
        })
        .collect()
}

fn boundaries(m: &emcraft_model::Model, f: &Fmt) -> Vec<Row> {
    let walls = match m.wall_conductivity {
        Some(sigma) => format!("Walls: σ = {sigma:.3e} S/m"),
        None => "Walls: perfect conductor".to_owned(),
    };
    let mut rows = vec![row(walls, "walls", Act::Edit("model.walls"))];
    if let Some(open) = &m.open {
        let kind = if open.shape == emcraft_model::OpenShape::Sphere { "spherical PML" } else { "PML" };
        rows.push(row(format!("Open: {kind} {} {}", f.len(open.thickness), f.unit()), "open", Act::Edit("model.open")));
    }
    for plane in &m.symmetry {
        let axis = if plane.axis == emcraft_model::Axis::X { "x" } else { "y" };
        let kind = if plane.kind == emcraft_model::SymmetryKind::Electric { "electric" } else { "magnetic" };
        rows.push(row(format!("Symmetry: {axis} = {} {}, {kind} wall", f.len(plane.at), f.unit()), "walls", Act::Edit("model.symmetry")));
    }
    if let Some(p) = &m.periodic {
        rows.push(row(format!("Periodic: {}", periodic_text(p)), "periodic", Act::Edit("model.periodic")));
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
    let text = match &m.mesh {
        Some(mesh) => format!("{} nodes, {} tetrahedra", mesh.nodes.len(), mesh.tets.len()),
        None => "Not meshed".to_owned(),
    };
    vec![row(text, "mesh", Act::Edit("mesh.generate"))]
}

fn results(app: &EmcraftApp) -> Vec<Row> {
    let r = &app.session.results;
    let fu = app.session.display.frequency;
    let mut rows = Vec::new();
    if let Some(sw) = &r.sweep {
        let text = format!("Sweep: {} frequencies, {} unknowns", sw.results.len(), sw.driven.dofs.unknowns);
        rows.push(row(text, "chart", Act::Show(View::SParameters)));
        rows.push(row("Fields", "field", Act::Show(View::Field)));
    }
    if let Some(e) = &r.eigen {
        rows.push(row(format!("Eigenmodes: {}", e.modes.len()), "eigen", Act::Show(View::Eigenmodes)));
    }
    if let Some(d) = &r.dispersion {
        rows.push(row(format!("Dispersion: {} phases", d.points.len()), "dispersion", Act::Show(View::Dispersion)));
    }
    if let Some(td) = &r.time_domain {
        rows.push(row(format!("Time domain: {} frequencies", td.frequencies.len()), "time", Act::Show(View::SParameters)));
    }
    if let Some(rad) = &r.radiation {
        rows.push(row(format!("Radiation at {:.4} {}", fu.from_si(rad.result.frequency), fu.symbol()), "radiation", Act::Show(View::FarField)));
    }
    if r.td_radiation.is_some() {
        rows.push(row("Time-domain radiation", "radiation", Act::Show(View::FarField)));
    }
    if app.port_modes.is_some() {
        rows.push(row("Port modes", "port_modes", Act::Show(View::PortModes)));
    }
    if app.compare.is_some() {
        rows.push(row("Comparison with theory", "compare", Act::Show(View::Compare)));
    }
    rows
}

/// The browser card.
pub fn show(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
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
                scenario::publish_rect("browser", ui.max_rect());
                ui.label(egui::RichText::new("BROWSER").size(t.font_small).color(t.text_dim));
                let tree = tree(app);
                let mut act = None;
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
                            *section,
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
                        scenario::publish_target(&format!("fold:{section}"), r);
                        if resp.clicked() {
                            if folded {
                                app.browser.folded.retain(|f| f != section);
                            } else {
                                app.browser.folded.push((*section).to_owned());
                            }
                        }
                        if folded {
                            continue;
                        }
                        if rows.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(26.0);
                                ui.label(egui::RichText::new("none").size(t.font_small).color(t.text_dim));
                            });
                        }
                        for r in rows {
                            let key = format!("{section}/{}", r.text);
                            let selected = app.browser.selected.as_deref() == Some(key.as_str());
                            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                            let text = r.text.trim_start();
                            let indent = 20.0 + 12.0 * (r.text.len() - text.len()) as f32 / 2.0;
                            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
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
                                text,
                                FontId::proportional(t.font_body),
                                ink,
                            );
                            scenario::publish_target(&format!("row:{text}"), rect);
                            let resp = resp.on_hover_text(text);
                            if resp.clicked() {
                                app.browser.selected = Some(key.clone());
                                app.view3d.selected = r.pick;
                                if let Act::Show(v) = &r.act {
                                    act = Some(r.act.clone());
                                    let _ = v;
                                }
                            }
                            if resp.double_clicked() && r.act.edits() {
                                act = Some(r.act.clone());
                            }
                        }
                    }
                });
                match act {
                    Some(Act::Show(v)) => crate::ribbon::show_view(app, v),
                    Some(a) if a.edits() && !app.busy() => a.open(app),
                    _ => {}
                }
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

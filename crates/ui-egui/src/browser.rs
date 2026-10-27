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
    let s = &app.session;
    let m = &s.model;
    let l = s.display.length;
    let lu = l.symbol();
    let fu = s.display.frequency;
    let mm = |x: f64| format!("{:.4}", l.from_si(x)).trim_end_matches('0').trim_end_matches('.').to_owned();
    let mut geometry = Vec::new();
    if let Some(w) = &m.waveguide {
        geometry.push(
            row(format!("Waveguide {} × {} × {} {lu}", mm(w.width), mm(w.height), mm(w.length())), "waveguide", Act::Edit("geometry.waveguide"))
                .picks(Pick::Guide),
        );
        for (i, sec) in w.sections.iter().enumerate() {
            let ap = sec.width.map(|a| format!(", aperture {}", mm(a))).unwrap_or_default();
            geometry.push(
                row(format!("  Section {}: {} {lu}, {}{ap}", i + 1, mm(sec.length), sec.material), "box", Act::Edit("geometry.waveguide"))
                    .picks(Pick::Section(i)),
            );
        }
        for (i, layer) in w.layers.iter().enumerate() {
            geometry.push(
                row(format!("  Layer {}: {} {lu}, {}", i + 1, mm(layer.thickness), layer.material), "material", Act::Edit("geometry.waveguide"))
                    .picks(Pick::Layer(i)),
            );
        }
        for (i, st) in w.strips.iter().enumerate() {
            geometry.push(
                row(format!("  Strip {}: y {}, width {}", i + 1, mm(st.y), mm(st.width)), "port_modes", Act::Edit("geometry.waveguide"))
                    .picks(Pick::Strip(i)),
            );
        }
    }
    if let Some(solid) = &m.solid {
        let shape = format!("{:?}", solid.shape);
        let name = shape.split([' ', '{']).next().unwrap_or("Solid").to_owned();
        geometry.push(row(format!("Solid: {name}, {}", solid.material), "box", Act::Edit("geometry.solid")).picks(Pick::Solid));
    }
    for (i, f) in m.features.iter().enumerate() {
        let text = format!("{f:?}");
        let kind = text.split([' ', '{', '(']).next().unwrap_or("Feature").to_owned();
        geometry.push(row(format!("  Boolean {}: {kind}", i + 1), "feature", Act::Edit("geometry.feature")).picks(Pick::Feature(i)));
    }
    for (i, inc) in m.inclusions.iter().enumerate() {
        let what = inc.material.clone().unwrap_or_else(|| "conductor".into());
        geometry.push(row(format!("  Inclusion {}: {what}", i + 1), "inclusion", Act::Edit("geometry.inclusion")).picks(Pick::Inclusion(i)));
    }
    let materials = m
        .materials
        .iter()
        .map(|mat| {
            let loss = if mat.loss_tangent > 0.0 { format!(", tan δ {}", mat.loss_tangent) } else { String::new() };
            row(format!("{}: εr {}, μr {}{loss}", mat.name, mat.eps_r, mat.mu_r), "material", Act::None)
        })
        .collect();
    let mut ports: Vec<Row> = m
        .ports
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let z = p.reference.map(|z| format!(", {z} Ω")).unwrap_or_default();
            row(format!("Port {}: {:?}, {} modes{z}", p.name, p.end, p.modes), "port", Act::Show(View::PortModes)).picks(Pick::Port(i))
        })
        .collect();
    ports.extend(m.lumped.iter().map(|p| row(format!("Lumped {}: {} Ω", p.name, p.impedance), "lumped", Act::None)));
    let sources = m
        .sources
        .iter()
        .map(|src| match src {
            emcraft_model::Source::Dipole { length, current, .. } => row(format!("Dipole: {} {lu}, {current} A", mm(*length)), "dipole", Act::None),
            emcraft_model::Source::PlaneWave { direction, .. } => {
                row(format!("Plane wave along [{:.2}, {:.2}, {:.2}]", direction[0], direction[1], direction[2]), "plane_wave", Act::None)
            }
        })
        .collect();
    let mut boundaries = Vec::new();
    boundaries.push(match m.wall_conductivity {
        Some(sigma) => row(format!("Walls: σ = {sigma:.3e} S/m"), "walls", Act::Edit("model.walls")),
        None => row("Walls: perfect conductor", "walls", Act::Edit("model.walls")),
    });
    if let Some(open) = &m.open {
        boundaries.push(row(format!("Open: PML {} {lu}", mm(open.thickness)), "open", Act::Edit("model.open")));
    }
    let mesh = match &m.mesh {
        Some(mesh) => vec![row(format!("{} nodes, {} tetrahedra", mesh.nodes.len(), mesh.tets.len()), "mesh", Act::Edit("mesh.generate"))],
        None => vec![row("Not meshed", "mesh", Act::Edit("mesh.generate"))],
    };
    let mut results = Vec::new();
    if let Some(sw) = &s.results.sweep {
        results.push(row(
            format!("Sweep: {} frequencies, {} unknowns", sw.results.len(), sw.driven.dofs.unknowns),
            "chart",
            Act::Show(View::SParameters),
        ));
        results.push(row("Fields", "field", Act::Show(View::Field)));
    }
    if let Some(e) = &s.results.eigen {
        results.push(row(format!("Eigenmodes: {}", e.modes.len()), "eigen", Act::Show(View::Eigenmodes)));
    }
    if let Some(td) = &s.results.time_domain {
        results.push(row(format!("Time domain: {} frequencies", td.frequencies.len()), "time", Act::Show(View::SParameters)));
    }
    if let Some(r) = &s.results.radiation {
        results.push(row(format!("Radiation at {:.4} {}", fu.from_si(r.result.frequency), fu.symbol()), "radiation", Act::Show(View::FarField)));
    }
    if s.results.td_radiation.is_some() {
        results.push(row("Time-domain radiation", "radiation", Act::Show(View::FarField)));
    }
    if app.port_modes.is_some() {
        results.push(row("Port modes", "port_modes", Act::Show(View::PortModes)));
    }
    if app.compare.is_some() {
        results.push(row("Comparison with theory", "compare", Act::Show(View::Compare)));
    }
    vec![
        ("Geometry", geometry),
        ("Materials", materials),
        ("Ports", ports),
        ("Sources", sources),
        ("Boundaries", boundaries),
        ("Mesh", mesh),
        ("Results", results),
    ]
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
                            if resp.double_clicked() && matches!(r.act, Act::Edit(_)) {
                                act = Some(r.act.clone());
                            }
                        }
                    }
                });
                match act {
                    Some(Act::Show(v)) => crate::ribbon::show_view(app, v),
                    Some(Act::Edit(id)) if !app.busy() => {
                        if let Err(e) = app.open_dialog(id) {
                            app.message(e, true);
                        }
                    }
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

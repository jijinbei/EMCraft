//! The import dialog (`geometry.import`), drawn for importing a SolidCraft design at a glance:
//! what can be imported as chips (a SolidCraft design, 3MF, STL), a large Choose file… (the
//! system's file dialog, or the path prompt without one; a file dropped on the window lands
//! here too, [`crate::files::dropped`]), the file and what it holds (`geometry.import.inspect`:
//! its bodies, size, units and a design's parameters, each editable), one line with a pictogram
//! on what importing makes of it, the other parameters folded under Options with short labels
//! and the command's description behind "?".

use egui::{Align2, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Map, Value, json};

use crate::dialog_ui::Asked;
use crate::dialogs::Dialog;
use crate::glyphs::{self, Glyph};
use crate::handle::{Fixed, Handle};
use crate::lang::{Raw, args, command_doc, tr, trf};
use crate::state::ImportProbe;
use crate::theme::Tokens;
use crate::{EmcraftApp, Hint, Style, UiExt, ui_kit};

/// The command this card is for.
pub(crate) const ID: &str = "geometry.import";

/// The parameters shown above Options: the file, and a design's parameters as their own rows.
const ABOVE: &[&str] = &["path", "parameters"];

/// A parameter's short label under Options.
fn short(name: &'static str) -> &'static str {
    match name {
        "path" => "File",
        "unit" => "STL unit",
        "angle" => "Edge angle",
        "material" => "Filling",
        "region" => "Region",
        "keep" => "Keep",
        "margin" => "Margin",
        "materials" => "Materials",
        "open" => "Open (PML)",
        "thickness" => "PML thickness",
        "fit_surfaces" => "Fit surfaces",
        "parameters" => "Parameters",
        "tolerance" => "Tolerance",
        other => other,
    }
}

fn text<'d>(d: &'d Dialog, name: &str) -> &'d str {
    d.fields.iter().find(|f| f.name == name).map_or("", |f| f.text.trim())
}

/// What the probe of the dialog's file is keyed by: the path and the STL unit.
fn key(d: &Dialog) -> String {
    format!("{}\u{1f}{}", text(d, "path"), text(d, "unit"))
}

/// Reads what the open import dialog's file holds when the file (or its unit) changed.
pub(crate) fn prepare(app: &mut EmcraftApp) {
    let Some(d) = app.dialog.as_ref().filter(|d| d.command.id == ID) else {
        app.replies.import = None;
        return;
    };
    let path = text(d, "path").to_owned();
    if path.is_empty() {
        app.replies.import = None;
        return;
    }
    let key = key(d);
    if app.replies.import.as_ref().is_some_and(|p| p.key == key) {
        return;
    }
    let mut params = json!({"path": path});
    if let Ok(u) = text(d, "unit").parse::<f64>() {
        params["unit"] = json!(u);
    }
    let reply = app.session.execute("geometry.import.inspect", &params).map_err(|e| e.to_string());
    app.replies.import = Some(ImportProbe { key, reply });
}

/// The card's body; the controls drawn, each with whether it takes text.
pub(crate) fn body(app: &EmcraftApp, d: &mut Dialog, ui: &mut egui::Ui, asked: &mut Asked) -> Vec<(egui::Id, bool)> {
    let t = &Tokens::get();
    let session = &app.session;
    let probe = app.replies.import.as_ref().filter(|p| p.key == key(d));
    let reply = probe.and_then(|p| p.reply.as_ref().ok());
    let format = reply.and_then(|r| r["format"].as_str());
    chips(ui, format, t);
    ui.add_space(t.space_6);
    let empty = text(d, "path").is_empty();
    let pick = ui_kit::glyph_button(ui, Glyph::OpenFile, "Choose file…", vec2(0.0, 40.0), empty, !app.busy(), Fixed::DialogPick.into());
    asked.pick |= pick.hint("A SolidCraft design (.solidcraft), 3MF or STL").clicked();
    ui.vertical_centered(|ui| ui_kit::small(ui, "or drop it on the window", t.text_dim));
    ui.add_space(t.space_4);
    let mut drawn = crate::dialog_ui::fields(ui, d, session, &|n| n == "path", &short);
    match probe.map(|p| &p.reply) {
        Some(Ok(r)) => detected(ui, d, r, session, &mut drawn, t),
        Some(Err(e)) => {
            ui_kit::small(ui, Raw(e), t.form_error);
        }
        None => {}
    }
    ui.add_space(t.space_6);
    outcome(ui, d, reply, t);
    ui.add_space(t.space_6);
    // Options: the rest, folded.
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    let fold = ui.hit(r, Fixed::DialogOptions.into(), Sense::click());
    let p = ui.painter();
    if fold.hovered() {
        p.rect_filled(r, t.radius_sm, t.hover);
    }
    // A triangle pointing right when folded, down when open.
    let c = pos2(r.left() + 9.0, r.center().y);
    let tri = if d.options {
        vec![c + vec2(-4.5, -2.5), c + vec2(4.5, -2.5), c + vec2(0.0, 3.5)]
    } else {
        vec![c + vec2(-2.5, -4.5), c + vec2(3.5, 0.0), c + vec2(-2.5, 4.5)]
    };
    p.add(egui::Shape::convex_polygon(tri, t.text, Stroke::NONE));
    p.text(pos2(r.left() + 20.0, r.center().y), Align2::LEFT_CENTER, tr("Options"), FontId::proportional(t.font_label), t.text);
    fold.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr("Options")));
    if fold.clicked() {
        d.options = !d.options;
    }
    let design = format == Some("solidcraft");
    if d.options {
        let shown = |n: &str| !ABOVE.contains(&n) || (n == "parameters" && !design);
        drawn.extend(crate::dialog_ui::fields(ui, d, session, &shown, &short));
    }
    // The command's whole description, last.
    if d.help {
        ui.add_space(t.space_6);
        ui_kit::caption(ui, Raw(command_doc(ID)));
    }
    drawn
}

/// What can be imported, as chips; the detected format's lit.
fn chips(ui: &mut egui::Ui, format: Option<&str>, t: &Tokens) {
    let kinds = [
        ("solidcraft", Glyph::Feature, "SolidCraft", "parametric"),
        ("3mf", Glyph::Box, "3MF", "face tags kept"),
        ("stl", Glyph::Mesh, "STL", "one body"),
    ];
    let gap = 6.0;
    let w = ((ui.available_width() - 2.0 * gap) / 3.0).max(60.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (id, glyph, name, what) in kinds {
            let (r, _) = ui.allocate_exact_size(vec2(w, 46.0), Sense::hover());
            let lit = format == Some(id);
            let p = ui.painter();
            p.rect(
                r,
                t.radius_sm,
                if lit { t.accent_soft } else { t.control_fill },
                Stroke::new(1.0, if lit { t.accent } else { t.control_border }),
                egui::StrokeKind::Inside,
            );
            let icon = Rect::from_min_size(pos2(r.left() + 6.0, r.center().y - 11.0), vec2(22.0, 22.0));
            glyphs::paint(p, icon, glyph, t.text, t.accent);
            let x = icon.right() + 6.0;
            let clip = p.with_clip_rect(r.shrink(2.0));
            clip.text(pos2(x, r.center().y - 2.0), Align2::LEFT_BOTTOM, name, FontId::proportional(t.font_label), t.text);
            clip.text(pos2(x, r.center().y + 1.0), Align2::LEFT_TOP, tr(what), FontId::proportional(t.font_small), t.text_dim);
        }
    });
}

/// What the file holds: its bodies, size and units; a design's parameters as fields that write
/// the dialog's `parameters`.
fn detected(ui: &mut egui::Ui, d: &mut Dialog, r: &Value, session: &emcraft_engine::Session, drawn: &mut Vec<(egui::Id, bool)>, t: &Tokens) {
    let l = session.display().length.symbol();
    let frame = egui::Frame::NONE.fill(t.panel).corner_radius(t.radius_sm).inner_margin(egui::Margin::symmetric(8, 6));
    let shown = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        let bodies: Vec<&str> = r["bodies"].as_array().into_iter().flatten().filter_map(|b| b["name"].as_str()).collect();
        let names = bodies.iter().take(6).copied().collect::<Vec<_>>().join(", ");
        let more = if bodies.len() > 6 { "…" } else { "" };
        let count = if bodies.len() == 1 { tr("1 body").to_owned() } else { trf("{n} bodies", args!(n = bodies.len())) };
        ui.horizontal_wrapped(|ui| {
            ui.text(Style::Strong, count);
            ui.text(Style::Small, format!("{names}{more}"));
        });
        if let Some(size) = r["size"].as_array() {
            let v: Vec<String> = size.iter().filter_map(Value::as_f64).map(|x| crate::dialogs::value_text(&json!((x * 1e6).round() / 1e6))).collect();
            let units = if r["units"] == "display" { tr("in display units (STL has none)") } else { tr("units from the file") };
            ui_kit::small(ui, format!("{} {l} · {units}", v.join(" × ")), t.text_dim);
        }
        let params = r["parameters"].as_array().filter(|p| !p.is_empty());
        if let Some(params) = params {
            ui.add_space(t.space_4);
            ui_kit::small(ui, "Parameters", t.text_dim);
            parameters(ui, d, params, drawn, t);
        }
    });
    crate::scenario::publish_target(Fixed::DialogDetected, shown.response.rect);
}

/// A design's parameters, each a field holding its value as saved until changed; a change is
/// written into the dialog's `parameters` (name → number, or an expression the design reads).
fn parameters(ui: &mut egui::Ui, d: &mut Dialog, params: &[Value], drawn: &mut Vec<(egui::Id, bool)>, t: &Tokens) {
    let Some(field) = d.field_mut("parameters") else { return };
    let mut set: Map<String, Value> = serde_json::from_str(field.text.trim()).ok().and_then(|v: Value| v.as_object().cloned()).unwrap_or_default();
    let mut changed = false;
    for q in params.iter().take(64) {
        let Some(name) = q["name"].as_str() else { continue };
        let unit = q["unit"].as_str().unwrap_or("");
        let saved = q["value"].as_f64().map(|x| crate::dialogs::value_text(&json!(x))).unwrap_or_default();
        let id = egui::Id::new(("import_param", name));
        let mut value =
            ui.data(|m| m.get_temp::<String>(id)).unwrap_or_else(|| set.get(name).map(crate::dialogs::value_text).unwrap_or_else(|| saved.clone()));
        ui_kit::row(ui, Raw(name), false, |ui| {
            let w = ui.available_width() - if unit.is_empty() { 8.0 } else { 40.0 };
            let r = ui.named(egui::TextEdit::singleline(&mut value).desired_width(w).char_limit(256), Handle::DialogParam(name));
            drawn.push((r.id, true));
            if r.changed() {
                changed = true;
                let v = value.trim();
                if v.is_empty() || v == saved {
                    set.remove(name);
                } else if let Ok(x) = crate::dialogs::parse_number(v) {
                    set.insert(name.to_owned(), json!(x));
                } else {
                    set.insert(name.to_owned(), json!(v));
                }
            }
            if !unit.is_empty() {
                ui_kit::small(ui, Raw(unit), t.text_dim);
            }
        });
        ui.data_mut(|m| m.insert_temp(id, value));
    }
    if changed {
        field.text = if set.is_empty() { String::new() } else { Value::Object(set).to_string() };
    }
}

/// What importing makes of the file: one line and its pictogram, by the region chosen.
fn outcome(ui: &mut egui::Ui, d: &mut Dialog, reply: Option<&Value>, t: &Tokens) {
    let bodies = reply.and_then(|r| r["bodies"].as_array()).map_or(0, Vec::len);
    let region = text(d, "region");
    let open = d.fields.iter().any(|f| f.name == "open" && f.on);
    let (kind, line) = match region {
        "cavity" => (Pictogram::Cavity, "Metal bodies become walls; the air they enclose is the region"),
        "box" | "" if open => (Pictogram::Box, "Metal bodies in a box of air lined by a PML"),
        "box" => (Pictogram::Box, "Metal bodies in a box of air: the air is the region"),
        "" if bodies == 1 => (Pictogram::One, "The body is the region; its z ends take ports"),
        "" => (Pictogram::Box, "Metal bodies in a box of air: the air is the region"),
        _ => (Pictogram::One, "The named body is the region; the others metal in it"),
    };
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(44.0, 30.0), Sense::hover());
        pictogram(ui.painter(), r, kind, t);
        let w = ui.available_width() - 28.0;
        ui.add_sized(vec2(w, 30.0), egui::Label::new(Style::Small.rich(tr(line))).wrap());
        let help = ui.named(egui::Button::new(Style::Strong.rich("?")).min_size(vec2(22.0, 22.0)), Fixed::DialogHelp.into());
        if help.on_hover_text(command_doc(ID)).clicked() {
            d.help = !d.help;
        }
    });
}

#[derive(Clone, Copy)]
enum Pictogram {
    /// Bodies in a box of air.
    Box,
    /// A metal block with a channel: the air inside is the region.
    Cavity,
    /// One body that is the region.
    One,
}

/// A small drawing of what the import makes: metal grey, the region's air in the accent.
fn pictogram(p: &egui::Painter, r: Rect, kind: Pictogram, t: &Tokens) {
    let metal = t.text_dim;
    let air = t.accent_soft;
    let edge = Stroke::new(1.0, t.accent);
    let r = r.shrink(2.0);
    match kind {
        Pictogram::Box => {
            p.rect(r, 2.0, air, edge, egui::StrokeKind::Inside);
            p.rect_filled(Rect::from_center_size(r.center() - vec2(8.0, 0.0), vec2(8.0, 14.0)), 1.0, metal);
            p.circle_filled(r.center() + vec2(9.0, 0.0), 5.0, metal);
        }
        Pictogram::Cavity => {
            p.rect_filled(r, 2.0, metal);
            let channel = Rect::from_center_size(r.center(), vec2(r.width(), r.height() * 0.4));
            p.rect(channel, 0.0, air, edge, egui::StrokeKind::Inside);
        }
        Pictogram::One => {
            let body = r.shrink2(vec2(6.0, 4.0));
            p.rect(body, 2.0, air, edge, egui::StrokeKind::Inside);
            // Its two ends, where ports go.
            for x in [body.left(), body.right()] {
                p.line_segment([pos2(x, body.top() - 2.0), pos2(x, body.bottom() + 2.0)], Stroke::new(2.5, t.tab_ports[0]));
            }
        }
    }
}

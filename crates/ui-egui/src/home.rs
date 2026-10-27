//! The start page, after SolidCraft's (`crates/ui-egui/src/home.rs`: New and Open, the recent
//! designs and the samples, each a card with a picture), written anew for EMCraft: New
//! project and Open, the recent projects, the samples drawn by the 3D view, and the settings
//! new projects start with. Shown on launch without a sample and from the app bar's house;
//! anything opened leaves it.

use std::collections::HashMap;

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use emcraft_engine::Session;
use emcraft_model::units::{Frequency, Length};
use emcraft_render::view3d::{self, Camera, StandardView};
use serde_json::Value;

use crate::files::{self, Then};
use crate::scene3d::{self, Overlay};
use crate::theme::Tokens;
use crate::{EmcraftApp, Picture, glyphs, scenario};

/// A card's picture, in points.
const THUMB: [f32; 2] = [240.0, 150.0];

#[derive(Default)]
pub struct Home {
    pub open: bool,
    /// Each sample's picture, drawn the first time the page shows it.
    thumbs: HashMap<&'static str, Option<Picture>>,
}

/// A sample's model without its mesh or solve, drawn from the corner.
fn thumbnail(name: &str) -> Option<Picture> {
    let sample = emcraft_engine::sample::find(name)?;
    let script: Vec<Value> = serde_json::from_str(sample.script).ok()?;
    let steps: Vec<Value> = script
        .into_iter()
        .filter(|s| s.get("command").and_then(Value::as_str).is_some_and(|c| !c.starts_with("mesh.") && !c.starts_with("solve.")))
        .collect();
    let mut s = Session::new();
    s.run_script(&Value::Array(steps)).ok()?;
    let built = scene3d::build(&s, Overlay::Materials);
    let (lo, hi) = built.bounds?;
    let (w, h) = ((THUMB[0] * 2.0) as usize, (THUMB[1] * 2.0) as usize);
    let mut cam = Camera::default();
    cam.standard(StandardView::Iso);
    cam.fit(lo, hi, f64::from(THUMB[0] / THUMB[1]));
    cam.distance *= 0.8;
    let t = Tokens::get();
    let rgb = |c: Color32| emcraft_render::Rgb(c.r(), c.g(), c.b());
    let opt = view3d::Options { width: w, height: h, supersample: 2, background: (rgb(t.viewport_top), rgb(t.viewport_bottom)), line_width: 2.0 };
    Some(Picture::new(format!("home:{name}"), &view3d::render(&built.scene, &cam, &opt)))
}

pub fn show(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin::symmetric(36, 24))).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("EMCraft").size(30.0).strong().color(t.text));
                ui.label(egui::RichText::new("Start").size(30.0).color(t.text_dim));
            });
            ui.add_space(t.space_12);
            ui.horizontal(|ui| {
                if big(ui, "new", "New project", true, "home:new") {
                    files::request(app, Then::New);
                }
                if big(ui, "open_file", "Open…", false, "home:open") {
                    files::request(app, Then::Open);
                }
            });
            ui.add_space(t.space_16);
            let right = 300.0;
            ui.horizontal_top(|ui| {
                let left = (ui.available_width() - right - 32.0).max(THUMB[0] + 16.0);
                ui.vertical(|ui| {
                    ui.set_width(left);
                    recent(app, ui);
                    ui.add_space(t.space_16);
                    samples(app, ui);
                });
                ui.add_space(32.0);
                ui.vertical(|ui| {
                    ui.set_width(right);
                    heading(ui, "New projects");
                    settings(app, ui);
                });
            });
        });
    });
}

fn heading(ui: &mut egui::Ui, text: &str) {
    let t = Tokens::get();
    ui.label(egui::RichText::new(text).size(t.font_heading).strong().color(t.text));
    ui.add_space(t.space_4);
}

/// A large button: a glyph and a label; the primary one in the accent.
fn big(ui: &mut egui::Ui, glyph: &str, label: &str, primary: bool, handle: &str) -> bool {
    let t = Tokens::get();
    let g = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(t.font_heading), t.text);
    let (r, resp) = ui.allocate_exact_size(vec2((56.0 + g.size().x + 24.0).max(180.0), 60.0), Sense::click());
    let (fill, ink) = if primary {
        (if resp.hovered() { t.accent.gamma_multiply(0.85) } else { t.accent }, t.on_accent)
    } else {
        (if resp.hovered() { t.hover } else { t.panel_header }, t.text)
    };
    ui.painter().rect(r, 10.0, fill, Stroke::new(1.0, if primary { t.accent } else { t.border }), egui::StrokeKind::Inside);
    glyphs::paint(
        ui.painter(),
        Rect::from_center_size(pos2(r.left() + 32.0, r.center().y), vec2(30.0, 30.0)),
        glyph,
        ink,
        if primary { ink } else { t.accent },
    );
    ui.painter().text(pos2(r.left() + 56.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(t.font_heading), ink);
    scenario::publish_target(handle, r);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Cards in rows as wide as fit.
fn grid(ui: &mut egui::Ui, n: usize, mut each: impl FnMut(&mut egui::Ui, usize)) {
    let w = THUMB[0] + 16.0 + 12.0;
    let per_row = (((ui.available_width() + 12.0) / w).floor() as usize).max(1);
    for row in (0..n).step_by(per_row) {
        ui.horizontal(|ui| {
            for i in row..(row + per_row).min(n) {
                each(ui, i);
            }
        });
        ui.add_space(12.0);
    }
}

/// A card: a picture (or a glyph), a name and a line under it.
fn card(ui: &mut egui::Ui, pic: Option<&mut Picture>, glyph: &str, name: &str, sub: &str, dim: bool, handle: &str) -> egui::Response {
    let t = Tokens::get();
    let (r, resp) = ui.allocate_exact_size(vec2(THUMB[0] + 16.0, THUMB[1] + 64.0), Sense::click());
    let p = ui.painter();
    p.rect(
        r,
        10.0,
        t.panel_header,
        Stroke::new(if resp.hovered() { 2.0 } else { 1.0 }, if resp.hovered() { t.accent } else { t.border }),
        egui::StrokeKind::Inside,
    );
    let pr = Rect::from_min_size(r.min + vec2(8.0, 8.0), vec2(THUMB[0], THUMB[1]));
    match pic {
        Some(pic) => {
            let id = pic.texture(ui.ctx());
            p.image(id, pr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            p.rect_filled(pr, 6.0, t.panel);
            glyphs::paint(p, Rect::from_center_size(pr.center(), vec2(56.0, 56.0)), glyph, if dim { t.text_dim } else { t.icon }, t.accent);
        }
    }
    let clip = p.with_clip_rect(r.shrink(8.0));
    clip.text(
        pos2(r.left() + 10.0, pr.bottom() + 18.0),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(t.font_body),
        if dim { t.text_dim } else { t.text },
    );
    clip.text(pos2(r.left() + 10.0, pr.bottom() + 40.0), Align2::LEFT_CENTER, sub, FontId::proportional(t.font_small), t.text_dim);
    scenario::publish_target(handle, r);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn recent(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    heading(ui, "Recent");
    let list = app.prefs.recent.clone();
    if list.is_empty() {
        ui.label(egui::RichText::new("Projects you open or save appear here.").size(t.font_small).color(t.text_dim));
        return;
    }
    let mut open = None;
    let mut forget = None;
    grid(ui, list.len(), |ui, i| {
        let path = &list[i];
        let p = std::path::Path::new(path);
        let name = p.file_stem().map_or_else(|| path.clone(), |s| s.to_string_lossy().into_owned());
        let exists = p.exists();
        let folder = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
        let sub = if exists { folder } else { "missing".to_owned() };
        let r = card(ui, None, "waveguide", &name, &sub, !exists, &format!("home:recent:{i}"));
        let r = r.on_hover_text(path.as_str());
        r.context_menu(|ui| {
            if ui.button("Remove from the list").clicked() {
                forget = Some(i);
            }
        });
        if r.clicked() && exists {
            open = Some(path.clone());
        }
    });
    if let Some(i) = forget {
        app.prefs.recent.remove(i);
    }
    if let Some(path) = open {
        files::request(app, Then::OpenPath(path));
    }
}

fn samples(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    heading(ui, "Samples");
    let list = emcraft_engine::sample::SAMPLES;
    let mut chosen = None;
    grid(ui, list.len(), |ui, i| {
        let s = &list[i];
        let pic = app.home.thumbs.entry(s.name).or_insert_with(|| thumbnail(s.name));
        if card(ui, pic.as_mut(), "sample", s.name, s.label, false, &format!("sample:{}", s.name)).on_hover_text(s.label).clicked() {
            chosen = Some(s.name.to_owned());
        }
    });
    if let Some(name) = chosen {
        files::request(app, Then::Sample(name));
    }
}

/// The settings new projects start with and the window's look, also under the app bar's gear.
pub fn settings(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    egui::Grid::new("settings").num_columns(2).spacing(vec2(t.space_12, t.space_8)).show(ui, |ui| {
        ui.label(egui::RichText::new("Length").color(t.text_dim));
        ui.horizontal(|ui| {
            for (u, name) in [(Length::Millimetre, "mm"), (Length::Metre, "m")] {
                let r = ui.selectable_label(app.prefs.length == u, name);
                scenario::publish_target(&format!("settings:length:{name}"), r.rect);
                if r.clicked() {
                    app.prefs.length = u;
                }
            }
        });
        ui.end_row();
        ui.label(egui::RichText::new("Frequency").color(t.text_dim));
        ui.horizontal(|ui| {
            for (u, name) in [(Frequency::Gigahertz, "GHz"), (Frequency::Megahertz, "MHz"), (Frequency::Hertz, "Hz")] {
                let r = ui.selectable_label(app.prefs.frequency == u, name);
                scenario::publish_target(&format!("settings:frequency:{name}"), r.rect);
                if r.clicked() {
                    app.prefs.frequency = u;
                }
            }
        });
        ui.end_row();
        ui.label(egui::RichText::new("Theme").color(t.text_dim));
        ui.horizontal(|ui| {
            for (dark, name) in [(false, "Light"), (true, "Dark")] {
                let r = ui.selectable_label(app.dark == dark, name);
                scenario::publish_target(&format!("settings:theme:{}", name.to_lowercase()), r.rect);
                if r.clicked() {
                    app.dark = dark;
                }
            }
        });
        ui.end_row();
        ui.label(egui::RichText::new("Dialogs").color(t.text_dim));
        ui.horizontal(|ui| {
            for (docked, name) in [(false, "Floating"), (true, "Docked")] {
                let r = ui.selectable_label(app.dock_dialogs == docked, name);
                scenario::publish_target(&format!("settings:dialogs:{}", name.to_lowercase()), r.rect);
                if r.clicked() {
                    app.dock_dialogs = docked;
                    if let Some(d) = app.dialog.as_mut() {
                        d.docked = docked;
                    }
                }
            }
        });
        ui.end_row();
    });
    ui.add_space(t.space_4);
    ui.label(
        egui::RichText::new("Units apply to new projects; an open one changes its own with MODEL › Units.").size(t.font_small).color(t.text_dim),
    );
}

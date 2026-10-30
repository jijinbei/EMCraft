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

use crate::action::Pref;
use crate::files::{FileAction, Then};
use crate::glyphs::{self, Glyph};
use crate::handle::{Fixed, Handle};
use crate::scene3d::{self, Overlay};
use crate::theme::Tokens;
use crate::{Action, Actions, EmcraftApp, Picture, UiExt, scenario};

/// A card's picture, in points.
const THUMB: [f32; 2] = [240.0, 150.0];

#[derive(Default)]
pub struct Home {
    pub open: bool,
    /// Each sample's picture, drawn the first time the page shows it.
    pub(crate) thumbs: Thumbs,
}

/// The samples' pictures, by name, with the theme they were drawn in (`None`: the sample
/// could not be drawn).
pub type Thumbs = HashMap<&'static str, (crate::theme::ThemeKey, Option<Picture>)>;

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
    let t = &Tokens::get();
    let rgb = crate::theme::rgb;
    let opt = view3d::Options { width: w, height: h, supersample: 2, background: (rgb(t.viewport_top), rgb(t.viewport_bottom)), line_width: 2.0 };
    Some(Picture::new(&view3d::render(&built.scene, &cam, &opt)))
}

pub fn show(app: &EmcraftApp, thumbs: &mut Thumbs, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin::symmetric(36, 24))).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("EMCraft").size(30.0).strong().color(t.text));
                ui.label(egui::RichText::new("Start").size(30.0).color(t.text_dim));
            });
            ui.add_space(t.space_12);
            ui.horizontal(|ui| {
                if big(ui, Glyph::New, "New project", true, Fixed::HomeNew) {
                    out.push(Action::File(FileAction::Request(Then::New)));
                }
                if big(ui, Glyph::OpenFile, "Open…", false, Fixed::HomeOpen) {
                    out.push(Action::File(FileAction::Request(Then::Open)));
                }
            });
            ui.add_space(t.space_16);
            let right = 300.0;
            ui.horizontal_top(|ui| {
                let left = (ui.available_width() - right - 32.0).max(THUMB[0] + 16.0);
                ui.vertical(|ui| {
                    ui.set_width(left);
                    recent(app, ui, out);
                    ui.add_space(t.space_16);
                    samples(thumbs, ui, out);
                });
                ui.add_space(32.0);
                ui.vertical(|ui| {
                    ui.set_width(right);
                    heading(ui, "New projects");
                    settings(app, ui, out);
                });
            });
        });
    });
}

fn heading(ui: &mut egui::Ui, text: &str) {
    let t = &Tokens::get();
    ui.label(egui::RichText::new(text).size(t.font_heading).strong().color(t.text));
    ui.add_space(t.space_4);
}

/// A large button: a glyph and a label; the primary one in the accent.
fn big(ui: &mut egui::Ui, glyph: Glyph, label: &str, primary: bool, handle: Fixed) -> bool {
    let t = &Tokens::get();
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
fn card(ui: &mut egui::Ui, pic: Option<&mut Picture>, glyph: Glyph, name: &str, sub: &str, dim: bool, handle: Handle) -> egui::Response {
    let t = &Tokens::get();
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

fn recent(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    heading(ui, "Recent");
    let list = &app.prefs.recent;
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
        let r = card(ui, None, Glyph::Waveguide, &name, &sub, !exists, Handle::Recent(i));
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
        out.push(Action::Pref(Pref::ForgetRecent(i)));
    }
    if let Some(path) = open {
        out.push(Action::File(FileAction::Request(Then::OpenPath(path))));
    }
}

fn samples(thumbs: &mut Thumbs, ui: &mut egui::Ui, out: &mut Actions) {
    heading(ui, "Samples");
    let list = emcraft_engine::sample::SAMPLES;
    let mut chosen = None;
    grid(ui, list.len(), |ui, i| {
        let s = &list[i];
        // Drawn again in a new theme (the view's ground and the scene's colours are its).
        let key = crate::theme::key();
        let slot = thumbs.entry(s.name).or_insert_with(|| (key.clone(), thumbnail(s.name)));
        if slot.0 != key {
            *slot = (key, thumbnail(s.name));
        }
        let pic = &mut slot.1;
        if card(ui, pic.as_mut(), Glyph::Sample, s.name, s.label, false, Handle::Sample(s.name)).on_hover_text(s.label).clicked() {
            chosen = Some(s.name.to_owned());
        }
    });
    if let Some(name) = chosen {
        out.push(Action::File(FileAction::Request(Then::Sample(name))));
    }
}

/// A setting's row of the settings grid: its name, then a selectable label per choice
/// (`(value, label, id, hint)`, published as `settings:<group>:<id>`); a click asks for
/// `action(value)`.
#[allow(clippy::too_many_arguments)]
fn setting<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    out: &mut Actions,
    name: &str,
    group: &'static str,
    current: T,
    choices: &[(T, &str, &str, &str)],
    action: impl Fn(T) -> Action,
) {
    let t = &Tokens::get();
    ui.label(egui::RichText::new(name).color(t.text_dim));
    ui.horizontal_wrapped(|ui| {
        for &(value, label, id, hint) in choices {
            ui.act(true, egui::Button::selectable(current == value, label), Handle::Setting(group, id), hint, || action(value), out);
        }
    });
    ui.end_row();
}

/// A theme's name as its button reads: the built-in ones capitalised.
fn theme_label(name: &str) -> &str {
    match name {
        "light" => "Light",
        "dark" => "Dark",
        other => other,
    }
}

/// The settings new projects start with and the window's look, also under the app bar's gear.
pub fn settings(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    egui::Grid::new("settings").num_columns(2).spacing(vec2(t.space_12, t.space_8)).show(ui, |ui| {
        setting(ui, out, "Length", "length", app.prefs.length, &[(Length::Millimetre, "mm", "mm", ""), (Length::Metre, "m", "m", "")], |u| {
            Action::Pref(Pref::Length(u))
        });
        let frequencies = [(Frequency::Gigahertz, "GHz", "GHz", ""), (Frequency::Megahertz, "MHz", "MHz", ""), (Frequency::Hertz, "Hz", "Hz", "")];
        setting(ui, out, "Frequency", "frequency", app.prefs.frequency, &frequencies, |u| Action::Pref(Pref::Frequency(u)));
        // Every theme there is: the built-in ones, then the user's (docs/themes.md).
        let names = app.themes.names();
        let themes: Vec<(usize, &str, &str, &str)> = app
            .themes
            .list
            .iter()
            .enumerate()
            .map(|(i, th)| (i, if th.fallback { th.name.as_str() } else { theme_label(&th.name) }, th.name.as_str(), th.description.as_str()))
            .collect();
        let current = names.iter().position(|n| *n == app.theme).unwrap_or(0);
        setting(ui, out, "Theme", "theme", current, &themes, |i| Action::Pref(Pref::Theme(names.get(i).copied().unwrap_or("light").to_owned())));
        let drawing = [
            (false, "Quality", "quality", "the CPU renderer: depth-buffered and smoothed at rest"),
            (true, "Fast", "fast", "the GPU, every frame: for large meshes"),
        ];
        setting(ui, out, "3D drawing", "3d", app.prefs.gpu3d, &drawing, |gpu| Action::Pref(Pref::Gpu3d(gpu)));
        setting(
            ui,
            out,
            "Dialogs",
            "dialogs",
            app.dock_dialogs,
            &[(false, "Floating", "floating", ""), (true, "Docked", "docked", "")],
            Action::SetDock,
        );
    });
    ui.add_space(t.space_4);
    ui.label(
        egui::RichText::new("Units apply to new projects; an open one changes its own with MODEL › Units.").size(t.font_small).color(t.text_dim),
    );
}

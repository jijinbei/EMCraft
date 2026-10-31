//! The start page, after SolidCraft's (`crates/ui-egui/src/home.rs`: New and Open, the recent
//! designs and the samples, each a card with a picture), written anew for EMCraft: New
//! project and Open, the recent projects, the samples drawn by the 3D view, and the units new
//! projects start with (the rest on the settings page). Shown on launch without a sample and from the app bar's house;
//! anything opened leaves it.

use std::collections::HashMap;

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use emcraft_engine::Session;
use emcraft_render::view3d::{self, Camera, StandardView};
use serde_json::Value;

use crate::action::Pref;
use crate::files::{FileAction, Then};
use crate::glyphs::{self, Glyph};
use crate::handle::{Fixed, Handle};
use crate::lang::{Raw, args, sample_label, tr, trf};
use crate::scene3d::{self, Overlay};
use crate::settings::Section;
use crate::theme::Tokens;
use crate::{Action, Actions, EmcraftApp, Hint, Picture, Style, UiExt, scenario};

/// A card's picture, in points.
const THUMB: [f32; 2] = [240.0, 150.0];

#[derive(Default)]
pub struct Home {
    pub open: bool,
    /// What the job is for, chosen first: the templates shown are those made for it, and the
    /// project opened or begun is set to it (`project.analysis`).
    pub purpose: Option<crate::steps::Solver>,
    /// Each sample's picture, drawn the first time the page shows it.
    pub(crate) thumbs: Thumbs,
}

/// The samples' pictures, by name, with the theme they were drawn in (`None`: the sample
/// could not be drawn).
pub type Thumbs = HashMap<&'static str, (crate::theme::ThemeKey, Option<Picture>)>;

/// A sample's model without its mesh or solve, drawn from the corner.
fn thumbnail(name: &str) -> Option<Picture> {
    template_picture(name, &Value::Null, [(THUMB[0] * 2.0) as usize, (THUMB[1] * 2.0) as usize]).ok().map(|(p, _)| p)
}

/// Template `name`'s model with its parameters changed to `values`, without its mesh or solve,
/// drawn from the corner `size` pixels, with the camera it was drawn from; why it cannot be built.
pub fn template_picture(name: &str, values: &Value, size: [usize; 2]) -> Result<(Picture, Camera), String> {
    let steps: Vec<emcraft_engine::Step> = emcraft_engine::template_steps(name, values)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|s| !s.command.starts_with("mesh.") && !s.command.starts_with("solve."))
        .collect();
    let mut s = Session::new();
    s.run_steps(&steps).map_err(|e| e.to_string())?;
    let built = scene3d::build(&s, Overlay::Materials);
    let (lo, hi) = built.bounds.ok_or("nothing to draw")?;
    let [w, h] = size;
    let mut cam = Camera::default();
    cam.standard(StandardView::Iso);
    cam.fit(lo, hi, w as f64 / h.max(1) as f64);
    cam.distance *= 0.8;
    let t = &Tokens::get();
    let rgb = crate::theme::rgb;
    let opt = view3d::Options { width: w, height: h, supersample: 2, background: (rgb(t.viewport_top), rgb(t.viewport_bottom)), line_width: 2.0 };
    Ok((Picture::new(&view3d::render(&built.scene, &cam, &opt)), cam))
}

pub fn show(app: &EmcraftApp, thumbs: &mut Thumbs, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin::symmetric(36, 24))).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.text(Style::Title, Raw("EMCraft"));
                ui.label(egui::RichText::new(tr("Start")).size(30.0).color(t.text_dim));
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
                    recover(app, ui, out);
                    recent(app, ui, out);
                    ui.add_space(t.space_16);
                    purposes(app, ui, out);
                    ui.add_space(t.space_16);
                    samples(app, thumbs, ui, out);
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
    ui.text(Style::Heading, text);
    ui.add_space(Tokens::get().space_4);
}

/// A large button: a glyph and a label; the primary one in the accent.
fn big(ui: &mut egui::Ui, glyph: Glyph, label: &str, primary: bool, handle: Fixed) -> bool {
    let t = &Tokens::get();
    let label = tr(label);
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

/// The autosaved copies found on start, newer than their projects: each a compact card — a
/// glyph, the project's name and how old the copy is (the files on hover) — with Recover and a
/// dismiss that deletes it.
fn recover(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let found = &app.autosave.found;
    if found.is_empty() {
        return;
    }
    let t = &Tokens::get();
    for (i, f) in found.iter().enumerate() {
        let tip = match &f.original {
            Some(o) => trf("Autosaved copy of {project}\n{file}", args!(project = o, file = f.file.display())),
            None => trf("Autosaved copy of a project never saved\n{file}", args!(file = f.file.display())),
        };
        let r = t.card(t.accent_soft).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width().min(520.0));
            ui.horizontal(|ui| {
                let (g, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                glyphs::paint(ui.painter(), g, Glyph::Save, t.icon, t.accent);
                ui.text(Style::Strong, Raw(&f.name));
                ui.text(Style::SmallDim, crate::autosave::ago(f.modified));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let x = ui.add(egui::Button::new("×").frame(false));
                    scenario::publish_target(Handle::RecoverDismiss(i), x.rect);
                    if x.hint("Discard the copy").clicked() {
                        out.push(Action::DismissRecovery(i));
                    }
                    let then = Then::Recover { file: f.file.display().to_string(), original: f.original.clone() };
                    ui.act(
                        !app.busy(),
                        crate::ui_kit::styled("Recover", true),
                        Handle::Recover(i),
                        "",
                        || Action::File(FileAction::Request(then)),
                        out,
                    );
                });
            });
        });
        r.response.hint(Raw(&tip));
        ui.add_space(t.space_4);
    }
    ui.add_space(t.space_12);
}

fn recent(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    heading(ui, "Recent");
    let list = &app.prefs.recent;
    if list.is_empty() {
        ui.text(Style::SmallDim, "Projects you open or save appear here.");
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
        let sub = if exists { folder } else { tr("missing").to_owned() };
        let r = card(ui, None, Glyph::Waveguide, &name, &sub, !exists, Handle::Recent(i));
        let r = r.on_hover_text(path.as_str());
        r.context_menu(|ui| {
            if ui.button(tr("Remove from the list")).clicked() {
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

/// What the job is for: the four kinds of analysis as cards, the one chosen marked; a click
/// chooses it (again: lets it go, every template shown).
fn purposes(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    heading(ui, "Purpose");
    ui.horizontal(|ui| {
        for kind in crate::steps::Solver::ALL {
            let crate::steps::KindCard { glyph, name, detail } = crate::steps::kind_card(kind);
            let on = app.home.purpose == Some(kind);
            let (r, resp) = ui.allocate_exact_size(vec2(150.0, 76.0), Sense::click());
            let fill = if on {
                t.accent_soft
            } else if resp.hovered() {
                t.hover
            } else {
                t.panel_header
            };
            let stroke = if on { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.border) };
            ui.painter().rect(r, 10.0, fill, stroke, egui::StrokeKind::Inside);
            glyphs::paint(ui.painter(), Rect::from_center_size(pos2(r.center().x, r.top() + 28.0), vec2(34.0, 34.0)), glyph, t.icon, t.accent);
            ui.painter().text(
                pos2(r.center().x, r.bottom() - 14.0),
                Align2::CENTER_CENTER,
                tr(name),
                FontId::proportional(t.font_body),
                if on { t.text } else { t.text_dim },
            );
            scenario::publish_target(Handle::Purpose(kind), r);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr(name)));
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).hint(format!("{}\n{}", tr(name), tr(detail))).clicked() {
                out.push(Action::Purpose((!on).then_some(kind)));
            }
        }
    });
}

fn samples(app: &EmcraftApp, thumbs: &mut Thumbs, ui: &mut egui::Ui, out: &mut Actions) {
    heading(ui, "Templates");
    let purpose = app.home.purpose;
    let list: Vec<&emcraft_engine::templates::Template> =
        emcraft_engine::templates::TEMPLATES.iter().filter(|s| purpose.is_none_or(|k| s.kinds.contains(&k))).collect();
    let mut chosen = None;
    let mut blank = false;
    // With a purpose, the first card begins an empty project for it.
    let offset = usize::from(purpose.is_some());
    grid(ui, list.len() + offset, |ui, i| {
        if i < offset {
            if card(ui, None, Glyph::New, tr("Blank"), tr("An empty project for this purpose"), false, Fixed::HomeBlank.into()).clicked() {
                blank = true;
            }
            return;
        }
        let s = list[i - offset];
        // Drawn again in a new theme (the view's ground and the scene's colours are its).
        let key = crate::theme::key();
        let slot = thumbs.entry(s.name).or_insert_with(|| (key.clone(), thumbnail(s.name)));
        if slot.0 != key {
            *slot = (key, thumbnail(s.name));
        }
        let pic = &mut slot.1;
        let about = sample_label(s.name);
        if card(ui, pic.as_mut(), Glyph::Sample, s.name, about, false, Handle::Sample(s.name)).hint(Raw(about)).clicked() {
            chosen = Some(s.name.to_owned());
        }
    });
    if let Some(name) = chosen {
        out.push(Action::Template(Some(name)));
    }
    if blank {
        out.push(Action::File(FileAction::Request(Then::New)));
    }
}

/// The units new projects start with, and the way to every other setting.
fn settings(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    crate::settings::unit_choices(app, ui, out);
    ui.add_space(t.space_4);
    ui.text(Style::SmallDim, "Units apply to new projects; an open one changes its own with MODEL › Units.");
    ui.add_space(t.space_12);
    let all = crate::ui_kit::styled("All settings…", false);
    ui.act(true, all, Fixed::HomeSettings.into(), "The theme, the 3D view, dialogs and files", || Action::Settings(Some(Section::Appearance)), out);
}

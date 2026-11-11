//! A template opened from the start page or File ▸ Samples: a modal card with the template's
//! parameters on the left and its model drawn on the right, drawn again as they change; Open
//! builds the project from it (`file.sample` with the values changed).

use egui::{Sense, vec2};
use serde_json::{Value, json};

use crate::files::{FileAction, Then};
use crate::handle::{Fixed, Handle};
use crate::lang::{Raw, sample_label};
use crate::pending::Pending;
use crate::theme::Tokens;
use crate::{Action, Actions, EmcraftApp, Picture, Style, UiExt, scenario, ui_kit};
pub use emcraft_ui_core::template_card::{Field, TemplateCard};

/// The preview's size, in points, and in pixels (drawn at twice it).
const PREVIEW: [f32; 2] = [520.0, 340.0];
const PREVIEW_PIXELS: [usize; 2] = [(PREVIEW[0] * 2.0) as usize, (PREVIEW[1] * 2.0) as usize];
/// The parameters' column and their labels' (labels wrap within it).
const FIELDS_WIDTH: f32 = 320.0;
const LABEL_WIDTH: f32 = 160.0;

/// What the card shows of the model: the preview and what it was drawn from (the template, the
/// fields' texts and the theme), the camera it was drawn from and where the parameters show
/// on it, and the one being drawn off the frame ([`crate::home::draw_template`]).
#[derive(Default)]
pub struct Preview {
    picture: Option<Picture>,
    drawn: Option<(String, Vec<String>, crate::theme::ThemeKey)>,
    camera: Option<emcraft_render::view3d::Camera>,
    marks: Vec<emcraft_engine::TemplateMark>,
    drawing: Option<Pending<Result<crate::home::Drawing, String>>>,
}

impl Preview {
    /// Draws the preview again, off the frame, when `card`'s fields or the theme changed since
    /// it was drawn; takes it once drawn. `card` learns why the model cannot be built.
    fn refresh(&mut self, card: &mut TemplateCard, ctx: &egui::Context) {
        if self.drawn.as_ref().is_some_and(|(name, ..)| *name != card.name) {
            *self = Preview::default();
        }
        let key = (card.name.clone(), card.texts(), crate::theme::key());
        if self.drawn.as_ref() != Some(&key) {
            self.drawn = Some(key);
            // A drawing begun before is let go: only the latest fields' picture is shown.
            self.drawing =
                card.values().map(|v| crate::home::draw_template(ctx, &card.name, v, PREVIEW_PIXELS)).map_err(|e| card.error = Some(e)).ok();
        }
        let Some(done) = self.drawing.as_ref().and_then(Pending::take) else { return };
        self.drawing = None;
        match done {
            Ok(d) => {
                self.picture = Some(Picture::new(&d.canvas));
                self.camera = Some(d.camera);
                self.marks = card.values().ok().and_then(|v| emcraft_engine::template_marks(&card.name, &v).ok()).unwrap_or_default();
                card.error = None;
            }
            // The last good picture stays, with why this one cannot be built.
            Err(e) => card.error = Some(e),
        }
    }

    /// `card` as the control channel reports it, with what the preview shows.
    pub fn inspect(&self, card: &TemplateCard) -> Value {
        let mut v = card.inspect();
        v["preview"] = json!(self.picture.is_some());
        v["marks"] = json!(self.marks.len());
        v
    }
}

/// The template's card: its parameters, its preview and Open, Cancel and Reset.
pub fn show(dlg: &mut TemplateCard, preview: &mut Preview, ctx: &egui::Context, out: &mut Actions) {
    preview.refresh(dlg, ctx);
    let t = &Tokens::get();
    let modal = egui::Modal::new(egui::Id::new("emcraft_template")).frame(t.card(t.dialog_body).inner_margin(16)).show(ctx, |ui| {
        // What it is in brief, its name (the sample `file.sample` opens) in a chip.
        ui.horizontal(|ui| {
            ui.text(Style::Heading, Raw(&crate::ui_ext::brief(sample_label(&dlg.name))));
            ui.add_space(8.0);
            egui::Frame::NONE.fill(t.panel_header).corner_radius(t.radius_sm).inner_margin(egui::Margin::symmetric(5, 1)).show(ui, |ui| {
                ui.label(egui::RichText::new(&dlg.name).monospace().size(t.font_small).color(t.text_dim));
            });
        });
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            fields(ui, dlg);
            picture(ui, dlg, preview);
        });
        if let Some(e) = &dlg.error {
            ui_kit::small(ui, Raw(e), t.form_error);
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let ready = dlg.error.is_none();
            let values = dlg.values().unwrap_or(Value::Null);
            let name = dlg.name.clone();
            ui.act(ready, ui_kit::styled(t!("Open"), true), Fixed::TemplateOpen.into(), Action::OpenTemplate(name, values), out);
            ui.act(true, ui_kit::styled(t!("Cancel"), false), Fixed::TemplateCancel.into(), Action::Template(None), out);
            if ui.named(egui::Button::new(crate::lang::tr(t!("Reset"))).frame(false), Fixed::TemplateReset.into()).clicked() {
                *dlg = dlg.clone().reset();
            }
        });
    });
    if modal.should_close() {
        out.push(Action::Template(None));
    }
}

/// The parameters' fields; the one under the pointer, else the one being typed in, is the
/// dialog's active one.
fn fields(ui: &mut egui::Ui, dlg: &mut TemplateCard) {
    let t = &Tokens::get();
    ui.vertical(|ui| {
        ui.set_width(FIELDS_WIDTH);
        ui_kit::section(ui, t!("Parameters"));
        egui::ScrollArea::vertical().id_salt("template_fields").auto_shrink([false, true]).max_height(PREVIEW[1]).show(ui, |ui| {
            if dlg.fields.is_empty() {
                ui_kit::caption(ui, t!("none"));
            }
            let (mut hovered, mut focused) = (None, None);
            for f in &mut dlg.fields {
                let bad = f.value().is_none();
                // Its label in words (wrapped, never cut), its name (what expressions use) dim
                // under it; then its field and unit.
                let (focus, over) = ui
                    .horizontal(|ui| {
                        ui.allocate_ui_with_layout(vec2(LABEL_WIDTH, t.control_height), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_width(LABEL_WIDTH);
                            match &f.label {
                                Some(l) => {
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(crate::lang::parameter_label(l)).size(t.font_label).color(t.form_ink))
                                            .wrap(),
                                    );
                                    ui.label(egui::RichText::new(&f.name).monospace().size(t.font_small - 1.0).color(t.text_dim));
                                }
                                None => {
                                    ui.label(egui::RichText::new(&f.name).size(t.font_label).color(t.form_ink));
                                }
                            }
                        });
                        let mut edit = egui::TextEdit::singleline(&mut f.text).desired_width(80.0).char_limit(32);
                        if bad {
                            edit = edit.background_color(t.form_error_fill).text_color(t.form_error);
                        }
                        let r = ui.named(edit, Handle::TemplateField(&f.name));
                        ui_kit::small(ui, Raw(f.unit), t.text_dim);
                        (r.has_focus(), ui.rect_contains_pointer(ui.min_rect()))
                    })
                    .inner;
                ui.add_space(2.0);
                if over {
                    hovered = Some(f.name.clone());
                }
                if focus {
                    focused = Some(f.name.clone());
                }
            }
            dlg.active = hovered.or(focused);
        });
    });
}

/// The model as the fields make it (dimmed when they do not make one), with the active
/// parameter's dimension on it.
fn picture(ui: &mut egui::Ui, dlg: &TemplateCard, preview: &mut Preview) {
    let (r, _) = ui.allocate_exact_size(vec2(PREVIEW[0], PREVIEW[1]), Sense::hover());
    scenario::publish_target(Fixed::TemplatePreview, r);
    if let Some(p) = preview.picture.as_mut() {
        let id = p.texture(ui.ctx());
        let mut image = egui::Image::new((id, vec2(PREVIEW[0], PREVIEW[1])));
        if dlg.error.is_some() {
            image = image.tint(egui::Color32::WHITE.gamma_multiply(0.45));
        }
        image.paint_at(ui, r);
    }
    if let (Some(cam), Some(name), None) = (&preview.camera, &dlg.active, &dlg.error)
        && let Some(m) = preview.marks.iter().find(|m| &m.name == name)
    {
        let field = dlg.fields.iter().find(|f| &f.name == name);
        let (value, unit) = field.map_or(("", ""), |f| (f.text.as_str(), f.unit));
        let shown = field.and_then(|f| f.label.as_deref()).map_or(name.as_str(), crate::lang::parameter_label);
        mark(ui.painter(), cam, r, m, &format!("{shown} = {value} {unit}"));
    }
}

/// Opens the template: its project built from the values (after asking about unsaved changes).
pub fn open(app: &mut EmcraftApp, name: String, values: Value) {
    app.ui_state.close(crate::ui_state::Layer::Template);
    let template = emcraft_engine::templates::find(&name);
    let purpose =
        app.home.purpose.filter(|p| template.is_some_and(|t| t.kinds.contains(p))).or_else(|| template.and_then(|t| t.kinds.first().copied()));
    crate::files::apply(app, FileAction::Request(Then::Sample { name, values, purpose }));
}

/// A parameter's dimension on the preview: a line between its two points with a head at each
/// end, and its name and value beside its middle.
fn mark(p: &egui::Painter, cam: &emcraft_render::view3d::Camera, rect: egui::Rect, m: &emcraft_engine::TemplateMark, text: &str) {
    let t = &Tokens::get();
    let (Some(a), Some(b)) = (crate::tools3d::screen(cam, rect, m.from), crate::tools3d::screen(cam, rect, m.to)) else { return };
    let p = p.with_clip_rect(rect);
    let s = egui::Stroke::new(2.5, t.accent);
    let d = b - a;
    let len = d.length();
    if len > 1.0 {
        let u = d / len;
        let n = vec2(-u.y, u.x);
        let head = 8.0f32.min(len / 3.0);
        p.line_segment([a, b], s);
        for (tip, dir) in [(b, u), (a, -u)] {
            p.add(egui::Shape::convex_polygon(
                vec![tip, tip - dir * head + n * head * 0.45, tip - dir * head - n * head * 0.45],
                t.accent,
                egui::Stroke::NONE,
            ));
        }
        // Short ticks across the ends, as a drawing's dimension has.
        for q in [a, b] {
            p.line_segment([q - n * 6.0, q + n * 6.0], egui::Stroke::new(1.2, t.accent));
        }
    } else {
        p.circle_filled(a, 4.0, t.accent);
    }
    let g = p.layout_no_wrap(text.to_owned(), egui::FontId::proportional(t.font_small), t.on_accent);
    let mid = a + d / 2.0;
    let box_ = egui::Rect::from_center_size(mid + vec2(0.0, -14.0), g.size() + vec2(10.0, 4.0));
    p.rect_filled(box_, 4.0, t.accent);
    p.galley(box_.min + vec2(5.0, 2.0), g, t.on_accent);
    scenario::publish_target(Fixed::TemplateMark, box_);
}

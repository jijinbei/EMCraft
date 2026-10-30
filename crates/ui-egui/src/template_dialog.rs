//! A template opened from the start page or File ▸ Samples: a modal card with the template's
//! parameters on the left and its model drawn on the right, drawn again as they change; Open
//! builds the project from it (`file.sample` with the values changed).

use egui::{Sense, vec2};
use serde_json::{Map, Value, json};

use crate::files::{FileAction, Then};
use crate::handle::{Fixed, Handle};
use crate::lang::{Raw, sample_label};
use crate::theme::Tokens;
use crate::{Action, Actions, EmcraftApp, Picture, Style, UiExt, scenario, ui_kit};

/// The preview's size, in points.
const PREVIEW: [f32; 2] = [520.0, 340.0];

/// A parameter's field: its name, its text, its unit and the template's own value.
pub struct Field {
    pub name: String,
    pub text: String,
    pub unit: &'static str,
    pub default: f64,
}

/// The open template.
pub struct TemplateDialog {
    pub name: String,
    pub fields: Vec<Field>,
    /// The preview, and what it was drawn from (the fields' texts and the theme).
    preview: Option<Picture>,
    drawn: Option<(Vec<String>, crate::theme::ThemeKey)>,
    /// Why the model cannot be built from the fields as they stand.
    pub error: Option<String>,
}

impl TemplateDialog {
    /// Template `name` with its own values (`None`: no such template).
    pub fn new(app: &EmcraftApp, name: &str) -> Option<TemplateDialog> {
        let d = app.session.display();
        let fields = emcraft_engine::template_parameters(name)?
            .into_iter()
            .map(|p| Field {
                unit: match p.quantity.as_deref() {
                    Some("length") => d.length.symbol(),
                    Some("frequency") => d.frequency.symbol(),
                    _ => "",
                },
                text: crate::steps::fmt(p.value),
                default: p.value,
                name: p.name,
            })
            .collect();
        Some(TemplateDialog { name: name.to_owned(), fields, preview: None, drawn: None, error: None })
    }

    /// The values changed from the template's, by name; an error naming a field that is not a
    /// number.
    pub fn values(&self) -> Result<Value, String> {
        let mut map = Map::new();
        for f in &self.fields {
            let v = f.text.trim().parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("{}: not a number", f.name))?;
            if v != f.default {
                map.insert(f.name.clone(), json!(v));
            }
        }
        Ok(Value::Object(map))
    }

    /// Draws the preview again when the fields or the theme changed since it was drawn.
    fn refresh(&mut self) {
        let key = (self.fields.iter().map(|f| f.text.clone()).collect::<Vec<_>>(), crate::theme::key());
        if self.drawn.as_ref() == Some(&key) {
            return;
        }
        self.drawn = Some(key);
        let size = [(PREVIEW[0] * 2.0) as usize, (PREVIEW[1] * 2.0) as usize];
        match self.values().and_then(|v| crate::home::template_picture(&self.name, &v, size)) {
            Ok(p) => {
                self.preview = Some(p);
                self.error = None;
            }
            // The last good picture stays, with why this one cannot be built.
            Err(e) => self.error = Some(e),
        }
    }

    pub fn inspect(&self) -> Value {
        let fields: Map<String, Value> = self.fields.iter().map(|f| (f.name.clone(), json!(f.text))).collect();
        json!({"name": self.name, "fields": fields, "error": self.error, "preview": self.preview.is_some()})
    }
}

pub fn show(app: &EmcraftApp, dlg: &mut TemplateDialog, ctx: &egui::Context, out: &mut Actions) {
    dlg.refresh();
    let t = &Tokens::get();
    let busy = app.busy();
    let modal = egui::Modal::new(egui::Id::new("emcraft_template")).frame(t.card(t.dialog_body).inner_margin(16)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.text(Style::Heading, Raw(&dlg.name));
            ui.add_space(8.0);
            ui_kit::small(ui, sample_label(&dlg.name), t.text_dim);
        });
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            // The parameters.
            ui.vertical(|ui| {
                ui.set_width(250.0);
                ui_kit::section(ui, "Parameters");
                egui::ScrollArea::vertical().id_salt("template_fields").max_height(PREVIEW[1]).show(ui, |ui| {
                    if dlg.fields.is_empty() {
                        ui_kit::caption(ui, "none");
                    }
                    for f in &mut dlg.fields {
                        ui_kit::row(ui, Raw(&f.name), false, |ui| {
                            let bad = f.text.trim().parse::<f64>().map_or(true, |v| !v.is_finite());
                            let mut edit = egui::TextEdit::singleline(&mut f.text).desired_width(90.0).char_limit(32);
                            if bad {
                                edit = edit.background_color(t.form_error_fill).text_color(t.form_error);
                            }
                            ui.named(edit, Handle::TemplateField(&f.name));
                            ui_kit::small(ui, Raw(f.unit), t.text_dim);
                        });
                    }
                });
            });
            // The model as the fields make it.
            let (r, _) = ui.allocate_exact_size(vec2(PREVIEW[0], PREVIEW[1]), Sense::hover());
            scenario::publish_target(Fixed::TemplatePreview, r);
            if let Some(p) = dlg.preview.as_mut() {
                let id = p.texture(ui.ctx());
                let mut image = egui::Image::new((id, vec2(PREVIEW[0], PREVIEW[1])));
                if dlg.error.is_some() {
                    image = image.tint(egui::Color32::WHITE.gamma_multiply(0.45));
                }
                image.paint_at(ui, r);
            }
        });
        if let Some(e) = &dlg.error {
            ui_kit::small(ui, Raw(e), t.form_error);
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let ready = dlg.error.is_none() && !busy;
            let values = dlg.values().unwrap_or(Value::Null);
            let name = dlg.name.clone();
            ui.act(ready, ui_kit::styled("Open", true), Fixed::TemplateOpen.into(), "", || Action::OpenTemplate(name, values), out);
            ui.act(true, ui_kit::styled("Cancel", false), Fixed::TemplateCancel.into(), "", || Action::Template(None), out);
            if ui.named(egui::Button::new(crate::lang::tr("Reset")).frame(false), Fixed::TemplateReset.into()).clicked() {
                for f in &mut dlg.fields {
                    f.text = crate::steps::fmt(f.default);
                }
            }
        });
    });
    if modal.should_close() {
        out.push(Action::Template(None));
    }
}

/// Opens the template: its project built from the values (after asking about unsaved changes).
pub fn open(app: &mut EmcraftApp, name: String, values: Value) {
    app.template = None;
    crate::files::apply(app, FileAction::Request(Then::Sample { name, values }));
}

//! Drawing the command dialogs ([`crate::dialogs`]) as cards, after SolidCraft's Panel/Window:
//! a title band with the command's glyph, the dock switch and Close; the command's description;
//! one aligned row per parameter (tables for lists of records); the error band; Cancel and OK in
//! the footer. Floating cards can be dragged by their title; docked ones sit at the right.

use egui::{Align2, Stroke, vec2};

use crate::dialogs::{Cell, Column, Dialog, Field, Kind, MAX_FIELD, MAX_ROWS};
use crate::handle::{Fixed, Handle};
use crate::lang::{Raw, args, command_doc, param_doc, tr, trf};
use crate::theme::Tokens;
use crate::{Action, Actions, EmcraftApp, Hint, Style, UiExt, scenario, ui_kit};

/// What the card asked for this frame: at most one thing (a cancel wins over an OK pressed with
/// it, as Close does).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Asked {
    #[default]
    Nothing,
    Ok,
    Cancel,
    /// Dock or float the dialogs.
    Dock,
    /// The import dialog's file is to be chosen in the file dialog.
    Pick,
}

impl Asked {
    /// `what` asked for when `asked` (a cancel kept over anything after it).
    pub(crate) fn ask(&mut self, what: Asked, asked: bool) {
        if asked && *self != Asked::Cancel {
            *self = what;
        }
    }
}

/// A floating dialog card over the viewport.
pub fn floating(app: &EmcraftApp, d: &mut Dialog, ctx: &egui::Context, out: &mut Actions) {
    let t = &Tokens::get();
    let Some(view) = app.viewport_rect else { return };
    let default = view.right_top() + vec2(-t.form_card_width - 16.0, 16.0);
    let mut asked = Asked::default();
    let max_h = (view.height() - 32.0).max(200.0);
    let resp = egui::Area::new(egui::Id::new("emcraft_dialog"))
        .order(egui::Order::Middle)
        .default_pos(default)
        .movable(true)
        .constrain(true)
        .fade_in(false)
        .show(ctx, |ui| {
            t.card(t.dialog_body).corner_radius(t.radius_xl).show(ui, |ui| {
                ui.set_width(t.form_card_width);
                asked = card(app, d, ui, max_h, !out.allows(&Action::DialogOk));
            });
        });
    scenario::publish_rect(Fixed::Dialog, resp.response.rect);
    act(app, asked, out);
}

/// The dialog docked in a panel at the right.
pub fn docked(app: &EmcraftApp, d: &mut Dialog, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let mut asked = Asked::default();
    egui::Panel::right("emcraft_dialog_dock")
        .exact_size(t.form_card_width + 12.0)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(egui::Margin { left: 0, right: 6, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            let h = ui.available_height();
            t.card(t.dialog_body).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(h - 2.0);
                scenario::publish_rect(Fixed::Dialog, ui.max_rect());
                asked = card(app, d, ui, h - 4.0, !out.allows(&Action::DialogOk));
            });
        });
    act(app, asked, out);
}

/// Where a docked dialog was while the job its OK started runs: its title, the job's progress
/// and Cancel, so the right column stays as it was until the job ends.
pub fn docked_running(app: &EmcraftApp, d: &Dialog, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let accent = t.tab(app.tab)[0];
    egui::Panel::right("emcraft_dialog_dock")
        .exact_size(t.form_card_width + 12.0)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(egui::Margin { left: 0, right: 6, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            let h = ui.available_height();
            t.card(t.dialog_body).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(h - 22.0);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::hover());
                    crate::glyphs::paint(ui.painter(), r, crate::commands::glyph(d.command.id), t.icon, accent);
                    ui.text(Style::Heading, Raw(crate::label(d.command.id)));
                });
                ui.add_space(8.0);
                crate::messages::job_card(app, ui, out);
            });
        });
}

/// The dialog inside a step's panel.
pub fn embedded(app: &EmcraftApp, d: &mut Dialog, ui: &mut egui::Ui, max_h: f32, out: &mut Actions) {
    scenario::publish_rect(Fixed::Dialog, ui.max_rect());
    let asked = card(app, d, ui, max_h, !out.allows(&Action::DialogOk));
    act(app, asked, out);
}

/// What the card's buttons and keys ask for: docking flips where dialogs go (the open one too).
fn act(app: &EmcraftApp, asked: Asked, out: &mut Actions) {
    match asked {
        Asked::Nothing => {}
        Asked::Ok => out.push(Action::DialogOk),
        Asked::Cancel => out.push(Action::DialogCancel),
        Asked::Dock => out.push(Action::SetDock(!app.dock_dialogs)),
        Asked::Pick => out.push(Action::File(crate::files::FileAction::PickPart)),
    }
}

/// `busy`: OK waits for a running job.
fn card(app: &EmcraftApp, d: &mut Dialog, ui: &mut egui::Ui, max_h: f32, busy: bool) -> Asked {
    let t = &Tokens::get();
    let accent = t.tab(app.tab)[0];
    let session = &app.session;
    let mut asked = Asked::default();
    let id = d.command.id;
    match ui_kit::header(ui, crate::commands::glyph(id), Raw(crate::label(id)), Some(d.docked), accent) {
        Some(ui_kit::HeaderClick::Close) => asked.ask(Asked::Cancel, true),
        Some(ui_kit::HeaderClick::Dock) => asked.ask(Asked::Dock, true),
        None => {}
    }
    let body_h = (max_h - t.form_header_height - t.form_footer_height - 8.0).max(80.0);
    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
        egui::ScrollArea::vertical().max_height(body_h).auto_shrink([false, true]).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if id == crate::import_ui::ID {
                d.widgets = crate::import_ui::body(app, d, ui, &mut asked);
            } else {
                let help = ui_kit::button(ui, t!("Help"), false, Fixed::DialogHelp);
                let mut toggles = vec![(help.id, false)];
                if help.clicked() {
                    d.help = !d.help;
                }
                if d.help {
                    ui_kit::caption(ui, Raw(command_doc(id)));
                }
                ui.add_space(t.space_6);
                if id == "mesh.generate" {
                    ui_kit::caption(ui, t!("Choose an element size, or set elements per wavelength. Smaller elements increase memory and solve time; check convergence before trusting a result."));
                    let basic = |name: &str| matches!(name, "size" | "per_wavelength" | "frequency");
                    d.widgets = fields(ui, d, session, &basic, &|name| Raw(crate::lang::lookup(crate::param_labels::param_label(id, name))));
                    let options = ui_kit::button(ui, t!("Advanced settings"), false, Fixed::DialogOptions);
                    toggles.push((options.id, false));
                    if options.clicked() {
                        d.options = !d.options;
                    }
                    if d.options {
                        let drawn = fields(ui, d, session, &|name| !basic(name), &|name| Raw(crate::lang::lookup(crate::param_labels::param_label(id, name))));
                        d.widgets.extend(drawn);
                    }
                } else {
                    d.widgets = fields(ui, d, session, &|_| true, &|name| Raw(crate::lang::lookup(crate::param_labels::param_label(id, name))));
                }
                d.widgets.extend(toggles);
            }
            if let Some(e) = app.workflow.dialog_estimate.as_ref().filter(|e| e.asked["command"] == id) {
                ui.add_space(t.space_6);
                crate::steps::estimate_line(ui, &e.reply, Fixed::DialogEstimate);
            }
            if let Some(e) = &d.error {
                ui.add_space(t.space_6);
                ui_kit::error_band(ui, Raw(e));
            }
        });
    });
    // Footer.
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.form_footer_height), egui::Sense::hover());
    ui.painter().rect_filled(r, egui::CornerRadius { nw: 0, ne: 0, sw: t.radius_xl, se: t.radius_xl }, t.dialog_footer);
    ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, t.dialog_rule));
    let mut footer =
        ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(vec2(12.0, 10.0))).layout(egui::Layout::right_to_left(egui::Align::Center)));
    let ok = footer.named_enabled(!busy, ui_kit::styled(ok_label(id), true), Fixed::DialogOk.into());
    let ok = ok.hint(if busy { t!("a job is running") } else { t!("") });
    asked.ask(Asked::Ok, ok.clicked());
    let cancel = ui_kit::button(&mut footer, t!("Cancel"), false, Fixed::DialogCancel);
    asked.ask(Asked::Cancel, cancel.clicked());
    d.widgets.extend([(ok.id, false), (cancel.id, false)]);
    if busy {
        footer.text(Style::SmallDim, t!("busy…"));
    }
    keys(app, d, ui, busy, &mut asked);
    asked
}

/// The dialog's keys: Enter is OK while one of its text fields has the keyboard (or nothing
/// has), Escape closes it while one of its controls has the keyboard (or nothing has) — and
/// neither while something over it is open (the search, a drop-down, a prompt, the question
/// before changes are lost), which takes them itself. A key the dialog takes is used up.
fn keys(app: &EmcraftApp, d: &Dialog, ui: &mut egui::Ui, busy: bool, asked: &mut Asked) {
    let over = app.ui_state.searching() || app.ui_state.ask().is_some() || ui.ctx().any_popup_open();
    if over {
        return;
    }
    // Who had the keyboard when the key was pressed: egui lets go of it on Escape (and a field
    // on Enter) before the frame is drawn, so the end of the last frame says.
    let focus = app.focus_last;
    let ours = |text_only: bool| focus.is_none_or(|f| d.widgets.iter().any(|(id, text)| *id == f && (*text || !text_only)));
    let take = |ui: &mut egui::Ui, key: egui::Key| ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key));
    if ours(true) && !busy && take(ui, egui::Key::Enter) {
        asked.ask(Asked::Ok, true);
    }
}

/// The primary button's label: what a long command does (Mesh, Solve, Sweep…), OK for the
/// rest — never Run, which is the workflow's step 6.
pub fn ok_label(id: &str) -> crate::lang::Text {
    match id {
        _ if id.starts_with("mesh.") => t!("Mesh"),
        "solve.device" => t!("OK"),
        _ if id.starts_with("solve.") => t!("Solve"),
        "study.sweep" => t!("Sweep"),
        "study.optimize" => t!("Optimize"),
        "geometry.import" => t!("Import"),
        "file.open" | "file.sample" => t!("Open"),
        "results.compare" | "results.field_error" => t!("Compare"),
        _ => t!("OK"),
    }
}

/// The fields `shown` picks, each labelled by `label` (its parameter's name — an id, as scripts
/// write it — for most dialogs), its description on hover; the controls drawn, each with
/// whether it takes text.
pub(crate) fn fields<L: crate::lang::Shown>(
    ui: &mut egui::Ui,
    d: &mut Dialog,
    session: &emcraft_engine::Session,
    shown: &dyn Fn(&str) -> bool,
    label: &dyn Fn(&'static str) -> L,
) -> Vec<(egui::Id, bool)> {
    let t = &Tokens::get();
    let id = d.command.id;
    // The controls drawn, each with whether it takes text; the first text field takes the
    // keyboard when the dialog opens.
    let mut drawn: Vec<(egui::Id, bool)> = Vec::new();
    let mut focus = std::mem::take(&mut d.focus_first);
    let mut text_field = |r: &egui::Response, drawn: &mut Vec<(egui::Id, bool)>| {
        drawn.push((r.id, true));
        if std::mem::take(&mut focus) {
            r.request_focus();
        }
    };
    for f in d.fields.iter_mut().filter(|f| shown(f.name)) {
        let unit = crate::dialogs::unit(f.quantity, session);
        let name = f.name;
        let handle = Handle::DialogField(name);
        let invalid = f.error.is_some();
        let hint = param_doc(id, name);
        match &f.kind {
            Kind::Table(cols) => {
                let cols = *cols;
                drawn.extend(table(ui, f, cols, hint, session));
            }
            Kind::Bool => {
                ui_kit::row(ui, label(name), f.required, |ui| {
                    let r = crate::math::tip(ui.named(egui::Checkbox::new(&mut f.on, ""), handle), hint);
                    drawn.push((r.id, false));
                });
            }
            Kind::Vector(parts) => {
                let parts = *parts;
                // Words over each box (a face's plane, sides); single letters (x, y, z) as hints.
                let worded = parts.iter().any(|p| p.len() > 2);
                if worded {
                    ui.horizontal(|ui| {
                        ui.add_space(t.form_label_width);
                        let w = ((ui.available_width() - 30.0) / parts.len() as f32 - 6.0).max(30.0);
                        for p in parts {
                            ui.add_sized(
                                vec2(w, 12.0),
                                egui::Label::new(egui::RichText::new(crate::lang::lookup(p)).size(t.font_small).color(t.text_dim)).truncate(),
                            );
                        }
                    });
                }
                ui_kit::row(ui, label(name), f.required, |ui| {
                    let w = ((ui.available_width() - 30.0) / parts.len() as f32 - 6.0).max(30.0);
                    for (k, (label, v)) in parts.iter().zip(f.parts.iter_mut()).enumerate() {
                        let hint = if worded { "" } else { *label };
                        let r = ui.named(egui::TextEdit::singleline(v).desired_width(w).hint_text(hint).char_limit(64), Handle::DialogPart(name, k));
                        if k == 0 {
                            scenario::publish_target(handle, r.rect);
                        }
                        text_field(&r, &mut drawn);
                        crate::math::tip(r, hint);
                    }
                    if !unit.is_empty() {
                        ui.label(egui::RichText::new(unit).size(t.font_small).color(t.text_dim));
                    }
                });
            }
            Kind::Choice(list) => {
                let list = list.clone();
                ui_kit::row(ui, label(name), f.required, |ui| {
                    let r = crate::math::tip(choice(ui, (name, 0, 0), &mut f.text, &list, ui.available_width() - 8.0, !f.required), hint);
                    scenario::publish_target(handle, r.rect);
                    drawn.push((r.id, false));
                });
            }
            Kind::Json | Kind::Numbers | Kind::Number | Kind::Integer | Kind::Text => {
                let placeholder = match &f.kind {
                    Kind::Json => "JSON",
                    Kind::Numbers => "a, b, c",
                    _ if f.required => "",
                    _ => tr(t!("default")),
                };
                ui_kit::row(ui, label(name), f.required, |ui| {
                    let w = ui.available_width() - if unit.is_empty() { 8.0 } else { 40.0 };
                    let mut edit = egui::TextEdit::singleline(&mut f.text).desired_width(w).hint_text(placeholder).char_limit(MAX_FIELD);
                    if invalid {
                        edit = edit.background_color(t.form_error_fill).text_color(t.form_error);
                    }
                    let r = crate::math::tip(ui.named(edit, handle), hint);
                    text_field(&r, &mut drawn);
                    if !unit.is_empty() {
                        ui.label(egui::RichText::new(unit).size(t.font_small).color(t.text_dim));
                    }
                });
            }
        }
        if let Some(e) = &f.error {
            ui.horizontal(|ui| {
                ui.add_space(t.form_label_width + 6.0);
                ui.label(egui::RichText::new(e).size(t.font_small).color(t.form_error));
            });
        }
    }
    drawn
}

/// A table field: a row of cells per entry (sections, layers), each removable, and a button
/// that adds one. The cells drawn, each with whether it takes text.
fn table(ui: &mut egui::Ui, f: &mut Field, cols: &'static [Column], doc: &str, session: &emcraft_engine::Session) -> Vec<(egui::Id, bool)> {
    let t = &Tokens::get();
    let mut drawn = Vec::new();
    let name = f.name;
    ui_kit::section(ui, format!("{name}{}", if f.required { " *" } else { "" }));
    ui_kit::caption(ui, Raw(doc));
    let materials: Vec<String> = session.model().materials.iter().map(|m| m.name.clone()).collect();
    let mut remove = None;
    egui::Grid::new(("table", name)).num_columns(cols.len() + 1).spacing(vec2(4.0, 3.0)).show(ui, |ui| {
        for c in cols {
            let text = if c.required { format!("{}*", c.name) } else { c.name.to_owned() };
            ui.label(egui::RichText::new(text).size(t.font_small).color(t.text_dim));
        }
        ui.label("");
        ui.end_row();
        for (i, r) in f.rows.iter_mut().enumerate() {
            for (j, (c, cell)) in cols.iter().zip(r.iter_mut()).enumerate() {
                let resp = match c.cell {
                    Cell::Material => choice(ui, (name, i, j), cell, &materials, 70.0, false),
                    Cell::Number => ui.add(egui::TextEdit::singleline(cell).desired_width(44.0).char_limit(64)),
                };
                scenario::publish_target(Handle::DialogCell(name, i, c.name), resp.rect);
                drawn.push((resp.id, c.cell == Cell::Number));
            }
            let x = ui.named(egui::Button::new("×").small(), Handle::DialogRemove(name, i)).hint(t!("remove this row"));
            if x.clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        f.rows.remove(i);
    }
    let add = ui.named_enabled(
        f.rows.len() < MAX_ROWS,
        egui::Button::new(trf(t!("Add {row}"), args!(row = name.trim_end_matches('s')))),
        Handle::DialogAdd(name),
    );
    if add.clicked() {
        let row = cols.iter().map(|c| if c.cell == Cell::Material { "vacuum".to_owned() } else { String::new() }).collect();
        f.rows.push(row);
    }
    drawn
}

/// A drop-down of `list` writing the chosen value into `text`; `optional` adds "default".
fn choice(ui: &mut egui::Ui, id: (&str, usize, usize), text: &mut String, list: &[String], width: f32, optional: bool) -> egui::Response {
    let word = |v: &str| crate::param_labels::value_label(v).map_or_else(|| v.to_owned(), |l| crate::lang::lookup(l).to_owned());
    let shown = if text.is_empty() { tr(t!("default")).to_owned() } else { word(text) };
    let r = egui::ComboBox::from_id_salt(("choice", id.0, id.1, id.2)).selected_text(shown).width(width).show_ui(ui, |ui| {
        if optional {
            ui.selectable_value(text, String::new(), tr(t!("default")));
        }
        for v in list {
            let r = ui.selectable_value(text, v.clone(), word(v));
            scenario::publish_target(Handle::Choice(id.0, v), r.rect);
        }
    });
    let _ = Align2::LEFT_CENTER;
    r.response
}

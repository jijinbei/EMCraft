//! The browser: the session's model and results as a tree on a floating card at the left, as
//! the model is made: its bodies and what each is made of, the materials (and what uses each),
//! the region solved and what bounds it (its outer boundary, its symmetry planes), what drives
//! it, its mesh and its results. Rows
//! are read from the session every frame; clicking a result shows it, double-clicking a
//! geometry, mesh or boundary row opens its command's dialog on the model's values. A row that
//! names an item of the model (a material, a port, a body, a source) edits and deletes it: its
//! right-click menu (Edit, Delete, Show in 3D), a double-click (Edit) and the Delete key on the
//! selected row (undoable; a refusal shows as an error).
//!
//! Rows show their symbols set as mathematics ([`crate::math`]: `εr` with a lowered r, `S₂₁`)
//! while their plain text names them (handles, keys). A waveguide port's row says its part in
//! the S-parameters ([`PortRole`]): the driven input, whose reflection is S11, or an output,
//! the transmission to it S21; its menu drives it (`view.driven`).

use egui::{Align2, FontId, Rect, Sense, Stroke, pos2, vec2};

use crate::glyphs::{self, Glyph};

use crate::handle::{Fixed, Handle};
use crate::lang::{self, args};
use crate::scene3d::Pick;
use crate::theme::Tokens;
use crate::{Action, Actions, EmcraftApp, Style, UiExt, View, scenario};

/// Folded sections (what is selected is the window's one selection: `View3d::selected`).
#[derive(Clone, Debug, Default)]
pub struct Browser {
    pub folded: Vec<String>,
}

pub use emcraft_ui_core::browser::{Act, PortRole, Row, face_place, periodic_text, port_roles, port_text, stage_item};

/// What a double-click on a row asks for: the dialog it edits with.
pub fn act_edit(act: Act) -> Option<Action> {
    match act {
        Act::Edit(id) => Some(Action::dialog(id)),
        Act::EditWith(id, values) => Some(Action::dialog_with(id, values)),
        _ => None,
    }
}

/// What a row is in this window: the step its edit belongs to, what a double-click asks for.
pub trait RowExt {
    /// The workflow step its edit command belongs to ([`crate::steps::Step::of_command`]).
    fn step(&self) -> Option<crate::steps::Step>;
    /// What a double-click (or Edit) asks for: its item's edit dialog, else its command's.
    fn edit_action(&self) -> Option<Action>;
}

impl RowExt for Row {
    fn step(&self) -> Option<crate::steps::Step> {
        let command = match (&self.item, &self.act) {
            (Some(item), _) => item.edit,
            (None, Act::Edit(id) | Act::EditWith(id, _)) => id,
            _ => return None,
        };
        crate::steps::Step::of_command(command)
    }

    fn edit_action(&self) -> Option<Action> {
        match &self.item {
            Some(item) => Some(Action::EditItem(item.clone())),
            None => act_edit(self.act.clone()),
        }
    }
}

/// A port role's colour (the theme's `port_input` or `port_output`).
pub fn role_colour(role: &PortRole) -> egui::Color32 {
    let sc = crate::theme::scene();
    crate::theme::color32(if role.input { sc.port_input } else { sc.port_output })
}

pub use emcraft_ui_core::browser::Item;

/// A port role's mark in `r`: for the input a wave going in with its reflection coming back,
/// for an output a wave going out of a face.
pub fn role_mark(p: &egui::Painter, r: Rect, input: bool, ink: egui::Color32) {
    let s = Stroke::new(1.6, ink);
    let (l, rt, c) = (r.left() + 1.0, r.right() - 1.0, r.center());
    let head = |p: &egui::Painter, tip: egui::Pos2, dir: f32| {
        p.add(egui::Shape::convex_polygon(vec![tip, tip + vec2(-4.0 * dir, -3.0), tip + vec2(-4.0 * dir, 3.0)], ink, Stroke::NONE));
    };
    if input {
        // The face at the right; the wave in above, the reflection back below.
        p.line_segment([pos2(rt, r.top() + 1.0), pos2(rt, r.bottom() - 1.0)], Stroke::new(2.0, ink));
        let (y0, y1) = (c.y - 3.0, c.y + 3.5);
        p.line_segment([pos2(l, y0), pos2(rt - 5.0, y0)], s);
        head(p, pos2(rt - 2.0, y0), 1.0);
        p.line_segment([pos2(rt - 3.0, y1), pos2(l + 4.0, y1)], Stroke::new(1.2, ink.gamma_multiply(0.8)));
        head(p, pos2(l + 1.0, y1), -1.0);
    } else {
        // The face at the left, the wave leaving it.
        p.line_segment([pos2(l, r.top() + 1.0), pos2(l, r.bottom() - 1.0)], Stroke::new(2.0, ink));
        p.line_segment([pos2(l + 3.0, c.y), pos2(rt - 3.0, c.y)], s);
        head(p, pos2(rt, c.y), 1.0);
    }
}

/// The row of what `pick` is: its key (its id) and itself.
pub fn row_of(app: &EmcraftApp, pick: Pick) -> Option<(String, Row)> {
    emcraft_ui_core::browser::row_of(&app.session, &app.replies, pick)
}

/// The rows of what `picks` are, in their order (those without a row left out).
pub fn rows_of(app: &EmcraftApp, picks: &[Pick]) -> Vec<(String, Row)> {
    emcraft_ui_core::browser::rows_of(&app.session, &app.replies, picks)
}

/// The tree's sections and rows for the window's session ([`emcraft_ui_core::browser::tree`]).
pub fn tree(app: &EmcraftApp) -> Vec<(&'static str, Vec<Row>)> {
    emcraft_ui_core::browser::tree(&app.session, &app.replies)
}

/// The items of the model the selected parts in 3D name, in their order.
pub fn selected_items(app: &EmcraftApp) -> Vec<Item> {
    rows_of(app, app.ui_state.selection()).into_iter().filter_map(|(_, r)| r.item).collect()
}

/// Whether a row shows selected: one of the parts selected in 3D, or (a row with no part) the
/// row clicked.
fn row_selected(app: &EmcraftApp, r: &Row) -> bool {
    match r.pick {
        Some(p) => app.ui_state.selection().contains(&p),
        None => app.ui_state.selection().row() == Some(r.id.as_str()),
    }
}

/// The browser card.
pub fn show(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    egui::Panel::left("emcraft_browser")
        .default_size(300.0)
        .size_range(200.0..=480.0)
        .resizable(true)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(egui::Margin { left: 6, right: 0, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            t.card(t.panel).inner_margin(egui::Margin::symmetric(6, 6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height());
                scenario::publish_rect(Fixed::Browser, ui.max_rect());
                ui.text(Style::SmallDim, t!("BROWSER"));
                let tree = tree(app);
                delete_key(app, ui, &tree, out);
                let stale = crate::steps::results_stale(app);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for (section, rows) in &tree {
                        // An empty section stays folded, its heading dim.
                        let folded = rows.is_empty() || app.browser.folded.iter().any(|f| f == section);
                        section_header(ui, section, rows.len(), folded, out);
                        if folded {
                            continue;
                        }
                        for r in rows {
                            browser_row(app, ui, r, stale && *section == "Results", out);
                        }
                    }
                });
            });
        });
}

/// A section's glyph, before its heading.
fn section_glyph(section: &str) -> Glyph {
    match section {
        "Variables" => Glyph::Variable,
        "Bodies" => Glyph::Box,
        "Materials" => Glyph::Material,
        "Domain" => Glyph::Region,
        "Excitation" => Glyph::Port,
        "Mesh" => Glyph::Mesh,
        "Results" => Glyph::Chart,
        "PCB" => Glyph::Part,
        "Source" => Glyph::Radiation,
        "Stages" => Glyph::Lumped,
        "Receiver" => Glyph::Table,
        "Measurements" => Glyph::Touchstone,
        _ => Glyph::Blank,
    }
}

/// A section's heading row: its chevron, glyph, name and count, the heading standing out
/// ([`crate::theme::Emphasis::Primary`]); a click folds or unfolds it.
fn section_header(ui: &mut egui::Ui, section: &'static str, count: usize, folded: bool, out: &mut Actions) {
    let t = &Tokens::get();
    let e = crate::theme::Emphasis::Primary;
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), t.size(e) + 14.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, section));
    if resp.hovered() {
        ui.painter().rect_filled(r, t.radius_sm, t.hover);
    }
    chevron(ui, pos2(r.left() + 9.0, r.center().y), !folded, t.text_dim);
    // An empty section's heading is dim.
    let ink = if count == 0 { t.text_dim } else { t.ink(e) };
    let icon = t.size(e) + 2.0;
    let glyph = Rect::from_min_size(pos2(r.left() + 18.0, r.center().y - icon / 2.0), vec2(icon, icon));
    glyphs::paint(ui.painter(), glyph, section_glyph(section), ink, if count == 0 { t.text_dim } else { t.accent });
    let text = pos2(glyph.right() + 6.0, r.center().y);
    ui.painter().text(text, Align2::LEFT_CENTER, lang::lookup(section), FontId::proportional(t.size(e)), ink);
    ui.painter().text(pos2(r.right() - 6.0, r.center().y), Align2::RIGHT_CENTER, count.to_string(), FontId::proportional(t.font_small), t.text_dim);
    scenario::publish_target(Handle::Fold(section), r);
    if resp.clicked() {
        out.push(Action::Fold((*section).to_owned()));
    }
}

/// A row: its glyph, its name and, dimmer, its detail in one line (the detail cut short first),
/// the eye of a displayed object, a port's role, the out-of-date mark; a click selects it (or
/// toggles it in the selection), a double click edits it.
fn browser_row(app: &EmcraftApp, ui: &mut egui::Ui, r: &Row, out_of_date: bool, out: &mut Actions) {
    let t = &Tokens::get();
    let key = r.id.clone();
    let selected = row_selected(app, r);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    let indent = 20.0 + 16.0 * f32::from(r.depth);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, r.tip()));
    if selected {
        ui.painter().rect_filled(rect, t.radius_sm, t.selected_bg);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, t.radius_sm, t.hover);
    }
    let mark = Rect::from_center_size(pos2(rect.left() + indent + 7.0, rect.center().y), vec2(15.0, 15.0));
    match &r.role {
        Some(role) => role_mark(ui.painter(), mark, role.input, role_colour(role)),
        None => glyphs::paint(ui.painter(), mark, r.glyph, t.icon, t.accent),
    }
    // A result of a model changed after its run: dimmed, marked at the right.
    let ink = if selected {
        t.selected_ink
    } else if out_of_date {
        t.text_dim
    } else {
        t.text
    };
    let mut clip = rect;
    if let Some(object) = &r.eye {
        clip.max.x = visibility_eye(app, ui, rect, object, out).left() - 3.0;
    }
    if out_of_date {
        let c = pos2(rect.right() - 12.0, rect.center().y);
        crate::steps::stale_mark(ui.painter(), c);
        clip.max.x -= 24.0;
    }
    if let Some(role) = &r.role {
        clip.max.x = role_pill(ui, rect, role).left() - 4.0;
    }
    // The name whole, then the detail dimmer, cut short with an ellipsis where it meets the
    // pill, the eye or the edge.
    let left = rect.left() + indent + 19.0;
    let painter = ui.painter().with_clip_rect(clip);
    let name = ui.painter().layout_job(crate::math::job(&crate::math::texify(&r.shown), t.font_body, ink));
    let name_width = name.size().x.min((clip.right() - left).max(8.0));
    painter.galley(pos2(left, rect.center().y - name.size().y / 2.0), name, ink);
    if !r.detail.is_empty() {
        let from = left + name_width + 8.0;
        let dim = if selected { t.selected_ink.gamma_multiply(0.75) } else { t.text_dim };
        let mut job = crate::math::job(&r.detail_tex(), t.font_small, dim);
        job.wrap =
            egui::text::TextWrapping {
                max_width: (clip.right() - from).max(1.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…')
            };
        let detail = ui.painter().layout_job(job);
        if from < clip.right() - 12.0 {
            painter.galley(pos2(from, rect.center().y - detail.size().y / 2.0), detail, dim);
        }
    }
    scenario::publish_target(Handle::Row(&r.id), rect);
    let resp = if out_of_date {
        let tip = lang::trf(t!("{row} — out of date"), args!(row = r.tip()));
        crate::math::tip(resp, &tip)
    } else {
        crate::math::tip(resp, &r.tip())
    };
    // Shift or Ctrl toggles a part in what is selected, as a click in 3D.
    let toggles = ui.input(|i| i.modifiers.shift || i.modifiers.ctrl || i.modifiers.command);
    if resp.clicked()
        && toggles
        && let Some(p) = r.pick
    {
        let mut picks = app.ui_state.selection().clone();
        picks.toggle(p);
        let row = rows_of(app, &picks).pop().map(|(k, _)| k);
        out.push(Action::Select { row, picks: picks.to_vec() });
    } else if resp.clicked() {
        out.push(Action::Select { row: Some(key.clone()), picks: r.pick.into_iter().collect() });
        // A row of the model takes the workflow to its step (the mesh's to Mesh, a port's to
        // Excitation), whose panel shows it; a double-click edits it.
        if let Some(step) = r.step().filter(|s| app.place.step() != Some(*s)) {
            out.push(Action::OpenStep(Some(step)));
        }
        match &r.act {
            Act::Show(v) => out.push(Action::ShowView(*v)),
            Act::Run(command, params) => out.push(Action::run(command, params.clone())),
            _ => {}
        }
    }
    if resp.double_clicked() && !app.busy() {
        out.extend(r.edit_action());
    }
    if r.editable() || r.pick.is_some() {
        resp.context_menu(|ui| row_menu(app, r, &key, ui, out));
    }
}

/// The eye at the right of `rect` that shows or hides `object`; its rectangle.
fn visibility_eye(app: &EmcraftApp, ui: &egui::Ui, rect: Rect, object: &emcraft_engine::visibility::DisplayObject, out: &mut Actions) -> Rect {
    let t = &Tokens::get();
    let visible = app.session.views().visible(object);
    let name = object.name();
    let kind = object.kind();
    let eye = Rect::from_center_size(pos2(rect.right() - 13.0, rect.center().y), vec2(22.0, 20.0));
    let response = ui.interact(eye, ui.id().with(("object_eye", kind, &name)), Sense::click());
    scenario::publish_target(
        Handle::Choice(
            match kind {
                "material" => "material_visibility",
                _ => "geometry_visibility",
            },
            &name,
        ),
        eye,
    );
    let ink = if visible { t.text } else { t.text_dim };
    let c = eye.center();
    let points = vec![
        c + vec2(-8.0, 0.0),
        c + vec2(-4.0, -4.0),
        c + vec2(0.0, -5.0),
        c + vec2(4.0, -4.0),
        c + vec2(8.0, 0.0),
        c + vec2(4.0, 4.0),
        c + vec2(0.0, 5.0),
        c + vec2(-4.0, 4.0),
    ];
    ui.painter().add(egui::Shape::closed_line(points, Stroke::new(1.2, ink)));
    ui.painter().circle_filled(eye.center(), 2.0, ink);
    if !visible {
        ui.painter().line_segment([eye.left_bottom() + vec2(3.0, -3.0), eye.right_top() + vec2(-3.0, 3.0)], Stroke::new(1.4, ink));
    }
    if response.on_hover_text(lang::tr(if visible { t!("Hide object") } else { t!("Show object") })).clicked() {
        out.push(Action::run("view.visibility", serde_json::json!({"kind":kind,"name":name,"visible":!visible})));
    }
    eye
}

/// How far below the line box a subscript's glyphs reach, as a share of the font's size.
const SUBSCRIPT_DROP: f32 = 0.35;

/// A port's role at the right of its row: its mark's colour as a soft pill, the role in short
/// with the S-parameter set as mathematics. The rectangle it took.
fn role_pill(ui: &egui::Ui, rect: Rect, role: &PortRole) -> Rect {
    let t = &Tokens::get();
    let galley = ui.painter().layout_job(crate::math::rich(&role.text(), t.font_small, t.text));
    // A subscript drops below the line box by about a third of the font's size (crate::math):
    // room for it under the text.
    let drop = SUBSCRIPT_DROP * t.font_small;
    let inked = galley.rect;
    let size = inked.size() + vec2(12.0, 4.0 + drop);
    let pill = Rect::from_min_size(pos2(rect.right() - 4.0 - size.x, rect.center().y - size.y / 2.0), size);
    let ink = role_colour(role);
    ui.painter().rect_filled(pill, size.y / 2.0, ink.gamma_multiply(0.2));
    ui.painter().rect_stroke(pill, size.y / 2.0, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
    ui.painter().galley(pill.min + vec2(6.0, 2.0) - inked.min.to_vec2(), galley, t.text);
    pill
}

/// A row's right-click menu: Edit, Delete (its item's), Show in 3D (what it is there); a port
/// that is an output can be made the input (`view.driven`).
fn row_menu(app: &EmcraftApp, r: &Row, key: &str, ui: &mut egui::Ui, out: &mut Actions) {
    if let Some(role) = r.role.as_ref().filter(|role| !role.input) {
        let drive = egui::Button::new(lang::tr(t!("Make the input")));
        let params = serde_json::json!({"port": role.name});
        ui.act(true, drive, Fixed::RowMenuDrive.into(), Action::run("view.driven", params), out);
    }
    if let Some(edit) = r.edit_action() {
        ui.act(true, egui::Button::new(lang::tr(t!("Edit…"))), Fixed::RowMenuEdit.into(), edit, out);
    }
    if let Some(item) = &r.item {
        // A row among several parts selected deletes them all.
        let several = r.pick.is_some_and(|p| app.ui_state.selection().len() > 1 && app.ui_state.selection().contains(&p));
        let items = if several { selected_items(app) } else { vec![item.clone()] };
        let delete = egui::Button::new(lang::tr(t!("Delete"))).shortcut_text("Del");
        ui.act(true, delete, Fixed::RowMenuDelete.into(), Action::DeleteItems(items), out);
    }
    if let Some(pick) = r.pick {
        let show = egui::Button::new(lang::tr(t!("Show in 3D")));
        if ui.act(true, show, Fixed::RowMenuShow.into(), Action::Select { row: Some(key.to_owned()), picks: vec![pick] }, out).clicked() {
            out.push(Action::SetView(View::Model));
        }
    }
}

/// Delete deletes the items of the parts selected in 3D, else the selected row's (when no field
/// takes the key and nothing asks).
fn delete_key(app: &EmcraftApp, ui: &egui::Ui, tree: &[(&'static str, Vec<Row>)], out: &mut Actions) {
    if app.ui_state.selection().is_clear() {
        return;
    }
    if app.busy() || app.ui_state.dialog().is_some() || app.ui_state.ask().is_some() || ui.ctx().text_edit_focused() {
        return;
    }
    if !ui.input(|i| i.key_pressed(egui::Key::Delete)) {
        return;
    }
    let picked = selected_items(app);
    if !picked.is_empty() {
        out.push(Action::DeleteItems(picked));
        return;
    }
    let Some(selected) = app.ui_state.selection().row() else { return };
    let item = tree.iter().flat_map(|(_, rows)| rows.iter()).find(|r| r.id == selected);
    if let Some(item) = item.and_then(|r| r.item.clone()) {
        out.push(Action::DeleteItems(vec![item]));
    }
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

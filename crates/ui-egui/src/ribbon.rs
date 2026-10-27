//! The application bar, the ribbon (tabs, panels of command buttons) and command search, laid
//! out as SolidCraft's (`crates/ui-egui/src/toolbar.rs`: a dark application bar, the ribbon on a
//! floating card, tabs with their accent's soft fill, 60 pt buttons with the glyph over the label,
//! panel names under their buttons, the search field at the tab row's right), written anew for
//! EMCraft's commands.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::theme::Tokens;
use crate::{EmcraftApp, View, glyphs, scenario};

/// The ribbon: tabs of panels of commands, each command in one place (views are switched
/// from the view tabs over the viewport, files from the File menu and the start page).
pub const TABS: &[(&str, &[(&str, &[&str])])] = &[
    (
        "MODEL",
        &[
            ("GEOMETRY", &["geometry.import", "geometry.waveguide", "geometry.solid", "geometry.body"]),
            ("SETTINGS", &["units.set", "project.frequency"]),
        ],
    ),
    ("MATERIALS", &[("MATERIALS", &["model.material.create"]), ("BOUNDARIES", &["model.walls", "model.open", "model.symmetry", "model.periodic"])]),
    (
        "PORTS & SOURCES",
        &[("PORTS", &["model.port.create", "model.port.lumped"]), ("SOURCES", &["source.dipole", "source.plane_wave", "source.clear"])],
    ),
    ("MESH", &[("MESH", &["mesh.generate", "mesh.adapt"])]),
    (
        "SIMULATE",
        &[
            ("FREQUENCY DOMAIN", &["solve.frequency", "solve.eigenmodes", "solve.dispersion", "solve.radiation"]),
            ("TIME DOMAIN", &["solve.time_domain"]),
            ("PORTS", &["solve.port_modes"]),
            ("STUDY", &["project.variable", "study.sweep", "study.optimize"]),
            ("COMPUTE", &["solve.device"]),
        ],
    ),
    (
        "RESULTS",
        &[
            ("ANALYSIS", &["results.compare", "results.check", "results.tdr"]),
            ("EXPORT", &["results.touchstone", "view.sparameters", "view.field", "view.animation", "view.pattern", "view.dispersion"]),
        ],
    ),
];

/// A button's label: the command's own, shortened where the ribbon has little room.
pub fn button_label(id: &str) -> &str {
    match id {
        "geometry.waveguide" => "Waveguide",
        "geometry.solid" => "Region",
        "geometry.body" => "Body",
        "geometry.body.material" => "Body material",
        "geometry.feature" => "Boolean",
        "geometry.inclusion" => "Inclusion",
        "geometry.import" => "Import",
        "file.sample" => "Sample",
        "units.set" => "Units",
        "project.frequency" => "Band",
        "results.check" => "Checks",
        "results.touchstone" => "Touchstone",
        "model.material.create" => "Material",
        "model.walls" => "Walls",
        "model.open" => "Open boundary",
        "model.symmetry" => "Symmetry",
        "model.periodic" => "Periodic",
        "solve.dispersion" => "Dispersion",
        "view.dispersion" => "Dispersion PNG",
        "model.port.create" => "Port",
        "model.port.lumped" => "Lumped port",
        "source.dipole" => "Dipole",
        "source.plane_wave" => "Plane wave",
        "source.clear" => "Clear sources",
        "mesh.generate" => "Mesh",
        "solve.frequency" => "Frequency sweep",
        "solve.eigenmodes" => "Eigenmodes",
        "solve.radiation" => "Radiation",
        "solve.time_domain" => "Time domain",
        "solve.port_modes" => "Port modes",
        "solve.device" => "Device",
        "results.tdr" => "TDR",
        "project.variable" => "Variables",
        "study.sweep" => "Sweep",
        "study.optimize" => "Optimize",
        "results.compare" => "Compare",
        "view.sparameters" => "S-param PNG",
        "view.field" => "Field PNG",
        "view.animation" => "Field video",
        "view.pattern" => "Pattern PNG",
        other => crate::label(other),
    }
}

const APP_BAR_HEIGHT: f32 = 34.0;
const TAB_ROW_HEIGHT: f32 = 30.0;
const BUTTON_HEIGHT: f32 = 60.0;
const PANEL_LABEL_HEIGHT: f32 = 16.0;
const BUTTON_MIN_WIDTH: f32 = 58.0;

/// The dark application bar: the name, the samples menu, the theme switch.
pub fn app_bar(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    egui::Panel::top("emcraft_app_bar").exact_size(APP_BAR_HEIGHT).show_separator_line(false).frame(egui::Frame::NONE.fill(t.app_bar)).show(
        ui,
        |ui| {
            let r = ui.max_rect();
            let p = ui.painter();
            // The start page's house, then the wordmark.
            let house = Rect::from_center_size(pos2(r.left() + 20.0, r.center().y), vec2(26.0, 26.0));
            let home = ui.interact(house, ui.id().with("bar:start"), Sense::click());
            if home.hovered() || app.home.open {
                p.rect_filled(house, t.radius_sm, Color32::from_white_alpha(if app.home.open { 40 } else { 28 }));
            }
            glyphs::paint(p, house.shrink(4.0), "start", t.app_bar_text, t.accent);
            scenario::publish_target("bar:start", house);
            if home.on_hover_text("Start page").clicked() {
                app.home.open = !app.home.open;
            }
            let p = ui.painter();
            p.text(pos2(r.left() + 40.0, r.center().y), Align2::LEFT_CENTER, "EMCraft", FontId::proportional(15.0), t.app_bar_text);
            let mut x = r.left() + 120.0;
            let bar_button = |ui: &mut egui::Ui, x: &mut f32, text: &str, handle: &str| {
                let g = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(t.font_button), t.app_bar_text);
                let br = Rect::from_min_size(pos2(*x, r.top() + 4.0), vec2(g.size().x + 20.0, r.height() - 8.0));
                let resp = ui.interact(br, ui.id().with(("bar", handle)), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
                if resp.hovered() {
                    ui.painter().rect_filled(br, t.radius_sm, Color32::from_white_alpha(28));
                }
                ui.painter().galley(pos2(br.left() + 10.0, br.center().y - g.size().y / 2.0), g, t.app_bar_text);
                scenario::publish_target(handle, br);
                *x = br.right() + 4.0;
                resp
            };
            let busy = app.busy();
            let file = bar_button(ui, &mut x, "File", "menu:file");
            egui::Popup::menu(&file).show(|ui| crate::files::menu(app, ui));
            let edit = bar_button(ui, &mut x, "Edit", "menu:edit");
            egui::Popup::menu(&edit).show(|ui| {
                ui.set_min_width(200.0);
                let (u, r) = (app.session.undo.len(), app.session.redo.len());
                let undo = ui.add_enabled(u > 0 && !busy, egui::Button::new(format!("Undo  ({u})")).shortcut_text("Ctrl+Z"));
                scenario::publish_target("edit:undo", undo.rect);
                if undo.clicked() {
                    let _ = app.run("edit.undo", json!({}));
                }
                let redo = ui.add_enabled(r > 0 && !busy, egui::Button::new(format!("Redo  ({r})")).shortcut_text("Ctrl+Y"));
                scenario::publish_target("edit:redo", redo.rect);
                if redo.clicked() {
                    let _ = app.run("edit.redo", json!({}));
                }
            });
            // The project's name in the middle, a dot while it has unsaved changes.
            if !app.home.open {
                let title = crate::files::title(app);
                ui.painter().text(r.center(), Align2::CENTER_CENTER, &title, FontId::proportional(t.font_body), t.app_bar_text);
                scenario::publish_rect("bar:title", Rect::from_center_size(r.center(), vec2(200.0, r.height())));
            }
            // At the right: the settings.
            let gear = Rect::from_center_size(pos2(r.right() - 22.0, r.center().y), vec2(26.0, 26.0));
            let resp = ui.interact(gear, ui.id().with("bar:settings"), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(gear, t.radius_sm, Color32::from_white_alpha(28));
            }
            glyphs::paint(ui.painter(), gear.shrink(4.0), "settings", t.app_bar_text, t.app_bar_text);
            scenario::publish_target("bar:settings", gear);
            let resp = resp.on_hover_text("Settings");
            egui::Popup::menu(&resp).show(|ui| {
                ui.set_min_width(300.0);
                crate::home::settings(app, ui);
            });
            // The running job, at the right.
            if let Some(job) = &app.job {
                ui.painter().text(
                    pos2(r.right() - 12.0, r.center().y),
                    Align2::RIGHT_CENTER,
                    format!("{} …", crate::label(&job.command)),
                    FontId::proportional(t.font_body),
                    t.app_bar_text,
                );
            }
        },
    );
}

/// The ribbon card: tab row (with command search) over the active tab's panels.
pub fn ribbon(app: &mut EmcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let height = TAB_ROW_HEIGHT + BUTTON_HEIGHT + PANEL_LABEL_HEIGHT + 12.0;
    egui::Panel::top("emcraft_ribbon")
        .exact_size(height + 12.0)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(6))
        .show(ui, |ui| {
            let card = ui.max_rect();
            ui.painter().add(t.card(t.toolbar).paint(card));
            scenario::publish_rect("ribbon", card);
            let row = Rect::from_min_size(card.min, vec2(card.width(), TAB_ROW_HEIGHT));
            tabs(app, ui, row);
            search_field(app, ui, row);
            let mut x = card.left() + 6.0;
            let top = row.bottom() + 4.0;
            let tab = app.tab.clone();
            let Some((_, panels)) = TABS.iter().find(|(name, _)| *name == tab) else { return };
            let accent = t.tab(&tab);
            let busy = app.busy();
            for (panel, items) in panels.iter() {
                let x0 = x;
                for &id in items.iter() {
                    let (text, glyph) = (button_label(id), glyphs::for_command(id));
                    let g = ui.painter().layout(text.to_owned(), FontId::proportional(t.font_small), t.text, 80.0);
                    let w = (g.size().x + 14.0).max(BUTTON_MIN_WIDTH);
                    let br = Rect::from_min_size(pos2(x, top), vec2(w, BUTTON_HEIGHT));
                    let enabled = !busy;
                    let active = app.dialog.as_ref().is_some_and(|d| d.command.id == id);
                    let resp = ui.interact(br, ui.id().with(("ribbon", id)), if enabled { Sense::click() } else { Sense::hover() });
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text));
                    if active {
                        ui.painter().rect_filled(br, t.radius_sm, accent[2]);
                    } else if resp.hovered() && enabled {
                        ui.painter().rect_filled(br, t.radius_sm, t.hover);
                    }
                    let ink = if enabled { t.icon } else { t.icon.gamma_multiply(0.4) };
                    glyphs::paint(
                        ui.painter(),
                        Rect::from_center_size(pos2(br.center().x, br.top() + 18.0), vec2(26.0, 26.0)),
                        glyph,
                        ink,
                        accent[0],
                    );
                    let color = if enabled { if active { accent[1] } else { t.text } } else { t.text_dim };
                    let g = ui.painter().layout(text.to_owned(), FontId::proportional(t.font_small), color, w - 6.0);
                    ui.painter().galley(pos2(br.center().x - g.size().x / 2.0, br.top() + 35.0), g, color);
                    scenario::publish_target(&format!("toolbar:{id}"), br);
                    if resp.on_hover_text(emcraft_engine::find_command(id).map_or("", |c| c.doc)).clicked()
                        && let Err(e) = app.open_dialog(id)
                    {
                        app.message(e, true);
                    }
                    x += w + 3.0;
                }
                // The panel's name under its buttons, and a divider after it.
                let pr = Rect::from_min_max(pos2(x0, top + BUTTON_HEIGHT), pos2(x, top + BUTTON_HEIGHT + PANEL_LABEL_HEIGHT));
                ui.painter().text(pr.center(), Align2::CENTER_CENTER, panel, FontId::proportional(10.5), t.text_dim);
                scenario::publish_rect(&format!("panel:{tab}:{panel}"), pr);
                x += 6.0;
                ui.painter().vline(x, (top + 4.0)..=(top + BUTTON_HEIGHT + PANEL_LABEL_HEIGHT - 2.0), Stroke::new(1.0, t.border));
                x += 7.0;
            }
        });
}

/// Switches the viewport to `v`, solving the port modes when that view has none yet.
pub fn show_view(app: &mut EmcraftApp, v: View) {
    app.view = v;
    if v == View::PortModes && app.views.port_modes.is_none() && app.session.model.mesh.is_some() && !app.busy() {
        let _ = app.open_dialog("solve.port_modes");
    }
}

fn tabs(app: &mut EmcraftApp, ui: &mut egui::Ui, row: Rect) {
    let t = Tokens::get();
    let mut x = row.left() + 4.0;
    for (tab, _) in TABS {
        let g = ui.painter().layout_no_wrap((*tab).to_owned(), FontId::proportional(t.font_button), t.text);
        let tr = Rect::from_min_size(pos2(x, row.top() + 4.0), vec2(g.size().x + 24.0, TAB_ROW_HEIGHT - 8.0));
        let resp = ui.interact(tr, ui.id().with(("tab", *tab)), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, *tab));
        let active = app.tab == *tab;
        let accent = t.tab(tab);
        let fill = if active {
            accent[2]
        } else if resp.hovered() {
            t.hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(tr, 4, fill);
        let ink = if active { accent[1] } else { t.text };
        ui.painter().text(tr.center(), Align2::CENTER_CENTER, *tab, FontId::proportional(t.font_button), ink);
        scenario::publish_target(&format!("tab:{tab}"), tr);
        if resp.clicked() {
            app.tab = (*tab).to_owned();
        }
        x = tr.right() + 2.0;
    }
    ui.painter().hline(row.x_range(), row.bottom() - 0.5, Stroke::new(1.0, t.toolbar.lerp_to_gamma(t.border, 0.5)));
}

/// Command search: the query and the highlighted row.
#[derive(Clone, Debug, Default)]
pub struct Search {
    pub text: String,
    pub open: bool,
    pub selected: usize,
    /// The field's rectangle last frame (the popup hangs under it).
    pub field: Option<Rect>,
    /// Focus the field on the next frame.
    pub focus: bool,
}

/// Longest query (characters).
const MAX_QUERY: usize = 4096;
/// Most rows shown.
const MAX_RESULTS: usize = 12;

/// The commands matching a query, best first: id or label prefix, then substring.
pub fn matches(query: &str) -> Vec<&'static emcraft_engine::CommandSpec> {
    let q = query.trim().to_lowercase();
    let q = q.split_whitespace().next().unwrap_or("");
    let mut scored: Vec<(u8, &'static emcraft_engine::CommandSpec)> = emcraft_engine::COMMANDS
        .iter()
        .filter_map(|c| {
            let (id, label) = (c.id.to_lowercase(), c.label.to_lowercase());
            let score = if q.is_empty() {
                3
            } else if id == q {
                0
            } else if id.starts_with(q) || label.starts_with(q) {
                1
            } else if id.contains(q) || label.contains(q) || c.doc.to_lowercase().contains(q) {
                2
            } else {
                return None;
            };
            Some((score, c))
        })
        .collect();
    scored.sort_by_key(|(s, _)| *s);
    scored.into_iter().map(|(_, c)| c).take(MAX_RESULTS).collect()
}

/// A query of the form `command.id {json}`: the command and its parameters.
pub fn direct(query: &str) -> Option<Result<(String, Value), String>> {
    let q = query.trim();
    let (id, rest) = q.split_once(char::is_whitespace)?;
    let rest = rest.trim();
    if !rest.starts_with('{') {
        return None;
    }
    emcraft_engine::find_command(id)?;
    Some(serde_json::from_str::<Value>(rest).map(|v| (id.to_owned(), v)).map_err(|e| format!("not JSON: {e}")))
}

fn search_field(app: &mut EmcraftApp, ui: &mut egui::Ui, row: Rect) {
    let t = Tokens::get();
    let r = Rect::from_min_size(pos2(row.right() - 300.0, row.top() + 4.0), vec2(292.0, TAB_ROW_HEIGHT - 8.0));
    let mut field = ui.new_child(egui::UiBuilder::new().max_rect(r));
    let edit = egui::TextEdit::singleline(&mut app.search.text)
        .id(egui::Id::new("emcraft_search"))
        .hint_text("Search commands (/)")
        .char_limit(MAX_QUERY)
        .desired_width(r.width())
        .margin(egui::Margin::symmetric(8, 3));
    let resp = field.add(edit);
    scenario::publish_target("search:field", resp.rect);
    if app.search.focus {
        resp.request_focus();
        app.search.focus = false;
    }
    if resp.changed() || resp.gained_focus() {
        app.search.open = true;
        app.search.selected = 0;
    }
    if resp.has_focus() {
        app.search.open = true;
    }
    app.search.field = Some(resp.rect);
    // Keys while the field has focus.
    if resp.has_focus() || resp.lost_focus() {
        let (up, down, enter, esc) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        let n = matches(&app.search.text).len();
        if down && n > 0 {
            app.search.selected = (app.search.selected + 1).min(n - 1);
        }
        if up {
            app.search.selected = app.search.selected.saturating_sub(1);
        }
        if esc {
            app.search.open = false;
            app.search.text.clear();
        }
        if enter && app.search.open {
            // The Enter is the search's: a dialog it opens must not take it as OK.
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            choose(app, None);
        }
    }
    // `/` opens search from anywhere.
    if !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Slash)) {
        app.search.focus = true;
    }
    let _ = t;
}

/// Runs the search's choice: the `command {json}` typed, or the dialog of the highlighted (or
/// `pick`ed) command.
pub fn choose(app: &mut EmcraftApp, pick: Option<&str>) {
    let query = app.search.text.clone();
    app.search.open = false;
    app.search.text.clear();
    if pick.is_none() {
        match direct(&query) {
            Some(Ok((id, params))) => {
                let _ = app.run(&id, params);
                return;
            }
            Some(Err(e)) => {
                app.message(e, true);
                return;
            }
            None => {}
        }
    }
    let list = matches(&query);
    let id = pick.map(str::to_owned).or_else(|| list.get(app.search.selected.min(list.len().saturating_sub(1))).map(|c| c.id.to_owned()));
    if let Some(id) = id
        && let Err(e) = app.open_dialog(&id)
    {
        app.message(e, true);
    }
}

/// The search results under the field.
pub fn search_popup(app: &mut EmcraftApp, ctx: &egui::Context) {
    let (true, Some(field)) = (app.search.open, app.search.field) else { return };
    let t = Tokens::get();
    let list = matches(&app.search.text);
    let direct = direct(&app.search.text);
    let mut pick = None;
    let area = egui::Area::new(egui::Id::new("emcraft_search_popup"))
        .order(egui::Order::Foreground)
        .fade_in(false)
        .fixed_pos(pos2(field.right() - 420.0, field.bottom() + 4.0));
    let resp = area.show(ctx, |ui| {
        t.card(t.dialog_body).inner_margin(6).show(ui, |ui| {
            ui.set_width(420.0);
            if let Some(d) = &direct {
                match d {
                    Ok((id, _)) => ui.label(egui::RichText::new(format!("Enter runs {id} with these parameters")).color(t.success)),
                    Err(e) => ui.label(egui::RichText::new(e).color(t.form_error)),
                };
            }
            if list.is_empty() {
                ui.label(egui::RichText::new("No command matches.").color(t.text_dim));
            }
            for (i, c) in list.iter().enumerate() {
                let selected = i == app.search.selected;
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, c.label));
                if selected || resp.hovered() {
                    ui.painter().rect_filled(r, t.radius_sm, if selected { t.accent_soft } else { t.hover });
                }
                glyphs::paint(
                    ui.painter(),
                    Rect::from_center_size(pos2(r.left() + 18.0, r.center().y), vec2(22.0, 22.0)),
                    glyphs::for_command(c.id),
                    t.icon,
                    t.accent,
                );
                ui.painter().text(
                    pos2(r.left() + 38.0, r.top() + 11.0),
                    Align2::LEFT_CENTER,
                    c.label,
                    FontId::proportional(t.font_label),
                    t.form_ink,
                );
                ui.painter().text(pos2(r.left() + 38.0, r.top() + 27.0), Align2::LEFT_CENTER, c.id, FontId::monospace(t.font_small), t.text_dim);
                scenario::publish_target(&format!("search:row:{}", c.id), r);
                if resp.clicked() {
                    pick = Some(c.id);
                }
            }
        });
    });
    scenario::publish_rect("search:popup", resp.response.rect);
    if let Some(id) = pick {
        choose(app, Some(id));
    } else if resp.response.clicked_elsewhere() && !ctx.memory(|m| m.has_focus(egui::Id::new("emcraft_search"))) {
        app.search.open = false;
    }
}

/// The history's export: where to write it (the session's model-changing commands as a script
/// that rebuilds the model, `cargo run -p emcraft-cli -- run FILE`).
pub fn export_prompt(app: &mut EmcraftApp, ctx: &egui::Context) {
    let Some(mut path) = app.export.take() else { return };
    let t = Tokens::get();
    let (mut ok, mut cancel) = (false, false);
    let at = app.viewport_rect.map_or(pos2(200.0, 200.0), |r| r.center_top() + vec2(-200.0, 40.0));
    egui::Area::new(egui::Id::new("emcraft_export")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        t.card(t.dialog_body).inner_margin(12).show(ui, |ui| {
            ui.set_width(400.0);
            ui.label(egui::RichText::new("Export history as script").size(t.font_label).strong().color(t.form_ink));
            ui.label(
                egui::RichText::new(format!("{} commands that rebuild the model", app.session.history.len())).size(t.font_small).color(t.text_dim),
            );
            let r = ui.add(egui::TextEdit::singleline(&mut path).desired_width(380.0).char_limit(4096));
            scenario::publish_target("export:path", r.rect);
            ui.horizontal(|ui| {
                ok = crate::ui_kit::button(ui, "Export", true, true, "export:ok").clicked();
                cancel = crate::ui_kit::button(ui, "Cancel", false, true, "export:cancel").clicked();
            });
        });
    });
    if cancel {
        return;
    }
    if ok {
        let text = serde_json::to_string_pretty(&app.session.history).unwrap_or_else(|_| "[]".into());
        match std::fs::write(path.trim(), text) {
            Ok(()) => app.message(format!("Exported {} commands to {}.", app.session.history.len(), path.trim()), false),
            Err(e) => {
                app.message(format!("Cannot write {}: {e}", path.trim()), true);
                app.export = Some(path);
            }
        }
        return;
    }
    app.export = Some(path);
}

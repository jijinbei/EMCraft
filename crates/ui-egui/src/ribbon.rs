//! The application bar, the ribbon (the open step's panels of command buttons) and command
//! search, laid out after SolidCraft's (`crates/ui-egui/src/toolbar.rs`: a dark application bar,
//! the ribbon on a floating card, 60 pt buttons with the glyph over the label, panel names under
//! their buttons), written anew for EMCraft's commands. The step bar chooses the panels (each step
//! its tab); the search field sits at the ribbon's right.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde_json::Value;

use crate::commands::{button_label, glyph};
use crate::glyphs::{self, Glyph};
use crate::handle::{Fixed, Handle};
use crate::lang::{Raw, args, command_doc, tr, trf};
use crate::theme::Tokens;
use crate::tools3d::Tool;
use crate::{Action, Actions, EmcraftApp, Hint, Style, UiExt, scenario};

emcraft_macros::named_enum! {
    /// A ribbon tab: its id is its title (the control channel's `tab:<id>` handles, `ui.set
    /// {"tab": id}`).
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum RibbonTab {
        #[default]
        Model = "MODEL",
        Materials = "MATERIALS",
        PortsSources = "PORTS & SOURCES",
        Boundaries = "BOUNDARIES",
        Mesh = "MESH",
        Simulate = "SIMULATE",
        Results = "RESULTS",
    }
}

/// The ribbon: tabs of panels of commands, each command in one place (views are switched
/// from the view tabs over the viewport, files from the File menu and the start page).
pub const TABS: &[(RibbonTab, &[(&str, &[&str])])] = &[
    (
        RibbonTab::Model,
        &[
            ("GEOMETRY", &["geometry.import", "geometry.waveguide", "geometry.body", "model.region", "model.background"]),
            ("SETTINGS", &["units.set"]),
            ("PCB", &["pcb.import", "pcb.sample", "pcb.segment", "pcb.via", "pcb.pour", "pcb.element"]),
        ],
    ),
    (RibbonTab::Materials, &[("MATERIALS", &["model.material.create"])]),
    (
        RibbonTab::PortsSources,
        &[
            ("PORTS", &["model.port.create", "model.port.lumped"]),
            ("ELEMENTS", &["model.element.create"]),
            ("SOURCES", &["source.dipole", "source.plane_wave", "source.clear"]),
        ],
    ),
    // The tabs follow the step bar: the boundaries are step 4, after the ports (step 3).
    (
        RibbonTab::Boundaries,
        &[(
            "BOUNDARIES",
            &["model.walls", "model.open", "model.symmetry", "model.symmetry.find", "model.rotation", "model.rotation.find", "model.periodic"],
        )],
    ),
    (RibbonTab::Mesh, &[("MESH", &["mesh.generate", "mesh.adapt"])]),
    (
        RibbonTab::Simulate,
        &[
            // The solve itself is Run's (step 7), its device Setup's (step 5).
            ("PERIODIC", &["solve.dispersion"]),
            ("STUDY", &["project.variable", "study.sweep", "study.optimize"]),
        ],
    ),
    (
        RibbonTab::Results,
        &[
            // Comparison, checks, TDR and Touchstone are the result views' and Results' (step 8).
            ("EXPORT", &["view.sparameters", "view.field", "view.animation", "view.pattern", "view.dispersion"]),
        ],
    ),
];

/// The 3D view's tools on the ribbon, after a panel's commands: the ports made by pointing at
/// the model, beside the port commands.
pub const TOOLS: &[(RibbonTab, &str, &[Tool])] = &[(RibbonTab::PortsSources, "PORTS", &[Tool::Port, Tool::Lumped])];

const APP_BAR_HEIGHT: f32 = 34.0;
const TAB_ROW_HEIGHT: f32 = 30.0;
const BUTTON_HEIGHT: f32 = 60.0;
const PANEL_LABEL_HEIGHT: f32 = 16.0;
const BUTTON_MIN_WIDTH: f32 = 58.0;
/// The search field's least width with its hint; with less room it shows only its key.
const SEARCH_MIN_WIDTH: f32 = 120.0;
/// The search field's room at the ribbon's right.
const SEARCH_WIDTH: f32 = 300.0;
/// The width of the button that opens what has no room.
const MORE_WIDTH: f32 = 28.0;

/// The dark application bar: the name, the samples menu, the theme switch.
pub fn app_bar(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    egui::Panel::top("emcraft_app_bar").exact_size(APP_BAR_HEIGHT).show_separator_line(false).frame(egui::Frame::NONE.fill(t.app_bar)).show(
        ui,
        |ui| {
            let r = ui.max_rect();
            // The start page's house, then the wordmark.
            let house = Rect::from_center_size(pos2(r.left() + 20.0, r.center().y), vec2(26.0, 26.0));
            let home = ui.hit(house, Fixed::BarStart.into(), Sense::click());
            let p = ui.painter();
            let at_home = app.page == crate::home::Page::Home;
            if home.hovered() || at_home {
                p.rect_filled(house, t.radius_sm, if at_home { t.app_bar_active } else { t.app_bar_hover });
            }
            glyphs::paint(p, house.shrink(4.0), Glyph::Start, t.app_bar_text, t.accent);
            if home.hint("Start page").clicked() {
                out.push(Action::SetHome(!at_home));
            }
            let p = ui.painter();
            p.text(pos2(r.left() + 40.0, r.center().y), Align2::LEFT_CENTER, "EMCraft", FontId::proportional(15.0), t.app_bar_text);
            let mut x = r.left() + 120.0;
            let bar_button = |ui: &mut egui::Ui, x: &mut f32, text: &str, handle: Fixed| {
                let text = tr(text);
                let g = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(t.font_button), t.app_bar_text);
                let br = Rect::from_min_size(pos2(*x, r.top() + 4.0), vec2(g.size().x + 20.0, r.height() - 8.0));
                let resp = ui.hit(br, handle.into(), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
                if resp.hovered() {
                    ui.painter().rect_filled(br, t.radius_sm, t.app_bar_hover);
                }
                ui.painter().galley(pos2(br.left() + 10.0, br.center().y - g.size().y / 2.0), g, t.app_bar_text);
                *x = br.right() + 4.0;
                resp
            };
            let busy = app.busy();
            let file = bar_button(ui, &mut x, "File", Fixed::MenuFile);
            egui::Popup::menu(&file).show(|ui| crate::files::menu(app, ui, out));
            let edit = bar_button(ui, &mut x, "Edit", Fixed::MenuEdit);
            egui::Popup::menu(&edit).show(|ui| {
                ui.set_min_width(200.0);
                let (u, r) = (app.session.undo().len(), app.session.redo().len());
                let undo = egui::Button::new(trf("Undo  ({n})", args!(n = u))).shortcut_text("Ctrl+Z");
                ui.act(u > 0 && !busy, undo, Fixed::EditUndo.into(), || Action::Undo, out);
                let redo = egui::Button::new(trf("Redo  ({n})", args!(n = r))).shortcut_text("Ctrl+Y");
                ui.act(r > 0 && !busy, redo, Fixed::EditRedo.into(), || Action::Redo, out);
            });
            // The project's name in the middle, a dot while it has unsaved changes.
            let mut title_right = x;
            if !app.page.home() {
                let title = crate::files::title(app);
                let shape = ui.painter().text(r.center(), Align2::CENTER_CENTER, &title, FontId::proportional(t.font_body), t.app_bar_text);
                title_right = title_right.max(shape.right());
                scenario::publish_rect(Fixed::BarTitle, Rect::from_center_size(r.center(), vec2(200.0, r.height())));
            }
            // At the right: the settings.
            let gear = Rect::from_center_size(pos2(r.right() - 22.0, r.center().y), vec2(26.0, 26.0));
            let resp = ui.hit(gear, Fixed::BarSettings.into(), Sense::click());
            if resp.hovered() || app.page.settings().is_some() {
                ui.painter().rect_filled(gear, t.radius_sm, if app.page.settings().is_some() { t.app_bar_active } else { t.app_bar_hover });
            }
            glyphs::paint(ui.painter(), gear.shrink(4.0), Glyph::Settings, t.app_bar_text, t.app_bar_text);
            if resp.hint("Settings").clicked() {
                out.push(Action::Settings(if app.page.settings().is_some() { None } else { Some(crate::settings::Section::default()) }));
            }
            // The running job, left of the gear, cut short with an ellipsis where the title
            // leaves no room.
            if let Some(job) = &app.job {
                let (left, right) = (title_right + 24.0, gear.left() - 10.0);
                if right - left > 40.0 {
                    let mut lay =
                        egui::text::LayoutJob::simple_singleline(crate::messages::job_text(job), FontId::proportional(t.font_small), t.app_bar_text);
                    lay.wrap = egui::text::TextWrapping::truncate_at_width(right - left);
                    let g = ui.painter().layout_job(lay);
                    let at = pos2(right - g.size().x, r.center().y - g.size().y / 2.0);
                    scenario::publish_target(Fixed::BarJob, Rect::from_min_size(at, g.size()));
                    ui.painter().galley(at, g, t.app_bar_text);
                }
            }
        },
    );
}

/// The ribbon card: the open step's panels (the step bar chooses them; no tabs of its own), the
/// command search at its right.
pub fn ribbon(app: &EmcraftApp, search: &mut Search, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let height = BUTTON_HEIGHT + PANEL_LABEL_HEIGHT + 12.0;
    egui::Panel::top("emcraft_ribbon")
        .exact_size(height + 12.0)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.viewport_top).inner_margin(6))
        .show(ui, |ui| {
            let card = ui.max_rect();
            ui.painter().add(t.card(t.toolbar).paint(card));
            scenario::publish_rect(Fixed::Ribbon, card);
            let row = Rect::from_min_size(card.min, vec2(card.width(), TAB_ROW_HEIGHT));
            let search_left = (card.right() - SEARCH_WIDTH).max(card.left() + card.width() * 0.5);
            search_field(search, ui, row, search_left, out);
            panels(app, ui, card, search_left - 8.0, card.top() + 6.0, out);
        });
}

/// The open tab's panels along `top`: each one's buttons, its name under them and a divider
/// after it; the buttons past the card's right edge go into a menu at its end.
fn panels(app: &EmcraftApp, ui: &mut egui::Ui, card: Rect, right: f32, top: f32, out: &mut Actions) {
    let t = &Tokens::get();
    let tab = app.tab;
    // The PCB diagnosis has its own stages, not the steps: every tab's panels that fit it.
    let shown: Vec<RibbonTab> = if chosen_kind(app) == Some(crate::steps::Solver::Pcb) { RibbonTab::ALL.to_vec() } else { vec![tab] };
    let panels: Vec<(RibbonTab, &str, Vec<Item>)> = TABS
        .iter()
        .filter(|(name, _)| shown.contains(name))
        .flat_map(|&(tab, listed)| ordered(app, listed).into_iter().map(move |p| (tab, p.name, p.items(app, tab))))
        .collect();
    let enabled = !app.busy();
    let mut x = card.left() + 6.0;
    let all: f32 = panels.iter().map(|(_, _, items)| items.iter().map(|i| button_width(ui, i.text()) + 3.0).sum::<f32>() + 13.0).sum();
    let limit = if x + all > right - 4.0 { right - 6.0 - MORE_WIDTH } else { f32::INFINITY };
    let mut more: Vec<Item> = Vec::new();
    for (tab, panel, items) in &panels {
        let tab = *tab;
        let accent = t.tab(tab);
        let x0 = x;
        for &item in items {
            if !more.is_empty() || x + button_width(ui, item.text()) > limit {
                more.push(item);
                continue;
            }
            let active = item.active(app);
            let b = Button { text: item.text(), glyph: item.glyph(), handle: item.handle(), enabled, active };
            let (resp, w) = button(ui, b, pos2(x, top), accent);
            if item.hint(resp).clicked() {
                out.push(item.action(active));
            }
            x += w + 3.0;
        }
        if x == x0 {
            continue;
        }
        let pr = Rect::from_min_max(pos2(x0, top + BUTTON_HEIGHT), pos2(x, top + BUTTON_HEIGHT + PANEL_LABEL_HEIGHT));
        ui.painter().text(pr.center(), Align2::CENTER_CENTER, tr(panel), FontId::proportional(10.5), t.text_dim);
        scenario::publish_rect(Handle::Panel(tab, panel), pr);
        x += 6.0;
        ui.painter().vline(x, (top + 4.0)..=(top + BUTTON_HEIGHT + PANEL_LABEL_HEIGHT - 2.0), Stroke::new(1.0, t.border));
        x += 7.0;
    }
    if !more.is_empty() {
        let r = Rect::from_min_size(pos2(right - 4.0 - MORE_WIDTH, top), vec2(MORE_WIDTH, BUTTON_HEIGHT));
        let resp = more_button(ui, r, Fixed::ToolbarMore, "More commands");
        egui::Popup::menu(&resp).show(|ui| {
            for item in more {
                let active = item.active(app);
                let r = ui.named_enabled(enabled, egui::Button::selectable(active, item.text()), item.handle());
                if item.hint(r).clicked() {
                    out.push(item.action(active));
                    ui.close();
                }
            }
        });
    }
}

/// A panel of a tab as the ribbon shows it: its name and its commands, in the order they go.
#[derive(Clone, Debug, PartialEq)]
struct Panel {
    name: &'static str,
    commands: Vec<&'static str>,
}

impl Panel {
    /// What it holds: its commands, then the 3D view's tools it holds (unless the job's kind of
    /// analysis has no ports).
    fn items(&self, app: &EmcraftApp, tab: RibbonTab) -> Vec<Item> {
        let ports = chosen_kind(app).is_none_or(|k| fits(k, "model.port.create"));
        let tools = TOOLS.iter().filter(|(on, p, _)| ports && *on == tab && *p == self.name).flat_map(|(_, _, tools)| tools.iter().copied());
        self.commands.iter().map(|&id| Item::Command(id)).chain(tools.map(Item::Tool)).collect()
    }
}

/// The tab's panels and their commands; SIMULATE's with the solve of the kind of analysis chosen
/// at step 0 first (its panel first, the solve first in it).
fn chosen_kind(app: &EmcraftApp) -> Option<crate::steps::Solver> {
    app.session.model().job.chosen().or_else(|| app.session.model().pcb.as_ref().map(|_| crate::steps::Solver::Pcb))
}

fn ordered(app: &EmcraftApp, panels: &'static [(&'static str, &'static [&'static str])]) -> Vec<Panel> {
    let chosen = chosen_kind(app);
    // A job of a chosen kind sees only its commands (the search still finds the others).
    let out: Vec<Panel> = panels
        .iter()
        .map(|(name, items)| Panel {
            name,
            commands: items.iter().copied().filter(|id| chosen.is_none_or(|k| fits(k, id)) && ready(app, id)).collect(),
        })
        .filter(|p| !p.commands.is_empty())
        .collect();
    out
}

/// Whether what a ribbon command works on is there: a board's edits need a board.
fn ready(app: &EmcraftApp, id: &str) -> bool {
    match id {
        "pcb.segment" | "pcb.via" | "pcb.pour" | "pcb.element" => app.session.model().pcb.is_some(),
        _ => true,
    }
}

/// Whether a ribbon command belongs to a job of this kind of analysis: those of the other kinds
/// are left out of the ribbon once a kind is chosen (the search still finds them).
pub fn fits(kind: crate::steps::Solver, id: &str) -> bool {
    use crate::steps::Solver::{Eigenmodes, Frequency, Pcb, Radiation, TimeDomain};
    if kind == Pcb {
        return id.starts_with("pcb.") || id.starts_with("file.") || matches!(id, "project.variable" | "project.analysis" | "units.set");
    }
    if id.starts_with("pcb.") {
        return false;
    }
    match id {
        "solve.frequency" => kind == Frequency,
        "solve.eigenmodes" | "solve.dispersion" | "view.dispersion" => kind == Eigenmodes,
        "solve.radiation" => kind == Radiation,
        "solve.time_domain" => kind == TimeDomain,
        // Open problems: the layer, sources and the far field.
        "model.open" | "view.pattern" | "source.dipole" | "source.plane_wave" | "source.clear" => matches!(kind, Radiation | TimeDomain),
        // S-parameters between ports.
        "results.touchstone" | "view.sparameters" | "results.tdr" | "results.compare" | "results.check" => matches!(kind, Frequency | TimeDomain),
        // Nothing drives a resonator.
        "model.port.create" | "model.port.lumped" | "solve.port_modes" | "model.element.create" => kind != Eigenmodes,
        _ => true,
    }
}

/// What a ribbon panel holds: a command (its dialog) or a tool of the 3D view.
#[derive(Clone, Copy)]
enum Item {
    Command(&'static str),
    Tool(Tool),
}

impl Item {
    fn text(self) -> &'static str {
        match self {
            Item::Command(id) => button_label(id),
            Item::Tool(tool) => tr(tool.label()),
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Item::Command(id) => glyph(id),
            Item::Tool(tool) => tool.glyph(),
        }
    }

    fn handle(self) -> Handle<'static> {
        match self {
            Item::Command(id) => Handle::Toolbar(id),
            Item::Tool(tool) => Handle::RibbonTool(tool),
        }
    }

    /// Whether what it opens is open: the command's dialog, the tool.
    fn active(self, app: &EmcraftApp) -> bool {
        match self {
            Item::Command(id) => app.dialog.as_ref().is_some_and(|d| d.command.id == id),
            Item::Tool(tool) => app.view3d.tools.tool == Some(tool),
        }
    }

    fn hint(self, r: egui::Response) -> egui::Response {
        match self {
            Item::Command(id) => r.hint(Raw(command_doc(id))),
            Item::Tool(tool) => r.hint(tool.hint()),
        }
    }

    /// What a click does: the command's dialog, the tool on (or off, when it is on).
    fn action(self, active: bool) -> Action {
        match self {
            Item::Command(id) => Action::dialog(id),
            Item::Tool(tool) => Action::Tool((!active).then_some(tool)),
        }
    }
}

/// A ribbon button: its label, glyph and name, whether it can be clicked and whether what it
/// opens is open.
struct Button<'a> {
    text: &'a str,
    glyph: Glyph,
    handle: Handle<'a>,
    enabled: bool,
    active: bool,
}

/// Most rows of a ribbon button's label: two fit under its glyph.
const LABEL_ROWS: usize = 2;

/// The widest a ribbon button grows to keep its label in [`LABEL_ROWS`] rows.
const BUTTON_MAX_WIDTH: f32 = 160.0;

/// `text` laid out as a button's label within `width`: at most [`LABEL_ROWS`] rows, the rest cut
/// with an ellipsis.
fn label_galley(ui: &egui::Ui, text: &str, width: f32, color: Color32) -> std::sync::Arc<egui::Galley> {
    let t = &Tokens::get();
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), FontId::proportional(t.font_small), color, width);
    job.wrap.max_rows = LABEL_ROWS;
    job.halign = egui::Align::Center;
    ui.painter().layout_job(job)
}

/// The width of a ribbon button labelled `text`: its label wrapped at 80 pt, wider (up to
/// [`BUTTON_MAX_WIDTH`]) where that takes more than [`LABEL_ROWS`] rows.
fn button_width(ui: &egui::Ui, text: &str) -> f32 {
    let t = &Tokens::get();
    let rows = |w: f32| ui.painter().layout(text.to_owned(), FontId::proportional(t.font_small), t.text, w).rows.len();
    let mut wrap = 80.0;
    while rows(wrap) > LABEL_ROWS && wrap + 14.0 < BUTTON_MAX_WIDTH {
        wrap += 10.0;
    }
    let g = ui.painter().layout(text.to_owned(), FontId::proportional(t.font_small), t.text, wrap);
    (g.size().x + 14.0).clamp(BUTTON_MIN_WIDTH, BUTTON_MAX_WIDTH)
}

/// Draws a ribbon button at `at` (60 pt high, its glyph over its label) in the tab's `accent`;
/// its response and width.
fn button(ui: &mut egui::Ui, b: Button, at: egui::Pos2, accent: [Color32; 3]) -> (egui::Response, f32) {
    let t = &Tokens::get();
    let Button { text, glyph, handle, enabled, active } = b;
    let w = button_width(ui, text);
    let br = Rect::from_min_size(at, vec2(w, BUTTON_HEIGHT));
    let resp = ui.hit(br, handle, if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text));
    if active {
        ui.painter().rect_filled(br, t.radius_sm, accent[2]);
    } else if resp.hovered() && enabled {
        ui.painter().rect_filled(br, t.radius_sm, t.hover);
    }
    let ink = if enabled { t.icon } else { t.icon.gamma_multiply(0.4) };
    glyphs::paint(ui.painter(), Rect::from_center_size(pos2(br.center().x, br.top() + 18.0), vec2(26.0, 26.0)), glyph, ink, accent[0]);
    let color = if enabled { if active { accent[1] } else { t.text } } else { t.text_dim };
    let g = label_galley(ui, text, w - 6.0, color);
    ui.painter().galley(pos2(br.center().x, br.top() + 35.0), g, color);
    (resp, w)
}

/// The button that opens what has no room (`»`), drawn in `r`; its response.
fn more_button(ui: &mut egui::Ui, r: Rect, handle: Fixed, hint: &str) -> egui::Response {
    let t = &Tokens::get();
    let resp = ui.hit(r, handle.into(), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr(hint)));
    if resp.hovered() {
        ui.painter().rect_filled(r, t.radius_sm, t.hover);
    }
    ui.painter().text(r.center(), Align2::CENTER_CENTER, "»", FontId::proportional(t.font_button + 2.0), t.text);
    resp.hint(hint)
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

/// The commands matching a query, best first: id or label prefix, then substring (the label and
/// description in English and in the current language).
pub fn matches(query: &str) -> Vec<&'static emcraft_engine::CommandSpec> {
    let q = query.trim().to_lowercase();
    let q = q.split_whitespace().next().unwrap_or("");
    let mut scored: Vec<(u8, &'static emcraft_engine::CommandSpec)> = emcraft_engine::COMMANDS
        .iter()
        .filter_map(|c| {
            let (id, label, shown) = (c.id.to_lowercase(), c.label.to_lowercase(), crate::label(c.id).to_lowercase());
            let doc = |d: &str| d.to_lowercase().contains(q);
            let score = if q.is_empty() {
                3
            } else if id == q {
                0
            } else if id.starts_with(q) || label.starts_with(q) || shown.starts_with(q) {
                1
            } else if id.contains(q) || label.contains(q) || shown.contains(q) || doc(c.doc) || doc(command_doc(c.id)) {
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

/// The search field at the tab row's right, narrower where the tabs (ending at `tabs_end`)
/// leave less room.
fn search_field(search: &mut Search, ui: &mut egui::Ui, row: Rect, left: f32, out: &mut Actions) {
    // Too little room: a small field showing only its key, wide over the tabs while in use.
    let id = egui::Id::new("emcraft_search");
    let room = row.right() - 8.0 - left - 12.0;
    let in_use = search.focus || !search.text.is_empty() || ui.memory(|m| m.has_focus(id));
    let width = if room < SEARCH_MIN_WIDTH && in_use { 240.0f32.min(row.width() - 16.0) } else { room.clamp(32.0, 292.0) };
    let hint = if width < SEARCH_MIN_WIDTH { "/".to_owned() } else { tr("Search commands (/)").to_owned() };
    let r = Rect::from_min_size(pos2(row.right() - 8.0 - width, row.top() + 4.0), vec2(width, TAB_ROW_HEIGHT - 8.0));
    let mut field = ui.new_child(egui::UiBuilder::new().max_rect(r));
    let edit = egui::TextEdit::singleline(&mut search.text)
        .id(id)
        .hint_text(hint)
        .char_limit(MAX_QUERY)
        .desired_width(r.width())
        .margin(egui::Margin::symmetric(8, 3));
    let resp = field.add(edit);
    scenario::publish_target(Fixed::SearchField, resp.rect);
    if search.focus {
        resp.request_focus();
        search.focus = false;
    }
    if resp.changed() || resp.gained_focus() {
        search.open = true;
        search.selected = 0;
    }
    if resp.has_focus() {
        search.open = true;
    }
    search.field = Some(resp.rect);
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
        let n = matches(&search.text).len();
        if down && n > 0 {
            search.selected = (search.selected + 1).min(n - 1);
        }
        if up {
            search.selected = search.selected.saturating_sub(1);
        }
        if esc {
            search.open = false;
            search.text.clear();
        }
        if enter && search.open {
            // The Enter is the search's: a dialog it opens must not take it as OK.
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            out.extend(choose(search, None));
        }
    }
    // `/` opens search from anywhere.
    if !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Slash)) {
        search.focus = true;
    }
}

/// The search's choice, the search closed: the `command {json}` typed, or the dialog of the
/// highlighted (or `pick`ed) command.
pub fn choose(search: &mut Search, pick: Option<&str>) -> Option<Action> {
    let query = std::mem::take(&mut search.text);
    search.open = false;
    if pick.is_none() {
        match direct(&query) {
            Some(Ok((id, params))) => return Some(Action::Run { id, params }),
            Some(Err(e)) => return Some(Action::error(e)),
            None => {}
        }
    }
    let list = matches(&query);
    let id = pick.or_else(|| list.get(search.selected.min(list.len().saturating_sub(1))).map(|c| c.id))?;
    Some(Action::dialog(id))
}

/// The search results under the field.
pub fn search_popup(search: &mut Search, ctx: &egui::Context, out: &mut Actions) {
    let (true, Some(field)) = (search.open, search.field) else { return };
    let t = &Tokens::get();
    let list = matches(&search.text);
    let direct = direct(&search.text);
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
                    Ok((id, _)) => ui.label(egui::RichText::new(trf("Enter runs {id} with these parameters", args!(id = id))).color(t.success)),
                    Err(e) => ui.label(egui::RichText::new(e).color(t.form_error)),
                };
            }
            if list.is_empty() {
                ui.text(Style::Dim, "No command matches.");
            }
            for (i, c) in list.iter().enumerate() {
                let selected = i == search.selected;
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
                scenario::publish_target(Handle::SearchRow(c.id), r);
                let label = crate::label(c.id);
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
                if selected || resp.hovered() {
                    ui.painter().rect_filled(r, t.radius_sm, if selected { t.accent_soft } else { t.hover });
                }
                glyphs::paint(
                    ui.painter(),
                    Rect::from_center_size(pos2(r.left() + 18.0, r.center().y), vec2(22.0, 22.0)),
                    glyph(c.id),
                    t.icon,
                    t.accent,
                );
                ui.painter().text(pos2(r.left() + 38.0, r.top() + 11.0), Align2::LEFT_CENTER, label, FontId::proportional(t.font_label), t.form_ink);
                ui.painter().text(pos2(r.left() + 38.0, r.top() + 27.0), Align2::LEFT_CENTER, c.id, FontId::monospace(t.font_small), t.text_dim);
                if resp.clicked() {
                    pick = Some(c.id);
                }
            }
        });
    });
    scenario::publish_rect(Fixed::SearchPopup, resp.response.rect);
    if let Some(id) = pick {
        out.extend(choose(search, Some(id)));
    } else if resp.response.clicked_elsewhere() && !ctx.memory(|m| m.has_focus(egui::Id::new("emcraft_search"))) {
        search.open = false;
    }
}

/// The history's export: where to write it (the session's model-changing commands as a script
/// that rebuilds the model, `cargo run -p emcraft-cli -- run FILE`). `export` is the path
/// field's text while the prompt is open.
pub fn export_prompt(app: &EmcraftApp, ask: &mut Option<crate::files::Ask>, ctx: &egui::Context, out: &mut Actions) {
    let Some(crate::files::Ask::Export(path)) = ask.as_mut() else { return };
    let t = &Tokens::get();
    let (mut ok, mut cancel) = (false, false);
    let at = app.viewport_rect.map_or(pos2(200.0, 200.0), |r| r.center_top() + vec2(-200.0, 40.0));
    egui::Area::new(egui::Id::new("emcraft_export")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        t.card(t.dialog_body).inner_margin(12).show(ui, |ui| {
            ui.set_width(400.0);
            ui.text(Style::Prompt, "Export history as script");
            ui.text(Style::SmallDim, trf("{n} commands that rebuild the model", args!(n = app.session.history().len())));
            let r = ui.add(egui::TextEdit::singleline(path).desired_width(380.0).char_limit(4096));
            scenario::publish_target(Fixed::ExportPath, r.rect);
            ui.horizontal(|ui| {
                ok = crate::ui_kit::button(ui, "Export", true, Fixed::ExportOk).clicked();
                cancel = crate::ui_kit::button(ui, "Cancel", false, Fixed::ExportCancel).clicked();
            });
        });
    });
    if cancel || ok {
        let path = ask.take();
        if let (true, Some(crate::files::Ask::Export(path))) = (ok && !cancel, path) {
            out.push(Action::ExportHistory(path));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every ribbon button's label, in every language, fits its button in two rows: none runs
    /// over the panel's name below it.
    #[test]
    fn every_ribbon_label_fits_its_button_in_two_rows() {
        let h = crate::scenario::Harness::new();
        for lang in crate::lang::ids() {
            crate::lang::set(lang);
            let mut out = h.ctx.run_ui(egui::RawInput::default(), |ui| {
                for (_, panels) in TABS {
                    for (_, items) in *panels {
                        for id in *items {
                            let text = button_label(id);
                            let w = button_width(ui, text);
                            let t = &Tokens::get();
                            let full = ui.painter().layout(text.to_owned(), FontId::proportional(t.font_small), t.text, w - 6.0);
                            assert!(full.rows.len() <= LABEL_ROWS, "{lang}: `{text}` takes {} rows at {w}", full.rows.len());
                        }
                    }
                }
            });
            out.textures_delta.clear();
        }
        crate::lang::set("en");
    }

    #[test]
    fn tabs_parse_from_their_ids_in_the_ribbons_order() {
        for (i, t) in RibbonTab::ALL.into_iter().enumerate() {
            assert_eq!(RibbonTab::parse(t.id()), Some(t));
            assert_eq!(TABS.get(i).map(|(tab, _)| *tab), Some(t));
        }
        assert_eq!(TABS.len(), RibbonTab::ALL.len());
        assert_eq!(RibbonTab::parse("PORTS & SOURCES"), Some(RibbonTab::PortsSources));
        assert_eq!(RibbonTab::parse("Model"), None);
    }

    /// The ribbon holds no solve (Run's) and no device (Setup's); a chosen kind's commands only,
    /// the other kinds' left out (the search still finds them).
    #[test]
    fn the_ribbon_holds_no_solve_and_a_kinds_own_commands() {
        let mut h = crate::scenario::Harness::new();
        let all = |app: &EmcraftApp| -> Vec<&str> { TABS.iter().flat_map(|(_, p)| ordered(app, p)).flat_map(|p| p.commands).collect() };
        let listed = all(&h.app);
        // The lumped element and the region have buttons of their own; an inclusion is a body
        // (ADR 0005).
        assert!(
            listed.contains(&"model.element.create") && listed.contains(&"model.region") && !listed.contains(&"geometry.inclusion"),
            "{listed:?}"
        );
        assert!(
            listed.iter().all(|id| !matches!(
                *id,
                "solve.frequency" | "solve.eigenmodes" | "solve.radiation" | "solve.time_domain" | "solve.device" | "solve.port_modes"
            )),
            "{listed:?}"
        );
        h.app.session.execute("project.analysis", &serde_json::json!({"kind": "time_domain"})).unwrap();
        let td = all(&h.app);
        assert!(!td.contains(&"solve.dispersion") && td.contains(&"study.sweep") && td.contains(&"source.plane_wave"), "{td:?}");
        let model = TABS.iter().find(|(t, _)| *t == RibbonTab::Model).map_or(&[][..], |(_, p)| *p);
        let shown = ordered(&h.app, model);
        assert!(shown.iter().all(|p| p.name != "PCB"));
        assert_eq!(shown.len() + 1, model.len());
        let simulate = TABS.iter().find(|(t, _)| *t == RibbonTab::Simulate).map_or(&[][..], |(_, p)| *p);
        h.app.session.execute("project.analysis", &serde_json::json!({"kind":"pcb"})).unwrap();
        let pcb = ordered(&h.app, simulate);
        assert!(pcb.iter().flat_map(|p| &p.commands).all(|id| *id == "project.variable"));
        h.app.session.execute("pcb.sample", &serde_json::json!({})).unwrap();
        assert!(h.app.session.model().job.chosen().is_none());
        assert_eq!(chosen_kind(&h.app), Some(crate::steps::Solver::Pcb));
        assert_eq!(ordered(&h.app, simulate), pcb);
    }
}

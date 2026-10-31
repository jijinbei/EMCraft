//! The settings page: every preference in one place, by section — the look (the themes, each
//! a card drawn in its own colours, and the user's theme folder), the window's language, the
//! units new projects start with, how the 3D view is drawn, where dialogs open, and the files
//! the app keeps. Opened from
//! the app bar's gear or the start page; it takes the window until Done or Escape, and every
//! change applies at once and is kept with the preferences.

use egui::{Align2, FontId, Rect, Sense, Stroke, pos2, vec2};
use emcraft_engine::theme::Base;
use emcraft_model::units::{Frequency, Length};

use crate::action::Pref;
use crate::handle::{Fixed, Handle};
use crate::lang::{Raw, Shown, args, shown, tr, trf};
use crate::theme::{Theme, Tokens};
use crate::{Action, Actions, EmcraftApp, Hint, Style, UiExt, scenario};

emcraft_macros::named_enum! {
    /// A section of the settings page.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum Section {
        #[default]
        Appearance = "appearance" => "Appearance",
        Language = "language" => "Language",
        Units = "units" => "Units",
        View3d = "3d" => "3D view",
        Dialogs = "dialogs" => "Dialogs",
        Files = "files" => "Files",
    }
}

/// A theme card's size, in points.
const CARD: [f32; 2] = [168.0, 132.0];
/// The width of the section list.
const NAV: f32 = 180.0;
/// The widest the page's content grows.
const CONTENT: f32 = 760.0;

/// Draws the settings page showing `section`.
pub fn show(app: &EmcraftApp, section: Section, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin::symmetric(36, 24))).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.text(Style::Title, "Settings");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let done = crate::ui_kit::styled("Done", true);
                ui.act(true, done, Fixed::SettingsClose.into(), "Back (Escape)", || Action::Settings(None), out);
            });
        });
        ui.add_space(t.space_16);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(NAV);
                for s in Section::ALL {
                    let button = egui::Button::selectable(s == section, egui::RichText::new(shown(s)).size(t.font_button))
                        .min_size(vec2(NAV, t.control_height));
                    ui.act(true, button, Handle::SettingsSection(s), "", || Action::Settings(Some(s)), out);
                }
            });
            ui.add_space(32.0);
            ui.vertical(|ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(CONTENT);
                    ui.label(egui::RichText::new(shown(section)).size(t.font_heading + 4.0).strong().color(t.text));
                    ui.add_space(t.space_12);
                    match section {
                        Section::Appearance => appearance(app, ui, out),
                        Section::Language => language(app, ui, out),
                        Section::Units => units(app, ui, out),
                        Section::View3d => view3d(app, ui, out),
                        Section::Dialogs => dialogs(app, ui, out),
                        Section::Files => files(app, ui, out),
                    }
                });
            });
        });
    });
}

/// A setting's heading and what it does.
fn heading(ui: &mut egui::Ui, title: &str, about: impl Shown) {
    let t = &Tokens::get();
    ui.label(egui::RichText::new(tr(title)).size(t.font_button).strong().color(t.text));
    if !about.shown().is_empty() {
        ui.text(Style::SmallDim, about);
    }
    ui.add_space(t.space_6);
}

/// The space between two settings.
fn gap(ui: &mut egui::Ui) {
    ui.add_space(Tokens::get().space_16 + 4.0);
}

/// A setting's choices, side by side: each `(value, label, id)`, published as
/// `settings:<group>:<id>`; a click asks for `action(value)`. The labels are names (units,
/// languages), shown as they are.
fn choices<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    out: &mut Actions,
    group: &'static str,
    current: T,
    list: &[(T, &str, &str)],
    action: impl Fn(T) -> Action,
) {
    let t = &Tokens::get();
    ui.horizontal_wrapped(|ui| {
        for &(value, label, id) in list {
            let b = egui::Button::selectable(current == value, egui::RichText::new(label).size(t.font_button))
                .min_size(vec2(64.0, t.control_height))
                .frame_when_inactive(true);
            ui.act(true, b, Handle::Setting(group, id), "", || action(value), out);
        }
    });
}

/// A setting's choices one under another, each with what it means: `(value, label, id, about)`,
/// in the current language.
fn options<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    out: &mut Actions,
    group: &'static str,
    current: T,
    list: &[(T, &str, &str, &str)],
    action: impl Fn(T) -> Action,
) {
    let t = &Tokens::get();
    egui::Grid::new(group).num_columns(2).spacing(vec2(t.space_12, t.space_8)).show(ui, |ui| {
        for &(value, label, id, about) in list {
            let b = egui::Button::selectable(current == value, egui::RichText::new(tr(label)).size(t.font_button))
                .min_size(vec2(96.0, t.control_height))
                .frame_when_inactive(true);
            ui.act(true, b, Handle::Setting(group, id), "", || action(value), out);
            ui.text(Style::Dim, about);
            ui.end_row();
        }
    });
}

/// A theme's name as its card reads: the built-in ones capitalised (in the current language).
pub fn theme_label(th: &Theme) -> String {
    let name = th.name.as_str();
    match name.chars().next() {
        Some(first) if !th.fallback && Base::parse(name).is_some() => {
            tr(&first.to_uppercase().chain(name.chars().skip(1)).collect::<String>()).to_owned()
        }
        _ => name.to_owned(),
    }
}

fn appearance(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    heading(ui, "Theme", "The colours of the window, the 3D view and the pictures. It changes at once.");
    let (built_in, yours): (Vec<&Theme>, Vec<&Theme>) = app.themes.list.iter().map(|th| &**th).partition(|th| Base::parse(&th.name).is_some());
    theme_cards(app, ui, &built_in, out);
    gap(ui);
    heading(
        ui,
        "Your themes",
        "Theme files (JSON) in the folder below, each changing a built-in theme's colours (docs/themes.md). Read again after you edit them.",
    );
    if yours.is_empty() {
        ui.text(Style::Dim, "None yet.");
        ui.add_space(t.space_8);
    } else {
        theme_cards(app, ui, &yours, out);
    }
    ui.horizontal(|ui| {
        let reload = crate::ui_kit::styled("Read the themes again", false);
        ui.act(true, reload, Fixed::SettingsReloadThemes.into(), "", || Action::Pref(Pref::ReloadThemes), out);
        path_label(ui, app.themes.dir.as_deref());
    });
    for problem in &app.themes.problems {
        crate::ui_kit::small(ui, problem, t.error);
    }
}

fn language(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    heading(ui, "Language", "The language of the window. It changes at once.");
    let list: Vec<(&str, &str, &str)> = crate::lang::languages().iter().map(|l| (l.id, l.name.as_str(), l.id)).collect();
    choices(ui, out, "language", app.prefs.language(), &list, |id| Action::Pref(Pref::Language(id.to_owned())));
    gap(ui);
    heading(
        ui,
        "What stays in English",
        "The engine's messages and errors, the text drawn in pictures (charts, field plots), the command and parameter ids, and the command line.",
    );
}

/// A path as a label (its full text on hover), or that there is none.
fn path_label(ui: &mut egui::Ui, path: Option<&std::path::Path>) {
    let t = &Tokens::get();
    match path {
        Some(p) => {
            let text = p.display().to_string();
            ui.add(egui::Label::new(egui::RichText::new(&text).monospace().size(t.font_small).color(t.text_dim)).truncate()).on_hover_text(text);
        }
        None => {
            ui.text(Style::SmallDim, "no configuration folder");
        }
    }
}

/// The themes as cards in rows as wide as fit, the current one marked.
fn theme_cards(app: &EmcraftApp, ui: &mut egui::Ui, list: &[&Theme], out: &mut Actions) {
    let w = CARD[0] + 12.0;
    let per_row = (((ui.available_width() + 12.0) / w).floor() as usize).max(1);
    for row in list.chunks(per_row) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            for th in row {
                if theme_card(ui, th, th.name == app.theme).clicked() {
                    out.push(Action::Pref(Pref::Theme(th.name.clone())));
                }
            }
        });
        ui.add_space(12.0);
    }
}

/// A theme's card: a small window drawn in its colours (app bar, ribbon, browser, a chart on
/// its picture ground with its series), its name and whether it is dark.
fn theme_card(ui: &mut egui::Ui, th: &Theme, current: bool) -> egui::Response {
    let t = &Tokens::get();
    let (r, resp) = ui.allocate_exact_size(vec2(CARD[0], CARD[1]), Sense::click());
    let p = ui.painter();
    let ring = if current {
        Stroke::new(2.0, t.accent)
    } else {
        Stroke::new(if resp.hovered() { 2.0 } else { 1.0 }, if resp.hovered() { t.accent_soft } else { t.border })
    };
    p.rect(r, 10.0, t.panel_header, ring, egui::StrokeKind::Inside);
    // The window in miniature.
    let (o, s) = (&th.tokens, &th.palette);
    let win = Rect::from_min_size(r.min + vec2(8.0, 8.0), vec2(CARD[0] - 16.0, 84.0));
    p.rect_filled(win, 5.0, o.panel);
    let bar = Rect::from_min_size(win.min, vec2(win.width(), 10.0));
    p.rect_filled(bar, egui::CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, o.app_bar);
    p.rect_filled(Rect::from_min_size(bar.left_bottom(), vec2(win.width(), 12.0)), 0.0, o.toolbar);
    let tabs = [o.tab_model[0], o.tab_materials[0], o.tab_ports[0], o.tab_mesh[0], o.tab_simulate[0], o.tab_results[0]];
    for (i, c) in tabs.into_iter().enumerate() {
        p.rect_filled(Rect::from_min_size(bar.left_bottom() + vec2(5.0 + 13.0 * i as f32, 4.0), vec2(9.0, 4.0)), 1.0, c);
    }
    let body = Rect::from_min_max(bar.left_bottom() + vec2(0.0, 12.0), win.max);
    let browser = Rect::from_min_size(body.min + vec2(4.0, 4.0), vec2(38.0, body.height() - 8.0));
    p.rect(browser, 2.0, o.panel, Stroke::new(1.0, o.border), egui::StrokeKind::Inside);
    for (i, ink) in [o.text, o.text_dim, o.text_dim, o.text].into_iter().enumerate() {
        let y = browser.top() + 7.0 + 10.0 * i as f32;
        p.line_segment([pos2(browser.left() + 5.0, y), pos2(browser.right() - 5.0 - 6.0 * (i % 2) as f32, y)], Stroke::new(2.0, ink));
    }
    let chart = Rect::from_min_max(pos2(browser.right() + 4.0, body.top() + 4.0), body.max - vec2(4.0, 4.0));
    let c32 = crate::theme::color32;
    p.rect(chart, 2.0, c32(s.ground), Stroke::new(1.0, o.border), egui::StrokeKind::Inside);
    for k in 1..3 {
        let y = chart.top() + chart.height() * k as f32 / 3.0;
        p.line_segment([pos2(chart.left() + 3.0, y), pos2(chart.right() - 3.0, y)], Stroke::new(1.0, c32(s.grid)));
    }
    for (k, c) in s.series.iter().take(3).enumerate() {
        let points: Vec<egui::Pos2> = (0..=12)
            .map(|i| {
                let x = i as f32 / 12.0;
                let y = 0.5 + 0.32 * ((x * 5.0 + k as f32 * 1.7).sin()) * (1.0 - 0.25 * k as f32);
                pos2(chart.left() + 4.0 + x * (chart.width() - 8.0), chart.top() + y * chart.height())
            })
            .collect();
        p.add(egui::Shape::line(points, Stroke::new(1.5, c32(*c))));
    }
    let button = Rect::from_min_size(pos2(chart.right() - 26.0, chart.bottom() - 11.0), vec2(22.0, 8.0));
    p.rect_filled(button, 2.0, o.accent);
    // Its name, and a mark on the current one.
    let name_at = pos2(r.left() + 12.0, win.bottom() + 20.0);
    let clip = p.with_clip_rect(r.shrink(6.0));
    clip.text(name_at, Align2::LEFT_CENTER, theme_label(th), FontId::proportional(t.font_body), t.text);
    let kind = tr(if th.is_dark() { "dark" } else { "light" });
    clip.text(pos2(r.right() - 12.0, name_at.y), Align2::RIGHT_CENTER, kind, FontId::proportional(t.font_small), t.text_dim);
    if current {
        let dot = pos2(r.right() - 14.0, r.top() + 14.0);
        p.circle(dot, 7.0, t.accent, Stroke::new(1.5, t.panel_header));
        p.line_segment([dot + vec2(-3.0, 0.0), dot + vec2(-1.0, 2.5)], Stroke::new(1.5, t.on_accent));
        p.line_segment([dot + vec2(-1.0, 2.5), dot + vec2(3.5, -2.5)], Stroke::new(1.5, t.on_accent));
    }
    scenario::publish_target(Handle::Setting("theme", &th.name), r);
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, current, th.name.as_str()));
    resp.hint(Raw(&th.description))
}

/// The units new projects start with.
pub fn unit_choices(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    egui::Grid::new("units").num_columns(2).spacing(vec2(t.space_12, t.space_8)).show(ui, |ui| {
        ui.text(Style::Dim, "Length");
        choices(ui, out, "length", app.prefs.length, &[(Length::Millimetre, "mm", "mm"), (Length::Metre, "m", "m")], |u| {
            Action::Pref(Pref::Length(u))
        });
        ui.end_row();
        ui.text(Style::Dim, "Frequency");
        let frequencies = [(Frequency::Gigahertz, "GHz", "GHz"), (Frequency::Megahertz, "MHz", "MHz"), (Frequency::Hertz, "Hz", "Hz")];
        choices(ui, out, "frequency", app.prefs.frequency, &frequencies, |u| Action::Pref(Pref::Frequency(u)));
        ui.end_row();
    });
}

fn units(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    heading(
        ui,
        "New projects",
        "The units new projects start with: what dialogs, tables and pictures show and commands read. An open project changes its own with MODEL › Units.",
    );
    unit_choices(app, ui, out);
}

fn view3d(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    use crate::prefs::Drawing3d;
    heading(ui, "Drawing", "How the 3D view of the model, its mesh and fields is drawn.");
    let drawing = [
        (Drawing3d::Gpu, "GPU", "gpu", "Drawn by the graphics card with a depth buffer, still and moving. Without one, as Quality."),
        (
            Drawing3d::Cpu,
            "Quality",
            "quality",
            "Drawn on the CPU with a depth buffer and smoothed edges when the view rests; the GPU while you drag.",
        ),
        (Drawing3d::Fast, "Fast", "fast", "Drawn by the GPU every frame, back to front: for large meshes, where the CPU picture takes long."),
    ];
    options(ui, out, "3d", app.prefs.drawing, &drawing, |d| Action::Pref(Pref::Drawing(d)));
    gap(ui);
    // The mouse mappings, SolidCraft's: the same hands work in both apps.
    let nav = app.prefs.nav();
    heading(ui, "Mouse", nav.help);
    let mappings: Vec<(&str, &str, &str)> = crate::navigation::NAV_PRESETS
        .iter()
        .map(|p| (p.id, if p.id == crate::navigation::TOUCHPAD { tr(p.label) } else { p.label }, p.id))
        .collect();
    choices(ui, out, "navigation", nav.id, &mappings, |id| Action::Pref(Pref::Navigation(id.to_owned())));
    ui.add_space(Tokens::get().space_6);
    ui.text(
        Style::SmallDim,
        "Every mapping: Ctrl+left drag orbits, Shift snaps an orbit, the wheel zooms to the cursor, arrows turn, Ctrl+1–7 standard views.",
    );
    gap(ui);
    heading(ui, "Orbit around", "");
    use crate::navigation::{Navigator, OrbitAround};
    let around: Vec<(OrbitAround, &str, &str)> = OrbitAround::ALL.iter().map(|o| (*o, shown(*o), o.id())).collect();
    choices(ui, out, "orbit_around", app.prefs.orbit_around(), &around, |o| Action::Pref(Pref::OrbitAround(o)));
    gap(ui);
    heading(ui, "Navigator", "");
    let navigators: Vec<(Navigator, &str, &str)> = Navigator::ALL.iter().map(|n| (*n, shown(*n), n.id())).collect();
    choices(ui, out, "navigator", app.prefs.navigator(), &navigators, |n| Action::Pref(Pref::Navigator(n)));
    ui.add_space(Tokens::get().space_12);
    heading(ui, "On the GPU", "What the GPU's view adds to the picture; each off draws as the CPU does.");
    let l = app.prefs.gpu_look;
    let switches: [(&str, &str, bool, fn(&mut crate::prefs::GpuLook) -> &mut bool, &str); 5] = [
        ("Edges", "edges", l.edges, |l| &mut l.edges, "Lines where surfaces crease or end, found per pixel."),
        ("Shadows", "ao", l.ao, |l| &mut l.ao, "Ambient occlusion: corners and creases darkened."),
        ("Outline", "outline", l.outline, |l| &mut l.outline, "A ring round what is selected and what is under the pointer."),
        ("GPU picking", "pick", l.pick, |l| &mut l.pick, "Clicks read what the GPU drew under the pointer."),
        ("GPU field", "field", l.field, |l| &mut l.field, "The field's colour map drawn by the GPU: its phase plays without rebuilding the scene."),
    ];
    let t = &Tokens::get();
    egui::Grid::new("3d_gpu").num_columns(2).spacing(vec2(t.space_12, t.space_8)).show(ui, |ui| {
        for (label, id, on, field, about) in switches {
            let b = egui::Button::selectable(on, egui::RichText::new(tr(label)).size(t.font_button))
                .min_size(vec2(96.0, t.control_height))
                .frame_when_inactive(true);
            let mut next = l;
            *field(&mut next) = !on;
            ui.act(true, b, Handle::Setting("3d", id), "", || Action::Pref(Pref::GpuLook(next)), out);
            ui.text(Style::Dim, about);
            ui.end_row();
        }
    });
}

fn dialogs(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    heading(ui, "Where dialogs open", "A command's dialog, opened from the ribbon, the browser or the search.");
    let place = [
        (false, "Floating", "floating", "A card over the view, which you can move."),
        (true, "Docked", "docked", "In the panel at the right, beside the view."),
    ];
    options(ui, out, "dialogs", app.dock_dialogs, &place, Action::SetDock);
}

fn files(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let n = app.prefs.recent.len();
    let at_most = trf("The start page lists the projects you opened or saved last, at most {n}.", args!(n = crate::prefs::MAX_RECENT));
    heading(ui, "Recent projects", at_most);
    ui.horizontal(|ui| {
        let clear = crate::ui_kit::styled("Clear the list", false);
        ui.act(n > 0, clear, Fixed::SettingsClearRecent.into(), "", || Action::Pref(Pref::ClearRecent), out);
        ui.text(Style::Dim, if n == 1 { tr("1 project").to_owned() } else { trf("{n} projects", args!(n = n)) });
    });

    gap(ui);
    heading(ui, "Preferences file", "Where these settings are kept between launches.");
    path_label(ui, app.prefs_store.path.as_deref());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_themes_read_capitalised() {
        let themes = crate::theme::Themes::load(None);
        let labels: Vec<String> = themes.list.iter().map(|th| theme_label(th)).collect();
        assert_eq!(labels, ["Light", "Dark", "Mono", "Graphite", "Sepia", "Midnight", "Forest"]);
        assert_eq!(Section::parse("3d"), Some(Section::View3d));
    }
}

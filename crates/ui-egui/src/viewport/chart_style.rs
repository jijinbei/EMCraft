//! A chart's style, edited in place: a gear on the chart opens a small card of its axes
//! (label, ends — empty for automatic —, log scale), title, grid, legend and curves (shown,
//! colour, width). Every change is a `view.chart.style` command, so it is saved with the
//! project and the PNG exports follow it.

use emcraft_engine::views::{AxisId, ChartId, LegendAt};
use serde_json::{Map, json};

use super::*;
use crate::glyphs::{Glyph, paint};
use crate::state::{AxisText, StyleEdit};

/// The gear's side (points).
const GEAR: f32 = 22.0;
/// The card's width.
const CARD: f32 = 340.0;
/// The widths a curve may take (pixels); the usual one is the theme's.
const WIDTHS: [u8; 4] = [1, 2, 3, 5];

/// The legend's places as the card offers them: an arrow toward the corner each names.
const LEGENDS: [(LegendAt, &str, &str); 6] = [
    (LegendAt::Below, "↓", "legend below the chart"),
    (LegendAt::TopLeft, "↖", "legend top left"),
    (LegendAt::TopRight, "↗", "legend top right"),
    (LegendAt::BottomLeft, "↙", "legend bottom left"),
    (LegendAt::BottomRight, "↘", "legend bottom right"),
    (LegendAt::None, "–", "no legend"),
];

/// A curve's label as plain text: TeX's marks taken out (`$|S_{21}|$` → `|S21|`).
pub(super) fn plain(label: &str) -> String {
    label.chars().filter(|c| !matches!(c, '$' | '{' | '}' | '_' | '^')).collect()
}

/// The gear on `chart` (the picture's response) that opens and closes chart `id`'s card, and the
/// card when open.
pub(super) fn gear(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, chart: &egui::Response, id: ChartId, out: &mut Actions) {
    let t = &Tokens::get();
    let r = Rect::from_min_size(pos2(chart.rect.right() - GEAR - 6.0, chart.rect.top() + 6.0), vec2(GEAR, GEAR));
    let resp = ui.hit(r, Fixed::ChartStyle.into(), Sense::click());
    // Another chart's card closes with its view.
    if st.views.style.open.is_some_and(|o| o != id) {
        st.views.style = StyleEdit::default();
    }
    let open = st.views.style.open == Some(id);
    let fill = if open {
        t.accent_soft
    } else if resp.hovered() {
        t.hover
    } else {
        t.panel.gamma_multiply(0.85)
    };
    ui.painter().rect_filled(r, t.radius_sm, fill);
    paint(ui.painter(), r.shrink(3.0), Glyph::Settings, t.icon, t.accent);
    let resp = resp.hint("Axes and looks");
    if resp.clicked() {
        st.views.style = StyleEdit { open: (!open).then_some(id), ..StyleEdit::default() };
    }
    if st.views.style.open == Some(id) {
        card(app, st, ui, r, id, out);
    }
}

/// The text fields filled from the style as `view.chart.style` shows it, unless they were
/// filled from it already.
fn fill(edit: &mut StyleEdit, shown: &Value) {
    if edit.seen.as_ref() == Some(shown) {
        return;
    }
    let text = |v: &Value| match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map(emcraft_render::axes::format).unwrap_or_default(),
        _ => String::new(),
    };
    edit.title = text(&shown["title"]);
    for (fields, a) in edit.axes.iter_mut().zip(AxisId::ALL) {
        let axis = &shown[a.id()];
        *fields = AxisText { label: text(&axis["label"]), min: text(&axis["min"]), max: text(&axis["max"]) };
    }
    edit.seen = Some(shown.clone());
}

/// The style's list `key` (curves hidden, curves added) as `view.chart.style` shows it.
fn names<'a>(shown: &'a Value, key: &str) -> Vec<&'a str> {
    shown[key].as_array().into_iter().flatten().filter_map(Value::as_str).collect()
}

/// `list` with `key` in it (`on`) or out of it.
fn with<'a>(list: &[&'a str], key: &'a str, on: bool) -> Vec<&'a str> {
    let mut list: Vec<&str> = list.iter().copied().filter(|k| *k != key).collect();
    if on {
        list.push(key);
    }
    list
}

/// `view.chart.style` for chart `id` with `params` added.
fn style(id: ChartId, params: Value) -> Action {
    let mut p = Map::new();
    p.insert("chart".into(), json!(id.id()));
    if let Value::Object(o) = params {
        p.extend(o);
    }
    Action::run("view.chart.style", Value::Object(p))
}

/// A field's text as an axis end: a number, or null (automatic) when empty; `None` when it reads
/// as neither.
fn end(text: &str) -> Option<Value> {
    let t = text.trim();
    if t.is_empty() {
        return Some(Value::Null);
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| json!(v))
}

/// The card under the gear `at`.
fn card(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, at: Rect, id: ChartId, out: &mut Actions) {
    let t = &Tokens::get();
    let shown = emcraft_engine::views::shown_style(&app.session, id);
    fill(&mut st.views.style, &shown);
    let curves = st.views.curves.clone();
    let edit = &mut st.views.style;
    let pos = pos2((at.right() - CARD).max(ui.clip_rect().left()), at.bottom() + 4.0);
    let area = egui::Area::new(egui::Id::new("chart_style_card")).order(egui::Order::Foreground).fixed_pos(pos);
    let r = area.show(ui.ctx(), |ui| {
        t.card(t.dialog_body).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.set_width(CARD - 16.0);
            header(ui, edit, id, out);
            // The title.
            let r = ui.named(
                egui::TextEdit::singleline(&mut edit.title).hint_text(tr("title (automatic)")).desired_width(f32::INFINITY),
                Fixed::ChartStyleTitle.into(),
            );
            if r.lost_focus() && Some(edit.title.trim()) != shown["title"].as_str().or(Some("")) {
                out.push(style(id, json!({"title": edit.title.trim()})));
            }
            egui::Grid::new("chart_style_axes").num_columns(5).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
                for (fields, a) in edit.axes.iter_mut().zip(AxisId::ALL) {
                    axis_row(ui, &shown, fields, a.id(), id, out);
                }
            });
            grid_and_legend(ui, &shown, id, out);
            ui.separator();
            added_rows(ui, &shown, &emcraft_engine::views::optional_curves(&app.session, id), id, out);
            curve_rows(ui, &shown, &curves, id, out);
        });
    });
    scenario::publish_rect(Fixed::ChartStyleCard, r.response.rect);
}

/// The card's name, Reset and its close button.
fn header(ui: &mut egui::Ui, edit: &mut StyleEdit, id: ChartId, out: &mut Actions) {
    ui.horizontal(|ui| {
        ui.text(Style::Strong, "Axes and looks");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
            if crate::ui_kit::icon_button(ui, r, crate::ui_kit::Mark::Close, "Close", Fixed::ChartStyleClose).clicked() {
                edit.open = None;
            }
            ui.act(
                true,
                egui::Button::new(tr("Reset")).small(),
                Fixed::ChartStyleReset.into(),
                "Everything automatic again",
                || style(id, json!({"reset": true})),
                out,
            );
        });
    });
}

/// An axis' row of the card's grid: its label, its ends, log. A text field is committed when it
/// loses focus.
fn axis_row(ui: &mut egui::Ui, shown: &Value, fields: &mut AxisText, name: &'static str, id: ChartId, out: &mut Actions) {
    let t = &Tokens::get();
    let shown = &shown[name];
    ui.text(Style::Strong, Raw(name));
    let r = ui.named(egui::TextEdit::singleline(&mut fields.label).hint_text(tr("label")).desired_width(84.0), Handle::StyleAxis(name, "label"));
    if r.lost_focus() && Some(fields.label.trim()) != shown["label"].as_str().or(Some("")) {
        out.push(style(id, json!({format!("{name}_label"): fields.label.trim()})));
    }
    for (part, text, hint) in [("min", &mut fields.min, "low end (empty: automatic)"), ("max", &mut fields.max, "high end (empty: automatic)")] {
        let bad = end(text).is_none();
        let mut edit_box = egui::TextEdit::singleline(text).hint_text(tr("auto")).desired_width(48.0);
        if bad {
            edit_box = edit_box.text_color(t.form_error);
        }
        let r = ui.named(edit_box, Handle::StyleAxis(name, part)).hint(hint);
        if r.lost_focus()
            && let Some(v) = end(text)
            && v != shown[part]
        {
            out.push(style(id, json!({format!("{name}_{part}"): v})));
        }
    }
    let log = shown["log"].as_bool().unwrap_or(false);
    let r = ui.named(egui::Button::selectable(log, "log"), Handle::StyleAxis(name, "log")).hint("Logarithmic scale");
    if r.clicked() {
        out.push(style(id, json!({format!("{name}_scale"): if log { "linear" } else { "log" }})));
    }
    ui.end_row();
}

/// The grid's switch and the legend's places.
fn grid_and_legend(ui: &mut egui::Ui, shown: &Value, id: ChartId, out: &mut Actions) {
    ui.horizontal(|ui| {
        let grid = shown["grid"].as_bool().unwrap_or(true);
        let r = ui.named(egui::Button::selectable(grid, tr("grid")), Fixed::ChartStyleGrid.into());
        if r.clicked() {
            out.push(style(id, json!({"grid": !grid})));
        }
        ui.separator();
        let legend = shown["legend"].as_str().and_then(LegendAt::parse).unwrap_or_default();
        for (at, mark, hint) in LEGENDS {
            let r = ui.named(egui::Button::selectable(legend == at, mark).small(), Handle::Legend(at.id())).hint(hint);
            if r.clicked() && legend != at {
                out.push(style(id, json!({"legend": at.id()})));
            }
        }
    });
}

/// The curves the chart can add to its data (a closed form to compare with), each a checkbox;
/// nothing when it has none.
fn added_rows(ui: &mut egui::Ui, shown: &Value, optional: &[(String, String)], id: ChartId, out: &mut Actions) {
    if optional.is_empty() {
        return;
    }
    let t = &Tokens::get();
    let added = names(shown, "added");
    crate::ui_kit::small(ui, "Add", t.text_dim);
    for (key, label) in optional {
        let on = added.contains(&key.as_str());
        ui.horizontal(|ui| {
            let r = ui.named(egui::Checkbox::without_text(&mut on.clone()), Handle::CurveAdd(key)).hint("Draw it over the data");
            if r.clicked() {
                out.push(style(id, json!({"added": with(&added, key, !on)})));
            }
            ui.label(egui::RichText::new(label).size(t.font_small));
        });
    }
    ui.add_space(4.0);
}

/// A row per curve: shown or not, its colour (the theme's, or one of the palette's series
/// colours), its width.
fn curve_rows(ui: &mut egui::Ui, shown: &Value, curves: &[(String, String)], id: ChartId, out: &mut Actions) {
    let t = &Tokens::get();
    let hidden = names(shown, "hidden");
    let palette = crate::theme::palette();
    let swatches: Vec<emcraft_render::Rgb> =
        (0..emcraft_render::palette::SERIES as u8).map(|i| palette.pen(emcraft_render::Pen::Series(i))).chain([palette.ink]).collect();
    egui::ScrollArea::vertical().max_height(220.0).id_salt("chart_style_curves").show(ui, |ui| {
        for (key, label) in curves {
            let on = !hidden.contains(&key.as_str());
            ui.horizontal(|ui| {
                let r = ui.named(egui::Checkbox::without_text(&mut on.clone()), Handle::Curve(key)).hint("Show this curve");
                if r.clicked() {
                    out.push(style(id, json!({"hidden": with(&hidden, key, on)})));
                }
                ui.add(egui::Label::new(egui::RichText::new(label).size(t.font_small)).truncate()).on_hover_text(key.as_str());
            });
            ui.horizontal(|ui| {
                ui.add_space(22.0);
                let own = shown["curves"][key.as_str()]["colour"].as_str();
                // The theme's colour first (a ring), then the swatches.
                for (i, c) in std::iter::once(None).chain(swatches.iter().map(Some)).enumerate() {
                    let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                    let resp = ui.hit(r, Handle::CurveColour(key, i), Sense::click());
                    let chosen = match c {
                        None => own.is_none(),
                        Some(c) => own == Some(c.hex().as_str()),
                    };
                    match c {
                        None => {
                            ui.painter().circle_stroke(r.center(), 5.0, Stroke::new(1.5, t.text_dim));
                        }
                        Some(c) => {
                            ui.painter().circle_filled(r.center(), 5.5, crate::theme::color32(*c));
                        }
                    }
                    if chosen {
                        ui.painter().circle_stroke(r.center(), 7.0, Stroke::new(1.5, t.accent));
                    }
                    let resp = resp.hint(if c.is_none() { "the theme's colour" } else { "" });
                    if resp.clicked() && !chosen {
                        out.push(style(id, json!({"curve": key, "colour": c.map(|c| c.hex())})));
                    }
                }
                ui.separator();
                let width = shown["curves"][key.as_str()]["width"].as_f64();
                for w in WIDTHS {
                    let on = width == Some(f64::from(w));
                    let r = ui
                        .named(egui::Button::selectable(on, format!("{w}")).small(), Handle::CurveWidth(key, w))
                        .hint("line width (pixels); again: the usual");
                    if r.clicked() {
                        out.push(style(id, json!({"curve": key, "width": if on { Value::Null } else { json!(w) }})));
                    }
                }
            });
        }
    });
}

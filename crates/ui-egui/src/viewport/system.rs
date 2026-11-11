//! The system view, a node editor: the band over a canvas of nodes — the source, the stages and
//! the receiver — each with its settings edited in place and, once read, the chain's gain and
//! noise figure at its output; links drawn from an output port to an input port (dragged from
//! one to the other to link, the mark on a link to unlink); nodes dragged by their headers, a
//! right-click on the canvas adding a stage there. Under the canvas the chart and the budget,
//! read again at every change (`solve.system` takes a moment).

use super::*;
use crate::system_ui;
use emcraft_engine::system::{Row, SystemResult};
use emcraft_engine::view::SystemChart;
use emcraft_model::system::{DataSource, Part, RECEIVER, SOURCE, Stage};

/// A node's width, its header's height and a field's row (points).
const NODE_W: f32 = 184.0;
const HEAD_H: f32 = 24.0;
const ROW_H: f32 = 22.0;
/// The gap between nodes laid out along the chain, and the canvas' least and greatest height.
const STEP_X: f32 = 230.0;
const CANVAS_MIN: f32 = 220.0;
const CANVAS_MAX: f32 = 520.0;
/// A port's radius, and how near the pointer must come to drop a link on one.
const PORT_R: f32 = 6.0;
const PORT_REACH: f32 = 16.0;

pub(super) struct SystemView;

impl ChartView for SystemView {
    type Key = SharedChart;
    const HANDLE: Fixed = Fixed::ChartSystem;
    const CHART: emcraft_engine::views::ChartId = emcraft_engine::views::ChartId::System;
    const HEIGHT: Height = Height::new(0.35, 200.0, 420.0);

    fn controls(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) -> Option<Self::Key> {
        let m = app.session.model();
        // Read again at every change, once a revision.
        let r = app.session.results();
        let stale = r.system.is_none() || r.is_stale(emcraft_engine::ResultKind::System);
        if stale && !m.system.stages.is_empty() && !app.busy() && app.auto_solved != Some(app.revision) {
            out.push(Action::AutoSolve);
        }
        band(app, ui, out);
        if let Some(e) = app.auto_error.as_ref().filter(|_| stale) {
            ui.text(Style::SmallDim, Raw(e));
        }
        ui.add_space(4.0);
        if m.system.stages.is_empty() {
            start(app, ui, out);
            return None;
        }
        canvas(app, ui, out);
        ui.add_space(6.0);
        let r = r.system.as_deref()?;
        ui.horizontal(|ui| {
            for c in SystemChart::ALL {
                let on = st.views.system == c;
                if ui.named(egui::Button::selectable(on, tr(chart_label(c))), Handle::SystemChart(c.id())).clicked() {
                    st.views.system = c;
                }
            }
            if let Some(p) = r.nearest(r.at) {
                let unit = app.session.display().frequency;
                ui.text(Style::Dim, Raw(&trf(t!("budget at {f} {unit}"), crate::args!(f = unit.from_si(p.frequency), unit = unit.symbol()))));
            }
        });
        Some(SharedChart::System(st.views.system))
    }

    fn chart(app: &EmcraftApp, key: &Self::Key) -> Result<emcraft_render::chart::Chart, String> {
        let SharedChart::System(what) = key else { return Err("not a system chart".into()) };
        emcraft_engine::view::system_chart(&app.session, *what).map_err(|e| e.to_string())
    }

    fn below(app: &EmcraftApp, _st: &mut Parts, ui: &mut egui::Ui, _chart: &egui::Response, _size: egui::Vec2, _out: &mut Actions) {
        if let Some(r) = app.session.results().system.as_deref() {
            budget(app, r, ui);
        }
    }
}

/// A chart choice's name.
fn chart_label(c: SystemChart) -> crate::lang::Text {
    match c {
        SystemChart::Along => t!("Along the chain"),
        SystemChart::Figure => t!("Noise figure"),
        SystemChart::Gain => t!("Gain"),
    }
}

/// A number edited in place: where, its name on screen, the model's value and whether it may be
/// left empty.
struct Number<'a> {
    rect: Rect,
    handle: Handle<'a>,
    value: Option<f64>,
    optional: bool,
}

/// A number edited in place at `rect`: shown as the model has it, typed into while it has the
/// keyboard, committed when it loses it (Enter, a click elsewhere) if changed — `commit` with
/// the number, or `None` for an empty field when `optional`. Its text while typed lives in the
/// context's memory.
fn number_field(ui: &mut egui::Ui, field: Number, commit: impl FnOnce(Option<f64>) -> Option<Action>, out: &mut Actions) {
    let Number { rect, handle, value, optional } = field;
    let t = &Tokens::get();
    let id = egui::Id::new(("system_field", handle.to_string()));
    let shown = value.map_or_else(String::new, |v| format!("{}", (v * 1e6).round() / 1e6));
    let mut text = ui.ctx().data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| shown.clone());
    let edit =
        egui::TextEdit::singleline(&mut text).font(FontId::proportional(t.font_small)).horizontal_align(egui::Align::RIGHT).margin(vec2(4.0, 1.0));
    let r = ui.put(rect, edit);
    scenario::publish_target(handle, r.rect);
    if r.has_focus() {
        ui.ctx().data_mut(|d| d.insert_temp(id, text.clone()));
    }
    if r.lost_focus() {
        ui.ctx().data_mut(|d| d.remove::<String>(id));
        let trimmed = text.trim();
        let parsed = if trimmed.is_empty() && optional { Some(None) } else { trimmed.parse::<f64>().ok().filter(|v| v.is_finite()).map(Some) };
        if trimmed != shown
            && let Some(v) = parsed
            && let Some(a) = commit(v)
        {
            out.push(a);
        }
    }
}

/// The band over the canvas: start, stop, points and the budget's frequency, confirmed
/// (`project.setup`) as each is changed; the band the chain was read over until one is.
fn band(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let fu = app.session.display().frequency;
    let setup = match app.session.model().job.setup() {
        Some(emcraft_model::Setup::System { band, points, frequency }) => {
            Some((band.start.get(), band.stop.get(), points.get(), frequency.map(emcraft_model::Hertz::get)))
        }
        _ => None,
    };
    let read = app
        .session
        .results()
        .system
        .as_deref()
        .and_then(|r| Some((r.points.first()?.frequency, r.points.last()?.frequency, r.points.len(), Some(r.at))));
    let now = setup.or(read);
    let shown = |x: f64| fu.from_si(x);
    let (start, stop, points, at) =
        now.map_or((None, None, None, None), |(a, b, n, f)| (Some(shown(a)), Some(shown(b)), Some(n as f64), f.map(shown)));
    let settings = |key: &str, v: Option<f64>| -> Option<Action> {
        let (mut s0, mut s1, mut n, mut f) = (start, stop, points.unwrap_or(101.0), at);
        match key {
            "start" => s0 = v,
            "stop" => s1 = v,
            "points" => n = v?.round(),
            _ => f = v,
        }
        // One end given alone: the other a fifth of the way off it.
        let (s0, s1) = match (s0, s1) {
            (Some(a), Some(b)) => (a, b),
            (Some(a), None) => (a, a * 1.2),
            (None, Some(b)) => (b / 1.2, b),
            (None, None) => return None,
        };
        let mut p = json!({"kind": "system", "start": s0, "stop": s1, "points": n as u64});
        if let Some(f) = f {
            p["frequency"] = json!(f);
        }
        Some(Action::run("project.setup", p))
    };
    ui.horizontal(|ui| {
        let fields: [(&'static str, crate::lang::Text, Option<f64>, &str, bool); 4] = [
            ("start", t!("start"), start, fu.symbol(), false),
            ("stop", t!("stop"), stop, fu.symbol(), false),
            ("points", t!("Frequency points"), points, "", false),
            ("frequency", t!("budget at"), at, fu.symbol(), true),
        ];
        for (key, label, value, unit, optional) in fields {
            ui.text(Style::Small, label);
            let (rect, _) = ui.allocate_exact_size(vec2(72.0, 20.0), Sense::hover());
            number_field(ui, Number { rect, handle: Handle::SystemBand(key), value, optional }, |v| settings(key, v), out);
            if !unit.is_empty() {
                ui.text(Style::SmallDim, Raw(unit));
            }
            ui.add_space(6.0);
        }
    });
}

/// A chain with no stage yet: what a system is, and a large button for each kind of stage.
fn start(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.text(Style::Heading, t!("A chain from the antenna to the receiver"));
        ui.add_space(4.0);
        ui.text(Style::Dim, t!("Add its stages in order: an LNA, a filter's S-parameters (a Touchstone file or a sweep), lines, more amplifiers."));
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for (kind, label, tip) in system_ui::KINDS {
                let b = egui::Button::new(egui::RichText::new(format!("+ {}", tr(label))).size(t.font_heading)).min_size(vec2(170.0, 44.0));
                if ui.named_enabled(!app.busy(), b, Handle::SystemStart(kind)).on_hover_text(tr(tip)).clicked() {
                    out.push(Action::run("system.stage.add", system_ui::starting(kind)));
                }
            }
        });
    });
}

/// What a node is: the source, a stage (its index) or the receiver.
#[derive(Clone, Copy, PartialEq)]
enum Node {
    Source,
    Stage(usize),
    Receiver,
}

impl Node {
    /// Its name in links and commands (`@source`, `@receiver`, a stage's: no stage's begins
    /// with `@`).
    fn name(self, stages: &[Stage]) -> &str {
        match self {
            Node::Source => SOURCE,
            Node::Receiver => RECEIVER,
            Node::Stage(i) => &stages[i].name,
        }
    }

    /// The node a link's end names (`@source`, `@receiver`, a stage's).
    fn of(end: &str, stages: &[Stage]) -> Option<Node> {
        match end {
            SOURCE => Some(Node::Source),
            RECEIVER => Some(Node::Receiver),
            n => stages.iter().position(|s| s.name == n).map(Node::Stage),
        }
    }
}

/// A node's field: what it sets (its command's parameter), its label, its unit, its value and
/// whether it may be left empty.
struct Field {
    key: &'static str,
    label: &'static str,
    unit: String,
    value: Option<f64>,
    optional: bool,
}

/// The fields a stage edits in place (its other settings in its dialog, All settings…).
fn stage_fields(app: &EmcraftApp, stage: &Stage) -> Vec<Field> {
    let v = emcraft_engine::system::stage_params(&app.session, &stage.part);
    let lu = app.session.display().length.symbol();
    let f = |key: &'static str, label: &'static str, unit: &str, optional: bool| Field {
        key,
        label,
        unit: unit.to_owned(),
        value: v[key].as_f64(),
        optional,
    };
    match &stage.part {
        Part::Amplifier { .. } => {
            vec![f("gain", "G", "dB", false), f("noise_figure", "NF", "dB", false), f("oip3", "OIP3", "dBm", true), f("p1db", "P1dB", "dBm", true)]
        }
        Part::Attenuator { .. } => vec![f("loss", "L", "dB", false)],
        Part::Data { .. } => Vec::new(),
        Part::Line { .. } => vec![f("impedance", "Z₀", "Ω", false), f("length", "ℓ", lu, false), f("eps_r", "εr", "", false)],
        Part::Element { .. } => vec![f("resistance", "R", "Ω", false), f("inductance", "L", "nH", false), f("capacitance", "C", "pF", false)],
    }
}

/// The rows of fields a node shows.
fn field_rows(app: &EmcraftApp, node: Node) -> usize {
    match node {
        Node::Source | Node::Receiver => 3,
        Node::Stage(i) => {
            let stage = &app.session.model().system.stages[i];
            if matches!(stage.part, Part::Data { .. }) { 1 } else { stage_fields(app, stage).len() }
        }
    }
}

/// A node's height: its header, its rows of fields and its reading.
fn node_height(rows: usize) -> f32 {
    HEAD_H + 6.0 + rows as f32 * ROW_H + ROW_H + 6.0
}

/// Where each node sits on the canvas (its top left): a stage where it was put, else laid out
/// by its place along the chain (those off it under); the source left of the first, the
/// receiver right of the rightmost.
fn layout(stages: &[Stage], order: &[usize]) -> (Vec<egui::Pos2>, egui::Pos2, egui::Pos2) {
    let mut off = 0;
    let at: Vec<egui::Pos2> = stages
        .iter()
        .enumerate()
        .map(|(i, s)| match (s.at, order.iter().position(|k| *k == i)) {
            (Some([x, y]), _) => pos2(x as f32, y as f32),
            (None, Some(k)) => pos2(STEP_X * (k + 1) as f32, 20.0),
            (None, None) => {
                off += 1;
                pos2(STEP_X * off as f32, 200.0)
            }
        })
        .collect();
    let right = at.iter().map(|p| p.x).fold(0.0f32, f32::max);
    let last_y = order.last().map_or(20.0, |&i| at[i].y);
    (at, pos2(0.0, 20.0), pos2(right + STEP_X, last_y))
}

/// A cubic link from `a` (an output) to `b` (an input), bowed out horizontally.
fn link_curve(a: egui::Pos2, b: egui::Pos2, stroke: Stroke) -> egui::Shape {
    let bow = ((b.x - a.x).abs() * 0.5).max(40.0);
    egui::epaint::CubicBezierShape::from_points_stroke([a, a + vec2(bow, 0.0), b - vec2(bow, 0.0), b], false, Color32::TRANSPARENT, stroke).into()
}

/// The canvas: the nodes, their ports and links, the drags that pan it, move nodes and link
/// them, the right-click that adds a stage.
fn canvas(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let t = &Tokens::get();
    let m = app.session.model();
    let stages = &m.system.stages;
    let (order, _) = m.system.order();
    let (at, source_at, receiver_at) = layout(stages, &order);
    // As tall as its nodes reach, within its bounds.
    let lowest = at.iter().enumerate().map(|(i, p)| p.y + node_height(field_rows(app, Node::Stage(i)))).fold(source_at.y + node_height(3), f32::max);
    let (area, bg) = ui.allocate_exact_size(vec2(ui.available_width(), (lowest + 56.0).clamp(CANVAS_MIN, CANVAS_MAX)), Sense::click_and_drag());
    scenario::publish_rect(Fixed::SystemCanvas, area);
    // The nodes' widgets in a child of the canvas' own: placing them leaves the view's layout be.
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(area));
    let ui = &mut inner;
    let pan_id = egui::Id::new("system_pan");
    let mut pan = ui.ctx().data_mut(|d| d.get_temp::<egui::Vec2>(pan_id)).unwrap_or(vec2(16.0, 16.0));
    if bg.dragged() {
        pan += bg.drag_delta();
        ui.ctx().data_mut(|d| d.insert_temp(pan_id, pan));
    }
    let p = ui.painter().with_clip_rect(area);
    p.rect_filled(area, 8.0, t.panel);
    p.rect_stroke(area, 8.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    // A grid of dots under the nodes.
    let step = 24.0;
    let mut y = area.top() + pan.y.rem_euclid(step);
    while y < area.bottom() {
        let mut x = area.left() + pan.x.rem_euclid(step);
        while x < area.right() {
            p.circle_filled(pos2(x, y), 0.8, t.border);
            x += step;
        }
        y += step;
    }
    let origin = area.min + pan;
    // A node being dragged moves with the pointer until it is let go.
    let drag_id = |n: &str| egui::Id::new(("system_drag", n.to_owned()));
    let ctx = ui.ctx().clone();
    let moved = |n: &str| ctx.data_mut(|d| d.get_temp::<egui::Vec2>(drag_id(n))).unwrap_or(egui::Vec2::ZERO);
    let rect_of = |node: Node| {
        let top_left = match node {
            Node::Source => source_at,
            Node::Receiver => receiver_at,
            Node::Stage(i) => at[i] + moved(&stages[i].name),
        };
        Rect::from_min_size(origin + top_left.to_vec2(), vec2(NODE_W, node_height(field_rows(app, node))))
    };
    let nodes: Vec<Node> = std::iter::once(Node::Source).chain((0..stages.len()).map(Node::Stage)).chain(std::iter::once(Node::Receiver)).collect();
    let out_port = |n: Node| rect_of(n).right_top() + vec2(0.0, HEAD_H / 2.0);
    let in_port = |n: Node| rect_of(n).left_top() + vec2(0.0, HEAD_H / 2.0);
    // The links: the model's, or (a chain written before them) its order's.
    let links: Vec<(String, String)> = if m.system.links.is_empty() {
        let names: Vec<&str> =
            std::iter::once(SOURCE).chain(order.iter().map(|&i| stages[i].name.as_str())).chain(std::iter::once(RECEIVER)).collect();
        names.windows(2).map(|w| (w[0].to_owned(), w[1].to_owned())).collect()
    } else {
        m.system.links.iter().map(|l| (l.from.clone(), l.to.clone())).collect()
    };
    let on_chain = |n: Node| match n {
        Node::Stage(i) => order.contains(&i),
        _ => true,
    };
    for (from, to) in &links {
        let (Some(a), Some(b)) = (Node::of(from, stages), Node::of(to, stages)) else { continue };
        let live = on_chain(a) && on_chain(b);
        p.add(link_curve(out_port(a), in_port(b), Stroke::new(2.0, if live { t.accent } else { t.text_dim })));
        // Its mark at the middle takes it away.
        let mid = out_port(a).lerp(in_port(b), 0.5);
        let resp = ui.hit(Rect::from_center_size(mid, vec2(16.0, 16.0)), Handle::SystemUnlink(b.name(stages)), Sense::click());
        let hovered = resp.hovered();
        p.circle(mid, 6.0, if hovered { t.hover } else { t.panel }, Stroke::new(1.0, if hovered { t.accent } else { t.border }));
        p.text(mid, Align2::CENTER_CENTER, "×", FontId::proportional(t.font_small - 1.0), if hovered { t.text } else { t.text_dim });
        if resp.hint(t!("Take this link away")).clicked() {
            out.push(Action::run("system.unlink", json!({"from": a.name(stages), "to": b.name(stages)})));
        }
    }
    // A link being dragged from an output port, to the pointer.
    let linking_id = egui::Id::new("system_linking");
    let linking: Option<String> = ui.ctx().data_mut(|d| d.get_temp::<String>(linking_id));
    if let (Some(from), Some(ptr)) = (linking.as_deref().and_then(|n| Node::of(n, stages)), ui.ctx().pointer_latest_pos()) {
        p.add(link_curve(out_port(from), ptr, Stroke::new(2.0, t.accent)));
    }
    for &node in &nodes {
        let r = rect_of(node);
        let name = node.name(stages);
        let off = !on_chain(node);
        p.rect(r, 8.0, if off { t.panel_header } else { t.dialog_body }, Stroke::new(1.2, t.border), egui::StrokeKind::Inside);
        // The header: a stage's is dragged to move it, its menu has the rest.
        let head = Rect::from_min_size(r.min, vec2(r.width(), HEAD_H));
        p.rect_filled(head, egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, if off { t.border } else { t.accent_soft });
        let title = match node {
            Node::Source => tr(t!("Source")).to_owned(),
            Node::Receiver => tr(t!("Receiver")).to_owned(),
            Node::Stage(i) => format!("{}  ·  {}", emcraft_engine::text::clip(&stages[i].name, 18), tr(system_ui::kind_name(&stages[i].part))),
        };
        p.text(head.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, title, FontId::proportional(t.font_small), t.text);
        let sense = if matches!(node, Node::Stage(_)) { Sense::click_and_drag() } else { Sense::hover() };
        let resp = ui.hit(head, Handle::SystemNode(name), sense);
        if let Node::Stage(i) = node {
            let stage = &stages[i];
            if resp.dragged() {
                let d = moved(name) + resp.drag_delta();
                ui.ctx().data_mut(|m| m.insert_temp(drag_id(name), d));
            }
            if resp.drag_stopped() {
                let to = at[i] + moved(name);
                ui.ctx().data_mut(|m| m.remove::<egui::Vec2>(drag_id(name)));
                out.push(Action::run("system.stage.edit", json!({"stage": name, "at": [to.x.round(), to.y.round()]})));
            }
            let resp = resp.on_hover_cursor(egui::CursorIcon::Grab).hint(t!("Drag to move it; right-click for more"));
            resp.context_menu(|ui| {
                if ui.button(tr(t!("All settings…"))).clicked() {
                    out.push(Action::EditItem(system_ui::item(&stage.name)));
                    ui.close();
                }
                if ui.button(tr(t!("Delete"))).clicked() {
                    out.push(Action::run("system.stage.delete", json!({"stage": stage.name})));
                    ui.close();
                }
            });
            if off {
                p.text(
                    r.right_bottom() + vec2(-8.0, -6.0),
                    Align2::RIGHT_BOTTOM,
                    tr(t!("off the chain")),
                    FontId::proportional(t.font_small - 1.0),
                    t.text_dim,
                );
            }
        }
        // Its ports: the source has no input, the receiver no output.
        if node != Node::Source {
            let c = in_port(node);
            p.circle(c, PORT_R, t.panel, Stroke::new(1.5, t.accent));
            scenario::publish_target(Handle::SystemIn(name), Rect::from_center_size(c, vec2(2.0 * PORT_R, 2.0 * PORT_R)));
        }
        if node != Node::Receiver {
            let c = out_port(node);
            p.circle_filled(c, PORT_R, t.accent);
            let port = ui.hit(Rect::from_center_size(c, vec2(2.0 * PORT_REACH, 2.0 * PORT_REACH)), Handle::SystemOut(name), Sense::drag());
            if port.drag_started() {
                ui.ctx().data_mut(|d| d.insert_temp(linking_id, name.to_owned()));
            }
            if port.drag_stopped() {
                ui.ctx().data_mut(|d| d.remove::<String>(linking_id));
                let target = ui
                    .ctx()
                    .pointer_latest_pos()
                    .and_then(|ptr| nodes.iter().copied().find(|n| *n != Node::Source && *n != node && in_port(*n).distance(ptr) <= PORT_REACH));
                if let Some(to) = target {
                    out.push(Action::run("system.link", json!({"from": name, "to": to.name(stages)})));
                }
            }
            port.on_hover_cursor(egui::CursorIcon::Crosshair).hint(t!("Drag to another node's input to link them"));
        }
        node_body(app, ui, node, r, out);
    }
    // A right-click on the canvas adds a stage there, off the chain until linked.
    let at_click = bg.interact_pointer_pos().or_else(|| ui.ctx().pointer_interact_pos()).map(|ptr| ptr - origin);
    bg.context_menu(|ui| {
        ui.text(Style::SmallDim, t!("Add a stage here"));
        for (kind, label, _) in system_ui::KINDS {
            if ui.button(tr(label)).clicked() {
                let mut v = system_ui::starting(kind);
                if let Some(d) = at_click {
                    v["at"] = json!([d.x.round(), d.y.round()]);
                    v["linked"] = json!(false);
                }
                out.push(Action::run("system.stage.add", v));
                ui.close();
            }
        }
    });
}

/// A choice in place at `rect`: the options' texts and values, the one chosen; the value picked
/// when another is.
fn choice(ui: &mut egui::Ui, rect: Rect, id: egui::Id, options: &[(String, String)], current: &str) -> Option<String> {
    let mut chosen = current.to_owned();
    let shown = options.iter().find(|(v, _)| v == current).map_or(current, |(_, s)| s.as_str()).to_owned();
    ui.put(rect, |ui: &mut egui::Ui| {
        egui::ComboBox::from_id_salt(id)
            .selected_text(shown)
            .width(rect.width())
            .show_ui(ui, |ui| {
                for (v, s) in options {
                    ui.selectable_value(&mut chosen, v.clone(), s);
                }
            })
            .response
    });
    (chosen != current).then_some(chosen)
}

/// A node's fields in place under its header, and what the chain reads at its output.
fn node_body(app: &EmcraftApp, ui: &mut egui::Ui, node: Node, r: Rect, out: &mut Actions) {
    let t = &Tokens::get();
    let m = app.session.model();
    let p = ui.painter().with_clip_rect(r);
    let row = |k: usize| Rect::from_min_size(pos2(r.left() + 10.0, r.top() + HEAD_H + 4.0 + k as f32 * ROW_H), vec2(r.width() - 20.0, ROW_H - 2.0));
    let label = |k: usize, text: &str| {
        p.text(row(k).left_center(), Align2::LEFT_CENTER, text, FontId::proportional(t.font_small), t.text_dim);
    };
    let value_rect = |k: usize| Rect::from_min_max(pos2(row(k).left() + 56.0, row(k).top()), pos2(row(k).right() - 36.0, row(k).bottom()));
    let wide_rect = |k: usize| Rect::from_min_max(pos2(row(k).left() + 56.0, row(k).top()), row(k).max);
    let unit = |k: usize, u: &str| {
        p.text(row(k).right_center(), Align2::RIGHT_CENTER, u, FontId::proportional(t.font_small), t.text_dim);
    };
    let fu = app.session.display().frequency;
    let measured =
        |ports: usize| app.session.views().measured.iter().filter(move |x| x.data.outputs() == ports).map(|x| (x.name.clone(), x.name.clone()));
    let reading: Option<String> = match node {
        Node::Source => {
            let src = &m.system.source;
            label(0, tr(t!("antenna")));
            let options: Vec<(String, String)> =
                std::iter::once((String::new(), format!("{} Ω", m.system.receiver.impedance))).chain(measured(1)).collect();
            if let Some(a) = choice(ui, wide_rect(0), egui::Id::new("system_antenna"), &options, src.antenna.as_deref().unwrap_or("")) {
                out.push(Action::run("system.source", json!({"antenna": a})));
            }
            label(1, "T");
            number_field(
                ui,
                Number { rect: value_rect(1), handle: Handle::SystemField(SOURCE, "temperature"), value: Some(src.temperature), optional: false },
                |v| Some(Action::run("system.source", json!({"temperature": v?}))),
                out,
            );
            unit(1, "K");
            label(2, tr(t!("signal")));
            number_field(
                ui,
                Number { rect: value_rect(2), handle: Handle::SystemField(SOURCE, "power"), value: src.power, optional: true },
                |v| Some(Action::run("system.source", json!({"power": v}))),
                out,
            );
            unit(2, "dBm");
            None
        }
        Node::Receiver => {
            let rx = &m.system.receiver;
            label(0, "B");
            number_field(
                ui,
                Number {
                    rect: value_rect(0),
                    handle: Handle::SystemField(RECEIVER, "bandwidth"),
                    value: rx.bandwidth.map(|b| fu.from_si(b)),
                    optional: true,
                },
                |v| Some(Action::run("system.receiver", json!({"bandwidth": v}))),
                out,
            );
            unit(0, fu.symbol());
            label(1, "SNR");
            number_field(
                ui,
                Number { rect: value_rect(1), handle: Handle::SystemField(RECEIVER, "snr"), value: Some(rx.snr), optional: false },
                |v| Some(Action::run("system.receiver", json!({"snr": v?}))),
                out,
            );
            unit(1, "dB");
            label(2, "Z₀");
            number_field(
                ui,
                Number { rect: value_rect(2), handle: Handle::SystemField(RECEIVER, "impedance"), value: Some(rx.impedance), optional: false },
                |v| Some(Action::run("system.receiver", json!({"impedance": v?}))),
                out,
            );
            unit(2, "Ω");
            let sens = app.session.results().system.as_deref().and_then(|r| r.nearest(r.at)).and_then(|p| p.sensitivity_dbm);
            sens.map(|s| trf(t!("sensitivity {s} dBm"), crate::args!(s = format!("{s:.1}"))))
        }
        Node::Stage(i) => {
            let stage = &m.system.stages[i];
            let name = stage.name.as_str();
            if let Part::Data { data, .. } = &stage.part {
                label(0, "S");
                let current = match data {
                    DataSource::Sweep => "sweep".to_owned(),
                    DataSource::Measurement { name } => name.clone(),
                };
                let options: Vec<(String, String)> =
                    std::iter::once(("sweep".to_owned(), tr(t!("the sweep")).to_owned())).chain(measured(2)).collect();
                if let Some(d) = choice(ui, wide_rect(0), egui::Id::new(("system_data", name.to_owned())), &options, &current) {
                    out.push(Action::run("system.stage.edit", json!({"stage": name, "data": d})));
                }
            } else {
                for (k, f) in stage_fields(app, stage).iter().enumerate() {
                    label(k, f.label);
                    let key = f.key;
                    let commit = |v: Option<f64>| {
                        let mut p = json!({"stage": name});
                        p[key] = json!(v);
                        Some(Action::run("system.stage.edit", p))
                    };
                    number_field(
                        ui,
                        Number { rect: value_rect(k), handle: Handle::SystemField(name, key), value: f.value, optional: f.optional },
                        commit,
                        out,
                    );
                    unit(k, &f.unit);
                }
            }
            system_ui::reading(&app.session, name).map(|(g, nf)| {
                let f = |x: Option<f64>| x.map_or_else(|| "—".to_owned(), |v| format!("{v:.2}"));
                trf(t!("total {gain} dB · NF {nf} dB"), crate::args!(gain = f(g), nf = f(nf)))
            })
        }
    };
    if let Some(text) = reading {
        let rr = row(field_rows(app, node));
        p.text(rr.left_center(), Align2::LEFT_CENTER, text, FontId::proportional(t.font_small), t.accent);
    }
}

/// The budget at its frequency, a row a stage, and the receiver's reading under it.
fn budget(app: &EmcraftApp, r: &SystemResult, ui: &mut egui::Ui) {
    let Some(p) = r.nearest(r.at) else { return };
    let num = |x: Option<f64>, digits: usize| x.map_or_else(|| "—".to_owned(), |v| format!("{v:.digits$}"));
    let headers: Vec<String> = [
        t!("stage"),
        t!("gain (dB)"),
        t!("total gain (dB)"),
        t!("NF (dB)"),
        t!("Te (K)"),
        t!("signal (dBm)"),
        t!("noise (dBm)"),
        t!("SNR (dB)"),
        t!("IIP3 (dBm)"),
        t!("OIP3 (dBm)"),
        t!("OP1dB (dBm)"),
    ]
    .into_iter()
    .map(|h| tr(h).to_owned())
    .collect();
    let row = |name: &str, x: &Row| {
        vec![
            name.to_owned(),
            num(x.stage_gain_db, 2),
            num(x.gain_db, 2),
            num(x.nf_db, 3),
            num(x.noise_temperature, 1),
            num(x.signal_dbm, 2),
            num(x.noise_dbm, 2),
            num(x.snr_db, 2),
            num(x.iip3_dbm, 2),
            num(x.oip3_dbm, 2),
            num(x.op1db_dbm, 2),
        ]
    };
    let rows: Vec<Vec<String>> = r.stages.iter().zip(&p.rows).map(|((name, _), x)| row(name, x)).collect();
    egui::Grid::new("system_budget").striped(true).num_columns(headers.len()).show(ui, |ui| {
        super::dim_if_stale(ui);
        for h in &headers {
            ui.text(Style::Strong, Raw(h));
        }
        ui.end_row();
        for cells in &rows {
            for c in cells {
                ui.label(c);
            }
            ui.end_row();
        }
    });
    ui.add_space(4.0);
    if let (Some(t), Some(s)) = (p.system_temperature, p.sensitivity_dbm) {
        ui.text(
            Style::Body,
            Raw(&trf(t!("System temperature {t} K; sensitivity {s} dBm"), crate::args!(t = format!("{t:.1}"), s = format!("{s:.2}")))),
        );
    } else if app.session.model().system.receiver.bandwidth.is_none() {
        ui.text(Style::Dim, t!("Give the receiver a bandwidth (on its node) for its noise, SNR and sensitivity."));
    }
    if let Some(why) = &p.why {
        ui.text(Style::Dim, Raw(why));
    }
}

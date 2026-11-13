//! The PCB page: a board's guided diagnosis — prepare the board, select the route, set the
//! conditions, run the diagnosis, review the results — a pill a stage (`view.pcb`), the stage's
//! panel on the left and the board in 3D on the right (a net clicked there becomes the route).
//! The results: the findings on the return path, each section's ideal line, and the ideal line's
//! S-parameters and TDR drawn by the GPU.

use std::collections::BTreeSet;

use emcraft_engine::pcb::{Inspection, Stage};
use emcraft_model::units::{Time, Unit};
use emcraft_render::axes::Axis;
use emcraft_render::chart::{Chart, Series, Style};
use emcraft_render::palette::Pen;
use emcraft_ui_core::files::{FileAction, Then};
use emcraft_ui_core::lang::{args, lookup, sample_label, tr, trf};
use emcraft_ui_core::pcb::{finding_label, report, route};
use iced::widget::{Column, button, canvas, column, container, row, scrollable, shader, text, text_input};
use iced::{Alignment, Element, Fill, Widget};
use serde_json::json;

use crate::app::{App, Field, Message};
use crate::chart::Plot;
use crate::look::{self, Tokens};
use crate::view3d::shader::Viewer;

/// The conditions form's fields' node, in a [`Field`].
pub const CONDITIONS: &str = "@pcb";

/// The conditions form's fields: their parameter, label and value before any is confirmed.
const FIELDS: [(&str, emcraft_ui_core::lang::Text, &str); 3] =
    [("ground", t!("Reference net"), "GND"), ("reference", t!("Reference impedance (Ω)"), "50"), ("stop", t!("Maximum frequency"), "1")];

/// The panel's width (points).
const PANEL_W: f32 = 420.0;

fn run(id: &str, params: serde_json::Value) -> Message {
    Message::Run(emcraft_ui_core::system::gesture::Command { id: emcraft_engine::find_command(id).map_or("view.pcb", |c| c.id), params })
}

/// A field of the conditions form as it stands: what is typed, else the confirmed value, else
/// its default.
pub fn condition(app: &App, key: &'static str) -> String {
    let field = Field { node: CONDITIONS.to_owned(), key };
    let confirmed = app.session.views().pcb.conditions().map(|c| c.shown(app.session.display().frequency));
    let default = FIELDS.iter().find(|(k, ..)| *k == key).map_or("", |(.., d)| d);
    app.drafts
        .get(&field)
        .cloned()
        .unwrap_or_else(|| confirmed.as_ref().and_then(|c| c.get(key)).map_or_else(|| default.to_owned(), emcraft_ui_core::dialogs::value_text))
}

/// The conditions as the form stands, the numbers read as numbers.
pub fn conditions(app: &App) -> serde_json::Value {
    let number = |s: String| s.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map_or(serde_json::Value::String(s), serde_json::Value::from);
    json!({"ground": condition(app, "ground").trim(), "reference": number(condition(app, "reference")), "stop": number(condition(app, "stop"))})
}

/// Whether the form is the conditions confirmed.
fn confirmed(app: &App) -> bool {
    let shown = app.session.views().pcb.conditions().map(|c| c.shown(app.session.display().frequency));
    shown.is_some_and(|c| emcraft_ui_core::steps::same(&c, &conditions(app)))
}

/// The stages' pills: each open once the one before it is done.
fn pills(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let current = emcraft_engine::pcb::stage(&app.session);
    let loaded = app.session.model().pcb().is_some();
    let net = route(&app.session);
    let ready = net.is_some_and(|n| report(&app.session, &app.replies, n).is_some());
    let pills = Stage::ALL.into_iter().enumerate().map(|(i, stage)| {
        let open = match stage {
            Stage::Board => true,
            Stage::Net => loaded,
            Stage::Settings | Stage::Inspect => loaded && net.is_some(),
            Stage::Results => loaded && ready,
        };
        let label = format!("{} {}", i + 1, lookup(stage.label()));
        button(text(label).size(t.font_button))
            .style(look::toggle(t, stage == current))
            .on_press_maybe((open && app.job.is_none()).then(|| run("view.pcb", json!({"stage": stage.id()}))))
            .boxed()
    });
    row(pills).spacing(t.space_6).wrap().boxed()
}

/// The stage's panel.
fn panel(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let stage = emcraft_engine::pcb::stage(&app.session);
    let selected = route(&app.session).unwrap_or("");
    let idle = app.job.is_none();
    let small = |s: String| text(s).size(t.font_small).color(t.text_dim).boxed();
    let body: Vec<Element<Message>> = match (app.session.model().pcb(), stage) {
        (None, _) | (Some(_), Stage::Board) => sources(app),
        (Some(board), Stage::Net) => {
            let nets: BTreeSet<&str> = board.segments.iter().map(|s| s.net.as_str()).collect();
            let list = nets.into_iter().map(|n| {
                button(text(n.to_owned()).size(t.font_body))
                    .style(look::row(t, n == selected))
                    .on_press_maybe(idle.then(|| run("view.pcb", json!({"net": n, "stage": Stage::Net.id()}))))
                    .boxed()
            });
            vec![small(tr(t!("Choose a routed net")).to_owned()), Column::with_children(list).spacing(t.space_2).boxed()]
        }
        (Some(_), Stage::Settings) => {
            let fields = FIELDS.iter().map(|(key, label, _)| {
                let field = Field { node: CONDITIONS.to_owned(), key };
                let typed = field.clone();
                let value = condition(app, key);
                let input = text_input("", value).size(t.font_body).padding([t.space_4, t.space_6]).width(120).style(look::field(t));
                row![text(tr(*label)).size(t.font_body).width(200), input.on_input(move |s| Message::Typed(typed.clone(), s))]
                    .spacing(t.space_8)
                    .align_y(Alignment::Center)
                    .boxed()
            });
            let ok = confirmed(app);
            let confirm: Element<Message> = if ok {
                text(format!("✔ {}", tr(t!("Conditions confirmed")))).size(t.font_body).color(t.success).boxed()
            } else {
                let b = button(text(format!("✔ {}", tr(t!("Confirm conditions")))).size(t.font_button)).style(look::button(t, true));
                column![
                    b.on_press_maybe(idle.then(|| run("pcb.conditions", conditions(app)))),
                    small(tr(t!("Not confirmed: the diagnosis waits for these conditions.")).to_owned())
                ]
                .spacing(t.space_4)
                .boxed()
            };
            fields.chain([small(app.session.display().frequency.symbol().to_owned()), confirm]).collect()
        }
        (Some(_), Stage::Inspect) => {
            let unit = app.session.display().frequency;
            let summary = app
                .session
                .views()
                .pcb
                .conditions()
                .map(|c| small(format!("{selected} → {}  |  {} Ω  |  {} {}", c.ground, c.reference, unit.from_si(c.stop_hz), unit.symbol())));
            let ready = confirmed(app);
            let inspect = button(text(tr(t!("Inspect route"))).size(t.font_button)).style(look::button(t, true));
            let inspect = inspect.on_press_maybe((ready && idle).then(|| run("pcb.inspect", json!({"net": selected}))));
            let hint = (!ready).then(|| small(tr(t!("Confirm the conditions first (step 3).")).to_owned()));
            summary.into_iter().chain(std::iter::once(inspect.boxed())).chain(hint).collect()
        }
        (Some(_), Stage::Results) => results(app, report(&app.session, &app.replies, selected)),
    };
    let error = app.error.as_ref().map(|e| text(e.clone()).size(t.font_small).color(t.error).boxed());
    let stackup = (stage != Stage::Results).then(|| stackup(app, selected));
    let head = text(lookup(stage.label()).to_owned()).size(t.font_heading).boxed();
    let content =
        Column::with_children(std::iter::once(head).chain(body).chain(error).chain(stackup.flatten())).spacing(t.space_8).padding(t.space_12);
    container(scrollable(content)).width(PANEL_W).height(Fill).style(look::ground(t)).boxed()
}

/// Where a board comes from: the PCB templates (a KiCad file through the ribbon's Import).
fn sources(app: &App) -> Vec<Element<'_, Message>> {
    let t = app.look.tokens;
    let templates = emcraft_engine::templates::made_for(emcraft_engine::workflow::Analysis::Pcb).map(|s| {
        let open = Then::Sample { name: s.name.to_owned(), values: serde_json::Value::Null, purpose: None };
        button(text(sample_label(s.name).to_owned()).size(t.font_body))
            .style(look::row(t, false))
            .on_press(Message::File(FileAction::Request(open)))
            .boxed()
    });
    vec![crate::parts::heading(t, t!("Templates")), Column::with_children(templates).spacing(t.space_2).boxed()]
}

/// The layer stackup, top first: each copper layer and the dielectric under it.
fn stackup<'a>(app: &'a App, selected: &str) -> Option<Element<'a, Message>> {
    let t = app.look.tokens;
    let board = app.session.model().pcb()?;
    let l = app.session.display().length;
    let ground = condition(app, "ground");
    let rows = board.layers.iter().enumerate().flat_map(|(i, layer)| {
        let chosen = board.segments.iter().any(|s| s.net == selected && s.layer == layer.name);
        let reference = board.copper.iter().any(|c| c.net == ground && c.layer == layer.name);
        let ink = if chosen {
            t.warning
        } else if reference {
            t.success
        } else {
            t.text
        };
        let copper = text(layer.name.clone()).size(t.font_body).color(ink).boxed();
        let under = board.layers.get(i + 1).map(|next| {
            let h = l.from_si(next.z - layer.z).abs();
            let line = match layer.loss_below {
                0.0 => format!("    εr = {}  ·  {h:.3} {}", layer.eps_below, l.symbol()),
                tan_d => format!("    εr = {}  ·  tan δ = {tan_d}  ·  {h:.3} {}", layer.eps_below, l.symbol()),
            };
            text(line).size(t.font_small).color(t.text_dim).boxed()
        });
        std::iter::once(copper).chain(under)
    });
    let head = crate::parts::heading(t, t!("Layer stackup (section)"));
    Some(Column::with_children(std::iter::once(head).chain(rows)).spacing(t.space_4).boxed())
}

/// A chart of the ideal line: lines of `series` (key, label, points) over `x`, magnitudes on `y`.
fn line_chart(title: String, x: Axis, y: Axis, series: Vec<(&str, String, Vec<[f64; 2]>)>) -> Chart {
    let list = series
        .into_iter()
        .enumerate()
        .map(|(k, (key, label, pts))| Series::new(key, label, Pen::Series(u8::try_from(k).unwrap_or(0)), Style::Line, pts))
        .collect();
    Chart::new(title, x, y, list)
}

/// The report: the route's length, the findings on its return path, each section's ideal line,
/// and the ideal line's S-parameters and TDR.
fn results<'a>(app: &'a App, r: Option<&'a Inspection>) -> Vec<Element<'a, Message>> {
    let t = app.look.tokens;
    let Some(r) = r else {
        let again = app
            .replies
            .pcb
            .is_some()
            .then(|| text(tr(t!("Conditions changed; inspect the route again."))).size(t.font_small).color(t.text_dim).boxed());
        return again.into_iter().collect();
    };
    let small = |s: String, ink| text(s).size(t.font_small).color(ink).boxed();
    let heading = |s: emcraft_ui_core::lang::Text| crate::parts::heading(t, s);
    let mut seen = BTreeSet::new();
    let findings = r.findings.iter().filter(|f| seen.insert((f.code.id(), f.detail.clone()))).flat_map(|f| {
        let ink = if f.code == emcraft_engine::pcb::Code::NearbySignal { t.warning } else { t.error };
        [text(tr(finding_label(f.code))).size(t.font_body).color(ink).boxed(), small(f.detail.clone(), t.text_dim)]
    });
    let none = r.findings.is_empty().then(|| small(tr(t!("No issues found.")).to_owned(), t.text_dim));
    let sections = r.sections.iter().map(|s| {
        small(format!("{} → {}: {:.2} Ω, {:.1} ps", s.id, s.reference_layer, s.ideal_impedance, Time::Picosecond.from_si(s.ideal_delay_s)), t.text)
    });
    let length =
        trf(t!("Selected net: {net}; routed length {length} {unit}"), args!(net = r.net, length = format!("{:.3}", r.length), unit = r.length_unit));
    let head = [small(length, t.text), heading(t!("Return-path geometry"))];
    let line = [heading(t!("Ideal transmission-line estimate"))];
    let si: Vec<Element<Message>> = match r.si.as_ref().filter(|_| r.model_available) {
        Some(si) => si_charts(app, t, si),
        None => vec![small(tr(t!("No S-parameters: the route is outside the line model.")).to_owned(), t.text_dim)],
    };
    head.into_iter().chain(findings).chain(none).chain(line).chain(sections).chain(si).collect()
}

/// The ideal line's S-parameters and its TDR, each a chart drawn by the GPU.
fn si_charts<'a>(app: &'a App, t: Tokens, si: &emcraft_engine::pcb::Line) -> Vec<Element<'a, Message>> {
    let fu = &si.frequency_unit;
    let along = |f: fn(&emcraft_engine::pcb::LinePoint) -> f64| si.points.iter().map(|p| [p.frequency, f(p)]).collect::<Vec<_>>();
    let top = si.points.last().map_or(1.0, |p| p.frequency);
    let s = line_chart(
        String::new(),
        Axis::new(&format!("f ({fu})"), 0.0, top),
        Axis::new("|S|", 0.0, 1.0),
        vec![("s11", "|S11|".into(), along(|p| p.s11.mag)), ("s21", "|S21|".into(), along(|p| p.s21.mag))],
    );
    let horizon = 4.0 * si.delay_s.max(si.resolution_s);
    let shown: Vec<[f64; 2]> =
        si.tdr.iter().take_while(|p| p.time_s <= horizon).map(|p| [Time::Nanosecond.from_si(p.time_s), p.reflection]).collect();
    let end = shown.last().map_or(1.0, |p| p[0]);
    let tdr = line_chart(
        tr(t!("TDR reflection (ideal line model)")).to_owned(),
        Axis::new("t (ns)", 0.0, end),
        Axis::new("Γ", -1.0, 1.0),
        vec![("tdr", "Γ".into(), shown)],
    );
    let delay = trf(t!("Propagation delay: {value} ps"), args!(value = format!("{:.1}", Time::Picosecond.from_si(si.delay_s))));
    let palette = &app.look.theme.palette;
    [s, tdr]
        .into_iter()
        .map(|c| canvas(Plot { chart: std::borrow::Cow::Owned(c), palette }).width(Fill).height(220).boxed())
        .chain(std::iter::once(text(delay).size(t.font_small).color(t.text).boxed()))
        .collect()
}

/// The PCB page: the stages, the panel, and the board in 3D.
pub fn view(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let picture: Element<Message> = match app.view3d.shown.as_ref().filter(|s| s.bounds.is_some()) {
        Some(s) => {
            let viewer = Viewer {
                shot: app.shot(s),
                nav: app.nav,
                hover: false,
                tool: false,
                grips: Vec::new(),
                around: app.prefs.orbit_around(),
                plane: None,
            };
            // Drawn without a window, the board is a picture of it (crate::headless).
            if app.headless { crate::view3d::shader::picture(viewer.shot, t) } else { shader(viewer).width(Fill).height(Fill).boxed() }
        }
        None => crate::parts::empty(t, tr(t!("Prepare board")), None),
    };
    column![pills(app), row![panel(app), picture].spacing(t.space_12).height(Fill)].spacing(t.space_8).padding(t.space_12).height(Fill).boxed()
}

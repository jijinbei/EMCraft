//! A result chart's style card in the iced window, under the chart's choices while open (a ⚙
//! beside them opens and closes it): its title, each axis' label, ends and log scale, the grid,
//! the legend's place, Reset, the curves it may add and each curve shown or not, its colour
//! (the theme's, or one of the palette's series colours) and its width. Every change is one
//! `view.chart.style` command ([`emcraft_ui_core::results::style`]).

use emcraft_engine::views::{AxisId, LegendAt};
use emcraft_ui_core::lang::tr;
use emcraft_ui_core::results::style::{self, Field, LEGENDS, StyleForm, WIDTHS};
use iced::widget::{Column, button, checkbox, container, row, text, text_input};
use iced::{Alignment, Element, Widget};
use serde_json::{Value, json};

use crate::app::{App, Message};
use crate::look::{self, Tokens};

/// What the card asks for.
#[derive(Clone, Debug)]
pub enum StyleAct {
    /// The card of the page's chart opened or closed.
    Toggle,
    Typed(Field, String),
    Commit(Field),
    /// A `view.chart.style` command.
    Send(Value),
}

fn send(a: StyleAct) -> Message {
    Message::Style(a)
}

impl App {
    /// What the style card asks for.
    pub(crate) fn style_act(&mut self, a: StyleAct) {
        let Some(id) = style::chart_of(self.page) else { return };
        let shown = emcraft_engine::views::shown_style(&self.session, id);
        match a {
            StyleAct::Toggle => self.style = std::mem::take(&mut self.style).toggled(id),
            StyleAct::Typed(f, s) => self.style = std::mem::take(&mut self.style).synced(&shown).typed(f, &s),
            StyleAct::Commit(f) => {
                if let Some(params) = self.style.commit(f, &shown) {
                    self.run_now("view.chart.style", &params);
                }
            }
            StyleAct::Send(params) => self.run_now("view.chart.style", &params),
        }
    }

    /// The card's fields following the style as shown, until typed in.
    pub(crate) fn sync_style(&mut self) {
        match (self.style.open, style::chart_of(self.page)) {
            (Some(open), Some(id)) if open == id => {
                let shown = emcraft_engine::views::shown_style(&self.session, id);
                self.style = std::mem::take(&mut self.style).synced(&shown);
            }
            (Some(_), _) => self.style = StyleForm::default(),
            (None, _) => {}
        }
    }
}

/// The ⚙ that opens and closes the card, on a page whose chart has a style.
pub fn gear(app: &App) -> Option<Element<'_, Message>> {
    let t = app.look.tokens;
    style::chart_of(app.page)?;
    let open = app.style.open.is_some();
    Some(button(text("⚙").size(t.font_button)).style(look::button(t, open)).on_press(send(StyleAct::Toggle)).boxed())
}

fn chip<'a>(t: Tokens, label: &str, on: bool, msg: Message) -> Element<'a, Message> {
    button(text(label.to_owned()).size(t.font_small)).style(look::button(t, on)).on_press(msg).boxed()
}

fn field<'a>(t: Tokens, f: Field, value: &'a str, hint: &'a str, width: f32) -> Element<'a, Message> {
    text_input(hint, value)
        .size(t.font_small)
        .width(width)
        .padding([2, 6])
        .style(look::field(t))
        .on_input(move |s| send(StyleAct::Typed(f, s)))
        .on_submit(send(StyleAct::Commit(f)))
        .boxed()
}

/// The card, while open on the page's chart.
pub fn card(app: &App) -> Option<Element<'_, Message>> {
    let id = app.style.open.filter(|o| style::chart_of(app.page) == Some(*o))?;
    let t = app.look.tokens;
    let shown = emcraft_engine::views::shown_style(&app.session, id);
    let p = move |more: Value| send(StyleAct::Send(style::params(id, more)));
    let s = &app.style;
    let head = row![
        text(tr(t!("Axes and looks"))).size(t.font_body),
        chip(t, tr(t!("Reset")), false, p(json!({"reset": true}))),
        chip(t, "✕", false, send(StyleAct::Toggle))
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let title = field(t, Field::Title, &s.title, tr(t!("title (automatic)")), 320.0);
    let axes = AxisId::ALL.into_iter().enumerate().map(|(k, a)| {
        let log = shown[a.id()]["log"].as_bool().unwrap_or(false);
        let scale = json!({format!("{}_scale", a.id()): if log { "linear" } else { "log" }});
        row![
            text(a.id()).size(t.font_small).width(16),
            field(t, Field::Label(k), s.text(Field::Label(k)), tr(t!("label")), 140.0),
            field(t, Field::Min(k), s.text(Field::Min(k)), tr(t!("auto")), 64.0),
            field(t, Field::Max(k), s.text(Field::Max(k)), tr(t!("auto")), 64.0),
            chip(t, "log", log, p(scale))
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .boxed()
    });
    let grid = shown["grid"].as_bool().unwrap_or(true);
    let legend = shown["legend"].as_str().and_then(LegendAt::parse).unwrap_or_default();
    let places = LEGENDS.iter().map(|(at, mark, _)| chip(t, mark, legend == *at, p(json!({"legend": at.id()}))));
    let looks = row(std::iter::once(chip(t, tr(t!("grid")), grid, p(json!({"grid": !grid})))).chain(places)).spacing(4).align_y(Alignment::Center);
    // The curves the chart may add, and its own.
    let added = style::names(&shown, "added");
    let optional = emcraft_engine::views::optional_curves(&app.session, id);
    let adds = optional.into_iter().map(|(key, label)| {
        let on = added.contains(&key.as_str());
        let list: Vec<String> = style::with(&added, &key, !on).into_iter().map(str::to_owned).collect();
        checkbox(on)
            .label(format!("{} {}", tr(t!("Add")), style::plain(&label)))
            .text_size(t.font_small)
            .on_toggle(move |_| p(json!({"added": list.clone()})))
            .boxed()
    });
    let hidden = style::names(&shown, "hidden");
    let curves: Vec<(String, String)> = app
        .charts
        .page
        .as_ref()
        .and_then(|c| c.shown.as_ref().ok())
        .map(|c| c.curves().into_iter().map(|(k, l)| (k.to_owned(), l.to_owned())).collect())
        .unwrap_or_default();
    let palette = &app.look.theme.palette;
    let swatches: Vec<emcraft_render::Rgb> =
        (0..emcraft_render::palette::SERIES as u8).map(|i| palette.pen(emcraft_render::Pen::Series(i))).chain([palette.ink]).collect();
    let rows = curves.into_iter().map(|(key, label)| {
        let on = !hidden.contains(&key.as_str());
        let list: Vec<String> = style::with(&hidden, &key, on).into_iter().map(str::to_owned).collect();
        let show = checkbox(on).label(style::plain(&label)).text_size(t.font_small).on_toggle(move |_| p(json!({"hidden": list.clone()})));
        let own = shown["curves"][key.as_str()]["colour"].as_str().map(str::to_owned);
        let theirs = chip(t, "○", own.is_none(), p(json!({"curve": key, "colour": Value::Null})));
        let colours = swatches.iter().map(|c| {
            let chosen = own.as_deref() == Some(c.hex().as_str());
            let ink = look::rgb(*c);
            button(text(if chosen { "●" } else { "■" }).size(t.font_small).color(ink))
                .style(look::quiet(t))
                .on_press(p(json!({"curve": key, "colour": c.hex()})))
                .boxed()
        });
        let width = shown["curves"][key.as_str()]["width"].as_f64();
        let widths = WIDTHS.iter().map(|w| {
            let on = width == Some(f64::from(*w));
            chip(t, &w.to_string(), on, p(json!({"curve": key, "width": if on { Value::Null } else { json!(w) }})))
        });
        let line = row(std::iter::once(theirs).chain(colours).chain(widths)).spacing(2).align_y(Alignment::Center);
        Column::with_children([show.boxed(), line.boxed()]).spacing(2).boxed()
    });
    let body =
        Column::with_children([head.boxed(), title].into_iter().chain(axes).chain(std::iter::once(looks.boxed())).chain(adds).chain(rows)).spacing(6);
    Some(container(body).padding(10).width(560).style(look::ground(t)).boxed())
}

/// The card's handles: `chart:style` opens or closes it; while open `style:reset`,
/// `style:grid` and `style:legend:<id>`.
pub fn handles(app: &App) -> Vec<(String, Message)> {
    let Some(id) = style::chart_of(app.page).filter(|_| !app.settings && !app.home) else { return Vec::new() };
    let toggle = ("chart:style".to_owned(), send(StyleAct::Toggle));
    let open = (app.style.open == Some(id)).then(|| {
        let shown = emcraft_engine::views::shown_style(&app.session, id);
        let grid = shown["grid"].as_bool().unwrap_or(true);
        let p = move |more: Value| send(StyleAct::Send(style::params(id, more)));
        [("style:reset".to_owned(), p(json!({"reset": true}))), ("style:grid".to_owned(), p(json!({"grid": !grid})))]
            .into_iter()
            .chain(LEGENDS.iter().map(move |(at, _, _)| (format!("style:legend:{}", at.id()), p(json!({"legend": at.id()})))))
            .collect::<Vec<_>>()
    });
    std::iter::once(toggle).chain(open.into_iter().flatten()).collect()
}

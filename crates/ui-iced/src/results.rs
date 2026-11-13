//! A result page that is one of the engine's charts — TDR, the pulse, the far field, dispersion,
//! a study, the analyses read from a sweep ([`emcraft_ui_core::view::View::chart`]) — drawn by
//! the GPU ([`crate::charts`]), its words in the window's language, or why there is nothing to
//! show yet; with its page's choices over it — the TDR's port and transform, the far-field cut,
//! the pulse sent through and what is charted of it — and the dispersion diagram's table under
//! it. The choices are the shared rules' ([`emcraft_ui_core::results::views`]).

use emcraft_engine::tdr::Mode;
use emcraft_engine::view::{Cut, FarComponent, FarQuantity};
use emcraft_ui_core::analyses::{BUTTONS, summary};
use emcraft_ui_core::lang::{lookup, tr};
use emcraft_ui_core::results::views::{CHIRPS, PulseForm, TDR_MODES};
use emcraft_ui_core::view::View;
use iced::widget::{Column, button, canvas, pick_list, row, scrollable, slider, text, text_input};
use iced::{Alignment, Element, Fill, Widget};

use crate::app::{App, Message};
use crate::chart::Plot;
use crate::look;
use crate::parts::chip;

/// The chart's height (points).
const CHART_H: f32 = 520.0;

/// Result page `page`.
pub fn view(app: &App, page: View) -> Element<'_, Message> {
    let t = app.look.tokens;
    let picture: Element<Message> = match app.charts.page.as_ref().map(|p| &p.shown) {
        Some(Ok(c)) => canvas(Plot { chart: std::borrow::Cow::Borrowed(c), palette: &app.look.theme.palette }).width(Fill).height(CHART_H).boxed(),
        Some(Err(e)) => {
            let missing = page.missing(&app.session, &app.replies);
            crate::parts::empty(t, missing.map_or_else(|| e.clone(), |m| tr(m).to_owned()), None)
        }
        None => text("").boxed(),
    };
    // The analyses: a button each (its dialog), the chart of the last, a line on its numbers.
    let (above, below) = match page {
        View::Analyses => analyses(app),
        View::Tdr => (Some(tdr(app)), Some(note(app, t!("Round-trip time from the port's reference plane.")))),
        View::FarField => (Some(pattern(app)), None),
        View::Pulse => (Some(pulse(app)), None),
        View::Dispersion => (None, emcraft_ui_core::results::views::dispersion(&app.session).map(|tb| crate::table::view(t, &tb))),
        View::Study => (study_choices(app), emcraft_ui_core::results::study::points(&app.session, &app.study).map(|tb| crate::table::view(t, &tb))),
        _ => (None, None),
    };
    // The chart's style: its ⚙ beside the page's choices, its card under them.
    let above = match (above, crate::style_card::gear(app)) {
        (Some(a), Some(g)) => Some(row![a, g].spacing(t.space_8).align_y(Alignment::Start).boxed()),
        (a, g) => a.or(g),
    };
    let style = crate::style_card::card(app);
    scrollable(
        Column::with_children(above.into_iter().chain(style).chain(std::iter::once(picture)).chain(below)).spacing(t.space_8).padding(t.space_16),
    )
    .boxed()
}

/// The analyses' buttons, the one shown lit, and a line on its numbers.
fn analyses(app: &App) -> (Option<Element<'_, Message>>, Option<Element<'_, Message>>) {
    let t = app.look.tokens;
    let shown = app.replies.analysis.as_ref();
    let idle = app.job.is_none();
    let buttons = BUTTONS.iter().map(|(id, label, _)| {
        let on = shown.is_some_and(|a| a.command == *id);
        button(text(tr(*label)).size(t.font_button))
            .style(look::toggle(t, on))
            .on_press_maybe(idle.then(|| Message::Command((*id).to_owned())))
            .boxed()
    });
    let line = shown.and_then(|a| summary(&app.session, a)).map(|l| text(l).size(t.font_body).color(t.text).boxed());
    (Some(row(buttons).spacing(t.space_6).wrap().boxed()), line)
}

/// What a result page's choices ask for.
#[derive(Clone, Debug)]
pub enum RAct {
    TdrPort(String),
    TdrMode(Option<Mode>),
    Cut(Cut),
    Angle(f64),
    Quantity(FarQuantity),
    Component(FarComponent),
    /// The pattern's dB below its top, and whether it is drawn round a centre.
    Range(u8),
    Polar(bool),
    /// A pulse field typed, by name; its chirp, wave and what is charted; Run.
    Pulse(&'static str, String),
    Chirp(&'static str),
    Wave(usize),
    PulseShow(bool),
    PulseRun,
    /// The S-parameter a study's chart shows; a grid's variable charted along, and the value
    /// one is held at.
    StudyParameter(String),
    Along(usize),
    Held(usize, usize),
}

fn send(a: RAct) -> Message {
    Message::R(a)
}

impl App {
    /// What a result page's choice asks for.
    pub(crate) fn r_act(&mut self, a: RAct) {
        let bounded = emcraft_ui_core::dialogs::bounded;
        match a {
            RAct::TdrPort(p) => self.tdr.port = Some(p),
            RAct::TdrMode(m) => self.tdr.mode = m,
            RAct::Cut(c) => self.pattern = self.pattern.with_cut(c),
            RAct::Angle(a) => self.pattern.angle = a,
            RAct::Quantity(q) => self.pattern.quantity = q,
            RAct::Component(c) => self.pattern.component = c,
            RAct::Range(k) => {
                self.pattern.range = emcraft_engine::view::RANGES.get(usize::from(k)).copied().unwrap_or(emcraft_engine::view::RANGES[0])
            }
            RAct::Polar(on) => self.pattern.polar = on,
            RAct::Pulse(name, s) => match name {
                "frequency" => self.pulse.frequency = bounded(&s),
                "width" => self.pulse.width = bounded(&s),
                "length" => self.pulse.length = bounded(&s),
                "bandwidth" => self.pulse.bandwidth = bounded(&s),
                _ => {}
            },
            RAct::Chirp(c) => {
                self.pulse.chirp = Some(c.to_owned());
                if c != "linear" {
                    self.pulse.bandwidth.clear();
                }
            }
            RAct::Wave(w) => self.pulse.wave = Some(w),
            RAct::PulseShow(on) => self.pulse.show_frequency = on,
            RAct::PulseRun => self.start("results.pulse", self.pulse.params()),
            RAct::StudyParameter(p) => self.study.parameter = Some(p),
            RAct::Along(k) => self.study = std::mem::take(&mut self.study).along(k),
            RAct::Held(k, i) => self.study = std::mem::take(&mut self.study).held(k, i),
        }
    }

    /// A project with a pulse of its own (a template's, a file's): sent once when its page
    /// opens, its settings the form's.
    pub(crate) fn own_pulse(&mut self) {
        if self.page != View::Pulse || self.pulse.restored || self.replies.pulse.is_some() || self.job.is_some() {
            return;
        }
        if let Some(own) = self.session.views().pulse.clone() {
            self.pulse = PulseForm { restored: true, ..PulseForm::of(&own) };
            self.start("results.pulse", own);
        }
    }
}

fn note<'a>(app: &App, s: emcraft_ui_core::lang::Text) -> Element<'a, Message> {
    let t = app.look.tokens;
    text(tr(s)).size(t.font_small).color(t.text_dim).boxed()
}

/// The study in a line, the S-parameter a sweep shows, and a grid's slice: the variable charted
/// along, and the value each other is held at.
fn study_choices(app: &App) -> Option<Element<'_, Message>> {
    use emcraft_ui_core::results::study;
    let t = app.look.tokens;
    let s = app.session.results().study.as_ref()?;
    let line = text(study::summary(s)).size(t.font_body).color(t.text).boxed();
    let shown = app.study.shown(s);
    let list = study::parameters(s);
    let parameters = (list.len() > 1).then(|| {
        let chips =
            list.iter().map(|p| chip(t, format!("|{}|", p.to_uppercase()), shown.as_ref() == Some(p), Some(send(RAct::StudyParameter(p.clone())))));
        row(std::iter::once(text(tr(t!("Show"))).size(t.font_small).color(t.text_dim).boxed()).chain(chips))
            .spacing(t.space_6)
            .align_y(Alignment::Center)
            .wrap()
            .boxed()
    });
    let grid = s.is_grid().then(|| {
        let along = s.variables.iter().enumerate().map(|(k, n)| chip(t, n, app.study.slice.free == k, Some(send(RAct::Along(k)))));
        let along = row(std::iter::once(text(tr(t!("Along"))).size(t.font_small).color(t.text_dim).boxed()).chain(along))
            .spacing(t.space_6)
            .align_y(Alignment::Center)
            .wrap()
            .boxed();
        let held = study::held(s, &app.study.slice).into_iter().map(|(k, name, values, now)| {
            let label = emcraft_ui_core::lang::trf(t!("{name} at"), emcraft_ui_core::lang::args!(name = name));
            let chips = values.into_iter().enumerate().map(move |(i, v)| chip(t, &v, now == i, Some(send(RAct::Held(k, i)))));
            row(std::iter::once(text(label).size(t.font_small).color(t.text_dim).boxed()).chain(chips))
                .spacing(t.space_6)
                .align_y(Alignment::Center)
                .wrap()
                .boxed()
        });
        Column::with_children(std::iter::once(along).chain(held)).spacing(t.space_4).boxed()
    });
    Some(Column::with_children(std::iter::once(line).chain(parameters).chain(grid)).spacing(t.space_6).boxed())
}

/// The TDR's port and transform.
fn tdr(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let ports: Vec<String> = app.session.sparameter_entries().into_iter().map(|e| e.name).collect();
    let shown = app.tdr.port.clone().or_else(|| ports.first().cloned());
    let port = pick_list(shown, ports, String::clone).on_select(|p| send(RAct::TdrPort(p))).text_size(t.font_button).style(look::choice(t));
    let modes = TDR_MODES.iter().map(|(m, label, _)| chip(t, tr(*label), app.tdr.mode == *m, Some(send(RAct::TdrMode(*m)))));
    row(std::iter::once(text(tr(t!("port"))).size(t.font_body).boxed()).chain(std::iter::once(port.boxed())).chain(modes))
        .spacing(t.space_8)
        .align_y(Alignment::Center)
        .wrap()
        .boxed()
}

/// The dynamic ranges offered, by their place, smallest first.
fn ranges() -> impl Iterator<Item = (u8, f64)> {
    let mut all: Vec<(u8, f64)> = emcraft_engine::view::RANGES.iter().enumerate().map(|(k, r)| (k as u8, *r)).collect();
    all.sort_by(|a, b| a.1.total_cmp(&b.1));
    all.into_iter()
}

/// The far-field cut, its angle, what it measures and the polarization.
fn pattern(app: &App) -> Element<'_, Message> {
    let t = app.look.tokens;
    let p = app.pattern;
    let cuts = Cut::ALL.into_iter().map(|c| chip(t, lookup(c.label()), p.cut == c, Some(send(RAct::Cut(c)))));
    let (lo, hi, name) = p.range();
    let angle = slider(lo..=hi, p.angle.clamp(lo, hi), |a| send(RAct::Angle(a))).step(1.0).width(200);
    let said = text(format!("{name} {:.0}", p.angle)).size(t.font_body);
    let quantities = FarQuantity::ALL.into_iter().map(|q| chip(t, lookup(q.label()), p.quantity == q, Some(send(RAct::Quantity(q)))));
    let components = FarComponent::ALL.into_iter().map(|c| chip(t, lookup(c.label()), p.component == c, Some(send(RAct::Component(c)))));
    // How it is drawn: round a centre or along an axis, and the dB below its top.
    let polar = chip(t, tr(t!("Polar")), p.polar, Some(send(RAct::Polar(!p.polar))));
    let ranges = ranges().map(|(k, r)| chip(t, format!("{r:.0} dB"), p.range == r, Some(send(RAct::Range(k)))));
    row(cuts.chain([angle.boxed(), said.boxed()]).chain(quantities).chain(components).chain(std::iter::once(polar)).chain(ranges))
        .spacing(t.space_8)
        .align_y(Alignment::Center)
        .wrap()
        .vertical_spacing(t.space_6)
        .boxed()
}

/// A pulse field: its name, its text and its unit.
fn pulse_field<'a>(
    t: crate::look::Tokens,
    name: &'static str,
    label: emcraft_ui_core::lang::Text,
    value: &'a str,
    unit: &'a str,
) -> Element<'a, Message> {
    row![
        text(tr(label)).size(t.font_body),
        text_input("", value)
            .size(t.font_body)
            .width(80)
            .padding([t.space_4, t.space_6])
            .style(look::field(t))
            .on_input(move |s| send(RAct::Pulse(name, s))),
        text(unit).size(t.font_small).color(t.text_dim)
    ]
    .spacing(t.space_4)
    .align_y(Alignment::Center)
    .boxed()
}

/// The pulse: its carrier, width, a cell's length and wave, the chirp (and a linear one's band),
/// Run, and whether its power or its frequency is charted.
fn pulse<'a>(app: &'a App) -> Element<'a, Message> {
    let t = app.look.tokens;
    let f = &app.pulse;
    let (fu, lu) = (app.session.display().frequency.symbol(), app.session.display().length.symbol());
    let field = |name, label, value: &'a str, unit: &'a str| pulse_field(t, name, label, value, unit);
    let waves: Vec<String> = app.session.results().bloch.as_ref().map(|b| b.waves.iter().map(|w| w.name.clone()).collect()).unwrap_or_default();
    let bloch = !waves.is_empty();
    let cell = bloch.then(|| field("length", t!("length"), &f.length, lu));
    let wave_chips = (waves.len() > 1).then(|| {
        let now = f.wave.unwrap_or(1);
        waves.iter().enumerate().map(|(k, name)| chip(t, name, now == k + 1, Some(send(RAct::Wave(k + 1))))).collect::<Vec<_>>()
    });
    let chirps = CHIRPS.iter().map(|(id, label, _)| chip(t, tr(*label), f.chirp() == *id, Some(send(RAct::Chirp(id)))));
    let band = (f.chirp() == "linear").then(|| field("bandwidth", t!("bandwidth"), &f.bandwidth, fu));
    let run = button(text(tr(t!("Run"))).size(t.font_button))
        .style(look::button(t, true))
        .on_press_maybe(app.job.is_none().then_some(send(RAct::PulseRun)));
    let show = [(false, t!("Power")), (true, t!("Frequency"))]
        .map(|(on, label)| chip(t, tr(label), f.show_frequency == on, Some(send(RAct::PulseShow(on)))));
    let parts = [field("frequency", t!("carrier"), &f.frequency, fu), field("width", t!("width"), &f.width, "ns")]
        .into_iter()
        .chain(cell)
        .chain(wave_chips.into_iter().flatten())
        .chain(chirps)
        .chain(band)
        .chain(std::iter::once(run.boxed()))
        .chain(show);
    row(parts).spacing(t.space_8).align_y(Alignment::Center).wrap().vertical_spacing(t.space_6).boxed()
}

/// The result pages' choices by their handles, for scenarios and agents: `tdr:mode:<id>`
/// (`auto` the engine's pick), `pattern:cut:<id>`, `pattern:quantity:<id>`,
/// `pattern:component:<id>`, `pulse:chirp:<id>`, `pulse:show:power|frequency`, `pulse:run`.
pub fn handles(app: &App) -> Vec<(String, Message)> {
    if app.settings || app.home {
        return Vec::new();
    }
    match app.page {
        View::Tdr => TDR_MODES.iter().map(|(m, _, _)| (format!("tdr:mode:{}", m.map_or("auto", Mode::id)), send(RAct::TdrMode(*m)))).collect(),
        View::FarField => Cut::ALL
            .into_iter()
            .map(|c| (format!("pattern:cut:{}", c.id()), send(RAct::Cut(c))))
            .chain(FarQuantity::ALL.into_iter().map(|q| (format!("pattern:quantity:{}", q.id()), send(RAct::Quantity(q)))))
            .chain([("pattern:polar".to_owned(), send(RAct::Polar(!app.pattern.polar)))])
            .chain(ranges().map(|(k, r)| (format!("pattern:range:{r:.0}"), send(RAct::Range(k)))))
            .chain(FarComponent::ALL.into_iter().map(|c| (format!("pattern:component:{}", c.id()), send(RAct::Component(c)))))
            .collect(),
        View::Pulse => CHIRPS
            .iter()
            .map(|(id, _, _)| (format!("pulse:chirp:{id}"), send(RAct::Chirp(id))))
            .chain([("pulse:show:power".to_owned(), send(RAct::PulseShow(false))), ("pulse:show:frequency".to_owned(), send(RAct::PulseShow(true)))])
            .chain(std::iter::once(("pulse:run".to_owned(), send(RAct::PulseRun))))
            .collect(),
        View::Study => app.session.results().study.as_ref().map_or_else(Vec::new, |s| {
            use emcraft_ui_core::results::study;
            let parameters = study::parameters(s).into_iter().map(|p| (format!("study:parameter:{p}"), send(RAct::StudyParameter(p))));
            let along = s.variables.iter().enumerate().map(|(k, n)| (format!("study:along:{n}"), send(RAct::Along(k))));
            let held = study::held(s, &app.study.slice)
                .into_iter()
                .flat_map(|(k, n, values, _)| (0..values.len()).map(move |i| (format!("study:at:{n}:{i}"), send(RAct::Held(k, i)))));
            parameters.chain(along).chain(held).collect()
        }),
        _ => Vec::new(),
    }
}

/// The result pages' choices for `ui.inspect`, and whether the page's chart is drawn (or why
/// not).
pub fn inspect(app: &App) -> serde_json::Value {
    let p = app.pattern;
    serde_json::json!({
        "tdr": app.tdr.params(),
        "pattern": {"cut": p.cut.id(), "angle": p.angle, "quantity": p.quantity.id(), "component": p.component.id()},
        "pulse": {"params": app.pulse.params(), "show_frequency": app.pulse.show_frequency},
        "study": {
            "parameter": app.session.results().study.as_ref().and_then(|s| app.study.shown(s)),
            "along": app.study.slice.free,
            "at": app.study.slice.at,
            "points": emcraft_ui_core::results::study::points(&app.session, &app.study).map(|t| t.rows.len()),
        },
        "chart": app.charts.page.as_ref().map(|c| match &c.shown { Ok(chart) => serde_json::json!({"title": chart.title, "grid": chart.look.grid}), Err(e) => serde_json::json!({"error": e}) }),
        "style": app.style.open.map(emcraft_engine::views::ChartId::id),
    })
}

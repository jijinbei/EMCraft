//! How the project's results are looked at, saved with the project (docs/project-file.md,
//! `views`) and set by commands, never by the model's history: each chart's style — its axes'
//! ranges (automatic or fixed) and scales (linear or logarithmic), its title and axis labels,
//! the curves shown, each curve's colour and width, the grid and where the legend goes —
//! applied alike to the window's charts and the `view.*` PNGs ([`style_chart`]); and the named
//! field probes (`view.probe.*`, read by `results.probes`).
//!
//! A frequency axis keeps its fixed range in hertz, so it stays where it was when the display
//! units change; the other axes keep the numbers they show.

use std::collections::BTreeMap;

use emcraft_model::Point3;
use emcraft_render::Pen;
use emcraft_render::chart::{Chart, Legend};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::theme::Colour;
use crate::{EngineError, Params, Result, Session};

emcraft_macros::named_enum! {
    /// A chart a style belongs to (`chart`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum ChartId {
        SParameters = "sparameters" => "S-parameters",
        Tdr = "tdr" => "TDR",
        Pattern = "pattern" => "far field",
        Dispersion = "dispersion" => "dispersion",
        Study = "study" => "study",
    }
}

emcraft_macros::named_enum! {
    /// Where a chart's legend goes (`legend`).
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum LegendAt {
        #[default]
        Below = "below",
        TopLeft = "top-left",
        TopRight = "top-right",
        BottomLeft = "bottom-left",
        BottomRight = "bottom-right",
        None = "none",
    }
}

emcraft_macros::named_serde!(LegendAt);

impl LegendAt {
    fn legend(self) -> Legend {
        match self {
            LegendAt::Below => Legend::Below,
            LegendAt::TopLeft => Legend::TopLeft,
            LegendAt::TopRight => Legend::TopRight,
            LegendAt::BottomLeft => Legend::BottomLeft,
            LegendAt::BottomRight => Legend::BottomRight,
            LegendAt::None => Legend::None,
        }
    }
}

emcraft_macros::named_enum! {
    /// An axis' scale (`x_scale`, `y_scale`).
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum Scale {
        #[default]
        Linear = "linear",
        Log = "log",
    }
}

emcraft_macros::named_enum! {
    /// One of a chart's axes.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum AxisId {
        X = "x",
        Y = "y",
    }
}

/// The most curves a style names, curves hidden, probes kept; the longest text kept.
pub const MAX_CURVES: usize = 64;
pub const MAX_PROBES: usize = 64;
const MAX_TEXT: usize = 200;
const MAX_NAME: usize = 64;

/// The thinnest and the widest a curve is drawn (pixels).
pub const WIDTHS: std::ops::RangeInclusive<f64> = 0.5..=12.0;

fn is_false(b: &bool) -> bool {
    !*b
}

/// An axis' style: its label, its range's ends (each automatic when `None`), its scale.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AxisStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub log: bool,
}

impl AxisStyle {
    fn is_auto(&self) -> bool {
        *self == AxisStyle::default()
    }
}

/// A curve's own colour and width (each the chart's usual when `None`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CurveStyle {
    /// `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
}

/// A chart's style; the default changes nothing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChartStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "AxisStyle::is_auto")]
    pub x: AxisStyle,
    #[serde(default, skip_serializing_if = "AxisStyle::is_auto")]
    pub y: AxisStyle,
    /// The keys of the curves left out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<String>,
    /// The keys of the curves added to the data ([`optional_curves`]: a closed form to compare
    /// with); none by default, a chart shows what was solved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<String>,
    /// Curves' own colours and widths, by key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub curves: BTreeMap<String, CurveStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legend: Option<LegendAt>,
}

impl ChartStyle {
    pub fn axis(&self, a: AxisId) -> &AxisStyle {
        match a {
            AxisId::X => &self.x,
            AxisId::Y => &self.y,
        }
    }

    fn axis_mut(&mut self, a: AxisId) -> &mut AxisStyle {
        match a {
            AxisId::X => &mut self.x,
            AxisId::Y => &mut self.y,
        }
    }

    /// The style as read from a file, checked: its texts and lists within their caps, its
    /// numbers finite, its colours and widths valid.
    fn checked(self, what: &str) -> Result<ChartStyle> {
        let bad = |why: String| EngineError::failed(format!("{what}: {why}"));
        let long = |t: &Option<String>| t.as_ref().is_some_and(|t| t.chars().count() > MAX_TEXT);
        if long(&self.title) || long(&self.x.label) || long(&self.y.label) {
            return Err(bad(format!("a title or label longer than {MAX_TEXT} characters")));
        }
        for a in [&self.x, &self.y] {
            if [a.min, a.max].iter().flatten().any(|v| !v.is_finite()) {
                return Err(bad("an axis' end is not a finite number".into()));
            }
        }
        if self.hidden.len() > MAX_CURVES || self.curves.len() > MAX_CURVES {
            return Err(bad(format!("more than {MAX_CURVES} curves")));
        }
        if self.hidden.iter().chain(self.curves.keys()).any(|k| k.chars().count() > MAX_NAME) {
            return Err(bad("a curve's key is too long".into()));
        }
        for c in self.curves.values() {
            if c.colour.as_deref().is_some_and(|c| Colour::parse(c).is_none()) {
                return Err(bad("a colour is not #rrggbb".into()));
            }
            if c.width.is_some_and(|w| !WIDTHS.contains(&w)) {
                return Err(bad(format!("a width outside {} to {} pixels", WIDTHS.start(), WIDTHS.end())));
            }
        }
        Ok(self)
    }
}

/// A named point where the fields are read (`view.probe.add`), in metres.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Probe {
    pub name: String,
    pub at: [f64; 3],
}

impl Probe {
    pub fn point(&self) -> Point3 {
        Point3::new(self.at[0], self.at[1], self.at[2])
    }
}

/// How the project's results are looked at: the charts' styles by chart id, the probes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Views {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub charts: BTreeMap<String, ChartStyle>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub probes: Vec<Probe>,
}

impl Views {
    /// The style of chart `id` (the default when it has none).
    pub fn style(&self, id: ChartId) -> ChartStyle {
        self.charts.get(id.id()).cloned().unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.charts.is_empty() && self.probes.is_empty()
    }

    /// The views as read from a project file, checked: charts the engine knows, styles within
    /// their caps, probes named once each with finite points.
    pub fn checked(self) -> Result<Views> {
        let mut charts = BTreeMap::new();
        for (id, style) in self.charts {
            if ChartId::parse(&id).is_none() {
                return Err(EngineError::failed(crate::text::unknown(
                    format_args!("views: no chart `{}`", crate::text::clip(&id, 40)),
                    "known: ",
                    ChartId::IDS,
                )));
            }
            let style = style.checked(&format!("views: chart `{id}`"))?;
            charts.insert(id, style);
        }
        if self.probes.len() > MAX_PROBES {
            return Err(EngineError::failed(format!("views: more than {MAX_PROBES} probes")));
        }
        for (i, p) in self.probes.iter().enumerate() {
            if p.name.trim().is_empty() || p.name.chars().count() > MAX_NAME || !p.at.iter().all(|v| v.is_finite()) {
                return Err(EngineError::failed(format!("views: probe {} needs a name and a finite point", i + 1)));
            }
            if self.probes[..i].iter().any(|q| q.name == p.name) {
                return Err(EngineError::failed(format!("views: two probes named `{}`", crate::text::clip(&p.name, 40))));
            }
        }
        Ok(Views { charts, probes: self.probes })
    }
}

/// Whether an axis of chart `id` is a frequency (its fixed ends kept in hertz).
fn is_frequency(id: ChartId, axis: AxisId) -> bool {
    matches!((id, axis), (ChartId::SParameters, AxisId::X) | (ChartId::Dispersion, AxisId::Y))
}

/// `chart` in the project's style for chart `id`: its title and labels, its axes' ranges and
/// scales, the curves left out, the curves' own colours and widths, the grid and the legend.
/// A logarithmic axis over a range through zero takes the positive points' range instead.
///
/// ```
/// use emcraft_engine::views::{ChartId, style_chart};
/// use emcraft_render::chart::{Chart, Series, Style};
/// use emcraft_render::{Axis, Pen};
///
/// let mut s = emcraft_engine::Session::new();
/// let chart = || {
///     let a = Series::new("a", "a", Pen::Series(0), Style::Line, vec![[1.0, 1.0], [2.0, 4.0]]);
///     let b = Series::new("b", "b", Pen::Series(1), Style::Line, vec![[1.0, 2.0], [2.0, 3.0]]);
///     Chart::new("t", Axis::new("f (GHz)", 1.0, 2.0), Axis::new("y", 0.0, 5.0), vec![a, b])
/// };
/// // No style: the chart as it was.
/// assert_eq!(style_chart(&s, ChartId::Study, chart()), chart());
/// let p = serde_json::json!({"chart": "study", "title": "Mine", "y_max": 10, "y_scale": "log", "hidden": ["b"], "curve": "a", "colour": "#00a000", "legend": "top-right"});
/// s.execute("view.chart.style", &p).unwrap();
/// let styled = style_chart(&s, ChartId::Study, chart());
/// assert_eq!((styled.title.as_str(), styled.y.max, styled.y.log), ("Mine", 10.0, true));
/// assert_eq!(styled.series.len(), 1);
/// assert_eq!(styled.series[0].pen, Pen::Custom(emcraft_render::Rgb(0, 160, 0)));
/// // Bad input is refused, the style left as it was.
/// assert!(s.execute("view.chart.style", &serde_json::json!({"chart": "study", "width": 99, "curve": "a"})).is_err());
/// assert!(s.execute("view.chart.style", &serde_json::json!({"chart": "nope"})).is_err());
/// ```
/// The curves chart `id` can add to its data ([`ChartStyle::added`]), each with its key and
/// label: for the S-parameters, the closed form or mode matching the model's guide has
/// (`view::reference`), none otherwise.
pub fn optional_curves(s: &Session, id: ChartId) -> Vec<(String, String)> {
    match id {
        ChartId::SParameters => match crate::view::reference(s) {
            Ok(r) => {
                let label = r.label();
                vec![("s21.ref".to_owned(), format!("|S21| {label}")), ("s11.ref".to_owned(), format!("|S11| {label}"))]
            }
            Err(_) => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Whether chart `id`'s style adds curve `key`.
pub fn added(s: &Session, id: ChartId, key: &str) -> bool {
    s.views.charts.get(id.id()).is_some_and(|st| st.added.iter().any(|k| k == key))
}

pub fn style_chart(s: &Session, id: ChartId, mut chart: Chart) -> Chart {
    let Some(style) = s.views.charts.get(id.id()) else { return chart };
    if let Some(t) = style.title.as_ref().filter(|t| !t.is_empty()) {
        chart.title = t.clone();
    }
    let fu = s.display.frequency;
    for a in AxisId::ALL {
        let st = style.axis(a);
        let shown = |v: f64| if is_frequency(id, a) { fu.from_si(v) } else { v };
        let data: Vec<f64> = chart
            .series
            .iter()
            .filter(|c| !style.hidden.contains(&c.key))
            .flat_map(|c| c.points.iter().map(move |p| if a == AxisId::X { p[0] } else { p[1] }))
            .collect();
        let axis = match a {
            AxisId::X => &mut chart.x,
            AxisId::Y => &mut chart.y,
        };
        if let Some(l) = st.label.as_ref().filter(|l| !l.is_empty()) {
            axis.label = l.clone();
        }
        axis.log = st.log;
        if st.log && axis.min.min(axis.max) <= 0.0 {
            // The positive points' range, out to whole decades.
            let (lo, hi) = crate::view::chart::span(data.into_iter().filter(|v| *v > 0.0 && v.is_finite()));
            if lo.is_finite() && hi.is_finite() {
                (axis.min, axis.max) = (10f64.powf(lo.log10().floor()), 10f64.powf(hi.log10().ceil()).max(lo * 10.0));
            }
        }
        if let Some(v) = st.min {
            axis.min = shown(v);
        }
        if let Some(v) = st.max {
            axis.max = shown(v);
        }
    }
    chart.series.retain(|c| !style.hidden.contains(&c.key));
    for c in &mut chart.series {
        if let Some(cs) = style.curves.get(&c.key) {
            if let Some(colour) = cs.colour.as_deref().and_then(Colour::parse) {
                c.pen = Pen::Custom(colour.rgb());
            }
            if cs.width.is_some() {
                c.width = cs.width;
            }
        }
    }
    if let Some(g) = style.grid {
        chart.look.grid = g;
    }
    if let Some(l) = style.legend {
        chart.look.legend = l.legend();
    }
    chart
}

/// Chart `id` as the session draws it with nothing chosen (the S-parameters in dB, the first
/// port's TDR, the far field's φ = 0 cut, the study's first slice), unstyled.
pub fn chart_of(s: &Session, id: ChartId) -> Result<Chart> {
    match id {
        ChartId::SParameters => crate::view::sparameter_chart(s),
        ChartId::Tdr => crate::results_tdr_chart(s, &Params::new(&json!({}), s.display)),
        ChartId::Pattern => crate::view::pattern_chart(s, 0.0, 91),
        ChartId::Dispersion => crate::view::dispersion_chart(s),
        ChartId::Study => {
            let study = s.results.study.as_ref().ok_or(EngineError::NotSolved { what: "study", run: "study.sweep or study.optimize" })?;
            let slice = crate::view::study::Slice::named(study, None)?;
            crate::view::chart_with_units(study, None, &slice, &crate::view::variable_units(s, study))
        }
    }
}

/// A style as a command's reply gives it, the fixed ends of a frequency axis in display units.
fn shown_of(s: &Session, id: ChartId, style: &ChartStyle) -> Value {
    let mut v = serde_json::to_value(style).unwrap_or_else(|_| json!({}));
    for a in AxisId::ALL.into_iter().filter(|a| is_frequency(id, *a)) {
        for end in ["min", "max"] {
            if let Some(x) = v[a.id()][end].as_f64() {
                v[a.id()][end] = json!(s.show_frequency(x));
            }
        }
    }
    v
}

/// Chart `id`'s style as `view.chart.style` replies it: the fixed ends of a frequency axis in
/// display units.
pub fn shown_style(s: &Session, id: ChartId) -> Value {
    shown_of(s, id, &s.views.style(id))
}

/// `view.chart.style`: changes what is given of a chart's style (all of it back to automatic
/// first with `reset`); the style and the chart's curves (when it can be drawn now).
pub fn chart_style(s: &mut Session, p: &Params) -> Result<Value> {
    let id: ChartId = p.pick("chart").required()?;
    let mut style = if p.flag("reset").or(false)? { ChartStyle::default() } else { s.views.style(id) };
    let text = |name: &str| -> Result<Option<Option<String>>> {
        match p.string(name).optional()? {
            Some(t) if t.chars().count() > MAX_TEXT => Err(p.error(name, format!("is longer than {MAX_TEXT} characters"))),
            Some(t) => Ok(Some((!t.trim().is_empty()).then(|| t.to_owned()))),
            None => Ok(p.cleared(name).then_some(None)),
        }
    };
    if let Some(t) = text("title")? {
        style.title = t;
    }
    for a in AxisId::ALL {
        if let Some(t) = text(&format!("{a}_label"))? {
            style.axis_mut(a).label = t;
        }
        for (end, which) in [("min", 0), ("max", 1)] {
            let name = format!("{a}_{end}");
            let value = if p.cleared(&name) {
                Some(None)
            } else if is_frequency(id, a) {
                p.number(&name).optional()?.map(|v| Some(s.display.frequency.to_si(v)))
            } else {
                p.number(&name).optional()?.map(Some)
            };
            if let Some(v) = value {
                let axis = style.axis_mut(a);
                if which == 0 { axis.min = v } else { axis.max = v }
            }
        }
        if let (Some(lo), Some(hi)) = (style.axis(a).min, style.axis(a).max)
            && lo >= hi
        {
            return Err(p.error(&format!("{a}_max"), "must be above the axis' min"));
        }
        if let Some(scale) = p.pick::<Scale>(&format!("{a}_scale")).optional()? {
            style.axis_mut(a).log = scale == Scale::Log;
        }
        if style.axis(a).log && [style.axis(a).min, style.axis(a).max].iter().flatten().any(|v| *v <= 0.0) {
            return Err(p.error(&format!("{a}_scale"), "log needs the axis' fixed ends above 0"));
        }
    }
    if let Some(list) = p.list("added", MAX_CURVES).optional()? {
        let known: Vec<String> = optional_curves(s, id).into_iter().map(|(k, _)| k).collect();
        style.added = list
            .iter()
            .map(|k| {
                k.as_str()
                    .filter(|k| known.iter().any(|q| q == k))
                    .map(str::to_owned)
                    .ok_or_else(|| p.error("added", format!("holds the keys of curves this chart can add: {}", known.join(", "))))
            })
            .collect::<Result<Vec<String>>>()?;
    }
    if let Some(list) = p.list("hidden", MAX_CURVES).optional()? {
        style.hidden = list
            .iter()
            .map(|k| k.as_str().filter(|k| k.chars().count() <= MAX_NAME).map(str::to_owned).ok_or_else(|| p.error("hidden", "holds curves' keys")))
            .collect::<Result<Vec<String>>>()?;
    }
    if let Some(g) = p.flag("grid").optional()? {
        style.grid = Some(g);
    }
    if let Some(l) = p.pick::<LegendAt>("legend").optional()? {
        style.legend = Some(l);
    }
    let chart = chart_of(s, id).ok();
    let curve = p.string("curve").optional()?;
    if (p.has("colour") || p.cleared("colour") || p.has("width") || p.cleared("width")) && curve.is_none() {
        return Err(p.error("curve", "is needed with `colour` or `width`: the curve's key"));
    }
    if let Some(key) = curve {
        if key.chars().count() > MAX_NAME {
            return Err(p.error("curve", "is too long"));
        }
        if let Some(c) = &chart
            && !c.series.iter().any(|x| x.key == key)
        {
            let what = format_args!("names no curve of the chart: `{}`", crate::text::clip(key, 40));
            return Err(p.error("curve", crate::text::unknown(what, "curves: ", c.curves().into_iter().map(|(k, _)| k))));
        }
        let mut cs = style.curves.get(key).cloned().unwrap_or_default();
        if p.cleared("colour") {
            cs.colour = None;
        } else if let Some(c) = p.string("colour").optional()? {
            let colour = Colour::parse(c).filter(|c| c.a == 255).ok_or_else(|| p.error("colour", "must be #rrggbb"))?;
            cs.colour = Some(colour.hex());
        }
        if p.cleared("width") {
            cs.width = None;
        } else if let Some(w) = p.number("width").optional()? {
            if !WIDTHS.contains(&w) {
                return Err(p.error("width", format!("must be {} to {} pixels", WIDTHS.start(), WIDTHS.end())));
            }
            cs.width = Some(w);
        }
        if cs == CurveStyle::default() {
            style.curves.remove(key);
        } else if style.curves.len() >= MAX_CURVES && !style.curves.contains_key(key) {
            return Err(p.error("curve", format!("at most {MAX_CURVES} curves have styles of their own")));
        } else {
            style.curves.insert(key.to_owned(), cs);
        }
    }
    if style == ChartStyle::default() {
        s.views.charts.remove(id.id());
    } else {
        s.views.charts.insert(id.id().to_owned(), style.clone());
    }
    let mut reply = json!({"chart": id.id(), "style": shown_of(s, id, &style)});
    if let Some(c) = chart {
        reply["curves"] = json!(c.curves().iter().map(|(k, l)| json!({"key": k, "label": l})).collect::<Vec<_>>());
    }
    Ok(reply)
}

/// A probe's name: `name`, or the first free `P1`, `P2`, …
fn probe_name(s: &Session, p: &Params, name: &str) -> Result<String> {
    match p.string(name).optional()? {
        Some(n) if n.trim().is_empty() || n.chars().count() > MAX_NAME => Err(p.error(name, format!("must be 1 to {MAX_NAME} characters"))),
        Some(n) if s.views.probes.iter().any(|q| q.name == n) => {
            Err(p.error(name, format!("`{}` names another probe already", crate::text::clip(n, 40))))
        }
        Some(n) => Ok(n.to_owned()),
        None => Ok((1..=MAX_PROBES + 1).map(|k| format!("P{k}")).find(|n| !s.views.probes.iter().any(|q| &q.name == n)).unwrap_or_default()),
    }
}

/// A probe's point: `at`, which must lie in the model's bounds.
fn probe_point(s: &Session, p: &Params) -> Result<[f64; 3]> {
    let at = p.point("at").required()?;
    let b = s.model.bounds().ok_or_else(|| EngineError::failed("no geometry to probe"))?;
    let slack = 1e-9 * (b.max - b.min).to_array().iter().fold(1.0f64, |m, v| m.max(v.abs()));
    let inside = (0..3).all(|k| at.to_array()[k] >= b.min.to_array()[k] - slack && at.to_array()[k] <= b.max.to_array()[k] + slack);
    if !inside {
        return Err(p.error("at", "lies outside the model"));
    }
    Ok(at.to_array())
}

/// The probes as a reply lists them: name and point in display units.
fn probes_reply(s: &Session) -> Value {
    let l = s.display.length;
    json!({"probes": s.views.probes.iter().map(|q| json!({"name": q.name, "at": q.at.map(|v| l.from_si(v))})).collect::<Vec<_>>()})
}

/// The index of the probe `name` names.
fn probe_named(s: &Session, p: &Params) -> Result<usize> {
    let name = p.string("name").required()?;
    s.views.probes.iter().position(|q| q.name == name).ok_or_else(|| {
        p.error(
            "name",
            crate::text::unknown(
                format_args!("names no probe: `{}`", crate::text::clip(name, 40)),
                "probes: ",
                s.views.probes.iter().map(|q| q.name.as_str()),
            ),
        )
    })
}

/// `view.probe.add`.
pub fn probe_add(s: &mut Session, p: &Params) -> Result<Value> {
    if s.views.probes.len() >= MAX_PROBES {
        return Err(EngineError::failed(format!("at most {MAX_PROBES} probes")));
    }
    let name = probe_name(s, p, "name")?;
    let at = probe_point(s, p)?;
    s.views.probes.push(Probe { name: name.clone(), at });
    let mut reply = probes_reply(s);
    reply["name"] = json!(name);
    Ok(reply)
}

/// `view.probe.move`: a probe to another point, or renamed.
pub fn probe_move(s: &mut Session, p: &Params) -> Result<Value> {
    let i = probe_named(s, p)?;
    let at = if p.has("at") { Some(probe_point(s, p)?) } else { None };
    let rename = if p.has("rename") { Some(probe_name(s, p, "rename")?) } else { None };
    let probe = s.views.probes.get_mut(i).ok_or_else(|| EngineError::failed("no such probe"))?;
    if let Some(at) = at {
        probe.at = at;
    }
    if let Some(n) = rename {
        probe.name = n;
    }
    Ok(probes_reply(s))
}

/// `view.probe.delete`.
pub fn probe_delete(s: &mut Session, p: &Params) -> Result<Value> {
    let i = probe_named(s, p)?;
    s.views.probes.remove(i);
    Ok(probes_reply(s))
}

/// `results.probes`: E and H at each probe for a solution (a frequency and excitation, or an
/// eigenmode), with their magnitudes.
pub fn probes(s: &mut Session, p: &Params) -> Result<Value> {
    let mut view = crate::view::FieldView::new(0.0, crate::view::Plane::H);
    match p.integer("mode", 1..=crate::EIGEN_MAX).optional()? {
        Some(m) => view.mode = Some(m - 1),
        None => {
            view.frequency = p
                .frequency("frequency")
                .or_maybe(s.results.sweep.as_ref().and_then(|sw| sw.results.first()).map(|r| r.frequency))
                .required_or("or `mode`")?
        }
    }
    view.excitation = crate::args::excitation(s, p)?;
    let points: Vec<Point3> = s.views.probes.iter().map(Probe::point).collect();
    let (f, values) = crate::view::fields_at(s, &view, &points)?;
    let l = s.display.length;
    let parts = |v: &emcraft_model::CVec3| v.iter().map(|c| json!([c.re, c.im])).collect::<Vec<_>>();
    let rows: Vec<Value> = s
        .views
        .probes
        .iter()
        .zip(&values)
        .map(|(q, v)| {
            let mut row = json!({"name": q.name, "at": q.at.map(|x| l.from_si(x))});
            match v {
                Some((e, h)) => {
                    row["e"] = json!(parts(e));
                    row["h"] = json!(parts(h));
                    let norm = |v: &emcraft_model::CVec3| v.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
                    row["e_magnitude"] = json!(norm(e));
                    row["h_magnitude"] = json!(norm(h));
                }
                None => row["outside"] = json!(true),
            }
            row
        })
        .collect();
    Ok(json!({"frequency": s.show_frequency(f), "probes": rows}))
}

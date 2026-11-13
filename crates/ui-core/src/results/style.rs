//! A result chart's style, edited in place as every window shows it: a card of its title, axes
//! (label, ends — empty for automatic —, log scale), grid, legend, the curves it may add (a
//! closed form to compare with) and its curves (shown, colour, width). Every change is one
//! `view.chart.style` command, so it is saved with the project and the PNG exports follow it.

use emcraft_engine::views::{AxisId, ChartId, LegendAt};
use serde_json::{Map, Value, json};

use crate::lang::Text;

/// The widths a curve may take (pixels); the usual one is the theme's.
pub const WIDTHS: [u8; 4] = [1, 2, 3, 5];

/// The legend's places as the card offers them: an arrow toward the corner each names.
pub const LEGENDS: [(LegendAt, &str, Text); 6] = [
    (LegendAt::Below, "↓", t!("legend below the chart")),
    (LegendAt::TopLeft, "↖", t!("legend top left")),
    (LegendAt::TopRight, "↗", t!("legend top right")),
    (LegendAt::BottomLeft, "↙", t!("legend bottom left")),
    (LegendAt::BottomRight, "↘", t!("legend bottom right")),
    (LegendAt::None, "–", t!("no legend")),
];

/// An axis' label, low end and high end as typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AxisText {
    pub label: String,
    pub min: String,
    pub max: String,
}

/// The card: which chart's is open, its text fields as typed, and the style (as
/// `view.chart.style` shows it) they were filled from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleForm {
    pub open: Option<ChartId>,
    pub title: String,
    pub axes: [AxisText; 2],
    pub seen: Option<Value>,
}

/// A field of the card by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Title,
    Label(usize),
    Min(usize),
    Max(usize),
}

/// A style value as the card's text: a string, a number in short, else empty.
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map_or_default(emcraft_render::axes::format),
        _ => String::new(),
    }
}

impl StyleForm {
    /// The card for `id` opened (or, when it is open, closed), its fields filled afresh.
    pub fn toggled(self, id: ChartId) -> StyleForm {
        StyleForm { open: (self.open != Some(id)).then_some(id), ..StyleForm::default() }
    }

    /// The fields filled from the style `shown` (as `view.chart.style` shows it), unless they
    /// were filled from it already.
    pub fn synced(self, shown: &Value) -> StyleForm {
        if self.seen.as_ref() == Some(shown) {
            return self;
        }
        let axes = AxisId::ALL.map(|a| {
            let axis = &shown[a.id()];
            AxisText { label: text(&axis["label"]), min: text(&axis["min"]), max: text(&axis["max"]) }
        });
        StyleForm { title: text(&shown["title"]), axes, seen: Some(shown.clone()), ..self }
    }

    /// Field `f`'s text.
    pub fn text(&self, f: Field) -> &str {
        match f {
            Field::Title => &self.title,
            Field::Label(k) => self.axes.get(k).map_or("", |a| a.label.as_str()),
            Field::Min(k) => self.axes.get(k).map_or("", |a| a.min.as_str()),
            Field::Max(k) => self.axes.get(k).map_or("", |a| a.max.as_str()),
        }
    }

    /// The card with field `f` typed as `s`.
    pub fn typed(self, f: Field, s: &str) -> StyleForm {
        let mut form = self;
        let s = crate::dialogs::bounded(s);
        let slot = match f {
            Field::Title => Some(&mut form.title),
            Field::Label(k) => form.axes.get_mut(k).map(|a| &mut a.label),
            Field::Min(k) => form.axes.get_mut(k).map(|a| &mut a.min),
            Field::Max(k) => form.axes.get_mut(k).map(|a| &mut a.max),
        };
        if let Some(slot) = slot {
            *slot = s;
        }
        form
    }

    /// `view.chart.style`'s parameters committing field `f` as typed, when it changes the style
    /// `shown` (an axis end that reads as no number commits nothing).
    pub fn commit(&self, f: Field, shown: &Value) -> Option<Value> {
        let id = self.open?;
        let axis = |k: usize| AxisId::ALL.get(k).map(|a| a.id());
        let (key, value, now) = match f {
            Field::Title => ("title".to_owned(), json!(self.title.trim()), shown["title"].clone()),
            Field::Label(k) => (format!("{}_label", axis(k)?), json!(self.text(f).trim()), shown[axis(k)?]["label"].clone()),
            Field::Min(k) => (format!("{}_min", axis(k)?), end(self.text(f))?, shown[axis(k)?]["min"].clone()),
            Field::Max(k) => (format!("{}_max", axis(k)?), end(self.text(f))?, shown[axis(k)?]["max"].clone()),
        };
        let unchanged = value == now || (value == json!("") && now.is_null());
        (!unchanged).then(|| params(id, json!({key: value})))
    }
}

/// A field's text as an axis end: a number, or null (automatic) when empty; `None` when it reads
/// as neither.
///
/// ```
/// use emcraft_ui_core::results::style::end;
/// assert_eq!((end(" 3.5 "), end(""), end("abc")), (Some(serde_json::json!(3.5)), Some(serde_json::Value::Null), None));
/// ```
pub fn end(text: &str) -> Option<Value> {
    let t = text.trim();
    if t.is_empty() {
        return Some(Value::Null);
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| json!(v))
}

/// `view.chart.style`'s parameters for chart `id` with `more` added.
pub fn params(id: ChartId, more: Value) -> Value {
    let mut p = Map::new();
    p.insert("chart".into(), json!(id.id()));
    if let Value::Object(o) = more {
        p.extend(o);
    }
    Value::Object(p)
}

/// The style's list `key` (curves hidden, curves added) as shown.
pub fn names<'a>(shown: &'a Value, key: &str) -> Vec<&'a str> {
    shown[key].as_array().into_iter().flatten().filter_map(Value::as_str).collect()
}

/// `list` with `key` in it (`on`) or out of it.
pub fn with<'a>(list: &[&'a str], key: &'a str, on: bool) -> Vec<&'a str> {
    list.iter().copied().filter(|k| *k != key).chain(on.then_some(key)).collect()
}

/// A curve's label as plain text: TeX's marks taken out (`$|S_{21}|$` → `|S21|`).
pub fn plain(label: &str) -> String {
    label.chars().filter(|c| !matches!(c, '$' | '{' | '}' | '_' | '^')).collect()
}

/// The chart a result page's style card edits.
pub fn chart_of(page: crate::view::View) -> Option<ChartId> {
    use crate::view::View;
    match page {
        View::SParameters => Some(ChartId::SParameters),
        View::Tdr => Some(ChartId::Tdr),
        View::FarField => Some(ChartId::Pattern),
        View::Dispersion => Some(ChartId::Dispersion),
        View::Study => Some(ChartId::Study),
        _ => None,
    }
}

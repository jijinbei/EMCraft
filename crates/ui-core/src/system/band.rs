//! A system's band as its bar edits it: the band confirmed (or read), one of its fields typed
//! anew, and the settings `project.setup` confirms from them.

use serde_json::{Map, Value, json};

/// The ratio of the band's stop to its start when only one end is given.
const ONE_END_SPAN: f64 = 1.2;

/// The frequency points a band takes when none is known yet.
const DEFAULT_POINTS: f64 = 101.0;

/// The band's fields in display units, each known or not: its start and stop, its points and
/// the frequency its budget is shown at.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Band {
    pub start: Option<f64>,
    pub stop: Option<f64>,
    pub points: Option<f64>,
    pub at: Option<f64>,
}

/// Its field `key` (`start`, `stop`, `points`, `frequency`).
pub fn field(band: Band, key: &str) -> Option<f64> {
    match key {
        "start" => band.start,
        "stop" => band.stop,
        "points" => band.points,
        _ => band.at,
    }
}

/// `band` with its field `key` typed as `value` (none: emptied).
fn edited(band: Band, key: &str, value: Option<f64>) -> Band {
    match key {
        "start" => Band { start: value, ..band },
        "stop" => Band { stop: value, ..band },
        "points" => Band { points: value.map(f64::round), ..band },
        _ => Band { at: value, ..band },
    }
}

/// The settings `project.setup` confirms once `key` is typed as `value`: both ends (one given
/// alone, the other a fifth off it), the points and, when known, the budget's frequency; none
/// without either end.
///
/// ```
/// use emcraft_ui_core::system::band::{Band, setup};
/// let s = setup(Band::default(), "start", Some(1.0)).unwrap();
/// assert_eq!((s["start"].as_f64(), s["stop"].as_f64(), s["points"].as_u64()), (Some(1.0), Some(1.2), Some(101)));
/// assert!(s.get("frequency").is_none());
/// assert_eq!(setup(Band::default(), "points", Some(11.0)), None, "no band yet");
/// ```
pub fn setup(band: Band, key: &str, value: Option<f64>) -> Option<Value> {
    let b = edited(band, key, value);
    let (start, stop) = match (b.start, b.stop) {
        (Some(a), Some(z)) => (a, z),
        (Some(a), None) => (a, a * ONE_END_SPAN),
        (None, Some(z)) => (z / ONE_END_SPAN, z),
        (None, None) => return None,
    };
    let points = b.points.unwrap_or(DEFAULT_POINTS).max(1.0) as u64;
    let fields = [("kind", json!("system")), ("start", json!(start)), ("stop", json!(stop)), ("points", json!(points))];
    let at = b.at.map(|f| ("frequency", json!(f)));
    Some(Value::Object(fields.into_iter().chain(at).map(|(k, v)| (k.to_owned(), v)).collect::<Map<_, _>>()))
}

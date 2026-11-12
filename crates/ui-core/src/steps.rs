//! The workflow's steps as the window shows them.

/// A port mode picked in the Excitation panel: the Port modes view marks its row and scrolls
/// to it once.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModeFocus {
    pub port: String,
    pub mode: usize,
    pub scroll: bool,
}

/// Two settings alike, numbers to a part in 10⁹ (a unit's round trip).
///
/// ```
/// use serde_json::json;
/// assert!(emcraft_ui_core::steps::same(&json!({"stop": 1.0}), &json!({"stop": 1.0000000001})));
/// assert!(!emcraft_ui_core::steps::same(&json!({"stop": 1.0}), &json!({"stop": 1.1})));
/// ```
pub fn same(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= 1e-9 * x.abs().max(y.abs()),
            _ => x == y,
        },
        _ => a == b,
    }
}

//! What a scenario checks, whatever window it drives: a value at a dotted path of a request's
//! reply (`ui.inspect`'s, a command's) and how it compares with what is wanted — the same, any
//! (`"*"`), a number within a relative tolerance (`[value, tolerance]`), within bounds
//! (`{"min", "max"}`), or a text that includes or excludes another.
//!
//! ```
//! use emcraft_ui_core::scenario::{at_path, compare};
//! use serde_json::json;
//! let v = json!({"view3d": {"yaw": 90.0000001}, "rows": ["a", "b"]});
//! assert_eq!(at_path(&v, "rows.1"), Some(&json!("b")));
//! assert!(compare(&v["view3d"]["yaw"], &json!([90, 1e-6])).is_ok());
//! assert!(compare(&json!("solved in 2 s"), &json!({"includes": "solved"})).is_ok());
//! assert!(compare(&json!(3), &json!({"max": 2})).is_err());
//! ```

use serde_json::Value;

/// `v` at a dotted path (`model.mesh.tetrahedra`, `dialog.errors.width`, `ports.0.name`).
/// The value at `path` (keys and array indices, dotted) in `v`.
pub fn at_path<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').filter(|k| !k.is_empty()).try_fold(v, |v, k| match k.parse::<usize>() {
        Ok(i) if v.is_array() => v.get(i),
        _ => v.get(k),
    })
}

/// Whether `got` is what `want` asks for, or why not.
pub fn compare(got: &Value, want: &Value) -> Result<(), String> {
    match want {
        Value::String(s) if s == "*" => {
            if got.is_null() {
                return Err("missing".into());
            }
        }
        Value::Array(a) if a.len() == 2 && a.iter().all(Value::is_number) && got.is_number() => {
            let (w, tol, g) = (a[0].as_f64().unwrap_or(f64::NAN), a[1].as_f64().unwrap_or(1e-3), got.as_f64().unwrap_or(f64::NAN));
            if !((g - w).abs() <= tol * w.abs().max(1.0)) {
                return Err(format!("got {g}, want {w} (±{tol} rel)"));
            }
        }
        Value::Object(o) if o.contains_key("includes") || o.contains_key("excludes") => {
            let g = got.as_str().ok_or_else(|| format!("got {got}, want a text"))?;
            if let Some(w) = o.get("includes").and_then(Value::as_str).filter(|w| !g.contains(w)) {
                return Err(format!("`{g}` does not contain `{w}`"));
            }
            if let Some(w) = o.get("excludes").and_then(Value::as_str).filter(|w| g.contains(w)) {
                return Err(format!("`{g}` contains `{w}`"));
            }
        }
        Value::Object(o) if o.contains_key("min") || o.contains_key("max") => {
            let g = got.as_f64().ok_or_else(|| format!("got {got}, want a number"))?;
            if o.get("min").and_then(Value::as_f64).is_some_and(|m| g < m) || o.get("max").and_then(Value::as_f64).is_some_and(|m| g > m) {
                return Err(format!("got {g}, want within {want}"));
            }
        }
        _ => {
            if got != want {
                return Err(format!("got {got}, want {want}"));
            }
        }
    }
    Ok(())
}

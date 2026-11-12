//! Scenarios in the iced window, as the egui window runs them (`tests/scenarios/*.json`): the
//! window driven by the control channel's requests the way an agent drives it, with checks on
//! what it shows. A step is
//! - `{"call": METHOD, "params": {…}, "fail"?: true}`: a request (`fail`: it must be refused),
//!   a long command's answered once its job ends;
//! - `{"click": HANDLE}`: what the handle presses (`ui.handles` lists them);
//! - `{"frames": n}`: frames drawn (a running job looked at);
//! - `{"until": {…}, "timeout"?: s}`: frames until the checks pass;
//! - `{"expect": {…}}`: checks — `inspect` (`{path: value}` on `ui.inspect`, compared as
//!   [`emcraft_ui_core::scenario::compare`] does), `handles` shown, `hidden` not;
//! - `{"note": "…"}`: a comment.

use std::sync::Arc;
use std::time::{Duration, Instant};

use emcraft_control::ControlRequest;
use emcraft_ui_core::scenario::{at_path, compare};
use serde_json::{Value, json};

use crate::app::{App, Message};

/// How long a request (a solve) may take before the scenario fails.
const REPLY: Duration = Duration::from_secs(300);

/// A frame: the window looks at its job.
fn frame(app: &mut App) {
    app.update(Message::Tick(Instant::now()));
}

/// Request `method` with `params`: its reply, frames drawn until it comes.
pub fn call(app: &mut App, method: &str, params: Value) -> Result<Value, String> {
    let (req, rx) = ControlRequest::new(method, params);
    app.update(Message::Control(Arc::new(req)));
    let began = Instant::now();
    loop {
        if let Ok(v) = rx.try_recv() {
            return Ok(v);
        }
        if began.elapsed() > REPLY {
            return Err(format!("{method}: no reply in {} s", REPLY.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(10));
        frame(app);
    }
}

/// Whether the window shows what `e` asks for, or why not.
pub fn check(app: &App, e: &Value) -> Result<(), String> {
    let ui = crate::control::inspect(app);
    let shown: Vec<String> = crate::control::handles(app).into_iter().map(|(h, _)| h).collect();
    for (k, v) in e.as_object().ok_or("expect takes an object")? {
        match k.as_str() {
            "inspect" => {
                for (path, want) in v.as_object().ok_or("inspect takes {path: value}")? {
                    let got = at_path(&ui, path).cloned().unwrap_or(Value::Null);
                    compare(&got, want).map_err(|m| format!("{path}: {m}"))?;
                }
            }
            "handles" | "hidden" => {
                for n in v.as_array().into_iter().flatten().filter_map(Value::as_str) {
                    if shown.iter().any(|h| h == n) != (k == "handles") {
                        return Err(format!("{n}: want it {}", if k == "handles" { "shown" } else { "hidden" }));
                    }
                }
            }
            other => return Err(format!("unknown check `{other}`")),
        }
    }
    Ok(())
}

/// Runs `steps` on `app`; the first step that fails, by its index.
pub fn run(app: &mut App, steps: &[Value]) -> Result<(), String> {
    for (i, step) in steps.iter().enumerate() {
        let at = |e: String| format!("step {i} {step}: {e}");
        if let Some(method) = step.get("call").and_then(Value::as_str) {
            let reply = call(app, method, step.get("params").cloned().unwrap_or_else(|| json!({}))).map_err(at)?;
            let refused = reply["ok"] == json!(false);
            let want_refused = step.get("fail").and_then(Value::as_bool) == Some(true);
            if refused != want_refused {
                return Err(at(format!("reply {reply}")));
            }
        } else if let Some(h) = step.get("click").and_then(Value::as_str) {
            let reply = call(app, "ui.click", json!({"handle": h})).map_err(at)?;
            if reply["ok"] != json!(true) {
                return Err(at(format!("reply {reply}")));
            }
        } else if let Some(n) = step.get("frames").and_then(Value::as_u64) {
            (0..n.min(10_000)).for_each(|_| frame(app));
        } else if let Some(e) = step.get("until") {
            let timeout = Duration::from_secs_f64(step.get("timeout").and_then(Value::as_f64).unwrap_or(60.0).clamp(0.0, 3600.0));
            let began = Instant::now();
            while let Err(e2) = check(app, e) {
                if began.elapsed() > timeout {
                    return Err(at(e2));
                }
                std::thread::sleep(Duration::from_millis(10));
                frame(app);
            }
        } else if let Some(e) = step.get("expect") {
            check(app, e).map_err(at)?;
        } else if step.get("note").is_none() {
            return Err(at("an unknown step".into()));
        }
    }
    Ok(())
}

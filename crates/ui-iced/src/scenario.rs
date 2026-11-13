//! Scenarios in the iced window (`tests/scenarios/*.json`): the
//! window driven by the control channel's requests the way an agent drives it, with checks on
//! what it shows. A step is
//! - `{"call": METHOD, "params": {…}, "fail"?: true}`: a request (`fail`: it must be refused),
//!   a long command's answered once its job ends;
//! - `{"click": HANDLE}`: what the handle presses (`ui.handles` lists them);
//! - `{"press": TEXT}`: a click on the widget showing TEXT, through the window's layout as the
//!   pointer would (`"double": true`: twice);
//! - `{"field": TEXT, "text": NEW, "enter"?: true}`: a click into the field showing TEXT (`#id`:
//!   the field of that id), what is in it selected and NEW typed over it (Enter after);
//! - `{"key": KEY}`: a key pressed and let go (`Escape`, `Delete`, `Enter`, `Tab`, `/`, a letter,
//!   with `Ctrl+`, `Shift+`, `Alt+` before it), given first to the widget that has the keyboard
//!   then to the window's own keys as iced would;
//! - `{"size": [w, h]}`: the window's size for the steps after (1400 × 900 at first);
//! - `{"shot": NAME}`: a picture of the window written into `$EMCRAFT_SCENARIO_SHOTS` (when set)
//!   as `NAME-<renderer>.png`, drawn headless (`ICED_TEST_BACKEND=tiny-skia`: on the CPU);
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

/// The window's size for the steps that look at its layout, at first.
const SIZE: (f32, f32) = (1400.0, 900.0);

/// The window as the steps that use its layout see it: headless, at `size`.
fn simulator(app: &App, size: (f32, f32)) -> iced_test::Simulator<'_, Message> {
    let settings = iced::Settings { fonts: vec![emcraft_render::text::face_bytes().into()], ..iced::Settings::default() };
    iced_test::Simulator::with_size(settings, size, app.view())
}

/// Key `spec` (`Ctrl+Z`, `Escape`, `/`): the key and its modifiers.
fn key(spec: &str) -> Result<(iced::keyboard::Key, iced::keyboard::Modifiers), String> {
    use iced::keyboard::{Key, Modifiers, key::Named};
    let mut parts: Vec<&str> = spec.split('+').collect();
    // `Ctrl++`: the plus key.
    if spec.ends_with("++") {
        parts.truncate(parts.len().saturating_sub(2));
        parts.push("+");
    }
    let name = parts.pop().filter(|n| !n.is_empty()).ok_or_else(|| format!("no key in `{spec}`"))?;
    let modifiers = parts.iter().try_fold(Modifiers::empty(), |m, p| match p.to_ascii_lowercase().as_str() {
        "ctrl" | "cmd" => Ok(m | Modifiers::COMMAND),
        "shift" => Ok(m | Modifiers::SHIFT),
        "alt" => Ok(m | Modifiers::ALT),
        other => Err(format!("no modifier `{other}`")),
    })?;
    let named = match name {
        "Escape" => Some(Named::Escape),
        "Delete" => Some(Named::Delete),
        "Enter" => Some(Named::Enter),
        "Tab" => Some(Named::Tab),
        "Backspace" => Some(Named::Backspace),
        "Left" => Some(Named::ArrowLeft),
        "Right" => Some(Named::ArrowRight),
        "Up" => Some(Named::ArrowUp),
        "Down" => Some(Named::ArrowDown),
        _ => None,
    };
    match (named, name.chars().count()) {
        (Some(n), _) => Ok((Key::Named(n), modifiers)),
        (None, 1) => Ok((Key::Character(name.to_lowercase().into()), modifiers)),
        (None, _) => Err(format!("no key `{name}`")),
    }
}

/// The events of key `k` pressed and let go with `modifiers` held.
fn key_events(k: iced::keyboard::Key, modifiers: iced::keyboard::Modifiers) -> Vec<iced::Event> {
    use iced::keyboard::{Event as K, Location, key::NativeCode, key::Physical};
    let text = match (&k, modifiers.command() || modifiers.alt()) {
        (iced::keyboard::Key::Character(c), false) => Some(c.clone()),
        _ => None,
    };
    let physical = Physical::Unidentified(NativeCode::Unidentified);
    [
        K::ModifiersChanged(modifiers),
        K::KeyPressed {
            key: k.clone(),
            modified_key: k.clone(),
            physical_key: physical,
            location: Location::Standard,
            modifiers,
            text,
            repeat: false,
        },
        K::KeyReleased { key: k.clone(), modified_key: k, physical_key: physical, location: Location::Standard, modifiers },
        K::ModifiersChanged(iced::keyboard::Modifiers::empty()),
    ]
    .into_iter()
    .map(iced::Event::Keyboard)
    .collect()
}

/// `events` given to the window's widgets, then to its own keys as its subscriptions would:
/// the messages they make.
fn send_events(sim: &mut iced_test::Simulator<'_, Message>, events: Vec<iced::Event>) -> Vec<Message> {
    let statuses = sim.simulate(events.clone());
    let window = iced::window::Id::unique();
    let own = events.into_iter().zip(statuses).flat_map(|(e, status)| {
        [crate::files::shortcut, crate::files::dropped, crate::view3d::shader::keys]
            .into_iter()
            .filter_map(|listen| listen(e.clone(), status, window))
            .collect::<Vec<_>>()
    });
    let own: Vec<Message> = own.collect();
    sim.drain().chain(own).collect()
}

/// A step that goes through the window's layout (`press`, `field`, `key`, `shot`): the messages
/// it makes, or why it could not be done.
fn through_layout(app: &App, step: &Value, size: (f32, f32)) -> Result<Vec<Message>, String> {
    let mut sim = simulator(app, size);
    if let Some(t) = step.get("press").and_then(Value::as_str) {
        let times = if step.get("double").and_then(Value::as_bool) == Some(true) { 2 } else { 1 };
        for _ in 0..times {
            sim.click(t).map_err(|e| e.to_string())?;
        }
        return Ok(sim.into_messages().collect());
    }
    if let Some(f) = step.get("field").and_then(Value::as_str) {
        let typed = step.get("text").and_then(Value::as_str).ok_or("a field takes `text`")?;
        match f.strip_prefix('#') {
            Some(id) => sim.click(iced::widget::Id::from(id.to_owned())).map(|_| ()).map_err(|e| e.to_string())?,
            None => sim.click(f).map(|_| ()).map_err(|e| e.to_string())?,
        }
        let mut out = send_events(&mut sim, key_events(iced::keyboard::Key::Character("a".into()), iced::keyboard::Modifiers::COMMAND));
        let _ = sim.typewrite(typed);
        out.extend(sim.drain());
        if step.get("enter").and_then(Value::as_bool) == Some(true) {
            out.extend(send_events(
                &mut sim,
                key_events(iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter), iced::keyboard::Modifiers::empty()),
            ));
        }
        return Ok(out);
    }
    if let Some(k) = step.get("key").and_then(Value::as_str) {
        let (k, modifiers) = key(k)?;
        return Ok(send_events(&mut sim, key_events(k, modifiers)));
    }
    if let Some(name) = step.get("shot").and_then(Value::as_str) {
        if let Ok(dir) = std::env::var("EMCRAFT_SCENARIO_SHOTS") {
            let shot = sim.snapshot(&app.theme()).map_err(|e| e.to_string())?;
            let path = std::path::Path::new(&dir).join(name);
            let _ = std::fs::create_dir_all(&dir);
            let _ = shot.matches_image(&path).map_err(|e| e.to_string())?;
        }
        return Ok(Vec::new());
    }
    Err("an unknown step".into())
}

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
    let mut size = SIZE;
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
        } else if let Some(s) = step.get("size").and_then(Value::as_array) {
            let side = |k: usize| s.get(k).and_then(Value::as_f64).filter(|v| (200.0..=8000.0).contains(v)).map(|v| v as f32);
            size = side(0).zip(side(1)).ok_or_else(|| at("a size is [w, h], each 200 to 8000".into()))?;
            // The window's columns follow its width.
            app.window = size;
        } else if ["press", "field", "key", "shot"].iter().any(|k| step.get(k).is_some()) {
            for m in through_layout(app, step, size).map_err(at)? {
                app.update(m);
            }
        } else if step.get("note").is_none() {
            return Err(at("an unknown step".into()));
        }
    }
    Ok(())
}

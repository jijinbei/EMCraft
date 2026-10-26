//! Programmatic control of the running app (agents, tests), after SolidCraft's
//! (`crates/ui-egui/src/control.rs`): requests `{method, params}` handled on the UI thread
//! between frames, pointer and keyboard input injected as real egui events. The transport (JSON
//! lines on 127.0.0.1 with a per-launch token) lives in the `emcraft` app; the scenarios call
//! [`handle`] directly. Methods are listed in docs/control-protocol.md.

use std::sync::mpsc::Sender;

use serde_json::{Value, json};

use crate::{EmcraftApp, View, scenario};

/// A request from the control channel, with where its reply goes.
pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    pub reply: Sender<Value>,
}

impl ControlRequest {
    pub fn new(method: impl Into<String>, params: Value) -> (ControlRequest, std::sync::mpsc::Receiver<Value>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (ControlRequest { method: method.into(), params, reply: tx }, rx)
    }
}

/// What handling a request gave.
pub enum Outcome {
    /// The reply, now.
    Done(Value),
    /// The reply comes later through the request's sender (a job's end, a screenshot).
    Later,
}

/// Requests handled per frame, so a flood cannot starve drawing.
pub const MAX_PER_FRAME: usize = 64;
/// Synthetic events queued at most.
pub const MAX_EVENTS: usize = 10_000;

fn ok(v: Value) -> Outcome {
    Outcome::Done(json!({"ok": true, "result": v}))
}

fn err(e: impl std::fmt::Display) -> Outcome {
    Outcome::Done(json!({"ok": false, "error": e.to_string()}))
}

fn wrap(r: Result<Value, impl std::fmt::Display>) -> Outcome {
    match r {
        Ok(v) => ok(v),
        Err(e) => err(e),
    }
}

/// Handles the queued requests (called once a frame).
pub fn serve(app: &mut EmcraftApp, ctx: &egui::Context) {
    let Some(rx) = app.control_rx.take() else { return };
    for _ in 0..MAX_PER_FRAME {
        let Ok(req) = rx.try_recv() else { break };
        if let Outcome::Done(v) = handle(app, ctx, &req) {
            let _ = req.reply.send(v);
        }
        ctx.request_repaint();
    }
    app.control_rx = Some(rx);
}

fn key_from(name: &str) -> Option<egui::Key> {
    egui::Key::from_name(name).or(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Some(egui::Key::Enter),
        "esc" | "escape" => Some(egui::Key::Escape),
        "delete" => Some(egui::Key::Delete),
        "backspace" => Some(egui::Key::Backspace),
        "space" => Some(egui::Key::Space),
        "tab" => Some(egui::Key::Tab),
        "a" => Some(egui::Key::A),
        "/" | "slash" => Some(egui::Key::Slash),
        _ => None,
    })
}

fn push(app: &mut EmcraftApp, events: Vec<egui::Event>) -> Result<(), String> {
    if app.synthetic.len() + events.len() > MAX_EVENTS {
        return Err("too much queued input".into());
    }
    app.synthetic.extend(events);
    Ok(())
}

fn button(p: &Value) -> egui::PointerButton {
    match p.get("button").and_then(Value::as_str) {
        Some("right") => egui::PointerButton::Secondary,
        Some("middle") => egui::PointerButton::Middle,
        _ => egui::PointerButton::Primary,
    }
}

/// A point from `x`, `y`, or from `handle` (a published scenario handle).
fn point(p: &Value, x: &str, y: &str) -> Result<egui::Pos2, String> {
    if let Some(h) = p.get("handle").and_then(Value::as_str) {
        return scenario::handle_at(h).ok_or_else(|| format!("no handle `{}` on screen", h.chars().take(100).collect::<String>()));
    }
    let f = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite() && v.abs() <= 1e6);
    match (f(x), f(y)) {
        (Some(x), Some(y)) => Ok(egui::pos2(x as f32, y as f32)),
        _ => Err(format!("`{x}` and `{y}` must be numbers within ±1000000 (or give `handle`)")),
    }
}

fn mods(p: &Value) -> egui::Modifiers {
    let b = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
    egui::Modifiers { shift: b("shift"), alt: b("alt"), ctrl: b("ctrl") || b("cmd"), command: b("ctrl") || b("cmd"), mac_cmd: false }
}

/// The UI state, as `ui.inspect` reports it.
pub fn inspect(app: &EmcraftApp) -> Value {
    let progress = app.job.as_ref().map(|j| {
        let s = j.snapshot();
        json!({"command": j.command, "done": s.done, "total": s.total, "stage": s.stage, "fraction": s.fraction(), "elapsed": j.elapsed()})
    });
    let dialog = app.dialog.as_ref().map(|d| {
        let fields: serde_json::Map<String, Value> = d
            .fields
            .iter()
            .map(|f| {
                let v = match &f.kind {
                    crate::dialogs::Kind::Bool => json!(f.on),
                    crate::dialogs::Kind::Vector(_) => json!(f.parts),
                    crate::dialogs::Kind::Table(_) => json!(f.rows),
                    _ => json!(f.text),
                };
                (f.name.to_owned(), v)
            })
            .collect();
        let errors: serde_json::Map<String, Value> =
            d.fields.iter().filter_map(|f| f.error.as_ref().map(|e| (f.name.to_owned(), json!(e)))).collect();
        json!({"command": d.command.id, "docked": d.docked, "fields": fields, "errors": errors, "error": d.error})
    });
    json!({
        "tab": app.tab,
        "view": app.view.id(),
        "dark": app.dark,
        "dock_dialogs": app.dock_dialogs,
        "dialog": dialog,
        "busy": app.busy(),
        "progress": progress,
        "status": app.status().map(|m| json!({"text": m.text, "error": m.error})),
        "messages": app.messages.len(),
        "search": {"open": app.search.open, "text": app.search.text},
        "viewport": app.viewport_rect.map(|r| json!([r.left(), r.top(), r.width(), r.height()])),
        "field": {"frequency": app.field.view.frequency, "plane": app.field.view.plane.name(), "component": app.field.view.component.name(), "phase": app.field.view.phase, "mode": app.field.view.mode, "animate": app.field.animate, "shown": app.field.picture.is_some()},
        "chart": app.chart.as_ref().map(|c| c.key.clone()),
        "port_modes": app.port_modes.is_some(),
        "compare": app.compare.as_ref().and_then(|c| c.get("worst")).cloned(),
        "frames": app.frames,
        "workflow": crate::steps::inspect(app),
        "view3d": app.view3d.inspect(),
        "undo": app.session.undo.len(),
        "redo": app.session.redo.len(),
        "history": app.session.history.len(),
        "export": app.export,
        "model": app.session.summary(),
    })
}

/// Runs a command for the channel: a quick one replies now, a long one when its job ends
/// (`wait: false`: at once, with `{"started": id}`; follow it with `ui.inspect`).
fn execute(app: &mut EmcraftApp, req: &ControlRequest, id: &str, params: Value, wait: bool) -> Outcome {
    let long = crate::worker::is_long(id);
    let r = app.run(id, params);
    match r {
        Ok(v) if long && wait => match app.job.as_mut() {
            Some(job) => {
                job.waiting.push(req.reply.clone());
                Outcome::Later
            }
            None => ok(v),
        },
        other => wrap(other),
    }
}

/// Handles one request.
pub fn handle(app: &mut EmcraftApp, ctx: &egui::Context, req: &ControlRequest) -> Outcome {
    let p = &req.params;
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    let b = |k: &str| p.get(k).and_then(Value::as_bool);
    match req.method.as_str() {
        "engine.execute" => {
            let Some(id) = s("command") else { return err("missing `command`") };
            let params = p.get("params").cloned().filter(|v| !v.is_null()).unwrap_or(json!({}));
            execute(app, req, id, params, b("wait").unwrap_or(true))
        }
        "engine.script" => {
            let Some(steps) = p.get("commands").and_then(Value::as_array) else { return err("`commands` must be an array of {command, params}") };
            if steps.len() > 10_000 {
                return err("at most 10000 commands");
            }
            match app.run_script(Value::Array(steps.clone())) {
                Ok(_) if b("wait").unwrap_or(true) => match app.job.as_mut() {
                    Some(job) => {
                        job.waiting.push(req.reply.clone());
                        Outcome::Later
                    }
                    None => ok(Value::Null),
                },
                other => wrap(other),
            }
        }
        "engine.commands" => ok(Value::Array(
            emcraft_engine::COMMANDS
                .iter()
                .map(|c| {
                    let params: Vec<Value> = c
                        .params
                        .iter()
                        .map(|q| json!({"name": q.name, "kind": q.kind.name(), "required": q.required, "doc": q.doc, "choices": q.choices}))
                        .collect();
                    json!({"id": c.id, "label": c.label, "doc": c.doc, "params": params, "long": crate::worker::is_long(c.id)})
                })
                .collect(),
        )),
        "ui.inspect" => ok(inspect(app)),
        "ui.set" => {
            for k in ["tab", "view"] {
                if p.get(k).is_some_and(|v| !v.is_string()) {
                    return err(format!("`{k}` must be a string"));
                }
            }
            if let Some(tab) = s("tab") {
                if !crate::ribbon::TABS.iter().any(|(t, _)| *t == tab) {
                    return err(format!("unknown tab `{}`", tab.chars().take(40).collect::<String>()));
                }
                app.tab = tab.to_owned();
            }
            if let Some(v) = s("view") {
                match View::parse(v) {
                    Some(v) => app.view = v,
                    None => return err(format!("unknown view `{}`", v.chars().take(40).collect::<String>())),
                }
            }
            if let Some(d) = b("dark") {
                app.dark = d;
            }
            if let Some(d) = b("dock") {
                app.dock_dialogs = d;
                if let Some(dialog) = app.dialog.as_mut() {
                    dialog.docked = d;
                }
            }
            if let Some(a) = b("animate") {
                app.field.animate = a;
            }
            // The workflow step (`none` closes the panel) and the 3D camera.
            match p.get("step") {
                None => {}
                Some(Value::String(x)) if x == "none" => crate::steps::open_step(app, None),
                Some(Value::String(x)) => match x.parse::<crate::steps::Step>().ok() {
                    Some(step) => crate::steps::open_step(app, Some(step)),
                    None => return err(format!("unknown step `{}`", x.chars().take(40).collect::<String>())),
                },
                Some(_) => return err("`step` must be a string"),
            }
            if let Some(v) = s("standard_view") {
                match emcraft_render::view3d::StandardView::parse(v) {
                    Some(v) => app.view3d.standard(v),
                    None => return err(format!("unknown standard view `{}`", v.chars().take(40).collect::<String>())),
                }
            }
            if let Some(c) = p.get("camera") {
                let f = |k: &str| c.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
                let cam = &mut app.view3d.camera;
                if let Some(v) = f("yaw") {
                    cam.yaw = v.to_radians();
                }
                if let Some(v) = f("pitch") {
                    cam.pitch = v.to_radians();
                }
                if let Some(v) = f("distance").filter(|v| *v > 0.0) {
                    cam.distance = v;
                }
                match c.get("projection").and_then(Value::as_str) {
                    Some("orthographic") => cam.projection = emcraft_render::view3d::Projection::Orthographic,
                    Some("perspective") => cam.projection = emcraft_render::view3d::Projection::Perspective,
                    _ => {}
                }
                *cam = cam.sanitized();
            }
            ok(inspect(app))
        }
        "ui.start" => {
            let Some(id) = s("command") else { return err("missing `command`") };
            if let Err(e) = app.open_dialog(id) {
                return err(e);
            }
            if let (Some(d), Some(Value::Object(o))) = (app.dialog.as_mut(), p.get("params")) {
                for (k, v) in o {
                    if let Err(e) = d.set(k, v) {
                        app.dialog = None;
                        return err(e);
                    }
                }
            }
            ok(inspect(app)["dialog"].clone())
        }
        "ui.dialog" => {
            if app.dialog.is_none() {
                return err("no dialog is open");
            }
            if let (Some(d), Some(Value::Object(o))) = (app.dialog.as_mut(), p.get("set")) {
                for (k, v) in o {
                    if let Err(e) = d.set(k, v) {
                        return err(e);
                    }
                }
            }
            if b("cancel") == Some(true) {
                app.dialog = None;
                return ok(json!({"closed": true}));
            }
            if b("ok") == Some(true) {
                return wrap(app.dialog_ok());
            }
            ok(inspect(app)["dialog"].clone())
        }
        "ui.cancel" => ok(json!({"cancelled": app.cancel_job()})),
        "ui.messages" => {
            if b("clear") == Some(true) {
                app.messages.clear();
            }
            ok(json!(app.messages.iter().map(|m| json!({"text": m.text, "error": m.error, "count": m.count})).collect::<Vec<_>>()))
        }
        "ui.move" | "ui.click" => {
            let pos = match point(p, "x", "y") {
                Ok(p) => p,
                Err(e) => return err(e),
            };
            let m = mods(p);
            let mut ev = vec![egui::Event::ModifiersChanged(m), egui::Event::PointerMoved(pos)];
            if req.method == "ui.click" {
                let button = button(p);
                for _ in 0..if b("double") == Some(true) { 2 } else { 1 } {
                    ev.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers: m });
                    ev.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers: m });
                }
            }
            wrap(push(app, ev).map(|()| json!({"x": pos.x, "y": pos.y})))
        }
        "ui.drag" => {
            let (a, z) = match (point(p, "x0", "y0"), point(p, "x1", "y1")) {
                (Ok(a), Ok(z)) => (a, z),
                (Err(e), _) | (_, Err(e)) => return err(e),
            };
            let (button, m) = (button(p), mods(p));
            let steps = p.get("steps").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(10.0).clamp(1.0, 200.0) as usize;
            let mut ev = vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE), egui::Event::PointerMoved(a)];
            ev.push(egui::Event::PointerButton { pos: a, button, pressed: true, modifiers: m });
            for k in 1..=steps {
                ev.push(egui::Event::PointerMoved(a + (z - a) * (k as f32 / steps as f32)));
            }
            ev.push(egui::Event::PointerButton { pos: z, button, pressed: false, modifiers: m });
            wrap(push(app, ev).map(|()| Value::Null))
        }
        "ui.scroll" => {
            let pos = match point(p, "x", "y") {
                Ok(p) => p,
                Err(e) => return err(e),
            };
            let delta = p.get("delta").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(-1.0).clamp(-100.0, 100.0) as f32;
            let ev = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(0.0, delta),
                    modifiers: Default::default(),
                    phase: egui::TouchPhase::Move,
                },
            ];
            wrap(push(app, ev).map(|()| Value::Null))
        }
        "ui.key" => {
            let Some(key) = s("key").and_then(key_from) else { return err("unknown key") };
            let m = mods(p);
            let ev = vec![
                egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: m },
                egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: m },
            ];
            wrap(push(app, ev).map(|()| Value::Null))
        }
        "ui.text" => {
            let text: String = s("text").unwrap_or("").chars().take(crate::dialogs::MAX_FIELD).collect();
            wrap(push(app, vec![egui::Event::Text(text)]).map(|()| Value::Null))
        }
        "ui.handles" => {
            let list = scenario::handles(s("prefix").unwrap_or(""));
            ok(json!(list.into_iter().map(|(n, p)| json!({"name": n, "x": p.x, "y": p.y})).collect::<Vec<_>>()))
        }
        "ui.at" => match s("handle").and_then(scenario::handle_at) {
            Some(q) => ok(json!([q.x, q.y])),
            None => err("that handle is not on screen"),
        },
        "ui.render" => {
            let f = |k: &str, d: f64, lo: f64, hi: f64| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(d).clamp(lo, hi);
            let size = egui::vec2(f("width", 1500.0, 320.0, 4096.0) as f32, f("height", 960.0, 240.0, 4096.0) as f32);
            let scale = f("scale", 1.0, 0.5, 2.0) as f32;
            let canvas = crate::raster::render_app(app, size, scale);
            let Some(png) = canvas.png() else { return err("could not encode the picture") };
            match s("path") {
                Some(path) if !path.trim().is_empty() => match std::fs::write(path, &png) {
                    Ok(()) => ok(json!({"path": path, "width": canvas.w, "height": canvas.h, "bytes": png.len(), "renderer": "cpu"})),
                    Err(e) => err(format!("cannot write {path}: {e}")),
                },
                _ => ok(json!({"width": canvas.w, "height": canvas.h, "bytes": png.len(), "renderer": "cpu"})),
            }
        }
        "ui.screenshot" => {
            app.request_screenshot(s("path").map(str::to_owned), req.reply.clone());
            ctx.request_repaint();
            Outcome::Later
        }
        "ui.resize" => {
            let f = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
            let (Some(w), Some(h)) = (f("width"), f("height")) else { return err("missing width/height") };
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w.clamp(640.0, 8192.0) as f32, h.clamp(480.0, 8192.0) as f32)));
            ok(Value::Null)
        }
        "app.quit" => {
            app.quit_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ok(json!({"quitting": true}))
        }
        other => err(format!("unknown method `{}`", other.chars().take(64).collect::<String>())),
    }
}

/// A window screenshot as PNG, written to `path` or measured.
pub fn save_screenshot(image: &egui::ColorImage, path: Option<&str>) -> Value {
    let (w, h) = (image.size[0], image.size[1]);
    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let canvas = emcraft_render::Canvas { w, h, rgba };
    let Some(png) = canvas.png() else { return json!({"ok": false, "error": "could not encode the screenshot"}) };
    match path {
        Some(path) => match std::fs::write(path, &png) {
            Ok(()) => json!({"ok": true, "result": {"path": path, "width": w, "height": h}}),
            Err(e) => json!({"ok": false, "error": format!("cannot write {path}: {e}")}),
        },
        None => json!({"ok": true, "result": {"width": w, "height": h, "bytes": png.len()}}),
    }
}

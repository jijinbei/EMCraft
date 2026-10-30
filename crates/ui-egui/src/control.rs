//! Programmatic control of the running app (agents, tests), after SolidCraft's
//! (`crates/ui-egui/src/control.rs`): requests `{method, params}` handled on the UI thread
//! between frames, pointer and keyboard input injected as real egui events. The transport (JSON
//! lines on 127.0.0.1 with a per-launch token) lives in the `emcraft` app; the scenarios call
//! [`handle`] directly. Methods are listed in docs/control-protocol.md.

use std::sync::mpsc::Sender;

use serde_json::{Value, json};

use emcraft_engine::text::clip;

use crate::action::Pref;
use crate::{Action, EmcraftApp, View, scenario};

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
    let Some(rx) = app.control.rx.take() else { return };
    for _ in 0..MAX_PER_FRAME {
        let Ok(req) = rx.try_recv() else { break };
        if let Outcome::Done(v) = handle(app, ctx, &req) {
            let _ = req.reply.send(v);
        }
        ctx.request_repaint();
    }
    app.control.rx = Some(rx);
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
    if app.control.synthetic.len() + events.len() > MAX_EVENTS {
        return Err("too much queued input".into());
    }
    app.control.synthetic.extend(events);
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
        return scenario::handle_at(h).ok_or_else(|| format!("no handle `{}` on screen", clip(h, 100)));
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
        "tab": app.tab.id(),
        "view": app.view.id(),
        "theme": app.theme,
        "dark": app.themes.get(&app.theme).is_dark(),
        "themes": app.themes.names(),
        "theme_problems": app.themes.problems,
        "language": app.prefs.language(),
        "languages": crate::lang::ids(),
        "home": app.home.open,
        "settings": app.settings.map(|s| s.id()),
        "project": {"name": app.session.name(), "path": app.session.path(), "dirty": app.session.is_dirty(), "title": crate::files::title(app)},
        "files": {"prompt": app.files.prompt.as_ref().map(|p| p.text.clone()), "confirm": app.files.confirm.is_some(), "then": app.files.confirm.as_ref().map(|t| format!("{t:?}"))},
        "quitting": app.quit_requested,
        "autosave": app.autosave_inspect(),
        "recent": app.prefs.recent,
        "dock_dialogs": app.dock_dialogs,
        "dialog": dialog,
        "busy": app.busy(),
        "progress": progress,
        "status": app.status().map(|m| json!({"text": m.text, "error": m.error})),
        "messages": app.messages.len(),
        "search": {"open": app.search.open, "text": app.search.text},
        "viewport": app.viewport_rect.map(|r| json!([r.left(), r.top(), r.width(), r.height()])),
        "field": {"frequency": app.field.view.frequency, "plane": app.field.view.plane.id(), "component": app.field.view.component.id(), "phase": app.field.view.phase, "mode": app.field.view.mode, "animate": app.field.player.playing, "speed": app.field.player.speed, "shown": app.field.picture.is_some(), "arrows": [app.field.view.show_e, app.field.view.show_h], "hover": app.field.hover.map(|p| json!({"at": p.at, "e": [p.e.x.norm(), p.e.y.norm(), p.e.z.norm()]})), "pinned": app.field.pinned, "at": app.field.at, "beside_3d": app.field.beside_3d, "excitation": app.session.sparameter_entries().get(app.field.view.excitation).map(|e| e.name.clone())},
        "chart": app.views.chart.key().map(|k| format!("{k:?}")),
        "sparameters": app.views.sdisplay.id(),
        "variables": app.session.variables(),
        "study": app.session.results().study.as_ref().map(|s| json!({
            "kind": if s.kind == emcraft_engine::study::Kind::Sweep { "sweep" } else { "optimize" },
            "variables": s.variables, "rows": s.rows.len(), "converged": s.converged,
            "best": s.best.and_then(|b| s.rows.get(b)).map(|r| r.values.clone()),
            "chart": app.views.study.chart.key().map(|k| format!("{k:?}")), "parameter": app.views.study.parameter,
            "grid": s.grid.iter().map(Vec::len).collect::<Vec<_>>(), "free": s.variables.get(app.views.study.slice.free), "at": app.views.study.slice.at,
        })),
        "port_modes": app.replies.port_modes.is_some(),
        "compare": app.replies.compare.as_ref().and_then(|c| c.get("worst")).cloned(),
        "frames": app.frames,
        "workflow": crate::steps::inspect(app),
        "view3d": app.view3d.inspect(),
        "undo": app.session.undo().len(),
        "redo": app.session.redo().len(),
        "history": app.session.history().len(),
        "export": app.export,
        "model": app.session.summary(),
    })
}

/// Runs a command for the channel: a quick one replies now, a long one when its job ends
/// (`wait: false`: at once, with `{"started": id}`; follow it with `ui.inspect`).
fn execute(app: &mut EmcraftApp, req: &ControlRequest, id: &str, params: Value, wait: bool) -> Outcome {
    let long = crate::commands::is_long(id);
    match app.apply(Action::run(id, params)) {
        Ok(v) if long && wait => after_job(app, req, v),
        other => wrap(other),
    }
}

/// A request's parameters, read with their checks.
#[derive(Clone, Copy)]
struct Args<'a>(&'a Value);

impl<'a> Args<'a> {
    fn str(self, k: &str) -> Option<&'a str> {
        self.0.get(k).and_then(Value::as_str)
    }

    fn bool(self, k: &str) -> Option<bool> {
        self.0.get(k).and_then(Value::as_bool)
    }

    /// A finite number.
    fn num(self, k: &str) -> Option<f64> {
        self.0.get(k).and_then(Value::as_f64).filter(|v| v.is_finite())
    }

    /// A finite number clamped to `lo..=hi`, `default` when absent.
    fn clamped(self, k: &str, default: f64, lo: f64, hi: f64) -> f64 {
        self.num(k).unwrap_or(default).clamp(lo, hi)
    }

    fn get(self, k: &str) -> Option<&'a Value> {
        self.0.get(k)
    }
}

/// What a method gives: its outcome, or why it refused.
type Handled = Result<Outcome, String>;

/// A job to wait for, or the reply now: `r`'s value goes back when the job ends, if one runs.
fn after_job(app: &mut EmcraftApp, req: &ControlRequest, value: Value) -> Outcome {
    match app.job.as_mut() {
        Some(job) => {
            job.waiting.push(req.reply.clone());
            Outcome::Later
        }
        None => ok(value),
    }
}

/// Handles one request.
pub fn handle(app: &mut EmcraftApp, ctx: &egui::Context, req: &ControlRequest) -> Outcome {
    let a = Args(&req.params);
    let handled = match req.method.as_str() {
        "engine.execute" => engine_execute(app, req, a),
        "engine.script" => engine_script(app, req, a),
        "engine.commands" => Ok(ok(commands())),
        "ui.inspect" => Ok(ok(inspect(app))),
        "ui.set" => ui_set(app, a),
        "ui.start" => ui_start(app, a),
        "ui.dialog" => ui_dialog(app, a),
        "ui.cancel" => Ok(wrap(app.apply(Action::CancelJob))),
        "ui.messages" => Ok(ok(messages(app, a))),
        "ui.move" => pointer(app, a, None),
        "ui.click" => pointer(app, a, Some(a.bool("double") == Some(true))),
        "ui.drag" => drag(app, a),
        "ui.scroll" => scroll(app, a),
        "ui.key" => key(app, a),
        "ui.text" => {
            let text: String = clip(a.str("text").unwrap_or(""), crate::dialogs::MAX_FIELD);
            push(app, vec![egui::Event::Text(text)]).map(|()| ok(Value::Null))
        }
        "ui.handles" => {
            let list = scenario::handles(a.str("prefix").unwrap_or(""));
            Ok(ok(json!(list.into_iter().map(|(n, p)| json!({"name": n, "x": p.x, "y": p.y})).collect::<Vec<_>>())))
        }
        "ui.at" => a.str("handle").and_then(scenario::handle_at).map(|q| ok(json!([q.x, q.y]))).ok_or_else(|| "that handle is not on screen".into()),
        "ui.render" => render(app, a),
        "ui.screenshot" => {
            app.request_screenshot(a.str("path").map(str::to_owned), req.reply.clone());
            ctx.request_repaint();
            Ok(Outcome::Later)
        }
        "ui.resize" => {
            let (Some(w), Some(h)) = (a.num("width"), a.num("height")) else { return err("missing width/height") };
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w.clamp(640.0, 8192.0) as f32, h.clamp(480.0, 8192.0) as f32)));
            Ok(ok(Value::Null))
        }
        "app.quit" => {
            // As the window's close: unsaved work or a running job asks first, unless forced.
            if a.bool("force") == Some(true) {
                app.quit_forced();
            } else {
                crate::files::apply(app, crate::files::FileAction::Request(crate::files::Then::Quit));
            }
            if app.quit_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Ok(ok(json!({"quitting": app.quit_requested, "asking": app.files.confirm.is_some()})))
        }
        "app.autosave" => autosave(app, a),
        other => Err(format!("unknown method `{}`", clip(other, 64))),
    };
    handled.unwrap_or_else(err)
}

/// The autosave: its folder (`dir`, `null` for none: the copies found there are offered on the
/// start page, as on launch), its interval (`every`, s) and a copy written at once (`now`).
fn autosave(app: &mut EmcraftApp, a: Args) -> Handled {
    match a.get("dir") {
        None => {}
        Some(Value::Null) => {
            app.autosave.dir = None;
            app.autosave.found.clear();
        }
        Some(Value::String(d)) if !d.trim().is_empty() && d.len() <= 4096 => {
            let dir = std::path::PathBuf::from(d);
            app.autosave.found = crate::autosave::scan(&dir);
            app.autosave.dir = Some(dir);
            if !app.autosave.found.is_empty() {
                app.apply(Action::SetHome(true)).map_err(|e| e.to_string())?;
            }
        }
        Some(_) => return Err("`dir` must be a folder's path (at most 4096 bytes) or null".into()),
    }
    if let Some(v) = a.get("every") {
        app.autosave.every = v.as_f64().filter(|s| s.is_finite() && (0.0..=86_400.0).contains(s)).ok_or("`every` must be 0 to 86400 s")?;
    }
    if a.bool("now") == Some(true) {
        app.autosave_now()?;
    }
    Ok(ok(app.autosave_inspect()))
}

fn engine_execute(app: &mut EmcraftApp, req: &ControlRequest, a: Args) -> Handled {
    let id = a.str("command").ok_or("missing `command`")?;
    let params = a.get("params").cloned().filter(|v| !v.is_null()).unwrap_or(json!({}));
    Ok(execute(app, req, id, params, a.bool("wait").unwrap_or(true)))
}

fn engine_script(app: &mut EmcraftApp, req: &ControlRequest, a: Args) -> Handled {
    let steps = a.get("commands").and_then(Value::as_array).ok_or("`commands` must be an array of {command, params}")?;
    if steps.len() > 10_000 {
        return Err("at most 10000 commands".into());
    }
    app.run_script(Value::Array(steps.clone())).map_err(|e| e.to_string())?;
    Ok(if a.bool("wait").unwrap_or(true) { after_job(app, req, Value::Null) } else { ok(Value::Null) })
}

/// Every command with its parameters, for agents.
fn commands() -> Value {
    emcraft_engine::COMMANDS
        .iter()
        .map(|c| {
            let params: Vec<Value> = c
                .params
                .iter()
                .map(|q| json!({"name": q.name, "kind": q.kind.id(), "required": q.required, "doc": q.doc, "choices": q.choices}))
                .collect();
            json!({"id": c.id, "label": c.label, "doc": c.doc, "params": params, "long": crate::commands::is_long(c.id)})
        })
        .collect()
}

/// Sets what the window shows: tab, view, theme, language, docking, the field's plane
/// position and animation, the workflow step, the 3D camera. Each is the action the window's own control
/// asks for; all are read before any is performed.
fn ui_set(app: &mut EmcraftApp, a: Args) -> Handled {
    for k in ["tab", "view"] {
        if a.get(k).is_some_and(|v| !v.is_string()) {
            return Err(format!("`{k}` must be a string"));
        }
    }
    let mut asked = Vec::new();
    if let Some(tab) = a.str("tab") {
        asked.push(Action::SetTab(crate::ribbon::RibbonTab::parse(tab).ok_or_else(|| format!("unknown tab `{}`", clip(tab, 40)))?));
    }
    if let Some(v) = a.str("view") {
        asked.push(Action::SetView(View::parse(v).ok_or_else(|| format!("unknown view `{}`", clip(v, 40)))?));
    }
    // The theme by name; `dark` names the built-in ones by a switch. A reload reads the user's
    // theme files again before a theme is chosen.
    if a.bool("reload_themes") == Some(true) {
        app.apply(Action::Pref(Pref::ReloadThemes)).map_err(|e| e.to_string())?;
    }
    if let Some(d) = a.bool("dark") {
        asked.push(Action::Pref(Pref::Theme(if d { "dark" } else { "light" }.into())));
    }
    match a.get("theme") {
        None => {}
        Some(Value::String(name)) if app.themes.find(name).is_some() => asked.push(Action::Pref(Pref::Theme(name.clone()))),
        Some(Value::String(name)) => return Err(format!("no theme `{}` (there are {})", clip(name, 40), app.themes.names().join(", "))),
        Some(_) => return Err("`theme` must be a theme's name".into()),
    }
    match a.get("language") {
        None => {}
        Some(Value::String(id)) if crate::lang::find(id).is_some() => asked.push(Action::Pref(Pref::Language(id.clone()))),
        Some(Value::String(id)) => return Err(format!("no language `{}` (there are {})", clip(id, 16), crate::lang::ids().join(", "))),
        Some(_) => return Err("`language` must be a language's id".into()),
    }
    if let Some(d) = a.bool("dock") {
        asked.push(Action::SetDock(d));
    }
    // The settings page at a section; `none` closes it.
    match a.get("settings") {
        None => {}
        Some(Value::String(x)) if x == "none" => asked.push(Action::Settings(None)),
        Some(Value::String(x)) => {
            let section = crate::settings::Section::parse(x).ok_or_else(|| {
                format!(
                    "unknown settings section `{}` (there are {} or none)",
                    clip(x, 40),
                    <crate::settings::Section as emcraft_macros::Named>::listed()
                )
            })?;
            asked.push(Action::Settings(Some(section)));
        }
        Some(_) => return Err("`settings` must be a section's name or `none`".into()),
    }
    if let Some(on) = a.bool("animate") {
        asked.push(Action::SetPlaying(on));
    }
    // The workflow step (`none` closes the panel).
    match a.get("step") {
        None => {}
        Some(Value::String(x)) if x == "none" => asked.push(Action::OpenStep(None)),
        Some(Value::String(x)) => {
            let step = x.parse::<crate::steps::Step>().map_err(|_| format!("unknown step `{}`", clip(x, 40)))?;
            asked.push(Action::OpenStep(Some(step)));
        }
        Some(_) => return Err("`step` must be a string".into()),
    }
    if let Some(v) = a.str("standard_view") {
        let view = emcraft_render::view3d::StandardView::parse(v).ok_or_else(|| format!("unknown standard view `{}`", clip(v, 40)))?;
        asked.push(Action::StandardView(view));
    }
    for action in asked {
        app.apply(action).map_err(|e| e.to_string())?;
    }
    if let Some(at) = a.num("field_at") {
        app.field.at = Some(at);
    }
    if let Some(c) = a.get("camera") {
        let c = Args(c);
        let cam = &mut app.view3d.camera;
        if let Some(v) = c.num("yaw") {
            cam.yaw = v.to_radians();
        }
        if let Some(v) = c.num("pitch") {
            cam.pitch = v.to_radians();
        }
        if let Some(v) = c.num("distance").filter(|v| *v > 0.0) {
            cam.distance = v;
        }
        match c.str("projection") {
            Some("orthographic") => cam.projection = emcraft_render::view3d::Projection::Orthographic,
            Some("perspective") => cam.projection = emcraft_render::view3d::Projection::Perspective,
            _ => {}
        }
        *cam = cam.sanitized();
    }
    Ok(ok(inspect(app)))
}

/// Opens a command's dialog with fields set, as a person would before pressing OK.
fn ui_start(app: &mut EmcraftApp, a: Args) -> Handled {
    let id = a.str("command").ok_or("missing `command`")?;
    app.apply(Action::dialog(id)).map_err(|e| e.to_string())?;
    if let (Some(d), Some(Value::Object(o))) = (app.dialog.as_mut(), a.get("params")) {
        for (k, v) in o {
            if let Err(e) = d.set(k, v) {
                app.dialog = None;
                return Err(e);
            }
        }
    }
    Ok(ok(inspect(app)["dialog"].clone()))
}

/// Sets the open dialog's fields, then OK or Cancel.
fn ui_dialog(app: &mut EmcraftApp, a: Args) -> Handled {
    let d = app.dialog.as_mut().ok_or("no dialog is open")?;
    if let Some(Value::Object(o)) = a.get("set") {
        for (k, v) in o {
            d.set(k, v)?;
        }
    }
    if a.bool("cancel") == Some(true) {
        return Ok(wrap(app.apply(Action::DialogCancel)));
    }
    if a.bool("ok") == Some(true) {
        return Ok(wrap(app.apply(Action::DialogOk)));
    }
    Ok(ok(inspect(app)["dialog"].clone()))
}

fn messages(app: &mut EmcraftApp, a: Args) -> Value {
    if a.bool("clear") == Some(true) {
        app.messages.clear();
    }
    json!(app.messages.iter().map(|m| json!({"text": m.text, "error": m.error, "count": m.count})).collect::<Vec<_>>())
}

/// The pointer moved to a point, then clicked there (`double`: twice) when `click` is given.
fn pointer(app: &mut EmcraftApp, a: Args, click: Option<bool>) -> Handled {
    let pos = point(a.0, "x", "y")?;
    let m = mods(a.0);
    let mut ev = vec![egui::Event::ModifiersChanged(m), egui::Event::PointerMoved(pos)];
    if let Some(double) = click {
        let button = button(a.0);
        for _ in 0..if double { 2 } else { 1 } {
            ev.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers: m });
            ev.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers: m });
        }
    }
    push(app, ev)?;
    Ok(ok(json!({"x": pos.x, "y": pos.y})))
}

fn drag(app: &mut EmcraftApp, a: Args) -> Handled {
    let (from, to) = (point(a.0, "x0", "y0")?, point(a.0, "x1", "y1")?);
    let (button, m) = (button(a.0), mods(a.0));
    let steps = a.clamped("steps", 10.0, 1.0, 200.0) as usize;
    let mut ev = vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE), egui::Event::PointerMoved(from)];
    ev.push(egui::Event::PointerButton { pos: from, button, pressed: true, modifiers: m });
    ev.extend((1..=steps).map(|k| egui::Event::PointerMoved(from + (to - from) * (k as f32 / steps as f32))));
    ev.push(egui::Event::PointerButton { pos: to, button, pressed: false, modifiers: m });
    push(app, ev)?;
    Ok(ok(Value::Null))
}

fn scroll(app: &mut EmcraftApp, a: Args) -> Handled {
    let pos = point(a.0, "x", "y")?;
    let delta = a.clamped("delta", -1.0, -100.0, 100.0) as f32;
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, delta),
        modifiers: Default::default(),
        phase: egui::TouchPhase::Move,
    };
    push(app, vec![egui::Event::PointerMoved(pos), wheel])?;
    Ok(ok(Value::Null))
}

fn key(app: &mut EmcraftApp, a: Args) -> Handled {
    let key = a.str("key").and_then(key_from).ok_or("unknown key")?;
    let m = mods(a.0);
    let press = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: m };
    push(app, vec![press(true), press(false)])?;
    Ok(ok(Value::Null))
}

/// The whole window drawn on the CPU, written to `path` or measured.
fn render(app: &mut EmcraftApp, a: Args) -> Handled {
    let size = egui::vec2(a.clamped("width", 1500.0, 320.0, 4096.0) as f32, a.clamped("height", 960.0, 240.0, 4096.0) as f32);
    let canvas = crate::raster::render_app(app, size, a.clamped("scale", 1.0, 0.5, 2.0) as f32);
    let png = canvas.png().ok_or("could not encode the picture")?;
    let reply = json!({"width": canvas.w, "height": canvas.h, "bytes": png.len(), "renderer": "cpu"});
    match a.str("path").filter(|p| !p.trim().is_empty()) {
        Some(path) => {
            std::fs::write(path, &png).map_err(|e| format!("cannot write {path}: {e}"))?;
            let mut reply = reply;
            reply["path"] = json!(path);
            Ok(ok(reply))
        }
        None => Ok(ok(reply)),
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

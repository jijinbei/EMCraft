//! User scenarios run headless, after SolidCraft's (`crates/ui-egui/src/scenario.rs`): the real
//! app drawn frame by frame into an egui context without a window, driven by the control
//! channel's requests (clicks, keys, text, dialog OK) the way a person would use it, with checks
//! on the result.
//!
//! A scenario is a JSON list of steps:
//! - `{"call": "ui.start", "params": {…}, "fail"?: true}`: any control request (`fail`: it must
//!   be refused)
//! - `{"click": AT, "double"?: true}`, `{"move": AT}`, `{"drag": [AT, AT]}` where AT is `[x, y]`
//!   or a published handle's name (`"toolbar:mesh.generate"`, `"tab:SIMULATE"`, `"row:…"`), or
//!   `{"handle": name, "offset": [dx, dy]}`
//! - `{"widget": "Label"}`: click the widget whose accessible label is that text
//! - `{"key": "Enter", "cmd"?: true}`, `{"text": "22.86"}`; `{"frames": n}`
//! - `{"until": {…}, "timeout"?: s}`: frames until the checks pass (a job running on the worker)
//! - `{"expect": {…}}`: checks, see [`check`]
//! - `{"shot": "name"}`: with `EMCRAFT_SCENARIO_SHOTS` set, the frame as a PNG in that folder
//! - `{"note": "…"}`: a comment
//!
//! The same requests drive the live app over the control channel.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::EmcraftApp;
use crate::control::{ControlRequest, Outcome, handle};

thread_local! {
    /// Named points drawn this frame (buttons, rows, fields): scenarios and the control
    /// channel's `ui.handles` / `ui.at` find widgets by them.
    static HANDLES: RefCell<Vec<(String, egui::Pos2)>> = const { RefCell::new(Vec::new()) };
    static RECTS: RefCell<Vec<(String, egui::Rect)>> = const { RefCell::new(Vec::new()) };
    /// Last frame's, read between frames.
    static LAST: RefCell<(Vec<(String, egui::Pos2)>, Vec<(String, egui::Rect)>)> = const { RefCell::new((Vec::new(), Vec::new())) };
}

/// Most handles kept per frame.
const MAX_HANDLES: usize = 20_000;

pub fn publish_handle(name: &str, at: egui::Pos2) {
    HANDLES.with(|h| {
        let mut h = h.borrow_mut();
        if h.len() < MAX_HANDLES {
            h.push((name.to_owned(), at));
        }
    });
}

pub fn publish_rect(name: &str, r: egui::Rect) {
    RECTS.with(|h| {
        let mut h = h.borrow_mut();
        if h.len() < MAX_HANDLES {
            h.push((name.to_owned(), r));
        }
    });
}

/// A rectangle and its centre as a handle, under one name.
pub fn publish_target(name: &str, r: egui::Rect) {
    publish_handle(name, r.center());
    publish_rect(name, r);
}

/// Starts a frame: this frame's handles become last frame's.
pub fn clear_handles() {
    let h = HANDLES.with(|h| std::mem::take(&mut *h.borrow_mut()));
    let r = RECTS.with(|r| std::mem::take(&mut *r.borrow_mut()));
    // A frame that drew nothing (an offscreen render's first pass) keeps the last ones.
    if !h.is_empty() || !r.is_empty() {
        LAST.with(|l| *l.borrow_mut() = (h, r));
    }
}

fn current_and_last<T>(f: impl Fn(&[(String, egui::Pos2)], &[(String, egui::Rect)]) -> Option<T>) -> Option<T> {
    let now = HANDLES.with(|h| RECTS.with(|r| f(&h.borrow(), &r.borrow())));
    now.or_else(|| LAST.with(|l| f(&l.borrow().0, &l.borrow().1)))
}

/// Where a handle was drawn (this frame, else last frame).
pub fn handle_at(name: &str) -> Option<egui::Pos2> {
    current_and_last(|h, _| h.iter().rfind(|(n, _)| n == name).map(|x| x.1))
}

pub fn rect_of(name: &str) -> Option<egui::Rect> {
    current_and_last(|_, r| r.iter().rfind(|(n, _)| n == name).map(|x| x.1))
}

/// Every handle whose name starts with `prefix` (between frames: the frame just drawn).
pub fn handles(prefix: &str) -> Vec<(String, egui::Pos2)> {
    let list = HANDLES.with(|h| h.borrow().clone());
    let list = if list.is_empty() { LAST.with(|l| l.borrow().0.clone()) } else { list };
    list.into_iter().filter(|(n, _)| n.starts_with(prefix)).collect()
}

/// The app in a headless egui context.
pub struct Harness {
    pub app: EmcraftApp,
    pub ctx: egui::Context,
    pub output: egui::PlatformOutput,
    pub size: egui::Vec2,
    textures: crate::raster::Textures,
    last: Option<(Vec<egui::epaint::ClippedShape>, f32)>,
    time: f64,
}

impl Default for Harness {
    fn default() -> Self {
        Harness::new()
    }
}

impl Harness {
    pub fn new() -> Harness {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut h = Harness {
            app: EmcraftApp::new(),
            ctx,
            output: Default::default(),
            size: egui::vec2(1500.0, 960.0),
            textures: Default::default(),
            last: None,
            time: 0.0,
        };
        h.frames(3);
        h
    }

    /// Lays out and draws one frame, feeding queued input.
    pub fn frame(&mut self) {
        self.time += 1.0 / 60.0;
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
            time: Some(self.time),
            focused: true,
            ..Default::default()
        };
        self.app.raw_input_hook(&mut raw);
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        self.textures.update(&out.textures_delta);
        out.textures_delta.clear();
        self.last = Some((out.shapes, out.pixels_per_point));
        self.output = out.platform_output;
    }

    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
    }

    /// Frames until the queued input is used up.
    pub fn settle(&mut self) {
        for _ in 0..2000 {
            if self.app.synthetic.is_empty() {
                break;
            }
            self.frame();
        }
        self.frames(3);
    }

    /// The last frame as a picture.
    pub fn picture(&self) -> Option<emcraft_render::Canvas> {
        let (shapes, ppp) = self.last.clone()?;
        Some(crate::raster::raster(&self.ctx, shapes, ppp, self.size, &self.textures))
    }

    /// One control request; a reply that comes later (a job) is waited for, frame by frame.
    pub fn call(&mut self, method: &str, params: Value) -> Value {
        let (req, rx) = ControlRequest::new(method, params);
        let out = match handle(&mut self.app, &self.ctx, &req) {
            Outcome::Done(v) => v,
            Outcome::Later => {
                let began = Instant::now();
                loop {
                    self.frame();
                    if let Ok(v) = rx.try_recv() {
                        break v;
                    }
                    if began.elapsed() > Duration::from_secs(300) {
                        break json!({"ok": false, "error": "no answer in 300 s"});
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        };
        self.settle();
        out
    }

    /// The centre of the visible widget whose accessible label or value is `text` (the last
    /// drawn first: popups are drawn after what opened them).
    pub fn widget(&self, text: &str) -> Option<[f64; 2]> {
        let tree = self.output.accesskit_update.as_ref()?;
        tree.nodes
            .iter()
            .rev()
            .filter(|(_, n)| n.label() == Some(text) || n.value() == Some(text))
            .find_map(|(_, n)| n.bounds().filter(|b| b.x1 > b.x0 && b.y1 > b.y0).map(|b| [(b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0]))
    }

    /// A step's point on screen. A handle is read once the layout holds it still (a card that
    /// just opened grows over its first frames), as a person aims at what has stopped moving.
    pub fn at(&mut self, v: &Value) -> Result<[f64; 2], String> {
        if let Some(name) = v.as_str().or_else(|| v.get("handle").and_then(Value::as_str)) {
            let mut last = handle_at(name);
            for _ in 0..30 {
                self.frame();
                let now = handle_at(name);
                if now == last {
                    break;
                }
                last = now;
            }
            let off = v.get("offset").map_or([0.0, 0.0], |o| [0, 1].map(|i| o.get(i).and_then(Value::as_f64).unwrap_or(0.0)));
            return last.map(|p| [p.x as f64 + off[0], p.y as f64 + off[1]]).ok_or_else(|| format!("no handle `{name}` on screen"));
        }
        match (v.get(0).and_then(Value::as_f64), v.get(1).and_then(Value::as_f64)) {
            (Some(x), Some(y)) => Ok([x, y]),
            _ => Err(format!("not a point: {v}")),
        }
    }

    /// Runs one step.
    pub fn step(&mut self, s: &Value) -> Result<(), String> {
        let flag = |k: &str| s.get(k).and_then(Value::as_bool).unwrap_or(false);
        let res = if let Some(m) = s.get("call").and_then(Value::as_str) {
            self.call(m, s.get("params").cloned().unwrap_or(json!({})))
        } else if let Some(at) = s.get("click") {
            let [x, y] = self.at(at)?;
            // A pause before each click, as a hand makes: clicks in quick succession would count
            // as double and triple clicks.
            self.time += 1.0;
            self.call("ui.move", json!({"x": x, "y": y}));
            self.call("ui.click", json!({"x": x, "y": y, "double": flag("double"), "shift": flag("shift"), "button": s.get("button").cloned().unwrap_or(json!("left"))}))
        } else if let Some(at) = s.get("move") {
            let [x, y] = self.at(at)?;
            self.call("ui.move", json!({"x": x, "y": y}))
        } else if let Some(d) = s.get("drag") {
            let [x0, y0] = self.at(d.get(0).unwrap_or(&Value::Null))?;
            let [x1, y1] = self.at(d.get(1).unwrap_or(&Value::Null))?;
            self.call("ui.drag", json!({"x0": x0, "y0": y0, "x1": x1, "y1": y1, "steps": 8}))
        } else if let Some(text) = s.get("widget").and_then(Value::as_str) {
            let [x, y] = self.widget(text).ok_or_else(|| format!("no widget `{text}` on screen"))?;
            self.call("ui.move", json!({"x": x, "y": y}));
            self.call("ui.click", json!({"x": x, "y": y}))
        } else if let Some(k) = s.get("key").and_then(Value::as_str) {
            self.call("ui.key", json!({"key": k, "cmd": flag("cmd"), "shift": flag("shift")}))
        } else if let Some(t) = s.get("text").and_then(Value::as_str) {
            self.call("ui.text", json!({"text": t}))
        } else if let Some(n) = s.get("frames").and_then(Value::as_u64) {
            self.frames(n.min(10_000) as usize);
            return Ok(());
        } else if s.get("note").is_some() {
            return Ok(());
        } else if let Some(name) = s.get("shot").and_then(Value::as_str) {
            if let Some(dir) = std::env::var_os("EMCRAFT_SCENARIO_SHOTS") {
                let pic = self.picture().ok_or("no frame drawn yet")?;
                let png = pic.png().ok_or("could not encode the shot")?;
                let path = std::path::Path::new(&dir).join(format!("{name}.png"));
                std::fs::write(&path, png).map_err(|e| format!("shot {}: {e}", path.display()))?;
            }
            return Ok(());
        } else if let Some(e) = s.get("until") {
            let limit = Duration::from_secs_f64(s.get("timeout").and_then(Value::as_f64).unwrap_or(120.0).clamp(0.1, 600.0));
            let began = Instant::now();
            let mut last = String::new();
            while began.elapsed() < limit {
                match check(self, e) {
                    Ok(()) => return Ok(()),
                    Err(m) => last = m,
                }
                self.frames(2);
                std::thread::sleep(Duration::from_millis(5));
            }
            return Err(format!("timed out: {last}"));
        } else if let Some(e) = s.get("expect") {
            return check(self, e);
        } else {
            return Err(format!("unknown step {s}"));
        };
        let failed = res["ok"] != json!(true);
        if failed != flag("fail") {
            return Err(format!("{s}: {res}"));
        }
        Ok(())
    }

    /// Runs a scenario; the error names the failing step.
    pub fn run(&mut self, steps: &Value) -> Result<(), String> {
        for (i, s) in steps.as_array().ok_or("a scenario is a list of steps")?.iter().enumerate() {
            self.step(s).map_err(|e| format!("step {i}: {e}"))?;
        }
        Ok(())
    }
}

/// `v` at a dotted path (`model.mesh.tetrahedra`, `dialog.errors.width`, `ports.0.name`).
pub fn at_path<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').filter(|k| !k.is_empty()).try_fold(v, |v, k| match k.parse::<usize>() {
        Ok(i) if v.is_array() => v.get(i),
        _ => v.get(k),
    })
}

/// Checks:
/// - `view`, `tab`: the viewport's view id, the ribbon tab
/// - `dialog`: null (none open) or the open dialog's command id
/// - `busy`: whether a job runs
/// - `status`: a substring of the last message, or null for none; `status_error`: whether it is
///   an error
/// - `handles`: handles that must be on screen; `hidden`: handles that must not
/// - `inspect`: `{path: value}` in `ui.inspect`'s reply (a number may be `[value, rel tol]`;
///   `{"min": a, "max": b}` bounds it; `"*"` only asks that it is there and not null)
/// - `query`: `{command, params?, path, value}`: a quick command's reply at a path
/// - `picture`: the last frame has pixels other than its background (a drawn UI)
pub fn check(h: &mut Harness, e: &Value) -> Result<(), String> {
    let ui = crate::control::inspect(&h.app);
    let obj = e.as_object().ok_or("expect takes an object")?;
    for (k, v) in obj {
        match k.as_str() {
            "view" | "tab" | "busy" => {
                if ui[k] != *v {
                    return Err(format!("{k}: got {}, want {v}", ui[k]));
                }
            }
            "dialog" => {
                let got = &ui["dialog"]["command"];
                if (v.is_null() && !ui["dialog"].is_null()) || (!v.is_null() && got != v) {
                    return Err(format!("dialog: got {got}, want {v}"));
                }
            }
            "status" => {
                let text = ui["status"]["text"].as_str().unwrap_or("");
                match v.as_str() {
                    Some(want) if !text.contains(want) => return Err(format!("status: `{text}` does not contain `{want}`")),
                    None if v.is_null() && !text.is_empty() => return Err(format!("status: got `{text}`, want none")),
                    _ => {}
                }
            }
            "status_error" => {
                if ui["status"]["error"] != *v {
                    return Err(format!("status_error: got {} ({})", ui["status"]["error"], ui["status"]["text"]));
                }
            }
            "handles" | "hidden" => {
                for n in v.as_array().into_iter().flatten().filter_map(Value::as_str) {
                    if handle_at(n).is_some() != (k == "handles") {
                        return Err(format!("{n}: want {}", if k == "handles" { "shown" } else { "hidden" }));
                    }
                }
            }
            "inspect" => {
                for (path, want) in v.as_object().ok_or("inspect takes {path: value}")? {
                    let got = at_path(&ui, path).cloned().unwrap_or(Value::Null);
                    compare(&got, want).map_err(|m| format!("{path}: {m}"))?;
                }
            }
            "query" => {
                let id = v["command"].as_str().ok_or("query needs `command`")?;
                if crate::commands::is_long(id) {
                    return Err(format!("query: {id} is a long command"));
                }
                let reply = h.app.session.clone().execute(id, &v.get("params").cloned().unwrap_or(json!({}))).map_err(|e| e.to_string())?;
                let got = at_path(&reply, v["path"].as_str().unwrap_or("")).cloned().unwrap_or(Value::Null);
                compare(&got, &v["value"]).map_err(|m| format!("query {id}: {m}"))?;
            }
            "picture" => {
                let pic = h.picture().ok_or("no frame")?;
                let first: Vec<u8> = pic.rgba.get(..4).map(<[u8]>::to_vec).unwrap_or_default();
                let varied = pic.rgba.as_chunks::<4>().0.iter().filter(|p| p.as_slice() != first.as_slice()).count();
                if (varied > pic.w * pic.h / 20) != v.as_bool().unwrap_or(true) {
                    return Err(format!("picture: {varied} pixels differ from the corner"));
                }
            }
            other => return Err(format!("unknown check `{other}`")),
        }
    }
    Ok(())
}

fn compare(got: &Value, want: &Value) -> Result<(), String> {
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

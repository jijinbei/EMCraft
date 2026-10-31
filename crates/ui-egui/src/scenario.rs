//! User scenarios run headless, after SolidCraft's (`crates/ui-egui/src/scenario.rs`): the real
//! app drawn frame by frame into an egui context without a window, driven by the control
//! channel's requests (clicks, keys, text, dialog OK) the way a person would use it, with checks
//! on the result.
//!
//! A scenario is a JSON list of steps:
//! - `{"call": "ui.start", "params": {…}, "fail"?: true}`: any control request (`fail`: it must
//!   be refused)
//! - `{"click": AT, "double"?: true}`, `{"move": AT}`, `{"drag": [AT, AT], "button"?, "ctrl"?,
//!   "shift"?, "alt"?}` where AT is `[x, y]`
//!   or a published handle's name (`"toolbar:mesh.generate"`, `"tab:SIMULATE"`, `"row:…"`), or
//!   `{"handle": name, "offset": [dx, dy]}`
//! - `{"widget": "Label"}`: click the widget whose accessible label is that text
//! - `{"key": "Enter", "cmd"?: true, "press"?: "down" | "up"}` (held: down, frames, up),
//!   `{"text": "22.86"}`; `{"frames": n}`
//! - `{"until": {…}, "timeout"?: s}`: frames until the checks pass (a job running on the worker)
//! - `{"expect": {…}}`: checks, see [`check`]
//! - `{"shot": "name"}`: with `EMCRAFT_SCENARIO_SHOTS` set, the frame as a PNG in that folder
//! - `{"note": "…"}`: a comment
//!
//! The same requests drive the live app over the control channel.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::control::{ControlRequest, Outcome, handle};
use crate::{EmcraftApp, Handle};

/// Something drawn this frame under a name (a button, a row, a field): a point to click (a
/// handle) and the rectangle it covers, either or both. Scenarios and the control channel's
/// `ui.handles` / `ui.at` find widgets by them.
#[derive(Clone, Debug)]
struct Named {
    name: String,
    at: Option<egui::Pos2>,
    rect: Option<egui::Rect>,
}

thread_local! {
    /// This frame's, and last frame's (read between frames).
    static NAMED: RefCell<Vec<Named>> = const { RefCell::new(Vec::new()) };
    static LAST: RefCell<Vec<Named>> = const { RefCell::new(Vec::new()) };
}

/// Most names kept per frame.
const MAX_NAMED: usize = 40_000;

/// Keeps `name` (its text written once, straight from the handle) with its point and rectangle.
fn publish(name: Handle, at: Option<egui::Pos2>, rect: Option<egui::Rect>) {
    NAMED.with(|n| {
        let mut n = n.borrow_mut();
        if n.len() < MAX_NAMED {
            n.push(Named { name: name.to_string(), at, rect });
        }
    });
}

/// A point to click, by name.
pub fn publish_handle<'a>(name: impl Into<Handle<'a>>, at: egui::Pos2) {
    publish(name.into(), Some(at), None);
}

/// A rectangle, by name (not a handle).
pub fn publish_rect<'a>(name: impl Into<Handle<'a>>, r: egui::Rect) {
    publish(name.into(), None, Some(r));
}

/// A rectangle and its centre as a handle, under one name.
pub fn publish_target<'a>(name: impl Into<Handle<'a>>, r: egui::Rect) {
    publish(name.into(), Some(r.center()), Some(r));
}

/// Starts a frame: this frame's handles become last frame's.
pub fn clear_handles() {
    let now = NAMED.with(|n| std::mem::take(&mut *n.borrow_mut()));
    // A frame that drew nothing (an offscreen render's first pass) keeps the last ones.
    if !now.is_empty() {
        LAST.with(|l| *l.borrow_mut() = now);
    }
}

/// The last thing named `name` with `what` of it (this frame's, else last frame's).
fn latest<T>(name: &str, what: impl Fn(&Named) -> Option<T>) -> Option<T> {
    let find = |list: &[Named]| list.iter().rev().filter(|n| n.name == name).find_map(&what);
    NAMED.with(|n| find(&n.borrow())).or_else(|| LAST.with(|l| find(&l.borrow())))
}

/// Where a handle was drawn (this frame, else last frame).
pub fn handle_at(name: &str) -> Option<egui::Pos2> {
    latest(name, |n| n.at)
}

pub fn rect_of(name: &str) -> Option<egui::Rect> {
    latest(name, |n| n.rect)
}

/// Every handle whose name starts with `prefix` (between frames: the frame just drawn).
pub fn handles(prefix: &str) -> Vec<(String, egui::Pos2)> {
    let pick = |list: &[Named]| -> Vec<(String, egui::Pos2)> {
        list.iter().filter_map(|n| n.at.map(|at| (n.name.clone(), at))).filter(|(n, _)| n.starts_with(prefix)).collect()
    };
    let has_handles = |list: &[Named]| list.iter().any(|n| n.at.is_some());
    NAMED.with(|n| {
        let n = n.borrow();
        if has_handles(&n) { pick(&n) } else { LAST.with(|l| pick(&l.borrow())) }
    })
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

    /// Frames until the queued input is used up and the 3D view's change of view has ended
    /// (its animation steps by the frames' clock, so a scenario sees where it ends).
    pub fn settle(&mut self) {
        for _ in 0..2000 {
            if self.app.control.synthetic.is_empty() {
                break;
            }
            self.frame();
        }
        self.frames(3);
        for _ in 0..600 {
            if !self.app.view3d.animating() {
                break;
            }
            self.frame();
        }
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
            let button = s.get("button").cloned().unwrap_or(json!("left"));
            self.call("ui.drag", json!({"x0": x0, "y0": y0, "x1": x1, "y1": y1, "steps": 8, "button": button, "ctrl": flag("ctrl"), "shift": flag("shift"), "alt": flag("alt")}))
        } else if let Some(text) = s.get("widget").and_then(Value::as_str) {
            let [x, y] = self.widget(text).ok_or_else(|| format!("no widget `{text}` on screen"))?;
            self.call("ui.move", json!({"x": x, "y": y}));
            self.call("ui.click", json!({"x": x, "y": y}))
        } else if let Some(k) = s.get("key").and_then(Value::as_str) {
            self.call(
                "ui.key",
                json!({"key": k, "cmd": flag("cmd"), "shift": flag("shift"), "press": s.get("press").cloned().unwrap_or(Value::Null)}),
            )
        } else if let Some(t) = s.get("text").and_then(Value::as_str) {
            self.call("ui.text", json!({"text": t}))
        } else if let Some(n) = s.get("frames").and_then(Value::as_u64) {
            self.frames(n.min(10_000) as usize);
            return Ok(());
        } else if s.get("note").is_some() {
            return Ok(());
        } else if let Some(size) = s.get("size") {
            // The window resized, as a person drags its corner.
            let side = |i: usize| size.get(i).and_then(Value::as_f64).filter(|v| v.is_finite()).map(|v| v.clamp(200.0, 4000.0) as f32);
            let (Some(w), Some(h)) = (side(0), side(1)) else {
                return Err(format!("not a size: {size}"));
            };
            self.size = egui::vec2(w, h);
            self.frames(3);
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
/// - `handles`: handles that must be on screen; `hidden`: handles that must not; `inside`:
///   handles whose centre lies in the window (not run past its edge)
/// - `inspect`: `{path: value}` in `ui.inspect`'s reply (a number may be `[value, rel tol]`;
///   `{"min": a, "max": b}` bounds it; `{"includes": s, "excludes": t}` asks a text to hold `s`
///   and not `t`; `"*"` only asks that it is there and not null)
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
            "inside" => {
                for n in v.as_array().into_iter().flatten().filter_map(Value::as_str) {
                    let p = handle_at(n).ok_or_else(|| format!("{n}: not shown"))?;
                    if !egui::Rect::from_min_size(egui::Pos2::ZERO, h.size).contains(p) {
                        return Err(format!("{n}: at {p:?}, past the window's edge {:?}", h.size));
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

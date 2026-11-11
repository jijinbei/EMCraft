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

/// The farthest a point given to a request may lie (points, either side of the origin).
const MAX_POINT: f64 = 1e6;

/// The longest path a request may give (bytes).
const MAX_PATH: usize = 4096;

/// The longest autosave interval (s): a day.
const MAX_AUTOSAVE: f64 = 86_400.0;

/// The most commands one script request runs.
const MAX_SCRIPT: usize = 10_000;

/// The pinch factors a zoom request may give.
const PINCH: std::ops::RangeInclusive<f64> = 0.05..=20.0;

/// The largest side a rendered window may have (points).
const MAX_WINDOW: f64 = 4096.0;

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
    egui::Key::from_name(name).or_else(|| match name.to_ascii_lowercase().as_str() {
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
    let f = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite() && v.abs() <= MAX_POINT);
    match (f(x), f(y)) {
        (Some(x), Some(y)) => Ok(egui::pos2(x as f32, y as f32)),
        _ => Err(format!("`{x}` and `{y}` must be numbers within ±{MAX_POINT} (or give `handle`)")),
    }
}

fn mods(p: &Value) -> egui::Modifiers {
    let b = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
    egui::Modifiers { shift: b("shift"), alt: b("alt"), ctrl: b("ctrl") || b("cmd"), command: b("ctrl") || b("cmd"), mac_cmd: false }
}

/// The UI state, as `ui.inspect` reports it.
pub fn inspect(app: &EmcraftApp) -> Value {
    let mut v = summary(app);
    details(app, &mut v);
    v
}

/// A running job's progress: its command, steps, stage, residual, times, text and device.
fn job_progress(j: &crate::worker::Job) -> Value {
    let s = j.snapshot();
    json!({
        "command": j.command,
        "done": s.done,
        "total": s.total,
        "stage": s.stage,
        "open": s.open,
        "residual": s.residual,
        "fraction": s.fraction(),
        "elapsed": j.elapsed(),
        "estimate": j.estimate,
        "remaining": j.remaining(),
        "text": crate::messages::job_text(j),
        "device": j.place().map(|w| json!({"device": w.device.id(), "gpu": w.gpu(), "fronts": w.fronts})),
    })
}

/// The open dialog: its command, fields as typed, errors, and the import dialog's file.
fn dialog_state(app: &EmcraftApp) -> Option<Value> {
    app.ui_state.dialog().map(|d| {
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
        // The import dialog: what the chosen file holds (`geometry.import.inspect`'s reply or why
        // it cannot be read) and whether its options are unfolded.
        let detected = app.replies.import.as_ref().map(|p| match &p.reply {
            Ok(v) => json!(v),
            Err(e) => json!({"error": e}),
        });
        json!({"command": d.command.id, "docked": d.docked, "fields": fields, "errors": errors, "error": d.error,
               "detected": detected, "options": d.options, "help": d.help})
    })
}

/// What the file prompt asks: a path being typed, a save to confirm and what follows it.
fn files_state(app: &EmcraftApp) -> Value {
    let ask = app.ui_state.ask();
    json!({
        "prompt": match ask { Some(crate::files::Ask::Path(p)) => Some(p.text.clone()), _ => None },
        "confirm": matches!(ask, Some(crate::files::Ask::Save(_))),
        "then": match ask { Some(crate::files::Ask::Save(t)) => Some(format!("{t:?}")), _ => None },
    })
}

/// The field view: its frequency, plane, component, phase, playback, what is drawn and hovered.
fn field_state(app: &EmcraftApp) -> Value {
    let f = &app.field;
    json!({
        "frequency": f.view.frequency,
        "plane": f.view.plane.id(),
        "component": f.view.component.id(),
        "phase": f.view.phase,
        "mode": f.view.mode,
        "animate": f.player.playing,
        "speed": f.player.speed,
        "shown": f.picture.is_some() || (f.beside_3d && f.samples.is_some()),
        "arrows": [f.view.show_e, f.view.show_h],
        "hover": f.hover.map(|p| json!({"at": p.at, "e": [p.e.x.norm(), p.e.y.norm(), p.e.z.norm()]})),
        "pinned": f.pinned,
        "at": f.at,
        "beside_3d": f.beside_3d,
        "plot_zoom": f.plot_zoom,
        "excitation": app.session.sparameter_entries().get(f.view.excitation).map(|e| e.name.clone()),
    })
}

/// The last study: its kind, variables, rows, best point and how its chart is sliced.
fn study_state(app: &EmcraftApp) -> Option<Value> {
    app.session.results().study.as_ref().map(|s| {
        json!({
            "kind": match s.kind {
                emcraft_engine::study::Kind::Sweep => "sweep",
                emcraft_engine::study::Kind::Optimize => "optimize",
                emcraft_engine::study::Kind::Tolerance => "tolerance",
            },
            "variables": s.variables, "rows": s.rows.len(), "converged": s.converged,
            "best": s.best.and_then(|b| s.rows.get(b)).map(|r| r.values.clone()),
            "chart": app.views.study.chart.key().map(|k| format!("{k:?}")), "parameter": app.views.study.parameter,
            "grid": s.grid.iter().map(Vec::len).collect::<Vec<_>>(), "free": s.variables.get(app.views.study.slice.free), "at": app.views.study.slice.at,
            "surrogate": s.surrogate.is_some(),
            "predicted": s.rows.iter().filter(|r| r.is_predicted()).count(),
        })
    })
}

/// The PCB diagnosis: the board, its report, the net and conditions, the stage.
fn pcb_state(app: &EmcraftApp) -> Value {
    let views = app.session.views();
    json!({
        "loaded": app.session.model().pcb().is_some(),
        "report": app.replies.pcb,
        "selected_net": views.pcb.net(),
        "conditions": views.pcb.conditions().map(|c| c.shown(app.session.display().frequency)),
        "unconfirmed": app.views.pcb.unconfirmed,
        "stage": emcraft_engine::pcb::stage(&app.session).id(),
        "illustration": views.pcb_illustration,
    })
}

/// The window's state at large: tab, view, theme, language, project, dialogs, job, views.
fn summary(app: &EmcraftApp) -> Value {
    json!({
        "tab": app.tab.id(),
        "view": app.place.view().id(),
        "theme": app.theme,
        "dark": app.themes.get(&app.theme).is_dark(),
        "themes": app.themes.names(),
        "theme_problems": app.themes.problems,
        "language": app.prefs.language(),
        "font": app.fonts.in_use(app.language),
        "languages": crate::lang::ids(),
        "home": app.ui_state.page.start(),
        "template": app.ui_state.template().map(|card| app.template_preview.inspect(card)),
        "settings": app.ui_state.page.settings_shown().map(super::settings::Section::id),
        "project": {"name": app.session.name(), "path": app.session.path(), "dirty": app.session.is_dirty(), "title": crate::files::title(app)},
        "files": files_state(app),
        "quitting": app.quit_requested,
        "autosave": app.autosave_inspect(),
        "recent": app.prefs.recent,
        "dock_dialogs": app.dock_dialogs,
        "dialog": dialog_state(app),
        "busy": app.busy(),
        "progress": app.job.as_ref().map(job_progress),
        "status": app.status().map(|m| json!({"text": m.text, "error": m.error})),
        "messages": app.messages.len(),
        "search": {"open": app.ui_state.searching(), "text": app.search.text},
        "viewport": app.viewport_rect.map(|r| json!([r.left(), r.top(), r.width(), r.height()])),
        "field": field_state(app),
        "chart": app.views.chart.key().map(|k| format!("{k:?}")),
        "sparameters": app.views.sdisplay.id(),
        "variables": app.session.variables(),
        "study": study_state(app),
        "port_modes": app.replies.port_modes.is_some(),
        "compare": app.replies.compare.as_ref().map(|c| c.worst),
        "frames": app.frames,
        "workflow": crate::steps::inspect(app),
        "view3d": app.view3d.inspect(&app.ui_state, app.session.display().length),
        "navigation": {"mapping": app.prefs.nav().id, "orbit_around": app.prefs.orbit_around().id(), "navigator": app.prefs.navigator().id()},
        "undo": app.session.undo().len(),
        "redo": app.session.redo().len(),
        "history": app.session.history().len(),
        "export": match app.ui_state.ask() { Some(crate::files::Ask::Export(p)) => Some(p.clone()), _ => None },
        "model": app.session.summary(),
    })
}

/// The rest of the report, by part: how results are looked at, the history, the ports, the
/// probes, the PCB, visibility, the estimates.
fn details(app: &EmcraftApp, v: &mut Value) {
    // How results are looked at: the chart's style, the band to refine, the reference planes,
    // the probes, the far field's cut, the run's estimates.
    let estimate = |e: Option<&Result<Value, String>>| {
        e.and_then(|e| e.as_ref().ok()).map(|e| json!({"seconds": e["seconds"], "basis": e["basis"], "command": e["command"]}))
    };
    // How the chart view last drawn packed its blocks.
    v["drawing"] = json!({"gpu": app.prefs.gpu_look});
    v["packed"] = json!(app.views.packed.map(|[beside, under]| json!({"beside": beside, "under": under})));
    let s = &app.session;
    v["purpose"] = json!(app.home.purpose.map(emcraft_model::Analysis::id));
    v["page"] = json!(app.ui_state.page.shown().id());
    v["browser_row"] = json!(app.ui_state.selection().row());
    v["recording"] = json!(app.recorder.on.as_ref().map(|r| r.entries.len()));
    v["replaying"] = json!(app.recorder.playing.as_ref().map(std::collections::VecDeque::len));
    v["layers"] = json!(crate::layers::stack(app).iter().map(|l| l.id()).collect::<Vec<_>>());
    v["tabs"] = json!(app.ui_state.page.tabs().iter().map(|t| t.id()).collect::<Vec<_>>());
    v["kept"] = json!(emcraft_engine::archive::rows(&app.session));
    v["compare"] = json!(app.views.compare);
    v["chart_style"] = chart_style(app);
    v["refine"] = json!(app.views.refine.band.map(|(a, b)| [s.show_frequency(a), s.show_frequency(b)]));
    v["deembed"] = json!(s.model().ports.iter().map(|p| s.show_length(p.deembed)).collect::<Vec<_>>());
    // Each waveguide port's role as the browser and the 3D view show it, the driven one the input.
    v["ports"] =
        json!(crate::browser::port_roles(s).iter().map(|r| json!({"name": r.name, "role": r.id(), "s": r.sparameter()})).collect::<Vec<_>>());
    v["driven"] = json!(s.sparameter_entries().get(s.driven_entry()).map(|e| e.name.clone()));
    v["probes"] = json!(s.views().probes.iter().map(|p| json!({"name": p.name, "at": p.at.map(|x| s.show_length(x))})).collect::<Vec<_>>());
    v["field"]["s_curves"] = json!(
        app.session
            .results()
            .sweep
            .as_ref()
            .and_then(|s| s.results.first())
            .map(|r| (0..r.s.ports()).map(|p| format!("S{}{}", p + 1, app.field.view.excitation + 1)).collect::<Vec<_>>())
            .unwrap_or_default()
    );
    v["field"]["power"] = json!(app.field.power.as_ref().and_then(|p| p.value.as_ref().ok()));
    v["pcb"] = pcb_state(app);
    v["hidden_geometry"] = json!(app.session.views().hidden_geometry);
    v["hidden_materials"] = json!(app.session.views().hidden_materials);
    // The eigenmode table's cavity figures per mode (its T, or null).
    v["eigen_cavity"] =
        json!(app.views.eigen_table.as_ref().map(|e| e.cavity.iter().map(|c| c.as_ref().map(|c| c["transit"].clone())).collect::<Vec<_>>()));
    v["field"]["db"] = json!(app.field.view.db);
    v["field"]["contours"] = json!(app.field.view.contours);
    let pattern = &app.views.pattern;
    v["pattern"] = json!({"cut": pattern.cut.id(), "angle": pattern.angle, "quantity": pattern.quantity.id(), "component": pattern.component.id()});
    v["estimate"] =
        json!({"run": estimate(crate::steps::run_estimate(app)), "dialog": estimate(app.workflow.dialog_estimate.as_ref().map(|e| &e.reply))});
}

/// The chart view's style (`view.chart.style`'s, as it replies it), whether its card is open,
/// and the curves of the chart drawn; null away from a chart view.
fn chart_style(app: &EmcraftApp) -> Value {
    use emcraft_engine::views::ChartId;
    let id = match app.place.view() {
        View::SParameters => ChartId::SParameters,
        View::Tdr => ChartId::Tdr,
        View::Pulse => ChartId::Pulse,
        View::FarField => ChartId::Pattern,
        View::Dispersion => ChartId::Dispersion,
        View::Study => ChartId::Study,
        View::Analyses => ChartId::Analyses,
        View::System => ChartId::System,
        _ => return Value::Null,
    };
    json!({
        "chart": id.id(),
        "style": emcraft_engine::views::shown_style(&app.session, id),
        "open": app.views.style.open == Some(id),
        "curves": app.views.curves.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
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

    /// `k` as one of `T`'s ids, a `what`: none when absent; an error naming the choices.
    fn named<T: emcraft_macros::Named>(self, k: &str, what: &str) -> Result<Option<T>, String> {
        match self.get(k) {
            None => Ok(None),
            Some(Value::String(x)) => T::parse(x).map(Some).ok_or_else(|| format!("unknown {what} `{}` (there are {})", clip(x, 40), T::listed())),
            Some(_) => Err(format!("`{k}` must be a {what}'s name")),
        }
    }

    /// [`Args::named`], or `none` for none of them (`Some(None)`).
    fn named_or_none<T: emcraft_macros::Named>(self, k: &str, what: &str) -> Result<Option<Option<T>>, String> {
        match self.get(k) {
            None => Ok(None),
            Some(Value::String(x)) if x == "none" => Ok(Some(None)),
            Some(Value::String(x)) => {
                T::parse(x).map(|t| Some(Some(t))).ok_or_else(|| format!("unknown {what} `{}` (there are {} or none)", clip(x, 40), T::listed()))
            }
            Some(_) => Err(format!("`{k}` must be a {what}'s name or `none`")),
        }
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
        "ui.record" => record(app, a),
        "ui.replay" => replay(app, a),
        "app.crashes" => crashes(app, a),
        "ui.messages" => Ok(ok(messages(app, a))),
        "ui.move" => pointer(app, a, Press::None),
        "ui.click" => pointer(app, a, if a.bool("double") == Some(true) { Press::Double } else { Press::Single }),
        "ui.drag" => drag(app, a),
        "ui.scroll" => scroll(app, a),
        "ui.key" => key(app, a),
        "ui.text" => {
            let text: String = clip(a.str("text").unwrap_or(""), crate::dialogs::MAX_FIELD);
            push(app, vec![egui::Event::Text(text)]).map(|()| ok(Value::Null))
        }
        "ui.drop" => {
            let Some(path) = a.str("path").filter(|p| !p.trim().is_empty()) else { return err("give `path`: the file dropped on the window") };
            if app.control.dropped.len() >= 16 {
                return err("too many files dropped at once");
            }
            app.control.dropped.push(clip(path, MAX_PATH).into());
            Ok(ok(Value::Null))
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
            Ok(ok(json!({"quitting": app.quit_requested, "asking": matches!(app.ui_state.ask(), Some(crate::files::Ask::Save(_)))})))
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
        Some(v) => {
            let dir = folder(v).ok_or_else(|| format!("`dir` must be a folder's path (at most {MAX_PATH} bytes) or null"))?;
            app.autosave.found = crate::autosave::scan(&dir);
            app.autosave.dir = Some(dir);
            if !app.autosave.found.is_empty() {
                app.apply(Action::Show(crate::page::Tab::Start)).map_err(|e| e.to_string())?;
            }
        }
    }
    if let Some(v) = a.get("every") {
        app.autosave.every = v
            .as_f64()
            .filter(|s| s.is_finite() && (0.0..=MAX_AUTOSAVE).contains(s))
            .ok_or_else(|| format!("`every` must be 0 to {MAX_AUTOSAVE} s"))?;
    }
    if a.bool("now") == Some(true) {
        app.autosave_now()?;
    }
    Ok(ok(app.autosave_inspect()))
}

/// Starts recording (`on`: true) or stops it, the recording written to `path` when given.
fn record(app: &mut EmcraftApp, a: Args) -> Handled {
    if a.bool("on").ok_or("`on` must be true or false")? {
        app.start_recording();
        return Ok(ok(json!({"recording": true})));
    }
    let r = app.stop_recording().ok_or("nothing is being recorded")?;
    let n = r.entries.len();
    if let Some(path) = a.str("path") {
        let text = serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", clip(path, 200)))?;
    }
    Ok(ok(json!({"actions": n, "dropped": r.dropped, "recording": r})))
}

/// Plays a recording back: from `path` (a file or a crash report's folder), or `recording`
/// itself.
fn replay(app: &mut EmcraftApp, a: Args) -> Handled {
    let r = match (a.str("path"), a.get("recording")) {
        (Some(path), _) => crate::record::Recording::read(std::path::Path::new(path))?,
        (None, Some(r)) => crate::record::Recording::parse(&r.to_string())?,
        (None, None) => return Err("give `path` or `recording`".into()),
    };
    Ok(wrap(app.play(r)))
}

/// A request's folder: a non-blank path of at most [`MAX_PATH`] bytes; none for anything else.
fn folder(v: &Value) -> Option<std::path::PathBuf> {
    v.as_str().filter(|d| !d.trim().is_empty() && d.len() <= MAX_PATH).map(std::path::PathBuf::from)
}

/// Where crash reports go (`dir`: a folder, the journal begun there), and the reports there.
fn crashes(app: &mut EmcraftApp, a: Args) -> Handled {
    match a.get("dir") {
        None => {}
        Some(v) => app.keep_crash_reports(folder(v).ok_or_else(|| format!("`dir` must be a folder's path (at most {MAX_PATH} bytes)"))?, false),
    }
    let reports = app.crashes.dir.as_deref().map(crate::crash::list).unwrap_or_default();
    Ok(ok(json!({
        "dir": app.crashes.dir.as_ref().map(|d| d.display().to_string()),
        "journal": app.crashes.journal.as_ref().map(|j| j.path().display().to_string()),
        "reports": reports.iter().map(|r| json!({"id": r.id, "kind": r.kind, "message": r.message, "count": r.count, "seen": r.seen, "folder": r.folder.display().to_string()})).collect::<Vec<_>>(),
    })))
}

fn engine_execute(app: &mut EmcraftApp, req: &ControlRequest, a: Args) -> Handled {
    let id = a.str("command").ok_or("missing `command`")?;
    let params = a.get("params").cloned().filter(|v| !v.is_null()).unwrap_or_else(|| json!({}));
    Ok(execute(app, req, id, params, a.bool("wait").unwrap_or(true)))
}

fn engine_script(app: &mut EmcraftApp, req: &ControlRequest, a: Args) -> Handled {
    let steps = a.get("commands").and_then(Value::as_array).ok_or("`commands` must be an array of {command, params}")?;
    if steps.len() > MAX_SCRIPT {
        return Err(format!("at most {MAX_SCRIPT} commands"));
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
    let mut asked = Vec::new();
    asked.extend(a.named("tab", "tab")?.map(Action::SetTab));
    asked.extend(a.named("view", "view")?.map(Action::SetView));
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
    asked.extend(a.named_or_none("settings", "settings section")?.map(|s| s.map_or(Action::CloseSettings, |s| Action::OpenSettings(Some(s)))));
    // A page of the window by its tab (`start`, `project`, `settings`).
    asked.extend(a.named_or_none::<crate::page::Tab>("page", "page")?.flatten().map(Action::Show));
    if let Some(on) = a.bool("animate") {
        asked.push(Action::SetPlaying(on));
    }
    // A test hook: the running job's estimate (s), to see the job past it on a small model.
    if a.get("job_estimate").is_some() {
        let e = a.num("job_estimate").filter(|e| e.is_finite() && *e >= 0.0).ok_or("`job_estimate` must be seconds, ≥ 0")?;
        app.job.as_mut().ok_or("`job_estimate`: no job runs")?.estimate = Some(e);
    }
    // The 3D view's tool (`none` turns it off).
    asked.extend(a.named_or_none("tool", "tool")?.map(Action::Tool));
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
    // The section view: `{"axis", "at"?, "keep"?}` (`at` in display units, the model's middle
    // when left out), `null` off.
    let section = match a.get("section") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(s) => Some(Some(cut_of(app, Args(s))?)),
    };
    // The 3D view's navigation: the mouse mapping, what an orbit turns about, the navigator.
    if let Some(id) = a.str("navigation") {
        let p = crate::navigation::NAV_PRESETS.iter().find(|p| p.id == id).ok_or_else(|| {
            format!(
                "unknown mouse mapping `{}` (there are {})",
                clip(id, 40),
                crate::navigation::NAV_PRESETS.iter().map(|p| p.id).collect::<Vec<_>>().join(", ")
            )
        })?;
        asked.push(Action::Pref(Pref::Navigation(p.id.into())));
    }
    asked.extend(a.named("orbit_around", "orbit centre")?.map(|o| Action::Pref(Pref::OrbitAround(o))));
    asked.extend(a.named("navigator", "navigator")?.map(|n| Action::Pref(Pref::Navigator(n))));
    asked.extend(a.named("standard_view", "standard view")?.map(Action::StandardView));
    for action in asked {
        app.apply(action).map_err(|e| e.to_string())?;
    }
    if let Some(s) = section {
        app.view3d.section = s;
    }
    if let Some(at) = a.num("field_at") {
        app.field.at = Some(at);
    }
    if let Some(c) = a.get("camera") {
        let c = Args(c);
        let mut cam = app.view3d.camera;
        let cam = &mut cam;
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
        let cam = *cam;
        // Turning or moving the camera ends a view change; the projection alone does not.
        if ["yaw", "pitch", "distance"].iter().any(|k| c.get(k).is_some()) {
            app.view3d.set_camera(cam);
        } else {
            app.view3d.camera = cam.sanitized();
        }
    }
    Ok(ok(inspect(app)))
}

/// A section view's cut from `ui.set`'s `section`.
fn cut_of(app: &EmcraftApp, s: Args) -> Result<crate::view3d::Cut, String> {
    let name = s.str("axis").ok_or("`section` takes {\"axis\": \"x\" | \"y\" | \"z\", \"at\"?, \"keep\"?: \"low\" | \"high\"} or null")?;
    let axis = crate::handle::AXES.iter().position(|a| *a == name).ok_or_else(|| format!("unknown axis `{}` (x, y or z)", clip(name, 16)))?;
    let keep = match s.str("keep") {
        None => emcraft_render::view3d::Keep::Low,
        Some(k) => emcraft_render::view3d::Keep::parse(k).ok_or_else(|| format!("unknown side `{}` (low or high)", clip(k, 16)))?,
    };
    let l = app.session.display().length;
    let at = match s.get("at") {
        None => {
            let bounds = app.view3d.bounds().ok_or("give `at`: the 3D view has no model drawn yet")?;
            crate::view3d::Cut::middle(axis, bounds).at
        }
        Some(v) => l.to_si(v.as_f64().filter(|x| x.is_finite() && x.abs() <= 1e9).ok_or("`at` must be a number within ±1e9 (display units)")?),
    };
    Ok(crate::view3d::Cut { axis, at, keep })
}

/// Opens a command's dialog with fields set, as a person would before pressing OK.
fn ui_start(app: &mut EmcraftApp, a: Args) -> Handled {
    let id = a.str("command").ok_or("missing `command`")?;
    app.apply(Action::dialog(id)).map_err(|e| e.to_string())?;
    if let (Some(d), Some(Value::Object(o))) = (app.ui_state.dialog_mut(), a.get("params")) {
        for (k, v) in o {
            if let Err(e) = d.set(k, v) {
                app.ui_state.take_dialog();
                return Err(e);
            }
        }
    }
    Ok(ok(inspect(app)["dialog"].clone()))
}

/// Sets the open dialog's fields, then OK or Cancel.
fn ui_dialog(app: &mut EmcraftApp, a: Args) -> Handled {
    let d = app.ui_state.dialog_mut().ok_or("no dialog is open")?;
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

/// What the pointer does where it moves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Press {
    /// Nothing: it only moves.
    None,
    Single,
    Double,
}

/// The pointer moved to a point, then pressed there as `press` says.
fn pointer(app: &mut EmcraftApp, a: Args, press: Press) -> Handled {
    let pos = point(a.0, "x", "y")?;
    let m = mods(a.0);
    let mut ev = vec![egui::Event::ModifiersChanged(m), egui::Event::PointerMoved(pos)];
    let clicks = match press {
        Press::None => 0,
        Press::Single => 1,
        Press::Double => 2,
    };
    let button = button(a.0);
    for _ in 0..clicks {
        ev.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers: m });
        ev.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers: m });
    }
    push(app, ev)?;
    Ok(ok(json!({"x": pos.x, "y": pos.y})))
}

fn drag(app: &mut EmcraftApp, a: Args) -> Handled {
    let (from, to) = (point(a.0, "x0", "y0")?, point(a.0, "x1", "y1")?);
    let (button, m) = (button(a.0), mods(a.0));
    let steps = a.clamped("steps", 10.0, 1.0, 200.0) as usize;
    // `press: false` goes on with a drag whose button is still down, `release: false` leaves it
    // down (a picture of the drag under way).
    let (press, release) = (a.bool("press") != Some(false), a.bool("release") != Some(false));
    let mut ev = Vec::new();
    if press {
        ev.extend([egui::Event::ModifiersChanged(egui::Modifiers::NONE), egui::Event::PointerMoved(from)]);
        ev.push(egui::Event::PointerButton { pos: from, button, pressed: true, modifiers: m });
    }
    ev.extend((1..=steps).map(|k| egui::Event::PointerMoved(from + (to - from) * (k as f32 / steps as f32))));
    if release {
        ev.push(egui::Event::PointerButton { pos: to, button, pressed: false, modifiers: m });
    }
    push(app, ev)?;
    Ok(ok(Value::Null))
}

/// The wheel or trackpad at a point: `delta` lines up (and `dx` sideways), or with `unit`
/// `point` that many points; `pan: true` sends them as a two-finger scroll (a gesture's start,
/// move and end); `zoom` is a pinch's factor; the modifiers as for a click.
fn scroll(app: &mut EmcraftApp, a: Args) -> Handled {
    let pos = point(a.0, "x", "y")?;
    let unit = match a.str("unit") {
        None | Some("line") => egui::MouseWheelUnit::Line,
        Some("point") => egui::MouseWheelUnit::Point,
        Some("page") => egui::MouseWheelUnit::Page,
        Some(u) => return Err(format!("unknown unit `{}` (line, point or page)", clip(u, 16))),
    };
    let limit = if unit == egui::MouseWheelUnit::Point { 4000.0 } else { 100.0 };
    let delta = egui::vec2(a.clamped("dx", 0.0, -limit, limit) as f32, a.clamped("delta", -1.0, -limit, limit) as f32);
    let modifiers = mods(a.0);
    let wheel = |phase, delta| egui::Event::MouseWheel { unit, delta, modifiers, phase };
    let mut ev = vec![egui::Event::ModifiersChanged(modifiers), egui::Event::PointerMoved(pos)];
    if let Some(z) = a.num("zoom") {
        if !(z.is_finite() && PINCH.contains(&z)) {
            return Err(format!("`zoom` must be a pinch factor within {}–{}", PINCH.start(), PINCH.end()));
        }
        ev.push(egui::Event::Zoom(z as f32));
        ev.push(egui::Event::PointerMoved(pos));
    } else if a.bool("pan") == Some(true) {
        let unit_ok = unit == egui::MouseWheelUnit::Point;
        if !unit_ok {
            return Err("a two-finger scroll (`pan`) moves by points: give `unit`: `point`".into());
        }
        ev.extend([
            wheel(egui::TouchPhase::Start, egui::Vec2::ZERO),
            wheel(egui::TouchPhase::Move, delta),
            wheel(egui::TouchPhase::End, egui::Vec2::ZERO),
        ]);
    } else {
        ev.push(wheel(egui::TouchPhase::Move, delta));
    }
    push(app, ev)?;
    Ok(ok(Value::Null))
}

fn key(app: &mut EmcraftApp, a: Args) -> Handled {
    let key = a.str("key").and_then(key_from).ok_or("unknown key")?;
    let m = mods(a.0);
    let press = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: m };
    // `press`: only down (held until an `up`) or only up; else a tap.
    let events = match a.str("press") {
        None => vec![press(true), press(false)],
        Some("down") => vec![press(true), egui::Event::ModifiersChanged(m)],
        Some("up") => vec![press(false)],
        Some(p) => return Err(format!("unknown press `{}` (down or up)", clip(p, 16))),
    };
    push(app, events)?;
    Ok(ok(Value::Null))
}

/// The whole window drawn on the CPU, written to `path` or measured.
fn render(app: &mut EmcraftApp, a: Args) -> Handled {
    let size = egui::vec2(a.clamped("width", 1500.0, 320.0, MAX_WINDOW) as f32, a.clamped("height", 960.0, 240.0, MAX_WINDOW) as f32);
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
    let rgba: Vec<u8> = image.pixels.iter().flat_map(egui::Color32::to_srgba_unmultiplied).collect();
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

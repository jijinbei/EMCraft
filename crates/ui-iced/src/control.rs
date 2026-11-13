//! The control channel in the iced window (docs/control-protocol.md): agents and scenarios ask
//! it what it shows (`ui.inspect`), run commands (`engine.execute`, `engine.script`, a long one
//! answered when its job ends), change what it shows (`ui.set`), fill and confirm a dialog
//! (`ui.dialog`) and click what it names (`ui.click`, `ui.handles`). iced takes no input made up
//! by the app itself, so a click names what it presses — a handle — and the window does what
//! pressing it does: [`handles`] is every handle the window shows now with its message.

use std::sync::Arc;

use emcraft_control::ControlRequest;
use emcraft_ui_core::files::{Answer, FileAction, Then};
use emcraft_ui_core::scene3d::Overlay;
use emcraft_ui_core::view::View;
use iced::Task;
use serde_json::{Value, json};

use crate::app::{App, Message};
use crate::view3d::Nav;

/// The most commands a script request runs.
const MAX_SCRIPT: usize = 10_000;

fn ok(v: Value) -> Value {
    json!({"ok": true, "result": v})
}

fn err(e: impl std::fmt::Display) -> Value {
    json!({"ok": false, "error": e.to_string()})
}

/// Every handle the window shows now, with the message pressing it sends: the bar's pages and
/// buttons, the tree's ▶ and ＋ (and the commands of the ＋ list shown) and its ghost rows, the
/// model page's overlays, views and tools, the browser's rows, the selection's card, the open
/// dialog's and question's buttons.
pub fn handles(app: &App) -> Vec<(String, Message)> {
    let replies = &app.replies;
    let pages = crate::run::PAGES.into_iter().filter(|v| v.available(&app.session, replies)).map(|v| (format!("page:{}", v.id()), Message::Page(v)));
    let bar = [
        ("start".to_owned(), Message::Home(!app.home)),
        ("settings".to_owned(), Message::Settings(!app.settings)),
        ("file:new".to_owned(), Message::File(FileAction::Request(Then::New))),
        ("file:open".to_owned(), Message::File(FileAction::Request(Then::Open))),
        ("file:save".to_owned(), Message::File(FileAction::Save)),
        ("file:save_as".to_owned(), Message::File(FileAction::SaveAs)),
        ("edit:undo".to_owned(), Message::Start("edit.undo".into())),
        ("edit:redo".to_owned(), Message::Start("edit.redo".into())),
    ];
    // The tree's ▶ and ＋ (the open list's commands too).
    let kind = app.solver();
    let parts: Vec<&emcraft_ui_core::outline::Part> = app.tree.iter().flat_map(|(_, t)| t).filter(|_| !app.home && !app.settings).collect();
    let plays = parts.iter().filter_map(|p| match p.head {
        emcraft_ui_core::outline::Head::Step(s) => {
            emcraft_ui_core::outline::play(s).map(|id| (format!("play:{}", s.id()), crate::browser::play_message(id)))
        }
        emcraft_ui_core::outline::Head::Section(_) => None,
    });
    let offers = |p: &&&emcraft_ui_core::outline::Part| !emcraft_ui_core::outline::adds(p.head, kind).is_empty();
    let adds = parts.iter().filter(offers).map(|p| {
        let open = app.adding == Some(p.key());
        (format!("add:{}", p.key()), Message::Add((!open).then_some(p.key())))
    });
    let commands = parts.iter().filter(|p| app.adding == Some(p.key())).flat_map(|p| emcraft_ui_core::outline::adds(p.head, kind)).filter_map(|a| {
        let handle = match a {
            emcraft_ui_core::outline::Add::Command(id) => format!("command:{id}"),
            emcraft_ui_core::outline::Add::NewDesign => "new:design".to_owned(),
            // The tools' handles are the model page's (`tool:<id>`).
            emcraft_ui_core::outline::Add::Tool(_) => return None,
        };
        Some((handle, crate::browser::add_message(a)))
    });
    // A design of the user's opened in SolidCraft from its row.
    let designs = app.rows().filter_map(|r| match &r.act {
        emcraft_ui_core::browser::Act::Design(path) => Some((format!("solidcraft:{path}"), Message::OpenDesign(path.clone()))),
        _ => None,
    });
    // The groups' ghost rows (`ghost:<group>`).
    let ghosts = parts.iter().flat_map(|p| &p.groups).filter_map(|(name, _)| {
        emcraft_ui_core::outline::ghost(name, kind)
            .map(|id| (format!("ghost:{name}"), crate::browser::add_message(emcraft_ui_core::outline::Add::Command(id))))
    });
    let tabs = plays.chain(adds).chain(designs).chain(ghosts).collect::<Vec<_>>();
    let model = (app.page == View::Model).then(|| {
        let overlays = Overlay::ALL.into_iter().map(|o| (format!("overlay:{}", o.id()), Message::View3d(Nav::Overlay(o))));
        let views = emcraft_render::view3d::StandardView::ALL
            .into_iter()
            .map(|v| (format!("view3d:std:{}", v.id()), Message::View3d(Nav::Goal(emcraft_ui_core::navigation::Goal::standard(v)))));
        let section = app.view3d.bounds().map(|b| emcraft_ui_core::scene3d::Cut::middle(0, b));
        let fixed = [
            ("view3d:fit".to_owned(), Message::View3d(Nav::Goal(emcraft_ui_core::navigation::Goal::Fit))),
            ("view3d:section".to_owned(), Message::View3d(Nav::Section(app.view3d.section.map_or(section, |_| None)))),
            ("view3d:section:look".to_owned(), Message::View3d(Nav::LookAtCut)),
            ("view3d:section:dims".to_owned(), Message::View3d(Nav::Dims(!app.view3d.section_dims))),
        ];
        // Each tool turned on, or off when on.
        let tools =
            emcraft_ui_core::tools::Tool::ALL.into_iter().map(|k| (format!("tool:{}", k.id()), Message::Tool((app.tool != Some(k)).then_some(k))));
        // The field's peak in the legend, pinned.
        let peak = app.view3d.shown.as_ref().and_then(|s| s.bar.as_ref()?.peak.as_ref()).map(|(_, at)| ("field:peak".to_owned(), Message::Pin(*at)));
        overlays.chain(views).chain(fixed).chain(tools).chain(peak).collect::<Vec<_>>()
    });
    let rows = app.rows().map(|r| (format!("row:{}", r.id), Message::Row(r.id.clone())));
    let steps = crate::steps::stepped(app)
        .then(|| emcraft_engine::workflow::Step::ALL.into_iter().map(|s| (format!("step:{}", s.id()), Message::Step(Some(s)))).collect::<Vec<_>>());
    let panel = crate::step_panel::handles(app);
    let spar = crate::sparameters::handles(app);
    let result_views = crate::results::handles(app);
    let style = crate::style_card::handles(app);
    // The start page's purposes and templates, and the template card's buttons.
    use crate::template_dialog::TemplateAct;
    let start = app.home.then(|| {
        let import = ("home:import".to_owned(), Message::File(FileAction::Request(Then::Import)));
        let purposes = emcraft_ui_core::steps::Solver::ALL.into_iter().map(|k| (format!("purpose:{}", k.id()), Message::Purpose(Some(k))));
        let templates =
            emcraft_engine::templates::TEMPLATES.iter().map(|s| (format!("template:{}", s.name), Message::Template(TemplateAct::Open(Some(s.name)))));
        let blank = app.purpose.map(|k| ("home:blank".to_owned(), Message::File(FileAction::Request(Then::Blank(k)))));
        std::iter::once(import).chain(blank).chain(purposes).chain(templates).collect::<Vec<_>>()
    });
    let card_buttons = app.template.card.is_some().then(|| {
        [
            ("template:build".to_owned(), Message::Template(TemplateAct::Build)),
            ("template:cancel".to_owned(), Message::Template(TemplateAct::Open(None))),
        ]
    });
    let search = (!app.search.trim().is_empty()).then(|| {
        let picks = emcraft_ui_core::search::matches(&app.search).into_iter().map(|c| (format!("search:{}", c.id), Message::SearchPick(Some(c.id))));
        std::iter::once(("search:enter".to_owned(), Message::SearchPick(None))).chain(picks).collect::<Vec<_>>()
    });
    let card = (!app.selection.is_clear()).then(|| [("card:delete".to_owned(), Message::Delete)]);
    let dialog = app.dialog.is_some().then(|| {
        [
            ("dialog:ok".to_owned(), Message::Dialog(crate::dialog::Edit::Ok)),
            ("dialog:cancel".to_owned(), Message::Dialog(crate::dialog::Edit::Cancel)),
        ]
        .into_iter()
        .chain(crate::import::handles(app))
        .collect::<Vec<_>>()
    });
    let asking = app.asking.is_some().then(|| {
        [Answer::Save, Answer::Discard, Answer::Cancel]
            .map(|a| (format!("ask:{}", format!("{a:?}").to_lowercase()), Message::File(FileAction::Confirm(a))))
    });
    let job = app.job.is_some().then(|| ("job:cancel".to_owned(), Message::Cancel));
    let reports = crate::reports::handles(app);
    let log = (!app.log.lines().is_empty()).then(|| ("messages:toggle".to_owned(), Message::Log(!app.log_open)));
    pages
        .chain(bar)
        .chain(tabs)
        .chain(commands)
        .chain(model.into_iter().flatten())
        .chain(rows)
        .chain(steps.into_iter().flatten())
        .chain(panel)
        .chain(spar)
        .chain(result_views)
        .chain(style)
        .chain(start.into_iter().flatten())
        .chain(card_buttons.into_iter().flatten())
        .chain(search.into_iter().flatten())
        .chain(card.into_iter().flatten())
        .chain(dialog.into_iter().flatten())
        .chain(asking.into_iter().flatten())
        .chain(job)
        .chain(reports)
        .chain(log)
        .collect()
}

/// What the window shows, for agents and scenarios.
pub fn inspect(app: &App) -> Value {
    let v = &app.view3d;
    let c = v.camera;
    json!({
        "project": {"name": app.session.name(), "dirty": app.session.is_dirty(), "path": app.session.path()},
        "page": app.page.id(),
        "settings": app.settings,
        "start": app.home,
        "adding": app.adding,
        "error": app.error,
        "font": app.font.text(),
        "recording": app.recorder.on.as_ref().map(|r| r.entries.len()),
        "replaying": app.recorder.playing.as_ref().map(std::collections::VecDeque::len),
        "reports": app.crashes.shown.as_ref().map(Vec::len),
        "messages": {
            "last": app.log.last().map(|l| json!({"text": l.text, "error": l.error, "count": l.count})),
            "count": app.log.lines().len(),
            "open": app.log_open,
        },
        "job": app.job.as_ref().map(|j| json!({"command": j.command, "text": j.text()})),
        "dialog": app.dialog.as_ref().map(|d| json!({
            "command": d.command.id,
            "error": d.error,
            "options": d.options,
            "help": d.help,
            "detected": crate::import::inspect(app, d),
            "fields": d.fields.iter().map(|f| (f.name.to_owned(), json!({"text": f.text, "on": f.on, "parts": f.parts, "error": f.error}))).collect::<serde_json::Map<_, _>>(),
        })),
        "asking": app.asking.as_ref().map(|t| format!("{t:?}")),
        "selection": app.selection.iter().map(|p| format!("{p:?}")).collect::<Vec<_>>(),
        "tools": app.tools.inspect(app.tool, app.session.display().length),
        "template": app.template.card.as_ref().map(emcraft_ui_core::template_card::TemplateCard::inspect),
        "purpose": app.purpose.map(emcraft_model::Analysis::id),
        "toasts": app.toasts.shown().iter().map(|t| json!({"text": t.text, "error": t.error})).collect::<Vec<_>>(),
        "search": {"query": app.search, "matches": emcraft_ui_core::search::matches(&app.search).iter().map(|c| c.id).collect::<Vec<_>>()},
        "workflow": crate::steps::inspect(app),
        "sparameters": crate::sparameters::inspect(app),
        "result_views": crate::results::inspect(app),
        "view3d": {
            "overlay": v.overlay.id(),
            "yaw": c.yaw.to_degrees(),
            "pitch": c.pitch.to_degrees(),
            "distance": c.distance,
            "animating": v.animating(),
            "size": [v.size.0, v.size.1],
            "section_dims": v.section_dims,
            "section_notes": app.section_notes().map(|(d, p)| json!({"dims": d.iter().map(|d| d.text.clone()).collect::<Vec<_>>(), "ports": p.iter().map(|p| p.text.clone()).collect::<Vec<_>>()})),
            "projection": format!("{:?}", c.projection).to_lowercase(),
            "section": v.section.map(|s| s.inspect(app.session.display().length)),
            "field": field_inspect(app),
        },
        "results": {
            "sweep": app.session.results().sweep.is_some(),
            "eigen": app.session.results().eigen.is_some(),
            "port_modes": app.replies.port_modes.is_some(),
            "analysis": app.replies.analysis.as_ref().map(|a| a.command.clone()),
            "pcb": app.replies.pcb.is_some(),
        },
    })
}

/// The field view as `ui.inspect` shows it: what it shows, its clamp, the pinned point, the
/// colour bar's ticks and the peak it says.
fn field_inspect(app: &App) -> Value {
    let (v, f) = (&app.view3d, &app.view3d.field);
    let bar = v.shown.as_ref().and_then(|s| s.bar.as_ref());
    json!({
        "plane": f.plane.id(),
        "component": f.component.id(),
        "frequency": f.frequency,
        "phase": f.phase.to_degrees(),
        "mode": f.mode,
        "clamp": f.clamp,
        "pinned": app.pinned,
        "ticks": bar.map(|b| b.ticks.clone()),
        "peak": bar.and_then(|b| b.peak.as_ref()).map(|(value, _)| value.clone()),
        "error": v.field_error,
    })
}

impl App {
    /// The window after control request `req`, and what iced does next; the reply sent now,
    /// or later (a long command's job, a picture of the window).
    pub(crate) fn control(&mut self, req: Arc<ControlRequest>) -> Task<Message> {
        let p = &req.params;
        let reply = match req.method.as_str() {
            "engine.execute" => self.execute(&req),
            "engine.script" => self.script(&req),
            "engine.commands" => Some(ok(emcraft_ui_core::commands::catalogue())),
            "ui.inspect" => Some(ok(inspect(self))),
            "ui.handles" => Some(ok(json!(handles(self).into_iter().map(|(h, _)| h).collect::<Vec<_>>()))),
            "ui.click" => Some(match p.get("handle").and_then(Value::as_str) {
                Some(h) => match handles(self).into_iter().find(|(name, _)| name == h) {
                    // Pressed now: what it leaves to do later (a file dialog) follows on its own.
                    Some((_, m)) => {
                        let later = self.react(m);
                        let _ = req.reply.send(ok(Value::Null));
                        return later;
                    }
                    None => err(format!("no handle `{}` on screen (ui.handles lists them)", emcraft_engine::text::clip(h, 80))),
                },
                None => err("give `handle`: a name ui.handles lists"),
            }),
            "ui.set" => {
                // The start page and the settings as their buttons open them (the start page's
                // pictures drawn), after the rest is set.
                let reply = self.set(p);
                let set = reply["ok"] == true;
                let opened = ["settings", "start"].into_iter().filter(|_| set).filter_map(|k| Some((k, p.get(k)?.as_bool()?)));
                let later: Vec<Task<Message>> =
                    opened.map(|(k, on)| self.react(if k == "start" { Message::Home(on) } else { Message::Settings(on) })).collect();
                let reply = if later.is_empty() { reply } else { ok(inspect(self)) };
                let _ = req.reply.send(reply);
                return Task::batch(later);
            }
            "ui.point" => Some(self.point(p)),
            "ui.dialog" => Some(self.dialog_request(p)),
            "ui.record" => Some(self.record_request(p)),
            "ui.replay" => Some(self.replay_request(p)),
            "ui.crashes" => Some(self.crashes_request(p)),
            "ui.drop" => match p.get("path").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
                Some(path) => {
                    let later = self.react(Message::File(FileAction::Dropped(emcraft_engine::text::clip(path, 4096))));
                    let _ = req.reply.send(ok(Value::Null));
                    return later;
                }
                None => Some(err("give `path`: the file dropped on the window")),
            },
            "ui.cancel" => {
                self.cancel();
                Some(ok(Value::Null))
            }
            "ui.messages" => Some(ok(json!({
                "error": self.error,
                "messages": self.log.lines().iter().map(|l| json!({"text": l.text, "error": l.error, "count": l.count})).collect::<Vec<_>>(),
            }))),
            // The window drawn without it: on the CPU, the 3D view offscreen (crate::headless).
            "ui.render" => {
                let path = p.get("path").and_then(Value::as_str);
                let size = p.get("size").and_then(Value::as_array).and_then(|s| Some((s.first()?.as_f64()? as f32, s.get(1)?.as_f64()? as f32)));
                Some(match crate::headless::render(self, size.unwrap_or(self.window)) {
                    Ok((w, h, rgba)) => crate::app::save_picture(emcraft_render::Canvas { w: w as usize, h: h as usize, rgba }, path),
                    Err(e) => err(e),
                })
            }
            // The window as presented.
            "ui.screenshot" => {
                let path = p.get("path").and_then(Value::as_str).map(str::to_owned);
                let r = req.clone();
                return iced::window::latest()
                    .and_then(iced::window::screenshot)
                    .map(move |shot| Message::Replied(r.clone(), crate::app::save_screenshot(&shot, path.as_deref())));
            }
            other => Some(err(format!("unknown method `{}`", emcraft_engine::text::clip(other, 80)))),
        };
        if let Some(v) = reply {
            let _ = req.reply.send(v);
        }
        Task::none()
    }

    /// `engine.execute`: a quick command run now, a long one on the worker, answered when its
    /// job ends (`wait`, the default) or at once.
    fn execute(&mut self, req: &ControlRequest) -> Option<Value> {
        let p = &req.params;
        let Some(id) = p.get("command").and_then(Value::as_str) else { return Some(err("missing `command`")) };
        let params = p.get("params").cloned().filter(|v| !v.is_null()).unwrap_or_else(|| json!({}));
        if self.job.is_some() {
            return Some(err("a job is running: one at a time"));
        }
        if emcraft_ui_core::commands::is_long(id) {
            self.start(id, params);
            return match (self.job.as_mut(), p.get("wait").and_then(Value::as_bool).unwrap_or(true)) {
                (Some(job), true) => {
                    job.waiting.push(req.reply.clone());
                    None
                }
                (Some(_), false) => Some(ok(json!({"started": id}))),
                (None, _) => Some(err(self.error.clone().unwrap_or_default())),
            };
        }
        self.note_run(id, &params);
        Some(match self.session.execute(id, &params) {
            Ok(reply) => {
                self.error = None;
                self.after(id, &params, &reply);
                ok(reply)
            }
            Err(e) => err(e),
        })
    }

    /// `engine.script`: a JSON list of `{command, params}` run as one job on the worker.
    fn script(&mut self, req: &ControlRequest) -> Option<Value> {
        let Some(steps) = req.params.get("commands").and_then(Value::as_array).filter(|s| s.len() <= MAX_SCRIPT).cloned() else {
            return Some(err(format!("`commands` must be an array of at most {MAX_SCRIPT} {{command, params}}")));
        };
        if self.job.is_some() {
            return Some(err("a job is running: one at a time"));
        }
        // Noted as the commands it runs.
        let list =
            steps.iter().map(|s| (s["command"].as_str().unwrap_or_default().to_owned(), s.get("params").cloned().unwrap_or_else(|| json!({}))));
        self.note(emcraft_ui_core::action::Action::RunAll(list.collect()));
        let script = Value::Array(steps);
        let mut job =
            emcraft_ui_core::job::Job::spawn(&self.session, "engine.script", json!({}), None, move |s| s.run_script(&script).map(Value::Array));
        job.waiting.push(req.reply.clone());
        self.job = Some(job);
        None
    }

    /// `ui.set`: the page, the overlay, the settings or start page shown.
    fn set(&mut self, p: &Value) -> Value {
        let named = |k: &str| p.get(k).and_then(Value::as_str);
        if let Some(v) = named("page") {
            match View::parse(v) {
                Some(v) => self.update(Message::Page(v)),
                None => return err(format!("no page `{}`", emcraft_engine::text::clip(v, 40))),
            }
        }
        if let Some(o) = named("overlay") {
            match Overlay::parse(o) {
                Some(o) => self.update(Message::View3d(Nav::Overlay(o))),
                None => return err(format!("no overlay `{}`", emcraft_engine::text::clip(o, 40))),
            }
        }
        // A band picked on the S-parameter chart (display units), and a frequency clicked there.
        let fu = self.session.display().frequency;
        if let Some([a, b]) = p.get("band").and_then(Value::as_array).and_then(|v| Some([v.first()?.as_f64()?, v.get(1)?.as_f64()?])) {
            let (a, b) = (fu.to_si(a.min(b)), fu.to_si(a.max(b)));
            self.update(Message::S(crate::sparameters::SAct::Band((a.is_finite() && b.is_finite()).then_some((a, b)))));
        }
        if let Some(f) = p.get("pick").and_then(Value::as_f64).filter(|f| f.is_finite()) {
            self.update(Message::S(crate::sparameters::SAct::Pick(fu.to_si(f))));
        }
        // The open template card's parameters typed, by name.
        if let Some(fields) = p.get("template").and_then(Value::as_object) {
            for (k, v) in fields {
                let Some(i) = self.template.card.as_ref().and_then(|c| c.fields.iter().position(|f| f.name == *k)) else {
                    return err(format!("no template parameter `{}`", emcraft_engine::text::clip(k, 40)));
                };
                let text = v.as_str().map_or_else(|| v.to_string(), str::to_owned);
                self.update(Message::Template(crate::template_dialog::TemplateAct::Field(i, text)));
            }
        }
        // The command search typed (Enter: `search:enter`).
        if let Some(q) = p.get("search").and_then(Value::as_str) {
            self.update(Message::Search(q.to_owned()));
        }
        // The Setup form's fields typed, by name.
        if let Some(fields) = p.get("setup").and_then(Value::as_object) {
            for (k, v) in fields {
                let Some(name) = emcraft_ui_core::steps::form::NAMES.iter().chain(["touchstone"].iter()).find(|n| **n == k.as_str()) else {
                    return err(format!("no Setup field `{}`", emcraft_engine::text::clip(k, 40)));
                };
                let text = v.as_str().map_or_else(|| v.to_string(), str::to_owned);
                self.update(Message::StepAct(crate::step_panel::Act::Typed(name, text)));
            }
        }
        ok(inspect(self))
    }

    /// `ui.point`: the pointer at `at` (`[x, y]`, points from the 3D view's corner), what is
    /// under it found (a tool's face and snap, the field's values); with `click` it clicks there
    /// (`double`, `add` as Shift).
    fn point(&mut self, p: &Value) -> Value {
        let (w, h) = self.view3d.size;
        let at = p
            .get("at")
            .and_then(Value::as_array)
            .and_then(|a| Some(emcraft_ui_core::geom::Point::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32)))
            .filter(|q| q.is_finite() && (0.0..=w).contains(&q.x) && (0.0..=h).contains(&q.y));
        let Some(at) = at else { return err(format!("give `at`: [x, y] in points from the 3D view's corner, within {w} × {h}")) };
        let flag = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
        self.update(Message::Hover(Some(at)));
        if flag("click") {
            self.update(Message::Click(crate::view3d::Click { at, add: flag("add"), double: flag("double") }));
        }
        ok(inspect(self))
    }

    /// `ui.record`: recording begun (`on`: true) or ended (written to `path` when given; the
    /// recording replied).
    fn record_request(&mut self, p: &Value) -> Value {
        match p.get("on").and_then(Value::as_bool) {
            Some(true) => {
                self.start_recording();
                ok(json!({"recording": true}))
            }
            Some(false) => {
                let Some(r) = self.stop_recording() else { return err("nothing is being recorded") };
                let written = p.get("path").and_then(Value::as_str).map(|path| {
                    let text = serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?;
                    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", emcraft_engine::text::clip(path, 200)))
                });
                match written {
                    Some(Err(e)) => err(e),
                    _ => ok(json!({"actions": r.entries.len(), "dropped": r.dropped, "recording": r})),
                }
            }
            None => err("`on` must be true or false"),
        }
    }

    /// `ui.replay`: a recording played back, from `path` (a file or a crash report's folder) or
    /// `recording` itself.
    fn replay_request(&mut self, p: &Value) -> Value {
        let read = match (p.get("path").and_then(Value::as_str), p.get("recording")) {
            (Some(path), _) => emcraft_ui_core::record::Recording::read(std::path::Path::new(path)),
            (None, Some(r)) => emcraft_ui_core::record::Recording::parse(&r.to_string()),
            (None, None) => Err("give `path` or `recording`".into()),
        };
        match read.and_then(|r| self.play(r)) {
            Ok(v) => ok(v),
            Err(e) => err(e),
        }
    }

    /// `ui.crashes`: where crash reports go (`dir`: a folder, the journal begun there), and the
    /// reports there.
    fn crashes_request(&mut self, p: &Value) -> Value {
        if let Some(v) = p.get("dir") {
            let Some(dir) = v.as_str().filter(|d| !d.trim().is_empty() && d.len() <= 4096) else {
                return err("`dir` must be a folder's path (at most 4096 bytes)");
            };
            self.keep_crash_reports(dir.into(), false);
        }
        let reports = self.crashes.dir.as_deref().map(emcraft_ui_core::crash::list).unwrap_or_default();
        ok(json!({
            "dir": self.crashes.dir.as_ref().map(|d| d.display().to_string()),
            "journal": self.crashes.journal.as_ref().map(|j| j.path().display().to_string()),
            "reports": reports.iter().map(|r| json!({"id": r.id, "kind": r.kind, "message": r.message, "count": r.count, "seen": r.seen})).collect::<Vec<_>>(),
        }))
    }

    /// `ui.dialog`: a command's dialog opened (`open`), its fields set as typing would (`set`),
    /// then confirmed (`ok`) or closed (`cancel`).
    fn dialog_request(&mut self, p: &Value) -> Value {
        if let Some(id) = p.get("open").and_then(Value::as_str) {
            self.update(Message::Command(id.to_owned()));
        }
        if let (Some(values), Some(d)) = (p.get("set").and_then(Value::as_object), self.dialog.as_mut()) {
            for (k, v) in values {
                if let Err(e) = d.set(k, v) {
                    return err(e);
                }
            }
        }
        // A design's parameters typed in their rows (the import's), each as its text.
        for (k, v) in p.get("parameters").and_then(Value::as_object).into_iter().flatten() {
            let typed = v.as_str().map_or_else(|| v.to_string(), str::to_owned);
            let saved = crate::import::saved(self, k).unwrap_or_default();
            self.update(Message::ImportParameter(k.clone(), typed, saved));
        }
        if p.get("ok").and_then(Value::as_bool) == Some(true) {
            self.update(Message::Dialog(crate::dialog::Edit::Ok));
        }
        if p.get("cancel").and_then(Value::as_bool) == Some(true) {
            self.update(Message::Dialog(crate::dialog::Edit::Cancel));
        }
        ok(inspect(self))
    }
}

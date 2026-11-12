//! The control channel in the iced window (docs/control-protocol.md): agents and scenarios ask
//! it what it shows (`ui.inspect`), run commands (`engine.execute`, `engine.script`, a long one
//! answered when its job ends), change what it shows (`ui.set`), fill and confirm a dialog
//! (`ui.dialog`) and click what it names (`ui.click`, `ui.handles`). iced takes no input made up
//! by the app itself, so a click names what it presses — a handle — and the window does what
//! pressing it does: [`handles`] is every handle the window shows now with its message.

use std::sync::Arc;

use emcraft_control::ControlRequest;
use emcraft_ui_core::files::{Answer, FileAction, Then};
use emcraft_ui_core::ribbon::{self, TABS};
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
/// buttons, the ribbon's tabs and commands, the model page's overlays, views and commands, the
/// browser's rows, the selection's card, the open dialog's and question's buttons.
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
    let kind = app.session.model().analysis();
    let tabs = ribbon::tabs(kind).into_iter().map(|t| (format!("tab:{}", t.id()), Message::Tab(t)));
    let commands = TABS
        .iter()
        .filter(|(t, _)| *t == app.tab)
        .flat_map(|(_, panels)| panels.iter().flat_map(|(_, ids)| ids.iter()))
        .filter(|id| ribbon::fits(kind, id))
        .map(|id| (format!("command:{id}"), Message::Command((*id).to_owned())));
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
            ("toolbar:mesh.generate".to_owned(), Message::Start("mesh.generate".into())),
            ("toolbar:solve.run".to_owned(), Message::Start("solve.run".into())),
        ];
        // Each tool turned on, or off when on.
        let tools =
            emcraft_ui_core::tools::Tool::ALL.into_iter().map(|k| (format!("tool:{}", k.id()), Message::Tool((app.tool != Some(k)).then_some(k))));
        overlays.chain(views).chain(fixed).chain(tools).collect::<Vec<_>>()
    });
    let rows = app.rows().map(|r| (format!("row:{}", r.id), Message::Row(r.id.clone())));
    let steps = crate::steps::shown_bar(app)
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
        std::iter::once(import).chain(purposes).chain(templates).collect::<Vec<_>>()
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
    let card = (!app.selection.is_clear()).then(|| [("card:edit".to_owned(), Message::Edit), ("card:delete".to_owned(), Message::Delete)]);
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
        .chain(log)
        .collect()
}

/// What the window shows, for agents and scenarios.
pub fn inspect(app: &App) -> Value {
    let v = &app.view3d;
    let c = v.camera;
    let f = &v.field;
    json!({
        "project": {"name": app.session.name(), "dirty": app.session.is_dirty(), "path": app.session.path()},
        "page": app.page.id(),
        "settings": app.settings,
        "start": app.home,
        "tab": app.tab.id(),
        "error": app.error,
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
            "field": {"plane": f.plane.id(), "component": f.component.id(), "frequency": f.frequency, "phase": f.phase.to_degrees(), "mode": f.mode, "error": v.field_error},
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
            "ui.set" => Some(self.set(p)),
            "ui.point" => Some(self.point(p)),
            "ui.dialog" => Some(self.dialog_request(p)),
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
            "ui.screenshot" | "ui.render" => {
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
        let script = Value::Array(steps);
        let mut job =
            emcraft_ui_core::job::Job::spawn(&self.session, "engine.script", json!({}), None, move |s| s.run_script(&script).map(Value::Array));
        job.waiting.push(req.reply.clone());
        self.job = Some(job);
        None
    }

    /// `ui.set`: the page, the overlay, the ribbon's tab, the settings or start page shown.
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
        if let Some(t) = named("tab") {
            match ribbon::RibbonTab::parse(t) {
                Some(t) => self.update(Message::Tab(t)),
                None => return err(format!("no tab `{}`", emcraft_engine::text::clip(t, 40))),
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
        for (k, m) in [("settings", Message::Settings as fn(bool) -> Message), ("start", Message::Home)] {
            if let Some(on) = p.get(k).and_then(Value::as_bool) {
                self.update(m(on));
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

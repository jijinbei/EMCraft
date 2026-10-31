//! The job as a whole (docs/ux/workflow.md): its frequency band, where each step of the workflow
//! stands, an estimate of the solve, the project file (the model and the commands that built
//! it), undo and redo, and the results' exports and checks.

use std::collections::VecDeque;

use emcraft_model::units::Display;
use emcraft_model::{Band, Model};
use emcraft_solver::Complex64 as C;
use serde_json::{Value, json};

use crate::runs::Run;
use crate::{EngineError, Params, Result, Session, Step};

/// A state to undo to: the model, the display units and the history that built it, and —
/// before a command that replaced the project (`file.open`, `file.new`, `file.sample`) — the
/// project the model belonged to.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub model: Model,
    pub display: Display,
    pub history: Vec<Step>,
    pub variables: crate::expr::Variables,
    /// The project to go back to with the model, when the command after this state replaced it.
    pub project: Option<Box<Identity>>,
}

/// What a project is besides its model: its file, the sample it came from, the state it was
/// last saved or opened in, its runs and their results. Undoing a command that replaced the
/// project brings it back whole, so a save after the undo writes the project's own file (never
/// the one opened over it) and the dirty mark is the project's own.
#[derive(Clone, Debug)]
pub struct Identity {
    pub path: Option<String>,
    pub sample: Option<String>,
    /// The history as last saved or opened.
    pub saved: Vec<Step>,
    /// The runs as last saved or opened.
    pub saved_runs: Vec<Run>,
    pub runs: Vec<Run>,
    /// How its results were looked at, and as last saved or opened.
    pub views: crate::views::Views,
    pub saved_views: crate::views::Views,
    /// Its analysis history ([`crate::archive`]): the solves kept, the last id given, the one
    /// restored, and as last saved or opened.
    pub archive: Vec<crate::archive::Archived>,
    pub archive_next: u32,
    pub archive_current: Option<u32>,
    pub saved_archive: Vec<crate::archive::Mark>,
    pub results: crate::Results,
    /// The revision the results were solved at, and whether that was the model's revision then
    /// (they come back current; else stale).
    pub solved_revision: Option<u64>,
    pub current: bool,
}

/// The most states kept to undo to.
pub const UNDO_DEPTH: usize = 50;

/// Pushes a state to go back to, forgetting the oldest beyond [`UNDO_DEPTH`].
pub fn push_undo(stack: &mut VecDeque<Snapshot>, s: Snapshot) {
    stack.push_back(s);
    if stack.len() > UNDO_DEPTH {
        stack.pop_front();
    }
}

impl Session {
    /// The undoable state now, with `history` and the `project` to go back to.
    pub(crate) fn snapshot(&self, history: Vec<Step>, project: Option<Box<Identity>>) -> Snapshot {
        Snapshot { model: self.model.clone(), display: self.display, history, variables: self.variables.clone(), project }
    }

    /// The project the model belongs to, as it is now.
    pub(crate) fn identity(&self) -> Identity {
        Identity {
            path: self.path.clone(),
            sample: self.sample.clone(),
            saved: self.saved.clone(),
            saved_runs: self.saved_runs.clone(),
            runs: self.runs.clone(),
            views: self.views.clone(),
            saved_views: self.saved_views.clone(),
            archive: self.archive.clone(),
            archive_next: self.archive_next,
            archive_current: self.archive_current,
            saved_archive: self.saved_archive.clone(),
            results: self.results.clone(),
            solved_revision: self.solved_revision,
            current: self.solved_revision == Some(self.revision),
        }
    }

    /// The undoable state now, replaced by `with`; a new revision. The results stay, marked
    /// stale, unless `with` brings its project back: then that project's own come back.
    fn swap_snapshot(&mut self, with: Snapshot) -> Snapshot {
        let current = self.solved_revision == Some(self.revision);
        self.revision += 1;
        let project = with.project.map(|back| {
            let back = *back;
            let now = Identity {
                path: std::mem::replace(&mut self.path, back.path),
                sample: std::mem::replace(&mut self.sample, back.sample),
                saved: std::mem::replace(&mut self.saved, back.saved),
                saved_runs: std::mem::replace(&mut self.saved_runs, back.saved_runs),
                runs: std::mem::replace(&mut self.runs, back.runs),
                views: std::mem::replace(&mut self.views, back.views),
                saved_views: std::mem::replace(&mut self.saved_views, back.saved_views),
                archive: std::mem::replace(&mut self.archive, back.archive),
                archive_next: std::mem::replace(&mut self.archive_next, back.archive_next),
                archive_current: std::mem::replace(&mut self.archive_current, back.archive_current),
                saved_archive: std::mem::replace(&mut self.saved_archive, back.saved_archive),
                results: std::mem::replace(&mut self.results, back.results),
                solved_revision: self.solved_revision,
                current,
            };
            self.solved_revision = if back.current { Some(self.revision) } else { back.solved_revision };
            Box::new(now)
        });
        Snapshot {
            model: std::mem::replace(&mut self.model, with.model),
            display: std::mem::replace(&mut self.display, with.display),
            history: std::mem::replace(&mut self.history, with.history),
            variables: std::mem::replace(&mut self.variables, with.variables),
            project,
        }
    }
}

/// What a command does to the job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Changes the model: joins the history, can be undone.
    Record,
    /// Changes the model and brings its results along (a port's reference plane moved, its
    /// S-parameters re-referenced): as `Record`, the results current if they were.
    Adjust,
    /// Replaces the project (a sample, a project file, a new one): can be undone, back to the
    /// project before it (its file, its saved state, its runs and results), and brings its own
    /// history.
    Replace,
    /// Rebuilds the model from a history of its own making (a variable changed, an
    /// optimization's best point): can be undone; the project stays the same.
    Rebuild,
    /// Solves: its results belong to the model's current revision.
    Solve,
    /// Changes how the results are looked at (a chart's style, a probe): saved with the
    /// project, not in its history; no undo.
    View,
    None,
}

pub fn frequency(s: &mut Session, p: &Params) -> Result<Value> {
    let (start, stop) = (p.hertz("start").required()?, p.hertz("stop").required()?);
    s.model.set_band(Some(Band::new(start, stop)?));
    Ok(json!({"start": s.show_frequency(start.get()), "stop": s.show_frequency(stop.get()), "unit": s.display.frequency.symbol()}))
}

/// `project.analysis`: the kind of analysis the job is set up for.
pub fn analysis(s: &mut Session, p: &Params) -> Result<Value> {
    let kind: emcraft_model::Analysis = p.pick("kind").required()?;
    s.model.set_analysis(Some(kind));
    Ok(crate::workflow::analysis_reply(&s.model))
}

pub fn units(s: &mut Session, p: &Params) -> Result<Value> {
    if let Some(length) = p.choice("length").optional()? {
        s.display.length = length;
    }
    if let Some(frequency) = p.choice("frequency").optional()? {
        s.display.frequency = frequency;
    }
    Ok(crate::reply::units(s.display))
}

/// A template's parameters: the variables its script defines, in order, each with its value
/// and what it measures (`length`, `frequency` or none).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct TemplateParameter {
    pub name: String,
    pub value: f64,
    pub quantity: Option<String>,
}

/// The parameters of template `name` (`None`: no such template).
///
/// ```
/// let p = emcraft_engine::template_parameters("iris-bandpass").unwrap_or_default();
/// assert!(p.iter().any(|v| v.name == "irises" && v.value == 4.0 && v.quantity.is_none()));
/// assert!(emcraft_engine::template_parameters("nothing").is_none());
/// ```
pub fn template_parameters(name: &str) -> Option<Vec<TemplateParameter>> {
    let script: Vec<Step> = serde_json::from_str(emcraft_templates::find(name)?.script).ok()?;
    Some(
        script
            .iter()
            .filter(|st| st.command == crate::script::DEFINE)
            .filter_map(|st| {
                let name = st.param("name")?.as_str()?.to_owned();
                let value = st.param("value")?.as_f64()?;
                Some(TemplateParameter { name, value, quantity: st.param("quantity").and_then(Value::as_str).map(str::to_owned) })
            })
            .collect(),
    )
}

/// Template `name`'s script with its parameters set to `values` (name → number, in the display
/// units); an error naming a value that is not a finite number or a parameter the template does
/// not have.
///
/// ```
/// use serde_json::json;
/// let steps = emcraft_engine::template_steps("iris-bandpass", &json!({"irises": 5})).unwrap_or_default();
/// assert!(steps.iter().any(|s| s.defines("irises") && s.param("value") == Some(&json!(5.0))));
/// assert!(emcraft_engine::template_steps("iris-bandpass", &json!({"wings": 2})).is_err());
/// assert!(emcraft_engine::template_steps("iris-bandpass", &json!({"irises": "many"})).is_err());
/// ```
pub fn template_steps(name: &str, values: &Value) -> Result<Vec<Step>> {
    let template = emcraft_templates::find(name).ok_or_else(|| {
        EngineError::Params(crate::text::unknown(
            format_args!("names no sample: `{}`", crate::text::clip(name, 40)),
            "known: ",
            emcraft_templates::names(),
        ))
    })?;
    let mut script: Vec<Step> = serde_json::from_str(template.script).map_err(|e| EngineError::failed(format!("sample {name}: {e}")))?;
    let given = match values {
        Value::Null => return Ok(script),
        Value::Object(map) => map,
        _ => return Err(EngineError::Params("values: must be an object of numbers by parameter".into())),
    };
    if given.len() > crate::study::MAX_VARIABLES {
        return Err(EngineError::Params(format!("values: at most {} parameters", crate::study::MAX_VARIABLES)));
    }
    for (key, v) in given {
        let number = v
            .as_f64()
            .filter(|x| x.is_finite())
            .ok_or_else(|| EngineError::Params(format!("values.{}: must be a finite number", crate::text::clip(key, crate::expr::MAX_NAME))))?;
        let mut found = false;
        for params in script.iter_mut().filter(|st| st.defines(key)).filter_map(|st| st.params.as_object_mut()) {
            params.insert("value".to_owned(), json!(number));
            found = true;
        }
        if !found {
            let known: Vec<String> = template_parameters(name).unwrap_or_default().into_iter().map(|p| p.name).collect();
            return Err(EngineError::Params(crate::text::unknown(
                format_args!("values: `{}` is no parameter of {name}", crate::text::clip(key, crate::expr::MAX_NAME)),
                "its parameters: ",
                known,
            )));
        }
    }
    Ok(script)
}

/// Where a template's parameter shows on its model: a dimension from `from` to `to` (m).
#[derive(Clone, Debug, PartialEq)]
pub struct TemplateMark {
    pub name: String,
    pub from: [f64; 3],
    pub to: [f64; 3],
}

/// Where template `name`'s parameters show on its model built with `values` (as
/// [`template_steps`] takes them), each a dimension in metres, in the template's order of
/// parameters; the default display units (mm) read its coordinates.
///
/// ```
/// use serde_json::json;
/// let marks = emcraft_engine::template_marks("iris-bandpass", &json!({"w_end": 14})).unwrap_or_default();
/// let w = marks.iter().find(|m| m.name == "w_end").map(|m| m.to[0] - m.from[0]);
/// assert!(w.is_some_and(|w| (w - 14e-3).abs() < 1e-12));
/// assert!(emcraft_engine::template_marks("iris-bandpass", &json!({"wings": 1})).is_err());
/// ```
pub fn template_marks(name: &str, values: &Value) -> Result<Vec<TemplateMark>> {
    let steps = template_steps(name, values)?;
    let template = emcraft_templates::find(name).ok_or_else(|| EngineError::failed(format!("no template `{name}`")))?;
    // The parameters' values, each read in order as the script defines it.
    let mut vars = crate::expr::Variables::new();
    for st in steps.iter().filter(|st| st.command == crate::script::DEFINE) {
        if let (Some(n), Some(v)) = (st.param("name").and_then(Value::as_str), st.param("value")) {
            let x = match v {
                Value::String(e) => crate::expr::body(e).ok_or_else(|| EngineError::failed(format!("{n}: not an expression")))?.to_owned(),
                v => v.to_string(),
            };
            let value = crate::expr::eval(&x, &vars).map_err(|e| EngineError::failed(format!("{n}: {e}")))?;
            vars.insert(n.to_owned(), value);
        }
    }
    let marks: serde_json::Map<String, Value> =
        serde_json::from_str(template.marks).map_err(|e| EngineError::failed(format!("marks of {name}: {e}")))?;
    let l = Display::default().length;
    let point = |v: &Value| -> Result<[f64; 3]> {
        let list = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| EngineError::failed(format!("marks of {name}: a point is [x, y, z]")))?;
        let mut out = [0.0; 3];
        for (o, c) in out.iter_mut().zip(list) {
            let x = match c {
                Value::String(e) => crate::expr::body(e).map_or_else(|| Err(format!("`{e}` is no expression")), |b| crate::expr::eval(b, &vars)),
                c => c.as_f64().ok_or_else(|| format!("{c} is no number")),
            }
            .map_err(|e| EngineError::failed(format!("marks of {name}: {e}")))?;
            *o = l.to_si(x);
        }
        Ok(out)
    };
    template_parameters(name)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| marks.get(&p.name).map(|m| Ok(TemplateMark { from: point(&m["from"])?, to: point(&m["to"])?, name: p.name })))
        .collect()
}

/// `file.templates`: the templates, each with its label and parameters.
pub fn templates(s: &mut Session, _: &Params) -> Result<Value> {
    let d = s.display;
    let list: Vec<Value> = emcraft_templates::TEMPLATES
        .iter()
        .map(|t| {
            let parameters: Vec<Value> = template_parameters(t.name)
                .unwrap_or_default()
                .into_iter()
                .map(|p| {
                    let unit = match p.quantity.as_deref() {
                        Some("length") => d.length.symbol(),
                        Some("frequency") => d.frequency.symbol(),
                        _ => "",
                    };
                    json!({"name": p.name, "value": p.value, "quantity": p.quantity, "unit": unit})
                })
                .collect();
            json!({"name": t.name, "label": t.label, "parameters": parameters})
        })
        .collect();
    Ok(json!({"templates": list}))
}

/// Replaces the session with a sample's model (and its results, when `solve`), its parameters
/// set to `values`.
pub fn sample(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    let solve = p.flag("solve").or(false)?;
    let script = template_steps(name, p.value().get("values").unwrap_or(&Value::Null)).map_err(|e| match e {
        EngineError::Params(m) if m.starts_with("names no sample") => p.error("name", m),
        e => e,
    })?;
    let mut fresh = s.fresh();
    // Without `solve`, the sample's solves are recorded as its runs, not run.
    for (i, step) in script.iter().enumerate() {
        crate::progress::report(i, script.len(), &step.command)?;
        let spec =
            crate::find_command(&step.command).ok_or_else(|| EngineError::failed(format!("sample {name}: unknown command `{}`", step.command)))?;
        if spec.effect == Effect::Solve && !solve {
            let run = Run { command: step.command.clone(), params: step.params.clone(), units: fresh.display };
            crate::runs::record(&mut fresh.runs, run);
        } else {
            fresh.execute(&step.command, &step.params).map_err(|e| e.in_step(i + 1, &step.command))?;
        }
    }
    // The kind of analysis the job was chosen for (the start page's purpose).
    if let Some(kind) = p.string("analysis").optional()? {
        fresh.execute("project.analysis", &json!({"kind": kind})).map_err(|e| p.error("analysis", e))?;
    }
    fresh.mark_saved();
    fresh.sample = Some(name.to_owned());
    // Undo goes back to before the sample, not through the steps that built it.
    fresh.undo.clear();
    s.replace_with(fresh);
    Ok(s.summary())
}

pub fn summary(s: &mut Session, _: &Params) -> Result<Value> {
    Ok(s.summary())
}

/// Where each step of the workflow stands ([`crate::workflow`]); no solve.
pub fn status(s: &mut Session, _: &Params) -> Result<Value> {
    let not_needed: Vec<&str> = crate::workflow::not_needed(s.model.analysis).iter().map(|x| x.id()).collect();
    let mut reply = json!({
        "steps": crate::workflow::steps(s),
        "revision": s.revision,
        "analysis": crate::workflow::analysis_reply(&s.model),
        "not_needed": not_needed,
    });
    if let Some(b) = s.model.band {
        reply["frequency"] =
            json!({"start": s.show_frequency(b.start.get()), "stop": s.show_frequency(b.stop.get()), "unit": s.display.frequency.symbol()});
    }
    Ok(reply)
}

/// A project file (docs/project-file.md): the history that builds the model, the runs solved
/// on it (their parameters, not their results), and the model for other tools to read (opening
/// replays the history and does not read it).
#[derive(serde::Serialize, serde::Deserialize)]
struct ProjectFile<'m> {
    format: String,
    #[serde(default = "first_version")]
    version: u32,
    #[serde(default)]
    history: Vec<Step>,
    /// Since version 2.
    #[serde(default)]
    runs: Vec<Run>,
    /// Since version 3: how the results are looked at.
    #[serde(default, skip_serializing_if = "crate::views::Views::is_empty")]
    views: crate::views::Views,
    /// Since version 4: the analysis history (its solves' identities and results, not their
    /// models: [`crate::archive::file::StoredHistory`]), read only from a file of that version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    analysis_history: Option<Value>,
    #[serde(skip_deserializing)]
    model: Option<&'m Model>,
}

/// What a project file's `format` says.
const FORMAT: &str = "emcraft-project";

/// The newest version of project files read (2: the runs; 3: the views; 4: the analysis
/// history). A project is written in the oldest version that holds it: without an analysis
/// history as version 3, without views either as version 2, which older EMCraft reads.
const VERSION: u32 = 4;
const VERSION_WITHOUT_HISTORY: u32 = 3;
const VERSION_WITHOUT_VIEWS: u32 = 2;

fn first_version() -> u32 {
    1
}

/// Most bytes of a project file read.
const MAX_PROJECT_BYTES: u64 = 64 << 20;

/// The extension of project files.
pub const EXTENSION: &str = "emcraft";

/// `file.save`: the project to `path` (or its own file); with `copy`, a copy that leaves the
/// session's file, name and saved state alone (an autosave).
pub fn save(s: &mut Session, p: &Params) -> Result<Value> {
    let copy = p.flag("copy").or(false)?;
    let path = match p.string("path").optional()? {
        Some(path) if path.trim().is_empty() => return Err(p.error("path", "must not be empty")),
        Some(path) if std::path::Path::new(path).extension().is_none() => format!("{path}.{EXTENSION}"),
        Some(path) => path.to_owned(),
        None if copy => return Err(p.error("path", "is required for a copy")),
        None => s.path.clone().ok_or_else(|| p.error("path", "is required: the project has no file yet"))?,
    };
    let version = match (s.archive.is_empty(), s.views.is_empty()) {
        (false, _) => VERSION,
        (true, false) => VERSION_WITHOUT_HISTORY,
        (true, true) => VERSION_WITHOUT_VIEWS,
    };
    let analysis_history = match s.archive.is_empty() {
        true => None,
        false => Some(serde_json::to_value(crate::archive::file::stored(&s.archive, s.archive_next)).map_err(EngineError::failed)?),
    };
    let file = ProjectFile {
        format: FORMAT.to_owned(),
        version,
        history: s.history.clone(),
        runs: s.runs.clone(),
        views: s.views.clone(),
        analysis_history,
        model: Some(&s.model),
    };
    // Through a JSON value, whose objects keep their keys sorted: the file's text does not
    // depend on the order the fields are declared in.
    let doc = serde_json::to_value(&file).map_err(EngineError::failed)?;
    let text = pretty(&doc)?;
    write_atomically(&path, text.as_bytes())?;
    let kept = s.archive.len();
    if copy {
        return Ok(json!({"path": path, "copy": true, "commands": s.history.len(), "runs": s.runs.len(), "kept": kept}));
    }
    s.path = Some(path.clone());
    s.mark_saved();
    Ok(json!({"path": path, "name": s.name(), "commands": s.history.len(), "runs": s.runs.len(), "kept": kept}))
}

/// `doc` as a project file writes it: pretty printed, two spaces a level, with each list of
/// numbers on one line (an S matrix, a point), so a kept sweep's thousands of numbers do not
/// take a line each.
///
/// ```
/// let text = emcraft_engine::project::pretty(&serde_json::json!({"b": [1, 2.5], "a": [{"c": []}]}))?;
/// assert_eq!(text, "{\n  \"a\": [\n    {\n      \"c\": []\n    }\n  ],\n  \"b\": [1, 2.5]\n}");
/// # Ok::<(), emcraft_engine::EngineError>(())
/// ```
pub fn pretty(doc: &Value) -> Result<String> {
    fn pad(out: &mut String, depth: usize) {
        out.extend(std::iter::repeat_n("  ", depth));
    }
    fn write(v: &Value, depth: usize, out: &mut String) -> Result<()> {
        match v {
            Value::Array(list) if !list.is_empty() && list.iter().all(Value::is_number) => {
                let items: Vec<String> = list.iter().map(Value::to_string).collect();
                out.push('[');
                out.push_str(&items.join(", "));
                out.push(']');
            }
            Value::Array(list) if !list.is_empty() => {
                out.push_str("[\n");
                for (i, x) in list.iter().enumerate() {
                    pad(out, depth + 1);
                    write(x, depth + 1, out)?;
                    out.push_str(if i + 1 < list.len() { ",\n" } else { "\n" });
                }
                pad(out, depth);
                out.push(']');
            }
            Value::Object(map) if !map.is_empty() => {
                out.push_str("{\n");
                for (i, (k, x)) in map.iter().enumerate() {
                    pad(out, depth + 1);
                    out.push_str(&serde_json::to_string(k).map_err(EngineError::failed)?);
                    out.push_str(": ");
                    write(x, depth + 1, out)?;
                    out.push_str(if i + 1 < map.len() { ",\n" } else { "\n" });
                }
                pad(out, depth);
                out.push('}');
            }
            v => out.push_str(&serde_json::to_string(v).map_err(EngineError::failed)?),
        }
        Ok(())
    }
    let mut out = String::new();
    write(doc, 0, &mut out)?;
    Ok(out)
}

/// Writes `bytes` to a temporary file beside `path` and renames it over: a crash or a full
/// disk leaves the old file whole.
fn write_atomically(path: &str, bytes: &[u8]) -> Result<()> {
    let target = std::path::Path::new(path);
    let temp = target.with_extension(format!("{}.tmp", target.extension().and_then(|e| e.to_str()).unwrap_or(EXTENSION)));
    let fail = |e: std::io::Error| EngineError::cannot_write(path, e);
    std::fs::write(&temp, bytes).map_err(fail)?;
    std::fs::rename(&temp, target).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        fail(e)
    })
}

/// An empty project with the session's display units.
pub fn new(s: &mut Session, _: &Params) -> Result<Value> {
    let mut fresh = s.fresh();
    if s.display != fresh.display {
        fresh.execute("units.set", &crate::reply::units(s.display))?;
        fresh.mark_saved();
    }
    s.replace_with(fresh);
    Ok(s.summary())
}

/// What a project file holds besides its model: the history, the runs, the views and the
/// analysis history (its solves and the last id given).
struct Stored {
    history: Vec<Step>,
    runs: Vec<Run>,
    views: crate::views::Views,
    archive: Vec<crate::archive::Archived>,
    archive_next: u32,
}

/// A project file read and checked.
fn read_project(path: &str) -> Result<Stored> {
    let bytes = crate::geometry::read_capped(path, MAX_PROJECT_BYTES)?;
    let doc: Value = serde_json::from_slice(&bytes).map_err(|e| EngineError::failed(format!("`{path}`: not a project file ({e})")))?;
    if doc.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(EngineError::failed(format!("`{path}`: not an EMCraft project")));
    }
    let file: ProjectFile<'_> = serde_json::from_value(doc).map_err(|e| EngineError::failed(format!("`{path}`: not a project file ({e})")))?;
    if file.version > VERSION {
        return Err(EngineError::failed(format!(
            "`{path}`: a project of version {}, newer than this EMCraft reads ({VERSION}): open it with a newer EMCraft",
            file.version
        )));
    }
    let runs = crate::runs::checked(file.runs).map_err(|e| EngineError::failed(format!("`{path}`: {e}")))?;
    let views = file.views.checked().map_err(|e| EngineError::failed(format!("`{path}`: {e}")))?;
    // Older versions hold no analysis history.
    let (archive, archive_next) = match file.analysis_history.filter(|_| file.version >= VERSION) {
        Some(h) => {
            let h: crate::archive::file::StoredHistory =
                serde_json::from_value(h).map_err(|e| EngineError::failed(format!("`{path}`: not a project file ({e})")))?;
            crate::archive::file::read(h).map_err(|e| EngineError::failed(format!("`{path}`: {e}")))?
        }
        None => (Vec::new(), 0),
    };
    Ok(Stored { history: file.history, runs, views, archive, archive_next })
}

/// Opens a project: its history replayed in a fresh session (so the mesh comes back the same),
/// its runs restored without running them. With `original`, `path` is a copy of that project
/// (an autosave being recovered): the session takes the original's file, and is clean only
/// where it matches what that file holds.
pub fn open(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let original = match p.string("original").optional()? {
        Some(o) if o.trim().is_empty() => return Err(p.error("original", "must not be empty")),
        o => o,
    };
    let stored = read_project(path)?;
    let mut fresh = s.fresh();
    fresh.run_steps(&stored.history)?;
    // Undo inside the opened project, then back to before it.
    fresh.undo.clear();
    fresh.runs = stored.runs;
    fresh.views = stored.views;
    fresh.archive = stored.archive;
    fresh.archive_next = stored.archive_next;
    match original {
        None => {
            fresh.path = Some(path.to_owned());
            fresh.mark_saved();
        }
        Some(o) => {
            // The original as it was last saved: the copy may be ahead of it. One that cannot be
            // read is unlike anything recovered (a baseline no history equals).
            let saved = read_project(o).unwrap_or_else(|_| Stored {
                history: vec![Step::new(UNREADABLE, json!({}))],
                runs: Vec::new(),
                views: crate::views::Views::default(),
                archive: Vec::new(),
                archive_next: 0,
            });
            fresh.path = Some(o.to_owned());
            fresh.saved = saved.history;
            fresh.saved_runs = saved.runs;
            fresh.saved_views = saved.views;
            fresh.saved_archive = crate::archive::marks(&saved.archive);
        }
    }
    s.replace_with(fresh);
    Ok(
        json!({"path": s.path, "name": s.name(), "commands": s.history.len(), "runs": s.runs.len(), "kept": s.archive.len(), "dirty": s.is_dirty(), "summary": s.summary()}),
    )
}

/// The saved baseline of a recovered project whose original cannot be read: no command has this
/// id, so no history equals it.
const UNREADABLE: &str = "(the original could not be read)";

pub fn undo(s: &mut Session, _: &Params) -> Result<Value> {
    let back = s.undo.pop_back().ok_or_else(|| EngineError::failed("nothing to undo"))?;
    let now = s.swap_snapshot(back);
    push_undo(&mut s.redo, now);
    Ok(stacks(s))
}

pub fn redo(s: &mut Session, _: &Params) -> Result<Value> {
    let next = s.redo.pop_back().ok_or_else(|| EngineError::failed("nothing to redo"))?;
    let now = s.swap_snapshot(next);
    push_undo(&mut s.undo, now);
    Ok(stacks(s))
}

/// Undo's and redo's reply: the states left on each side and the project now.
fn stacks(s: &Session) -> Value {
    json!({"undo": s.undo.len(), "redo": s.redo.len(), "path": s.path, "name": s.name(), "dirty": s.is_dirty()})
}

emcraft_macros::named_enum! {
    /// How a Touchstone file writes each complex value (`format`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Format {
        /// Real and imaginary parts.
        Ri = "ri",
        /// Magnitude and angle in degrees.
        Ma = "ma",
    }
}

emcraft_macros::named_serde!(Format);

/// The sweep's S-parameters as a Touchstone (version 1) file: frequencies in GHz, `ri` (real,
/// imaginary) or `ma` (magnitude, angle in degrees), the reference impedance on the option line.
pub fn touchstone(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let ma = p.choice("format").or(Format::Ri)? == Format::Ma;
    let sw = s.results.sweep()?;
    let entries = sw.entries();
    let n = entries.len();
    let refs: Vec<Option<f64>> = entries.iter().map(|e| e.reference).collect();
    let one = refs.first().copied().flatten().filter(|r| refs.iter().all(|x| *x == Some(*r)));
    let mut out = String::new();
    out.push_str("! EMCraft S-parameters\n");
    out.push_str(&format!("! ports: {}\n", entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>().join(", ")));
    if one.is_none() {
        out.push_str("! power waves of the ports' modes (no common reference impedance): R 50 is nominal\n");
    }
    out.push_str(&format!("# GHz S {} R {}\n", if ma { "MA" } else { "RI" }, one.unwrap_or(50.0)));
    let pair = |c: C| if ma { format!("{:.12e} {:.12e}", c.norm(), c.arg().to_degrees()) } else { format!("{:.12e} {:.12e}", c.re, c.im) };
    for r in &sw.results {
        let at = |i: usize, j: usize| r.s.get(i, j).unwrap_or_default();
        let f = format!("{:.12e}", r.frequency / 1e9);
        match n {
            1 => out.push_str(&format!("{f} {}\n", pair(at(0, 0)))),
            // Version 1's two-port order: S11 S21 S12 S22.
            2 => out.push_str(&format!("{f} {} {} {} {}\n", pair(at(0, 0)), pair(at(1, 0)), pair(at(0, 1)), pair(at(1, 1)))),
            _ => {
                // A row of the matrix per line, at most four values to a line (continued).
                for i in 0..n {
                    for (k, chunk) in (0..n).map(|j| pair(at(i, j))).collect::<Vec<_>>().chunks(4).enumerate() {
                        let lead = if i == 0 && k == 0 { format!("{f} ") } else { String::new() };
                        out.push_str(&format!("{lead}{}\n", chunk.join(" ")));
                    }
                }
            }
        }
    }
    std::fs::write(path, out).map_err(|e| EngineError::cannot_write(path, e))?;
    Ok(json!({"path": path, "ports": n, "frequencies": sw.results.len()}))
}

/// The sweep's physics checks: passivity (the largest singular value of S, ≤ 1 for a passive
/// structure), reciprocity (max |S − Sᵀ|) and unitarity (max |SᴴS − 1|, 0 for a lossless
/// structure), the worst over the frequencies.
pub fn check(s: &mut Session, _: &Params) -> Result<Value> {
    let sw = s.results.sweep()?;
    let worst = |f: fn(&emcraft_solver::SMatrix) -> f64| sw.results.iter().map(|r| f(&r.s)).fold(0.0, f64::max);
    let gain = worst(emcraft_solver::SMatrix::largest_singular_value);
    let asym = worst(emcraft_solver::SMatrix::reciprocity_error);
    let lossless = worst(emcraft_solver::SMatrix::unitarity_error);
    Ok(
        json!({"largest_singular_value": gain, "passive": gain <= 1.0 + 1e-9, "reciprocity_error": asym, "unitarity_error": lossless, "frequencies": sw.results.len()}),
    )
}

//! EMCraft's command engine. Everything a user can do is a command ([`CommandSpec`]: a dotted,
//! lower-case id, a label, documented parameters and a JSON `run`); the desktop UI, scripts and
//! the command line all reach the same commands through [`Session::execute`], which checks the
//! parameters, runs the command under a panic guard and restores the session when it fails.
//!
//! Parameters and replies are in the session's display units (millimetres and gigahertz by
//! default, [`emcraft_model::units`]); the model and the solvers are in SI.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod cmd;
pub mod params;
pub mod sample;
pub mod view;

use std::sync::Arc;

use emcraft_fem::Order;
use emcraft_model::Model;
use emcraft_model::units::Display;
use emcraft_solver::{Driven, FrequencyResult};
use serde_json::Value;

pub use cmd::COMMANDS;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("{0}")]
    Params(String),
    #[error("{0}")]
    Failed(String),
    #[error("internal error in `{0}` (the session was restored): please report it")]
    Panicked(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

impl From<emcraft_model::ModelError> for EngineError {
    fn from(e: emcraft_model::ModelError) -> Self {
        EngineError::Failed(e.to_string())
    }
}

impl From<emcraft_mesh::MeshError> for EngineError {
    fn from(e: emcraft_mesh::MeshError) -> Self {
        EngineError::Failed(e.to_string())
    }
}

impl From<emcraft_fdtd::FdtdError> for EngineError {
    fn from(e: emcraft_fdtd::FdtdError) -> Self {
        EngineError::Failed(e.to_string())
    }
}

impl From<emcraft_solver::SolveError> for EngineError {
    fn from(e: emcraft_solver::SolveError) -> Self {
        EngineError::Failed(e.to_string())
    }
}

/// The physical quantity of a numeric parameter, which decides its display unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantity {
    /// A plain number.
    None,
    Length,
    Frequency,
}

/// A command parameter.
#[derive(Clone, Copy, Debug)]
pub struct ParamSpec {
    pub name: &'static str,
    /// `number`, `integer`, `string`, `array`, `object` or `boolean`.
    pub kind: &'static str,
    pub quantity: Quantity,
    pub required: bool,
    pub doc: &'static str,
}

/// A command.
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub doc: &'static str,
    pub params: &'static [ParamSpec],
    pub run: fn(&mut Session, &Value) -> Result<Value>,
}

/// The most eigenmodes one command may ask for.
pub(crate) fn eigen_max() -> u64 {
    emcraft_solver::eigen::MAX_MODES as u64
}

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// A frequency sweep's results.
#[derive(Clone, Debug)]
pub struct Sweep {
    pub order: Order,
    pub driven: Arc<Driven>,
    /// Ascending frequency.
    pub results: Vec<FrequencyResult>,
}

impl Sweep {
    /// The result at a frequency (Hz) solved within 10⁻⁹ of it, or the nearest when `nearest`.
    pub fn at(&self, frequency: f64, nearest: bool) -> Option<&FrequencyResult> {
        let best = self.results.iter().min_by(|a, b| (a.frequency - frequency).abs().total_cmp(&(b.frequency - frequency).abs()))?;
        if nearest || (best.frequency - frequency).abs() <= 1e-9 * frequency.abs() { Some(best) } else { None }
    }
}

/// Limits on the work one command may ask for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// Frequencies in one sweep.
    pub frequencies: usize,
    /// Pixels on a picture's side.
    pub picture_side: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { frequencies: emcraft_solver::driven::MAX_FREQUENCIES, picture_side: 4096 }
    }
}

/// A session: the model, its results and the display units.
#[derive(Clone, Debug)]
pub struct Session {
    pub model: Model,
    pub sweep: Option<Sweep>,
    /// The cavity's eigenmodes from `solve.eigenmodes`.
    pub eigen: Option<Arc<emcraft_solver::eigen::Eigen>>,
    /// The time-domain solver's S-parameters from `solve.time_domain`.
    pub time_domain: Option<emcraft_fdtd::TdResult>,
    pub display: Display,
    pub limits: Limits,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

impl Session {
    pub fn new() -> Session {
        Session { model: Model::new(), sweep: None, eigen: None, time_domain: None, display: Display::default(), limits: Limits::default() }
    }

    /// Runs a command. Unknown parameters are refused; a failed or panicking command leaves the
    /// session as it was.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        // An id from hostile input is not echoed in full.
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.chars().take(100).collect()))?;
        check_keys(spec, params)?;
        let before = (self.model.clone(), self.sweep.clone(), self.eigen.clone(), self.time_domain.clone(), self.display);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (spec.run)(self, params)));
        match outcome {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => {
                (self.model, self.sweep, self.eigen, self.time_domain, self.display) = before;
                Err(e)
            }
            Err(_) => {
                (self.model, self.sweep, self.eigen, self.time_domain, self.display) = before;
                Err(EngineError::Panicked(spec.id.to_owned()))
            }
        }
    }

    /// Runs a script: a JSON array of `{"command", "params"}` steps; stops at the first error.
    /// Returns each step's reply.
    pub fn run_script(&mut self, script: &Value) -> Result<Vec<Value>> {
        let steps = script.as_array().ok_or_else(|| EngineError::Params("a script is a JSON array of {\"command\", \"params\"} steps".into()))?;
        let mut out = Vec::with_capacity(steps.len());
        for (i, step) in steps.iter().enumerate() {
            let id = step.get("command").and_then(Value::as_str).ok_or_else(|| EngineError::Params(format!("step {}: no `command`", i + 1)))?;
            let params = step.get("params").cloned().unwrap_or_else(|| Value::Object(Default::default()));
            let reply = self.execute(id, &params).map_err(|e| EngineError::Failed(format!("step {} ({id}): {e}", i + 1)))?;
            out.push(reply);
        }
        Ok(out)
    }

    /// A short summary of the model and results.
    pub fn summary(&self) -> Value {
        let l = self.display.length;
        let w = self.model.waveguide.as_ref().map(|w| {
            serde_json::json!({
                "width": l.from_si(w.width), "height": l.from_si(w.height), "length": l.from_si(w.length()),
                "sections": w.sections.iter().enumerate().map(|(i, s)| {
                    let (x0, a) = w.aperture(i).unwrap_or((0.0, w.width));
                    serde_json::json!({"length": l.from_si(s.length), "material": s.material, "aperture": [l.from_si(x0), l.from_si(a)]})
                }).collect::<Vec<_>>(),
            })
        });
        serde_json::json!({
            "units": {"length": l.symbol(), "frequency": self.display.frequency.symbol()},
            "materials": self.model.materials.iter().map(|m| serde_json::json!({"name": m.name, "eps_r": m.eps_r, "mu_r": m.mu_r})).collect::<Vec<_>>(),
            "waveguide": w,
            "ports": self.model.ports.iter().map(|p| serde_json::json!({"name": p.name, "end": p.end, "modes": p.modes})).collect::<Vec<_>>(),
            "mesh": self.model.mesh.as_ref().map(|m| serde_json::json!({"nodes": m.nodes.len(), "tetrahedra": m.tets.len()})),
            "results": self.sweep.as_ref().map(|s| serde_json::json!({"order": s.order.as_u8(), "unknowns": s.driven.dofs.unknowns, "frequencies": s.results.len()})),
            "eigenmodes": self.eigen.as_ref().map(|e| e.modes.len()),
        })
    }
}

fn check_keys(spec: &CommandSpec, params: &Value) -> Result<()> {
    let obj = match params {
        Value::Object(o) => o,
        Value::Null => return Ok(()),
        _ => return Err(EngineError::Params(format!("{}: parameters must be a JSON object", spec.id))),
    };
    for k in obj.keys() {
        if !spec.params.iter().any(|p| p.name == k) {
            let known: Vec<&str> = spec.params.iter().map(|p| p.name).collect();
            return Err(EngineError::Params(format!(
                "{}: unknown parameter `{}` (known: {})",
                spec.id,
                k.chars().take(64).collect::<String>(),
                known.join(", ")
            )));
        }
    }
    for p in spec.params.iter().filter(|p| p.required) {
        if !obj.contains_key(p.name) {
            return Err(EngineError::Params(format!("{}: missing parameter `{}`", spec.id, p.name)));
        }
    }
    Ok(())
}

/// Every command as a Markdown page (docs/features.md).
pub fn features_markdown() -> String {
    let mut s = String::from(
        "# Commands\n\nGenerated by `cargo xtask features` (`emcraft-cli features`). Lengths are in millimetres and frequencies in gigahertz unless the session's display units say otherwise.\n",
    );
    for c in COMMANDS {
        s.push_str(&format!("\n## `{}` — {}\n\n{}\n\n", c.id, c.label, c.doc));
        if c.params.is_empty() {
            s.push_str("No parameters.\n");
            continue;
        }
        s.push_str("| Parameter | Type | Unit | Required | Description |\n|---|---|---|---|---|\n");
        for p in c.params {
            let unit = match p.quantity {
                Quantity::None => "",
                Quantity::Length => "length",
                Quantity::Frequency => "frequency",
            };
            s.push_str(&format!("| `{}` | {} | {} | {} | {} |\n", p.name, p.kind, unit, if p.required { "yes" } else { "no" }, p.doc));
        }
    }
    s
}

#[cfg(test)]
mod tests;

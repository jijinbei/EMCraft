//! The steps of an electromagnetic job (docs/ux/workflow.md §1) and where each stands: worked
//! out from the model, the mesh and the results, without solving. `project.status` replies
//! with them; the desktop app's step bar reads them directly.

use std::fmt;
use std::str::FromStr;

use emcraft_model::units::C0;
use serde::{Deserialize, Serialize};

use crate::Session;

/// A step of the workflow, in the order a job goes through them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    Intent,
    Geometry,
    Materials,
    Excitation,
    Boundaries,
    Mesh,
    Setup,
    Run,
    Results,
}

impl Step {
    pub const ALL: [Step; 9] =
        [Step::Intent, Step::Geometry, Step::Materials, Step::Excitation, Step::Boundaries, Step::Mesh, Step::Setup, Step::Run, Step::Results];

    pub const fn id(self) -> &'static str {
        match self {
            Step::Intent => "intent",
            Step::Geometry => "geometry",
            Step::Materials => "materials",
            Step::Excitation => "excitation",
            Step::Boundaries => "boundaries",
            Step::Mesh => "mesh",
            Step::Setup => "setup",
            Step::Run => "run",
            Step::Results => "results",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Step::Intent => "Intent",
            Step::Geometry => "Geometry",
            Step::Materials => "Materials",
            Step::Excitation => "Excitation",
            Step::Boundaries => "Boundaries",
            Step::Mesh => "Mesh",
            Step::Setup => "Setup",
            Step::Run => "Run",
            Step::Results => "Results",
        }
    }

    /// The step's place in the workflow, from 0.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The commands that do the step's work.
    pub const fn commands(self) -> &'static [&'static str] {
        match self {
            Step::Intent => &["units.set"],
            Step::Geometry => &["geometry.waveguide", "geometry.solid", "geometry.feature", "geometry.inclusion", "geometry.import", "file.sample"],
            Step::Materials => &["model.material.create"],
            Step::Excitation => &["model.port.create", "model.port.lumped", "source.dipole", "source.plane_wave", "source.clear"],
            Step::Boundaries => &["model.walls", "model.open"],
            Step::Mesh => &["mesh.generate"],
            Step::Setup | Step::Run => &[],
            Step::Results => &["results.compare", "view.sparameters", "view.field", "view.pattern"],
        }
    }

    /// The step a command belongs to.
    pub fn of_command(id: &str) -> Option<Step> {
        Some(match id {
            "project.frequency" | "units.set" => Step::Intent,
            _ if id.starts_with("geometry.") || id == "file.sample" => Step::Geometry,
            "model.material.create" => Step::Materials,
            _ if id.starts_with("model.port.") || id.starts_with("source.") || id == "solve.port_modes" => Step::Excitation,
            "model.walls" | "model.open" => Step::Boundaries,
            "mesh.generate" | "project.estimate" => Step::Mesh,
            _ if id.starts_with("solve.") => Step::Run,
            _ if id.starts_with("results.") || id.starts_with("view.") => Step::Results,
            _ => return None,
        })
    }
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// A name that is not a step's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownStep;

impl FromStr for Step {
    type Err = UnknownStep;

    fn from_str(s: &str) -> Result<Step, UnknownStep> {
        Step::ALL.into_iter().find(|x| x.id() == s).ok_or(UnknownStep)
    }
}

/// Where a step stands, from worst to best.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Blocks a run.
    Missing,
    /// Results older than the model.
    Stale,
    /// Needs attention.
    Warning,
    Done,
}

impl Status {
    pub const fn id(self) -> &'static str {
        match self {
            Status::Missing => "missing",
            Status::Stale => "stale",
            Status::Warning => "warning",
            Status::Done => "done",
        }
    }
}

/// What a step needs, and the command that sees to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Check {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

/// One step's standing and what to do about it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StepState {
    pub step: Step,
    pub status: Status,
    pub messages: Vec<Check>,
}

impl StepState {
    fn new(step: Step) -> StepState {
        StepState { step, status: Status::Done, messages: Vec::new() }
    }

    /// Lowers the status to `status` with a message.
    fn note(&mut self, status: Status, text: impl Into<String>, command: Option<&str>) {
        self.status = self.status.min(status);
        self.messages.push(Check { text: text.into(), command: command.map(str::to_owned) });
    }
}

/// Where each step stands, from the model, the mesh and the results; in workflow order.
pub fn steps(s: &Session) -> [StepState; 9] {
    let m = &s.model;
    let band = m.band;
    let mut intent = StepState::new(Step::Intent);
    if band.is_none() {
        intent.note(
            Status::Warning,
            "no frequency band: set the job's band first, it sets the mesh, the sweep and the port previews",
            Some("project.frequency"),
        );
    }
    let mut geometry = StepState::new(Step::Geometry);
    let has_geometry = m.waveguide.is_some() || m.solid.is_some();
    if !has_geometry {
        geometry.note(Status::Missing, "no geometry: a waveguide, a solid or an imported surface", Some("geometry.waveguide"));
    }
    let mut materials = StepState::new(Step::Materials);
    if !has_geometry {
        materials.note(Status::Missing, "no geometry to give materials to", Some("geometry.solid"));
    }
    if m.ports.iter().any(|p| m.port_material(p.end).is_ok_and(|x| x.loss_tangent != 0.0 || x.is_dispersive())) {
        materials.note(
            Status::Warning,
            "a port lies in a lossy or dispersive material, which ports do not support: end the guide with a lossless, non-dispersive section",
            Some("geometry.waveguide"),
        );
    }
    let mut excitation = StepState::new(Step::Excitation);
    let open = m.open.is_some();
    if m.ports.is_empty() && m.lumped.is_empty() && m.sources.is_empty() {
        excitation.note(Status::Missing, "nothing drives the model: a waveguide port, a lumped port or a source", Some("model.port.create"));
    }
    if !open && m.ports.len() == 1 && m.sources.is_empty() && m.lumped.is_empty() {
        excitation.note(Status::Warning, "one port: S11 only (a closed guide usually has two)", Some("model.port.create"));
    }
    // A closed-form port's first mode against the band's start.
    if let Some(b) = band {
        for p in &m.ports {
            if let (Ok(emcraft_model::PortShape::Rect { a, .. }), Ok(mat)) = (m.port_shape_of(p), m.port_material(p.end)) {
                let cutoff = C0 / (2.0 * a * (mat.eps_r * mat.mu_r).sqrt());
                if b.start <= cutoff {
                    excitation.note(
                        Status::Warning,
                        format!(
                            "port `{}`: the band starts at or below its TE10 cut-off ({:.4} {}): no power enters there",
                            p.name,
                            s.show_frequency(cutoff),
                            s.display.frequency.symbol()
                        ),
                        Some("project.frequency"),
                    );
                }
            }
        }
    }
    let mut boundaries = StepState::new(Step::Boundaries);
    if !open && (!m.sources.is_empty() || !m.lumped.is_empty()) {
        boundaries.note(
            Status::Warning,
            "sources in a closed box: they radiate into its walls; line it with the open boundary for an antenna",
            Some("model.open"),
        );
    }
    if open && m.ports.iter().any(|p| p.face.is_none()) {
        boundaries.note(
            Status::Warning,
            "a port on an end face of an open box: ports inside an open problem go on faces (`face`)",
            Some("model.port.create"),
        );
    }
    let mut mesh = StepState::new(Step::Mesh);
    match &m.mesh {
        None => mesh.note(Status::Missing, "no mesh", Some("mesh.generate")),
        Some(me) => {
            // Elements per wavelength at the band's top in the densest material.
            if let Some(b) = band {
                let index = m.materials.iter().map(|x| x.index_at(b.stop)).fold(1.0, f64::max);
                let lambda = C0 / (b.stop * index);
                let volume: f64 = (0..me.tets.len()).filter_map(|t| me.volume(t)).sum();
                // The edge of a regular tetrahedron of the mean volume.
                let h = (6.0 * std::f64::consts::SQRT_2 * volume / me.tets.len().max(1) as f64).cbrt();
                if h > lambda / 4.0 {
                    mesh.note(Status::Warning,
                        format!(
                            "elements of about {:.3} {} are coarse for the band's top (λ/4 = {:.3} {} in the densest material; second-order elements want about λ/6)",
                            s.show_length(h),
                            s.display.length.symbol(),
                            s.show_length(lambda / 4.0),
                            s.display.length.symbol()
                        ),
                        Some("mesh.generate"),
                    );
                }
            }
        }
    }
    let mut setup = StepState::new(Step::Setup);
    if band.is_none() {
        setup.note(Status::Warning, "no band to sweep: give the sweep's frequencies when running, or set the band", Some("project.frequency"));
    }
    let solved = s.results.any();
    let mut run = StepState::new(Step::Run);
    let mut results = StepState::new(Step::Results);
    let blocking = [&geometry, &excitation, &mesh].iter().any(|x| x.status == Status::Missing);
    if blocking {
        run.note(Status::Missing, "the steps above marked missing must be done first", None);
    }
    if !solved {
        run.note(Status::Missing, "not run yet", Some("solve.frequency"));
        results.note(Status::Missing, "no results yet", Some("solve.frequency"));
    } else if s.solved_revision.is_some_and(|r| r < s.revision) {
        results.note(Status::Stale, "the model changed since these results were solved: run again", Some("solve.frequency"));
        run.note(Status::Stale, "the model changed since the last run", Some("solve.frequency"));
    }
    [intent, geometry, materials, excitation, boundaries, mesh, setup, run, results]
}

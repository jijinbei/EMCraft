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
            Step::Boundaries => &["model.walls", "model.open", "model.symmetry"],
            Step::Mesh => &["mesh.generate"],
            Step::Setup | Step::Run => &[],
            Step::Results => &["results.compare", "view.sparameters", "view.field", "view.animation", "view.pattern"],
        }
    }

    /// The step a command belongs to.
    pub fn of_command(id: &str) -> Option<Step> {
        Some(match id {
            "project.frequency" | "units.set" => Step::Intent,
            _ if id.starts_with("geometry.") || id == "file.sample" => Step::Geometry,
            "model.material.create" => Step::Materials,
            _ if id.starts_with("model.port.") || id.starts_with("source.") || id == "solve.port_modes" => Step::Excitation,
            "model.walls" | "model.open" | "model.symmetry" => Step::Boundaries,
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

/// A cut-off frequency to mark beside the band: a port mode's or a guide section's.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Cutoff {
    /// Hz.
    pub frequency: f64,
    /// What it belongs to: `TE10 port 1, 2`, `TE10 vacuum`.
    pub label: String,
    /// The first mode of a port: below it no power enters.
    pub first: bool,
}

/// Bessel zeros for a circular port's first modes: J1′ (TE11) and J0 (TM01).
const X11_PRIME: f64 = 1.841_183_781_340_659;
const X01: f64 = 2.404_825_557_695_773;

/// The cut-offs of the ports' first modes (closed form, from each port's cross-section and
/// filling) and of the guide's sections (TE10), in ascending order; a frequency shared by several
/// ports is listed once.
pub fn cutoffs(s: &Session) -> Vec<Cutoff> {
    let m = &s.model;
    let mut out: Vec<Cutoff> = Vec::new();
    let add = |frequency: f64, mode: &str, owner: String, first: bool, out: &mut Vec<Cutoff>| {
        if !(frequency.is_finite() && frequency > 0.0) {
            return;
        }
        match out.iter_mut().find(|c| (c.frequency - frequency).abs() <= 1e-6 * frequency && c.label.starts_with(mode)) {
            Some(c) if c.label.starts_with(&format!("{mode} port ")) && owner.starts_with("port ") => {
                c.label.push_str(&format!(", {}", owner.trim_start_matches("port ")));
            }
            Some(_) => {}
            None => out.push(Cutoff { frequency, label: format!("{mode} {owner}"), first }),
        }
    };
    for p in m.ports.iter().take(64) {
        let (Ok(shape), Ok(mat)) = (m.port_shape_of(p), m.port_material(p.end)) else { continue };
        let n = (mat.eps_r * mat.mu_r).sqrt();
        let owner = format!("port {}", p.name);
        match shape {
            emcraft_model::PortShape::Rect { a, b, .. } => {
                add(C0 / (2.0 * a * n), "TE10", owner.clone(), true, &mut out);
                add(C0 / (a * n), "TE20", owner.clone(), false, &mut out);
                add(C0 / (2.0 * b * n), "TE01", owner, false, &mut out);
            }
            emcraft_model::PortShape::Circle { r, .. } => {
                let k = C0 / (2.0 * std::f64::consts::PI * r * n);
                add(X11_PRIME * k, "TE11", owner.clone(), true, &mut out);
                add(X01 * k, "TM01", owner, false, &mut out);
            }
        }
    }
    for (fc, name) in crate::view::section_cutoffs(s) {
        if !out.iter().any(|c| (c.frequency - fc).abs() <= 1e-6 * fc) {
            out.push(Cutoff { frequency: fc, label: format!("TE10 {name}"), first: false });
        }
    }
    out.sort_by(|a, b| a.frequency.total_cmp(&b.frequency));
    out
}

/// Where each step stands, from the model, the mesh and the results; in workflow order.
pub fn steps(s: &Session) -> [StepState; 9] {
    let (geometry, excitation, mesh) = (geometry(s), excitation(s), mesh(s));
    let blocking = [&geometry, &excitation, &mesh].iter().any(|x| x.status == Status::Missing);
    let (run, results) = run_and_results(s, blocking);
    [intent(s), geometry, materials(s), excitation, boundaries(s), mesh, setup(s), run, results]
}

fn has_geometry(s: &Session) -> bool {
    s.model.waveguide.is_some() || s.model.solid.is_some()
}

fn intent(s: &Session) -> StepState {
    let mut st = StepState::new(Step::Intent);
    if s.model.band.is_none() {
        st.note(
            Status::Warning,
            "no frequency band: set the job's band first, it sets the mesh, the sweep and the port previews",
            Some("project.frequency"),
        );
    }
    st
}

fn geometry(s: &Session) -> StepState {
    let mut st = StepState::new(Step::Geometry);
    if !has_geometry(s) {
        st.note(Status::Missing, "no geometry: a waveguide, a solid or an imported surface", Some("geometry.waveguide"));
    }
    st
}

fn materials(s: &Session) -> StepState {
    let m = &s.model;
    let mut st = StepState::new(Step::Materials);
    if !has_geometry(s) {
        st.note(Status::Missing, "no geometry to give materials to", Some("geometry.solid"));
    }
    if m.ports.iter().any(|p| m.port_material(p.end).is_ok_and(|x| x.loss_tangent != 0.0 || x.is_dispersive())) {
        st.note(
            Status::Warning,
            "a port lies in a lossy or dispersive material, which ports do not support: end the guide with a lossless, non-dispersive section",
            Some("geometry.waveguide"),
        );
    }
    st
}

fn excitation(s: &Session) -> StepState {
    let m = &s.model;
    let mut st = StepState::new(Step::Excitation);
    if m.ports.is_empty() && m.lumped.is_empty() && m.sources.is_empty() {
        st.note(Status::Missing, "nothing drives the model: a waveguide port, a lumped port or a source", Some("model.port.create"));
    }
    if m.open.is_none() && m.ports.len() == 1 && m.sources.is_empty() && m.lumped.is_empty() {
        st.note(Status::Warning, "one port: S11 only (a closed guide usually has two)", Some("model.port.create"));
    }
    // A closed-form port's first mode against the band's start.
    let Some(b) = m.band else { return st };
    for p in &m.ports {
        let (Ok(emcraft_model::PortShape::Rect { a, .. }), Ok(mat)) = (m.port_shape_of(p), m.port_material(p.end)) else { continue };
        let cutoff = C0 / (2.0 * a * (mat.eps_r * mat.mu_r).sqrt());
        if b.start <= cutoff {
            st.note(
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
    st
}

fn boundaries(s: &Session) -> StepState {
    let m = &s.model;
    let open = m.open.is_some();
    let mut st = StepState::new(Step::Boundaries);
    if !open && (!m.sources.is_empty() || !m.lumped.is_empty()) {
        st.note(
            Status::Warning,
            "sources in a closed box: they radiate into its walls; line it with the open boundary for an antenna",
            Some("model.open"),
        );
    }
    if open && m.ports.iter().any(|p| p.face.is_none()) {
        st.note(
            Status::Warning,
            "a port on an end face of an open box: ports inside an open problem go on faces (`face`)",
            Some("model.port.create"),
        );
    }
    st
}

fn mesh(s: &Session) -> StepState {
    let m = &s.model;
    let mut st = StepState::new(Step::Mesh);
    let Some(me) = &m.mesh else {
        st.note(Status::Missing, "no mesh", Some("mesh.generate"));
        return st;
    };
    // Elements per wavelength at the band's top in the densest material.
    let Some(b) = m.band else { return st };
    let index = m.materials.iter().map(|x| x.index_at(b.stop)).fold(1.0, f64::max);
    let lambda = C0 / (b.stop * index);
    let volume: f64 = (0..me.tets.len()).filter_map(|t| me.volume(t)).sum();
    // The edge of a regular tetrahedron of the mean volume.
    let h = (6.0 * std::f64::consts::SQRT_2 * volume / me.tets.len().max(1) as f64).cbrt();
    if h > lambda / 4.0 {
        let unit = s.display.length.symbol();
        st.note(
            Status::Warning,
            format!(
                "elements of about {:.3} {unit} are coarse for the band's top (λ/4 = {:.3} {unit} in the densest material; second-order elements want about λ/6)",
                s.show_length(h),
                s.show_length(lambda / 4.0),
            ),
            Some("mesh.generate"),
        );
    }
    st
}

fn setup(s: &Session) -> StepState {
    let mut st = StepState::new(Step::Setup);
    if s.model.band.is_none() {
        st.note(Status::Warning, "no band to sweep: give the sweep's frequencies when running, or set the band", Some("project.frequency"));
    }
    st
}

/// The Run and Results steps, `blocking` when a step above is missing.
fn run_and_results(s: &Session, blocking: bool) -> (StepState, StepState) {
    let mut run = StepState::new(Step::Run);
    let mut results = StepState::new(Step::Results);
    if blocking {
        run.note(Status::Missing, "the steps above marked missing must be done first", None);
    }
    if !s.results.any() {
        run.note(Status::Missing, "not run yet", Some("solve.frequency"));
        results.note(Status::Missing, "no results yet", Some("solve.frequency"));
    } else if s.solved_revision.is_some_and(|r| r < s.revision) {
        results.note(Status::Stale, "the model changed since these results were solved: run again", Some("solve.frequency"));
        run.note(Status::Stale, "the model changed since the last run", Some("solve.frequency"));
    }
    (run, results)
}

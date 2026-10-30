//! The steps of an electromagnetic job (docs/ux/workflow.md §1) and where each stands: worked
//! out from the model, the mesh and the results, without solving. `project.status` replies
//! with them; the desktop app's step bar reads them directly. The checks follow the job's kind
//! of analysis (`project.analysis`) once one is chosen; until then they are as for any job.

use std::str::FromStr;

pub use emcraft_model::Analysis;
use emcraft_model::units::C0;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Session;
use crate::view::X11_PRIME;

emcraft_macros::named_enum! {
    /// A step of the workflow, in the order a job goes through them.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Step {
        Intent = "intent" => "Intent",
        Geometry = "geometry" => "Geometry",
        Materials = "materials" => "Materials",
        Excitation = "excitation" => "Excitation",
        Boundaries = "boundaries" => "Boundaries",
        Mesh = "mesh" => "Mesh",
        Setup = "setup" => "Setup",
        Run = "run" => "Run",
        Results = "results" => "Results",
    }
}

impl Step {
    /// The step's place in the workflow, from 0.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The commands that do the step's work.
    pub const fn commands(self) -> &'static [&'static str] {
        match self {
            Step::Intent => &["project.analysis", "units.set"],
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
            "project.frequency" | "project.analysis" | "units.set" => Step::Intent,
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

/// A name that is not a step's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownStep;

impl FromStr for Step {
    type Err = UnknownStep;

    fn from_str(s: &str) -> Result<Step, UnknownStep> {
        Step::parse(s).ok_or(UnknownStep)
    }
}

emcraft_macros::named_enum! {
    /// Where a step stands, from worst to best: something wrong that blocks a run, results
    /// older than the model, a warning that matters now, waiting on an earlier step, not done
    /// yet, done.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Status {
        /// Something is wrong: it blocks a run until put right.
        Missing = "missing" => "needs attention",
        /// Results older than the model.
        Stale = "stale" => "out of date",
        /// Worth a look now (a run may accept it).
        Warning = "warning" => "warning",
        /// Waits on an earlier step (the geometry, a run).
        Waiting = "waiting" => "waiting",
        /// Not done yet: the next thing to do, nothing wrong.
        Todo = "todo" => "to do",
        Done = "done" => "done",
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

/// The command that runs a kind of analysis.
///
/// ```
/// use emcraft_engine::workflow::{Analysis, solve_command};
/// assert_eq!(solve_command(Analysis::Eigenmodes), "solve.eigenmodes");
/// assert!(Analysis::ALL.iter().all(|a| emcraft_engine::find_command(solve_command(*a)).is_some()));
/// ```
pub const fn solve_command(a: Analysis) -> &'static str {
    match a {
        Analysis::Frequency => "solve.frequency",
        Analysis::Eigenmodes => "solve.eigenmodes",
        Analysis::Radiation => "solve.radiation",
        Analysis::TimeDomain => "solve.time_domain",
    }
}

/// The job's kind of analysis as the replies give it: its id (`kind`), whether it was chosen
/// (`project.analysis`) or is inferred from the model (`chosen`), and the command that runs it
/// (`solve`).
///
/// ```
/// use emcraft_model::{Analysis, Model};
/// let mut m = Model::default();
/// let inferred = emcraft_engine::workflow::analysis_reply(&m);
/// assert_eq!((inferred["kind"].as_str(), inferred["chosen"].as_bool()), (Some("eigenmodes"), Some(false)));
/// m.set_analysis(Some(Analysis::Radiation));
/// let chosen = emcraft_engine::workflow::analysis_reply(&m);
/// assert_eq!((chosen["solve"].as_str(), chosen["chosen"].as_bool()), (Some("solve.radiation"), Some(true)));
/// ```
pub fn analysis_reply(m: &emcraft_model::Model) -> Value {
    let a = m.analysis();
    json!({"kind": a.id(), "chosen": m.analysis.is_some(), "solve": solve_command(a)})
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

/// The first zero of J0, for a circular port's TM01 (its TE11 takes J1′'s, [`X11_PRIME`]).
const X01: f64 = 2.404_825_557_695_773;

/// The cut-offs of the ports' first modes (closed form, from each port's cross-section and
/// filling) and of the guide's sections (TE10), in ascending order; a frequency shared by several
/// ports is listed once.
pub fn cutoffs(s: &Session) -> Vec<Cutoff> {
    let m = &s.model;
    let mut out: Vec<Cutoff> = Vec::new();
    // A periodic cell's ports are Floquet ports: no guide cut-offs.
    if m.periodic.is_some() {
        return out;
    }
    let add = |frequency: f64, mode: &str, owner: &str, first: bool, out: &mut Vec<Cutoff>| {
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
                add(C0 / (2.0 * a * n), "TE10", &owner, true, &mut out);
                add(C0 / (a * n), "TE20", &owner, false, &mut out);
                add(C0 / (2.0 * b * n), "TE01", &owner, false, &mut out);
            }
            emcraft_model::PortShape::Circle { r, .. } => {
                let k = C0 / (2.0 * std::f64::consts::PI * r * n);
                add(X11_PRIME * k, "TE11", &owner, true, &mut out);
                add(X01 * k, "TM01", &owner, false, &mut out);
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
    let above = [intent(s), geometry(s), materials(s), excitation(s), boundaries(s), mesh(s), setup(s)];
    let (run, results) = run_and_results(s, &above);
    let [intent, geometry, materials, excitation, boundaries, mesh, setup] = above;
    [intent, geometry, materials, excitation, boundaries, mesh, setup, run, results]
}

/// The steps a run cannot do without: the geometry and the mesh; once a kind of analysis is
/// chosen that is driven, the excitation too (a resonator needs none), and for radiation the
/// open boundary.
pub fn required(chosen: Option<Analysis>) -> &'static [Step] {
    match chosen {
        None | Some(Analysis::Eigenmodes) => &[Step::Geometry, Step::Mesh],
        Some(Analysis::Frequency | Analysis::TimeDomain) => &[Step::Geometry, Step::Excitation, Step::Mesh],
        Some(Analysis::Radiation) => &[Step::Geometry, Step::Excitation, Step::Boundaries, Step::Mesh],
    }
}

/// The command that runs the job: the chosen kind's solve, else a sweep.
fn run_command(s: &Session) -> &'static str {
    s.model.analysis.map_or("solve.frequency", solve_command)
}

/// Whether anything was solved: a solver's results or a parameter study's.
fn solved(s: &Session) -> bool {
    s.results.any() || s.results.study.is_some()
}

fn has_geometry(s: &Session) -> bool {
    s.model.waveguide.is_some() || s.model.solid.is_some()
}

fn intent(s: &Session) -> StepState {
    let mut st = StepState::new(Step::Intent);
    // Once something is solved, the band has done its work.
    if s.model.band.is_none() && !solved(s) {
        st.note(
            Status::Todo,
            "no frequency band yet: set the job's band first, it sets the mesh, the sweep and the port previews",
            Some("project.frequency"),
        );
    }
    st
}

fn geometry(s: &Session) -> StepState {
    let mut st = StepState::new(Step::Geometry);
    if !has_geometry(s) {
        st.note(Status::Todo, "no geometry yet: a waveguide, a solid or an imported surface", Some("geometry.waveguide"));
    }
    st
}

fn materials(s: &Session) -> StepState {
    let m = &s.model;
    let mut st = StepState::new(Step::Materials);
    if !has_geometry(s) {
        st.note(Status::Waiting, "waits for the geometry: no bodies to give materials to yet", Some("geometry.solid"));
    }
    if let Some(p) = m.ports.iter().find(|p| lossy_terminal_port(m, p)) {
        st.note(
            Status::Warning,
            format!(
                "port `{}`: several strips in a lossy or dispersive cross-section, where terminal ports are not supported yet (its lossy modes are not power-orthogonal): set the port's `terminals` modal",
                p.name
            ),
            Some("model.port.create"),
        );
    }
    st
}

/// Whether a port asks for terminals at an end of a guide where two or more strips reach it and
/// a material of its cross-section is lossy or dispersive (refused when solving).
fn lossy_terminal_port(m: &emcraft_model::Model, p: &emcraft_model::Port) -> bool {
    let Some(w) = &m.waveguide else { return false };
    if p.terminals != emcraft_model::Terminals::Auto {
        return false;
    }
    let length = w.length();
    let at_end = |s: &&emcraft_model::Strip| match p.end {
        emcraft_model::End::Start => s.z0 <= 0.0,
        emcraft_model::End::End => s.z1 >= length,
    };
    if w.strips.iter().filter(at_end).count() < 2 {
        return false;
    }
    let fill = m.port_material(p.end).is_ok_and(|x| x.is_lossy());
    fill || w.layers.iter().filter_map(|l| m.material(&l.material).ok()).any(|(_, x)| x.is_lossy())
}

fn excitation(s: &Session) -> StepState {
    let m = &s.model;
    let mut st = StepState::new(Step::Excitation);
    // A cell periodic along z is solved for its dispersion, with nothing driving it.
    let bloch = m.periodic.is_some_and(|p| p.z);
    let chosen = m.analysis;
    // A resonator's eigenmodes need nothing to drive them.
    let resonator = s.results.eigen.is_some() || chosen == Some(Analysis::Eigenmodes);
    let driven = !(m.ports.is_empty() && m.lumped.is_empty() && m.sources.is_empty());
    if !has_geometry(s) {
        st.note(Status::Waiting, "waits for the geometry", Some("geometry.waveguide"));
    } else if !driven && !bloch && !resonator {
        if chosen == Some(Analysis::Radiation) {
            st.note(Status::Todo, "nothing drives the open problem yet: a lumped port, a dipole or a plane wave", Some("model.port.lumped"));
        } else {
            st.note(
                Status::Todo,
                "nothing drives the model yet: a waveguide port, a lumped port or a source (a resonator needs none: Run finds its eigenmodes)",
                Some("model.port.create"),
            );
        }
    }
    if chosen == Some(Analysis::Eigenmodes) && driven {
        st.note(Status::Warning, "eigenmodes leave the ports and sources out (closed by walls)", Some("project.analysis"));
    }
    if chosen != Some(Analysis::Eigenmodes) && m.open.is_none() && m.ports.len() == 1 && m.sources.is_empty() && m.lumped.is_empty() {
        st.note(Status::Warning, "one port: S11 only (a closed guide usually has two)", Some("model.port.create"));
    }
    // A closed-form port's first mode against the band's start.
    let Some(b) = m.band else { return st };
    for p in m.ports.iter().filter(|_| m.periodic.is_none()) {
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
    if !has_geometry(s) {
        st.note(Status::Waiting, "waits for the geometry: its walls are the boundaries", Some("geometry.waveguide"));
    } else if !open && m.analysis == Some(Analysis::Radiation) {
        st.note(Status::Todo, "radiation wants the open boundary (a perfectly matched layer) around the model", Some("model.open"));
    }
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
        if has_geometry(s) {
            st.note(Status::Todo, "no mesh yet", Some("mesh.generate"));
        } else {
            st.note(Status::Waiting, "waits for the geometry", Some("geometry.waveguide"));
        }
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
    if !has_geometry(s) {
        st.note(Status::Waiting, "waits for the geometry", Some("geometry.waveguide"));
    }
    if s.model.band.is_none() && !solved(s) {
        st.note(Status::Todo, "no band to sweep yet: set the band, or give the sweep's frequencies when running", Some("project.frequency"));
    }
    st
}

/// The Run and Results steps after the steps `above` them: Run needs attention while a step
/// above does, waits while a step it cannot do without is not done, and is to do until it ran;
/// Results wait for Run, and are out of date once the model changed after it.
fn run_and_results(s: &Session, above: &[StepState]) -> (StepState, StepState) {
    let mut run = StepState::new(Step::Run);
    let mut results = StepState::new(Step::Results);
    let names = |pick: &dyn Fn(&StepState) -> bool| above.iter().filter(|x| pick(x)).map(|x| x.step.id()).collect::<Vec<_>>().join(", ");
    let wrong = names(&|x| x.status == Status::Missing);
    let required = required(s.model.analysis);
    let waiting = names(&|x| required.contains(&x.step) && matches!(x.status, Status::Todo | Status::Waiting));
    let command = run_command(s);
    if !wrong.is_empty() {
        run.note(Status::Missing, format!("put {wrong} right first"), None);
    } else if !waiting.is_empty() {
        run.note(Status::Waiting, format!("waits for {waiting}"), None);
    }
    if !solved(s) {
        if wrong.is_empty() && waiting.is_empty() {
            run.note(Status::Todo, "not run yet", Some(command));
        }
        results.note(Status::Waiting, "no results yet: Run (step 7) makes them", Some(command));
    } else if s.solved_revision.is_some_and(|r| r < s.revision) {
        results.note(Status::Stale, "the model changed after this run: run again", Some(command));
        run.note(Status::Stale, "the model changed since the last run", Some(command));
    }
    (run, results)
}

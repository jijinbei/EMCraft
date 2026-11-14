//! The session: the model, its results, the display units, the history that built the model and
//! undo; [`Session::execute`] runs every command.

use std::collections::VecDeque;
use std::sync::Arc;

use emcraft_fem::Order;
use emcraft_model::Model;
use emcraft_model::units::Display;
use emcraft_solver::{Driven, FrequencyResult, SPort};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::{CommandSpec, check_keys, find_command};
use crate::project::{self, Effect};
use crate::runs::Run;
use crate::{EngineError, Params, Result, Step, expr, open, progress, solve, study};

/// A frequency sweep's results.
#[derive(Clone, Debug)]
pub struct Sweep {
    pub order: Order,
    pub driven: Arc<Driven>,
    /// Ascending frequency.
    pub results: Vec<FrequencyResult>,
    /// Each port's de-embedding (m) the S-parameters are referred to: the ports' as solved, or as
    /// `model.port.deembed` moved them since.
    pub deembed: Vec<f64>,
}

impl Sweep {
    /// A sweep's results as `driven` solved them (its ports' de-embedding).
    pub fn new(order: Order, driven: Arc<Driven>, results: Vec<FrequencyResult>) -> Sweep {
        let deembed = driven.ports.iter().map(|p| p.deembed).collect();
        Sweep { order, driven, results, deembed }
    }

    /// `results` (solved by this sweep's system) with their S referred to this sweep's
    /// reference planes: each port's waves moved by e^{γΔd} from the plane its system solved
    /// at (D. M. Pozar, *Microwave Engineering*, 4th ed., §4.3).
    pub(crate) fn referred(&self, results: Vec<FrequencyResult>) -> crate::Result<Vec<FrequencyResult>> {
        let solved: Vec<f64> = self.driven.ports.iter().map(|p| p.deembed).collect();
        rereference(&self.driven, results, &solved, &self.deembed)
    }
}

/// `results` (solved by `driven`) with each port's reference plane moved from de-embedding
/// `from` to `to` (m): S_pq e^{γ_p Δd_p + γ_q Δd_q}, γ the port's first mode's.
pub(crate) fn rereference(driven: &Driven, mut results: Vec<FrequencyResult>, from: &[f64], to: &[f64]) -> crate::Result<Vec<FrequencyResult>> {
    let moved: Vec<(usize, f64)> = from.iter().zip(to).enumerate().filter(|(_, (a, b))| a != b).map(|(k, (a, b))| (k, b - a)).collect();
    if moved.is_empty() {
        return Ok(results);
    }
    for r in &mut results {
        let mut shift = vec![emcraft_solver::Complex64::new(1.0, 0.0); r.s.ports()];
        for (k, d) in &moved {
            let gamma = driven.port_terms(*k, r.frequency)?.first().map_or(emcraft_solver::Complex64::new(0.0, 0.0), |t| t.gamma);
            if let Some(slot) = shift.get_mut(*k) {
                *slot = (gamma * *d).exp();
            }
        }
        let s = &r.s;
        r.s = emcraft_solver::SMatrix::from_fn(s.ports(), |p, q| s[(p, q)] * shift[p] * shift[q]);
    }
    Ok(results)
}

impl Sweep {
    /// S's entries: each port, or each conductor of a terminal port.
    pub fn entries(&self) -> Vec<SPort> {
        self.driven.entries()
    }

    /// Its S-parameters (n × n at each frequency); an error when a value is not finite or
    /// the matrices do not agree with the entries.
    pub fn sdata(&self) -> std::result::Result<crate::SData, String> {
        let n = self.results.first().map_or(0, |r| r.s.ports());
        let s = self.results.iter().flat_map(|r| r.s.rows().flatten().copied()).collect();
        crate::SData::square(self.results.iter().map(|r| r.frequency).collect(), n, s)
    }

    /// The result at a frequency (Hz) solved within 10⁻⁹ of it, or the nearest when `nearest`.
    pub fn at(&self, frequency: f64, nearest: bool) -> Option<&FrequencyResult> {
        let best = self.results.iter().min_by(|a, b| (a.frequency - frequency).abs().total_cmp(&(b.frequency - frequency).abs()))?;
        if nearest || crate::numbers::same_frequency(best.frequency, frequency) { Some(best) } else { None }
    }
}

/// A body of revolution's S-parameters: its azimuthal order, the ports' first modes' names, and
/// S at each frequency (every port excited).
#[derive(Clone, Debug, PartialEq)]
pub struct Revolution {
    pub m: u32,
    pub modes: Vec<String>,
    pub data: crate::sdata::SData,
    pub unknowns: usize,
}

/// A periodic guide's Bloch waves from one cell (`solve.revolution.bloch`, `solve.bloch`): the
/// period (m), the frequencies (Hz) and each wave going +z, its γ = α + jβ (1/m) at each.
#[derive(Clone, Debug, PartialEq)]
pub struct Bloch {
    pub period: f64,
    pub frequencies: Vec<f64>,
    pub waves: Vec<BlochWave>,
}

/// One Bloch wave: what it is mostly (its strongest port mode at the first frequency) and its γ.
#[derive(Clone, Debug, PartialEq)]
pub struct BlochWave {
    pub name: String,
    pub gamma: Vec<emcraft_solver::Complex64>,
}

/// What the solvers made of the model, each from its own command. Each result is shared: the
/// copy a command keeps to go back to when it fails costs no copy of the fields.
#[derive(Clone, Debug, Default)]
pub struct Results {
    /// The driven frequency sweep from `solve.frequency`.
    pub sweep: Option<Arc<Sweep>>,
    /// The cavity's eigenmodes from `solve.eigenmodes`.
    pub eigen: Option<Arc<emcraft_solver::eigen::Eigen>>,
    /// The time-domain solver's S-parameters from `solve.time_domain`.
    pub time_domain: Option<Arc<emcraft_fdtd::TdResult>>,
    /// The open problem from `solve.radiation`.
    pub radiation: Option<Arc<open::Radiated>>,
    /// The open problem from `solve.time_domain` on an open model.
    pub td_radiation: Option<Arc<emcraft_fdtd::open::OpenRun>>,
    /// The last parameter study (`study.sweep`, `study.optimize`): solved on models rebuilt
    /// from the history, so no change to the model now makes it wrong.
    pub study: Option<Arc<study::Study>>,
    /// A periodic cell's dispersion diagram from `solve.dispersion`.
    pub dispersion: Option<Arc<emcraft_solver::bloch::Dispersion>>,
    /// A body of revolution's S-parameters in one azimuthal order, from
    /// `solve.revolution.frequency`.
    pub revolution: Option<Arc<Revolution>>,
    /// A periodic guide's Bloch waves from one cell.
    pub bloch: Option<Arc<Bloch>>,
    /// A system's chain and its budget from `solve.system`.
    pub system: Option<Arc<crate::system::SystemResult>>,
    /// A particle-in-cell run from `solve.particles`.
    pub particles: Option<Arc<crate::particles::PicResult>>,
    /// The kinds held that are stale: the model changed after they were solved (without
    /// making them wrong, which drops them). Only kinds held; each solve makes its own fresh.
    stale: std::collections::BTreeSet<ResultKind>,
}

/// What changed in the model, which decides the results that no longer hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// The geometry, the mesh or a symmetry plane: every result was solved on another model.
    Geometry,
    /// The walls' conductivity: the sweep and the eigenmodes' Q.
    Walls,
    /// A waveguide port: the sweep and the time domain's S-parameters.
    Ports,
    /// The open boundary, a source or a lumped port: the open problem's results.
    Open,
    /// A system's chain, source or receiver: no result solved from it is wrong, its own budget
    /// stale (read again in a moment).
    System,
    /// A particle-in-cell run's sources: its run.
    Particles,
}

emcraft_macros::named_enum! {
    /// A kind of result the session holds (a field of [`Results`]): what drops it, what makes
    /// it, whether it is the model's own — one place for what every kind is.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub enum ResultKind {
        Sweep = "sweep",
        Eigen = "eigenmodes",
        TimeDomain = "time_domain",
        Radiation = "radiation",
        TdRadiation = "td_radiation",
        Dispersion = "dispersion",
        Study = "study",
        Revolution = "revolution",
        Bloch = "bloch",
        System = "system",
        Particles = "particles",
    }
}

emcraft_macros::named_serde!(ResultKind);

impl ResultKind {
    /// Whether a change of `change` to the model leaves this kind wrong.
    ///
    /// ```
    /// use emcraft_engine::{Change, ResultKind};
    /// assert!(ResultKind::Sweep.dropped_by(Change::Ports) && !ResultKind::Eigen.dropped_by(Change::Ports));
    /// // A study is solved on models rebuilt from the history: no change makes it wrong.
    /// assert!(!ResultKind::Study.dropped_by(Change::Geometry));
    /// ```
    pub const fn dropped_by(self, change: Change) -> bool {
        use ResultKind::*;
        match change {
            Change::Geometry => !matches!(self, Study | System),
            Change::Walls => matches!(self, Sweep | Eigen | Revolution | Bloch),
            Change::Ports => matches!(self, Sweep | TimeDomain | Revolution | Bloch),
            Change::Open => matches!(self, Radiation | TdRadiation),
            Change::System => false,
            Change::Particles => matches!(self, Particles),
        }
    }

    /// Whether it is a solve of the model now (a study's points are solves of other models).
    pub const fn of_the_model(self) -> bool {
        !matches!(self, ResultKind::Study)
    }

    /// The command that makes it.
    pub const fn command(self) -> &'static str {
        match self {
            ResultKind::Sweep => "solve.frequency",
            ResultKind::Eigen => "solve.eigenmodes",
            ResultKind::TimeDomain | ResultKind::TdRadiation => "solve.time_domain",
            ResultKind::Radiation => "solve.radiation",
            ResultKind::Dispersion => "solve.dispersion",
            ResultKind::Study => "study.sweep",
            ResultKind::Revolution => "solve.revolution.frequency",
            ResultKind::Bloch => "solve.bloch",
            ResultKind::System => "solve.system",
            ResultKind::Particles => "solve.particles",
        }
    }
}

impl Results {
    /// Whether the session holds a result of `kind`.
    pub fn has(&self, kind: ResultKind) -> bool {
        match kind {
            ResultKind::Sweep => self.sweep.is_some(),
            ResultKind::Eigen => self.eigen.is_some(),
            ResultKind::TimeDomain => self.time_domain.is_some(),
            ResultKind::Radiation => self.radiation.is_some(),
            ResultKind::TdRadiation => self.td_radiation.is_some(),
            ResultKind::Dispersion => self.dispersion.is_some(),
            ResultKind::Study => self.study.is_some(),
            ResultKind::Revolution => self.revolution.is_some(),
            ResultKind::Bloch => self.bloch.is_some(),
            ResultKind::System => self.system.is_some(),
            ResultKind::Particles => self.particles.is_some(),
        }
    }

    /// Whether the result of `kind` is held and stale: the model changed after it was solved.
    pub fn is_stale(&self, kind: ResultKind) -> bool {
        self.stale.contains(&kind)
    }

    /// Whether any result held is stale.
    pub fn any_stale(&self) -> bool {
        !self.stale.is_empty()
    }

    /// The stale kinds, in [`ResultKind::ALL`]'s order.
    pub fn stale_kinds(&self) -> impl Iterator<Item = ResultKind> + '_ {
        self.stale.iter().copied()
    }

    /// The model changed: every result held is stale.
    pub(crate) fn age(&mut self) {
        self.stale = self.kinds().collect();
    }

    /// `kinds` are stale (those held).
    pub(crate) fn mark_stale(&mut self, kinds: impl IntoIterator<Item = ResultKind>) {
        let held: Vec<ResultKind> = kinds.into_iter().filter(|k| self.has(*k)).collect();
        self.stale.extend(held);
    }

    /// The kinds this holds that `before` did not hold as they are, those a solve just made,
    /// are fresh.
    pub(crate) fn solved_since(&mut self, before: &Results) {
        fn same<T>(a: Option<&Arc<T>>, b: Option<&Arc<T>>) -> bool {
            match (a, b) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
        }
        for kind in ResultKind::ALL {
            let unchanged = match kind {
                ResultKind::Sweep => same(self.sweep.as_ref(), before.sweep.as_ref()),
                ResultKind::Eigen => same(self.eigen.as_ref(), before.eigen.as_ref()),
                ResultKind::TimeDomain => same(self.time_domain.as_ref(), before.time_domain.as_ref()),
                ResultKind::Radiation => same(self.radiation.as_ref(), before.radiation.as_ref()),
                ResultKind::TdRadiation => same(self.td_radiation.as_ref(), before.td_radiation.as_ref()),
                ResultKind::Dispersion => same(self.dispersion.as_ref(), before.dispersion.as_ref()),
                ResultKind::Study => same(self.study.as_ref(), before.study.as_ref()),
                ResultKind::Revolution => same(self.revolution.as_ref(), before.revolution.as_ref()),
                ResultKind::Bloch => same(self.bloch.as_ref(), before.bloch.as_ref()),
                ResultKind::System => same(self.system.as_ref(), before.system.as_ref()),
                ResultKind::Particles => same(self.particles.as_ref(), before.particles.as_ref()),
            };
            if !unchanged {
                self.stale.remove(&kind);
            }
        }
    }

    /// Lets the result of `kind` go.
    pub fn clear(&mut self, kind: ResultKind) {
        self.stale.remove(&kind);
        match kind {
            ResultKind::Sweep => self.sweep = None,
            ResultKind::Eigen => self.eigen = None,
            ResultKind::TimeDomain => self.time_domain = None,
            ResultKind::Radiation => self.radiation = None,
            ResultKind::TdRadiation => self.td_radiation = None,
            ResultKind::Dispersion => self.dispersion = None,
            ResultKind::Study => self.study = None,
            ResultKind::Revolution => self.revolution = None,
            ResultKind::Bloch => self.bloch = None,
            ResultKind::System => self.system = None,
            ResultKind::Particles => self.particles = None,
        }
    }

    /// The kinds the session holds, in [`ResultKind::ALL`]'s order.
    pub fn kinds(&self) -> impl Iterator<Item = ResultKind> + '_ {
        ResultKind::ALL.into_iter().filter(|k| self.has(*k))
    }

    /// Drops the results a change to the model leaves wrong ([`ResultKind::dropped_by`]).
    /// Changes that leave them right but old (the band, the units) keep them, marked stale by
    /// the workflow.
    pub fn invalidate(&mut self, change: Change) {
        for kind in ResultKind::ALL.into_iter().filter(|k| k.dropped_by(change)) {
            self.clear(kind);
        }
    }

    /// Whether any solver has run on the model (a study's solves are of other models).
    pub fn any(&self) -> bool {
        self.kinds().any(ResultKind::of_the_model)
    }

    /// The sweep, or an error asking for one.
    pub fn sweep(&self) -> Result<&Sweep> {
        self.sweep.as_deref().ok_or(EngineError::NotSolved { what: "results", run: "solve.frequency" })
    }
}

/// The session as it was before a command ran: to undo to when it succeeds, to go back to when
/// it fails.
struct Before {
    /// The model, units and variables, and the history (for commands that change the job).
    state: project::Snapshot,
    results: Results,
    revision: u64,
    /// How the results are looked at (small: kept for every command).
    views: crate::views::Views,
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

/// A session: the model, its results and the display units. Only commands change it
/// ([`Session::execute`]); everyone else reads it through its accessors.
#[derive(Clone, Debug, Default)]
pub struct Session {
    pub(crate) model: Model,
    pub(crate) results: Results,
    pub(crate) display: Display,
    pub(crate) limits: Limits,
    /// The commands that built the model, in order (a script that rebuilds it); shared with
    /// the undo states and the saved state, copied only when it changes.
    pub(crate) history: Arc<Vec<Step>>,
    /// States to go back to (`edit.undo`) and forward again (`edit.redo`), the latest last.
    pub(crate) undo: VecDeque<project::Snapshot>,
    pub(crate) redo: VecDeque<project::Snapshot>,
    /// The model's revision, raised by every command that changes it.
    pub(crate) revision: u64,
    /// The project file the session was opened from or last saved to (`.emcraft`).
    pub(crate) path: Option<String>,
    /// The sample the session was opened from, which names it until it is saved.
    pub(crate) sample: Option<String>,
    /// The project as last saved or opened: the session has unsaved changes where it differs
    /// (an undo back to it is clean again).
    pub(crate) saved: Saved,
    /// The project's runs: the last solve of each solve command ([`crate::runs`]), the latest
    /// last.
    pub(crate) runs: Vec<Run>,
    /// The analysis history: the solves kept with their models and results
    /// ([`crate::archive`]), the last id given and the kept solve the session was restored to.
    pub(crate) archive: Vec<crate::archive::Archived>,
    pub(crate) archive_next: u32,
    pub(crate) archive_current: Option<u32>,
    /// Where the solvers run (`solve.device`).
    pub(crate) compute: solve::Compute,
    /// The named variables expressions in parameters read (`project.variable`), defined by
    /// commands of the history.
    pub(crate) variables: expr::Variables,
    /// How the project's results are looked at (charts' styles, probes: [`crate::views`]).
    pub(crate) views: crate::views::Views,
    /// The run a solve command records in place of its own (`solve.refine` records the merged
    /// sweep as `solve.frequency`); taken when it succeeds.
    pub(crate) next_run: Option<Run>,
    /// Whether the mesh commands mesh, or (a project file's history replayed) are read and
    /// recorded only, the file's own mesh put on the model after ([`Session::replay`]).
    pub(crate) meshing: Meshing,
}

/// Reading the session.
/// What the mesh commands do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Meshing {
    /// They mesh.
    #[default]
    Run,
    /// They read and record their parameters only (a project file's history replayed: its
    /// mesh comes from the file).
    Recorded,
}

/// The project as last saved or opened: its history, its runs, how its results are looked at
/// and its kept solves' ids, pins and labels — what [`Session::is_dirty`] compares with.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Saved {
    pub history: Arc<Vec<Step>>,
    pub runs: Vec<Run>,
    pub views: crate::views::Views,
    pub archive: Vec<crate::archive::Mark>,
}

impl Saved {
    /// `s`'s project as it is now.
    pub(crate) fn of(s: &Session) -> Saved {
        Saved { history: s.history.clone(), runs: s.runs.clone(), views: s.views.clone(), archive: crate::archive::marks(&s.archive) }
    }

    /// Whether `s`'s project is this one (compared in place, nothing copied).
    fn is(&self, s: &Session) -> bool {
        self.history == s.history && self.runs == s.runs && self.views == s.views && crate::archive::marks(&s.archive) == self.archive
    }
}

/// How a command that succeeded ran: its wall-clock seconds and its reply.
struct Ran<'a> {
    seconds: f64,
    reply: &'a Value,
}

impl Session {
    pub fn model(&self) -> &Model {
        &self.model
    }

    pub fn results(&self) -> &Results {
        &self.results
    }

    /// The display units parameters are read in and replies written in.
    pub fn display(&self) -> Display {
        self.display
    }

    /// The limits on the work one command may ask for.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// The session with other limits.
    pub fn with_limits(self, limits: Limits) -> Session {
        Session { limits, ..self }
    }

    /// The commands that built the model, in order (a script that rebuilds it).
    pub fn history(&self) -> &[Step] {
        &self.history
    }

    /// The states `edit.undo` goes back to, the latest last.
    pub fn undo(&self) -> &VecDeque<project::Snapshot> {
        &self.undo
    }

    /// The states `edit.redo` goes forward to, the latest last.
    pub fn redo(&self) -> &VecDeque<project::Snapshot> {
        &self.redo
    }

    /// The model's revision, raised by every command that changes it.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The project file the session was opened from or last saved to.
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// The sample the session was opened from.
    pub fn sample(&self) -> Option<&str> {
        self.sample.as_deref()
    }

    /// Where the solvers run (`solve.device`).
    pub fn compute(&self) -> &solve::Compute {
        &self.compute
    }

    /// The named variables (`project.variable`).
    pub fn variables(&self) -> &expr::Variables {
        &self.variables
    }

    /// How the project's results are looked at: the charts' styles and the probes.
    pub fn views(&self) -> &crate::views::Views {
        &self.views
    }
}

impl Session {
    /// S's entries: the sweep's (each conductor of a terminal port its own), or before a sweep
    /// one per waveguide port.
    pub fn sparameter_entries(&self) -> Vec<SPort> {
        match &self.results.sweep {
            Some(sw) => sw.entries(),
            None => self
                .model
                .ports
                .iter()
                .enumerate()
                .map(|(i, p)| SPort { name: p.name.clone(), port: i, terminal: None, reference: p.reference.map(f64::from) })
                .collect(),
        }
    }

    /// The driven entry of S (`view.driven`): its index among [`Session::sparameter_entries`],
    /// the first when none is set or the one set names nothing now.
    pub fn driven_entry(&self) -> usize {
        self.views.driven.as_deref().and_then(|n| self.sparameter_entry(n)).map_or(0, |(k, _)| k)
    }

    /// The model's port the driven entry belongs to (its index in `model.ports`), when the model
    /// has ports.
    pub fn driven_port(&self) -> Option<usize> {
        self.sparameter_entries().get(self.driven_entry()).map(|e| e.port).filter(|&i| i < self.model.ports.len())
    }

    /// The entry of S named `name` (a terminal `<port>:<k>`, or a port: its first entry) and its
    /// index; `None` when nothing has that name.
    pub fn sparameter_entry(&self, name: &str) -> Option<(usize, SPort)> {
        let entries = self.sparameter_entries();
        let exact = entries.iter().position(|e| e.name == name);
        let by_port = || entries.iter().position(|e| self.model.ports.get(e.port).is_some_and(|p| p.name == name));
        exact.or_else(by_port).and_then(|k| entries.get(k).cloned().map(|e| (k, e)))
    }

    pub fn new() -> Session {
        Session::default()
    }

    /// The analysis history ([`crate::archive`]), the oldest first.
    pub fn archive(&self) -> &[crate::archive::Archived] {
        &self.archive
    }

    /// A fresh session with this one's limits.
    pub(crate) fn fresh(&self) -> Session {
        Session { limits: self.limits, compute: self.compute.clone(), ..Session::default() }
    }

    /// This session replaced by `fresh` (a sample, a project file, a new one), its undo kept:
    /// it goes back to before the replacement.
    pub(crate) fn replace_with(&mut self, fresh: Session) {
        *self = Session { undo: std::mem::take(&mut self.undo), ..fresh };
    }

    /// The project as it is now counts as saved.
    pub(crate) fn mark_saved(&mut self) {
        self.saved = Saved::of(self);
    }

    /// Whether the project changed since it was last saved or opened: its model's history or
    /// its runs (a solve with other parameters than the saved run's) or how its results are
    /// looked at (a chart's style, a probe) or its analysis history (a solve kept, pinned,
    /// labelled or let go).
    pub fn is_dirty(&self) -> bool {
        !self.saved.is(self)
    }

    /// The project's runs, the latest last ([`crate::runs`]).
    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    /// The project's name: its file's stem, the sample's name, or "Untitled".
    pub fn name(&self) -> String {
        match self.path.as_deref().and_then(|p| std::path::Path::new(p).file_stem()) {
            Some(stem) => stem.to_string_lossy().into_owned(),
            None => self.sample.clone().unwrap_or_else(|| "Untitled".to_owned()),
        }
    }

    /// Runs a command. Unknown parameters are refused; a failed or panicking command leaves the
    /// session as it was.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        // An id from hostile input is not echoed in full.
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(crate::text::clip(id, 100)))?;
        check_keys(spec, params)?;
        let before = self.before(spec.effect);
        let reader = Params::new(params, self.display).with_variables(&before.state.variables);
        let accelerator = self.compute.accelerator();
        let started = std::time::Instant::now();
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| emcraft_solver::accel::with(accelerator, || (spec.run)(self, &reader))));
        let error = match outcome {
            Ok(Ok(reply)) => {
                // An automatic region follows every change to what it holds (ADR 0005).
                let refreshed = if matches!(spec.effect, Effect::Record | Effect::Adjust) { self.model.refresh_region().map(|_| ()) } else { Ok(()) };
                match refreshed {
                    Ok(()) => {
                        self.commit(spec, params, before, Ran { seconds: started.elapsed().as_secs_f64(), reply: &reply });
                        return Ok(reply);
                    }
                    Err(e) => e.into(),
                }
            }
            Ok(Err(e)) => e,
            Err(_) => EngineError::Panicked(spec.id.to_owned()),
        };
        self.restore(before);
        Err(error)
    }

    /// What a command with `effect` may change, as it is now.
    fn before(&self, effect: Effect) -> Before {
        let history = if matches!(effect, Effect::None | Effect::View) { Arc::default() } else { self.history.clone() };
        // A command that replaces the project keeps the project to undo back to.
        let project = (effect == Effect::Replace).then(|| Box::new(self.identity()));
        Before { state: self.snapshot(history, project), results: self.results.clone(), revision: self.revision, views: self.views.clone() }
    }

    /// Records a command that succeeded by its effect: one that changed the model leaves the
    /// state before it to undo to and joins the history; a solve's results are current.
    fn commit(&mut self, spec: &CommandSpec, params: &Value, before: Before, ran: Ran) {
        match spec.effect {
            Effect::Record | Effect::Adjust | Effect::Replace | Effect::Rebuild => {
                if let Some(change) = spec.invalidates {
                    self.results.invalidate(change);
                }
                // What was hidden stays with its own object when others come and go.
                if spec.invalidates == Some(Change::Geometry) && spec.effect != Effect::Replace {
                    self.views.follow(&before.state.model, &self.model);
                }
                // A recorded change leaves the results it does not drop stale; an adjustment
                // brings them along, and a replaced session has its own.
                if spec.effect == Effect::Record {
                    self.results.age();
                }
                project::push_undo(&mut self.undo, before.state);
                self.redo.clear();
                if matches!(spec.effect, Effect::Record | Effect::Adjust) {
                    Arc::make_mut(&mut self.history).push(Step::new(spec.id, params.clone()));
                }
                // (A replaced session counts its own revisions from zero.)
                self.revision = before.revision.max(self.revision) + 1;
            }
            Effect::Solve => {
                self.results.solved_since(&before.results);
                let params = if params.is_null() { json!({}) } else { params.clone() };
                let run = self.next_run.take().unwrap_or_else(|| Run { command: spec.id.to_owned(), params, units: self.display });
                // A system's chain is read in a moment, again at every change: no history of it.
                if spec.id != "solve.system" {
                    let provenance = crate::archive::Provenance::of(self, spec.id, ran.seconds, ran.reply);
                    self.archive_current = Some(crate::archive::keep(self, &run.command, &run.params, provenance));
                }
                crate::runs::record(&mut self.runs, run);
            }
            Effect::View | Effect::None => {}
        }
    }

    /// Puts back the model, units, variables and results a failed command may have changed.
    fn restore(&mut self, before: Before) {
        self.model = before.state.model;
        self.display = before.state.display;
        self.variables = before.state.variables;
        self.results = before.results;
        self.views = before.views;
        self.next_run = None;
    }

    /// Runs a script: a JSON array of `{"command", "params"}` steps; stops at the first error.
    /// Returns each step's reply.
    pub fn run_script(&mut self, script: &Value) -> Result<Vec<Value>> {
        let steps = script.as_array().ok_or_else(|| EngineError::Params("a script is a JSON array of {\"command\", \"params\"} steps".into()))?;
        let steps = steps
            .iter()
            .enumerate()
            .map(|(i, step)| Step::deserialize(step).map_err(|_| EngineError::Params(format!("step {}: no `command`", i + 1))))
            .collect::<Result<Vec<Step>>>()?;
        self.run_steps(&steps)
    }

    /// Runs `steps` with the mesh commands doing `meshing`, then meshing as it was (whether the
    /// steps succeed or not).
    pub(crate) fn replay(&mut self, steps: &[Step], meshing: Meshing) -> Result<Vec<Value>> {
        let was = std::mem::replace(&mut self.meshing, meshing);
        let done = self.run_steps(steps);
        self.meshing = was;
        done
    }

    /// Runs the steps of a script in order; stops at the first error. Returns each step's
    /// reply.
    pub fn run_steps(&mut self, steps: &[Step]) -> Result<Vec<Value>> {
        steps
            .iter()
            .enumerate()
            .map(|(i, step)| {
                progress::report(i, steps.len(), &step.command)?;
                self.execute(&step.command, &step.params).map_err(|e| e.in_step(i + 1, &step.command))
            })
            .collect()
    }

    /// A frequency (Hz) as a value in display units.
    pub fn show_frequency(&self, hz: f64) -> f64 {
        self.display.frequency.from_si(hz)
    }

    pub fn show_length(&self, m: f64) -> f64 {
        self.display.length.from_si(m)
    }

    /// A short summary of the model and results.
    pub fn summary(&self) -> Value {
        let l = self.display.length;
        let w = self.model.waveguide().map(|w| {
            json!({
                "width": l.from_si(w.width.get()), "height": l.from_si(w.height.get()), "length": l.from_si(w.length()),
                "sections": w.sections.iter().enumerate().map(|(i, s)| {
                    let [a, _] = w.opening(i);
                    json!({"length": l.from_si(s.length.get()), "material": s.material, "aperture": [l.from_si(a.start), l.from_si(a.width)]})
                }).collect::<Vec<_>>(),
            })
        });
        let mut summary = json!({
            "units": crate::reply::units(self.display),
            "analysis": crate::workflow::analysis_reply(&self.model),
            "materials": self.model.materials.iter().map(|m| json!({"name": m.name, "eps_r": m.eps_r, "mu_r": m.mu_r, "loss_tangent": m.loss_tangent, "magnetic_loss_tangent": m.magnetic_loss_tangent})).collect::<Vec<_>>(),
            "pcb":self.model.pcb().map(|b|json!({"name":b.name,"segments":b.segments.len(),"copper_regions":b.copper.len(),"layers":b.layers.iter().map(|l|json!({"name":l.name,"z":self.display.length.from_si(l.z),"eps_below":l.eps_below})).collect::<Vec<_>>()})),
            "waveguide": w,
            "solid": self.model.domain.solid.as_ref().map(|s| json!({"shape": s.shape.kind(), "material": s.material})),
            "ports": self.model.ports.iter().map(|p| {
                let mut v = json!({"name": p.name, "end": p.end, "modes": p.modes});
                if let Some(f) = p.face {
                    v["axis"] = json!(f.axis.id());
                }
                v
            }).collect::<Vec<_>>(),
            "mesh": self.model.mesh.as_ref().map(|m| json!({"nodes": m.nodes.len(), "tetrahedra": m.tets.len()})),
            "results": self.results.sweep.as_ref().map(|s| json!({"order": s.order.as_u8(), "unknowns": s.driven.dofs.unknowns, "frequencies": s.results.len()})),
            "eigenmodes": self.results.eigen.as_ref().map(|e| e.modes.len()),
        });
        if !self.model.system.is_empty() || self.results.system.is_some() {
            let nf = self.results.system.as_ref().and_then(|r| r.nearest(r.at)).and_then(|p| p.rows.last()).and_then(|x| x.nf_db);
            summary["system"] = json!({"stages": self.model.system.stages.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), "nf_db": nf});
        }
        if !self.model.elements.is_empty() {
            summary["elements"] = crate::elements::summary(self);
        }
        summary
    }
}

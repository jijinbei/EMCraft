//! The session's results in a project file, so a project opens without solving again: each
//! result's shape in `project.json` and its numbers array parts under `results/` — one stream of
//! numbers per result, read back in the order written, and the large field vectors (a sweep's,
//! the eigenmodes', an open problem's) parts of their own.
//!
//! No system is written: a sweep's, the eigenmodes' and an open problem's are built again from
//! the model and its mesh (an assembly, no factorization), and the field vectors read must fit
//! them (their length the system's unknowns). A result that does not fit is left out, with the
//! reason, rather than refusing the file.

use std::sync::Arc;

use emcraft_fem::Order;
use emcraft_io::package::{Reader, Writer};
use emcraft_model::{CVec3, Model, Point3, Vec3};
use emcraft_solver::Complex64 as C;
use serde::{Deserialize, Serialize};

use super::{get_complex, put_complex};
use crate::study::{Kind, Predicted, Row, Study, Surrogate};
use crate::{Results, Session, Sweep};

/// Most bytes of field vectors a project file holds; past it the fields are left out.
pub const MAX_FIELD_BYTES: usize = 1 << 30;

/// Most numbers of one result's stream.
const MAX_NUMBERS: usize = 1 << 27;

/// Numbers written in order, to one array part.
#[derive(Default)]
struct Out(Vec<f64>);

impl Out {
    fn f(&mut self, x: f64) {
        self.0.push(x);
    }
    fn fs(&mut self, xs: &[f64]) {
        self.0.extend_from_slice(xs);
    }
    fn c(&mut self, c: C) {
        self.0.extend([c.re, c.im]);
    }
    fn cs(&mut self, cs: &[C]) {
        cs.iter().for_each(|c| self.c(*c));
    }
    fn n(&mut self, n: usize) {
        self.0.push(n as f64);
    }
    fn point(&mut self, p: Point3) {
        self.0.extend([p.x, p.y, p.z]);
    }
    fn cvec(&mut self, v: CVec3) {
        [v.x, v.y, v.z].into_iter().for_each(|c| self.c(c));
    }
    fn write(self, w: &mut Writer, name: &str) -> Result<String, String> {
        w.values(name, self.0).map_err(|e| e.to_string())?;
        Ok(name.to_owned())
    }
}

/// Numbers read back in the order written: running out, or numbers left over, is an error.
struct In {
    v: Vec<f64>,
    at: usize,
    what: &'static str,
}

impl In {
    fn read(r: &mut Reader, name: &str, what: &'static str) -> Result<In, String> {
        Ok(In { v: r.values(name, MAX_NUMBERS).map_err(|e| e.to_string())?, at: 0, what })
    }
    fn fs(&mut self, n: usize) -> Result<Vec<f64>, String> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.v.len()).ok_or_else(|| format!("{}: fewer numbers than its shape says", self.what))?;
        let out = self.v[self.at..end].to_vec();
        self.at = end;
        Ok(out)
    }
    fn f(&mut self) -> Result<f64, String> {
        Ok(self.fs(1)?[0])
    }
    fn c(&mut self) -> Result<C, String> {
        let p = self.fs(2)?;
        Ok(C::new(p[0], p[1]))
    }
    fn cs(&mut self, n: usize) -> Result<Vec<C>, String> {
        let p = self.fs(n.checked_mul(2).ok_or_else(|| format!("{}: too many values", self.what))?)?;
        Ok(p.as_chunks::<2>().0.iter().map(|&[re, im]| C::new(re, im)).collect())
    }
    /// A count or index: a whole number from 0 to 2⁵³.
    fn n(&mut self) -> Result<usize, String> {
        let x = self.f()?;
        if x.fract() == 0.0 && (0.0..=9.007_199_254_740_992e15).contains(&x) {
            Ok(x as usize)
        } else {
            Err(format!("{}: {x} is not a count", self.what))
        }
    }
    fn point(&mut self) -> Result<Point3, String> {
        let p = self.fs(3)?;
        Ok(Point3::new(p[0], p[1], p[2]))
    }
    fn cvec(&mut self) -> Result<CVec3, String> {
        Ok(CVec3 { x: self.c()?, y: self.c()?, z: self.c()? })
    }
    fn done(&self) -> Result<(), String> {
        if self.at == self.v.len() { Ok(()) } else { Err(format!("{}: more numbers than its shape says", self.what)) }
    }
}

fn order(n: u8) -> Result<Order, String> {
    Order::try_from(n).map_err(|n| format!("an element order is 1 or 2, not {n}"))
}

/// A frequency (Hz): finite and positive.
fn frequency(x: f64) -> Result<f64, String> {
    if x.is_finite() && x > 0.0 { Ok(x) } else { Err(format!("{x} is not a frequency")) }
}

/// The session's results in a file.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StoredSession {
    /// The kinds that are stale: the model changed after they were solved without dropping
    /// them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stale: Vec<crate::ResultKind>,
    /// The kept solve they are ([`crate::archive`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_current: Option<u32>,
    /// Whether the field vectors are written (none past [`MAX_FIELD_BYTES`], or when asked).
    pub fields: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sweep: Option<StoredSweep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    eigen: Option<StoredEigen>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    time_domain: Option<StoredTimeDomain>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dispersion: Option<StoredDispersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    study: Option<StoredStudy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    radiation: Option<StoredRadiation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    td_radiation: Option<StoredTdRadiation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revolution: Option<StoredRevolution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bloch: Option<StoredBloch>,
}

/// The bytes the results' field vectors take.
fn field_bytes(r: &Results) -> usize {
    let vectors = r.sweep.iter().flat_map(|sw| sw.results.iter().flat_map(|p| p.fields.iter().map(Vec::len)));
    let modes = r.eigen.iter().flat_map(|e| e.modes.iter().map(|m| m.vector.len()));
    let open = r.radiation.iter().map(|o| o.result.field.len());
    vectors.chain(modes).chain(open).fold(0usize, |a, n| a.saturating_add(n.saturating_mul(16)))
}

impl StoredSession {
    /// `s`'s results written as parts under `results/`, the fields when `fields` and within
    /// [`MAX_FIELD_BYTES`].
    pub(crate) fn write(s: &Session, fields: bool, w: &mut Writer) -> Result<StoredSession, String> {
        let r = &s.results;
        let fields = fields && field_bytes(r) <= MAX_FIELD_BYTES;
        Ok(StoredSession {
            stale: r.stale_kinds().collect(),
            archive_current: s.archive_current,
            fields,
            sweep: r.sweep.as_deref().map(|sw| StoredSweep::write(sw, fields, w)).transpose()?,
            eigen: r.eigen.as_deref().map(|e| StoredEigen::write(e, fields, w)).transpose()?,
            time_domain: r.time_domain.as_deref().map(|t| StoredTimeDomain::write(t, w)).transpose()?,
            dispersion: r.dispersion.as_deref().map(|d| StoredDispersion::write(d, w)).transpose()?,
            study: r.study.as_deref().map(|st| StoredStudy::write(st, w)).transpose()?,
            radiation: r.radiation.as_deref().map(|o| StoredRadiation::write(o, fields, w)).transpose()?,
            td_radiation: r.td_radiation.as_deref().map(|o| StoredTdRadiation::write(o, w)).transpose()?,
            revolution: r.revolution.as_deref().map(|b| StoredRevolution::write(b, w)).transpose()?,
            bloch: r.bloch.as_deref().map(|b| StoredBloch::write(b, w)).transpose()?,
        })
    }

    /// The results for `model` (its mesh on), each checked; those that do not fit are left
    /// out, each with why.
    pub(crate) fn read(self, r: &mut Reader, model: &Model) -> (Results, Vec<super::FileError>) {
        let mut out = Results::default();
        let mut dropped = Vec::new();
        let mut keep = |what: &str, got: Result<(), String>| {
            if let Err(why) = got {
                dropped.push(super::FileError { part: what.to_owned(), why });
            }
        };
        if let Some(x) = self.sweep {
            keep("the sweep", x.read(r, model).map(|v| out.sweep = Some(Arc::new(v))));
        }
        if let Some(x) = self.eigen {
            keep("the eigenmodes", x.read(r, model).map(|v| out.eigen = Some(Arc::new(v))));
        }
        if let Some(x) = self.time_domain {
            keep("the time domain", x.read(r).map(|v| out.time_domain = Some(Arc::new(v))));
        }
        if let Some(x) = self.revolution {
            keep("the body of revolution", x.read(r).map(|v| out.revolution = Some(Arc::new(v))));
        }
        if let Some(x) = self.bloch {
            keep("the Bloch waves", x.read(r).map(|v| out.bloch = Some(Arc::new(v))));
        }
        if let Some(x) = self.dispersion {
            keep("the dispersion", x.read(r).map(|v| out.dispersion = Some(Arc::new(v))));
        }
        if let Some(x) = self.study {
            keep("the study", x.read(r).map(|v| out.study = Some(Arc::new(v))));
        }
        if let Some(x) = self.radiation {
            keep("the open problem", x.read(r, model).map(|v| out.radiation = Some(Arc::new(v))));
        }
        if let Some(x) = self.td_radiation {
            keep("the open time domain", x.read(r).map(|v| out.td_radiation = Some(Arc::new(v))));
        }
        (out, dropped)
    }
}

/// A sweep: its shape (the numbers in `numbers`, each point's fields a part).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredSweep {
    order: u8,
    iterative: bool,
    /// S's entries.
    entries: usize,
    points: Vec<StoredPoint>,
    numbers: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredPoint {
    full: bool,
    /// Which entries' line impedance is known.
    impedances: Vec<bool>,
    /// The field vectors (one per excitation), when written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fields: Option<(usize, String)>,
}

impl StoredSweep {
    fn write(sw: &Sweep, fields: bool, w: &mut Writer) -> Result<StoredSweep, String> {
        let mut nums = Out::default();
        let entries = sw.results.first().map_or(0, |p| p.s.ports());
        let mut points = Vec::with_capacity(sw.results.len());
        for (k, p) in sw.results.iter().enumerate() {
            nums.f(p.frequency);
            p.s.rows().for_each(|row| nums.cs(row));
            p.impedances.iter().flatten().for_each(|z| nums.f(*z));
            nums.f(p.backward_error);
            [p.perturbed_pivots, p.factor_entries, p.largest_front].into_iter().for_each(|n| nums.n(n));
            let field = if fields && !p.fields.is_empty() {
                let name = format!("results/sweep/fields/{k}.c64");
                put_complex(w, &name, &p.fields.concat())?;
                Some((p.fields.len(), name))
            } else {
                None
            };
            points.push(StoredPoint { full: p.full, impedances: p.impedances.iter().map(Option::is_some).collect(), fields: field });
        }
        nums.fs(&sw.deembed);
        Ok(StoredSweep {
            order: sw.order.as_u8(),
            iterative: sw.driven.iterative,
            entries,
            points,
            numbers: nums.write(w, "results/sweep/numbers.f64")?,
        })
    }

    fn read(self, r: &mut Reader, model: &Model) -> Result<Sweep, String> {
        let order = order(self.order)?;
        let mut driven = emcraft_solver::Driven::new(model, order).map_err(|e| e.to_string())?;
        driven.iterative = self.iterative;
        let n = self.entries;
        if n != driven.entries().len() {
            return Err(format!("{n} entries of S, the model's ports {}", driven.entries().len()));
        }
        let unknowns = driven.dofs.unknowns;
        let mut nums = In::read(r, &self.numbers, "the sweep")?;
        let mut results = Vec::with_capacity(self.points.len().min(emcraft_solver::driven::MAX_FREQUENCIES));
        if self.points.len() > emcraft_solver::driven::MAX_FREQUENCIES {
            return Err(format!("more than {} frequencies", emcraft_solver::driven::MAX_FREQUENCIES));
        }
        for p in self.points {
            let f = frequency(nums.f()?)?;
            let s = nums.cs(n * n)?;
            let s = emcraft_solver::SMatrix::from_fn(n, |i, j| s[i * n + j]);
            if p.impedances.len() > n {
                return Err("more impedances than entries".into());
            }
            let impedances = p.impedances.iter().map(|known| if *known { nums.f().map(Some) } else { Ok(None) }).collect::<Result<_, _>>()?;
            let backward_error = nums.f()?;
            let (perturbed_pivots, factor_entries, largest_front) = (nums.n()?, nums.n()?, nums.n()?);
            let fields = match p.fields {
                Some((count, name)) if count <= n => {
                    let all = get_complex(r, &name, count.saturating_mul(unknowns))?;
                    all.chunks(unknowns.max(1)).map(<[C]>::to_vec).collect()
                }
                Some(_) => return Err("more field vectors than entries".into()),
                None => Vec::new(),
            };
            results.push(emcraft_solver::FrequencyResult {
                frequency: f,
                s,
                impedances,
                fields,
                backward_error,
                perturbed_pivots,
                factor_entries,
                largest_front,
                timings: Default::default(),
                full: p.full,
            });
        }
        if results.windows(2).any(|w| w[1].frequency < w[0].frequency) {
            return Err("frequencies must ascend".into());
        }
        let deembed = nums.fs(driven.ports.len())?;
        nums.done()?;
        if !deembed.iter().all(|d| d.is_finite()) {
            return Err("de-embedding must be finite".into());
        }
        let mut sweep = Sweep::new(order, Arc::new(driven), results);
        sweep.deembed = deembed;
        Ok(sweep)
    }
}

/// The eigenmodes: their numbers, and their vectors a part.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredEigen {
    order: u8,
    iterations: usize,
    /// Which of each mode's Q (walls, dielectric, magnetic) are known.
    modes: Vec<[bool; 3]>,
    numbers: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vectors: Option<String>,
    /// Each mode's system, when every wall combination was solved: its index into `walls`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    family: Vec<usize>,
    /// The symmetry planes' walls of each system, the first [`Eigen::driven`]'s.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    walls: Vec<Vec<emcraft_model::SymmetryKind>>,
}

impl StoredEigen {
    fn write(e: &emcraft_solver::eigen::Eigen, fields: bool, w: &mut Writer) -> Result<StoredEigen, String> {
        let mut nums = Out::default();
        for m in &e.modes {
            nums.f(m.frequency);
            [m.q_walls, m.q_dielectric, m.q_magnetic].into_iter().flatten().for_each(|q| nums.f(q));
        }
        let vectors = match fields {
            true => {
                let all: Vec<C> = e.modes.iter().flat_map(|m| m.vector.iter().copied()).collect();
                put_complex(w, "results/eigen/vectors.c64", &all)?;
                Some("results/eigen/vectors.c64".to_owned())
            }
            false => None,
        };
        let several = !e.families.is_empty();
        let walls = |d: &emcraft_solver::Driven| d.symmetry.iter().map(|q| q.kind).collect::<Vec<_>>();
        Ok(StoredEigen {
            family: if several { e.modes.iter().map(|m| m.family).collect() } else { Vec::new() },
            walls: if several { std::iter::once(&e.driven).chain(&e.families).map(walls).collect() } else { Vec::new() },
            order: e.driven.order.as_u8(),
            iterations: e.iterations,
            modes: e.modes.iter().map(|m| [m.q_walls.is_some(), m.q_dielectric.is_some(), m.q_magnetic.is_some()]).collect(),
            numbers: nums.write(w, "results/eigen/numbers.f64")?,
            vectors,
        })
    }

    fn read(self, r: &mut Reader, model: &Model) -> Result<emcraft_solver::eigen::Eigen, String> {
        if self.modes.len() > crate::EIGEN_MAX {
            return Err(format!("more than {} eigenmodes", crate::EIGEN_MAX));
        }
        // Each wall combination's system, when they were solved apart (the first is `driven`).
        let order = order(self.order)?;
        let mut systems = Vec::with_capacity(self.walls.len().max(1));
        if self.walls.is_empty() {
            systems.push(emcraft_solver::Driven::cavity(model, order).map_err(|e| e.to_string())?);
        } else {
            if self.walls.len() > 4 || self.family.len() != self.modes.len() || self.family.iter().any(|k| *k >= self.walls.len()) {
                return Err("the eigenmodes' wall combinations do not match their modes".into());
            }
            for kinds in &self.walls {
                systems.push(emcraft_solver::Driven::cavity(&model.with_walls(kinds), order).map_err(|e| e.to_string())?);
            }
        }
        let unknowns_of = |k: usize| systems.get(k).map_or(0, |d| d.dofs.unknowns);
        let family = |k: usize| self.family.get(k).copied().unwrap_or(0);
        let total: usize = (0..self.modes.len()).map(|k| unknowns_of(family(k))).sum();
        let mut nums = In::read(r, &self.numbers, "the eigenmodes")?;
        let vectors = match &self.vectors {
            Some(name) => get_complex(r, name, total)?,
            None => Vec::new(),
        };
        let mut at = 0usize;
        let mut modes = Vec::with_capacity(self.modes.len());
        for (k, known) in self.modes.iter().enumerate() {
            let frequency = frequency(nums.f()?)?;
            let mut q = [None; 3];
            for (slot, on) in q.iter_mut().zip(known) {
                if *on {
                    *slot = Some(nums.f()?);
                }
            }
            let n = unknowns_of(family(k));
            let vector = vectors.get(at..at + n).map(<[C]>::to_vec).unwrap_or_default();
            at += n;
            modes.push(emcraft_solver::eigen::Mode { frequency, vector, q_walls: q[0], q_dielectric: q[1], q_magnetic: q[2], family: family(k) });
        }
        nums.done()?;
        let mut systems = systems.into_iter();
        let driven = systems.next().ok_or("no system for the eigenmodes")?;
        Ok(emcraft_solver::eigen::Eigen { driven, modes, iterations: self.iterations, families: systems.collect() })
    }
}

fn two_ports() -> usize {
    2
}

fn first_port() -> Vec<usize> {
    vec![0]
}

/// The time domain's S-parameters and how it ran.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredTimeDomain {
    frequencies: usize,
    /// The model's ports and those excited (the guide's two, port 1 excited, when left out).
    #[serde(default = "two_ports")]
    ports: usize,
    #[serde(default = "first_port")]
    inputs: Vec<usize>,
    cells: usize,
    steps: usize,
    reference_steps: usize,
    converged: bool,
    numbers: String,
}

impl StoredTimeDomain {
    fn write(t: &emcraft_fdtd::TdResult, w: &mut Writer) -> Result<StoredTimeDomain, String> {
        let mut nums = Out::default();
        nums.fs(&t.frequencies);
        nums.cs(&t.s);
        nums.fs(&[t.dt, t.loss_reference]);
        Ok(StoredTimeDomain {
            frequencies: t.frequencies.len(),
            ports: t.ports,
            inputs: t.inputs.clone(),
            cells: t.cells,
            steps: t.steps,
            reference_steps: t.reference_steps,
            converged: t.converged,
            numbers: nums.write(w, "results/time_domain/numbers.f64")?,
        })
    }

    fn read(self, r: &mut Reader) -> Result<emcraft_fdtd::TdResult, String> {
        let mut nums = In::read(r, &self.numbers, "the time domain")?;
        let n = self.frequencies;
        let frequencies = nums.fs(n)?;
        let count = n.checked_mul(self.ports).and_then(|x| x.checked_mul(self.inputs.len())).ok_or("the time domain's S-parameters overflow")?;
        let s = nums.cs(count)?;
        let (dt, loss_reference) = (nums.f()?, nums.f()?);
        nums.done()?;
        let t = emcraft_fdtd::TdResult {
            frequencies,
            ports: self.ports,
            inputs: self.inputs,
            s,
            cells: self.cells,
            dt,
            steps: self.steps,
            reference_steps: self.reference_steps,
            converged: self.converged,
            loss_reference,
        };
        // The S-parameters checked as any set of them.
        crate::sdata::of_time_domain(&t)?;
        Ok(t)
    }
}

/// A dispersion diagram: its bands at each phase.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredDispersion {
    axis: usize,
    unknowns: usize,
    /// The bands at each phase.
    bands: Vec<usize>,
    numbers: String,
}

impl StoredDispersion {
    fn write(d: &emcraft_solver::bloch::Dispersion, w: &mut Writer) -> Result<StoredDispersion, String> {
        let mut nums = Out::default();
        nums.f(d.period);
        for p in &d.points {
            nums.fs(&[p.psi, p.beta]);
            nums.fs(&p.frequencies);
            nums.n(p.iterations);
        }
        Ok(StoredDispersion {
            axis: d.axis,
            unknowns: d.unknowns,
            bands: d.points.iter().map(|p| p.frequencies.len()).collect(),
            numbers: nums.write(w, "results/dispersion/numbers.f64")?,
        })
    }

    fn read(self, r: &mut Reader) -> Result<emcraft_solver::bloch::Dispersion, String> {
        if self.axis > 2 {
            return Err("a dispersion's axis is x, y or z".into());
        }
        let mut nums = In::read(r, &self.numbers, "the dispersion")?;
        let period = nums.f()?;
        let mut points = Vec::with_capacity(self.bands.len().min(1 << 12));
        for bands in self.bands {
            let (psi, beta) = (nums.f()?, nums.f()?);
            let frequencies = nums.fs(bands)?;
            let iterations = nums.n()?;
            points.push(emcraft_solver::bloch::BlochPoint { psi, beta, frequencies, iterations });
        }
        nums.done()?;
        if !(period.is_finite() && period > 0.0) {
            return Err("a dispersion's period must be a length".into());
        }
        Ok(emcraft_solver::bloch::Dispersion { axis: self.axis, period, points, unknowns: self.unknowns })
    }
}

/// A study's table: its rows' names and shapes here, their numbers a stream.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredStudy {
    optimize: bool,
    variables: Vec<String>,
    /// Each grid axis' length (a sweep's).
    grid: Vec<usize>,
    solve: String,
    frequency_unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    goal: Option<String>,
    rows: Vec<StoredRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    best: Option<usize>,
    converged: bool,
    /// The surrogate's length scales and whether it predicted the best point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    surrogate: Option<(usize, usize, bool)>,
    numbers: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredRow {
    frequencies: usize,
    s: Vec<String>,
    modes: usize,
    scalars: Vec<String>,
    objective: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// A prediction's S names, its modes' σs and its numbers' names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prediction: Option<(Vec<String>, usize, Vec<String>)>,
}

/// Longest name or text of a study read.
const MAX_STUDY_TEXT: usize = 4096;

fn text(t: String) -> Result<String, String> {
    if t.chars().count() <= MAX_STUDY_TEXT { Ok(t) } else { Err(format!("a study's text is at most {MAX_STUDY_TEXT} characters")) }
}

fn texts(v: Vec<String>) -> Result<Vec<String>, String> {
    if v.len() > crate::study::MAX_VARIABLES.max(64) {
        return Err("a study row names too many values".into());
    }
    v.into_iter().map(text).collect()
}

impl StoredStudy {
    fn write(st: &Study, w: &mut Writer) -> Result<StoredStudy, String> {
        let mut nums = Out::default();
        st.grid.iter().for_each(|axis| nums.fs(axis));
        let mut rows = Vec::with_capacity(st.rows.len());
        for row in &st.rows {
            nums.fs(&row.values);
            nums.fs(&row.frequencies);
            row.s.iter().for_each(|(_, v)| nums.cs(v));
            nums.fs(&row.modes);
            row.scalars.iter().for_each(|(_, x)| nums.f(*x));
            if let Some(o) = row.objective {
                nums.f(o);
            }
            if let Some(p) = &row.prediction {
                p.db.iter().for_each(|(_, db, sigma)| {
                    nums.fs(db);
                    nums.fs(sigma);
                });
                nums.fs(&p.modes_sigma);
                p.scalars_sigma.iter().for_each(|(_, x)| nums.f(*x));
            }
            rows.push(StoredRow {
                frequencies: row.frequencies.len(),
                s: row.s.iter().map(|(name, _)| name.clone()).collect(),
                modes: row.modes.len(),
                scalars: row.scalars.iter().map(|(name, _)| name.clone()).collect(),
                objective: row.objective.is_some(),
                error: row.error.clone(),
                prediction: row.prediction.as_ref().map(|p| {
                    (p.db.iter().map(|(n, _, _)| n.clone()).collect(), p.modes_sigma.len(), p.scalars_sigma.iter().map(|(n, _)| n.clone()).collect())
                }),
            });
        }
        if let Some(g) = &st.surrogate {
            nums.n(g.solves);
            nums.fs(&g.lengths);
            if let Some((mean, sigma)) = g.at_best {
                nums.fs(&[mean, sigma]);
            }
        }
        Ok(StoredStudy {
            optimize: st.kind == Kind::Optimize,
            variables: st.variables.clone(),
            grid: st.grid.iter().map(Vec::len).collect(),
            solve: st.solve.clone(),
            frequency_unit: st.frequency_unit.clone(),
            goal: st.goal.clone(),
            rows,
            best: st.best,
            converged: st.converged,
            surrogate: st.surrogate.as_ref().map(|g| (g.solves, g.lengths.len(), g.at_best.is_some())),
            numbers: nums.write(w, "results/study/numbers.f64")?,
        })
    }

    fn read(self, r: &mut Reader) -> Result<Study, String> {
        let v = self.variables.len();
        if v == 0
            || v > crate::study::MAX_VARIABLES
            || self.grid.len() > crate::study::MAX_SWEPT
            || self.grid.iter().any(|n| *n > crate::study::MAX_POINTS)
        {
            return Err("a study varies 1 to 200 variables, a grid at most 4 axes of 101 values".into());
        }
        let most = crate::study::MAX_GRID_POINTS.max(crate::study::MAX_EVALUATIONS);
        if self.rows.len() > most || self.best.is_some_and(|b| b >= self.rows.len()) {
            return Err(format!("a study has at most {most} rows, its best one of them"));
        }
        if !self.grid.is_empty() && self.grid.iter().product::<usize>() != self.rows.len() {
            return Err("a grid sweep has a row for each point of its grid".into());
        }
        let mut nums = In::read(r, &self.numbers, "the study")?;
        let grid = self.grid.iter().map(|n| nums.fs(*n)).collect::<Result<Vec<_>, _>>()?;
        let mut rows = Vec::with_capacity(self.rows.len());
        for row in self.rows {
            let f = row.frequencies;
            let values = nums.fs(v)?;
            let frequencies = nums.fs(f)?;
            let s = texts(row.s)?.into_iter().map(|name| Ok((name, nums.cs(f)?))).collect::<Result<Vec<_>, String>>()?;
            let modes = nums.fs(row.modes)?;
            let scalars = texts(row.scalars)?.into_iter().map(|name| Ok((name, nums.f()?))).collect::<Result<Vec<_>, String>>()?;
            let objective = if row.objective { Some(nums.f()?) } else { None };
            let prediction = match row.prediction {
                Some((db, sigmas, scalars)) => {
                    let db = texts(db)?.into_iter().map(|name| Ok((name, nums.fs(f)?, nums.fs(f)?))).collect::<Result<Vec<_>, String>>()?;
                    let modes_sigma = nums.fs(sigmas)?;
                    let scalars_sigma = texts(scalars)?.into_iter().map(|name| Ok((name, nums.f()?))).collect::<Result<Vec<_>, String>>()?;
                    Some(Predicted { db, modes_sigma, scalars_sigma })
                }
                None => None,
            };
            let error = row.error.map(text).transpose()?;
            rows.push(Row { values, frequencies, s, modes, scalars, objective, error, prediction });
        }
        let surrogate = match self.surrogate {
            Some((_, lengths, best)) => {
                let solves = nums.n()?;
                let lengths = nums.fs(lengths)?;
                let at_best = if best { Some((nums.f()?, nums.f()?)) } else { None };
                Some(Surrogate { solves, lengths, at_best })
            }
            None => None,
        };
        nums.done()?;
        let (solve, frequency_unit) = (text(self.solve)?, text(self.frequency_unit)?);
        Ok(Study {
            kind: if self.optimize { Kind::Optimize } else { Kind::Sweep },
            variables: texts(self.variables)?,
            grid,
            solve,
            frequency_unit,
            goal: self.goal.map(text).transpose()?,
            rows,
            best: self.best,
            converged: self.converged,
            surrogate,
        })
    }
}

/// The Huygens samples' numbers: point, weight, normal, E and H.
fn put_huygens(nums: &mut Out, h: &emcraft_theory::radiation::Huygens) {
    nums.f(h.frequency);
    nums.point(h.centre);
    for s in &h.samples {
        nums.point(s.point);
        nums.f(s.weight);
        nums.point(Point3::new(s.normal.x, s.normal.y, s.normal.z));
        nums.cvec(s.e);
        nums.cvec(s.h);
    }
}

fn get_huygens(nums: &mut In, samples: usize) -> Result<emcraft_theory::radiation::Huygens, String> {
    let frequency = frequency(nums.f()?)?;
    let centre = nums.point()?;
    let mut out = Vec::with_capacity(samples.min(1 << 20));
    for _ in 0..samples {
        let point = nums.point()?;
        let weight = nums.f()?;
        let n = nums.point()?;
        let (e, h) = (nums.cvec()?, nums.cvec()?);
        out.push(emcraft_theory::radiation::HuygensSample { point, weight, normal: Vec3::new(n.x, n.y, n.z), e, h });
    }
    Ok(emcraft_theory::radiation::Huygens { frequency, centre, samples: out })
}

/// An open problem solved at one frequency.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredRadiation {
    order: u8,
    ports: Vec<String>,
    feed: bool,
    lifted: usize,
    samples: usize,
    numbers: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    field: Option<String>,
}

impl StoredRadiation {
    fn write(o: &crate::open::Radiated, fields: bool, w: &mut Writer) -> Result<StoredRadiation, String> {
        let mut nums = Out::default();
        let res = &o.result;
        nums.fs(&[res.frequency, res.backward_error, o.power]);
        for p in &res.ports {
            [p.current, p.voltage, p.impedance, p.s11].into_iter().for_each(|c| nums.c(c));
            nums.f(p.power);
        }
        if let Some((r, power)) = res.feed {
            nums.c(r);
            nums.f(power);
        }
        for (slot, value) in &res.lifted {
            nums.n(*slot);
            nums.c(*value);
        }
        put_huygens(&mut nums, &o.huygens);
        let field = match fields {
            true => {
                put_complex(w, "results/radiation/field.c64", &res.field)?;
                Some("results/radiation/field.c64".to_owned())
            }
            false => None,
        };
        Ok(StoredRadiation {
            order: o.driven.order.as_u8(),
            ports: res.ports.iter().map(|p| p.name.clone()).collect(),
            feed: res.feed.is_some(),
            lifted: res.lifted.len(),
            samples: o.huygens.samples.len(),
            numbers: nums.write(w, "results/radiation/numbers.f64")?,
            field,
        })
    }

    fn read(self, r: &mut Reader, model: &Model) -> Result<crate::open::Radiated, String> {
        let driven = emcraft_solver::Driven::open(model, order(self.order)?).map_err(|e| e.to_string())?;
        let unknowns = driven.dofs.unknowns;
        let mut nums = In::read(r, &self.numbers, "the open problem")?;
        let (f, backward_error, power) = (frequency(nums.f()?)?, nums.f()?, nums.f()?);
        let ports = texts(self.ports)?
            .into_iter()
            .map(|name| {
                Ok(emcraft_solver::radiation::LumpedResult {
                    name,
                    current: nums.c()?,
                    voltage: nums.c()?,
                    impedance: nums.c()?,
                    s11: nums.c()?,
                    power: nums.f()?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let feed = if self.feed { Some((nums.c()?, nums.f()?)) } else { None };
        let lifted = (0..self.lifted).map(|_| Ok((nums.n()?, nums.c()?))).collect::<Result<Vec<_>, String>>()?;
        if lifted.iter().any(|(slot, _)| *slot >= unknowns.saturating_mul(4).max(1)) || lifted.windows(2).any(|w| w[1].0 <= w[0].0) {
            return Err("a plane wave's conductor values name slots out of order or out of range".into());
        }
        let huygens = get_huygens(&mut nums, self.samples)?;
        nums.done()?;
        let field = match &self.field {
            Some(name) => get_complex(r, name, unknowns)?,
            None => Vec::new(),
        };
        let result = emcraft_solver::radiation::OpenResult { frequency: f, field, backward_error, ports, feed, lifted };
        Ok(crate::open::Radiated { driven, result, huygens, power })
    }
}

/// An open problem run in the time domain.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredTdRadiation {
    frequencies: usize,
    ports: Vec<String>,
    /// The Huygens samples at each frequency.
    samples: Vec<usize>,
    cells: usize,
    steps: usize,
    converged: bool,
    numbers: String,
}

impl StoredTdRadiation {
    fn write(o: &emcraft_fdtd::open::OpenRun, w: &mut Writer) -> Result<StoredTdRadiation, String> {
        let mut nums = Out::default();
        nums.fs(&o.frequencies);
        for p in &o.ports {
            nums.cs(&p.voltage);
            nums.cs(&p.current);
            nums.cs(&p.impedance);
            nums.fs(&p.power);
        }
        nums.fs(&o.radiated);
        o.huygens.iter().for_each(|h| put_huygens(&mut nums, h));
        nums.f(o.dt);
        Ok(StoredTdRadiation {
            frequencies: o.frequencies.len(),
            ports: o.ports.iter().map(|p| p.name.clone()).collect(),
            samples: o.huygens.iter().map(|h| h.samples.len()).collect(),
            cells: o.cells,
            steps: o.steps,
            converged: o.converged,
            numbers: nums.write(w, "results/td_radiation/numbers.f64")?,
        })
    }

    fn read(self, r: &mut Reader) -> Result<emcraft_fdtd::open::OpenRun, String> {
        let mut nums = In::read(r, &self.numbers, "the open time domain")?;
        let n = self.frequencies;
        let frequencies = nums.fs(n)?;
        let ports = texts(self.ports)?
            .into_iter()
            .map(|name| {
                Ok(emcraft_fdtd::open::PortSpectrum { name, voltage: nums.cs(n)?, current: nums.cs(n)?, impedance: nums.cs(n)?, power: nums.fs(n)? })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let radiated = nums.fs(n)?;
        let huygens = self.samples.iter().map(|k| get_huygens(&mut nums, *k)).collect::<Result<Vec<_>, String>>()?;
        let dt = nums.f()?;
        nums.done()?;
        Ok(emcraft_fdtd::open::OpenRun { frequencies, ports, radiated, huygens, cells: self.cells, steps: self.steps, dt, converged: self.converged })
    }
}

/// A body of revolution's S-parameters: its order, its ports' modes, S at each frequency (a
/// part of frequencies, then the values).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredRevolution {
    m: u32,
    modes: Vec<String>,
    frequencies: usize,
    ports: usize,
    unknowns: usize,
    numbers: String,
}

impl StoredRevolution {
    fn write(b: &crate::Revolution, w: &mut Writer) -> Result<StoredRevolution, String> {
        let mut nums = Out::default();
        nums.fs(b.data.frequencies());
        nums.cs(b.data.values());
        Ok(StoredRevolution {
            m: b.m,
            modes: b.modes.clone(),
            frequencies: b.data.frequencies().len(),
            ports: b.data.outputs(),
            unknowns: b.unknowns,
            numbers: nums.write(w, "results/revolution/numbers.f64")?,
        })
    }

    fn read(self, r: &mut Reader) -> Result<crate::Revolution, String> {
        if self.m > 64 || self.modes.len() > crate::sdata::MAX_ENTRIES || self.modes.iter().any(|n| n.chars().count() > 64) {
            return Err("the body of revolution's order or modes are out of range".into());
        }
        let mut nums = In::read(r, &self.numbers, "the body of revolution")?;
        let frequencies = nums.fs(self.frequencies)?;
        let count = self.frequencies.checked_mul(self.ports).and_then(|x| x.checked_mul(self.ports)).ok_or("the body of revolution's S overflows")?;
        let s = nums.cs(count)?;
        nums.done()?;
        let data = crate::sdata::SData::square(frequencies, self.ports, s)?;
        Ok(crate::Revolution { m: self.m, modes: self.modes, data, unknowns: self.unknowns })
    }
}

/// A periodic guide's Bloch waves: the period, the frequencies and each wave's name, its γ
/// among the numbers after the frequencies, wave by wave.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredBloch {
    period: f64,
    frequencies: usize,
    waves: Vec<String>,
    numbers: String,
}

/// The most Bloch waves a project keeps.
const MAX_BLOCH_WAVES: usize = 64;

impl StoredBloch {
    fn write(b: &crate::Bloch, w: &mut Writer) -> Result<StoredBloch, String> {
        let mut nums = Out::default();
        nums.fs(&b.frequencies);
        for wave in &b.waves {
            nums.cs(&wave.gamma);
        }
        Ok(StoredBloch {
            period: b.period,
            frequencies: b.frequencies.len(),
            waves: b.waves.iter().map(|w| w.name.clone()).collect(),
            numbers: nums.write(w, "results/bloch/numbers.f64")?,
        })
    }

    fn read(self, r: &mut Reader) -> Result<crate::Bloch, String> {
        if !(self.period.is_finite() && self.period > 0.0) {
            return Err("the cell's period is not a positive length".into());
        }
        if self.waves.len() > MAX_BLOCH_WAVES || self.waves.iter().any(|n| n.chars().count() > 64) {
            return Err("the Bloch waves are out of range".into());
        }
        let mut nums = In::read(r, &self.numbers, "the Bloch waves")?;
        let frequencies = nums.fs(self.frequencies)?;
        let waves = self
            .waves
            .into_iter()
            .map(|name| Ok(crate::BlochWave { name, gamma: nums.cs(self.frequencies)? }))
            .collect::<Result<Vec<_>, String>>()?;
        nums.done()?;
        Ok(crate::Bloch { period: self.period, frequencies, waves })
    }
}

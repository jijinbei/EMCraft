//! Particle-in-cell runs (docs/adr/0012-particles.md, stage B): the commands that set a run's
//! sources (`particles.*`, kept in [`emcraft_model::particles`]) and `solve.particles`, which
//! runs them in the box of the model's geometry on `emcraft_fdtd::pic`'s grid.
//!
//! **The run.** The box is the geometry's bounds, cut into cells of about the size asked for
//! along each axis; its walls periodic or conducting as set; the time step 0.95 of the grid's
//! stability limit. Plasmas are loaded on a quiet lattice over a neutralising background,
//! their velocities the drift plus the wave; walls emit electrons; a voltage across an axis
//! sets a uniform field held by the outer circuit; a static magnetic field is uniform. Every
//! step is recorded at most [`HISTORY`] times (the field's and the particles' energy, the
//! particles held, the current emitted and collected on each wall, averaged between records)
//! and each probe's field at most [`PROBE_SAMPLES`] times; at the end, the particles (at most
//! [`SNAPSHOT`], evenly chosen) and each conducting wall's current over the run's second half.
//! A probe's frequency is that of the upward crossings of its strongest component through its
//! mean.

use std::sync::Arc;

use emcraft_fdtd::pic::{self, Axis, Grid, Pic, Population};
use emcraft_model::particles::{Emission, Emitter, Face, Particles, Plasma, Voltage, Wall, Wave};
use emcraft_model::units::C0;
use emcraft_particles::{ELECTRON, Pusher, V3, gamma};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{EngineError, Params, Result, Session};

/// The most records of the run's history, samples of each probe and particles kept at the end.
pub const HISTORY: usize = 2000;
pub const PROBE_SAMPLES: usize = 20_000;
pub const SNAPSHOT: usize = 20_000;

/// The time step as a fraction of the grid's stability limit.
const COURANT: f64 = 0.95;

/// The elementary charge (C): electronvolts to joules.
const EV: f64 = 1.602_176_634e-19;

/// A record of the run: its time (s), the energy in the field and in the particles (J), the
/// particles held, the current emitted and collected on each wall (A, by axis, low and high;
/// the charge's rate, negative for electrons), averaged since the last record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub t: f64,
    pub field: f64,
    pub kinetic: f64,
    pub particles: usize,
    pub emitted: f64,
    pub collected: [[f64; 2]; 3],
}

/// A probe: its point (m), its E (V/m) every `every` steps, and its frequency (Hz), if it rings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Probe {
    pub at: [f64; 3],
    pub every: usize,
    pub e: Vec<[f64; 3]>,
    pub frequency: Option<f64>,
}

/// A particle at the end: its position (m), momentum γv (m/s), kinetic energy (eV) and
/// population (the emitted ones first, then each plasma's).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snap {
    pub x: [f64; 3],
    pub u: [f64; 3],
    pub energy: f64,
    pub population: usize,
}

/// A run's result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PicResult {
    /// The box's lowest corner (m), its cells along each axis and their size (m), its walls.
    pub origin: [f64; 3],
    pub cells: [usize; 3],
    pub h: [f64; 3],
    pub walls: [Wall; 3],
    pub dt: f64,
    pub steps: usize,
    pub history: Vec<Record>,
    pub probes: Vec<Probe>,
    pub snapshot: Vec<Snap>,
    /// The particles held at the end, and those dropped because their momentum stopped being a
    /// number.
    pub particles: usize,
    pub lost: usize,
    /// Each wall's current (A) and the current emitted over the run's second half.
    pub collected: [[f64; 2]; 3],
    pub emitted: f64,
    /// The current (A) into the bodies inside the box over the run's second half.
    #[serde(default)]
    pub metal: f64,
    /// With bunches its only source: the field's energy gained over the run per bunch charge
    /// squared (V/C), the loss factor once the bunches have gone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loss_factor: Option<f64>,
}

impl PicResult {
    /// The result checked when read: finite numbers, its lists within their bounds.
    pub(crate) fn check(&self) -> std::result::Result<(), String> {
        let finite = self.origin.iter().chain(&self.h).all(|x| x.is_finite()) && self.dt.is_finite();
        let sized = self.history.len() <= HISTORY + 1 && self.probes.len() <= emcraft_model::particles::MAX_PROBES && self.snapshot.len() <= SNAPSHOT;
        let probes = self.probes.iter().all(|p| p.e.len() <= PROBE_SAMPLES + 1);
        (finite && sized && probes && self.cells.iter().all(|n| *n >= 1)).then_some(()).ok_or_else(|| "a particle run out of its bounds".to_owned())
    }

    /// A conducting wall's area (m²).
    pub fn area(&self, axis: usize) -> f64 {
        (0..3).filter(|d| *d != axis).map(|d| self.cells[d] as f64 * self.h[d]).product()
    }
}

/// The axes' names.
const AXES: [&str; 3] = ["x", "y", "z"];

/// An axis named `x`, `y` or `z`.
fn axis_of(p: &Params, name: &str) -> Result<usize> {
    match p.string(name).required()? {
        "x" => Ok(0),
        "y" => Ok(1),
        "z" => Ok(2),
        other => Err(p.error(name, format!("an axis is x, y or z, not `{other}`"))),
    }
}

/// The model's sources changed by `f`, checked, and the reply: the sources now.
fn change(s: &mut Session, f: impl FnOnce(&mut Particles) -> Result<()>) -> Result<Value> {
    let mut particles = s.model.particles.clone();
    f(&mut particles)?;
    s.model.set_particles(particles)?;
    Ok(sources_reply(s))
}

/// The sources in display units.
pub(crate) fn sources_reply(s: &Session) -> Value {
    let m = &s.model.particles;
    let probes: Vec<Value> = m.probes.iter().map(|x| json!(x.map(|c| s.show_length(c)))).collect();
    json!({
        "walls": {"x": m.walls[0].id(), "y": m.walls[1].id(), "z": m.walls[2].id()},
        "emitters": m.emitters,
        "bunches": m.bunches,
        "beams": m.beams,
        "plasmas": m.plasmas,
        "voltage": m.voltage.map(|v| json!({"axis": AXES[v.axis.min(2)], "volts": v.volts})),
        "magnetic": m.magnetic,
        "probes": probes,
        "open": m.open.iter().map(|f| f.name()).collect::<Vec<_>>(),
    })
}

/// `particles.walls`: how the box ends each axis given.
pub fn walls(s: &mut Session, p: &Params) -> Result<Value> {
    let given: [Option<Wall>; 3] = [p.pick("x").optional()?, p.pick("y").optional()?, p.pick("z").optional()?];
    change(s, |m| {
        for (wall, new) in m.walls.iter_mut().zip(given) {
            if let Some(w) = new {
                *wall = w;
            }
        }
        Ok(())
    })
}

/// `particles.emitter`: a wall emitting electrons.
pub fn emitter(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("face").required()?;
    let face = Face::parse(name).ok_or_else(|| p.error("face", "x-, x+, y-, y+, z- or z+"))?;
    let emission = match p.number("current").optional()? {
        Some(density) => Emission::Current { density },
        None => Emission::Limited,
    };
    let energy = p.number("energy").or(0.0)?;
    let per_node = p.integer("per_node", 1..=emcraft_model::particles::MAX_PER_NODE).or(4)?;
    change(s, |m| {
        m.emitters.push(Emitter { face, emission, energy, per_node });
        Ok(())
    })
}

/// `particles.plasma`: electrons filling the box on a neutralising background.
pub fn plasma(s: &mut Session, p: &Params) -> Result<Value> {
    let density = p.number("density").required()?;
    let per_cell = match p.raw("per_cell") {
        None => [2, 2, 2],
        Some(Value::Array(list)) => {
            let n: Option<Vec<usize>> = list.iter().map(|v| v.as_u64().and_then(|x| usize::try_from(x).ok())).collect();
            match n.as_deref() {
                Some([a, b, c]) => [*a, *b, *c],
                _ => return Err(p.error("per_cell", "an integer or [nx, ny, nz]")),
            }
        }
        Some(_) => [p.integer("per_cell", 1..=emcraft_model::particles::MAX_PER_AXIS).required()?; 3],
    };
    let drift = p.vector("drift").optional()?.map_or([0.0; 3], |v| [v.x, v.y, v.z]);
    let wave = match p.number("wave").optional()? {
        Some(amplitude) => {
            Some(Wave { axis: if p.has("wave_axis") { axis_of(p, "wave_axis")? } else { 0 }, amplitude, modes: p.integer("modes", 1..=1024).or(1)? })
        }
        None => None,
    };
    change(s, |m| {
        m.plasmas.push(Plasma { density, per_cell, drift, wave });
        Ok(())
    })
}

/// `particles.voltage`: a voltage held across an axis's walls, or none.
pub fn voltage(s: &mut Session, p: &Params) -> Result<Value> {
    let v = match p.number("volts").optional()? {
        Some(volts) => Some(Voltage { axis: axis_of(p, "axis")?, volts }),
        None => None,
    };
    change(s, |m| {
        m.voltage = v;
        Ok(())
    })
}

/// `particles.open`: a face left open (or, `closed`, closed again).
pub fn open(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("face").required()?;
    let face = Face::parse(name).ok_or_else(|| p.error("face", "x-, x+, y-, y+, z- or z+"))?;
    let closed = p.flag("closed").or(false)?;
    change(s, |m| {
        m.open.retain(|f| *f != face);
        if !closed {
            m.open.push(face);
        }
        Ok(())
    })
}

/// `particles.magnetic`: the uniform static magnetic field.
pub fn magnetic(s: &mut Session, p: &Params) -> Result<Value> {
    let b = p.vector("b").required()?;
    change(s, |m| {
        m.magnetic = [b.x, b.y, b.z];
        Ok(())
    })
}

/// `particles.probe`: a point whose field is recorded.
pub fn probe(s: &mut Session, p: &Params) -> Result<Value> {
    let at = p.point("at").required()?;
    change(s, |m| {
        m.probes.push([at.x, at.y, at.z]);
        Ok(())
    })
}

/// `particles.clear`: no emitters, plasmas, voltage, magnetic field or probes (the walls stay).
pub fn clear(s: &mut Session, _: &Params) -> Result<Value> {
    change(s, |m| {
        *m = Particles { walls: m.walls, ..Particles::default() };
        Ok(())
    })
}

/// The run's settings: its time (s), cell (m) and push, given or confirmed.
fn settings(s: &Session, p: &Params) -> Result<(f64, f64, Pusher)> {
    let setup = match s.model.job.setup() {
        Some(emcraft_model::Setup::Particles { time, cell, vay }) => Some((time.get(), cell.get(), *vay)),
        _ => None,
    };
    let hint = "give it (or confirm the settings with project.setup)";
    let time = match p.positive("time").optional()? {
        Some(ns) => ns * 1e-9,
        None => setup.map(|x| x.0).ok_or_else(|| p.error("time", hint))?,
    };
    let cell = p.positive_length("cell").or_maybe(setup.map(|x| x.1)).required_or(hint)?;
    let vay = p.flag("vay").or(setup.is_some_and(|x| x.2))?;
    Ok((time, cell, if vay { Pusher::Vay } else { Pusher::Boris }))
}

/// The grid of the model's box in cells of about `cell`.
/// The cells of the absorbing layer beyond an open face.
pub const LAYER: usize = 10;

/// The box the run's grid covers: the geometry's bounds (`lo`, `hi`, m), extended past each open
/// face by its absorbing layer.
struct Region {
    lo: [f64; 3],
    hi: [f64; 3],
    /// The grid's lowest corner (m).
    origin: [f64; 3],
}

impl Region {
    /// A point of the grid (m from its corner) as the model has it: carried back onto the
    /// geometry's box along an axis it was extended along, so the geometry runs on into the
    /// absorbing layers as it is at the open faces.
    /// The two coordinates across `axis` (m from the grid's corner) of `at`, else of the
    /// geometry's centre: where a bunch or beam enters.
    fn centre(&self, axis: usize, at: Option<[f64; 3]>) -> [f64; 2] {
        [(axis + 1) % 3, (axis + 2) % 3].map(|d| at.map_or(0.5 * (self.lo[d] + self.hi[d]), |x| x[d]) - self.origin[d])
    }

    fn model_point(&self, x: V3) -> emcraft_model::Point3 {
        // Just inside the faces: a solid's own faces are not inside it.
        let p: V3 = std::array::from_fn(|d| {
            let inset = 1e-6 * (self.hi[d] - self.lo[d]);
            (x[d] + self.origin[d]).clamp(self.lo[d] + inset, self.hi[d] - inset)
        });
        emcraft_model::Point3::new(p[0], p[1], p[2])
    }
}

/// The grid of the model's box in cells of about `cell`, each open face's layer beyond it.
fn grid(s: &Session, cell: f64) -> Result<(Grid, Region)> {
    let b = s.model.bounds().ok_or_else(|| EngineError::failed("no geometry: the box particles run in is its bounds (geometry.solid)"))?;
    let (lo, hi) = ([b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z]);
    let m = &s.model.particles;
    if let Some(f) = m.open.iter().find(|f| m.walls[f.axis.min(2)] != Wall::Conductor) {
        return Err(EngineError::failed(format!("face {} is open but its axis is periodic: open faces are on axes closed by walls", f.name())));
    }
    let opened = |d: usize, high: bool| usize::from(m.open.contains(&Face { axis: d, high }));
    let mut origin = lo;
    let axes: [Axis; 3] = std::array::from_fn(|d| {
        let length = hi[d] - lo[d];
        let cells = (length / cell).round().clamp(1.0, 1e9) as usize;
        let h = length / cells as f64;
        let (below, above) = (opened(d, false) * LAYER, opened(d, true) * LAYER);
        origin[d] = lo[d] - below as f64 * h;
        let wall = match m.walls[d] {
            Wall::Periodic => pic::Wall::Periodic,
            Wall::Conductor => pic::Wall::Conductor,
        };
        Axis { cells: cells + below + above, h, wall }
    });
    Ok((Grid::new(axes).map_err(fdtd)?, Region { lo, hi, origin }))
}

fn fdtd(e: emcraft_fdtd::FdtdError) -> EngineError {
    EngineError::failed(e.to_string())
}

/// A plasma's electrons on the grid, their velocities the drift plus the wave.
fn load(grid: &Grid, plasma: &Plasma) -> Result<Population> {
    let lengths = grid.lengths();
    let bad = |v: f64| !(v.is_finite() && v.abs() < C0);
    let momentum = |x: V3| -> V3 {
        let mut v = plasma.drift;
        if let Some(w) = plasma.wave {
            let l = lengths.get(w.axis).copied().unwrap_or(1.0);
            if let Some(c) = v.get_mut(w.axis) {
                *c += w.amplitude * (2.0 * std::f64::consts::PI * w.modes as f64 * x[w.axis.min(2)] / l).sin();
            }
        }
        let beta2 = v.iter().map(|c| c * c).sum::<f64>() / (C0 * C0);
        v.map(|c| c / (1.0 - beta2).max(1e-300).sqrt())
    };
    let speed = plasma.drift.iter().map(|c| c * c).sum::<f64>().sqrt() + plasma.wave.map_or(0.0, |w| w.amplitude.abs());
    if bad(speed) {
        return Err(EngineError::failed(format!("a plasma moving at {speed:e} m/s: below the speed of light")));
    }
    Population::uniform(grid, ELECTRON, plasma.density, plasma.per_cell, momentum).map_err(fdtd)
}

/// The run set up from the model's sources.
fn build(s: &Session, grid: Grid, region: &Region, dt: f64, pusher: Pusher) -> Result<Pic> {
    let m = &s.model.particles;
    let mut run = Pic::new(grid, dt, pusher).map_err(fdtd)?;
    for f in &m.open {
        run.absorb(f.axis, f.high, LAYER).map_err(fdtd)?;
    }
    let faces = m.emitters.iter().map(|e| e.face).chain(m.bunches.iter().map(|b| b.face)).chain(m.beams.iter().map(|b| b.face));
    if let Some(f) = faces.into_iter().find(|f| m.open.contains(f)) {
        return Err(EngineError::failed(format!("face {} is open: nothing is emitted through an open face", f.name())));
    }
    // The bodies' conductors and whatever lies outside the solid, as staircases of cells; the
    // dielectrics by their permittivity.
    if s.model.domain.solid.is_some() {
        let materials = &s.model.materials;
        if let Some(m) = materials.iter().find(|m| {
            m.mu_r.get() != 1.0 || !m.loss_tangent.is_zero() || !m.magnetic_loss_tangent.is_zero() || !m.dispersion.is_empty() || m.ferrite.is_some()
        }) {
            return Err(EngineError::failed(format!(
                "not supported yet: particle runs in a lossy, dispersive or magnetic material (`{}`): lossless dielectrics only",
                m.name
            )));
        }
        run.set_media(|x| match s.model.material_at(region.model_point(x)) {
            None => pic::Medium::Metal,
            Some(i) => pic::Medium::Dielectric(materials.get(i).map_or(1.0, |m| m.eps_r.get())),
        });
    }
    let emitted = run.add(Population::new(ELECTRON)).map_err(fdtd)?;
    for plasma in &m.plasmas {
        run.add(load(&grid, plasma)?).map_err(fdtd)?;
    }
    if !m.plasmas.is_empty() {
        run.neutralise();
    }
    if let Some(v) = m.voltage {
        let l = grid.lengths()[v.axis.min(2)];
        run.set_e(v.axis, |_| -v.volts / l);
        run.hold(v.axis).map_err(|e| EngineError::failed(format!("{e}: set the other axes periodic (particles.walls)")))?;
    }
    for (a, b) in m.magnetic.iter().enumerate() {
        if *b != 0.0 {
            run.set_b(a, |_| *b);
        }
    }
    for e in &m.emitters {
        let emission = match e.emission {
            Emission::Limited => pic::Emission::Limited,
            Emission::Current { density } => pic::Emission::Current(density),
        };
        let emitter =
            pic::Emitter { axis: e.face.axis, high: e.face.high, population: emitted, emission, energy: e.energy * EV, per_node: e.per_node };
        run.emit(emitter).map_err(|err| EngineError::failed(format!("the emitter on {}: {err}", e.face.name())))?;
    }
    for b in &m.bunches {
        run.inject(bunch(region, b, emitted)).map_err(|err| EngineError::failed(format!("the bunch through {}: {err}", b.face.name())))?;
    }
    for b in &m.beams {
        run.inject(beam(region, b, emitted)).map_err(|err| EngineError::failed(format!("the beam through {}: {err}", b.face.name())))?;
    }
    Ok(run)
}

/// A bunch on the grid: its duration from its length at its speed, its peak five durations in
/// (or at its delay), its centre the point given or the face's centre.
fn bunch(region: &Region, b: &emcraft_model::particles::Bunch, population: usize) -> pic::Bunch {
    let energy = b.energy * EV;
    let u = ELECTRON.momentum(energy, [1.0, 0.0, 0.0]);
    let speed = emcraft_particles::velocity(u)[0].max(1.0);
    let sigma = b.length / speed;
    let axis = b.face.axis.min(2);
    let centre = region.centre(axis, b.at);
    pic::Bunch {
        axis,
        high: b.face.high,
        population,
        pulse: pic::Pulse::Gaussian { charge: b.charge, t0: b.delay.unwrap_or(5.0 * sigma), sigma },
        centre,
        radius: b.radius,
        inner: 0.0,
        energy,
        per_step: b.per_step,
    }
}

/// A steady beam on the grid: its centre the point given or the face's centre.
fn beam(region: &Region, b: &emcraft_model::particles::Beam, population: usize) -> pic::Bunch {
    let axis = b.face.axis.min(2);
    pic::Bunch {
        axis,
        high: b.face.high,
        population,
        pulse: pic::Pulse::Steady { current: b.current, rise: b.rise },
        centre: region.centre(axis, b.at),
        radius: b.radius,
        inner: b.inner,
        energy: b.energy * EV,
        per_step: b.per_step,
    }
}

/// `particles.beam`: a steady beam of electrons sent in through a conducting face.
pub fn beam_command(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("face").required()?;
    let face = Face::parse(name).ok_or_else(|| p.error("face", "x-, x+, y-, y+, z- or z+"))?;
    let current = p.number("current").required()?;
    let energy = p.positive("energy").required()?;
    let radius = p.length("radius").or(0.0)?;
    let inner = p.length("inner").or(0.0)?;
    let at = p.point("at").optional()?.map(|x| [x.x, x.y, x.z]);
    let rise = p.number("rise").or(0.0)? * 1e-12;
    let per_step = p.integer("per_step", 1..=emcraft_model::particles::MAX_PER_NODE).or(16)?;
    change(s, |m| {
        m.beams.push(emcraft_model::particles::Beam { face, current, energy, radius, inner, at, rise, per_step });
        Ok(())
    })
}

/// `particles.bunch`: a Gaussian bunch of electrons sent in through a conducting face.
pub fn bunch_command(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("face").required()?;
    let face = Face::parse(name).ok_or_else(|| p.error("face", "x-, x+, y-, y+, z- or z+"))?;
    let charge = p.positive("charge").required()? * 1e-9;
    let length = p.positive_length("length").required()?;
    let energy = p.positive("energy").or(1e9)?;
    let radius = p.length("radius").or(0.0)?;
    let at = p.point("at").optional()?.map(|x| [x.x, x.y, x.z]);
    let delay = p.number("delay").optional()?.map(|ps| ps * 1e-12);
    let per_step = p.integer("per_step", 1..=emcraft_model::particles::MAX_PER_NODE).or(8)?;
    change(s, |m| {
        m.bunches.push(emcraft_model::particles::Bunch { face, charge, length, energy, radius, at, delay, per_step });
        Ok(())
    })
}

/// The frequency (Hz) of the upward crossings of `values`' strongest component through its
/// mean, sampled every `dt` (s) — a period from one to the next, whatever offset the mean of a
/// part of a period leaves; none with fewer than three.
fn frequency(values: &[[f64; 3]], dt: f64) -> Option<f64> {
    let n = values.len() as f64;
    let mean = |c: usize| values.iter().map(|v| v[c]).sum::<f64>() / n;
    let spread = |c: usize| values.iter().map(|v| (v[c] - mean(c)).powi(2)).sum::<f64>();
    let c = (0..3).max_by(|a, b| spread(*a).total_cmp(&spread(*b)))?;
    let m = mean(c);
    let mut crossings = Vec::new();
    for (k, w) in values.windows(2).enumerate() {
        let (a, b) = (w[0][c] - m, w[1][c] - m);
        if a < 0.0 && b >= 0.0 {
            crossings.push((k as f64 + a / (a - b)) * dt);
        }
    }
    let (first, last) = (*crossings.first()?, *crossings.last()?);
    (crossings.len() >= 3 && last > first).then(|| (crossings.len() - 1) as f64 / (last - first))
}

/// `solve.particles`: the model's particles run in its box.
pub fn solve(s: &mut Session, p: &Params) -> Result<Value> {
    if !s.model.particles.has_sources() {
        return Err(EngineError::failed("nothing puts particles in the box: an emitter (particles.emitter) or a plasma (particles.plasma)"));
    }
    let (time, cell, pusher) = settings(s, p)?;
    let (grid, region) = grid(s, cell)?;
    let origin = region.origin;
    let dt = grid.time_step(COURANT);
    let steps = (time / dt).ceil();
    if !(steps >= 1.0 && steps <= emcraft_fdtd::MAX_STEPS as f64) {
        return Err(p.error("time", format!("{steps} steps of {dt:e} s: 1 to {}", emcraft_fdtd::MAX_STEPS)));
    }
    let steps = steps as usize;
    let mut run = build(s, grid, &region, dt, pusher)?;
    let field_before = run.energy().field;
    let m = &s.model.particles;
    let only_bunches = !m.bunches.is_empty() && m.emitters.is_empty() && m.plasmas.is_empty() && m.beams.is_empty();
    let probes: Vec<[f64; 3]> = s.model.particles.probes.iter().map(|x| std::array::from_fn(|d| x[d] - origin[d])).collect();
    let (every, probe_every) = (steps.div_ceil(HISTORY), steps.div_ceil(PROBE_SAMPLES));
    let mut history = Vec::new();
    let mut fields: Vec<Vec<[f64; 3]>> = vec![Vec::new(); probes.len()];
    let (mut since, mut late) = (pic::Moved::default(), pic::Moved::default());
    let mut lost = 0;
    let mut charge = 0.0;
    let half = steps / 2;
    for n in 0..steps {
        if n % (steps / 200).max(1) == 0 {
            crate::progress::report(n, steps, "solve.particles")?;
        }
        let moved = run.step().map_err(fdtd)?;
        lost += moved.lost;
        charge += moved.emitted;
        for acc in [&mut since].into_iter().chain((n >= half).then_some(&mut late)) {
            acc.emitted += moved.emitted;
            acc.metal += moved.metal;
            for (a, b) in acc.collected.iter_mut().flatten().zip(moved.collected.iter().flatten()) {
                *a += b;
            }
        }
        if (n + 1) % probe_every == 0 {
            for (f, at) in fields.iter_mut().zip(&probes) {
                f.push(run.e_at(*at));
            }
        }
        if (n + 1) % every == 0 || n + 1 == steps {
            let span = (n + 1 - history.len() * every).max(1) as f64 * dt;
            let energy = run.energy();
            let rate = |q: f64| q / span;
            history.push(Record {
                t: run.time(),
                field: energy.field,
                kinetic: energy.kinetic,
                particles: run.particles(),
                emitted: rate(since.emitted),
                collected: since.collected.map(|w| w.map(rate)),
            });
            since = pic::Moved::default();
        }
    }
    let span = (steps - half) as f64 * dt;
    let result = PicResult {
        origin,
        cells: grid.axes.map(|a| a.cells),
        h: grid.axes.map(|a| a.h),
        walls: s.model.particles.walls,
        dt,
        steps,
        history,
        probes: probes
            .iter()
            .zip(fields)
            .map(|(at, e)| Probe {
                at: std::array::from_fn(|d| at[d] + origin[d]),
                every: probe_every,
                frequency: frequency(&e, probe_every as f64 * dt),
                e,
            })
            .collect(),
        snapshot: snapshot(&run, origin),
        particles: run.particles(),
        lost,
        collected: late.collected.map(|w| w.map(|q| q / span)),
        emitted: late.emitted / span,
        metal: late.metal / span,
        loss_factor: (only_bunches && charge != 0.0).then(|| (run.energy().field - field_before) / (charge * charge)),
    };
    let reply = reply(s, &result);
    s.results.particles = Some(Arc::new(result));
    Ok(reply)
}

/// At most [`SNAPSHOT`] of the run's particles, evenly chosen.
fn snapshot(run: &Pic, origin: [f64; 3]) -> Vec<Snap> {
    let stride = run.particles().div_ceil(SNAPSHOT).max(1);
    run.populations()
        .iter()
        .enumerate()
        .flat_map(|(k, pop)| pop.particles.iter().map(move |p| (k, pop.species, p)))
        .step_by(stride)
        .map(|(population, species, p)| Snap {
            x: std::array::from_fn(|d| p.x[d] + origin[d]),
            u: p.u,
            energy: species.kinetic(p.u) / EV,
            population,
        })
        .collect()
}

/// The axis the particles run along: the first beam's, bunch's or emitter's normal, else a plasma
/// wave's, else z.
pub fn along(s: &Session) -> usize {
    let m = &s.model.particles;
    let face = m.beams.first().map(|b| b.face).or_else(|| m.bunches.first().map(|b| b.face)).or_else(|| m.emitters.first().map(|e| e.face));
    face.map(|f| f.axis).or_else(|| m.plasmas.iter().find_map(|p| p.wave.map(|w| w.axis))).or_else(|| m.voltage.map(|v| v.axis)).unwrap_or(2).min(2)
}

/// The mean γ of the last particles in the middle third of the box along `axis` (none there:
/// none), a beam's after its space charge has slowed it.
fn mid_gamma(r: &PicResult, axis: usize) -> Option<f64> {
    let (lo, l) = (r.origin[axis], r.cells[axis] as f64 * r.h[axis]);
    let mid: Vec<f64> = r.snapshot.iter().filter(|p| p.x[axis] > lo + l / 3.0 && p.x[axis] < lo + 2.0 * l / 3.0).map(|p| gamma(p.u)).collect();
    (!mid.is_empty()).then(|| mid.iter().sum::<f64>() / mid.len() as f64)
}

/// A run's reply in display units: its grid, the particles at the end, each conducting wall's
/// current and density over the second half and the bodies', each probe's frequency, and with
/// bunches alone the loss factor (V/pC).
pub(crate) fn reply(s: &Session, r: &PicResult) -> Value {
    let mut walls = serde_json::Map::new();
    for axis in 0..3 {
        if r.walls[axis] != Wall::Conductor {
            continue;
        }
        for (side, high) in [(0, false), (1, true)] {
            let current = r.collected[axis][side];
            walls.insert(Face { axis, high }.name(), json!({"current": current, "density": current / r.area(axis)}));
        }
    }
    let probes: Vec<Value> =
        r.probes.iter().map(|p| json!({"at": p.at.map(|c| s.show_length(c)), "frequency": p.frequency.map(|f| s.show_frequency(f))})).collect();
    let last = r.history.last();
    let fastest = r.snapshot.iter().map(|p| p.energy).fold(0.0, f64::max);
    json!({
        "cells": r.cells,
        "cell": r.h.map(|h| s.show_length(h)),
        "dt": r.dt,
        "steps": r.steps,
        "time_ns": r.steps as f64 * r.dt * 1e9,
        "particles": r.particles,
        "lost": r.lost,
        "energy": {"field": last.map(|h| h.field), "kinetic": last.map(|h| h.kinetic)},
        "walls": walls,
        "emitted": r.emitted,
        "probes": probes,
        "fastest_ev": fastest,
        "gamma_max": r.snapshot.iter().map(|p| gamma(p.u)).fold(1.0, f64::max),
        "metal": r.metal,
        "mid_gamma": mid_gamma(r, along(s)),
        "growth_rate": pic::growth_rate(&r.history.iter().map(|h| h.t).collect::<Vec<_>>(), &r.history.iter().map(|h| h.field).collect::<Vec<_>>()),
        "loss_factor_v_per_pc": r.loss_factor.map(|k| k * 1e-12),
    })
}

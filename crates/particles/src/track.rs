//! A particle tracked through fields along a line: leapfrogged (positions at whole steps,
//! momenta at the half steps between), its points recorded, and how it ended.
//!
//! The fields come through [`Fields`]: a field where there is one, none (free space past the
//! line's ends), or a wall the particle is lost on. The line is an entry plane through
//! `origin`, normal to `axis`, and an exit plane `length` along it: a particle that crosses the
//! exit plane has gone through, one that falls back behind the entry plane went back.

use crate::push::{Pusher, Species, V3, add, dot, push, scale, velocity};

/// The most steps one particle is pushed.
pub const MAX_STEPS: usize = 10_000_000;

/// What there is at a point and time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sample {
    /// E (V/m) and B (T).
    Field(V3, V3),
    /// No field: free space.
    Free,
    /// A wall: the particle is lost on it.
    Wall,
}

/// Fields a particle is pushed through: what there is at point `x` (m) and time `t` (s).
pub trait Fields {
    fn at(&self, x: V3, t: f64) -> Sample;
}

impl<F: Fn(V3, f64) -> Sample> Fields for F {
    fn at(&self, x: V3, t: f64) -> Sample {
        self(x, t)
    }
}

/// Where the particle runs: the entry plane through `origin` normal to the unit vector `axis`,
/// the exit plane `length` (m) along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub origin: V3,
    pub axis: V3,
    pub length: f64,
}

impl Line {
    /// How far along the line `x` is (m).
    pub fn along(&self, x: V3) -> f64 {
        dot(add(x, scale(self.origin, -1.0)), self.axis)
    }
}

/// How a run is stepped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub pusher: Pusher,
    /// The time step (s).
    pub dt: f64,
    pub max_steps: usize,
    /// A point recorded every this many steps (the first and the last always).
    pub every: usize,
}

/// How a particle's run ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum End {
    /// Past the exit plane.
    Through,
    /// Back behind the entry plane.
    Back,
    /// On a wall, at this point (m).
    Lost(V3),
    /// The steps ran out.
    Steps,
    /// The field or the momentum stopped being a number (a field beyond any physics).
    NotFinite,
}

/// A recorded point: the time (s), the position (m) and the momentum γv (m/s) there (the mean
/// of the half steps either side).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub t: f64,
    pub x: V3,
    pub u: V3,
}

/// A particle's run: its recorded points, how it ended, its steps, and its momentum at the end
/// (m/s, past the last field it felt: exact there once the fields are behind it).
#[derive(Clone, Debug, PartialEq)]
pub struct Trajectory {
    pub points: Vec<Point>,
    pub end: End,
    pub steps: usize,
    pub u: V3,
}

/// Why a run cannot start.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum TrackError {
    #[error("the time step must be positive and finite, got {0}")]
    Step(f64),
    #[error("the steps must be 1 to {MAX_STEPS}, got {0}")]
    Steps(usize),
    #[error("the line must have a unit axis and a positive length")]
    Line,
    #[error("the particle's start must be finite")]
    Start,
}

/// Where and how a particle starts: its position (m), momentum γv (m/s) and time (s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Start {
    pub x: V3,
    pub u: V3,
    pub t: f64,
}

/// A particle of `species` from `start`, tracked through `fields` along `line` with
/// `settings`.
///
/// ```
/// use emcraft_particles::{ELECTRON, End, Line, Pusher, Sample, Settings, Start, track};
/// // A 1 MeV electron through 10 cm of free space: it goes through, its energy kept.
/// let mev = 1e6 * 1.602_176_634e-19;
/// let u = ELECTRON.momentum(mev, [0.0, 0.0, 1.0]);
/// let line = Line { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], length: 0.1 };
/// let settings = Settings { pusher: Pusher::Boris, dt: 1e-12, max_steps: 10_000, every: 10 };
/// let start = Start { x: [0.0; 3], u, t: 0.0 };
/// let run = track(ELECTRON, start, &|_: [f64; 3], _: f64| Sample::Free, line, settings)?;
/// assert_eq!(run.end, End::Through);
/// assert!((ELECTRON.kinetic(run.u) - mev).abs() < 1e-9 * mev);
/// # Ok::<(), emcraft_particles::TrackError>(())
/// ```
pub fn track(species: Species, start: Start, fields: &impl Fields, line: Line, settings: Settings) -> Result<Trajectory, TrackError> {
    let Settings { pusher, dt, max_steps, every } = settings;
    let Start { x, u, t: t0 } = start;
    if !(dt.is_finite() && dt > 0.0) {
        return Err(TrackError::Step(dt));
    }
    if max_steps == 0 || max_steps > MAX_STEPS {
        return Err(TrackError::Steps(max_steps));
    }
    if !((dot(line.axis, line.axis) - 1.0).abs() < 1e-9 && line.length.is_finite() && line.length > 0.0) {
        return Err(TrackError::Line);
    }
    if !(x.iter().chain(&u).all(|v| v.is_finite()) && t0.is_finite()) {
        return Err(TrackError::Start);
    }
    let every = every.max(1);
    let fields_at = |x: V3, t: f64| match fields.at(x, t) {
        Sample::Field(e, b) => Some((e, b)),
        Sample::Free => Some(([0.0; 3], [0.0; 3])),
        Sample::Wall => None,
    };
    let finite = |v: V3| v.iter().all(|c| c.is_finite());
    let mut points = vec![Point { t: t0, x, u }];
    let Some((e, b)) = fields_at(x, t0) else {
        return Ok(Trajectory { points, end: End::Lost(x), steps: 0, u });
    };
    // u from t0 to t0 + Δt/2: half a step with the start's fields.
    let mut half = push(pusher, species, u, e, b, 0.5 * dt);
    let (mut x, mut t) = (x, t0);
    let mut end = End::Steps;
    let mut steps = 0;
    for n in 1..=max_steps {
        steps = n;
        x = add(x, scale(velocity(half), dt));
        t = t0 + dt * n as f64;
        if !finite(x) || !finite(half) {
            end = End::NotFinite;
            break;
        }
        let s = line.along(x);
        let leaving = if s > line.length {
            Some(End::Through)
        } else if s < 0.0 {
            Some(End::Back)
        } else {
            None
        };
        let Some((e, b)) = fields_at(x, t) else {
            end = End::Lost(x);
            break;
        };
        if !(finite(e) && finite(b)) {
            end = End::NotFinite;
            break;
        }
        let next = push(pusher, species, half, e, b, dt);
        if n % every == 0 || leaving.is_some() {
            points.push(Point { t, x, u: scale(add(half, next), 0.5) });
        }
        half = next;
        if let Some(how) = leaving {
            end = how;
            break;
        }
    }
    if end != End::NotFinite && points.last().is_none_or(|p| p.t != t) {
        points.push(Point { t, x, u: half });
    }
    Ok(Trajectory { points, end, steps, u: half })
}

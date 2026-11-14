//! A particle-in-cell run's sources (docs/adr/0012-particles.md, stage B): how the box's walls
//! end each axis, the walls that emit electrons, the plasmas loaded at the start, a voltage
//! held across an axis, a uniform static magnetic field and the points whose field is recorded.
//! The box is the model's geometry's bounds; the engine's `solve.particles` runs it.
//!
//! Lengths are in metres, densities in m⁻³, velocities in m/s, energies in eV, voltages in V,
//! current densities in A/m², magnetic fields in T.

use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid};
use crate::model::Model;

/// The most emitters, plasmas and probes a run holds.
pub const MAX_EMITTERS: usize = 6;
pub const MAX_PLASMAS: usize = 8;
pub const MAX_PROBES: usize = 16;

/// The most macro-particles a plasma loads in a cell along an axis, and in a cell in all.
pub const MAX_PER_AXIS: usize = 64;
pub const MAX_PER_CELL: usize = 512;

/// The largest density (m⁻³), energy (eV), voltage (V), current density (A/m²), speed (m/s)
/// and magnetic field (T) a run takes.
pub const MAX_DENSITY: f64 = 1e32;
pub const MAX_ENERGY: f64 = 1e10;
pub const MAX_VOLTAGE: f64 = 1e10;
pub const MAX_CURRENT_DENSITY: f64 = 1e14;
pub const MAX_SPEED: f64 = 3e10;
pub const MAX_MAGNETIC: f64 = 1e4;

/// The most macro-particles a wall's node emits a step.
pub const MAX_PER_NODE: usize = 64;

emcraft_macros::named_enum! {
    /// How the box ends an axis.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum Wall {
        /// Perfectly conducting walls at both ends: particles reaching them are absorbed.
        #[default]
        Conductor = "conductor",
        /// What leaves through one end comes back through the other.
        Periodic = "periodic",
    }
}

emcraft_macros::named_serde!(Wall);

/// A face of the box: normal to `axis` (0 to 2), at its high end or its low one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Face {
    pub axis: usize,
    pub high: bool,
}

impl Face {
    /// The face named `x-`, `x+`, `y-`, `y+`, `z-` or `z+`.
    ///
    /// ```
    /// use emcraft_model::particles::Face;
    /// assert_eq!(Face::parse("z+"), Some(Face { axis: 2, high: true }));
    /// assert_eq!(Face::parse("z+").map(|f| f.name()), Some("z+".to_owned()));
    /// assert_eq!(Face::parse("w-"), None);
    /// ```
    pub fn parse(s: &str) -> Option<Face> {
        let mut c = s.chars();
        let axis = match c.next()? {
            'x' => 0,
            'y' => 1,
            'z' => 2,
            _ => return None,
        };
        let high = match (c.next()?, c.next()) {
            ('+', None) => true,
            ('-', None) => false,
            _ => return None,
        };
        Some(Face { axis, high })
    }

    /// Its name, as [`Face::parse`] reads it.
    pub fn name(self) -> String {
        format!("{}{}", ["x", "y", "z"].get(self.axis).copied().unwrap_or("?"), if self.high { '+' } else { '-' })
    }
}

impl TryFrom<String> for Face {
    type Error = String;
    fn try_from(s: String) -> std::result::Result<Face, String> {
        Face::parse(&s).ok_or_else(|| format!("a face is x-, x+, y-, y+, z- or z+, not `{s}`"))
    }
}

impl From<Face> for String {
    fn from(f: Face) -> String {
        f.name()
    }
}

/// How much a wall emits.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Emission {
    /// As much as the field at the wall draws (space-charge limited).
    Limited,
    /// A current density (A/m²).
    Current { density: f64 },
}

/// A wall emitting electrons.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Emitter {
    pub face: Face,
    pub emission: Emission,
    /// The electrons' kinetic energy as they leave (eV).
    #[serde(default)]
    pub energy: f64,
    /// Macro-particles from each node of the wall a step.
    pub per_node: usize,
}

/// A wave on a plasma's velocities: `amplitude` (m/s) along `axis`, `modes` wavelengths across
/// the box along it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wave {
    pub axis: usize,
    pub amplitude: f64,
    pub modes: usize,
}

/// Electrons of `density` filling the box at the start on a neutralising background of
/// immobile ions, `per_cell` macro-particles a cell along each axis, moving at `drift` with
/// `wave` on it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plasma {
    pub density: f64,
    pub per_cell: [usize; 3],
    #[serde(default)]
    pub drift: [f64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wave: Option<Wave>,
}

/// A voltage held across the walls of `axis` by the outer circuit: the high wall `volts` above
/// the low one.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Voltage {
    pub axis: usize,
    pub volts: f64,
}

/// The run's sources.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Particles {
    #[serde(default)]
    pub walls: [Wall; 3],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emitters: Vec<Emitter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plasmas: Vec<Plasma>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage: Option<Voltage>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub magnetic: [f64; 3],
    /// Points whose field is recorded (m, the model's coordinates).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub probes: Vec<[f64; 3]>,
}

fn is_zero(b: &[f64; 3]) -> bool {
    b.iter().all(|x| *x == 0.0)
}

/// Whether `x` is a finite number of at most `max` in size (and ≥ 0 when `signed` is false).
fn within(x: f64, max: f64, signed: bool) -> bool {
    x.is_finite() && x.abs() <= max && (signed || x >= 0.0)
}

impl Particles {
    /// Whether it holds anything to run: an emitter or a plasma.
    pub fn is_empty(&self) -> bool {
        *self == Particles::default()
    }

    /// Whether something puts particles in the box.
    pub fn has_sources(&self) -> bool {
        !(self.emitters.is_empty() && self.plasmas.is_empty())
    }

    /// The sources checked: their counts and every number within its bounds.
    ///
    /// ```
    /// use emcraft_model::particles::{Face, Emission, Emitter, Particles};
    /// let mut p = Particles::default();
    /// p.emitters.push(Emitter { face: Face { axis: 2, high: false }, emission: Emission::Limited, energy: 0.0, per_node: 4 });
    /// assert!(p.check().is_ok());
    /// p.emitters[0].per_node = 0;
    /// assert!(p.check().is_err());
    /// ```
    pub fn check(&self) -> Result<()> {
        if self.emitters.len() > MAX_EMITTERS || self.plasmas.len() > MAX_PLASMAS || self.probes.len() > MAX_PROBES {
            return Err(invalid("particles", format!("at most {MAX_EMITTERS} emitters, {MAX_PLASMAS} plasmas and {MAX_PROBES} probes")));
        }
        for e in &self.emitters {
            let current_ok = match e.emission {
                Emission::Limited => true,
                Emission::Current { density } => within(density, MAX_CURRENT_DENSITY, false),
            };
            if e.face.axis > 2 || !current_ok || !within(e.energy, MAX_ENERGY, false) || !(1..=MAX_PER_NODE).contains(&e.per_node) {
                return Err(invalid(
                    "emitter",
                    format!("{e:?}: a face, a current density ≥ 0, an energy ≥ 0 and 1 to {MAX_PER_NODE} particles a node"),
                ));
            }
        }
        for p in &self.plasmas {
            let per = p.per_cell.iter().all(|n| (1..=MAX_PER_AXIS).contains(n)) && p.per_cell.iter().product::<usize>() <= MAX_PER_CELL;
            let wave = p.wave.is_none_or(|w| w.axis <= 2 && within(w.amplitude, MAX_SPEED, true) && (1..=1024).contains(&w.modes));
            if !(within(p.density, MAX_DENSITY, false) && per && p.drift.iter().all(|v| within(*v, MAX_SPEED, true)) && wave) {
                return Err(invalid(
                    "plasma",
                    format!("{p:?}: a density ≥ 0, 1 to {MAX_PER_AXIS} particles a cell along each axis ({MAX_PER_CELL} in all), finite speeds"),
                ));
            }
        }
        if let Some(v) = self.voltage
            && (v.axis > 2 || !within(v.volts, MAX_VOLTAGE, true))
        {
            return Err(invalid("voltage", format!("{v:?}: an axis and a finite voltage")));
        }
        if !self.magnetic.iter().all(|b| within(*b, MAX_MAGNETIC, true)) || !self.probes.iter().flatten().all(|x| x.is_finite()) {
            return Err(invalid("particles", "the magnetic field and the probes must be finite"));
        }
        Ok(())
    }
}

impl Model {
    /// Sets the particle-in-cell run's sources after checking them.
    pub fn set_particles(&mut self, particles: Particles) -> Result<()> {
        particles.check()?;
        self.particles = particles;
        Ok(())
    }
}

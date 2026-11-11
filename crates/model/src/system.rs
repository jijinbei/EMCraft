//! A system: a chain of two-port stages from a source (an antenna's terminals) to a receiver —
//! amplifiers, attenuators, measured or solved S-parameters, lines and lumped elements — whose
//! gain, noise and linearity the engine reads stage by stage (`solve.system`, its budget). The
//! model holds what each stage is; the engine evaluates them.
//!
//! Lengths are in metres, frequencies in hertz, attenuation in nepers per metre; gains, losses
//! and noise figures are in dB, powers in dBm, temperatures in kelvin, as a budget states them.

use serde::{Deserialize, Serialize};

use crate::element::LumpedElement;
use crate::error::{ModelError, Result, invalid, named};
use crate::model::Model;

/// The most stages a chain holds.
pub const MAX_STAGES: usize = 32;

/// The most gain an amplifier stage has, and the most loss an attenuator (dB).
pub const MAX_GAIN_DB: f64 = 100.0;

/// The greatest noise figure a stage has (dB).
pub const MAX_FIGURE_DB: f64 = 60.0;

/// The span of powers a budget takes (dBm): intercept points, compression points, the signal.
pub const MAX_POWER_DBM: f64 = 100.0;

/// The hottest a source or the chain's parts may be (K).
pub const MAX_TEMPERATURE: f64 = 1e4;

/// The longest a line stage may be (m).
pub const MAX_LINE_LENGTH: f64 = 1e3;

/// The most attenuation a line stage has (Np/m).
pub const MAX_LINE_ATTENUATION: f64 = 1e6;

/// The standard temperature T₀ (K).
pub const T0: f64 = 290.0;

/// The chain: its stages from the source on, the source and the receiver's needs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Chain {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<Stage>,
    #[serde(default)]
    pub source: Source,
    #[serde(default)]
    pub receiver: Receiver,
}

/// A stage: its name (unique in the chain) and what it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    pub name: String,
    pub part: Part,
}

/// What a stage is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    /// An ideal amplifier, matched and unilateral on the chain's impedance: its gain and noise
    /// figure from a matched source (dB), and where known its output third-order intercept and
    /// output 1 dB compression point (dBm).
    Amplifier {
        gain: f64,
        noise_figure: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        oip3: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        p1db: Option<f64>,
    },
    /// A matched attenuator of `loss` dB at the chain's temperature.
    Attenuator { loss: f64 },
    /// A two-port's S-parameters from `data` (its ports swapped when `flip`).
    Data {
        data: DataSource,
        #[serde(default)]
        flip: bool,
    },
    /// A uniform line: impedance (Ω), length (m), relative permittivity, a conductor's
    /// attenuation (Np/m, growing as √(f/`reference`) from `reference` Hz when given) and a
    /// dielectric's loss tangent.
    Line {
        impedance: f64,
        length: f64,
        #[serde(default = "one")]
        eps_r: f64,
        #[serde(default)]
        attenuation: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference: Option<f64>,
        #[serde(default)]
        tan_delta: f64,
    },
    /// A lumped element in series along the line, or `shunt` across it.
    Element { element: LumpedElement, shunt: bool },
}

fn one() -> f64 {
    1.0
}

/// Where a data stage's S-parameters come from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum DataSource {
    /// A measurement kept with the project (results.import: a Touchstone file, a template's).
    Measurement { name: String },
    /// The project's own sweep.
    Sweep,
}

/// The chain's source: an antenna's terminals (a one-port measurement's impedance; none: the
/// chain's impedance), its noise temperature (K) and the signal it gives (dBm), when given.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub antenna: Option<String>,
    #[serde(default = "standard")]
    pub temperature: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power: Option<f64>,
}

fn standard() -> f64 {
    T0
}

impl Default for Source {
    fn default() -> Source {
        Source { antenna: None, temperature: T0, power: None }
    }
}

/// What the chain ends in: its reference impedance (Ω), the physical temperature of its
/// passive parts (K), and the receiver's noise bandwidth (Hz) and signal-to-noise ratio (dB).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Receiver {
    #[serde(default = "fifty")]
    pub impedance: f64,
    #[serde(default = "standard")]
    pub temperature: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bandwidth: Option<f64>,
    #[serde(default)]
    pub snr: f64,
}

fn fifty() -> f64 {
    50.0
}

impl Default for Receiver {
    fn default() -> Receiver {
        Receiver { impedance: 50.0, temperature: T0, bandwidth: None, snr: 0.0 }
    }
}

/// `v` finite and within `lo..=hi`, else why not.
fn within(what: &str, v: f64, lo: f64, hi: f64) -> Result<()> {
    if v.is_finite() && (lo..=hi).contains(&v) { Ok(()) } else { Err(invalid(what, format!("must be from {lo} to {hi}"))) }
}

impl Part {
    /// Checks the part's own settings, each in its range.
    ///
    /// ```
    /// use emcraft_model::system::Part;
    /// let lna = Part::Amplifier { gain: 20.0, noise_figure: 0.6, oip3: Some(30.0), p1db: None };
    /// assert!(lna.check().is_ok());
    /// assert!(Part::Amplifier { gain: 1e9, noise_figure: 0.6, oip3: None, p1db: None }.check().is_err());
    /// assert!(Part::Attenuator { loss: -3.0 }.check().is_err());
    /// ```
    pub fn check(&self) -> Result<()> {
        match self {
            Part::Amplifier { gain, noise_figure, oip3, p1db } => {
                within("amplifier gain (dB)", *gain, -MAX_GAIN_DB, MAX_GAIN_DB)?;
                within("amplifier noise figure (dB)", *noise_figure, 0.0, MAX_FIGURE_DB)?;
                for (what, p) in [("amplifier OIP3 (dBm)", oip3), ("amplifier P1dB (dBm)", p1db)] {
                    if let Some(p) = p {
                        within(what, *p, -MAX_POWER_DBM, MAX_POWER_DBM)?;
                    }
                }
                Ok(())
            }
            Part::Attenuator { loss } => within("attenuator loss (dB)", *loss, 0.0, MAX_GAIN_DB),
            Part::Data { data: DataSource::Measurement { name }, .. } => named("data stage's measurement", name),
            Part::Data { data: DataSource::Sweep, .. } => Ok(()),
            Part::Line { impedance, length, eps_r, attenuation, reference, tan_delta } => {
                within("line impedance (Ω)", *impedance, 1e-6, 1e6)?;
                within("line length (m)", *length, 0.0, MAX_LINE_LENGTH)?;
                within("line relative permittivity", *eps_r, 1.0, 1e4)?;
                within("line attenuation (Np/m)", *attenuation, 0.0, MAX_LINE_ATTENUATION)?;
                within("line loss tangent", *tan_delta, 0.0, 1.0)?;
                match reference {
                    Some(r) if !(r.is_finite() && *r > 0.0) => Err(invalid("line attenuation's frequency", "must be a positive frequency")),
                    _ => Ok(()),
                }
            }
            Part::Element { element, .. } => element.check(),
        }
    }

    /// The part's kind as `system.stage.add` names it.
    pub fn kind(&self) -> &'static str {
        match self {
            Part::Amplifier { .. } => "amplifier",
            Part::Attenuator { .. } => "attenuator",
            Part::Data { .. } => "data",
            Part::Line { .. } => "line",
            Part::Element { shunt: false, .. } => "series",
            Part::Element { shunt: true, .. } => "shunt",
        }
    }
}

impl Source {
    /// Checks the source: its temperature from 0 to [`MAX_TEMPERATURE`], its power in range, its
    /// antenna's name not empty.
    pub fn check(&self) -> Result<()> {
        within("antenna temperature (K)", self.temperature, 0.0, MAX_TEMPERATURE)?;
        if let Some(p) = self.power {
            within("source power (dBm)", p, -MAX_POWER_DBM - 100.0, MAX_POWER_DBM)?;
        }
        match &self.antenna {
            Some(a) => named("source antenna", a),
            None => Ok(()),
        }
    }
}

impl Receiver {
    /// Checks the receiver: a positive impedance, a temperature above 0, a positive bandwidth
    /// and an SNR in range.
    pub fn check(&self) -> Result<()> {
        within("chain impedance (Ω)", self.impedance, 1e-6, 1e6)?;
        within("chain temperature (K)", self.temperature, 1e-6, MAX_TEMPERATURE)?;
        if let Some(b) = self.bandwidth
            && !(b.is_finite() && b > 0.0 && b <= 1e15)
        {
            return Err(invalid("receiver bandwidth", "must be a positive frequency"));
        }
        within("receiver SNR (dB)", self.snr, -MAX_POWER_DBM, MAX_POWER_DBM)
    }
}

impl Chain {
    /// Whether the chain has nothing in it: no stage and the default source and receiver.
    pub fn is_empty(&self) -> bool {
        *self == Chain::default()
    }

    /// The index of the stage named `name`.
    pub fn index(&self, name: &str) -> Result<usize> {
        self.stages.iter().position(|s| s.name == name).ok_or_else(|| ModelError::Unknown { kind: "stage", name: name.to_owned() })
    }

    /// The name the next stage of `kind` takes when none is given: `kind` and the first number
    /// no stage has.
    ///
    /// ```
    /// use emcraft_model::system::{Chain, Part, Stage};
    /// let mut c = Chain::default();
    /// assert_eq!(c.fresh_name("amplifier"), "amplifier 1");
    /// c.stages.push(Stage { name: "amplifier 1".into(), part: Part::Attenuator { loss: 1.0 } });
    /// assert_eq!(c.fresh_name("amplifier"), "amplifier 2");
    /// ```
    pub fn fresh_name(&self, kind: &str) -> String {
        (1..=MAX_STAGES + 1).map(|k| format!("{kind} {k}")).find(|n| self.stages.iter().all(|s| &s.name != n)).unwrap_or_else(|| kind.to_owned())
    }
}

impl Model {
    /// Puts `stage` at `at` (the end when past it), after checking it: a name of its own, its
    /// part's settings, room in the chain.
    ///
    /// ```
    /// use emcraft_model::Model;
    /// use emcraft_model::system::{Part, Stage};
    /// let mut m = Model::default();
    /// let lna = Stage { name: "LNA".into(), part: Part::Amplifier { gain: 20.0, noise_figure: 0.6, oip3: None, p1db: None } };
    /// m.add_stage(9, lna.clone()).unwrap();
    /// assert!(m.add_stage(0, lna).is_err(), "a name of its own");
    /// m.add_stage(0, Stage { name: "pad".into(), part: Part::Attenuator { loss: 3.0 } }).unwrap();
    /// assert_eq!(m.system.stages[0].name, "pad");
    /// ```
    pub fn add_stage(&mut self, at: usize, stage: Stage) -> Result<()> {
        named("stage name", &stage.name)?;
        if self.system.stages.iter().any(|s| s.name == stage.name) {
            return Err(ModelError::Duplicate { kind: "stage", name: stage.name });
        }
        if self.system.stages.len() >= MAX_STAGES {
            return Err(invalid("chain", format!("at most {MAX_STAGES} stages")));
        }
        stage.part.check()?;
        let at = at.min(self.system.stages.len());
        self.system.stages.insert(at, stage);
        Ok(())
    }

    /// Replaces the stage at `i` with `stage`, checked as a new one would be.
    pub fn set_stage(&mut self, i: usize, stage: Stage) -> Result<()> {
        if i >= self.system.stages.len() {
            return Err(invalid("stage", "no such stage"));
        }
        named("stage name", &stage.name)?;
        if self.system.stages.iter().enumerate().any(|(k, s)| k != i && s.name == stage.name) {
            return Err(ModelError::Duplicate { kind: "stage", name: stage.name });
        }
        stage.part.check()?;
        self.system.stages[i] = stage;
        Ok(())
    }

    /// Removes the stage at `i`.
    pub fn remove_stage(&mut self, i: usize) -> Result<Stage> {
        if i >= self.system.stages.len() {
            return Err(invalid("stage", "no such stage"));
        }
        Ok(self.system.stages.remove(i))
    }

    /// Moves the stage at `from` to `to` (the end when past it).
    pub fn move_stage(&mut self, from: usize, to: usize) -> Result<()> {
        let stage = self.remove_stage(from)?;
        let to = to.min(self.system.stages.len());
        self.system.stages.insert(to, stage);
        Ok(())
    }

    /// Sets the chain's source after checking it.
    pub fn set_system_source(&mut self, source: Source) -> Result<()> {
        source.check()?;
        self.system.source = source;
        Ok(())
    }

    /// Sets the chain's receiver after checking it.
    pub fn set_receiver(&mut self, receiver: Receiver) -> Result<()> {
        receiver.check()?;
        self.system.receiver = receiver;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amp(name: &str) -> Stage {
        Stage { name: name.into(), part: Part::Amplifier { gain: 20.0, noise_figure: 1.0, oip3: None, p1db: None } }
    }

    #[test]
    fn stages_are_added_moved_and_removed_and_hostile_ones_refused() {
        let mut m = Model::default();
        for n in ["a", "b", "c"] {
            m.add_stage(usize::MAX, amp(n)).unwrap();
        }
        m.move_stage(0, 9).unwrap();
        let names: Vec<&str> = m.system.stages.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["b", "c", "a"]);
        assert!(m.set_stage(0, amp("c")).is_err(), "a name another stage has");
        m.set_stage(0, amp("b2")).unwrap();
        assert_eq!(m.remove_stage(2).unwrap().name, "a");
        assert!(m.remove_stage(5).is_err() && m.move_stage(5, 0).is_err());
        let bad = [
            Part::Amplifier { gain: f64::NAN, noise_figure: 1.0, oip3: None, p1db: None },
            Part::Amplifier { gain: 10.0, noise_figure: -1.0, oip3: None, p1db: None },
            Part::Amplifier { gain: 10.0, noise_figure: 1.0, oip3: Some(1e9), p1db: None },
            Part::Attenuator { loss: 1e9 },
            Part::Data { data: DataSource::Measurement { name: " ".into() }, flip: false },
            Part::Line { impedance: 0.0, length: 0.1, eps_r: 1.0, attenuation: 0.0, reference: None, tan_delta: 0.0 },
            Part::Line { impedance: 50.0, length: 1e9, eps_r: 1.0, attenuation: 0.0, reference: None, tan_delta: 0.0 },
            Part::Line { impedance: 50.0, length: 0.1, eps_r: 1.0, attenuation: 0.0, reference: Some(-1.0), tan_delta: 0.0 },
        ];
        for part in bad {
            assert!(m.add_stage(0, Stage { name: "x".into(), part: part.clone() }).is_err(), "{part:?}");
        }
        assert!(m.add_stage(0, Stage { name: "".into(), part: Part::Attenuator { loss: 1.0 } }).is_err());
        while m.system.stages.len() < MAX_STAGES {
            let n = m.system.fresh_name("pad");
            m.add_stage(0, Stage { name: n, part: Part::Attenuator { loss: 1.0 } }).unwrap();
        }
        assert!(m.add_stage(0, amp("one too many")).is_err());
    }

    #[test]
    fn the_source_and_receiver_refuse_hostile_settings_and_read_back_as_written() {
        let mut m = Model::default();
        assert!(m.set_system_source(Source { temperature: -1.0, ..Source::default() }).is_err());
        assert!(m.set_receiver(Receiver { bandwidth: Some(0.0), ..Receiver::default() }).is_err());
        assert!(m.set_receiver(Receiver { impedance: f64::INFINITY, ..Receiver::default() }).is_err());
        m.set_system_source(Source { antenna: Some("ant".into()), temperature: 150.0, power: Some(-90.0) }).unwrap();
        m.set_receiver(Receiver { bandwidth: Some(2e6), snr: 10.0, ..Receiver::default() }).unwrap();
        m.add_stage(0, amp("LNA")).unwrap();
        let text = serde_json::to_string(&m.system).unwrap();
        let back: Chain = serde_json::from_str(&text).unwrap();
        assert_eq!(back, m.system);
        assert!(Chain::default().is_empty() && !m.system.is_empty());
    }
}

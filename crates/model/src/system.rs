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

/// The source's node in links (no stage takes a name beginning with `@`).
pub const SOURCE: &str = "@source";

/// The receiver's node in links.
pub const RECEIVER: &str = "@receiver";

/// The chain: its stages (nodes on a canvas), the links between their ports, the source and
/// the receiver's needs. The chain read is the path the links make from the source: each node
/// feeds at most one and is fed by at most one (a series chain); stages off it are kept, not
/// read. Without links (a chain written before them) the stages are read in their order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Chain {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<Stage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
    #[serde(default)]
    pub source: Source,
    #[serde(default)]
    pub receiver: Receiver,
}

/// A stage: its name (unique in the chain), what it is and where it sits on the canvas (none:
/// laid out by its place in the chain).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    pub name: String,
    pub part: Part,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 2]>,
}

/// A link from a node's output to another's input, by name ([`SOURCE`], [`RECEIVER`] or a
/// stage's).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub from: String,
    pub to: String,
}

/// The farthest a node sits from the canvas' origin.
pub const MAX_CANVAS: f64 = 1e6;

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

    /// The stages the chain reads, in order from the source (indices into `stages`), and
    /// whether the path reaches the receiver: the links followed from [`SOURCE`] (each node's
    /// one output); without links, every stage in order.
    ///
    /// ```
    /// use emcraft_model::system::{Chain, Link, Part, Stage, RECEIVER, SOURCE};
    /// let pad = |n: &str| Stage { name: n.into(), part: Part::Attenuator { loss: 1.0 }, at: None };
    /// let mut c = Chain { stages: vec![pad("a"), pad("b"), pad("c")], ..Chain::default() };
    /// assert_eq!(c.order(), (vec![0, 1, 2], true));
    /// let link = |f: &str, t: &str| Link { from: f.into(), to: t.into() };
    /// c.links = vec![link(SOURCE, "c"), link("c", "a"), link("a", RECEIVER)];
    /// assert_eq!(c.order(), (vec![2, 0], true), "b is off the chain");
    /// ```
    pub fn order(&self) -> (Vec<usize>, bool) {
        if self.links.is_empty() {
            return ((0..self.stages.len()).collect(), true);
        }
        let mut out = Vec::new();
        let mut at = SOURCE;
        // Each stage once at most: a loop ends the path.
        for _ in 0..=self.stages.len() {
            let Some(next) = self.links.iter().find(|l| l.from == at).map(|l| l.to.as_str()) else { return (out, false) };
            if next == RECEIVER {
                return (out, true);
            }
            match self.index(next) {
                Ok(i) if !out.contains(&i) => out.push(i),
                _ => return (out, false),
            }
            at = &self.stages[out[out.len() - 1]].name;
        }
        (out, false)
    }

    /// The links of the series chain through `order` (indices into `stages`): the source, each
    /// stage, the receiver.
    fn series(&self, order: &[usize]) -> Vec<Link> {
        let names: Vec<&str> =
            std::iter::once(SOURCE).chain(order.iter().map(|&i| self.stages[i].name.as_str())).chain(std::iter::once(RECEIVER)).collect();
        names.windows(2).map(|w| Link { from: w[0].to_owned(), to: w[1].to_owned() }).collect()
    }

    /// Whether `name` is a node of the chain: the source, the receiver or a stage.
    fn has_node(&self, name: &str) -> bool {
        name == SOURCE || name == RECEIVER || self.stages.iter().any(|s| s.name == name)
    }

    /// The name the next stage of `kind` takes when none is given: `kind` and the first number
    /// no stage has.
    ///
    /// ```
    /// use emcraft_model::system::{Chain, Part, Stage};
    /// let mut c = Chain::default();
    /// assert_eq!(c.fresh_name("amplifier"), "amplifier 1");
    /// c.stages.push(Stage { name: "amplifier 1".into(), part: Part::Attenuator { loss: 1.0 }, at: None });
    /// assert_eq!(c.fresh_name("amplifier"), "amplifier 2");
    /// ```
    pub fn fresh_name(&self, kind: &str) -> String {
        (1..=MAX_STAGES + 1).map(|k| format!("{kind} {k}")).find(|n| self.stages.iter().all(|s| &s.name != n)).unwrap_or_else(|| kind.to_owned())
    }
}

/// A stage's own checks: a name of its own kind (not empty, not the source's or receiver's: no
/// leading `@`), its part's settings, its place on the canvas.
fn check_stage(stage: &Stage) -> Result<()> {
    named("stage name", &stage.name)?;
    if stage.name.starts_with('@') {
        return Err(invalid("stage name", "must not begin with `@` (the source's and receiver's)"));
    }
    if let Some(at) = stage.at
        && !at.iter().all(|x| x.is_finite() && x.abs() <= MAX_CANVAS)
    {
        return Err(invalid("stage's place", format!("finite, within {MAX_CANVAS} of the origin")));
    }
    stage.part.check()
}

impl Model {
    /// Puts `stage` at place `at` along the chain (the end when past it), after checking it: a name of its own, its
    /// part's settings, room in the chain.
    ///
    /// ```
    /// use emcraft_model::Model;
    /// use emcraft_model::system::{Part, Stage};
    /// let mut m = Model::default();
    /// let lna = Stage { name: "LNA".into(), part: Part::Amplifier { gain: 20.0, noise_figure: 0.6, oip3: None, p1db: None }, at: None };
    /// m.add_stage(9, lna.clone()).unwrap();
    /// assert!(m.add_stage(0, lna).is_err(), "a name of its own");
    /// m.add_stage(0, Stage { name: "pad".into(), part: Part::Attenuator { loss: 3.0 }, at: None }).unwrap();
    /// assert_eq!(m.system.order().0, [1, 0], "the pad first along the chain");
    /// ```
    pub fn add_stage(&mut self, at: usize, stage: Stage) -> Result<()> {
        check_stage(&stage)?;
        if self.system.stages.iter().any(|s| s.name == stage.name) {
            return Err(ModelError::Duplicate { kind: "stage", name: stage.name });
        }
        if self.system.stages.len() >= MAX_STAGES {
            return Err(invalid("chain", format!("at most {MAX_STAGES} stages")));
        }
        self.materialize();
        // Linked in before it joins the stages: an empty chain has no links to read it by.
        self.insert(&stage.name, at);
        self.system.stages.push(stage);
        Ok(())
    }

    /// Puts a stage on the canvas without linking it (the links made by hand).
    pub fn place_stage(&mut self, stage: Stage) -> Result<()> {
        check_stage(&stage)?;
        if self.system.stages.iter().any(|s| s.name == stage.name) {
            return Err(ModelError::Duplicate { kind: "stage", name: stage.name });
        }
        if self.system.stages.len() >= MAX_STAGES {
            return Err(invalid("chain", format!("at most {MAX_STAGES} stages")));
        }
        // A chain read in order (no links yet) keeps that order before one is left off it.
        self.materialize();
        self.system.stages.push(stage);
        Ok(())
    }

    /// A chain read in its stages' order (no links: written before them) given the links of
    /// that order, so a change to one link keeps the rest.
    fn materialize(&mut self) {
        if self.system.links.is_empty() && !self.system.stages.is_empty() {
            let order: Vec<usize> = (0..self.system.stages.len()).collect();
            self.system.links = self.system.series(&order);
        }
    }

    /// The stage `name` taken off the chain, what fed it now feeding what it fed.
    fn detach(&mut self, name: &str) {
        let fed = self.system.links.iter().find(|l| l.to == name).map(|l| l.from.clone());
        let feeds = self.system.links.iter().find(|l| l.from == name).map(|l| l.to.clone());
        self.system.links.retain(|l| l.from != name && l.to != name);
        if let (Some(a), Some(b)) = (fed, feeds) {
            self.system.links.push(Link { from: a, to: b });
        }
    }

    /// The stage `name` (off the chain) put at place `at` along the chain from the source (its
    /// end when past it): between the node there and the one before it.
    fn insert(&mut self, name: &str, at: usize) {
        let (order, complete) = self.system.order();
        let names: Vec<String> = order.iter().map(|&i| self.system.stages[i].name.clone()).collect();
        let k = at.min(names.len());
        let before = if k == 0 { SOURCE.to_owned() } else { names[k - 1].clone() };
        let after = match names.get(k) {
            Some(n) => Some(n.clone()),
            None if complete || self.system.links.is_empty() => Some(RECEIVER.to_owned()),
            None => None,
        };
        self.system.links.retain(|l| l.from != before);
        self.system.links.push(Link { from: before, to: name.to_owned() });
        if let Some(after) = after {
            self.system.links.push(Link { from: name.to_owned(), to: after });
        }
    }

    /// Links `from`'s output to `to`'s input: each output feeds one input and each input is fed
    /// by one output, so the links `from` fed and `to` was fed by go.
    ///
    /// ```
    /// use emcraft_model::Model;
    /// use emcraft_model::system::{Part, Stage, RECEIVER, SOURCE};
    /// let mut m = Model::default();
    /// for n in ["a", "b"] {
    ///     m.add_stage(9, Stage { name: n.into(), part: Part::Attenuator { loss: 1.0 }, at: None }).unwrap();
    /// }
    /// m.link(SOURCE, "b").unwrap();
    /// m.link("b", "a").unwrap();
    /// m.link("a", RECEIVER).unwrap();
    /// assert_eq!(m.system.order(), (vec![1, 0], true));
    /// assert!(m.link("a", "a").is_err() && m.link(RECEIVER, "a").is_err() && m.link("a", "nope").is_err());
    /// ```
    pub fn link(&mut self, from: &str, to: &str) -> Result<()> {
        if !(self.system.has_node(from) && self.system.has_node(to)) {
            return Err(invalid("link", "between the chain's nodes: the source, its stages, the receiver"));
        }
        if from == to || from == RECEIVER || to == SOURCE {
            return Err(invalid("link", "from a node's output to another node's input (the receiver has no output, the source no input)"));
        }
        self.materialize();
        self.system.links.retain(|l| l.from != from && l.to != to);
        self.system.links.push(Link { from: from.to_owned(), to: to.to_owned() });
        Ok(())
    }

    /// Takes the link from `from` to `to` away.
    pub fn unlink(&mut self, from: &str, to: &str) -> Result<()> {
        self.materialize();
        let before = self.system.links.len();
        self.system.links.retain(|l| !(l.from == from && l.to == to));
        if self.system.links.len() == before {
            return Err(invalid("link", "no such link"));
        }
        Ok(())
    }

    /// Replaces the stage at `i` with `stage`, checked as a new one would be.
    pub fn set_stage(&mut self, i: usize, stage: Stage) -> Result<()> {
        if i >= self.system.stages.len() {
            return Err(invalid("stage", "no such stage"));
        }
        check_stage(&stage)?;
        if self.system.stages.iter().enumerate().any(|(k, s)| k != i && s.name == stage.name) {
            return Err(ModelError::Duplicate { kind: "stage", name: stage.name });
        }
        self.materialize();
        let old = std::mem::replace(&mut self.system.stages[i], stage);
        let new = self.system.stages[i].name.clone();
        for l in &mut self.system.links {
            for end in [&mut l.from, &mut l.to] {
                if *end == old.name {
                    end.clone_from(&new);
                }
            }
        }
        Ok(())
    }

    /// Removes the stage at `i`: the chain closes over it (what fed it now feeds what it fed).
    pub fn remove_stage(&mut self, i: usize) -> Result<Stage> {
        if i >= self.system.stages.len() {
            return Err(invalid("stage", "no such stage"));
        }
        self.materialize();
        let name = self.system.stages[i].name.clone();
        self.detach(&name);
        Ok(self.system.stages.remove(i))
    }

    /// Moves the stage at `from` to place `to` along the chain (the end when past it), its
    /// canvas position kept.
    pub fn move_stage(&mut self, from: usize, to: usize) -> Result<()> {
        if from >= self.system.stages.len() {
            return Err(invalid("stage", "no such stage"));
        }
        self.materialize();
        let name = self.system.stages[from].name.clone();
        self.detach(&name);
        self.insert(&name, to);
        Ok(())
    }

    /// Puts the stage at `i` at `at` on the canvas.
    pub fn place(&mut self, i: usize, at: [f64; 2]) -> Result<()> {
        if !at.iter().all(|x| x.is_finite() && x.abs() <= MAX_CANVAS) {
            return Err(invalid("stage's place", format!("finite, within {MAX_CANVAS} of the origin")));
        }
        let s = self.system.stages.get_mut(i).ok_or_else(|| invalid("stage", "no such stage"))?;
        s.at = Some(at);
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
        Stage { name: name.into(), part: Part::Amplifier { gain: 20.0, noise_figure: 1.0, oip3: None, p1db: None }, at: None }
    }

    /// The names along the chain, from the source.
    fn chain(m: &Model) -> Vec<&str> {
        m.system.order().0.into_iter().map(|i| m.system.stages[i].name.as_str()).collect()
    }

    #[test]
    fn stages_are_added_moved_and_removed_along_the_chain_and_hostile_ones_refused() {
        let mut m = Model::default();
        for n in ["a", "b", "c"] {
            m.add_stage(usize::MAX, amp(n)).unwrap();
        }
        m.move_stage(0, 9).unwrap();
        assert_eq!(chain(&m), ["b", "c", "a"]);
        assert!(m.set_stage(1, amp("c")).is_err(), "a name another stage has");
        // A new name follows into the links.
        m.set_stage(1, amp("b2")).unwrap();
        assert_eq!(chain(&m), ["b2", "c", "a"]);
        // Removed: the chain closes over it.
        assert_eq!(m.remove_stage(2).unwrap().name, "c");
        assert_eq!(chain(&m), ["b2", "a"]);
        assert!(m.remove_stage(5).is_err() && m.move_stage(5, 0).is_err());
        // A stage placed off the chain is kept, not read, until linked.
        m.place_stage(Stage { at: Some([10.0, 20.0]), ..amp("spare") }).unwrap();
        assert_eq!(chain(&m), ["b2", "a"]);
        m.link("b2", "spare").unwrap();
        m.link("spare", "a").unwrap();
        assert_eq!(chain(&m), ["b2", "spare", "a"]);
        // Unlinked from the receiver: the path stops short of it.
        m.unlink("a", RECEIVER).unwrap();
        assert_eq!(m.system.order(), (vec![1, 2, 0], false));
        assert!(m.unlink("a", RECEIVER).is_err(), "no such link");
        assert!(m.place(0, [f64::NAN, 0.0]).is_err() && m.place(9, [0.0, 0.0]).is_err());
        m.place(0, [5.0, 6.0]).unwrap();
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
            assert!(m.add_stage(0, Stage { name: "x".into(), part: part.clone(), at: None }).is_err(), "{part:?}");
        }
        for name in ["", "@source", "@x"] {
            assert!(m.add_stage(0, Stage { name: name.into(), part: Part::Attenuator { loss: 1.0 }, at: None }).is_err(), "{name}");
        }
        assert!(m.add_stage(0, Stage { at: Some([1e9, 0.0]), ..amp("far") }).is_err());
        while m.system.stages.len() < MAX_STAGES {
            let n = m.system.fresh_name("pad");
            m.add_stage(0, Stage { name: n, part: Part::Attenuator { loss: 1.0 }, at: None }).unwrap();
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

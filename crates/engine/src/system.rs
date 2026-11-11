//! A system's chain ([`emcraft_model::system`]): its stages edited by `system.stage.*`, its
//! source and receiver set by `system.source` and `system.receiver`, and `solve.system` reading
//! it over a band. The chain's S comes from its stages' chain (ABCD) matrices (Pozar, Microwave
//! Engineering, 4th ed., §4.4, Tables 4.1–4.2), its noise from Hillbrand and Russer's chain
//! correlation matrices (IEEE Trans. CAS-23(4), 1976), and its budget stage by stage from the
//! source on: the available gain (Pozar §12.1), the noise figure and temperature, the signal,
//! noise and signal-to-noise ratio in the receiver's bandwidth, and the cascade's third-order
//! intercept and 1 dB compression points (Pozar §10.3: input-referred, 1/IIP3 = Σ G_{<k}/IIP3_k).
//! `circuit.cascade` chains the same blocks.

use std::sync::Arc;

use emcraft_model::system::{DataSource, MAX_STAGES, Part, Receiver, Source, Stage};
use emcraft_model::{Arrangement, LumpedElement, Point3};
use emcraft_solver::{Complex64 as C, SMatrix};
use serde_json::{Value, json};

use crate::network::{NoisePoint, SourceImpedance, interpolate, noise_at};
use crate::noise::{Budget, ChainNoise, NoiseParameters, T0, TwoPort};
use crate::sdata::SData;
use crate::{EngineError, Params, Result, Session};

/// The speed of light in vacuum (m/s).
const C0: f64 = 299_792_458.0;

/// Np per dB.
pub(crate) const NP_PER_DB: f64 = std::f64::consts::LN_10 / 20.0;

/// The most frequencies a chain is read at.
pub(crate) const MAX_POINTS: usize = 4001;

const ONE: C = C::new(1.0, 0.0);
const ZERO: C = C::new(0.0, 0.0);

/// A stage made ready to evaluate: a data stage's S read, gains as ratios.
pub(crate) enum Block {
    /// S over frequency on its ports' reference impedances; `flip` swaps its ports; its noise
    /// where known (else a passive network's at the chain's temperature).
    Data { data: SData, z0: [f64; 2], flip: bool, name: String, noise: Vec<NoisePoint> },
    /// A lumped element in series along the line, or shunt across it.
    Element { element: LumpedElement, shunt: bool },
    /// A uniform line: its impedance (Ω), length (m), relative permittivity and losses.
    Line { z0: f64, length: f64, eps_r: f64, loss: LineLoss },
    /// An ideal amplifier, matched and unilateral on `z0` (Ω): S21 = `gain` (a voltage ratio),
    /// its noise figure `figure` (a power ratio) from a matched source.
    Amplifier { gain: f64, figure: f64, z0: f64 },
    /// A matched attenuator on `z0` (Ω): S21 = S12 = `through` (a voltage ratio).
    Attenuator { through: f64, z0: f64 },
}

/// A line's losses: a conductor's attenuation (Np/m) at `reference` (Hz), growing as √f from it
/// (the skin effect; constant without a reference), and its dielectric's loss tangent, whose
/// attenuation is π f √εr tan δ / c (a uniformly filled line: εr the line's effective one).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LineLoss {
    conductor: f64,
    reference: Option<f64>,
    tan_delta: f64,
}

impl LineLoss {
    /// The attenuation (Np/m) at `f` (Hz) on a line of relative permittivity `eps_r`.
    fn alpha(&self, f: f64, eps_r: f64) -> f64 {
        let skin = self.reference.map_or(1.0, |r| (f / r).sqrt());
        self.conductor * skin + std::f64::consts::PI * f * eps_r.sqrt() * self.tan_delta / C0
    }
}

/// A lumped element from `resistance` (Ω), `inductance` (nH), `capacitance` (pF) and
/// `arrangement`, checked as `model.element.create` checks its own.
fn element(p: &Params) -> Result<LumpedElement> {
    // Where it sits does not matter in a circuit: a gap of a millimetre, its ends apart.
    let e = crate::elements::lumped(p, "block".into(), Point3::ORIGIN, Point3::new(0.0, 0.0, 1e-3), None)?;
    e.check().map_err(|err| p.error("resistance", err.to_string()))?;
    Ok(e)
}

/// The kinds of stage `system.stage.add` and `circuit.cascade` take.
pub const KINDS: [&str; 6] = ["amplifier", "attenuator", "data", "line", "series", "shunt"];

/// The part `p` describes (`type` and its settings, in display units), checked.
pub(crate) fn part(s: &Session, p: &Params) -> Result<Part> {
    let kind = p.string("type").required()?;
    let part = match kind {
        "amplifier" => Part::Amplifier {
            gain: p.number("gain").required()?,
            noise_figure: p.number("noise_figure").or(0.0)?,
            oip3: p.number("oip3").optional()?,
            p1db: p.number("p1db").optional()?,
        },
        "attenuator" => Part::Attenuator { loss: p.number("loss").required()? },
        "data" => Part::Data {
            data: match p.string("data").optional()? {
                None | Some("sweep") => DataSource::Sweep,
                Some(name) => DataSource::Measurement { name: crate::text::clip(name, 128) },
            },
            flip: p.flag("flip").or(false)?,
        },
        "line" => {
            let reference = p.frequency("at").optional()?;
            if reference.is_some_and(|r| !(r > 0.0)) {
                return Err(p.error("at", "a positive frequency"));
            }
            let attenuation = p.number("attenuation").or(0.0)?;
            if !(attenuation.is_finite() && attenuation >= 0.0) {
                return Err(p.error("attenuation", "a finite number ≥ 0 (dB per length unit)"));
            }
            Part::Line {
                impedance: p.positive("impedance").or(50.0)?,
                length: p.positive_length("length").required()?,
                eps_r: p.positive("eps_r").or(1.0)?,
                // dB per display length to Np/m: the display lengths in a metre.
                attenuation: attenuation * NP_PER_DB * s.show_length(1.0),
                reference,
                tan_delta: p.number("tan_delta").or(0.0)?,
            }
        }
        "series" | "shunt" => Part::Element { element: element(p)?, shunt: kind == "shunt" },
        other => {
            return Err(p.error("type", crate::text::unknown(format_args!("`{}` is not a stage", crate::text::clip(other, 20)), "kinds: ", KINDS)));
        }
    };
    part.check().map_err(|e| p.error("type", e.to_string()))?;
    Ok(part)
}

/// `part`'s settings as [`part`] takes them, in display units.
pub fn stage_params(s: &Session, part: &Part) -> Value {
    let mut v = match part {
        Part::Amplifier { gain, noise_figure, oip3, p1db } => {
            let mut v = json!({"gain": gain, "noise_figure": noise_figure});
            if let Some(x) = oip3 {
                v["oip3"] = json!(x);
            }
            if let Some(x) = p1db {
                v["p1db"] = json!(x);
            }
            v
        }
        Part::Attenuator { loss } => json!({"loss": loss}),
        Part::Data { data, flip } => {
            json!({"data": match data { DataSource::Sweep => "sweep", DataSource::Measurement { name } => name }, "flip": flip})
        }
        Part::Line { impedance, length, eps_r, attenuation, reference, tan_delta } => {
            let mut v = json!({
                "impedance": impedance,
                "length": s.show_length(*length),
                "eps_r": eps_r,
                "attenuation": attenuation / NP_PER_DB / s.show_length(1.0),
                "tan_delta": tan_delta,
            });
            if let Some(r) = reference {
                v["at"] = json!(s.show_frequency(*r));
            }
            v
        }
        Part::Element { element, .. } => json!({
            "resistance": element.resistance,
            "inductance": element.inductance * 1e9,
            "capacitance": element.capacitance * 1e12,
            "arrangement": match element.arrangement { Arrangement::Series => "series", Arrangement::Parallel => "parallel" },
        }),
    };
    v["type"] = json!(part.kind());
    v
}

impl Block {
    /// `part` made ready to evaluate on a chain of impedance `z0` (Ω): a data stage's S read
    /// from the sweep or the measurement it names.
    pub(crate) fn of(s: &Session, part: &Part, z0: f64) -> Result<Block> {
        Ok(match part {
            Part::Amplifier { gain, noise_figure, .. } => {
                Block::Amplifier { gain: 10f64.powf(gain / 20.0), figure: 10f64.powf(noise_figure / 10.0), z0 }
            }
            Part::Attenuator { loss } => Block::Attenuator { through: 10f64.powf(-loss / 20.0), z0 },
            Part::Data { data, flip } => {
                let (src, name) = match data {
                    DataSource::Sweep => (crate::network::named_source(s, None)?, "sweep".to_owned()),
                    DataSource::Measurement { name } => (crate::network::named_source(s, Some(name))?, name.clone()),
                };
                if src.data.outputs() != 2 || src.data.inputs().len() != 2 {
                    return Err(EngineError::failed(format!(
                        "`{}`: a two-port with both ports excited; this has {} ports",
                        crate::text::clip(&name, 40),
                        src.data.outputs()
                    )));
                }
                let z = |k: usize| src.references[k].unwrap_or(z0);
                Block::Data { z0: [z(0), z(1)], flip: *flip, data: src.data, name, noise: src.noise }
            }
            Part::Line { impedance, length, eps_r, attenuation, reference, tan_delta } => Block::Line {
                z0: *impedance,
                length: *length,
                eps_r: *eps_r,
                loss: LineLoss { conductor: *attenuation, reference: *reference, tan_delta: *tan_delta },
            },
            Part::Element { element, shunt } => Block::Element { element: element.clone(), shunt: *shunt },
        })
    }

    /// The frequencies a data block holds.
    pub(crate) fn frequencies(&self) -> Option<&[f64]> {
        match self {
            Block::Data { data, .. } => Some(data.frequencies()),
            _ => None,
        }
    }

    /// Its S at `f` (Hz) on its ports' references.
    fn two_port(&self, f: f64) -> Result<(TwoPort, [f64; 2])> {
        match self {
            Block::Amplifier { gain, z0, .. } => Ok((TwoPort { s11: ZERO, s12: ZERO, s21: C::new(*gain, 0.0), s22: ZERO }, [*z0, *z0])),
            Block::Attenuator { through, z0 } => {
                let t = C::new(*through, 0.0);
                Ok((TwoPort { s11: ZERO, s12: t, s21: t, s22: ZERO }, [*z0, *z0]))
            }
            Block::Data { data, z0, flip, name, .. } => data_two_port(data, *z0, *flip, name, f),
            Block::Element { .. } | Block::Line { .. } => Err(EngineError::failed("a block without S of its own")),
        }
    }

    /// The chain matrix [A, B, C, D] at `f` (Hz).
    pub(crate) fn chain(&self, f: f64) -> Result<[C; 4]> {
        match self {
            Block::Element { element, shunt } => {
                let y = element.admittance(f);
                if *shunt {
                    Ok([ONE, ZERO, y, ONE])
                } else if y.norm() == 0.0 {
                    Err(EngineError::failed("a series element open at this frequency: nothing goes through"))
                } else {
                    Ok([ONE, ONE / y, ZERO, ONE])
                }
            }
            Block::Line { z0, length, eps_r, loss } => {
                // γl = (α + jβ)l: A = D = cosh γl, B = Z₀ sinh γl, C = sinh γl / Z₀ (Pozar Table 4.1).
                let beta = 2.0 * std::f64::consts::PI * f * eps_r.sqrt() / C0;
                let gl = C::new(loss.alpha(f, *eps_r), beta) * length;
                let (ch, sh) = (gl.cosh(), gl.sinh());
                Ok([ch, sh * z0, sh / z0, ch])
            }
            Block::Data { .. } | Block::Amplifier { .. } | Block::Attenuator { .. } => {
                let (t, z) = self.two_port(f)?;
                let m = [t.s11, t.s12, t.s21, t.s22];
                let name = match self {
                    Block::Data { name, .. } => crate::text::clip(name, 40),
                    _ => "the stage".to_owned(),
                };
                SMatrix::from_fn(2, |i, j| m[2 * i + j])
                    .chain(z)
                    .ok_or_else(|| EngineError::failed(format!("`{name}` passes nothing (S21 = 0) at {:.6} GHz", f / 1e9)))
            }
        }
    }

    /// Its noise at `f` (Hz), its chain matrix `a`, every passive part at `temperature` (K): a
    /// lumped element's resistance's thermal noise, none for a lossless line, a data block's
    /// noise data, or without it a passive network's (Twiss); why there is none otherwise.
    pub(crate) fn noise(&self, a: [C; 4], f: f64, temperature: f64) -> std::result::Result<ChainNoise, String> {
        match self {
            Block::Element { element, shunt: true } => Ok(ChainNoise::shunt(element.admittance(f), temperature)),
            Block::Element { element, shunt: false } => Ok(ChainNoise::series(ONE / element.admittance(f), temperature)),
            Block::Line { loss, .. } if *loss == LineLoss::default() => Ok(ChainNoise::NONE),
            Block::Line { z0, .. } => {
                let s = s_of_chain(a, *z0).ok_or("a line with no S")?;
                let s = TwoPort { s11: s[0], s12: s[1], s21: s[2], s22: s[3] };
                ChainNoise::passive(&s, [*z0, *z0], a, temperature).ok_or_else(|| "a lossy line whose noise is not fixed by its S".to_string())
            }
            // Its own noise an uncorrelated wave kT₀(F − 1) entering its input: F_min = F at
            // Γ_opt = 0, and F = F_min + (F_min − 1)|Γs|²/(1 − |Γs|²) from another source, which
            // is R_n = z₀(F − 1)/4 (Hillbrand and Russer's F(Γs) with Γ_opt = 0).
            Block::Amplifier { figure, z0, .. } => {
                Ok(ChainNoise::of(NoiseParameters { f_min: *figure, y_opt: C::new(1.0 / z0, 0.0), rn: z0 * (figure - 1.0) / 4.0 }))
            }
            Block::Attenuator { .. } => {
                let (s, z) = self.two_port(f).map_err(|e| e.to_string())?;
                ChainNoise::passive(&s, z, a, temperature).ok_or_else(|| "an attenuator whose noise is not fixed by its S".to_string())
            }
            Block::Data { data, z0, flip, name, noise } => {
                let name = crate::text::clip(name, 40);
                if !noise.is_empty() {
                    if *flip {
                        return Err(format!("`{name}` is flipped: its noise data is its port 1's"));
                    }
                    return noise_at(noise, f).ok_or_else(|| format!("`{name}` has no noise data at {:.6} GHz", f / 1e9));
                }
                let (s, z) = data_two_port(data, *z0, *flip, &name, f).map_err(|e| e.to_string())?;
                ChainNoise::passive(&s, z, a, temperature)
                    .ok_or_else(|| format!("`{name}` gains and has no noise data (results.import a file with them)"))
            }
        }
    }
}

/// A data block's S at `f` (Hz) as the chain sees it (its ports swapped when flipped) and its
/// ports' references, or why it has none.
fn data_two_port(data: &SData, z0: [f64; 2], flip: bool, name: &str, f: f64) -> Result<(TwoPort, [f64; 2])> {
    let at = |p: usize, q: usize| interpolate(data, p, q, f);
    let [Some(s11), Some(s12), Some(s21), Some(s22)] = [at(0, 0), at(0, 1), at(1, 0), at(1, 1)] else {
        return Err(EngineError::failed(format!("`{}` has no S at {:.6} GHz: outside its band", crate::text::clip(name, 40), f / 1e9)));
    };
    Ok(if flip { (TwoPort { s11: s22, s12: s21, s21: s12, s22: s11 }, [z0[1], z0[0]]) } else { (TwoPort { s11, s12, s21, s22 }, z0) })
}

/// S on `z0` at both ports from a chain matrix (Pozar Table 4.2).
pub(crate) fn s_of_chain([a, b, c, d]: [C; 4], z0: f64) -> Option<[C; 4]> {
    let delta = a + b / z0 + c * z0 + d;
    if delta.norm() == 0.0 {
        return None;
    }
    let det = a * d - b * c;
    Some([(a + b / z0 - c * z0 - d) / delta, 2.0 * det / delta, 2.0 / delta, (-a + b / z0 - c * z0 + d) / delta])
}

/// 2 × 2 chain matrices multiplied.
fn times(x: [C; 4], y: [C; 4]) -> [C; 4] {
    [x[0] * y[0] + x[1] * y[2], x[0] * y[1] + x[1] * y[3], x[2] * y[0] + x[3] * y[2], x[2] * y[1] + x[3] * y[3]]
}

/// The chain so far after a block: its chain matrix and its noise (or why it has none).
pub(crate) struct Partial {
    pub chain: [C; 4],
    pub noise: std::result::Result<ChainNoise, String>,
}

/// `blocks` chained at `f` (Hz), the passive ones at `temperature` (K): the chain so far after
/// each block.
pub(crate) fn walk(blocks: &[Block], f: f64, temperature: f64) -> Result<Vec<Partial>> {
    let mut m = [ONE, ZERO, ZERO, ONE];
    let mut n: std::result::Result<ChainNoise, String> = Ok(ChainNoise::NONE);
    let mut out = Vec::with_capacity(blocks.len());
    for b in blocks {
        let a = b.chain(f)?;
        n = n.and_then(|n| Ok(n.then(m, b.noise(a, f, temperature)?)));
        m = times(m, a);
        out.push(Partial { chain: m, noise: n.clone() });
    }
    Ok(out)
}

/// The chain's S on `z0` from its chain matrix, as a [`TwoPort`].
pub(crate) fn two_port_of(chain: [C; 4], z0: f64) -> Option<TwoPort> {
    s_of_chain(chain, z0).map(|s| TwoPort { s11: s[0], s12: s[1], s21: s[2], s22: s[3] })
}

/// A stage's row of the budget at one frequency: the stage's own available gain and the
/// chain's from the source to its output (dB), the chain's noise figure (dB) and noise
/// temperature (K) there, the signal and noise at its output and their ratio (dBm, dB; with a
/// source power and a bandwidth), and the chain's input- and output-referred third-order
/// intercept and 1 dB compression points (dBm; where some stage so far has them).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Row {
    pub stage_gain_db: Option<f64>,
    pub gain_db: Option<f64>,
    pub nf_db: Option<f64>,
    pub noise_temperature: Option<f64>,
    pub signal_dbm: Option<f64>,
    pub noise_dbm: Option<f64>,
    pub snr_db: Option<f64>,
    pub iip3_dbm: Option<f64>,
    pub oip3_dbm: Option<f64>,
    pub ip1db_dbm: Option<f64>,
    pub op1db_dbm: Option<f64>,
}

/// The chain at one frequency: its S on the chain's impedance, its noise figure (dB) and
/// available gain (dB) from the source, the receiver's system temperature (K), noise (dBm) and
/// sensitivity (dBm), each stage's row, and why something is missing.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Point {
    pub frequency: f64,
    pub s: [[f64; 2]; 4],
    pub rows: Vec<Row>,
    pub system_temperature: Option<f64>,
    pub sensitivity_dbm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// `solve.system`'s result: the chain read at each frequency, the stages' names and kinds in
/// order, and the frequency (Hz) the budget is shown at.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SystemResult {
    pub stages: Vec<(String, String)>,
    pub points: Vec<Point>,
    pub at: f64,
}

impl SystemResult {
    /// Checks a result read from a file: at most [`MAX_STAGES`] stages and [`MAX_POINTS`]
    /// frequencies, positive frequencies, a row a stage at each, every number finite.
    pub(crate) fn check(&self) -> std::result::Result<(), String> {
        if self.stages.len() > MAX_STAGES || self.points.len() > MAX_POINTS {
            return Err(format!("at most {MAX_STAGES} stages and {MAX_POINTS} frequencies"));
        }
        if self.stages.iter().any(|(n, k)| n.chars().count() > 64 || !KINDS.contains(&k.as_str())) {
            return Err("a stage's name is at most 64 characters and its kind one of the stages'".into());
        }
        let positive = |f: f64| f.is_finite() && f > 0.0;
        let finite = |x: &Option<f64>| x.is_none_or(f64::is_finite);
        for p in &self.points {
            let rows_ok = p.rows.len() == self.stages.len()
                && p.rows.iter().all(|r| {
                    [
                        r.stage_gain_db,
                        r.gain_db,
                        r.nf_db,
                        r.noise_temperature,
                        r.signal_dbm,
                        r.noise_dbm,
                        r.snr_db,
                        r.iip3_dbm,
                        r.oip3_dbm,
                        r.ip1db_dbm,
                        r.op1db_dbm,
                    ]
                    .iter()
                    .all(finite)
                });
            let s_ok = p.s.iter().flatten().all(|x| x.is_finite());
            if !(positive(p.frequency) && rows_ok && s_ok && finite(&p.system_temperature) && finite(&p.sensitivity_dbm)) {
                return Err("a frequency positive, a row a stage, every number finite".into());
            }
            if p.why.as_ref().is_some_and(|w| w.chars().count() > 512) {
                return Err("a reason is at most 512 characters".into());
            }
        }
        if positive(self.at) { Ok(()) } else { Err("the budget's frequency must be positive".into()) }
    }

    /// The point nearest `f` (Hz).
    pub fn nearest(&self, f: f64) -> Option<&Point> {
        self.points.iter().min_by(|a, b| (a.frequency - f).abs().total_cmp(&(b.frequency - f).abs()))
    }
}

/// dB of a power ratio.
fn db(x: f64) -> f64 {
    10.0 * x.log10()
}

/// mW of dBm, and back.
fn mw(dbm: f64) -> f64 {
    10f64.powf(dbm / 10.0)
}

/// The chain's linearity so far, input-referred (mW): the sum Σ G_{<k}/P_k over the stages that
/// have the point (Pozar §10.3), none until one has.
#[derive(Clone, Copy, Default)]
struct Linearity {
    ip3: Option<f64>,
    p1db: Option<f64>,
}

impl Linearity {
    /// After a stage of own gain `g` (a ratio) behind a chain of gain `before`, of output
    /// intercept `oip3` and compression `op1db` (dBm): its input-referred points are
    /// OIP3/g and OP1dB − (G − 1 dB), each joined as 1/P += G_before/P_k.
    fn then(self, before: f64, g: f64, oip3: Option<f64>, op1db: Option<f64>) -> Linearity {
        let add = |sum: Option<f64>, own: Option<f64>| match (sum, own) {
            (s, Some(p)) => Some(s.unwrap_or(0.0) + before / p),
            (s, None) => s,
        };
        Linearity { ip3: add(self.ip3, oip3.map(|o| mw(o) / g)), p1db: add(self.p1db, op1db.map(|o| mw(o) / g * mw(1.0))) }
    }
}

/// The chain read at `f` (Hz): its blocks, their parts, the source's impedance (none: the
/// chain's), the source and receiver settings.
fn point(blocks: &[Block], parts: &[&Part], f: f64, antenna: Option<&SourceImpedance>, source: &Source, receiver: &Receiver) -> Result<Point> {
    let z0 = receiver.impedance;
    let walk = walk(blocks, f, receiver.temperature)?;
    let zs = match antenna {
        Some(a) => a.at(f),
        None => Some(C::new(z0, 0.0)),
    };
    let mut why = zs.is_none().then(|| "the antenna has no impedance here: outside its band".to_string());
    let gs = zs.map(|z| (z - z0) / (z + z0));
    let budget = receiver.bandwidth.map(|bandwidth| Budget { antenna_temperature: source.temperature, bandwidth, snr_db: receiver.snr });
    let mut rows = Vec::with_capacity(walk.len());
    let (mut before, mut lin) = (Some(1.0), Linearity::default());
    for (k, w) in walk.iter().enumerate() {
        let gain = gs.and_then(|g| two_port_of(w.chain, z0)?.available_gain(g));
        let figure = match (&w.noise, zs) {
            (Ok(n), Some(z)) => Some(n.figure(z)),
            (Err(e), _) => {
                why.get_or_insert_with(|| e.clone());
                None
            }
            _ => None,
        };
        let stage = gain.zip(before).map(|(g, b)| g / b);
        let (oip3, op1db) = match parts.get(k) {
            Some(Part::Amplifier { oip3, p1db, .. }) => (*oip3, *p1db),
            _ => (None, None),
        };
        if let (Some(b), Some(g)) = (before, stage) {
            lin = lin.then(b, g, oip3, op1db);
        }
        let te = figure.map(|f| T0 * (f - 1.0));
        let signal = source.power.zip(gain).map(|(p, g)| p + db(g));
        let noise = budget.zip(figure).zip(gain).map(|((b, f), g)| b.reading(f).noise_dbm + db(g));
        let input = |sum: Option<f64>| sum.filter(|x| *x > 0.0).map(|x| db(1.0 / x));
        rows.push(Row {
            stage_gain_db: stage.map(db),
            gain_db: gain.map(db),
            nf_db: figure.map(db),
            noise_temperature: te,
            signal_dbm: signal,
            noise_dbm: noise,
            snr_db: signal.zip(noise).map(|(s, n)| s - n),
            iip3_dbm: input(lin.ip3),
            oip3_dbm: input(lin.ip3).zip(gain).map(|(i, g)| i + db(g)),
            ip1db_dbm: input(lin.p1db),
            op1db_dbm: input(lin.p1db).zip(gain).map(|(i, g)| i + db(g) - 1.0),
        });
        before = gain;
    }
    let last = walk.last().map_or([ONE, ZERO, ZERO, ONE], |w| w.chain);
    let s = s_of_chain(last, z0).ok_or_else(|| EngineError::failed("the chain has no S on its impedance"))?;
    let figure = rows.last().and_then(|r| r.nf_db).map(|d| 10f64.powf(d / 10.0));
    let reading = budget.zip(figure).map(|(b, f)| b.reading(f));
    Ok(Point {
        frequency: f,
        s: s.map(|c| [c.re, c.im]),
        rows,
        system_temperature: reading.map(|r| r.system_temperature),
        sensitivity_dbm: reading.map(|r| r.sensitivity_dbm),
        why,
    })
}

/// The stage `stage` names (its name, or its position from 1).
fn stage_index(s: &Session, p: &Params) -> Result<usize> {
    let key = p.raw("stage").ok_or_else(|| p.error("stage", "name the stage (or give its position from 1)"))?;
    let n = s.model.system.stages.len();
    match key {
        Value::Number(_) => {
            let k = p.integer("stage", 1..=n.max(1)).required()?;
            if k > n { Err(p.error("stage", "no such stage")) } else { Ok(k - 1) }
        }
        _ => {
            let name = p.string("stage").required()?;
            s.model.system.index(name).map_err(|e| p.error("stage", e.to_string()))
        }
    }
}

/// The reply of a chain edit: its stages in order.
fn chain_reply(s: &Session) -> Value {
    let stages: Vec<Value> = s
        .model
        .system
        .stages
        .iter()
        .map(|st| {
            let mut v = stage_params(s, &st.part);
            v["name"] = json!(st.name);
            v
        })
        .collect();
    json!({"stages": stages})
}

/// `system.stage.add`: a stage of `type` with its settings, named `name` (default the kind and
/// a number), at `position` from 1 (default the end).
pub fn stage_add(s: &mut Session, p: &Params) -> Result<Value> {
    let part = part(s, p)?;
    let name = match p.string("name").optional()? {
        Some(n) => crate::text::clip(n, 64),
        None => s.model.system.fresh_name(part.kind()),
    };
    let at = p.integer("position", 1..=MAX_STAGES + 1).optional()?.map_or(usize::MAX, |k| k - 1);
    s.model.add_stage(at, Stage { name, part })?;
    Ok(chain_reply(s))
}

/// `system.stage.edit`: the settings given replace the stage's own (`rename` its name; `type`
/// another kind, whose settings must then be given).
pub fn stage_edit(s: &mut Session, p: &Params) -> Result<Value> {
    let i = stage_index(s, p)?;
    let old = s.model.system.stages[i].clone();
    let mut merged = stage_params(s, &old.part);
    let given = p.value().as_object().cloned().unwrap_or_default();
    if given.get("type").is_some_and(|t| t.as_str() != Some(old.part.kind())) {
        merged = json!({});
    }
    for (k, v) in given {
        if k != "stage" && k != "rename" {
            merged[k] = v;
        }
    }
    let vars = s.variables().clone();
    let q = Params::new(&merged, s.display).with_variables(&vars);
    let part = part(s, &q)?;
    let name = p.string("rename").optional()?.map_or(old.name, |n| crate::text::clip(n, 64));
    s.model.set_stage(i, Stage { name, part })?;
    Ok(chain_reply(s))
}

/// `system.stage.delete`.
pub fn stage_delete(s: &mut Session, p: &Params) -> Result<Value> {
    let i = stage_index(s, p)?;
    s.model.remove_stage(i)?;
    Ok(chain_reply(s))
}

/// `system.stage.move`: the stage to `position` from 1.
pub fn stage_move(s: &mut Session, p: &Params) -> Result<Value> {
    let i = stage_index(s, p)?;
    let to = p.integer("position", 1..=MAX_STAGES).required()? - 1;
    s.model.move_stage(i, to)?;
    Ok(chain_reply(s))
}

/// `system.source`: the settings given replace the source's own (`antenna` "" for none).
pub fn source(s: &mut Session, p: &Params) -> Result<Value> {
    let mut src = s.model.system.source.clone();
    if let Some(a) = p.string("antenna").optional()? {
        src.antenna = (!a.is_empty()).then(|| crate::text::clip(a, 128));
    }
    if let Some(t) = p.number("temperature").optional()? {
        src.temperature = t;
    }
    if p.has("power") {
        src.power = p.number("power").optional()?;
    }
    s.model.set_system_source(src)?;
    Ok(json!({"source": s.model.system.source}))
}

/// `system.receiver`: the settings given replace the receiver's own.
pub fn receiver(s: &mut Session, p: &Params) -> Result<Value> {
    let mut r = s.model.system.receiver.clone();
    if let Some(z) = p.positive("impedance").optional()? {
        r.impedance = z;
    }
    if let Some(t) = p.number("temperature").optional()? {
        r.temperature = t;
    }
    if p.has("bandwidth") {
        r.bandwidth = p.frequency("bandwidth").optional()?;
    }
    if let Some(x) = p.number("snr").optional()? {
        r.snr = x;
    }
    s.model.set_receiver(r)?;
    let r = &s.model.system.receiver;
    Ok(json!({"impedance": r.impedance, "temperature": r.temperature, "bandwidth": r.bandwidth.map(|b| s.show_frequency(b)), "snr": r.snr}))
}

/// The chain's blocks, ready to evaluate.
pub(crate) fn blocks(s: &Session) -> Result<Vec<Block>> {
    let z0 = s.model.system.receiver.impedance;
    s.model
        .system
        .stages
        .iter()
        .map(|st| Block::of(s, &st.part, z0).map_err(|e| EngineError::failed(format!("stage `{}`: {e}", crate::text::clip(&st.name, 40)))))
        .collect()
}

/// `solve.system`: the chain at `points` frequencies from `start` to `stop` (or the confirmed
/// settings'), its budget shown at `frequency` (default the band's centre).
pub fn solve(s: &mut Session, p: &Params) -> Result<Value> {
    if s.model.system.stages.is_empty() {
        return Err(EngineError::failed("the chain has no stages: add one with system.stage.add"));
    }
    let setup = match s.model.job.setup() {
        Some(emcraft_model::Setup::System { band, points, frequency }) => Some((*band, points.get(), *frequency)),
        _ => None,
    };
    let start = match p.frequency("start").optional()? {
        Some(f) => f,
        None => setup.map(|x| x.0.start.get()).ok_or_else(|| p.error("start", "give the band (or confirm it with project.setup)"))?,
    };
    let stop = match p.frequency("stop").optional()? {
        Some(f) => f,
        None => setup.map(|x| x.0.stop.get()).ok_or_else(|| p.error("stop", "give the band (or confirm it with project.setup)"))?,
    };
    if !(start > 0.0 && stop >= start) {
        return Err(p.error("stop", "at or above `start`, both positive"));
    }
    let n = p.integer("points", 1..=MAX_POINTS).or(setup.map_or(101, |x| x.1))?;
    let freqs = if n == 1 || stop == start { vec![start] } else { crate::numbers::evenly_spaced(start, stop, n) };
    let at = match p.frequency("frequency").optional()? {
        Some(f) => f,
        None => setup.and_then(|x| x.2).map_or(0.5 * (start + stop), emcraft_model::Hertz::get),
    };
    let blocks = blocks(s)?;
    let antenna = match &s.model.system.source.antenna {
        Some(name) => Some(crate::network::named_impedance(s, name)?),
        None => None,
    };
    let parts: Vec<&Part> = s.model.system.stages.iter().map(|st| &st.part).collect();
    let (source, receiver) = (s.model.system.source.clone(), s.model.system.receiver.clone());
    let mut points = Vec::with_capacity(freqs.len());
    for (i, f) in freqs.iter().enumerate() {
        crate::progress::report(i, freqs.len(), "solve.system")?;
        points.push(point(&blocks, &parts, *f, antenna.as_ref(), &source, &receiver)?);
    }
    let result = SystemResult { stages: s.model.system.stages.iter().map(|st| (st.name.clone(), st.part.kind().to_owned())).collect(), points, at };
    let reply = reply(s, &result);
    s.results.system = Some(Arc::new(result));
    Ok(reply)
}

/// A result's reply: the budget at its frequency, the band's least and greatest noise figure.
pub(crate) fn reply(s: &Session, r: &SystemResult) -> Value {
    let nf = |p: &Point| p.rows.last().and_then(|r| r.nf_db);
    let least = r.points.iter().filter_map(|p| nf(p).map(|x| (p.frequency, x))).min_by(|a, b| a.1.total_cmp(&b.1));
    let most = r.points.iter().filter_map(|p| nf(p).map(|x| (p.frequency, x))).max_by(|a, b| a.1.total_cmp(&b.1));
    let budget = r.nearest(r.at).map(|p| {
        let rows: Vec<Value> = r
            .stages
            .iter()
            .zip(&p.rows)
            .map(|((name, kind), row)| {
                let mut v = serde_json::to_value(row).unwrap_or(Value::Null);
                v["stage"] = json!(name);
                v["type"] = json!(kind);
                v
            })
            .collect();
        json!({
            "frequency": s.show_frequency(p.frequency),
            "stages": rows,
            "system_temperature": p.system_temperature,
            "sensitivity_dbm": p.sensitivity_dbm,
            "why": p.why,
        })
    });
    json!({
        "frequencies": r.points.len(),
        "budget": budget,
        "nf_db_least": least.map(|x| x.1),
        "least_at": least.map(|x| s.show_frequency(x.0)),
        "nf_db_most": most.map(|x| x.1),
        "most_at": most.map(|x| s.show_frequency(x.0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matched_lines_chain_matrix_is_its_own() {
        // λ/4 at 1 GHz in vacuum: A = D = 0, B = j Z₀, C = j/Z₀; two make a half wave, S21 = −1
        // (Pozar, Microwave Engineering, 4th ed., §4.4, Tables 4.1–4.2).
        let m = Block::Line { z0: 50.0, length: C0 / 4e9, eps_r: 1.0, loss: LineLoss::default() }.chain(1e9).unwrap();
        assert!(m[0].norm() < 1e-12 && (m[1] - C::new(0.0, 50.0)).norm() < 1e-9);
        let s = s_of_chain(times(m, m), 50.0).unwrap();
        assert!((s[2] + ONE).norm() < 1e-12 && s[0].norm() < 1e-12);
    }
}

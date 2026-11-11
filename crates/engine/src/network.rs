//! S-parameters as a network: measured ones read from Touchstone files (kept with the project's
//! views, drawn over the sweep and compared with it), the network matrices read from S —
//! impedance Z, admittance Y, chain ABCD (Pozar, Microwave Engineering, 4th ed., §4.3–4.4) —
//! and mixed-mode S of ports paired as differential lines (Bockelman & Eisenstadt, IEEE Trans.
//! MTT 43(7), 1995).
//!
//! The Touchstone reader follows the IBIS Open Forum's Touchstone File Format Specification
//! (version 2.0, 2009, which keeps version 1.1's files valid): the option line
//! `# <unit> <parameter> <format> R <ohms>`, comments after `!`, the values of a frequency
//! over as many lines as they take, a two-port's in the order S11 S21 S12 S22 (version 1, or
//! `[Two-Port Data Order] 21_12`), another's row by row; version 2's keywords for the number
//! of ports, the per-port references and the network data's extent. A two-port's noise
//! parameters follow its data (version 1: from a frequency no higher than the last;
//! version 2: `[Noise Data]`), five numbers a frequency: the frequency, F_min in dB, Γ_opt as
//! magnitude and angle on the option line's R whatever the format, and R_n — normalised to R in
//! version 1, in ohms in version 2 (Touchstone 2.0, "Noise Parameters").

use std::io::Read as _;
use std::sync::Arc;

use emcraft_solver::{Complex64 as C, MixedWave, SMatrix};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::noise::{ChainNoise, NoiseParameters};
use crate::sdata::{MAX_ENTRIES, MAX_FREQUENCIES, MAX_VALUES, SData};
use crate::{EngineError, Params, Result, Session};

/// The most measurements a project keeps.
pub const MAX_MEASURED: usize = 8;

/// The largest Touchstone file read (bytes).
pub const MAX_FILE: u64 = 64 * 1024 * 1024;

/// The longest name of a measurement (characters).
const MAX_NAME: usize = 64;

/// S-parameters read from a file (a measurement, another tool's): its name, the file it came
/// from, each port's reference impedance (Ω), S over frequency (every port excited) and a
/// two-port's noise, port 1 its input, at the frequencies it is known (none: not known).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(into = "Stored", try_from = "Stored")]
pub struct Measured {
    pub name: String,
    pub file: String,
    pub references: Vec<f64>,
    pub data: Arc<SData>,
    pub noise: Vec<NoisePoint>,
}

/// A two-port's noise at one frequency (Hz).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoisePoint {
    pub frequency: f64,
    pub noise: ChainNoise,
}

/// The noise of `points` at `f` (Hz), linear in frequency between its two nearest; none outside
/// them.
pub fn noise_at(points: &[NoisePoint], f: f64) -> Option<ChainNoise> {
    let k = points.partition_point(|p| p.frequency < f);
    let hi = points.get(k)?;
    if hi.frequency == f || k == 0 {
        return (hi.frequency == f).then_some(hi.noise);
    }
    let lo = points.get(k - 1)?;
    let t = (f - lo.frequency) / (hi.frequency - lo.frequency);
    let mix = |a: f64, b: f64| a + (b - a) * t;
    let (a, b) = (lo.noise, hi.noise);
    Some(ChainNoise { vv: mix(a.vv, b.vv), vi: a.vi + (b.vi - a.vi) * t, ii: mix(a.ii, b.ii) })
}

/// Whether `points` are a noise record: rising frequencies, each finite, a non-negative noise
/// voltage and current; at most [`MAX_FREQUENCIES`].
fn check_noise(points: &[NoisePoint]) -> std::result::Result<(), String> {
    if points.len() > MAX_FREQUENCIES {
        return Err(format!("more than {MAX_FREQUENCIES} noise frequencies"));
    }
    let fine = |p: &NoisePoint| {
        let n = p.noise;
        p.frequency.is_finite() && p.frequency > 0.0 && n.vv.is_finite() && n.vv >= 0.0 && n.ii.is_finite() && n.ii >= 0.0 && n.vi.is_finite()
    };
    if !points.iter().all(fine) || !points.windows(2).all(|w| w[0].frequency < w[1].frequency) {
        return Err("noise data: rising frequencies, finite values, R_n ≥ 0".into());
    }
    Ok(())
}

/// A measurement as a project file holds it: S flat, real and imaginary parts in turn.
#[derive(Clone, Serialize, Deserialize)]
struct Stored {
    name: String,
    #[serde(default)]
    file: String,
    references: Vec<f64>,
    frequencies: Vec<f64>,
    ports: usize,
    s: Vec<f64>,
    /// Each noise frequency's [f, C₁₁, Re C₁₂, Im C₁₂, C₂₂] (Hz, chain correlations).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    noise: Vec<[f64; 5]>,
}

impl From<Measured> for Stored {
    fn from(m: Measured) -> Stored {
        Stored {
            ports: m.data.outputs(),
            frequencies: m.data.frequencies().to_vec(),
            s: m.data.values().iter().flat_map(|c| [c.re, c.im]).collect(),
            noise: m.noise.iter().map(|p| [p.frequency, p.noise.vv, p.noise.vi.re, p.noise.vi.im, p.noise.ii]).collect(),
            name: m.name,
            file: m.file,
            references: m.references,
        }
    }
}

impl TryFrom<Stored> for Measured {
    type Error = String;

    /// A project file's measurement, checked as a file's: its name, references and S.
    fn try_from(s: Stored) -> std::result::Result<Measured, String> {
        check_name(&s.name)?;
        if s.ports == 0 || s.ports > MAX_ENTRIES || s.references.len() != s.ports || !s.references.iter().all(|r| r.is_finite() && *r > 0.0) {
            return Err(format!("measurement `{}`: one positive reference impedance per port", crate::text::clip(&s.name, 40)));
        }
        if !s.s.len().is_multiple_of(2) || s.s.len() / 2 > MAX_VALUES {
            return Err(format!("measurement `{}`: S is pairs of real and imaginary parts", crate::text::clip(&s.name, 40)));
        }
        let values = s.s.as_chunks::<2>().0.iter().map(|p| C::new(p[0], p[1])).collect();
        let data = SData::square(s.frequencies, s.ports, values).map_err(|e| format!("measurement `{}`: {e}", crate::text::clip(&s.name, 40)))?;
        let noise: Vec<NoisePoint> =
            s.noise.iter().map(|x| NoisePoint { frequency: x[0], noise: ChainNoise { vv: x[1], vi: C::new(x[2], x[3]), ii: x[4] } }).collect();
        check_noise(&noise).map_err(|e| format!("measurement `{}`: {e}", crate::text::clip(&s.name, 40)))?;
        if !noise.is_empty() && s.ports != 2 {
            return Err(format!("measurement `{}`: noise data is a two-port's", crate::text::clip(&s.name, 40)));
        }
        Ok(Measured { name: s.name, file: crate::text::clip(&s.file, 4096), references: s.references, data: Arc::new(data), noise })
    }
}

/// Whether `name` may name a measurement: 1 to 64 characters, not all blank.
fn check_name(name: &str) -> std::result::Result<(), String> {
    if name.trim().is_empty() || name.chars().count() > MAX_NAME {
        return Err(format!("a measurement's name has 1 to {MAX_NAME} characters"));
    }
    Ok(())
}

/// What a Touchstone file holds once read: S on each port's reference (Ω), the parameter it
/// was written as (`S`, `Y` or `Z`) and a two-port's noise (none when it gives none).
#[derive(Clone, Debug, PartialEq)]
pub struct Touchstone {
    pub data: SData,
    pub references: Vec<f64>,
    pub parameter: char,
    pub noise: Vec<NoisePoint>,
}

/// How a file writes each complex value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pair {
    Ri,
    Ma,
    Db,
}

impl Pair {
    fn value(self, a: f64, b: f64) -> C {
        match self {
            Pair::Ri => C::new(a, b),
            Pair::Ma => C::from_polar(a, b.to_radians()),
            Pair::Db => C::from_polar(10f64.powf(a / 20.0), b.to_radians()),
        }
    }
}

/// The number of ports a file's name gives: `.s2p` two, `.s12p` twelve; none for another.
///
/// ```
/// use emcraft_engine::network::ports_of_name;
/// assert_eq!(ports_of_name("filter.s2p"), Some(2));
/// assert_eq!(ports_of_name("/a/b/LINE.S12P"), Some(12));
/// assert_eq!(ports_of_name("data.ts"), None);
/// ```
pub fn ports_of_name(path: &str) -> Option<usize> {
    let ext = std::path::Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    let digits = ext.strip_prefix('s')?.strip_suffix('p')?;
    digits.parse().ok().filter(|n| (1..=MAX_ENTRIES).contains(n))
}

/// A Touchstone file's text read (see the module docs): `ports` from its name when the file
/// does not say (version 1). Hostile text is refused with what is wrong and on which line.
///
/// ```
/// use emcraft_engine::network::parse_touchstone;
///
/// // A version 1 two-port in dB and degrees: a 3 dB pad, matched.
/// let text = "! a pad\n# MHz S DB R 50\n1000 -40 0 -3 -90 -3 -90 -40 0\n2000 -40 0 -3 -180 -3 -180 -40 0\n";
/// let t = parse_touchstone(text, Some(2)).unwrap();
/// assert_eq!(t.data.frequencies(), [1e9, 2e9]);
/// let s21 = t.data.get(0, 1, 0).unwrap();
/// assert!((s21.norm() - 10f64.powf(-3.0 / 20.0)).abs() < 1e-12 && (s21.arg() + std::f64::consts::FRAC_PI_2).abs() < 1e-12);
/// // A record cut short, or a word among the numbers, is refused.
/// assert!(parse_touchstone("# GHz S RI\n1 0 0 1\n", Some(2)).is_err());
/// assert!(parse_touchstone("# GHz S RI\n1 0 x\n", Some(1)).is_err());
/// ```
pub fn parse_touchstone(text: &str, ports: Option<usize>) -> std::result::Result<Touchstone, String> {
    let mut reader = Reader::new(ports);
    for (k, raw) in text.lines().enumerate() {
        let line = raw.split('!').next().unwrap_or("").trim();
        if !line.is_empty() && reader.line(k + 1, line)? == Flow::Stop {
            break;
        }
    }
    let Reader { header, numbers, noise: noise_numbers, .. } = reader;
    let n = header.ports.ok_or("the number of ports is not known: name the file .s<n>p")?;
    if !(header.reference.is_finite() && header.reference > 0.0) {
        return Err("the reference impedance R must be positive".into());
    }
    let references = header.references.clone().unwrap_or_else(|| vec![header.reference; n]);
    if references.len() != n || !references.iter().all(|r| r.is_finite() && *r > 0.0) {
        return Err(format!("one positive reference impedance for each of the {n} ports"));
    }
    let (frequencies, s, used) = records(&header, n, &references, &numbers)?;
    if frequencies.is_empty() {
        return Err("no frequencies".into());
    }
    let rest = if header.version2 { &noise_numbers[..] } else { numbers.get(used..).unwrap_or(&[]) };
    if !rest.is_empty() && n != 2 {
        return Err("noise data is a two-port's".into());
    }
    let noise = noise_points(&header, rest)?;
    let data = SData::square(frequencies, n, s)?;
    Ok(Touchstone { data, references, parameter: header.parameter, noise })
}

/// What a Touchstone file's option line and keywords say.
struct Header {
    /// Hz per frequency unit.
    unit: f64,
    /// `S`, `Y` or `Z`.
    parameter: char,
    pair: Pair,
    /// The option line's R (Ω), every port's unless `[Reference]` gives each its own.
    reference: f64,
    references: Option<Vec<f64>>,
    ports: Option<usize>,
    /// Version 2 (its keywords; Z and Y in ohms and siemens, not normalised).
    version2: bool,
    /// A two-port's values in the order S11 S21 S12 S22 (version 1's).
    order21: bool,
}

/// Whether reading goes on after a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    Continue,
    Stop,
}

/// A Touchstone file read line by line: its header so far, where it is (an information block,
/// `[Reference]`'s continued values, `[Network Data]`) and the numbers of its data, each with its
/// line.
struct Reader {
    header: Header,
    option_seen: bool,
    in_references: bool,
    in_information: bool,
    in_data: bool,
    in_noise: bool,
    numbers: Vec<(f64, usize)>,
    /// Version 2's `[Noise Data]` numbers.
    noise: Vec<(f64, usize)>,
}

impl Reader {
    fn new(ports: Option<usize>) -> Reader {
        let header = Header { unit: 1e9, parameter: 'S', pair: Pair::Ma, reference: 50.0, references: None, ports, version2: false, order21: true };
        Reader {
            header,
            option_seen: false,
            in_references: false,
            in_information: false,
            in_data: false,
            in_noise: false,
            numbers: Vec::new(),
            noise: Vec::new(),
        }
    }

    /// One line's text (its comment cut, not empty).
    fn line(&mut self, line_no: usize, line: &str) -> std::result::Result<Flow, String> {
        if let Some(rest) = line.strip_prefix('[') {
            self.in_references = false;
            return self.keyword(line_no, rest);
        }
        if self.in_information {
            return Ok(Flow::Continue);
        }
        if let Some(rest) = line.strip_prefix('#') {
            // Only the first option line counts.
            if !self.option_seen {
                self.option_seen = true;
                self.options(line_no, rest)?;
            }
            return Ok(Flow::Continue);
        }
        self.data(line_no, line)?;
        Ok(Flow::Continue)
    }

    /// A version 2 keyword line, after its `[`.
    fn keyword(&mut self, line_no: usize, rest: &str) -> std::result::Result<Flow, String> {
        let (key, value) = rest.split_once(']').ok_or_else(|| format!("line {line_no}: a keyword without `]`"))?;
        let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
        let h = &mut self.header;
        match key.as_str() {
            "begin information" => self.in_information = true,
            "end information" => self.in_information = false,
            _ if self.in_information => {}
            "version" => h.version2 = true,
            "number of ports" => {
                let v: usize = value.parse().map_err(|_| format!("line {line_no}: the number of ports is a whole number"))?;
                if !(1..=MAX_ENTRIES).contains(&v) {
                    return Err(format!("line {line_no}: from 1 to {MAX_ENTRIES} ports"));
                }
                h.ports = Some(v);
            }
            "two-port data order" => {
                h.order21 = match value {
                    "12_21" => false,
                    "21_12" => true,
                    _ => return Err(format!("line {line_no}: the two-port data order is 12_21 or 21_12")),
                }
            }
            "reference" => {
                let r = value.split_whitespace().map(str::parse::<f64>).collect::<std::result::Result<Vec<_>, _>>();
                h.references = Some(r.map_err(|_| format!("line {line_no}: references are numbers"))?);
                self.in_references = true;
            }
            "matrix format" if !value.eq_ignore_ascii_case("full") => {
                return Err(format!("line {line_no}: not supported yet: a {value} matrix format (only Full)"));
            }
            "mixed-mode order" => return Err(format!("line {line_no}: not supported yet: mixed-mode files")),
            "network data" => self.in_data = true,
            "noise data" => (self.in_data, self.in_noise) = (false, true),
            "end" => return Ok(Flow::Stop),
            _ => {}
        }
        Ok(Flow::Continue)
    }

    /// The option line, after its `#`: the frequency unit, the parameter, the format and R.
    fn options(&mut self, line_no: usize, rest: &str) -> std::result::Result<(), String> {
        let h = &mut self.header;
        let words: Vec<String> = rest.split_whitespace().map(str::to_ascii_lowercase).collect();
        let mut w = words.iter();
        while let Some(word) = w.next() {
            match word.as_str() {
                "hz" => h.unit = 1.0,
                "khz" => h.unit = 1e3,
                "mhz" => h.unit = 1e6,
                "ghz" => h.unit = 1e9,
                "s" => h.parameter = 'S',
                "y" => h.parameter = 'Y',
                "z" => h.parameter = 'Z',
                "g" | "h" => return Err(format!("line {line_no}: not supported yet: hybrid ({}) parameters", word.to_ascii_uppercase())),
                "ri" => h.pair = Pair::Ri,
                "ma" => h.pair = Pair::Ma,
                "db" => h.pair = Pair::Db,
                "r" => h.reference = w.next().and_then(|v| v.parse::<f64>().ok()).ok_or_else(|| format!("line {line_no}: R needs a resistance"))?,
                other => return Err(format!("line {line_no}: `{}` is not an option", crate::text::clip(other, 20))),
            }
        }
        Ok(())
    }

    /// A line of numbers: `[Reference]`'s continued, or the data's.
    fn data(&mut self, line_no: usize, line: &str) -> std::result::Result<(), String> {
        let values = line.split_whitespace().map(str::parse::<f64>).collect::<std::result::Result<Vec<_>, _>>();
        let values = values.map_err(|_| format!("line {line_no}: numbers expected"))?;
        if self.in_references {
            self.header.references.get_or_insert_with(Vec::new).extend(values);
            return Ok(());
        }
        if self.header.version2 && !self.in_data && !self.in_noise {
            return Err(format!("line {line_no}: numbers before [Network Data]"));
        }
        let to = if self.in_noise { &mut self.noise } else { &mut self.numbers };
        if to.len() + values.len() > 2 * MAX_VALUES + 5 * MAX_FREQUENCIES {
            return Err(format!("more than {MAX_VALUES} values"));
        }
        to.extend(values.into_iter().map(|v| (v, line_no)));
        Ok(())
    }
}

/// The frequencies (Hz) and S (row-major per frequency) of a file's numbers: 1 + 2n² of them
/// a frequency, a two-port's reordered from S11 S21 S12 S22 (version 1), Y and Z turned into
/// S on `references`, and how many numbers they took: a version 1 two-port's noise data (from a
/// frequency no higher than the last) follows them.
fn records(h: &Header, n: usize, references: &[f64], numbers: &[(f64, usize)]) -> std::result::Result<(Vec<f64>, Vec<C>, usize), String> {
    let per = 1 + 2 * n * n;
    let mut frequencies = Vec::new();
    let mut s: Vec<C> = Vec::new();
    let mut used = 0;
    for record in numbers.chunks(per) {
        let (f, line_no) = record[0];
        if !h.version2 && n == 2 && frequencies.last().is_some_and(|last| f * h.unit <= *last) {
            break;
        }
        used += record.len();
        if record.len() < per {
            return Err(format!("line {line_no}: the frequency's values stop short ({per} numbers each)"));
        }
        if frequencies.len() == MAX_FREQUENCIES {
            return Err(format!("more than {MAX_FREQUENCIES} frequencies"));
        }
        frequencies.push(f * h.unit);
        let values: Vec<C> = record[1..].as_chunks::<2>().0.iter().map(|p| h.pair.value(p[0].0, p[1].0)).collect();
        let m: Vec<C> = if n == 2 && h.order21 { vec![values[0], values[2], values[1], values[3]] } else { values };
        s.extend(if h.parameter == 'S' {
            m
        } else {
            to_s(h, n, references, m).ok_or_else(|| format!("line {line_no}: {} has no S here", h.parameter))?
        });
    }
    Ok((frequencies, s, used))
}

/// A two-port's noise from its noise data's numbers, five a frequency: F_min (dB), Γ_opt's
/// magnitude and angle (°) on the option line's R, and R_n (normalised to R in version 1, in
/// ohms in version 2).
fn noise_points(h: &Header, numbers: &[(f64, usize)]) -> std::result::Result<Vec<NoisePoint>, String> {
    let r = h.reference;
    let points = numbers
        .chunks(5)
        .map(|x| {
            let line_no = x[0].1;
            let [(f, _), (nf, _), (mag, _), (angle, _), (rn, _)] = x else {
                return Err(format!("line {line_no}: noise data has five numbers a frequency"));
            };
            let gamma = C::from_polar(*mag, angle.to_radians());
            let y_opt = (C::new(1.0, 0.0) - gamma) / ((C::new(1.0, 0.0) + gamma) * r);
            let rn = if h.version2 { *rn } else { rn * r };
            if !(nf.is_finite() && *nf >= 0.0 && *mag >= 0.0 && *mag < 1.0 && y_opt.is_finite() && rn.is_finite() && rn >= 0.0) {
                return Err(format!("line {line_no}: noise data needs F_min ≥ 0 dB, |Γ_opt| < 1 and R_n ≥ 0"));
            }
            let parameters = NoiseParameters { f_min: 10f64.powf(nf / 10.0), y_opt, rn };
            Ok(NoisePoint { frequency: f * h.unit, noise: ChainNoise::of(parameters) })
        })
        .collect::<std::result::Result<Vec<_>, String>>()?;
    check_noise(&points)?;
    Ok(points)
}

/// S from a frequency's Y or Z (row-major): version 1's normalised to the references,
/// version 2's in siemens or ohms.
fn to_s(h: &Header, n: usize, references: &[f64], m: Vec<C>) -> Option<Vec<C>> {
    let scale = |i: usize, j: usize| if h.version2 { 1.0 } else { (references[i] * references[j]).sqrt() };
    let m: Vec<C> =
        m.into_iter().enumerate().map(|(k, v)| if h.parameter == 'Z' { v * scale(k / n, k % n) } else { v / scale(k / n, k % n) }).collect();
    let sm = if h.parameter == 'Z' { SMatrix::from_impedance(&m, references) } else { SMatrix::from_admittance(&m, references) }?;
    Some(sm.rows().flatten().copied().collect())
}

/// A file read whole, at most [`MAX_FILE`] bytes.
fn read_file(path: &str) -> Result<String> {
    let failed = |e: std::io::Error| EngineError::failed(format!("`{path}`: {e}"));
    let file = std::fs::File::open(path).map_err(failed)?;
    let mut text = String::new();
    file.take(MAX_FILE + 1).read_to_string(&mut text).map_err(failed)?;
    if text.len() as u64 > MAX_FILE {
        return Err(EngineError::failed(format!("`{path}`: larger than {} MB", MAX_FILE >> 20)));
    }
    Ok(text)
}

/// `results.import`: a Touchstone file's S kept as a measurement, by name (the file's stem
/// unless `name` is given; one of the same name replaced), drawn over the S-parameter chart.
pub fn import(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let text = match crate::geometry::template_file(p, path)? {
        Some(bytes) => String::from_utf8(bytes.to_vec()).map_err(|e| EngineError::failed(format!("`{path}`: {e}")))?,
        None => read_file(path)?,
    };
    let t = parse_touchstone(&text, ports_of_name(path)).map_err(|e| EngineError::failed(format!("`{path}`: {e}")))?;
    let stem = std::path::Path::new(path).file_stem().and_then(|x| x.to_str()).unwrap_or("measured").to_string();
    let name = p.string("name").optional()?.map_or(stem, str::to_string);
    check_name(&name).map_err(|e| p.error("name", e))?;
    let m = Measured { name: name.clone(), file: path.to_string(), references: t.references.clone(), data: Arc::new(t.data), noise: t.noise.clone() };
    keep(s, m.clone())?;
    let mut reply = summary(s, &m);
    if let Some(sweep) = s.results.sweep.as_ref()
        && let Ok(sim) = sweep.sdata()
        && let Some(o) = reply.as_object_mut()
    {
        o.insert("compared".into(), compare(s, &sim, &m.data));
    }
    if let Some(o) = reply.as_object_mut() {
        o.insert("parameter".into(), json!(t.parameter.to_string()));
    }
    Ok(reply)
}

/// Keeps `m` with the project's measurements, replacing one of the same name; an error past
/// [`MAX_MEASURED`].
pub(crate) fn keep(s: &mut Session, m: Measured) -> Result<()> {
    let list = &mut s.views.measured;
    match list.iter().position(|x| x.name == m.name) {
        Some(k) => list[k] = m,
        None if list.len() >= MAX_MEASURED => {
            return Err(EngineError::failed(format!("at most {MAX_MEASURED} measurements: delete one first (results.import.delete)")));
        }
        None => list.push(m),
    }
    Ok(())
}

/// The name a computed response is kept under: `name` when given (checked), else `default`.
pub(crate) fn kept_name(p: &Params, default: &str) -> Result<String> {
    let name = p.string("name").optional()?.map_or_else(|| default.to_string(), str::to_string);
    check_name(&name).map_err(|e| p.error("name", e))?;
    Ok(name)
}

/// What a measurement holds, for replies: its name, file, ports, references and band.
fn summary(s: &Session, m: &Measured) -> Value {
    let f = m.data.frequencies();
    json!({
        "name": m.name,
        "file": m.file,
        "ports": m.data.outputs(),
        "references": m.references,
        "frequencies": f.len(),
        "start": f.first().map(|x| s.show_frequency(*x)),
        "stop": f.last().map(|x| s.show_frequency(*x)),
        "noise_frequencies": m.noise.len(),
    })
}

/// `results.import.delete`: a measurement let go.
pub fn delete(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    let before = s.views.measured.len();
    s.views.measured.retain(|m| m.name != name);
    if s.views.measured.len() == before {
        return Err(p.error("name", format!("no measurement `{}`: {}", crate::text::clip(name, 40), names(s))));
    }
    Ok(json!({"deleted": name, "measured": s.views.measured.len()}))
}

/// The measurements' names, for an error.
fn names(s: &Session) -> String {
    if s.views.measured.is_empty() {
        return "none is imported (results.import)".into();
    }
    format!("imported: {}", s.views.measured.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", "))
}

/// The measurement `name` names, or the only one when no name is given.
fn measurement<'s>(s: &'s Session, p: &Params, key: &str) -> Result<&'s Measured> {
    match p.string(key).optional()? {
        Some(name) => s
            .views
            .measured
            .iter()
            .find(|m| m.name == name)
            .ok_or_else(|| p.error(key, format!("no measurement `{}`: {}", crate::text::clip(name, 40), names(s)))),
        None if s.views.measured.len() == 1 => Ok(&s.views.measured[0]),
        None => Err(p.error(key, format!("name the measurement: {}", names(s)))),
    }
}

/// S\[p\]\[q\] of `data` at `f` (Hz), linear in frequency between its two nearest; none outside
/// its band or when the entry is not held.
pub fn interpolate(data: &SData, p: usize, q: usize, f: f64) -> Option<C> {
    let fs = data.frequencies();
    let k = fs.partition_point(|x| *x < f);
    if k < fs.len() && crate::numbers::same_frequency(fs[k], f) {
        return data.get(k, p, q);
    }
    if k == 0 || k >= fs.len() {
        return None;
    }
    let (a, b) = (data.get(k - 1, p, q)?, data.get(k, p, q)?);
    let u = (f - fs[k - 1]) / (fs[k] - fs[k - 1]);
    Some(a + (b - a) * u)
}

/// The simulated S against the measured, entry by entry over the simulated frequencies within
/// the measured band: the largest and RMS |ΔS| and the largest difference of |S| in dB.
fn compare(s: &Session, sim: &SData, meas: &SData) -> Value {
    let n = sim.outputs().min(meas.outputs());
    let mut rows = Vec::new();
    let mut worst: Option<(f64, String)> = None;
    for &q in sim.inputs().iter().filter(|q| **q < n) {
        for p in 0..n {
            let diffs: Vec<(f64, C, C)> = sim.curve(p, q).filter_map(|(f, a)| Some((f, a, interpolate(meas, p, q, f)?))).collect();
            if diffs.is_empty() {
                continue;
            }
            let d: Vec<f64> = diffs.iter().map(|(_, a, b)| (a - b).norm()).collect();
            let max = d.iter().copied().fold(0.0, f64::max);
            let rms = (d.iter().map(|x| x * x).sum::<f64>() / d.len() as f64).sqrt();
            let db = diffs
                .iter()
                .map(|(_, a, b)| (emcraft_model::units::decibels(a.norm()) - emcraft_model::units::decibels(b.norm())).abs())
                .filter(|x| x.is_finite())
                .fold(0.0, f64::max);
            let entry = format!("S{}{}", p + 1, q + 1);
            if worst.as_ref().is_none_or(|(w, _)| max > *w) {
                worst = Some((max, entry.clone()));
            }
            let at = diffs.iter().zip(&d).max_by(|a, b| a.1.total_cmp(b.1)).map(|((f, ..), _)| s.show_frequency(*f));
            rows.push(
                json!({"entry": entry, "points": d.len(), "max_difference": max, "rms_difference": rms, "max_db_difference": db, "worst_at": at}),
            );
        }
    }
    json!({"entries": rows, "worst": worst.map(|(v, e)| json!({"entry": e, "max_difference": v}))})
}

/// `results.measured`: a measurement against the sweep.
pub fn measured(s: &mut Session, p: &Params) -> Result<Value> {
    let m = measurement(s, p, "name")?;
    let sweep = s.results.sweep()?;
    let sim = sweep.sdata().map_err(EngineError::failed)?;
    let mut reply = summary(s, m);
    let c = compare(s, &sim, &m.data);
    if c["entries"].as_array().is_none_or(Vec::is_empty) {
        return Err(EngineError::failed(format!("the sweep and `{}` share no frequency band", crate::text::clip(&m.name, 40))));
    }
    if let Some(o) = reply.as_object_mut() {
        o.insert("compared".into(), c);
    }
    Ok(reply)
}

/// S over frequency to read a network from: the sweep's or a measurement's, each entry's name
/// and reference impedance (none for a waveguide port's power waves).
pub(crate) struct Source {
    pub data: SData,
    pub names: Vec<String>,
    pub references: Vec<Option<f64>>,
    /// A two-port's noise, port 1 its input (a measurement's; none for the sweep).
    pub noise: Vec<NoisePoint>,
}

/// The S `data` names: `sweep` (the default) or a measurement.
pub(crate) fn source(s: &Session, p: &Params) -> Result<Source> {
    match p.string("data").optional()? {
        None | Some("sweep") => {
            let sweep = s.results.sweep()?;
            let entries = sweep.entries();
            Ok(Source {
                data: sweep.sdata().map_err(EngineError::failed)?,
                names: entries.iter().map(|e| e.name.clone()).collect(),
                references: entries.iter().map(|e| e.reference).collect(),
                noise: Vec::new(),
            })
        }
        Some(_) => {
            let m = measurement(s, p, "data")?;
            Ok(Source {
                data: (*m.data).clone(),
                names: (1..=m.data.outputs()).map(|k| k.to_string()).collect(),
                references: m.references.iter().map(|r| Some(*r)).collect(),
                noise: m.noise.clone(),
            })
        }
    }
}

/// A source's impedance over frequency: a one-port measurement's (an antenna's terminals from
/// solve.radiation_sweep, a .s1p), Z = R(1 + Γ)/(1 − Γ) on its reference R.
pub(crate) struct SourceImpedance {
    pub name: String,
    data: Arc<SData>,
    reference: f64,
}

impl SourceImpedance {
    /// Its impedance at `f` (Hz), Γ linear between its two nearest frequencies; none outside its
    /// band or where Γ = 1.
    pub fn at(&self, f: f64) -> Option<C> {
        let g = interpolate(&self.data, 0, 0, f)?;
        let z = (C::new(1.0, 0.0) + g) / (C::new(1.0, 0.0) - g) * self.reference;
        (z.is_finite() && z.re > 0.0).then_some(z)
    }
}

/// The measurement `source` names, a one-port, when given.
pub(crate) fn source_impedance(s: &Session, p: &Params) -> Result<Option<SourceImpedance>> {
    if !p.has("source") {
        return Ok(None);
    }
    let m = measurement(s, p, "source")?;
    if m.data.outputs() != 1 {
        return Err(p.error("source", format!("a one-port (an antenna's S11); `{}` has {} ports", crate::text::clip(&m.name, 40), m.data.outputs())));
    }
    Ok(Some(SourceImpedance { name: m.name.clone(), data: m.data.clone(), reference: m.references[0] }))
}

/// The ports a two-port reading takes (0-based) from `input` and `output` (1-based, default 1
/// and 2) of `n` ports: different, and both excited in `data` so S₁₁, S₂₁, S₁₂ and S₂₂ are held.
pub(crate) fn port_pair(p: &Params, data: &SData) -> Result<(usize, usize)> {
    let n = data.outputs();
    let input = p.integer("input", 1..=n.max(1)).or(1)? - 1;
    let output = p.integer("output", 1..=n.max(1)).or(2.min(n))? - 1;
    if input == output {
        return Err(p.error("output", "must differ from `input`"));
    }
    if ![input, output].iter().all(|q| data.inputs().contains(q)) {
        return Err(p.error("input", format!("ports {} and {} must both be excited in the data", input + 1, output + 1)));
    }
    Ok((input, output))
}

/// The frequencies to read: the one nearest `frequency` when given, else all.
fn frequencies_of(s: &Session, p: &Params, data: &SData) -> Result<Vec<usize>> {
    let fs = data.frequencies();
    match p.frequency("frequency").optional()? {
        None => Ok((0..fs.len()).collect()),
        Some(f) => {
            let k = (0..fs.len())
                .min_by(|a, b| (fs[*a] - f).abs().total_cmp(&(fs[*b] - f).abs()))
                .ok_or_else(|| EngineError::failed("no frequencies"))?;
            if (fs[k] - f).abs() > 0.5 * (fs[fs.len() - 1] - fs[0]).max(1e-6 * f) && fs.len() > 1 {
                return Err(p.error(
                    "frequency",
                    format!(
                        "outside the band ({} to {} {})",
                        s.show_frequency(fs[0]),
                        s.show_frequency(fs[fs.len() - 1]),
                        s.display.frequency.symbol()
                    ),
                ));
            }
            Ok(vec![k])
        }
    }
}

/// A row-major matrix as rows of [re, im].
pub(crate) fn matrix_json(m: &[C], n: usize) -> Value {
    Value::Array((0..n).map(|i| Value::Array((0..n).map(|j| json!([m[i * n + j].re, m[i * n + j].im])).collect())).collect())
}

emcraft_macros::named_enum! {
    /// Which network matrix `results.network` gives.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Network {
        /// Impedance, Ω.
        Z = "z",
        /// Admittance, S.
        Y = "y",
        /// A two-port's chain matrix [A B; C D].
        Abcd = "abcd",
        /// S itself.
        S = "s",
    }
}

/// `results.network`: Z, Y or ABCD (or S) at each frequency, and each port's input impedance
/// with the others matched.
pub fn network(s: &mut Session, p: &Params) -> Result<Value> {
    let kind = p.pick("kind").or(Network::Z)?;
    let src = source(s, p)?;
    let n = src.data.outputs();
    if src.data.inputs().len() != n {
        return Err(EngineError::failed("not every port was excited: no network matrix"));
    }
    if kind == Network::Abcd && n != 2 {
        return Err(p.error("kind", format!("ABCD is a two-port's; this has {n} ports")));
    }
    let given = p.positive("impedance").optional()?;
    let normalized = src.references.iter().any(Option::is_none) && given.is_none();
    let z0: Vec<f64> = src.references.iter().map(|r| r.or(given).unwrap_or(1.0)).collect();
    let mut matrices = Vec::new();
    let mut zin = Vec::new();
    let mut freqs = Vec::new();
    for k in frequencies_of(s, p, &src.data)? {
        let sm = SMatrix::from_fn(n, |i, j| src.data.at(k)[i * n + j]);
        let f = src.data.frequencies()[k];
        let m = match kind {
            Network::S => Some(src.data.at(k).to_vec()),
            Network::Z => sm.impedance(&z0),
            Network::Y => sm.admittance(&z0),
            Network::Abcd => sm.chain([z0[0], z0[1]]).map(|a| a.to_vec()),
        };
        let m = m.ok_or_else(|| {
            EngineError::failed(format!(
                "{} has no {} matrix at {} {}",
                if n == 1 { "the port" } else { "the network" },
                kind.id(),
                s.show_frequency(f),
                s.display.frequency.symbol()
            ))
        })?;
        matrices.push(matrix_json(&m, if kind == Network::Abcd { 2 } else { n }));
        let one = C::new(1.0, 0.0);
        zin.push(Value::Array(
            (0..n)
                .map(|i| {
                    let g = sm[(i, i)];
                    let z = (one + g) / (one - g) * z0[i];
                    json!([z.re, z.im])
                })
                .collect(),
        ));
        freqs.push(s.show_frequency(f));
    }
    let unit = match kind {
        Network::Z => "ohm",
        Network::Y => "siemens",
        Network::Abcd => "A, D: none; B: ohm; C: siemens",
        Network::S => "none",
    };
    Ok(json!({
        "kind": kind.id(),
        "unit": if normalized && kind != Network::S { "normalized" } else { unit },
        "ports": src.names,
        "references": z0,
        "normalized": normalized,
        "frequencies": freqs,
        "matrices": matrices,
        "input_impedance": zin,
    }))
}

/// The pairs of ports (0-based) `pairs` gives (1-based, `[[1, 2], [3, 4]]`), else ports taken
/// two by two in order (1 with 2, 3 with 4, …: an even number of them).
pub fn pairs_of(p: &Params, n: usize) -> Result<Vec<(usize, usize)>> {
    let Some(list) = p.list("pairs", MAX_ENTRIES).optional()? else {
        return default_pairs(n).ok_or_else(|| p.error("pairs", format!("{n} ports do not pair in order: give `pairs`, such as [[1, 3], [2, 4]]")));
    };
    let mut out = Vec::new();
    for v in list {
        let pair = v.as_array().filter(|a| a.len() == 2).and_then(|a| Some((a[0].as_u64()?, a[1].as_u64()?)));
        let (a, b) =
            pair.filter(|(a, b)| *a >= 1 && *b >= 1).ok_or_else(|| p.error("pairs", "each pair is two port numbers from 1, [positive, negative]"))?;
        out.push((a as usize - 1, b as usize - 1));
    }
    emcraft_solver::mixed_order(n, &out)
        .ok_or_else(|| p.error("pairs", format!("each of the {n} ports in one pair at most, its two ports different")))?;
    Ok(out)
}

/// Ports two by two in order (1 with 2, 3 with 4, …); none for an odd number or fewer than two.
pub fn default_pairs(n: usize) -> Option<Vec<(usize, usize)>> {
    (n >= 2 && n.is_multiple_of(2)).then(|| (0..n / 2).map(|k| (2 * k, 2 * k + 1)).collect())
}

/// The names of the mixed-mode waves: `d1` (pair 1's differential), `c1` (its common), `5`
/// (port 5, unpaired).
pub fn mixed_names(n: usize, pairs: &[(usize, usize)]) -> Vec<String> {
    emcraft_solver::mixed_order(n, pairs)
        .unwrap_or_default()
        .into_iter()
        .map(|w| match w {
            MixedWave::Differential(k) => format!("d{}", k + 1),
            MixedWave::Common(k) => format!("c{}", k + 1),
            MixedWave::Single(p) => (p + 1).to_string(),
        })
        .collect()
}

/// The mixed-mode S of `data` at each frequency (none when a pair does not fit or not every
/// port was excited), in [`mixed_names`]' order.
pub fn mixed_data(data: &SData, pairs: &[(usize, usize)]) -> Option<SData> {
    let n = data.outputs();
    if data.inputs().len() != n {
        return None;
    }
    let mut values = Vec::with_capacity(data.values().len());
    for k in 0..data.frequencies().len() {
        let sm = SMatrix::from_fn(n, |i, j| data.at(k)[i * n + j]);
        values.extend(sm.mixed_mode(pairs)?.rows().flatten().copied());
    }
    SData::square(data.frequencies().to_vec(), n, values).ok()
}

/// `results.mixed_mode`: mixed-mode S at each frequency, its waves named.
pub fn mixed_mode(s: &mut Session, p: &Params) -> Result<Value> {
    let src = source(s, p)?;
    let n = src.data.outputs();
    if src.data.inputs().len() != n {
        return Err(EngineError::failed("not every port was excited: no mixed-mode S"));
    }
    let pairs = pairs_of(p, n)?;
    let mixed = mixed_data(&src.data, &pairs).ok_or_else(|| EngineError::failed("no mixed-mode S of these pairs"))?;
    let ks = frequencies_of(s, p, &mixed)?;
    Ok(json!({
        "waves": mixed_names(n, &pairs),
        "pairs": pairs.iter().map(|(a, b)| [a + 1, b + 1]).collect::<Vec<_>>(),
        "ports": src.names,
        "frequencies": ks.iter().map(|k| s.show_frequency(mixed.frequencies()[*k])).collect::<Vec<_>>(),
        "matrices": ks.iter().map(|k| matrix_json(mixed.at(*k), n)).collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    fn c(re: f64, im: f64) -> C {
        C::new(re, im)
    }

    #[test]
    fn a_four_port_reads_row_by_row_over_continued_lines() {
        // Version 1, four ports: a row of the matrix per line here, any split allowed.
        let mut text = String::from("# Hz S RI R 75\n");
        for f in [1e9, 2e9] {
            let _ = write!(text, "{f}");
            for i in 0..4 {
                for j in 0..4 {
                    let _ = write!(text, " {} {}", i as f64 * 0.1, j as f64 * 0.01);
                }
                text.push('\n');
            }
        }
        let t = parse_touchstone(&text, Some(4)).unwrap();
        assert_eq!(t.references, vec![75.0; 4]);
        assert_eq!(t.data.get(1, 2, 3), Some(c(0.2, 0.03)));
    }

    #[test]
    fn version_2_keywords_set_the_ports_order_and_references() {
        let text = "[Version] 2.0\n# GHz S RI\n[Number of Ports] 2\n[Two-Port Data Order] 12_21\n[Reference] 50 25\n[Number of Frequencies] 1\n[Network Data]\n1 0.1 0 0.2 0 0.3 0 0.4 0\n[End]\n";
        let t = parse_touchstone(text, None).unwrap();
        assert_eq!(t.references, vec![50.0, 25.0]);
        // 12_21: S11 S12 S21 S22 in the file.
        assert_eq!(t.data.get(0, 0, 1), Some(c(0.2, 0.0)));
        assert_eq!(t.data.get(0, 1, 0), Some(c(0.3, 0.0)));
    }

    #[test]
    fn a_two_ports_noise_data_ends_its_s() {
        let text = "# GHz S MA R 50\n1 0.1 0 0.9 -90 0.9 -90 0.1 0\n2 0.1 0 0.9 -180 0.9 -180 0.1 0\n1 1.2 0.3 45 0.5\n2 1.4 0.3 60 0.6\n";
        assert_eq!(parse_touchstone(text, Some(2)).unwrap().data.frequencies().len(), 2);
    }

    #[test]
    fn noise_data_reads_the_same_from_either_version() {
        // The Touchstone 2.0 specification's examples 10 and 11: R_n normalised (0.38 × 50 Ω) in
        // version 1, in ohms (19 Ω) in version 2; Γ_opt on the option line's 50 Ω either way.
        let v1 = "# \n2 .95 -26 3.57 157 .04 76 .66 -14\n22 .60 -144 1.30 40 .14 40 .56 -85\n4 .7 .64 69 .38\n18 2.7 .46 -33 .40\n";
        let v2 = "[Version] 2.0\n#\n[Number of Ports] 2\n[Reference] 50 25.0\n[Network Data]\n2 .95 -26 3.57 157 .04 76 .66 -14\n22 .60 -144 1.30 40 .14 40 .56 -85\n[Noise Data]\n4 .7 .64 69 19\n18 2.7 .46 -33 20\n[End]\n";
        let (a, b) = (parse_touchstone(v1, Some(2)).unwrap(), parse_touchstone(v2, None).unwrap());
        assert_eq!(a.data.frequencies().len(), 2);
        assert_eq!(a.noise.len(), 2);
        for (x, y) in a.noise.iter().zip(&b.noise) {
            assert_eq!(x.frequency, y.frequency);
            let (p, q) = (x.noise.parameters().unwrap(), y.noise.parameters().unwrap());
            assert!((p.f_min - q.f_min).abs() < 1e-12 && (p.y_opt - q.y_opt).norm() < 1e-12 && (p.rn - q.rn).abs() < 1e-9, "{p:?} {q:?}");
        }
        let p = a.noise[0].noise.parameters().unwrap();
        assert!((10.0 * p.f_min.log10() - 0.7).abs() < 1e-12 && (p.rn - 19.0).abs() < 1e-9);
        let gamma = C::from_polar(0.64, 69f64.to_radians());
        assert!((p.y_opt - (C::new(1.0, 0.0) - gamma) / ((C::new(1.0, 0.0) + gamma) * 50.0)).norm() < 1e-15);
        // Half a record, |Γ_opt| ≥ 1, noise on a one-port: refused.
        assert!(parse_touchstone("# GHz S MA R 50\n2 .9 0 1 0 0 0 .9 0\n1 .5 .3\n", Some(2)).is_err());
        assert!(parse_touchstone("# GHz S MA R 50\n2 .9 0 1 0 0 0 .9 0\n1 .5 1 0 .2\n", Some(2)).is_err());
        assert!(
            parse_touchstone("[Version] 2.0\n# GHz S MA\n[Number of Ports] 1\n[Network Data]\n1 .5 0\n[Noise Data]\n1 .5 .1 0 10\n", None).is_err()
        );
    }

    #[test]
    fn z_written_normalised_comes_back_as_its_s() {
        // A one-port load Z = 2 (normalised to R = 50, so 100 Ω): Γ = (100 − 50)/(100 + 50).
        let t = parse_touchstone("# GHz Z RI R 50\n1 2 0\n", Some(1)).unwrap();
        assert!((t.data.get(0, 0, 0).unwrap() - c(1.0 / 3.0, 0.0)).norm() < 1e-12);
        assert_eq!(t.parameter, 'Z');
    }

    #[rstest::rstest]
    #[case::no_ports("# GHz S RI\n1 0 0\n", None)]
    #[case::hybrid("# GHz H RI\n1 0 0 0 0 0 0 0 0\n", Some(2))]
    #[case::frequencies_back("# GHz S RI\n2 0 0\n1 0 0\n", Some(1))]
    #[case::not_finite("# GHz S RI\n1 inf 0\n", Some(1))]
    #[case::negative_reference("# GHz S RI R -5\n1 0 0\n", Some(1))]
    #[case::empty("# GHz S RI\n", Some(1))]
    #[case::lower_matrix("[Version] 2.0\n[Number of Ports] 2\n[Matrix Format] Lower\n", None)]
    fn hostile_files_are_refused(#[case] text: &str, #[case] ports: Option<usize>) {
        assert!(parse_touchstone(text, ports).is_err());
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig { cases: 256, rng_seed: proptest::test_runner::RngSeed::Fixed(11), failure_persistence: None, ..Default::default() })]
        #[test]
        fn any_text_reads_or_is_refused_without_a_panic(text in "[#!\\[\\]a-zA-Z0-9 .eE+\\-\n_]{0,300}", ports in proptest::option::of(1usize..5)) {
            let _ = parse_touchstone(&text, ports);
        }
    }

    #[test]
    fn interpolation_is_linear_between_two_frequencies_and_none_outside() {
        let d = SData::square(vec![1e9, 2e9], 1, vec![c(0.0, 0.0), c(1.0, -1.0)]).unwrap();
        assert_eq!(interpolate(&d, 0, 0, 1.5e9), Some(c(0.5, -0.5)));
        assert_eq!(interpolate(&d, 0, 0, 2e9), Some(c(1.0, -1.0)));
        assert_eq!(interpolate(&d, 0, 0, 3e9), None);
        assert_eq!(interpolate(&d, 0, 0, 0.5e9), None);
    }
}

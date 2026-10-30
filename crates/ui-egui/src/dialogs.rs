//! Command dialogs, generated from the commands' parameter specs ([`emcraft_engine::ParamSpec`]):
//! a number field per number (with the display unit of its quantity), a choice for strings with
//! known values, a check box per boolean, coordinate fields for points and directions, a table for
//! lists of records (a waveguide's sections, layers and strips), a list field for lists of
//! numbers, and a JSON field for anything else. OK builds the command's JSON parameters and runs
//! the command; a field that does not parse is marked and nothing runs. Any number may instead be
//! an expression of the project's variables, starting with `=` (`=w/2`): it goes to the engine
//! as written, which evaluates it and keeps it in the history.
//!
//! Dialogs are only a way to write parameters: programmatic calls (`engine.execute`, scripts)
//! never open them.

use emcraft_engine::text::clip;
use emcraft_engine::{CommandSpec, ParamSpec, Quantity, Session};
use serde_json::{Map, Value, json};

use crate::lang::{args, tr, trf};

/// Longest text a field keeps (characters): hostile pastes are cut, not stored.
pub const MAX_FIELD: usize = 100_000;
/// Most rows in a table field.
pub const MAX_ROWS: usize = 1000;

/// What a column of a table field holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cell {
    Number,
    /// One of the model's materials.
    Material,
}

/// A table column: its key, what it holds and whether a row needs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    pub name: &'static str,
    pub cell: Cell,
    pub required: bool,
}

const fn col(name: &'static str, cell: Cell, required: bool) -> Column {
    Column { name, cell, required }
}

/// How a parameter is edited.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Number,
    Integer,
    Text,
    /// One of these values (or none, when the parameter is optional).
    Choice(Vec<String>),
    Bool,
    /// A fixed number of numbers: a point, a direction, a port's rectangle.
    Vector(&'static [&'static str]),
    /// A list of numbers (frequencies, angles), separated by commas or spaces.
    Numbers,
    Table(&'static [Column]),
    Json,
}

const SECTIONS: &[Column] = &[
    col("length", Cell::Number, true),
    col("material", Cell::Material, true),
    col("width", Cell::Number, false),
    col("offset", Cell::Number, false),
    col("height", Cell::Number, false),
    col("offset_y", Cell::Number, false),
];
const LAYERS: &[Column] = &[col("thickness", Cell::Number, true), col("material", Cell::Material, true)];
const STRIPS: &[Column] = &[
    col("y", Cell::Number, true),
    col("x0", Cell::Number, true),
    col("width", Cell::Number, true),
    col("z0", Cell::Number, false),
    col("z1", Cell::Number, false),
];
const VIAS: &[Column] = &[
    col("x0", Cell::Number, true),
    col("z0", Cell::Number, true),
    col("wx", Cell::Number, true),
    col("wz", Cell::Number, true),
    col("y0", Cell::Number, false),
    col("y1", Cell::Number, false),
];
const XYZ: &[&str] = &["x", "y", "z"];
const FACE: &[&str] = &["z", "x0", "y0", "a", "b"];

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

/// The editor of parameter `p` of command `command`.
pub fn kind_of(command: &str, p: &ParamSpec, session: &Session) -> Kind {
    if !p.choices.is_empty() {
        return Kind::Choice(strings(p.choices));
    }
    match (command, p.name) {
        (_, "sections") => return Kind::Table(SECTIONS),
        (_, "layers") => return Kind::Table(LAYERS),
        (_, "strips") => return Kind::Table(STRIPS),
        (_, "vias") => return Kind::Table(VIAS),
        ("file.sample", "name") => return Kind::Choice(emcraft_engine::sample::SAMPLES.iter().map(|s| s.name.to_owned()).collect()),
        // A body may be metal besides the materials.
        ("geometry.body" | "geometry.body.material", "material") => {
            let metal = std::iter::once(emcraft_engine::bodies::METAL.to_owned());
            return Kind::Choice(metal.chain(session.model().materials.iter().map(|m| m.name.clone())).collect());
        }
        ("geometry.body.material", "body") => {
            return Kind::Choice(emcraft_engine::bodies::bodies(session.model()).into_iter().map(|b| b.name).collect());
        }
        (_, "material") => return Kind::Choice(session.model().materials.iter().map(|m| m.name.clone()).collect()),
        (_, "excitation") => return Kind::Choice(session.model().ports.iter().map(|p| p.name.clone()).collect()),
        ("study.sweep", "variable") | ("project.variable.delete", "name") => return Kind::Choice(session.variables().keys().cloned().collect()),
        ("results.probe", "at") => return Kind::Json,
        (_, "size" | "at" | "direction" | "polarization" | "from" | "to") if p.kind == emcraft_engine::Kind::Array => return Kind::Vector(XYZ),
        (_, "face") => return Kind::Vector(FACE),
        (_, "frequencies") => return Kind::Numbers,
        // A pattern's polar angles are a list; a plane wave's incidence one number.
        (_, "theta") if p.kind == emcraft_engine::Kind::Array => return Kind::Numbers,
        _ => {}
    }
    match p.kind {
        emcraft_engine::Kind::Number => Kind::Number,
        emcraft_engine::Kind::Integer => Kind::Integer,
        emcraft_engine::Kind::Boolean => Kind::Bool,
        emcraft_engine::Kind::String => Kind::Text,
        emcraft_engine::Kind::Array | emcraft_engine::Kind::Object => Kind::Json,
    }
}

/// A parameter's editor and what it holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: &'static str,
    pub doc: &'static str,
    pub required: bool,
    pub quantity: Quantity,
    pub kind: Kind,
    /// The text of a number, text, choice, list or JSON field.
    pub text: String,
    pub on: bool,
    /// A vector's components.
    pub parts: Vec<String>,
    /// A table's cells, row by row.
    pub rows: Vec<Vec<String>>,
    /// Why the field does not parse (shown under it).
    pub error: Option<String>,
}

impl Field {
    /// Parameter `p`'s empty field, edited as `kind`.
    fn of(p: &ParamSpec, kind: Kind) -> Field {
        Field {
            name: p.name,
            doc: p.doc,
            required: p.required,
            quantity: p.quantity,
            kind,
            text: String::new(),
            on: false,
            parts: Vec::new(),
            rows: Vec::new(),
            error: None,
        }
    }

    /// Fills the field with an initial value (JSON, display units); what does not fit its kind
    /// leaves it empty.
    fn load(&mut self, v: &Value) {
        match &self.kind {
            Kind::Bool => self.on = v.as_bool().unwrap_or(false),
            Kind::Vector(names) => {
                self.parts = (0..names.len()).map(|i| v.get(i).map(value_text).unwrap_or_default()).collect();
            }
            Kind::Table(cols) => {
                self.rows = v.as_array().map(|rows| rows.iter().take(MAX_ROWS).map(|r| table_row(cols, r)).collect()).unwrap_or_default();
            }
            Kind::Numbers => self.text = v.as_array().map(|a| list_text(a)).unwrap_or_default(),
            _ => self.text = bounded(&value_text(v)),
        }
    }
}

/// A table row's cells from a JSON record, column by column (missing ones empty).
fn table_row(cols: &[Column], record: &Value) -> Vec<String> {
    cols.iter().map(|c| record.get(c.name).map(value_text).unwrap_or_default()).collect()
}

/// A list of numbers as a list field's text.
fn list_text(list: &[Value]) -> String {
    list.iter().map(value_text).collect::<Vec<_>>().join(", ")
}

/// An open dialog.
#[derive(Clone)]
pub struct Dialog {
    pub command: &'static CommandSpec,
    pub fields: Vec<Field>,
    /// The engine's refusal of the last OK, or the first field error.
    pub error: Option<String>,
    pub docked: bool,
}

/// `text` cut to [`MAX_FIELD`] characters.
pub fn bounded(text: &str) -> String {
    clip(text, MAX_FIELD)
}

fn number_text(v: f64) -> String {
    // Shortest round-trip form, without a trailing ".0".
    let s = format!("{v}");
    s.strip_suffix(".0").map(str::to_owned).unwrap_or(s)
}

/// A JSON value as a field's text.
fn value_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map(number_text).unwrap_or_else(|| n.to_string()),
        other => other.to_string(),
    }
}

impl Dialog {
    /// The dialog of `command`, filled with `initial` (JSON parameters) where it has them.
    pub fn new(command: &'static CommandSpec, session: &Session, initial: &Value, docked: bool) -> Dialog {
        let fields = command
            .params
            .iter()
            .map(|p| {
                let mut f = Field::of(p, kind_of(command.id, p, session));
                f.load(initial.get(p.name).unwrap_or(&Value::Null));
                f
            })
            .collect();
        Dialog { command, fields, error: None, docked }
    }

    pub fn field_mut(&mut self, name: &str) -> Option<&mut Field> {
        self.fields.iter_mut().find(|f| f.name == name)
    }

    /// The engine's refusal of OK, shown where it belongs: under the field it names (engine
    /// errors quote a parameter as `` `name` ``, or a table's column inside a row), else in the
    /// band under the fields.
    pub fn refused(&mut self, error: &str) {
        let quoted: Vec<&str> = error.split('`').skip(1).step_by(2).take(8).collect();
        let named = quoted.iter().find_map(|q| {
            self.fields
                .iter()
                .position(|f| f.name == *q)
                .or_else(|| self.fields.iter().position(|f| matches!(&f.kind, Kind::Table(cols) if cols.iter().any(|c| c.name == *q))))
        });
        match named.and_then(|i| self.fields.get_mut(i)) {
            Some(f) => {
                f.error = Some(clip(error, 400));
                self.error = None;
            }
            None => self.error = Some(error.to_owned()),
        }
    }

    /// Sets a field from a JSON value, as typing it would (the control channel's `ui.dialog`).
    pub fn set(&mut self, name: &str, v: &Value) -> Result<(), String> {
        let f = self.field_mut(name).ok_or_else(|| format!("the dialog has no field `{}`", clip(name, 64)))?;
        match f.kind.clone() {
            Kind::Bool => f.on = v.as_bool().ok_or_else(|| format!("`{name}` takes true or false"))?,
            Kind::Vector(names) => {
                let a = v.as_array().filter(|a| a.len() == names.len()).ok_or_else(|| format!("`{name}` takes {} numbers", names.len()))?;
                f.parts = a.iter().map(|x| bounded(&value_text(x))).collect();
            }
            Kind::Table(cols) => {
                let rows = v.as_array().filter(|a| a.len() <= MAX_ROWS).ok_or_else(|| format!("`{name}` takes a list of at most {MAX_ROWS} rows"))?;
                f.rows = rows.iter().map(|r| table_row(cols, r).iter().map(|c| bounded(c)).collect()).collect();
            }
            Kind::Numbers => {
                f.text = match v {
                    Value::Array(a) => bounded(&list_text(a)),
                    other => bounded(&value_text(other)),
                }
            }
            _ => f.text = bounded(&value_text(v)),
        }
        Ok(())
    }

    /// The command's parameters, or what does not parse. Marks each field's error.
    pub fn params(&mut self) -> Result<Value, String> {
        let mut out = Map::new();
        let mut first: Option<String> = None;
        for f in &mut self.fields {
            f.error = None;
            match field_value(f) {
                Ok(Some(v)) => {
                    out.insert(f.name.to_owned(), v);
                }
                Ok(None) if f.required => f.error = Some(tr("required").into()),
                Ok(None) => {}
                Err(e) => f.error = Some(e),
            }
            if let (None, Some(e)) = (&first, &f.error) {
                first = Some(format!("{}: {e}", f.name));
            }
        }
        match first {
            Some(e) => Err(e),
            None => Ok(Value::Object(out)),
        }
    }
}

/// A number field's value: a finite number, or an expression of the variables (`=w/2`) passed
/// on as written for the engine to evaluate.
pub fn numeric(text: &str) -> Result<Value, String> {
    let t = text.trim();
    if t.starts_with('=') {
        if t.len() > emcraft_engine::expr::MAX_LEN + 1 {
            return Err(trf("an expression holds at most {n} characters", args!(n = emcraft_engine::expr::MAX_LEN)));
        }
        return Ok(json!(t));
    }
    parse_number(t).map(number_value)
}

/// A finite number from a field's text.
pub fn parse_number(text: &str) -> Result<f64, String> {
    let t = text.trim();
    let v: f64 = t.parse().map_err(|_| trf("`{text}` is not a number", args!(text = clip(t, 40))))?;
    if v.is_finite() { Ok(v) } else { Err(tr("must be a finite number").into()) }
}

fn number_value(v: f64) -> Value {
    serde_json::Number::from_f64(v).map(Value::Number).unwrap_or(Value::Null)
}

/// The JSON value of a field: `None` when it is left empty.
fn field_value(f: &Field) -> Result<Option<Value>, String> {
    let empty = f.text.trim().is_empty();
    Ok(Some(match &f.kind {
        Kind::Number => {
            if empty {
                return Ok(None);
            }
            numeric(&f.text)?
        }
        Kind::Integer => {
            if empty {
                return Ok(None);
            }
            let t = f.text.trim();
            if t.starts_with('=') {
                return numeric(t).map(Some);
            }
            json!(t.parse::<i64>().map_err(|_| trf("`{text}` is not a whole number", args!(text = clip(t, 40))))?)
        }
        Kind::Text | Kind::Choice(_) => {
            if empty {
                return Ok(None);
            }
            json!(f.text.trim())
        }
        Kind::Bool => json!(f.on),
        Kind::Vector(_) => {
            if f.parts.iter().all(|p| p.trim().is_empty()) {
                return Ok(None);
            }
            let v = f.parts.iter().map(|p| numeric(p)).collect::<Result<Vec<_>, _>>()?;
            Value::Array(v)
        }
        Kind::Numbers => {
            if empty {
                return Ok(None);
            }
            let list = f.text.split([',', ' ', ';']).filter(|s| !s.trim().is_empty()).take(100_000);
            Value::Array(list.map(numeric).collect::<Result<Vec<_>, _>>()?)
        }
        Kind::Table(cols) => {
            if f.rows.is_empty() {
                return Ok(None);
            }
            let mut rows = Vec::with_capacity(f.rows.len());
            for (i, r) in f.rows.iter().enumerate() {
                let mut o = Map::new();
                for (c, cell) in cols.iter().zip(r) {
                    let cell = cell.trim();
                    if cell.is_empty() {
                        if c.required {
                            return Err(trf("row {n}: `{column}` is required", args!(n = (i + 1), column = c.name)));
                        }
                        continue;
                    }
                    let v = match c.cell {
                        Cell::Number => {
                            numeric(cell).map_err(|e| trf("row {n}, {column}: {error}", args!(n = (i + 1), column = c.name, error = e)))?
                        }
                        Cell::Material => json!(cell),
                    };
                    o.insert(c.name.to_owned(), v);
                }
                rows.push(Value::Object(o));
            }
            Value::Array(rows)
        }
        Kind::Json => {
            if empty {
                return Ok(None);
            }
            serde_json::from_str(f.text.trim()).map_err(|e| trf("not JSON: {error}", args!(error = e)))?
        }
    }))
}

/// What a dialog opens on: the model's own values where it has them (the waveguide being
/// edited, the next port's name), otherwise defaults that make a working example. Lengths and
/// frequencies are in the session's display units.
pub fn initial(command: &str, s: &Session) -> Value {
    let l = s.display().length;
    let ports = s.model().ports.len();
    let fu = s.display().frequency;
    // The job's band, and the frequency in its middle.
    let band = s.model().band.map(|b| (fu.from_si(b.start), fu.from_si(b.stop)));
    let middle = band.map_or(10.0, |(a, z)| 0.5 * (a + z));
    match command {
        // The model: its own guide or the next names, else a working example.
        "geometry.waveguide" => waveguide(s),
        "model.material.create" => json!({"name": format!("material{}", s.model().materials.len()), "eps_r": 2.2}),
        "geometry.solid" => json!({"shape": "box", "size": [22.86, 10.16, 30]}),
        "geometry.inclusion" => json!({"shape": "sphere", "radius": 2, "at": [11.43, 5.08, 15]}),
        "geometry.feature" => json!({"combine": "subtract", "shape": "cylinder", "radius": 1.5, "length": 10.16, "at": [11.43, 0, 15], "axis": "y"}),
        "geometry.import" => json!({"angle": 30, "open": false}),
        "geometry.body" => json!({"material": "metal", "shape": "cylinder", "radius": 1.5, "length": 10.16, "at": [11.43, 0, 15], "axis": "y"}),
        "model.walls" => json!({"conductivity": 5.8e7}),
        "model.symmetry" => json!({"axis": "x", "kind": "magnetic"}),
        "model.periodic" => periodic(s),
        "model.port.create" => json!({"name": format!("{}", ports + 1), "end": if ports == 0 { "start" } else { "end" }, "modes": 4}),
        "model.port.lumped" => json!({"name": format!("L{}", s.model().lumped.len() + 1), "from": [0, 0, -0.5], "to": [0, 0, 0.5], "impedance": 50}),
        "source.dipole" => json!({"at": [0, 0, 0], "direction": [0, 0, 1], "length": 1, "current": 1}),
        "source.plane_wave" => json!({"direction": [0, 0, 1], "polarization": [1, 0, 0]}),
        "model.open" => json!({"thickness": 10}),
        // The mesh: with a band, it follows it (elements per wavelength at its top).
        "mesh.generate" if band.is_some() => json!({"per_wavelength": 10, "curved": true}),
        "mesh.generate" => json!({"size": 3, "curved": true}),
        "mesh.adapt" if band.is_some() => json!({"per_wavelength": 6, "passes": 6, "tolerance": 0.02, "fraction": 0.3}),
        "mesh.adapt" => json!({"size": 6, "passes": 6, "tolerance": 0.02, "fraction": 0.3}),
        // The solves: over the band, or at its middle or top.
        "solve.frequency" => match band {
            Some((a, z)) => json!({"start": a, "stop": z, "points": 11, "order": 2}),
            None => json!({"start": 8, "stop": 12, "points": 11, "order": 2}),
        },
        "solve.eigenmodes" => json!({"near": middle, "count": 5, "order": 2}),
        "solve.radiation" => json!({"frequency": middle, "order": 2}),
        "solve.time_domain" => {
            let (a, z) = band.unwrap_or((8.0, 12.0));
            json!({"start": a, "stop": z, "points": 41, "cell": 1})
        }
        "solve.port_modes" => json!({"frequency": band.map_or(10.0, |(_, z)| z), "order": 2}),
        "solve.dispersion" => json!({"from": 0, "to": 180, "points": 11, "bands": 4, "near": band.map_or(15.0, |(_, z)| z), "order": 2}),
        "solve.device" => json!({"device": s.compute().device(), "precision": s.compute().precision.id()}),
        "project.frequency" => match band {
            Some((a, z)) => json!({"start": a, "stop": z}),
            None => json!({"start": 8, "stop": 12}),
        },
        // Results and pictures.
        "results.touchstone" => json!({"path": format!("emcraft.s{}p", ports.max(1)), "format": "ri"}),
        "results.compare" => json!({"modes": 160}),
        // The highest solved frequency of the sweep, as the field view opens on.
        "view.animation" => {
            let f = s.results().sweep.as_ref().and_then(|sw| sw.results.last()).map(|r| fu.from_si(r.frequency));
            json!({"path": "field-animation.png", "frames": 24, "frequency": f})
        }
        "view.dispersion" => json!({"path": "dispersion.png"}),
        "view.study" => json!({"path": "study.png"}),
        // The project.
        "file.save" | "file.open" => json!({"path": "project.emcraft"}),
        "file.sample" => json!({"name": "highpass", "solve": true}),
        "units.set" => json!({"length": l.symbol(), "frequency": fu.symbol()}),
        "project.variable" => {
            let name = ["l", "w", "h", "d", "er"].into_iter().map(str::to_owned).find(|n| !s.variables().contains_key(n));
            json!({"name": name.unwrap_or_else(|| format!("v{}", s.variables().len() + 1)), "value": 10})
        }
        "project.variable.delete" => json!({"name": s.variables().keys().next()}),
        "study.sweep" | "study.optimize" => study(command, s, band),
        _ => json!({}),
    }
}

/// The model's waveguide as `geometry.waveguide`'s parameters, or a WR-90 section.
fn waveguide(s: &Session) -> Value {
    let Some(w) = s.model().waveguide.as_ref() else {
        return json!({"width": 22.86, "height": 10.16, "sections": [{"length": 20, "material": "vacuum"}]});
    };
    let l = s.display().length;
    let opt = |v: Option<f64>| v.map(|x| json!(l.from_si(x)));
    let sections: Vec<Value> = w
        .sections
        .iter()
        .map(|sec| {
            let mut o = json!({"length": l.from_si(sec.length), "material": sec.material});
            for (k, v) in [("width", opt(sec.width)), ("offset", opt(sec.offset)), ("height", opt(sec.height)), ("offset_y", opt(sec.offset_y))] {
                if let Some(v) = v {
                    o[k] = v;
                }
            }
            o
        })
        .collect();
    let layers: Vec<Value> = w.layers.iter().map(|x| json!({"thickness": l.from_si(x.thickness), "material": x.material})).collect();
    let strips: Vec<Value> = w
        .strips
        .iter()
        .map(|x| json!({"y": l.from_si(x.y), "x0": l.from_si(x.x0), "width": l.from_si(x.width), "z0": l.from_si(x.z0), "z1": l.from_si(x.z1)}))
        .collect();
    let vias: Vec<Value> = w
        .vias
        .iter()
        .map(|v| json!({"x0": l.from_si(v.x0), "z0": l.from_si(v.z0), "wx": l.from_si(v.wx), "wz": l.from_si(v.wz), "y0": l.from_si(v.y0), "y1": l.from_si(v.y1)}))
        .collect();
    json!({"width": l.from_si(w.width), "height": l.from_si(w.height), "sections": sections, "layers": layers, "strips": strips, "vias": vias})
}

/// The model's periodic boundaries as `model.periodic`'s parameters, or a cell tied across x
/// and y at normal incidence.
fn periodic(s: &Session) -> Value {
    match &s.model().periodic {
        Some(p) => json!({"x": p.x, "y": p.y, "z": p.z, "theta": p.theta.to_degrees(), "phi": p.phi.to_degrees(),
                          "polarization": if p.polarization == emcraft_model::Polarization::Te { "te" } else { "tm" }}),
        None => json!({"x": true, "y": true, "z": false, "theta": 0, "phi": 0, "polarization": "te"}),
    }
}

/// A study of the first variable around its value: the solve the model calls for (radiation
/// for an open problem, eigenmodes without ports, else a sweep over the band) and, for an
/// optimization, the goal that goes with it.
fn study(command: &str, s: &Session, band: Option<(f64, f64)>) -> Value {
    let (name, v) = s.variables().iter().next().map_or((None, 1.0), |(n, v)| (Some(n.clone()), *v));
    let spread = |k: f64| if v == 0.0 { k } else { v * (1.0 + k) };
    let (solve, goal) = if s.model().open.is_some() {
        (json!({"command": "solve.radiation", "params": initial("solve.radiation", s)}), json!({"type": "maximize", "quantity": "radiated_power"}))
    } else if s.model().ports.is_empty() {
        let near = band.map_or(10.0, |(a, z)| 0.5 * (a + z));
        (
            json!({"command": "solve.eigenmodes", "params": {"near": near, "count": 3, "order": 2}}),
            json!({"type": "eigenfrequency", "mode": 1, "target": near}),
        )
    } else {
        let (a, z) = band.unwrap_or((8.0, 12.0));
        (
            json!({"command": "solve.frequency", "params": {"start": a, "stop": z, "points": 11, "order": 2}}),
            json!({"type": "min_max_db", "parameter": "s11"}),
        )
    };
    if command == "study.sweep" {
        json!({"variable": name, "start": spread(-0.2), "stop": spread(0.2), "points": 5, "solve": solve})
    } else {
        json!({"variables": [{"name": name, "min": spread(-0.5), "max": spread(0.5)}], "goal": goal, "solve": solve, "max_evaluations": 40})
    }
}

/// The unit a field's value is read in.
pub fn unit(q: Quantity, s: &Session) -> &'static str {
    match q {
        Quantity::None => "",
        Quantity::Length => s.display().length.symbol(),
        Quantity::Frequency => s.display().frequency.symbol(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str) -> &'static CommandSpec {
        emcraft_engine::find_command(id).unwrap()
    }

    /// Every command's dialog opens on its initial values and gives back parameters the engine
    /// takes (or, for those that need a model, refuses with an error, never a panic).
    #[test]
    fn every_command_has_a_dialog_whose_parameters_the_engine_reads() {
        let mut s = Session::new();
        s.execute("file.sample", &json!({"name": "highpass"})).unwrap();
        for c in emcraft_engine::COMMANDS {
            let mut d = Dialog::new(c, &s, &initial(c.id, &s), false);
            if let Ok(p) = d.params() {
                // Only cheap commands are run: the point is that the keys are the command's.
                // (Nor those that write files.)
                let writes = c.params.iter().any(|q| q.name == "path");
                if !crate::commands::is_long(c.id) && !c.id.starts_with("view.") && !writes {
                    let mut copy = s.clone();
                    let _ = copy.execute(c.id, &p);
                }
                for k in p.as_object().unwrap().keys() {
                    assert!(c.params.iter().any(|q| q.name == k), "{}: {k}", c.id);
                }
            }
        }
    }

    /// An engine refusal lands under the field it names, a table's column under its table, and
    /// anything else in the band.
    #[test]
    fn refusals_land_under_the_field_they_name() {
        let s = Session::new();
        let mut d = Dialog::new(spec("geometry.waveguide"), &s, &json!({}), false);
        d.refused("`width` must be > 0, got -5");
        assert_eq!(d.field_mut("width").and_then(|f| f.error.clone()).as_deref(), Some("`width` must be > 0, got -5"));
        assert_eq!(d.error, None);
        d.refused("section 2: `length` must be > 0, got -1");
        assert!(d.field_mut("sections").is_some_and(|f| f.error.is_some()));
        d.refused("unknown material `unobtainium`");
        assert_eq!(d.error.as_deref(), Some("unknown material `unobtainium`"));
        // Hostile text: unbalanced quotes, nothing named.
        d.refused("``` ` `");
        assert!(d.error.is_some());
    }

    /// The waveguide dialog round-trips the model's guide: the same parameters rebuild it.
    #[test]
    fn the_waveguide_dialog_holds_the_model_guide() {
        let mut s = Session::new();
        s.execute("file.sample", &json!({"name": "iris-bandpass"})).unwrap();
        let mut d = Dialog::new(spec("geometry.waveguide"), &s, &initial("geometry.waveguide", &s), false);
        let p = d.params().unwrap();
        let before = s.model().waveguide.clone();
        s.execute("geometry.waveguide", &p).unwrap();
        assert_eq!(s.model().waveguide, before);
    }

    /// Hostile text is refused field by field: nothing is built from it.
    #[test]
    fn fields_that_do_not_parse_are_marked() {
        let s = Session::new();
        let mut d = Dialog::new(spec("geometry.waveguide"), &s, &initial("geometry.waveguide", &s), false);
        d.field_mut("width").unwrap().text = "1e999".into();
        d.field_mut("height").unwrap().text = "abc".into();
        d.field_mut("sections").unwrap().rows[0][0] = "NaN".into();
        let e = d.params().unwrap_err();
        assert!(e.starts_with("width"), "{e}");
        assert!(d.fields.iter().filter(|f| f.error.is_some()).count() >= 3);
        d.set("width", &json!(22.86)).unwrap();
        d.set("height", &json!(10.16)).unwrap();
        d.set("sections", &json!([{"length": 5, "material": "vacuum"}])).unwrap();
        assert!(d.params().is_ok());
        assert!(d.set("nope", &json!(1)).is_err());
        assert!(d.set("sections", &json!("x")).is_err());
        let mut d = Dialog::new(spec("solve.frequency"), &s, &json!({}), false);
        d.field_mut("frequencies").unwrap().text = "8, 9 10;11".into();
        assert_eq!(d.params().unwrap()["frequencies"], json!([8.0, 9.0, 10.0, 11.0]));
        d.field_mut("points").unwrap().text = "2.5".into();
        assert!(d.params().is_err());
        d.field_mut("points").unwrap().text = "x".repeat(MAX_FIELD * 2);
        assert!(d.params().unwrap_err().len() < 200);
    }
}

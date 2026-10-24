//! The commands.

use std::sync::Arc;

use emcraft_fem::Order;
use emcraft_model::units::{Frequency, Length, decibels};
use emcraft_model::{End, Material, Port, Section, Waveguide};
use emcraft_solver::{Complex64 as C, Driven};
use serde_json::{Value, json};

use crate::params::{get, integer, number, required_number, required_string, string};
use crate::view::{self, FieldView, Plane};
use crate::{CommandSpec, EngineError, ParamSpec, Quantity, Result, Session, Sweep};

const fn p(name: &'static str, kind: &'static str, quantity: Quantity, required: bool, doc: &'static str) -> ParamSpec {
    ParamSpec { name, kind, quantity, required, doc }
}

use Quantity::{Frequency as F, Length as L, None as N};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "units.set",
        label: "Display units",
        doc: "Sets the units parameters are read in and replies written in.",
        params: &[p("length", "string", N, false, "`mm` or `m`"), p("frequency", "string", N, false, "`GHz`, `MHz` or `Hz`")],
        run: units_set,
    },
    CommandSpec {
        id: "model.material.create",
        label: "Create material",
        doc: "A linear, isotropic, lossless material.",
        params: &[
            p("name", "string", N, true, "unique name"),
            p("eps_r", "number", N, true, "relative permittivity εr > 0"),
            p("mu_r", "number", N, false, "relative permeability μr > 0 (default 1)"),
        ],
        run: material_create,
    },
    CommandSpec {
        id: "geometry.waveguide",
        label: "Rectangular waveguide",
        doc: "A rectangular waveguide along z with perfectly conducting walls: cross-section `width` (x) × `height` (y) from the origin, filled along z in sections, each with its own aperture across x (H-plane steps, inductive irises; neighbouring apertures must nest). Replaces any geometry, mesh and results.",
        params: &[
            p("width", "number", L, true, "broad wall a (x)"),
            p("height", "number", L, true, "narrow wall b (y)"),
            p(
                "sections",
                "array",
                N,
                true,
                "`[{\"length\", \"material\", \"width\"?, \"offset\"?}]` from z = 0: a section may narrow the broad wall to an aperture `width` wide (lengths), starting at x = `offset` (centred when left out)",
            ),
        ],
        run: waveguide,
    },
    CommandSpec {
        id: "mesh.generate",
        label: "Mesh",
        doc: "Meshes the waveguide with tetrahedra of about `size` on a side (six per cell of a structured grid; every section boundary and aperture wall is a grid plane), graded towards the re-entrant edges of steps and irises.",
        params: &[
            p("size", "number", L, true, "target element size"),
            p("edge", "number", N, false, "element size at step and iris edges, as a fraction of `size` (default 0.25; 1 = uniform)"),
            p("growth", "number", N, false, "element growth away from those edges (default 1.5)"),
            p(
                "size_y",
                "number",
                L,
                false,
                "element size across the narrow wall (default `size`); an H-plane device's TE10 fields do not vary along y, so one layer (the height) is exact there",
            ),
        ],
        run: mesh_generate,
    },
    CommandSpec {
        id: "model.port.create",
        label: "Create port",
        doc: "A waveguide port on an end face, matched to the guide's lowest modes. TE10 is excited and read; the other modes absorb what else reaches the port.",
        params: &[
            p("name", "string", N, true, "unique name"),
            p("end", "string", N, true, "`start` (z = 0) or `end` (z = length)"),
            p("modes", "integer", N, false, "modes in the port condition, 1 to 32 (default 4)"),
        ],
        run: port_create,
    },
    CommandSpec {
        id: "solve.frequency",
        label: "Frequency sweep",
        doc: "Solves the driven problem at each frequency, every port excited in turn by a unit TE10 wave: S-parameters and fields. Frequencies are `frequencies`, or `points` evenly from `start` to `stop`.",
        params: &[
            p("frequencies", "array", F, false, "the frequencies"),
            p("start", "number", F, false, "first frequency"),
            p("stop", "number", F, false, "last frequency"),
            p("points", "integer", N, false, "frequencies from start to stop (default 11)"),
            p("order", "integer", N, false, "element order: 1 (Whitney) or 2 (default)"),
        ],
        run: solve_frequency,
    },
    CommandSpec {
        id: "results.sparameters",
        label: "S-parameters",
        doc: "The S-parameter table of the last sweep: each S[p][q] (at port p for a wave into port q) as real, imaginary, magnitude, dB and phase (degrees).",
        params: &[],
        run: sparameters,
    },
    CommandSpec {
        id: "results.compare",
        label: "Compare with theory",
        doc: "Compares the sweep's S11 and S21 with the reference: the exact TE10 solution when every section has the full cross-section, mode matching (`modes` TE_m0 modes in the widest section) when the aperture changes. The errors per frequency and the worst.",
        params: &[p("modes", "integer", N, false, "TE_m0 modes in the widest section for mode matching, 10 to 1000 (default 160)")],
        run: compare,
    },
    CommandSpec {
        id: "results.field_error",
        label: "Field error against theory",
        doc: "The relative L2 errors ‖F − F_exact‖/‖F_exact‖ of E and H over the whole guide at a solved frequency, against the exact TE10 fields (port 1 excited).",
        params: &[p("frequency", "number", F, true, "a solved frequency")],
        run: field_error,
    },
    CommandSpec {
        id: "results.probe",
        label: "Probe fields",
        doc: "E (V/m) and H (A/m), complex, at points, for a solved frequency and an excited port.",
        params: &[
            p("frequency", "number", F, true, "a solved frequency"),
            p("at", "array", L, true, "points `[[x, y, z], …]`"),
            p("excitation", "string", N, false, "the excited port (default the first)"),
        ],
        run: probe,
    },
    CommandSpec {
        id: "view.field",
        label: "Field picture",
        doc: "Writes a PNG of the fields on a plane: a component as colour and the in-plane E and H as arrows, Re(F e^jωt) at a phase.",
        params: &[
            p("path", "string", N, true, "PNG file to write"),
            p("frequency", "number", F, true, "a solved frequency (the nearest is used)"),
            p("plane", "string", N, false, "`h` (H-plane, default), `e` (E-plane) or `cross` (cross-section)"),
            p("at", "number", L, false, "the plane's position along its normal (default the middle)"),
            p("component", "string", N, false, "colour: ex, ey (default), ez, hx, hy, hz, |e|, |h|"),
            p("arrows", "array", N, false, "vector fields drawn: `[\"e\", \"h\"]` (default both)"),
            p("phase", "number", N, false, "ωt in degrees (default 0)"),
            p("excitation", "string", N, false, "the excited port (default the first)"),
            p("width", "integer", N, false, "picture width in pixels (default 1100)"),
        ],
        run: view_field,
    },
    CommandSpec {
        id: "view.sparameters",
        label: "S-parameter chart",
        doc: "Writes a PNG of |S11| and |S21| in dB against frequency, the solved points over the exact TE10 curves, with the cut-off frequencies marked.",
        params: &[
            p("path", "string", N, true, "PNG file to write"),
            p("width", "integer", N, false, "pixels (default 1000)"),
            p("height", "integer", N, false, "pixels (default 600)"),
        ],
        run: view_sparameters,
    },
    CommandSpec {
        id: "file.sample",
        label: "Open sample",
        doc: "Replaces the session with a sample model (`highpass`: the waveguide high-pass filter), built by its script; `solve: true` also runs its sweep.",
        params: &[p("name", "string", N, true, "sample name"), p("solve", "boolean", N, false, "run the sample's sweep (default false)")],
        run: file_sample,
    },
    CommandSpec { id: "model.summary", label: "Summary", doc: "The model, mesh and results in brief.", params: &[], run: summary },
];

fn units_set(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(l) = string(p, "length")? {
        s.display.length = match l {
            "mm" => Length::Millimetre,
            "m" => Length::Metre,
            _ => return Err(EngineError::Params("`length` must be mm or m".into())),
        };
    }
    if let Some(f) = string(p, "frequency")? {
        s.display.frequency = match f {
            "GHz" => Frequency::Gigahertz,
            "MHz" => Frequency::Megahertz,
            "Hz" => Frequency::Hertz,
            _ => return Err(EngineError::Params("`frequency` must be GHz, MHz or Hz".into())),
        };
    }
    Ok(json!({"length": s.display.length.symbol(), "frequency": s.display.frequency.symbol()}))
}

fn material_create(s: &mut Session, p: &Value) -> Result<Value> {
    let name = required_string(p, "name")?;
    let eps = required_number(p, "eps_r")?;
    let mu = number(p, "mu_r")?.unwrap_or(1.0);
    s.model.add_material(Material::new(name, eps, mu)?)?;
    Ok(json!({"name": name}))
}

fn waveguide(s: &mut Session, p: &Value) -> Result<Value> {
    let width = s.positive_length(p, "width")?.ok_or_else(|| EngineError::Params("`width` is required".into()))?;
    let height = s.positive_length(p, "height")?.ok_or_else(|| EngineError::Params("`height` is required".into()))?;
    let list = get(p, "sections").and_then(Value::as_array).ok_or_else(|| EngineError::Params("`sections` must be an array".into()))?;
    if list.len() > 1000 {
        return Err(EngineError::Params("`sections`: at most 1000".into()));
    }
    let mut sections = Vec::with_capacity(list.len());
    for (i, sec) in list.iter().enumerate() {
        let length = s.positive_length(sec, "length").map_err(|e| EngineError::Params(format!("section {}: {e}", i + 1)))?;
        let length = length.ok_or_else(|| EngineError::Params(format!("section {}: `length` is required", i + 1)))?;
        let material = string(sec, "material")?.ok_or_else(|| EngineError::Params(format!("section {}: `material` is required", i + 1)))?;
        let width = s.positive_length(sec, "width").map_err(|e| EngineError::Params(format!("section {}: {e}", i + 1)))?;
        let offset = s.length(sec, "offset").map_err(|e| EngineError::Params(format!("section {}: {e}", i + 1)))?;
        sections.push(Section { length, material: material.to_owned(), width, offset });
    }
    s.model.set_waveguide(Waveguide { width, height, sections })?;
    s.sweep = None;
    Ok(json!({"length": s.show_length(s.model.waveguide.as_ref().map_or(0.0, |w| w.length()))}))
}

fn mesh_generate(s: &mut Session, p: &Value) -> Result<Value> {
    let size = s.positive_length(p, "size")?.ok_or_else(|| EngineError::Params("`size` is required".into()))?;
    let mut o = emcraft_mesh::Options::new(size);
    o.edge = number(p, "edge")?.unwrap_or(o.edge);
    o.growth = number(p, "growth")?.unwrap_or(o.growth);
    o.size_y = s.positive_length(p, "size_y")?;
    let mesh = emcraft_mesh::waveguide_with(&s.model, &o)?;
    let reply = json!({"nodes": mesh.nodes.len(), "tetrahedra": mesh.tets.len()});
    s.model.mesh = Some(mesh);
    s.sweep = None;
    Ok(reply)
}

fn port_create(s: &mut Session, p: &Value) -> Result<Value> {
    let name = required_string(p, "name")?;
    let end = match required_string(p, "end")? {
        "start" => End::Start,
        "end" => End::End,
        _ => return Err(EngineError::Params("`end` must be start or end".into())),
    };
    let modes = integer(p, "modes", 1..=emcraft_model::MAX_PORT_MODES as u64)?.unwrap_or(4) as u32;
    s.model.add_port(Port { name: name.to_owned(), end, modes })?;
    s.sweep = None;
    Ok(json!({"name": name}))
}

fn frequencies(s: &Session, p: &Value) -> Result<Vec<f64>> {
    let mut out = Vec::new();
    if let Some(list) = get(p, "frequencies") {
        let list = list.as_array().ok_or_else(|| EngineError::Params("`frequencies` must be an array".into()))?;
        if list.len() > s.limits.frequencies {
            return Err(EngineError::Params(format!("at most {} frequencies", s.limits.frequencies)));
        }
        for v in list {
            let f =
                v.as_f64().filter(|f| f.is_finite() && *f > 0.0).ok_or_else(|| EngineError::Params("`frequencies` must be numbers > 0".into()))?;
            out.push(s.display.frequency.to_si(f));
        }
    } else {
        let start = s.frequency(p, "start")?.ok_or_else(|| EngineError::Params("give `frequencies`, or `start` and `stop`".into()))?;
        let stop = s.frequency(p, "stop")?.ok_or_else(|| EngineError::Params("give `frequencies`, or `start` and `stop`".into()))?;
        let n = integer(p, "points", 1..=s.limits.frequencies as u64)?.unwrap_or(11) as usize;
        if stop < start {
            return Err(EngineError::Params("`stop` must not be below `start`".into()));
        }
        for k in 0..n {
            out.push(if n == 1 { start } else { start + (stop - start) * k as f64 / (n - 1) as f64 });
        }
    }
    if out.is_empty() {
        return Err(EngineError::Params("no frequencies".into()));
    }
    out.sort_by(f64::total_cmp);
    out.dedup();
    Ok(out)
}

fn solve_frequency(s: &mut Session, p: &Value) -> Result<Value> {
    let freqs = frequencies(s, p)?;
    let order = Order::from_u8(integer(p, "order", 1..=2)?.unwrap_or(2) as u8).unwrap_or(Order::Two);
    let driven = Driven::new(&s.model, order)?;
    let mut results = Vec::with_capacity(freqs.len());
    for f in &freqs {
        results.push(driven.solve(*f)?);
    }
    let unknowns = driven.dofs.unknowns;
    s.sweep = Some(Sweep { order, driven: Arc::new(driven), results });
    let mut reply = sparameters(s, &Value::Null)?;
    if let Some(o) = reply.as_object_mut() {
        o.insert("unknowns".into(), json!(unknowns));
        o.insert("order".into(), json!(order.as_u8()));
    }
    Ok(reply)
}

fn complex(c: C) -> Value {
    json!({"re": c.re, "im": c.im, "mag": c.norm(), "db": decibels(c.norm()), "deg": c.arg().to_degrees()})
}

fn sweep(s: &Session) -> Result<&Sweep> {
    s.sweep.as_ref().ok_or_else(|| EngineError::Failed("no results: solve first (solve.frequency)".into()))
}

fn sparameters(s: &mut Session, _: &Value) -> Result<Value> {
    let sw = sweep(s)?;
    let names: Vec<&str> = s.model.ports.iter().map(|p| p.name.as_str()).collect();
    let rows: Vec<Value> = sw
        .results
        .iter()
        .map(|r| {
            let mut m = serde_json::Map::new();
            m.insert("frequency".into(), json!(s.show_frequency(r.frequency)));
            for (i, row) in r.s.iter().enumerate() {
                for (j, v) in row.iter().enumerate() {
                    m.insert(format!("s{}{}", i + 1, j + 1), complex(*v));
                }
            }
            m.insert("backward_error".into(), json!(r.backward_error));
            Value::Object(m)
        })
        .collect();
    Ok(json!({"ports": names, "frequency_unit": s.display.frequency.symbol(), "points": rows}))
}

fn compare(s: &mut Session, p: &Value) -> Result<Value> {
    let modes = integer(p, "modes", 10..=1000)?.map_or(view::MODE_MATCHING_MODES, |n| n as usize);
    let sw = sweep(s)?;
    let reference = view::reference(s)?;
    let mut rows = Vec::new();
    let mut worst: f64 = 0.0;
    for r in &sw.results {
        let (s11, s21) = reference.s(r.frequency, modes)?;
        let (f11, f21) = (r.s[0][0], r.s.get(1).map_or(C::new(0.0, 0.0), |row| row[0]));
        let (e11, e21) = ((f11 - s11).norm(), (f21 - s21).norm());
        worst = worst.max(e11).max(e21);
        rows.push(json!({
            "frequency": s.show_frequency(r.frequency),
            "s11": complex(f11), "s21": complex(f21),
            "exact_s11": complex(s11), "exact_s21": complex(s21),
            "error_s11": e11, "error_s21": e21,
        }));
    }
    Ok(json!({"reference": reference.label(), "points": rows, "worst": worst}))
}

fn field_error(s: &mut Session, p: &Value) -> Result<Value> {
    let f = s.frequency(p, "frequency")?.ok_or_else(|| EngineError::Params("`frequency` is required".into()))?;
    let sw = sweep(s)?;
    let r = sw.at(f, false).ok_or_else(|| EngineError::Params(format!("{} {} was not solved", s.show_frequency(f), s.display.frequency.symbol())))?;
    let x = r.fields.first().ok_or_else(|| EngineError::Failed("no field".into()))?;
    let exact = view::te10_line(s)?.solve(r.frequency).map_err(|e| EngineError::Failed(e.to_string()))?;
    let (e, en) = sw.driven.l2_error(x, r.frequency, emcraft_solver::Field::E, |q| exact.e(q[0], q[1], q[2]))?;
    let (h, hn) = sw.driven.l2_error(x, r.frequency, emcraft_solver::Field::H, |q| exact.h(q[0], q[1], q[2]))?;
    Ok(json!({"frequency": s.show_frequency(r.frequency), "e": e / en, "h": h / hn}))
}

fn excitation(s: &Session, p: &Value) -> Result<usize> {
    match string(p, "excitation")? {
        None => Ok(0),
        Some(name) => Ok(s.model.port(name)?.0),
    }
}

fn probe(s: &mut Session, p: &Value) -> Result<Value> {
    let f = s.frequency(p, "frequency")?.ok_or_else(|| EngineError::Params("`frequency` is required".into()))?;
    let q = excitation(s, p)?;
    let sw = sweep(s)?;
    let r = sw.at(f, false).ok_or_else(|| EngineError::Params(format!("{} {} was not solved", s.show_frequency(f), s.display.frequency.symbol())))?;
    let x = r.fields.get(q).ok_or_else(|| EngineError::Failed("no field for that port".into()))?;
    let pts = get(p, "at").and_then(Value::as_array).ok_or_else(|| EngineError::Params("`at` must be an array of [x, y, z]".into()))?;
    if pts.len() > 100_000 {
        return Err(EngineError::Params("`at`: at most 100000 points".into()));
    }
    let mut out = Vec::with_capacity(pts.len());
    for pt in pts {
        let c = pt.as_array().filter(|a| a.len() == 3).ok_or_else(|| EngineError::Params("each point is [x, y, z]".into()))?;
        let mut xyz = [0.0; 3];
        for (d, v) in xyz.iter_mut().zip(c) {
            *d = s
                .display
                .length
                .to_si(v.as_f64().filter(|v| v.is_finite()).ok_or_else(|| EngineError::Params("coordinates must be finite numbers".into()))?);
        }
        let (e, h) = sw.driven.fields(x, r.frequency, xyz).ok_or_else(|| EngineError::Params(format!("{pt} is outside the mesh")))?;
        out.push(
            json!({"e": e.iter().map(|c| json!([c.re, c.im])).collect::<Vec<_>>(), "h": h.iter().map(|c| json!([c.re, c.im])).collect::<Vec<_>>()}),
        );
    }
    Ok(json!({"frequency": s.show_frequency(r.frequency), "points": out}))
}

fn write_png(path: &str, png: Option<Vec<u8>>) -> Result<()> {
    if path.trim().is_empty() {
        return Err(EngineError::Params("`path` must not be empty".into()));
    }
    let bytes = png.ok_or_else(|| EngineError::Failed("could not encode the picture".into()))?;
    std::fs::write(path, bytes).map_err(|e| EngineError::Failed(format!("cannot write {path}: {e}")))
}

/// A field view from command parameters.
pub fn field_view(s: &Session, p: &Value) -> Result<FieldView> {
    let f = s.frequency(p, "frequency")?.ok_or_else(|| EngineError::Params("`frequency` is required".into()))?;
    let plane = match string(p, "plane")? {
        None => Plane::H,
        Some(n) => Plane::parse(n).ok_or_else(|| EngineError::Params("`plane` must be h, e or cross".into()))?,
    };
    let mut v = FieldView::new(f, plane);
    v.at = s.length(p, "at")?;
    if let Some(c) = string(p, "component")? {
        v.component = c.to_ascii_lowercase();
    }
    if let Some(list) = get(p, "arrows") {
        let list = list.as_array().ok_or_else(|| EngineError::Params("`arrows` must be an array of \"e\" and \"h\"".into()))?;
        v.show_e = list.iter().any(|x| x.as_str() == Some("e"));
        v.show_h = list.iter().any(|x| x.as_str() == Some("h"));
    }
    v.phase = number(p, "phase")?.unwrap_or(0.0).to_radians();
    v.excitation = excitation(s, p)?;
    Ok(v)
}

fn view_field(s: &mut Session, p: &Value) -> Result<Value> {
    let path = required_string(p, "path")?;
    let width = integer(p, "width", 200..=s.limits.picture_side as u64)?.unwrap_or(1100) as usize;
    let v = field_view(s, p)?;
    let plot = view::field_plot(s, &v)?;
    let c = emcraft_render::plot::draw(&plot, width);
    write_png(path, c.png())?;
    Ok(json!({"path": path, "width": c.w, "height": c.h}))
}

fn view_sparameters(s: &mut Session, p: &Value) -> Result<Value> {
    let path = required_string(p, "path")?;
    let side = 200..=s.limits.picture_side as u64;
    let w = integer(p, "width", side.clone())?.unwrap_or(1000) as usize;
    let h = integer(p, "height", side)?.unwrap_or(600) as usize;
    let chart = view::sparameter_chart(s)?;
    let c = emcraft_render::chart::draw(&chart, w, h);
    write_png(path, c.png())?;
    Ok(json!({"path": path, "width": c.w, "height": c.h}))
}

fn file_sample(s: &mut Session, p: &Value) -> Result<Value> {
    let name = required_string(p, "name")?;
    let sample = crate::sample::find(name).ok_or_else(|| {
        EngineError::Params(format!("unknown sample `{}` (known: {})", name.chars().take(40).collect::<String>(), crate::sample::names().join(", ")))
    })?;
    let solve = get(p, "solve").and_then(Value::as_bool).unwrap_or(false);
    let mut fresh = Session { limits: s.limits, ..Session::new() };
    let script: Value = serde_json::from_str(sample.script).map_err(|e| EngineError::Failed(format!("sample {name}: {e}")))?;
    let steps: Vec<Value> = script
        .as_array()
        .map(|a| {
            a.iter().filter(|st| solve || !st.get("command").and_then(Value::as_str).is_some_and(|c| c.starts_with("solve."))).cloned().collect()
        })
        .unwrap_or_default();
    fresh.run_script(&Value::Array(steps))?;
    *s = fresh;
    Ok(s.summary())
}

fn summary(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(s.summary())
}

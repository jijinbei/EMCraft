//! Commands that build the model: materials, the waveguide, solids, imported surfaces,
//! inclusions and features, walls and ports.

use emcraft_model::{
    Axis, BoxSize, Conductivity, FaceSurface, Feature, Inclusion, Layer, MAX_POLES, Material, Metres, Point3, Pole, Port, PortFace, Positive,
    Section, Shape, Solid, Strip, Symmetry, SymmetryKind, Via, Waveguide,
};
use serde_json::{Value, json};

use crate::{EngineError, Params, Result, Session};

/// Most items in one list parameter (sections, layers, strips, vias).
const MAX_ITEMS: usize = 1000;

pub fn material(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    let material = Material::new(name, p.number("eps_r").required()?, p.number("mu_r").or(1.0)?)?
        .with_loss_tangent(p.number("loss_tangent").or(0.0)?)?
        .with_magnetic_loss_tangent(p.number("magnetic_loss_tangent").or(0.0)?)?;
    let material = material.with_dispersion(poles(p)?)?;
    s.model.add_material(material)?;
    Ok(json!({"name": name}))
}

/// A material's dispersion poles from `debye`, `lorentz` and `drude` (frequencies in display
/// units): a Debye pole's relaxation frequency f_r gives τ = 1/(2π f_r).
fn poles(p: &Params) -> Result<Vec<Pole>> {
    let mut poles = Vec::new();
    for kind in POLE_KINDS {
        poles.extend(poles_of(p, kind)?);
    }
    pole_count(poles)
}

/// `poles`, refused when there are more than a material holds.
pub(crate) fn pole_count(poles: Vec<Pole>) -> Result<Vec<Pole>> {
    if poles.len() > MAX_POLES {
        return Err(EngineError::Params(format!("a material holds at most {MAX_POLES} poles, got {}", poles.len())));
    }
    Ok(poles)
}

/// The parameters poles are given in, in the order a material keeps them.
pub(crate) const POLE_KINDS: [&str; 3] = ["debye", "lorentz", "drude"];

/// The poles of parameter `kind` (`debye`, `lorentz` or `drude`).
pub(crate) fn poles_of(p: &Params, kind: &str) -> Result<Vec<Pole>> {
    let mut poles = Vec::new();
    match kind {
        "debye" => {
            for d in p.items("debye", "Debye pole", MAX_POLES)? {
                let delta_eps = d.checked("delta_eps").required()?;
                let f = d.frequency("frequency").required()?;
                // τ = 1/(2π f_r) is computed: a frequency near 0 makes it infinite.
                let tau = Positive::new(1.0 / (2.0 * std::f64::consts::PI * f))
                    .map_err(|_| d.error("frequency", format!("gives no finite relaxation time, got {f}")))?;
                poles.push(Pole::Debye { delta_eps, tau });
            }
        }
        "lorentz" => {
            for l in p.items("lorentz", "Lorentz pole", MAX_POLES)? {
                let delta_eps = l.checked("delta_eps").required()?;
                let frequency = l.hertz("frequency").required()?;
                let damping = l.hertz("damping").required()?;
                poles.push(Pole::Lorentz { delta_eps, frequency, damping });
            }
        }
        _ => {
            for d in p.items("drude", "Drude pole", MAX_POLES)? {
                let plasma_frequency = d.hertz("plasma_frequency").required()?;
                let damping = d.hertz("damping").required()?;
                poles.push(Pole::Drude { plasma_frequency, damping });
            }
        }
    }
    Ok(poles)
}

/// The parameter a pole is given in.
pub(crate) fn pole_kind(pole: &Pole) -> &'static str {
    match pole {
        Pole::Debye { .. } => "debye",
        Pole::Lorentz { .. } => "lorentz",
        Pole::Drude { .. } => "drude",
    }
}

pub fn waveguide(s: &mut Session, p: &Params) -> Result<Value> {
    let width = p.metres("width").required()?;
    let height = p.metres("height").required()?;
    p.list("sections", MAX_ITEMS).required()?;
    let sections = p
        .items("sections", "section", MAX_ITEMS)?
        .iter()
        .map(|sec| {
            Ok(Section {
                length: sec.metres("length").required()?,
                material: sec.string("material").required()?.to_owned(),
                width: sec.metres("width").optional()?,
                offset: sec.length("offset").optional()?,
                height: sec.metres("height").optional()?,
                offset_y: sec.length("offset_y").optional()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let total: f64 = sections.iter().map(|x| x.length.get()).sum();
    let layers = p
        .items("layers", "layer", MAX_ITEMS)?
        .iter()
        .map(|l| Ok(Layer { thickness: l.metres("thickness").required()?, material: l.string("material").required()?.to_owned() }))
        .collect::<Result<Vec<_>>>()?;
    let strips = p
        .items("strips", "strip", MAX_ITEMS)?
        .iter()
        .map(|st| {
            Ok(Strip {
                y: st.length("y").required()?,
                x0: st.length("x0").required()?,
                width: st.metres("width").required()?,
                z0: st.length("z0").or(0.0)?,
                z1: st.length("z1").or(total)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let vias = p
        .items("vias", "via", MAX_ITEMS)?
        .iter()
        .map(|v| {
            Ok(Via {
                x0: v.length("x0").required()?,
                z0: v.length("z0").required()?,
                wx: v.metres("wx").required()?,
                wz: v.metres("wz").required()?,
                y0: v.length("y0").or(0.0)?,
                y1: v.length("y1").or(height.get())?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    s.model.set_waveguide(Waveguide { width, height, sections, layers, strips, vias })?;
    Ok(json!({"length": s.show_length(s.model.waveguide.as_ref().map_or(0.0, Waveguide::length))}))
}

/// `geometry.iris_filter`: a rectangular waveguide with a row of inductive irises.
pub fn iris_filter(s: &mut Session, p: &Params) -> Result<Value> {
    let width = p.metres("width").required()?;
    let height = p.metres("height").required()?;
    let end_width = p.metres("end_width").required()?;
    let cavity = p.metres("cavity").required()?;
    let f = emcraft_templates::IrisFilter {
        irises: p.integer("irises", 2..=emcraft_templates::MAX_IRISES).required()?,
        iris_length: p.metres("iris_length").required()?,
        end_width,
        inner_width: p.metres("inner_width").or(end_width)?,
        cavity,
        inner_cavity: p.metres("inner_cavity").or(cavity)?,
        feed: p.metres("feed").required()?,
        material: p.string("material").or("vacuum")?.to_owned(),
    };
    if f.end_width.max(f.inner_width.get()) > width.get() {
        return Err(p.error("end_width", "an iris's aperture must not be wider than the guide"));
    }
    let sections = emcraft_templates::iris_sections(&f);
    s.model.set_waveguide(Waveguide { width, height, sections, layers: Vec::new(), strips: Vec::new(), vias: Vec::new() })?;
    Ok(json!({"length": s.show_length(s.model.waveguide.as_ref().map_or(0.0, Waveguide::length)), "irises": f.irises}))
}

emcraft_macros::named_enum! {
    /// A primitive shape (`shape`).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(crate) enum Primitive {
        Box = "box",
        Cylinder = "cylinder",
        Sphere = "sphere",
        Prism = "prism",
        Revolved = "revolved",
    }
}

emcraft_macros::named_serde!(Primitive);

/// A shape from `shape`, `size`, `radius`, `length` and `points`.
pub(crate) fn shape(p: &Params) -> Result<Shape> {
    let dimension = |name| p.metres(name).required_or("for this shape");
    Ok(match p.choice("shape").required()? {
        Primitive::Box => Shape::Box { size: BoxSize::new(p.point("size").required()?.to_vec())? },
        Primitive::Cylinder => Shape::Cylinder { radius: dimension("radius")?, length: dimension("length")? },
        Primitive::Sphere => Shape::Sphere { radius: dimension("radius")? },
        Primitive::Prism => Shape::prism(&p.pairs("points", emcraft_model::MAX_PRISM_POINTS).required_or("for a prism")?, dimension("length")?)?,
        Primitive::Revolved => {
            let points = p.pairs("points", emcraft_model::MAX_PRISM_POINTS).required_or("for a revolved body")?;
            let sides = p.integer("sides", 3..=Shape::MAX_REVOLVED_SIDES).or(64)?;
            Shape::revolved("revolved", &points, sides)?
        }
    })
}

pub fn inclusion(s: &mut Session, p: &Params) -> Result<Value> {
    let inclusion =
        Inclusion { shape: shape(p)?, at: p.point("at").or(Point3::ORIGIN)?, material: p.string("material").optional()?.map(str::to_owned) };
    s.model.add_inclusion(inclusion)?;
    Ok(json!({"inclusions": s.model.inclusions.len()}))
}

/// Largest geometry file read.
const MAX_IMPORT_BYTES: u64 = 512 << 20;

/// A file's bytes, refusing one over `max` bytes before reading it.
pub(crate) fn read_capped(path: &str, max: u64) -> Result<Vec<u8>> {
    let len = std::fs::metadata(path).map_err(|e| EngineError::failed(format!("`{path}`: {e}")))?.len();
    if len > max {
        return Err(EngineError::failed(format!("`{path}` is {} MB; at most {} MB are read", len >> 20, max >> 20)));
    }
    std::fs::read(path).map_err(|e| EngineError::failed(format!("`{path}`: {e}")))
}

/// The bytes of the geometry file `path` names: a template's design (`template:<file>`,
/// compiled in, [`emcraft_templates::design`]) or a file read from disk.
fn geometry_file(p: &Params, path: &str) -> Result<std::borrow::Cow<'static, [u8]>> {
    if let Some(bytes) = emcraft_templates::design(path) {
        return Ok(std::borrow::Cow::Borrowed(bytes));
    }
    if path.starts_with(emcraft_templates::DESIGN_SCHEME) {
        let known = emcraft_templates::DESIGNS.iter().map(|d| format!("{}{}", emcraft_templates::DESIGN_SCHEME, d.file));
        return Err(
            p.error("path", crate::text::unknown(format_args!("names no template's design: `{}`", crate::text::clip(path, 64)), "designs: ", known))
        );
    }
    read_capped(path, MAX_IMPORT_BYTES).map(std::borrow::Cow::Owned)
}

pub fn import(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let unit = s.display.length.to_si(p.positive("unit").or(1.0)?);
    let angle = p.number("angle").check(|a| *a > 0.0 && *a < 180.0, "between 0 and 180 degrees").or(30.0)?;
    let material = p.string("material").or("vacuum")?.to_owned();
    let bytes = geometry_file(p, path)?;
    // A SolidCraft design: its bodies at the parameters given, each face's exact surface from the
    // design; any other file: its triangles, surfaces fitted.
    let (mut bodies, mut exact) = if emcraft_io::solidcraft::is_design(path) {
        let overrides = design_overrides(p, &bytes)?;
        let tolerance = p.positive_length("tolerance").optional()?.map(|t| t * 1e3);
        let read = emcraft_io::solidcraft::read(&bytes, &overrides, tolerance)?;
        let exact: Vec<Vec<Option<FaceSurface>>> = read.iter().map(|b| b.surfaces.clone()).collect();
        (read.into_iter().map(|b| b.surface).collect::<Vec<_>>(), Some(exact))
    } else {
        if p.has("parameters") {
            return Err(p.error("parameters", "only a SolidCraft design (.sc, .solidcraft) has parameters"));
        }
        (emcraft_io::read_bodies(path, &bytes, unit)?, None)
    };
    if bodies.is_empty() {
        return Err(EngineError::failed(format!("`{path}` holds no body")));
    }
    let fit = p.flag("fit_surfaces").or(true)?;
    // Each triangle's face as read (a B-rep face), before the split by angle.
    let mut sources: Vec<Vec<u32>> = bodies.iter().map(|b| b.tags.clone()).collect();
    let faces: u32 = bodies.iter_mut().map(|b| emcraft_io::split_faces(b, angle)).sum();
    let mut fitted = Fitted::default();
    let triangles: usize = bodies.iter().map(|b| b.triangles.len()).sum();
    let materials = body_materials(p, &bodies)?;
    // An open problem: the box of air lined by a perfectly matched layer, a quarter of the
    // band's longest wavelength of air between the parts and the layer and as much layer.
    let open = p.flag("open").or(false)?;
    let quarter = s.model.job.range().map(|b| emcraft_model::units::C0 / b.start.get() / 4.0);
    let thickness = match (open, p.metres("thickness").optional()?) {
        (false, _) => None,
        (true, Some(t)) => Some(t),
        (true, None) => Some(quarter.and_then(|q| Metres::new(q).ok()).ok_or_else(|| {
            p.error("thickness", "or confirm the run's settings (project.setup): the layer and the air before it are a quarter wavelength each")
        })?),
    };
    // The region the fields fill (ADR 0005): the body given `interior` (or named), the air the
    // metal bodies enclose, else a box of air around them all (always for an open problem).
    let interiors: Vec<&str> = materials.iter().filter(|(_, m)| m == crate::bodies::INTERIOR).map(|(b, _)| b.as_str()).collect();
    if interiors.len() > 1 {
        return Err(p.error("materials", "one body at most is the region (`interior`)"));
    }
    let given = p.string("region").optional()?.map(str::to_owned);
    if let (Some(b), Some(r)) = (interiors.first(), &given)
        && r != b
    {
        return Err(p.error("materials", format!("`{b}` is the region (`interior`): `region` cannot be `{r}`")));
    }
    let keep: Option<Keep> = p.choice("keep").optional()?;
    let mut cavity = None;
    let mut fallback = None;
    let region = match (given, interiors.first()) {
        (Some(r), _) => r,
        (None, Some(b)) => (*b).to_owned(),
        (None, None) if open || p.has("margin") => "box".into(),
        // Metal bodies enclosing air: it is the region. Bodies that enclose none (a part in
        // air) lie in a box of air.
        (None, None) => {
            let mut moved = bodies.clone();
            match enclosed_air(s, &mut moved, &materials, keep.unwrap_or_default(), &material) {
                Ok(c) => {
                    bodies = moved;
                    cavity = Some(c);
                    "cavity".into()
                }
                Err(e) => {
                    fallback = Some(e.to_string());
                    "box".into()
                }
            }
        }
    };
    if open && region != "box" {
        return Err(p.error("region", "an open problem lies in a box of air (`box`)"));
    }
    if region != "cavity" && keep.is_some() {
        return Err(p.error("keep", "chooses pieces of a cavity (`region`: `cavity`)"));
    }
    let mut placed = Vec::new();
    let mut imported = Vec::new();
    if region == "cavity" {
        if p.has("margin") {
            return Err(p.error("margin", "a cavity is the air inside the bodies' own bounding box: it takes no margin"));
        }
        if cavity.is_none() {
            cavity = Some(enclosed_air(s, &mut bodies, &materials, keep.unwrap_or_default(), &material)?);
        }
        // The metal bodies are the cavity's walls now; the others are placed in it.
        let metal = |b: &emcraft_io::Surface| material_of(&materials, &b.name) == crate::bodies::METAL;
        placed.extend(bodies.iter().filter(|b| metal(b)).map(|b| json!({"body": b.name, "material": crate::bodies::METAL, "as": "walls"})));
        imported.extend(
            bodies.iter().filter(|b| metal(b)).map(|b| emcraft_model::PartBody { name: b.name.clone(), material: crate::bodies::METAL.into() }),
        );
        let keep: Vec<bool> = bodies.iter().map(|b| !metal(b)).collect();
        retain_kept(&mut bodies, &keep);
        retain_kept(&mut sources, &keep);
        if let Some(e) = exact.as_mut() {
            retain_kept(e, &keep);
        }
    } else if region == "box" {
        let margin = p.positive_length("margin").optional()?;
        s.model.set_solid(Solid { shape: air_box(p, &mut bodies, margin, thickness.map(Metres::get), quarter)?, material: material.clone() })?;
    } else {
        let k = bodies.iter().position(|b| b.name == region).ok_or_else(|| p.error("region", format!("names no body `{region}` (or `box`)")))?;
        let body = bodies.remove(k);
        if !matches!(material_of(&materials, &body.name), crate::bodies::METAL | crate::bodies::INTERIOR) {
            return Err(p.error("materials", format!("`{}` is the region: its filling is `material`", body.name)));
        }
        placed.push(json!({"body": body.name, "material": crate::bodies::INTERIOR, "as": "region"}));
        imported.push(emcraft_model::PartBody { name: body.name.clone(), material: crate::bodies::INTERIOR.into() });
        let source = sources.remove(k);
        let known = exact.as_mut().map(|e| e.remove(k));
        s.model.set_solid(Solid { shape: surface(body, &source, fit, known.as_deref(), &mut fitted), material: material.clone() })?;
    }
    let mut exact = exact.map(Vec::into_iter);
    for (b, source) in bodies.into_iter().zip(sources) {
        let m = material_of(&materials, &b.name).to_owned();
        let name = b.name.clone();
        let known = exact.as_mut().and_then(Iterator::next);
        let kind = crate::bodies::place(&mut s.model, surface(b, &source, fit, known.as_deref(), &mut fitted), Point3::ORIGIN, Axis::Z, &m)?;
        placed.push(json!({"body": name, "material": m, "as": kind}));
    }
    if let Some(t) = thickness {
        s.model.set_open(Some(emcraft_model::Open { thickness: t, reflection: None, shape: emcraft_model::OpenShape::Box }))?;
    }
    s.model.imported = imported;
    let bounds = s.model.bounds().unwrap_or_default();
    let unit = s.display.length.to_si(1.0);
    let pieces = cavity
        .map(|c: emcraft_mesh::cavity::Cavity| json!({"kept": c.kept.iter().map(|v| v / unit.powi(3)).collect::<Vec<_>>(), "dropped": c.dropped, "axis": (["x", "y", "z"].get(c.axis).copied().unwrap_or("z"))}));
    Ok(json!({
        "triangles": triangles,
        "faces": faces,
        "planes": fitted.plane,
        "curved": {"cylinder": fitted.cylinder, "sphere": fitted.sphere, "cone": fitted.cone, "torus": fitted.torus},
        "region": region,
        "pieces": pieces,
        "cavity": fallback,
        "bodies": placed,
        "open": thickness.map(|t| s.show_length(t.get())),
        "volume": s.model.solid.as_ref().map(|x| x.shape.volume()),
        "bounds": bounds.corners().map(|c| c.map(|v| s.show_length(v))),
    }))
}

/// `geometry.import.inspect`'s reply: what a part holds.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportInspection {
    /// `solidcraft`, `stl` or `3mf`.
    pub format: String,
    pub bodies: Vec<ImportedBody>,
    /// Its extent (display units), none without vertices.
    pub size: Option<[f64; 3]>,
    /// `display` (STL has none) or `file`.
    pub units: String,
    /// A design's parameters.
    pub parameters: Vec<DesignParameter>,
}

/// A design's parameter: its value as saved (in `unit`), none when the design leaves it out.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DesignParameter {
    pub name: String,
    pub value: Option<f64>,
    pub unit: String,
}

/// A body of an inspected part.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportedBody {
    pub name: String,
    pub triangles: usize,
}

/// `geometry.import.inspect`: what a part holds — its format, bodies, size, units and a
/// design's parameters — read as `geometry.import` reads it, nothing imported.
pub fn import_inspect(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let unit = s.display.length.to_si(p.positive("unit").or(1.0)?);
    let bytes = geometry_file(p, path)?;
    let design = emcraft_io::solidcraft::is_design(path);
    let (format, bodies, parameters) = if design {
        let parameters = emcraft_io::solidcraft::parameters(&bytes)?;
        let bodies = emcraft_io::solidcraft::read(&bytes, &[], None)?.into_iter().map(|b| b.surface).collect::<Vec<_>>();
        let parameters = parameters.into_iter().map(|q| DesignParameter { name: q.name, value: q.value, unit: q.unit }).collect();
        ("solidcraft", bodies, parameters)
    } else {
        let format = match emcraft_io::Format::of(path)? {
            emcraft_io::Format::Stl => "stl",
            emcraft_io::Format::ThreeMf => "3mf",
        };
        (format, emcraft_io::read_bodies(path, &bytes, unit)?, Vec::new())
    };
    let lo = bodies.iter().flat_map(|b| b.positions.iter()).fold([f64::INFINITY; 3], |m, q| [0, 1, 2].map(|d| m[d].min(q[d])));
    let hi = bodies.iter().flat_map(|b| b.positions.iter()).fold([f64::NEG_INFINITY; 3], |m, q| [0, 1, 2].map(|d| m[d].max(q[d])));
    let size = (lo[0] <= hi[0]).then(|| [0, 1, 2].map(|d| s.show_length(hi[d] - lo[d])));
    Ok(json!(ImportInspection {
        format: format.to_owned(),
        bodies: bodies.iter().map(|b| ImportedBody { name: b.name.clone(), triangles: b.triangles.len() }).collect(),
        size,
        units: if format == "stl" { "display" } else { "file" }.to_owned(),
        parameters,
    }))
}

/// The material `materials` (body name, material) give the body `name`: metal unless named.
fn material_of<'m>(materials: &'m [(String, String)], name: &str) -> &'m str {
    materials.iter().find(|(n, _)| n == name).map_or(crate::bodies::METAL, |(_, m)| m.as_str())
}

emcraft_macros::named_enum! {
    /// Which pieces of a cavity's air are kept (`keep`).
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub(crate) enum Keep {
        #[default]
        Enclosed = "enclosed",
        Largest = "largest",
    }
}

emcraft_macros::named_serde!(Keep);

/// `region: cavity`: the metal bodies' bounding box less them, the pieces of air they enclose
/// across the two axes other than the channel's kept (`emcraft_mesh::cavity`), made the region; the bodies (all of them, the
/// other materials' too) moved with the box to the origin. Records the metal bodies in `placed`.
fn enclosed_air(
    s: &mut Session,
    bodies: &mut [emcraft_io::Surface],
    materials: &[(String, String)],
    keep: Keep,
    material: &str,
) -> Result<emcraft_mesh::cavity::Cavity> {
    let metal = |b: &emcraft_io::Surface| material_of(materials, &b.name) == crate::bodies::METAL;
    let walls: Vec<&emcraft_io::Surface> = bodies.iter().filter(|b| metal(b)).collect();
    if walls.is_empty() {
        return Err(EngineError::failed("a cavity is the air metal bodies enclose: every body is given another material"));
    }
    let lo = walls.iter().flat_map(|b| b.positions.iter()).fold([f64::INFINITY; 3], |m, q| [0, 1, 2].map(|d| m[d].min(q[d])));
    let shift = emcraft_model::Vec3::new(-lo[0], -lo[1], -lo[2]);
    for b in bodies.iter_mut() {
        b.positions.iter_mut().for_each(|q| *q += shift);
    }
    let walls: Vec<(String, emcraft_mesh::csg::Surface)> = bodies
        .iter()
        .filter(|b| metal(b))
        .map(|b| {
            (b.name.clone(), emcraft_mesh::csg::Surface { positions: b.positions.clone(), triangles: b.triangles.clone(), tags: b.tags.clone() })
        })
        .collect();
    let keep = match keep {
        Keep::Enclosed => emcraft_mesh::cavity::Keep::Enclosed,
        Keep::Largest => emcraft_mesh::cavity::Keep::Largest,
    };
    let c = emcraft_mesh::cavity::cavity(&walls, keep)?;
    let shape = Shape::Surface {
        name: "cavity".into(),
        positions: c.surface.positions.clone(),
        triangles: c.surface.triangles.clone(),
        tags: c.surface.tags.clone(),
        surfaces: Vec::new(),
    };
    s.model.set_solid(Solid { shape, material: material.to_owned() })?;
    Ok(c)
}

/// How many faces of the imported bodies got each kind of exact surface.
#[derive(Default)]
struct Fitted {
    plane: usize,
    cylinder: usize,
    sphere: usize,
    cone: usize,
    torus: usize,
}

/// An imported body as a shape, each face's exact surface fitted to its vertices (`fit`) where
/// one fits, `source` the triangles' faces before they were split by angle. With `known` (a
/// design's exact surfaces by source face) each face takes its source face's instead.
fn surface(b: emcraft_io::Surface, source: &[u32], fit: bool, known: Option<&[Option<FaceSurface>]>, fitted: &mut Fitted) -> Shape {
    let surfaces = match known {
        Some(known) => {
            let mut out = vec![None; b.tags.iter().map(|t| *t as usize + 1).max().unwrap_or(0)];
            for (tag, src) in b.tags.iter().zip(source) {
                if let (Some(slot), Some(f)) = (out.get_mut(*tag as usize), known.get(*src as usize)) {
                    *slot = *f;
                }
            }
            out
        }
        None if fit => emcraft_io::fit::fit_faces(&b, source),
        None => Vec::new(),
    };
    for f in surfaces.iter().flatten() {
        match f {
            FaceSurface::Plane { .. } => fitted.plane += 1,
            FaceSurface::Cylinder { .. } => fitted.cylinder += 1,
            FaceSurface::Sphere { .. } => fitted.sphere += 1,
            FaceSurface::Cone { .. } => fitted.cone += 1,
            FaceSurface::Torus { .. } => fitted.torus += 1,
        }
    }
    Shape::Surface { name: b.name, positions: b.positions, triangles: b.triangles, tags: b.tags, surfaces }
}

/// Keeps the items of `v` whose flag in `keep` is set (the bodies' lists kept in step).
fn retain_kept<T>(v: &mut Vec<T>, keep: &[bool]) {
    let mut k = keep.iter().copied();
    v.retain(|_| k.next().unwrap_or(false));
}

/// A design's parameter overrides from `parameters` (name → a number in the unit the design
/// shows it in, an expression of the project's variables (`"=a"`) read as such a number, or an
/// expression the design evaluates (`"3 mm"`)), refused for a name the design lacks.
fn design_overrides(p: &Params, bytes: &[u8]) -> Result<Vec<(String, String)>> {
    let Some(v) = p.raw("parameters") else { return Ok(Vec::new()) };
    let o = v.as_object().ok_or_else(|| p.error("parameters", "must map the design's parameter names to values"))?;
    let known = emcraft_io::solidcraft::parameters(bytes)?;
    o.iter()
        .map(|(name, value)| {
            let q = known.iter().find(|q| q.name == *name).ok_or_else(|| p.error("parameters", format!("the design has no parameter `{name}`")))?;
            let expression = match value {
                Value::Number(n) => format!("{} {}", n, q.unit),
                // An expression of the project's variables (`"=a/2"`): its number, in the
                // design's unit as a number given is (a template passes its parameters so).
                Value::String(e) if crate::expr::body(e).is_some() => format!("{} {}", p.number_of("parameters", value)?, q.unit),
                Value::String(e) => e.clone(),
                _ => return Err(p.error("parameters", format!("`{name}`: a number or an expression"))),
            };
            Ok((name.clone(), expression.trim().to_owned()))
        })
        .collect()
}

/// The bodies' materials from `materials` (body name → material); the others are metal.
fn body_materials(p: &Params, bodies: &[emcraft_io::Surface]) -> Result<Vec<(String, String)>> {
    let mut materials: Vec<(String, String)> = Vec::new();
    if let Some(v) = p.raw("materials") {
        let o = v.as_object().ok_or_else(|| p.error("materials", "must map body names to materials (or \"metal\")"))?;
        for (body, m) in o {
            let m = m.as_str().ok_or_else(|| p.error("materials", format!("`{body}`: the material must be a name")))?;
            if !bodies.iter().any(|b| &b.name == body) {
                return Err(p.error("materials", format!("names no body `{body}`")));
            }
            materials.push((body.clone(), m.to_owned()));
        }
    }
    Ok(materials)
}

/// The box of air around `bodies`, from the origin, the bodies moved into it: `margin` beyond
/// them all; by default a quarter of their largest extent, or with a layer of `thickness` as
/// much layer again and a `quarter` wavelength of air.
fn air_box(p: &Params, bodies: &mut [emcraft_io::Surface], margin: Option<f64>, thickness: Option<f64>, quarter: Option<f64>) -> Result<Shape> {
    let lo = bodies.iter().flat_map(|b| b.positions.iter()).fold([f64::INFINITY; 3], |m, q| [0, 1, 2].map(|d| m[d].min(q[d])));
    let hi = bodies.iter().flat_map(|b| b.positions.iter()).fold([f64::NEG_INFINITY; 3], |m, q| [0, 1, 2].map(|d| m[d].max(q[d])));
    let extent = (0..3).map(|d| hi[d] - lo[d]).fold(0.0f64, f64::max);
    let margin = match (margin, thickness) {
        (Some(m), Some(t)) if m <= t => {
            return Err(p.error("margin", "must exceed the layer's thickness: air must lie between the parts and the layer"));
        }
        (Some(m), _) => m,
        (None, Some(t)) => t + quarter.unwrap_or(t),
        (None, None) => 0.25 * extent,
    };
    let shift = emcraft_model::Vec3::new(margin - lo[0], margin - lo[1], margin - lo[2]);
    for b in bodies.iter_mut() {
        b.positions.iter_mut().for_each(|q| *q += shift);
    }
    let size = emcraft_model::Vec3::new(hi[0] - lo[0] + 2.0 * margin, hi[1] - lo[1] + 2.0 * margin, hi[2] - lo[2] + 2.0 * margin);
    Ok(Shape::Box { size: BoxSize::new(size)? })
}

pub fn feature(s: &mut Session, p: &Params) -> Result<Value> {
    let feature = Feature {
        combine: p.choice("combine").required()?,
        shape: shape(p)?,
        at: p.point("at").or(Point3::ORIGIN)?,
        axis: p.choice("axis").or_default()?,
    };
    s.model.add_feature(feature)?;
    Ok(json!({"features": s.model.features.len()}))
}

/// `model.region`: the region automatic round what the model holds (`auto`, its `margin` and
/// layer `thickness` a quarter wavelength at the band's top unless given) or pinned as it is.
pub fn region(s: &mut Session, p: &Params) -> Result<Value> {
    let mode = p.string("mode").required()?;
    match mode {
        "auto" => {
            s.model.region = emcraft_model::Region::Auto { margin: p.metres("margin").optional()?, thickness: p.metres("thickness").optional()? };
            s.model.refresh_region()?;
        }
        "pinned" => {
            if p.has("margin") || p.has("thickness") {
                return Err(p.error("margin", "and `thickness` are an automatic region's"));
            }
            s.model.region = emcraft_model::Region::Pinned;
        }
        other => return Err(p.error("mode", format!("must be `auto` or `pinned`, got `{}`", crate::text::clip(other, 40)))),
    }
    let b = s.model.solid.as_ref().and_then(|x| x.shape.box_bounds());
    let l = |v: f64| s.display.length.from_si(v);
    Ok(json!({
        "mode": mode,
        "box": b.map(|b| json!([[l(b.min.x), l(b.min.y), l(b.min.z)], [l(b.max.x), l(b.max.y), l(b.max.z)]])),
        "layer": s.model.open.map(|o| l(o.thickness.get())),
    }))
}

/// `model.background`: what fills the model where no body is (vacuum by default) — the
/// region's material.
pub fn background(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("material").required()?;
    s.model.material(name)?;
    s.model.background = Some(name.to_owned());
    if let Some(solid) = s.model.solid.as_mut() {
        name.clone_into(&mut solid.material);
        s.model.mesh = None;
    }
    Ok(json!({"background": name}))
}

pub fn solid(s: &mut Session, p: &Params) -> Result<Value> {
    s.model.set_solid(Solid { shape: shape(p)?, material: p.string("material").or("vacuum")?.to_owned() })?;
    Ok(json!({"volume": s.model.solid.as_ref().map(|x| x.shape.volume())}))
}

pub fn walls(s: &mut Session, p: &Params) -> Result<Value> {
    let sigma = p.checked::<Conductivity>("conductivity").optional()?;
    let thickness = p.metres("thickness").optional()?;
    s.model.set_wall_thickness(thickness)?;
    s.model.set_wall_conductivity(sigma);
    Ok(json!({"conductivity": sigma, "thickness": thickness.map(|t| s.show_length(t.get()))}))
}

pub fn symmetry(s: &mut Session, p: &Params) -> Result<Value> {
    let clear = p.flag("clear").or(false)?;
    let axis: Option<Axis> = p.choice("axis").optional()?;
    let mut planes = s.model.symmetry.clone();
    if clear {
        if p.has("kind") || p.has("at") {
            return Err(p.error("clear", "takes only `axis` (the plane to remove; all of them without it)"));
        }
        planes.retain(|q| axis.is_some_and(|a| q.axis != a));
    } else {
        let axis = axis.ok_or_else(|| p.error("axis", "is required (or `clear`)"))?;
        let kind: SymmetryKind = p.choice("kind").required()?;
        // The model's middle by default: the only plane its walls are symmetric about.
        let middle = s.model.bounds().map(|b| 0.5 * (b.min[axis.index()] + b.max[axis.index()]));
        let at = p.length("at").or_maybe(middle).required_or("no geometry to take the middle of")?;
        planes.retain(|q| q.axis != axis);
        planes.push(Symmetry { axis, at, kind });
        planes.sort_by_key(|q| q.axis.index());
    }
    s.model.set_symmetry(planes)?;
    let shown: Vec<Value> = s.model.symmetry.iter().map(|q| json!({"axis": q.axis.id(), "at": s.show_length(q.at), "kind": q.kind})).collect();
    Ok(json!({"planes": shown, "fraction": s.model.symmetry_fraction()}))
}

/// `model.symmetry.find`.
pub fn find_symmetry(s: &mut Session, p: &Params) -> Result<Value> {
    let kind: Option<SymmetryKind> = p.choice("kind").optional()?;
    let found = s.model.find_symmetry();
    if found.is_empty() {
        return Err(EngineError::failed("the model is not symmetric about the middle of any axis (its geometry, materials and ports together)"));
    }
    // The planes the ports decide first, then those `kind` does; two at most.
    let mut planes: Vec<Symmetry> = found.iter().filter_map(|f| f.kind.map(|k| Symmetry { axis: f.axis, at: f.at, kind: k })).collect();
    planes.extend(found.iter().filter(|f| f.kind.is_none()).filter_map(|f| kind.map(|k| Symmetry { axis: f.axis, at: f.at, kind: k })));
    planes.truncate(2);
    if planes.is_empty() {
        let axes: Vec<&str> = found.iter().map(|f| f.axis.id()).collect();
        return Err(p.error("kind", format!("is needed: the model is symmetric across {}, and no port decides the wall", axes.join(" and "))));
    }
    planes.sort_by_key(|q| q.axis.index());
    s.model.set_symmetry(planes)?;
    let shown: Vec<Value> = s.model.symmetry.iter().map(|q| json!({"axis": q.axis.id(), "at": s.show_length(q.at), "kind": q.kind})).collect();
    let all: Vec<Value> = found.iter().map(|f| json!({"axis": f.axis.id(), "at": s.show_length(f.at), "kind": f.kind})).collect();
    Ok(json!({"planes": shown, "fraction": s.model.symmetry_fraction(), "found": all}))
}

pub fn port(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    let axis: Option<Axis> = p.choice("axis").optional()?;
    let face = p
        .lengths("face")
        .and_then(|f| {
            let values: [f64; 5] = f.try_into().map_err(|_| p.error("face", "must be [at, u0, v0, a, b]"))?;
            Ok(PortFace::from_values(axis.unwrap_or_default(), values)?)
        })
        .optional()?;
    if face.is_none() && axis.is_some_and(|a| a != Axis::Z) {
        return Err(p.error("axis", "x or y needs `face` (a port's end lies across z)"));
    }
    let port = Port {
        name: name.to_owned(),
        end: p.choice("end").required()?,
        modes: p.integer("modes", 1..=emcraft_model::MAX_PORT_MODES as usize).or(4)? as u32,
        face,
        mode_solver: p.choice("mode_solver").or_default()?,
        reference: p.checked("impedance").optional()?,
        deembed: p.length("deembed").or(0.0)?,
        terminals: p.choice("terminals").or_default()?,
    };
    s.model.add_port(port)?;
    Ok(json!({"name": name}))
}

/// `geometry.measure`: the distance between two picked points, with the gap and angle their
/// faces or edges give ([`emcraft_model::measure`]), lengths in display units.
pub fn measure(s: &mut Session, p: &Params) -> Result<Value> {
    use emcraft_model::measure::{Kind, Mark};
    let mark = |at: &str, normal: &str, along: &str| -> Result<Mark> {
        let kind = match (p.vector(normal).optional()?, p.vector(along).optional()?) {
            (Some(_), Some(_)) => return Err(p.error(normal, format!("and `{along}` cannot both be given"))),
            (Some(n), None) => Kind::Plane(n),
            (None, Some(d)) => Kind::Line(d),
            (None, None) => Kind::Point,
        };
        Ok(Mark { at: p.point(at).required()?, kind })
    };
    let m = emcraft_model::measure::measure(mark("from", "from_normal", "from_along")?, mark("to", "to_normal", "to_along")?)?;
    let l = s.display().length;
    Ok(json!({
        "distance": l.from_si(m.distance),
        "delta": m.delta.to_array().map(|v| l.from_si(v)),
        "gap": m.gap.map(|g| l.from_si(g)),
        "angle": m.angle,
        "unit": l.symbol(),
    }))
}

/// `model.rotation`.
pub fn rotation(s: &mut Session, p: &Params) -> Result<Value> {
    if p.flag("clear").or(false)? {
        if p.has("n") || p.has("m") || p.has("centre") {
            return Err(p.error("clear", "takes nothing else"));
        }
        s.model.set_rotation(None)?;
        return Ok(rotation_shown(s));
    }
    let max = emcraft_model::symmetry::rotation::MAX_SECTORS as usize;
    let n = p.integer("n", emcraft_model::symmetry::rotation::MIN_SECTORS as usize..=max).required()?;
    let m = p.integer("m", 0..=n - 1).or(0)?;
    // The solid's middle across z by default: the axis of a cylinder, a sphere or a square box.
    let middle = s.model.bounds().map(|b| vec![0.5 * (b.min[0] + b.max[0]), 0.5 * (b.min[1] + b.max[1])]);
    let centre = p.lengths("centre").or_maybe(middle).required_or("no geometry to take the middle of")?;
    let [x, y] = centre[..] else {
        return Err(p.error("centre", "must be [x, y]"));
    };
    let rot = emcraft_model::Rotation::new(n as u32, m as u32, [x, y])?;
    s.model.set_rotation(Some(rot))?;
    Ok(rotation_shown(s))
}

/// The rotational symmetry as `model.rotation` replies it (in display units).
pub(crate) fn rotation_shown(s: &Session) -> Value {
    match s.model.rotation {
        None => json!({"rotation": null}),
        Some(r) => json!({"n": r.n, "m": r.m, "centre": [s.show_length(r.centre[0]), s.show_length(r.centre[1])], "fraction": 1.0 / f64::from(r.n)}),
    }
}

/// `model.rotation.find`.
pub fn find_rotation(s: &mut Session, p: &Params) -> Result<Value> {
    let found = s.model.find_rotation().ok_or_else(|| {
        let why = s.model.rotation_blocker().unwrap_or_else(|| "the solid does not turn onto itself about any axis along z".into());
        EngineError::failed(format!("no rotational symmetry found: {why}"))
    })?;
    let max = emcraft_model::symmetry::rotation::MAX_SECTORS as usize;
    // Round: a quarter by default; else the most sectors there are.
    let n = p.integer("n", emcraft_model::symmetry::rotation::MIN_SECTORS as usize..=max).or(found.most.unwrap_or(4) as usize)? as u32;
    if !found.allows(n) {
        return Err(p.error("n", format!("must divide {} (the model is {})", found.most.unwrap_or(n), found.group())));
    }
    let m = p.integer("m", 0..=n as usize - 1).or(0)? as u32;
    s.model.set_rotation(Some(emcraft_model::Rotation::new(n, m, found.centre)?))?;
    let mut reply = rotation_shown(s);
    reply["group"] = json!(found.group());
    Ok(reply)
}

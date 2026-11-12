//! Commands that build the model: materials, the waveguide, solids, imported surfaces,
//! inclusions and features, walls and ports.

use emcraft_model::{
    Axis, BoxSize, Conductivity, End, FaceSurface, Feature, Inclusion, Layer, MAX_POLES, Material, Metres, Point3, Pole, Port, PortFace, Positive,
    Section, Shape, Solid, Strip, Symmetry, SymmetryKind, Via, Waveguide,
};
use serde_json::{Value, json};

use crate::{EngineError, Params, Result, Session};
use emcraft_model::port::FrameAxes;

/// Most items in one list parameter (sections, layers, strips, vias).
const MAX_ITEMS: usize = 1000;

pub fn material(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    let material = Material::new(name, p.number("eps_r").required()?, p.number("mu_r").or(1.0)?)?
        .with_loss_tangent(p.number("loss_tangent").or(0.0)?)?
        .with_magnetic_loss_tangent(p.number("magnetic_loss_tangent").or(0.0)?)?;
    let material =
        material.with_dispersion(poles(p)?)?.with_thermal_conductivity(p.number("thermal_conductivity").optional()?)?.with_ferrite(ferrite(p)?)?;
    s.model.add_material(material)?;
    Ok(json!({"name": name}))
}

/// A material's magnetization `ferrite`: {"magnetization" (4πMs, G), "bias" (internal H₀,
/// Oe), "linewidth" (ΔH, Oe, default 0), "axis" (default z)}; none when absent.
pub(crate) fn ferrite(p: &Params) -> Result<Option<emcraft_model::ferrite::Ferrite>> {
    let Some(v) = p.raw("ferrite").filter(|v| !v.is_null()) else { return Ok(None) };
    if !v.is_object() {
        return Err(p.error("ferrite", "must be an object: {\"magnetization\": 1780, \"bias\": 1000, \"linewidth\": 20, \"axis\": \"z\"}"));
    }
    let f = p.nested(v, "ferrite");
    Ok(Some(emcraft_model::ferrite::Ferrite {
        magnetization: f.number("magnetization").required()?,
        bias: f.number("bias").required()?,
        linewidth: f.number("linewidth").or(0.0)?,
        axis: f.choice("axis").or(Axis::Z)?,
    }))
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
    Ok(json!({"length": s.show_length(s.model.waveguide().map_or(0.0, Waveguide::length))}))
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
    Ok(json!({"length": s.show_length(s.model.waveguide().map_or(0.0, Waveguide::length)), "irises": f.irises}))
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
    Ok(json!({"inclusions": s.model.inclusions().len()}))
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

/// The bytes of a template's file `path` names (`template:<file>`, compiled in,
/// [`emcraft_templates::design`]); `None` for a path of the user's, an error for a template's
/// file there is none of.
pub(crate) fn template_file(p: &Params, path: &str) -> Result<Option<&'static [u8]>> {
    if let Some(bytes) = emcraft_templates::design(path) {
        return Ok(Some(bytes));
    }
    if path.starts_with(emcraft_templates::DESIGN_SCHEME) {
        let known = emcraft_templates::DESIGNS.iter().map(|d| format!("{}{}", emcraft_templates::DESIGN_SCHEME, d.file));
        return Err(
            p.error("path", crate::text::unknown(format_args!("names no template's file: `{}`", crate::text::clip(path, 64)), "files: ", known))
        );
    }
    Ok(None)
}

/// The bytes of the geometry file `path` names: a template's design or a file read from disk.
fn geometry_file(p: &Params, path: &str) -> Result<std::borrow::Cow<'static, [u8]>> {
    match template_file(p, path)? {
        Some(bytes) => Ok(std::borrow::Cow::Borrowed(bytes)),
        None => read_capped(path, MAX_IMPORT_BYTES).map(std::borrow::Cow::Owned),
    }
}

pub fn import(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let unit = s.display.length.to_si(p.positive("unit").or(1.0)?);
    let angle = p.number("angle").check(|a| *a > 0.0 && *a < 180.0, "between 0 and 180 degrees").or(30.0)?;
    let material = p.string("material").or("vacuum")?.to_owned();
    let mut parts = read_part(p, path, unit)?;
    let fit = p.flag("fit_surfaces").or(true)?;
    let faces: u32 = parts.iter_mut().map(|b| emcraft_io::split_faces(&mut b.body, angle)).sum();
    let triangles: usize = parts.iter().map(|b| b.body.triangles.len()).sum();
    let materials = body_materials(p, &parts.iter().map(|b| b.body.clone()).collect::<Vec<_>>())?;
    let layer = OpenLayer::read(s, p)?;
    let region = choose_region(s, p, &mut parts, &materials, &material, layer.thickness.is_some())?;
    // The design's faces by name, each body's triangles in the order they are placed (ADR 0010).
    let named: Vec<(String, emcraft_model::faces::NamedFaces)> = parts
        .iter()
        .filter(|b| !b.names.is_empty())
        .map(|b| (b.body.name.clone(), emcraft_model::faces::NamedFaces { names: b.names.clone(), source: b.source.clone() }))
        .collect();
    let mut fitted = Fitted::default();
    let mut placed = Vec::new();
    let mut imported = Vec::new();
    let pieces = match &region {
        Region::Cavity(c) => {
            // The metal bodies are the cavity's walls now; the others are placed in it.
            let (walls, rest): (Vec<PartBody>, Vec<PartBody>) =
                parts.into_iter().partition(|b| material_of(&materials, &b.body.name) == crate::bodies::METAL);
            placed.extend(walls.iter().map(|b| json!({"body": b.body.name, "material": crate::bodies::METAL, "as": "walls"})));
            imported.extend(walls.into_iter().map(|b| crate::bodies::part(&b.body.name.clone(), wall_shape(b.body), false)));
            parts = rest;
            let unit = s.display.length.to_si(1.0);
            Some(
                json!({"kept": c.kept.iter().map(|v| v / unit.powi(3)).collect::<Vec<_>>(), "dropped": c.dropped, "axis": (["x", "y", "z"].get(c.axis).copied().unwrap_or("z"))}),
            )
        }
        Region::Box { .. } => {
            let margin = p.positive_length("margin").optional()?;
            let shape = air_box(p, &mut parts, margin, layer.thickness.map(Metres::get), layer.quarter)?;
            s.model.set_solid(Solid { shape, material: material.clone() })?;
            None
        }
        Region::Body(name) => {
            let k = parts.iter().position(|b| b.body.name == *name).ok_or_else(|| p.error("region", format!("names no body `{name}` (or `box`)")))?;
            let body = parts.remove(k);
            if !matches!(material_of(&materials, &body.body.name), crate::bodies::METAL | crate::bodies::INTERIOR) {
                return Err(p.error("materials", format!("`{}` is the region: its filling is `material`", body.body.name)));
            }
            placed.push(json!({"body": body.body.name, "material": crate::bodies::INTERIOR, "as": "region"}));
            let name = body.body.name.clone();
            let shape = surface(body.body, &body.source, fit, body.exact.as_deref(), &mut fitted);
            imported.push(crate::bodies::part(&name, shape.clone(), true));
            s.model.set_solid(Solid { shape, material: material.clone() })?;
            None
        }
    };
    for b in parts {
        let m = material_of(&materials, &b.body.name).to_owned();
        let name = b.body.name.clone();
        let kind = crate::bodies::place(&mut s.model, surface(b.body, &b.source, fit, b.exact.as_deref(), &mut fitted), Point3::ORIGIN, Axis::Z, &m)?;
        placed.push(json!({"body": name, "material": m, "as": kind}));
    }
    if let Some(t) = layer.thickness {
        s.model.set_open(Some(emcraft_model::Open { thickness: t, reflection: None, shape: emcraft_model::OpenShape::Box }))?;
    }
    s.model.set_parts(imported);
    s.model.faces.extend(named);
    let bounds = s.model.bounds().unwrap_or_default();
    Ok(json!({
        "triangles": triangles,
        "faces": faces,
        "planes": fitted.plane,
        "curved": {"cylinder": fitted.cylinder, "sphere": fitted.sphere, "cone": fitted.cone, "torus": fitted.torus, "nurbs": fitted.nurbs},
        "region": region.name(),
        "pieces": pieces,
        "cavity": region.why_no_cavity(),
        "bodies": placed,
        "open": layer.thickness.map(|t| s.show_length(t.get())),
        "volume": s.model.domain.solid.as_ref().map(|x| x.shape.volume()),
        "bounds": bounds.corners().map(|c| c.map(|v| s.show_length(v))),
    }))
}

/// A body read from a part: its surface, each triangle's face as read (a B-rep face, before the
/// split by angle) and, for a SolidCraft design, each face's exact surface.
#[derive(Clone)]
struct PartBody {
    body: emcraft_io::Surface,
    source: Vec<u32>,
    exact: Option<Vec<Option<FaceSurface>>>,
    /// A design's faces' persistent names, by the face index `source` holds (none for a mesh).
    names: Vec<String>,
}

impl PartBody {
    /// The body moved by `d`: its triangles and its exact surfaces together (moved apart, the
    /// mesh would bend its elements onto surfaces left where the design drew them).
    fn moved(&mut self, d: emcraft_model::Vec3) {
        self.body.positions.iter_mut().for_each(|q| *q += d);
        if let Some(exact) = &mut self.exact {
            for f in exact.iter_mut() {
                *f = f.take().map(|f| f.moved(d));
            }
        }
    }
}

/// The lowest corner of `parts`' boxes (each coordinate's least).
fn lowest(parts: &[PartBody]) -> [f64; 3] {
    parts.iter().flat_map(|b| b.body.positions.iter()).fold([f64::INFINITY; 3], |m, q| [0, 1, 2].map(|d| m[d].min(q[d])))
}

/// The bodies of the part at `path` (`unit` m per file unit): a SolidCraft design's at the
/// parameters given, each face's exact surface from the design; any other file's triangles.
fn read_part(p: &Params, path: &str, unit: f64) -> Result<Vec<PartBody>> {
    let bytes = geometry_file(p, path)?;
    let parts: Vec<PartBody> = if emcraft_io::solidcraft::is_design(path) {
        let overrides = design_overrides(p, &bytes)?;
        let tolerance = p.positive_length("tolerance").optional()?.map(|t| t * 1e3);
        let read = emcraft_io::solidcraft::read(&bytes, &overrides, tolerance)?;
        read.into_iter().map(|b| PartBody { source: b.surface.tags.clone(), exact: Some(b.surfaces), names: b.face_names, body: b.surface }).collect()
    } else {
        if p.has("parameters") {
            return Err(p.error("parameters", "only a SolidCraft design (.sc, .solidcraft) has parameters"));
        }
        emcraft_io::read_bodies(path, &bytes, unit)?
            .into_iter()
            .map(|b| PartBody { source: b.tags.clone(), exact: None, names: Vec::new(), body: b })
            .collect()
    };
    if parts.is_empty() {
        return Err(EngineError::failed(format!("`{path}` holds no body")));
    }
    Ok(parts)
}

/// A metal body as the cavity's wall: its triangles, no exact surfaces.
fn wall_shape(b: emcraft_io::Surface) -> Shape {
    Shape::Surface { name: b.name, positions: b.positions, triangles: b.triangles, tags: b.tags, surfaces: Vec::new() }
}

/// An open problem's layer: whether `open`, its `thickness` (by default a quarter of the band's
/// longest wavelength of air, as is the air before it).
struct OpenLayer {
    thickness: Option<Metres>,
    /// A quarter of the band's longest wavelength (m), when the band is set.
    quarter: Option<f64>,
}

impl OpenLayer {
    fn read(s: &Session, p: &Params) -> Result<OpenLayer> {
        let open = p.flag("open").or(false)?;
        let quarter = s.model.job.range().map(|b| emcraft_model::units::C0 / b.start.get() / 4.0);
        let thickness = match (open, p.metres("thickness").optional()?) {
            (false, _) => None,
            (true, Some(t)) => Some(t),
            (true, None) => Some(quarter.and_then(|q| Metres::new(q).ok()).ok_or_else(|| {
                p.error("thickness", "or confirm the run's settings (project.setup): the layer and the air before it are a quarter wavelength each")
            })?),
        };
        Ok(OpenLayer { thickness, quarter })
    }
}

/// The region the fields fill (ADR 0005).
enum Region {
    /// A body of the part (named, or the one given `interior`).
    Body(String),
    /// The air the metal bodies enclose, the bodies moved with it.
    Cavity(emcraft_mesh::cavity::Cavity),
    /// A box of air round every body; with why the bodies enclose none when that was tried.
    Box { why_no_cavity: Option<String> },
}

impl Region {
    /// The reply's `region`: `cavity`, `box` or the body's name.
    fn name(&self) -> &str {
        match self {
            Region::Body(name) => name,
            Region::Cavity(_) => "cavity",
            Region::Box { .. } => "box",
        }
    }

    fn why_no_cavity(&self) -> Option<&str> {
        match self {
            Region::Box { why_no_cavity } => why_no_cavity.as_deref(),
            _ => None,
        }
    }
}

/// The region: `region` given, else the body given `interior`, else the air the metal bodies
/// enclose (the bodies moved with it), else a box of air (always for an `open` problem).
fn choose_region(s: &mut Session, p: &Params, parts: &mut [PartBody], materials: &[(String, String)], material: &str, open: bool) -> Result<Region> {
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
    // The kind asked for, checked before any cavity is made.
    let explicit_cavity = given.as_deref() == Some("cavity");
    let boxed = matches!(given.as_deref(), Some("box")) || (given.is_none() && interiors.is_empty() && (open || p.has("margin")));
    let tried = given.is_none() && interiors.is_empty() && !boxed;
    if open && !boxed {
        return Err(p.error("region", "an open problem lies in a box of air (`box`)"));
    }
    if explicit_cavity && p.has("margin") {
        return Err(p.error("margin", "a cavity is the air inside the bodies' own bounding box: it takes no margin"));
    }
    let region = if explicit_cavity {
        Region::Cavity(enclosed_air(s, parts, materials, keep.unwrap_or_default(), material)?)
    } else if boxed {
        Region::Box { why_no_cavity: None }
    } else if tried {
        // Metal bodies enclosing air: it is the region. Bodies that enclose none (a part in
        // air) lie in a box of air.
        let mut moved = parts.to_vec();
        match enclosed_air(s, &mut moved, materials, keep.unwrap_or_default(), material) {
            Ok(c) => {
                parts.clone_from_slice(&moved);
                Region::Cavity(c)
            }
            Err(e) => Region::Box { why_no_cavity: Some(e.to_string()) },
        }
    } else {
        Region::Body(given.or_else(|| interiors.first().map(|b| (*b).to_owned())).unwrap_or_default())
    };
    if !matches!(region, Region::Cavity(_)) && keep.is_some() {
        return Err(p.error("keep", "chooses pieces of a cavity (`region`: `cavity`)"));
    }
    Ok(region)
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

/// A SolidCraft design the project imports (ADR 0010): its file as the import names it, the
/// parameters the import sets (name → the number or expression given) and the design's own
/// parameters (or why it cannot be read now).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ImportedDesign {
    pub path: String,
    /// A template's design (`template:<file>`), compiled in: set by the template's parameters.
    pub template: bool,
    pub set: serde_json::Map<String, Value>,
    pub parameters: std::result::Result<Vec<DesignParameter>, String>,
}

/// The SolidCraft designs `s`'s history imports, each once, in the order first imported.
pub fn designs(s: &Session) -> Vec<ImportedDesign> {
    let mut out: Vec<ImportedDesign> = Vec::new();
    for step in s.history().iter().filter(|st| st.command == "geometry.import") {
        let Some(path) = step.param("path").and_then(Value::as_str).filter(|p| emcraft_io::solidcraft::is_design(p)) else { continue };
        let set = step.param("parameters").and_then(Value::as_object).cloned().unwrap_or_default();
        match out.iter_mut().find(|d| d.path == path) {
            Some(d) => d.set = set,
            None => {
                let read = |p: &str| -> Result<Vec<DesignParameter>> {
                    let bytes = match emcraft_templates::design(p) {
                        Some(b) => std::borrow::Cow::Borrowed(b),
                        None => std::borrow::Cow::Owned(read_capped(p, MAX_IMPORT_BYTES)?),
                    };
                    let found = emcraft_io::solidcraft::parameters(&bytes)?;
                    Ok(found.into_iter().map(|q| DesignParameter { name: q.name, value: q.value, unit: q.unit }).collect())
                };
                let template = path.starts_with(emcraft_templates::DESIGN_SCHEME);
                out.push(ImportedDesign { path: path.to_owned(), template, set, parameters: read(path).map_err(|e| e.to_string()) });
            }
        }
    }
    out
}

/// `geometry.reload`: the model built again from its history, every imported file read again (a
/// design saved in SolidCraft since); the history, the runs and the file kept.
pub fn reload(s: &mut Session, _: &Params) -> Result<Value> {
    let history = s.history.to_vec();
    let rebuilt = crate::study::variables::rebuild(s, &history)?;
    crate::study::variables::adopt(s, rebuilt);
    Ok(json!({"designs": designs(s).len()}))
}

/// `geometry.design`: sets parameter `name` of the imported design `design` to `value` where it
/// is imported (a number in the unit the design shows it in, or an expression), the model built
/// again with it.
pub fn design(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("design").required()?;
    let name = p.string("name").required()?;
    let given = p.string("value").required()?.trim();
    if given.is_empty() || given.chars().count() > crate::expr::MAX_LEN + 1 {
        return Err(p.error("value", format!("give a number or an expression of at most {} characters", crate::expr::MAX_LEN)));
    }
    let found = designs(s).into_iter().find(|d| d.path == path).ok_or_else(|| {
        let known = designs(s).into_iter().map(|d| d.path);
        p.error(
            "design",
            crate::text::unknown(format_args!("names no design the project imports: `{}`", crate::text::clip(path, 80)), "designs: ", known),
        )
    })?;
    let parameters = found.parameters.map_err(|e| p.error("design", e))?;
    if !parameters.iter().any(|q| q.name == name) {
        let known = parameters.iter().map(|q| q.name.clone());
        return Err(p.error(
            "name",
            crate::text::unknown(format_args!("names no parameter of the design: `{}`", crate::text::clip(name, 64)), "parameters: ", known),
        ));
    }
    let value = given.parse::<f64>().ok().filter(|x| x.is_finite()).map_or_else(|| json!(given), |x| json!(x));
    let mut history = s.history.to_vec();
    for step in history.iter_mut().filter(|st| st.command == "geometry.import" && st.param("path").and_then(Value::as_str) == Some(path)) {
        if let Some(map) = step.params.as_object_mut() {
            let set = map.entry("parameters").or_insert_with(|| json!({}));
            if let Some(set) = set.as_object_mut() {
                set.insert(name.to_owned(), value.clone());
            }
        }
    }
    let rebuilt = crate::study::variables::rebuild(s, &history)?;
    crate::study::variables::adopt(s, rebuilt);
    Ok(json!({"design": path, "name": name, "value": value}))
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
    bodies: &mut [PartBody],
    materials: &[(String, String)],
    keep: Keep,
    material: &str,
) -> Result<emcraft_mesh::cavity::Cavity> {
    let metal = |b: &PartBody| material_of(materials, &b.body.name) == crate::bodies::METAL;
    let walls: Vec<PartBody> = bodies.iter().filter(|b| metal(b)).cloned().collect();
    if walls.is_empty() {
        return Err(EngineError::failed("a cavity is the air metal bodies enclose: every body is given another material"));
    }
    let lo = lowest(&walls);
    let shift = emcraft_model::Vec3::new(-lo[0], -lo[1], -lo[2]);
    bodies.iter_mut().for_each(|b| b.moved(shift));
    let walls: Vec<(String, emcraft_mesh::csg::Surface)> = bodies
        .iter()
        .filter(|b| metal(b))
        .map(|b| &b.body)
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
        name: emcraft_model::CAVITY.into(),
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
    nurbs: usize,
}

/// An imported body as a shape, each face's exact surface fitted to its vertices (`fit`) where
/// one fits, `source` the triangles' faces before they were split by angle. With `known` (a
/// design's exact surfaces by source face) each face takes its source face's instead.
fn surface(b: emcraft_io::Surface, source: &[u32], fit: bool, known: Option<&[Option<FaceSurface>]>, fitted: &mut Fitted) -> Shape {
    let surfaces = match known {
        Some(known) if fit => {
            let mut out = vec![None; b.tags.iter().map(|t| *t as usize + 1).max().unwrap_or(0)];
            for (tag, src) in b.tags.iter().zip(source) {
                if let (Some(slot), Some(f)) = (out.get_mut(*tag as usize), known.get(*src as usize)) {
                    slot.clone_from(f);
                }
            }
            out
        }
        None if fit => emcraft_io::fit::fit_faces(&b, source),
        _ => Vec::new(),
    };
    for f in surfaces.iter().flatten() {
        match f {
            FaceSurface::Plane { .. } => fitted.plane += 1,
            FaceSurface::Cylinder { .. } => fitted.cylinder += 1,
            FaceSurface::Sphere { .. } => fitted.sphere += 1,
            FaceSurface::Cone { .. } => fitted.cone += 1,
            FaceSurface::Torus { .. } => fitted.torus += 1,
            FaceSurface::Nurbs(_) => fitted.nurbs += 1,
        }
    }
    Shape::Surface { name: b.name, positions: b.positions, triangles: b.triangles, tags: b.tags, surfaces }
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
            // A plain number's parameter (its unit `none`) takes the number alone.
            let unit = if q.unit == "none" { "" } else { q.unit.as_str() };
            let expression = match value {
                Value::Number(n) => format!("{n} {unit}"),
                // An expression of the project's variables (`"=a/2"`): its number, in the
                // design's unit as a number given is (a template passes its parameters so).
                Value::String(e) if crate::expr::body(e).is_some() => format!("{} {unit}", p.number_of("parameters", value)?),
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
fn air_box(p: &Params, bodies: &mut [PartBody], margin: Option<f64>, thickness: Option<f64>, quarter: Option<f64>) -> Result<Shape> {
    let lo = lowest(bodies);
    let hi = bodies.iter().flat_map(|b| b.body.positions.iter()).fold([f64::NEG_INFINITY; 3], |m, q| [0, 1, 2].map(|d| m[d].max(q[d])));
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
    bodies.iter_mut().for_each(|b| b.moved(shift));
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
    Ok(json!({"features": s.model.features().len()}))
}

/// `model.region`: the region automatic round what the model holds (`auto`, its `margin` and
/// layer `thickness` a quarter wavelength at the band's top unless given) or pinned as it is.
pub fn region(s: &mut Session, p: &Params) -> Result<Value> {
    let mode = p.string("mode").required()?;
    match mode {
        "auto" => {
            s.model.domain.region =
                emcraft_model::Region::Auto { margin: p.metres("margin").optional()?, thickness: p.metres("thickness").optional()? };
            s.model.refresh_region()?;
        }
        "pinned" => {
            if p.has("margin") || p.has("thickness") {
                return Err(p.error("margin", "and `thickness` are an automatic region's"));
            }
            s.model.domain.region = emcraft_model::Region::Pinned;
        }
        other => return Err(p.error("mode", format!("must be `auto` or `pinned`, got `{}`", crate::text::clip(other, 40)))),
    }
    let b = s.model.domain.solid.as_ref().and_then(|x| x.shape.box_bounds());
    let l = |v: f64| s.display.length.from_si(v);
    Ok(json!({
        "mode": mode,
        "box": b.map(|b| json!([[l(b.min.x), l(b.min.y), l(b.min.z)], [l(b.max.x), l(b.max.y), l(b.max.z)]])),
        "layer": s.model.domain.open.map(|o| l(o.thickness.get())),
    }))
}

/// `model.background`: what fills the model where no body is (vacuum by default) — the
/// region's material.
pub fn background(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("material").required()?;
    s.model.material(name)?;
    s.model.domain.background = Some(name.to_owned());
    if let Some(solid) = s.model.domain.solid.as_mut() {
        name.clone_into(&mut solid.material);
        s.model.mesh = None;
    }
    Ok(json!({"background": name}))
}

pub fn solid(s: &mut Session, p: &Params) -> Result<Value> {
    s.model.set_solid(Solid { shape: shape(p)?, material: p.string("material").or("vacuum")?.to_owned() })?;
    Ok(json!({"volume": s.model.domain.solid.as_ref().map(|x| x.shape.volume())}))
}

pub fn walls(s: &mut Session, p: &Params) -> Result<Value> {
    let sigma = p.checked::<Conductivity>("conductivity").optional()?;
    let thickness = p.metres("thickness").optional()?;
    let roughness = p.metres("roughness").optional()?;
    if roughness.is_some() && sigma.is_none() {
        return Err(p.error("roughness", "needs a `conductivity`: a perfect conductor loses nothing, rough or smooth"));
    }
    s.model.set_wall_thickness(thickness)?;
    s.model.set_wall_roughness(roughness)?;
    s.model.set_wall_conductivity(sigma);
    Ok(json!({
        "conductivity": sigma,
        "thickness": thickness.map(|t| s.show_length(t.get())),
        "roughness": roughness.map(|h| s.show_length(h.get())),
    }))
}

pub fn symmetry(s: &mut Session, p: &Params) -> Result<Value> {
    let clear = p.flag("clear").or(false)?;
    let axis: Option<Axis> = p.choice("axis").optional()?;
    let mut planes = s.model.domain.symmetry.clone();
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
    let shown: Vec<Value> = s.model.domain.symmetry.iter().map(|q| json!({"axis": q.axis.id(), "at": s.show_length(q.at), "kind": q.kind})).collect();
    Ok(json!({"planes": shown, "fraction": s.model.symmetry_fraction()}))
}

/// `model.symmetry.find`.
/// `model.symmetry.auto`: every symmetry the model has, searched in one go and set where doing
/// so changes no answer — the mirror planes whose walls the ports decide (two at most); with no
/// port, the planes found with every combination of walls to solve (an eigenmode search merges
/// them: [`emcraft_model::Model::every_wall`]). What is found but would choose among the answers
/// is said with why, not set: the rotation about z (a sector keeps one harmonic of the whole).
/// The former planes and rotation are cleared first; finding nothing is no error.
pub fn auto_symmetry(s: &mut Session, _: &Params) -> Result<Value> {
    s.model.set_symmetry(Vec::new())?;
    s.model.set_rotation(None)?;
    let mut skipped: Vec<Value> = Vec::new();
    let found = s.model.find_symmetry();
    let mut planes: Vec<Symmetry> = found.iter().filter_map(|f| f.kind.map(|k| Symmetry { axis: f.axis, at: f.at, kind: k })).collect();
    for f in found.iter().filter(|f| f.kind.is_none()) {
        skipped.push(json!({"symmetry": format!("plane across {}", f.axis.id()), "why": "no port decides its wall: either wall keeps one family of modes (model.symmetry sets it)"}));
    }
    for extra in planes.iter().skip(2) {
        skipped.push(json!({"symmetry": format!("plane across {}", extra.axis.id()), "why": "two planes at most"}));
    }
    planes.truncate(2);
    planes.sort_by_key(|q| q.axis.index());
    if let Some(r) = s.model.find_rotation() {
        skipped.push(json!({"symmetry": format!("rotation about z ({})", r.group()), "why": "a sector keeps one harmonic of the whole (model.rotation sets it)"}));
    }
    // No port to decide them: an eigenmode search solves every combination of walls and merges
    // its modes (nothing is lost); with ports, the planes they decide only.
    let mut every_wall = false;
    if planes.is_empty() && s.model.ports.is_empty() && !found.is_empty() {
        let mut all: Vec<Symmetry> = found.iter().map(|f| Symmetry { axis: f.axis, at: f.at, kind: SymmetryKind::Magnetic }).collect();
        all.truncate(2);
        all.sort_by_key(|q| q.axis.index());
        // The planes set are no longer left out; the others are, two being the most.
        skipped.retain(|x| !x["symmetry"].as_str().is_some_and(|w| w.starts_with("plane across ")));
        for f in found.iter().filter(|f| !all.iter().any(|q| q.axis == f.axis)) {
            skipped.push(json!({"symmetry": format!("plane across {}", f.axis.id()), "why": "two planes at most"}));
        }
        s.model.set_symmetry_every_wall(all)?;
        every_wall = true;
    } else if !planes.is_empty() {
        s.model.set_symmetry(planes)?;
    }
    let shown: Vec<Value> = s.model.domain.symmetry.iter().map(|q| json!({"axis": q.axis.id(), "at": s.show_length(q.at), "kind": q.kind})).collect();
    Ok(json!({"planes": shown, "fraction": s.model.symmetry_fraction(), "every_wall": every_wall, "skipped": skipped}))
}

pub fn find_symmetry(s: &mut Session, p: &Params) -> Result<Value> {
    let kind: Option<SymmetryKind> = p.choice("kind").optional()?;
    // Planes a model cannot carry yet are said as such, not as an asymmetric model (an open
    // horn is symmetric, its PML box too).
    let m = &s.model;
    if m.domain.open.is_some() || !m.lumped.is_empty() || !m.sources.is_empty() {
        return Err(EngineError::failed(
            "not supported yet: symmetry planes in an open problem or with lumped ports or sources (the whole model is solved)",
        ));
    }
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
    let shown: Vec<Value> = s.model.domain.symmetry.iter().map(|q| json!({"axis": q.axis.id(), "at": s.show_length(q.at), "kind": q.kind})).collect();
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
    // A design's face by its name: found again wherever the design now puts it (ADR 0010).
    let named = match p.string("face_name").optional()? {
        Some(_) if face.is_some() => return Err(p.error("face_name", "give `face_name` or `face`, not both")),
        Some(n) => Some(
            emcraft_model::faces::port_face(&s.model, n)
                .map_err(|why| p.error("face_name", format!("port `{}`: {why}", crate::text::clip(name, 64))))?,
        ),
        None => None,
    };
    let face = face.or(named);
    // The side the guide lies on: as given, else (a named face) where the model is.
    let end: End = match (p.choice("end").optional()?, named) {
        (Some(e), _) => e,
        (None, Some(f)) => guide_side(&s.model, &f),
        (None, None) => return Err(p.error("end", "required: `start` or `end`")),
    };
    // An imported cavity's channel along x or y: its end is its opening there.
    let face = match face {
        Some(f) => Some(f),
        None => opening(&s.model, end).map_err(|why| p.error("end", why))?,
    };
    let port = Port {
        name: name.to_owned(),
        end,
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

/// The side of face `f` the guide lies on: `start` where the model is beyond it along its axis,
/// else `end`.
fn guide_side(m: &emcraft_model::Model, f: &PortFace) -> End {
    let FrameAxes { u, v, .. } = emcraft_model::port::Frame::new(f.axis).axes();
    let k = f.axis.index();
    let span = m.bounds().map_or(1.0, |b| (0..3).map(|d| b.max[d] - b.min[d]).fold(1e-9, f64::max));
    let mut q = Point3::ORIGIN;
    q[u] = f.u0 + f.a.get() / 2.0;
    q[v] = f.v0 + f.b.get() / 2.0;
    q[k] = f.at + 1e-4 * span;
    if m.material_at(q).is_some() { End::Start } else { End::End }
}

/// The opening of an imported cavity a port at `end` goes on when its channel runs along x or
/// y (none along z, or for any other region: the end face across z is the port's); refused
/// when the cavity opens on that side across several axes.
fn opening(m: &emcraft_model::Model, end: End) -> std::result::Result<Option<PortFace>, String> {
    let openings = m.openings();
    if openings.is_empty() || openings.iter().any(|(_, f)| f.axis == Axis::Z) {
        return Ok(None);
    }
    let here: Vec<PortFace> = openings.iter().filter(|(e, _)| *e == end).map(|(_, f)| *f).collect();
    match here[..] {
        [f] => Ok(Some(f)),
        [] => Err(format!("the cavity has no opening at its {} (its openings: {}); give `face` and `axis`", end.id(), shown_openings(&openings))),
        _ => Err(format!("the cavity opens at its {} across several axes ({}); give `face` and `axis`", end.id(), shown_openings(&openings))),
    }
}

fn shown_openings(o: &[(End, PortFace)]) -> String {
    o.iter().map(|(e, f)| format!("{} {}", f.axis.id(), e.id())).collect::<Vec<_>>().join(", ")
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
    match s.model.domain.rotation {
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

#[cfg(test)]
mod tests {
    use emcraft_model::{FaceSurface, Point3, Vec3};

    use super::PartBody;

    /// A body moves with its exact surfaces (the box of air shifts both).
    #[test]
    fn a_part_moves_with_its_exact_surfaces() {
        let body = emcraft_io::Surface { name: "post".into(), positions: vec![Point3::new(1.0, 0.0, 0.0)], triangles: Vec::new(), tags: Vec::new() };
        let post = FaceSurface::Cylinder { origin: Point3::ORIGIN, axis: Vec3::unit(2), radius: 1.0 };
        let mut part = PartBody { body, source: Vec::new(), exact: Some(vec![Some(post), None]), names: Vec::new() };
        let d = Vec3::new(0.03, 0.02, 0.01);
        part.moved(d);
        assert_eq!(part.body.positions, vec![Point3::new(1.03, 0.02, 0.01)]);
        let moved = FaceSurface::Cylinder { origin: Point3::new(0.03, 0.02, 0.01), axis: Vec3::unit(2), radius: 1.0 };
        assert_eq!(part.exact, Some(vec![Some(moved), None]));
    }
}

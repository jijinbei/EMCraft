//! Commands that build the model: materials, the waveguide, solids, imported surfaces,
//! inclusions and features, walls and ports.

use emcraft_model::{
    Axis, FaceSurface, Feature, Inclusion, Layer, MAX_POLES, Material, Point3, Pole, Port, PortFace, Section, Shape, Solid, Strip, Symmetry,
    SymmetryKind, Via, Waveguide,
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
                let delta_eps = d.positive("delta_eps").required()?;
                let f = d.frequency("frequency").required()?;
                poles.push(Pole::Debye { delta_eps, tau: 1.0 / (2.0 * std::f64::consts::PI * f) });
            }
        }
        "lorentz" => {
            for l in p.items("lorentz", "Lorentz pole", MAX_POLES)? {
                let delta_eps = l.positive("delta_eps").required()?;
                let frequency = l.frequency("frequency").required()?;
                let damping = l.frequency("damping").required()?;
                poles.push(Pole::Lorentz { delta_eps, frequency, damping });
            }
        }
        _ => {
            for d in p.items("drude", "Drude pole", MAX_POLES)? {
                let plasma_frequency = d.frequency("plasma_frequency").required()?;
                let damping = d.frequency("damping").required()?;
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
    let width = p.positive_length("width").required()?;
    let height = p.positive_length("height").required()?;
    p.list("sections", MAX_ITEMS).required()?;
    let sections = p
        .items("sections", "section", MAX_ITEMS)?
        .iter()
        .map(|sec| {
            Ok(Section {
                length: sec.positive_length("length").required()?,
                material: sec.string("material").required()?.to_owned(),
                width: sec.positive_length("width").optional()?,
                offset: sec.length("offset").optional()?,
                height: sec.positive_length("height").optional()?,
                offset_y: sec.length("offset_y").optional()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let total: f64 = sections.iter().map(|x| x.length).sum();
    let layers = p
        .items("layers", "layer", MAX_ITEMS)?
        .iter()
        .map(|l| Ok(Layer { thickness: l.positive_length("thickness").required()?, material: l.string("material").required()?.to_owned() }))
        .collect::<Result<Vec<_>>>()?;
    let strips = p
        .items("strips", "strip", MAX_ITEMS)?
        .iter()
        .map(|st| {
            Ok(Strip {
                y: st.length("y").required()?,
                x0: st.length("x0").required()?,
                width: st.positive_length("width").required()?,
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
                wx: v.positive_length("wx").required()?,
                wz: v.positive_length("wz").required()?,
                y0: v.length("y0").or(0.0)?,
                y1: v.length("y1").or(height)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    s.model.set_waveguide(Waveguide { width, height, sections, layers, strips, vias })?;
    Ok(json!({"length": s.show_length(s.model.waveguide.as_ref().map_or(0.0, Waveguide::length))}))
}

/// `geometry.iris_filter`: a rectangular waveguide with a row of inductive irises.
pub fn iris_filter(s: &mut Session, p: &Params) -> Result<Value> {
    let width = p.positive_length("width").required()?;
    let height = p.positive_length("height").required()?;
    let end_width = p.positive_length("end_width").required()?;
    let cavity = p.positive_length("cavity").required()?;
    let f = emcraft_templates::IrisFilter {
        irises: p.integer("irises", 2..=emcraft_templates::MAX_IRISES).required()?,
        iris_length: p.positive_length("iris_length").required()?,
        end_width,
        inner_width: p.positive_length("inner_width").or(end_width)?,
        cavity,
        inner_cavity: p.positive_length("inner_cavity").or(cavity)?,
        feed: p.positive_length("feed").required()?,
        material: p.string("material").or("vacuum")?.to_owned(),
    };
    if f.end_width.max(f.inner_width) > width {
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
    }
}

emcraft_macros::named_serde!(Primitive);

/// A shape from `shape`, `size`, `radius`, `length` and `points`.
pub(crate) fn shape(p: &Params) -> Result<Shape> {
    let dimension = |name| p.positive_length(name).required_or("for this shape");
    Ok(match p.choice("shape").required()? {
        Primitive::Box => Shape::Box { size: p.point("size").required()?.to_vec() },
        Primitive::Cylinder => Shape::Cylinder { radius: dimension("radius")?, length: dimension("length")? },
        Primitive::Sphere => Shape::Sphere { radius: dimension("radius")? },
        Primitive::Prism => Shape::prism(&p.pairs("points", emcraft_model::MAX_PRISM_POINTS).required_or("for a prism")?, dimension("length")?)?,
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

pub fn import(s: &mut Session, p: &Params) -> Result<Value> {
    let path = p.string("path").required()?;
    let unit = s.display.length.to_si(p.positive("unit").or(1.0)?);
    let angle = p.number("angle").check(|a| *a > 0.0 && *a < 180.0, "between 0 and 180 degrees").or(30.0)?;
    let material = p.string("material").or("vacuum")?.to_owned();
    let bytes = read_capped(path, MAX_IMPORT_BYTES)?;
    let mut bodies = emcraft_io::read_bodies(path, &bytes, unit)?;
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
    let quarter = s.model.band.map(|b| emcraft_model::units::C0 / b.start / 4.0);
    let thickness = match (open, p.positive_length("thickness").optional()?) {
        (false, _) => None,
        (true, Some(t)) => Some(t),
        (true, None) => Some(quarter.ok_or_else(|| {
            p.error("thickness", "or set the band (project.frequency): the layer and the air before it are a quarter wavelength each")
        })?),
    };
    // The region the fields fill: one body alone (or the one named), else a box of air around
    // them all (always for an open problem).
    let region = p
        .string("region")
        .optional()?
        .map(str::to_owned)
        .unwrap_or_else(|| if bodies.len() == 1 && !open { bodies[0].name.clone() } else { "box".into() });
    if open && region != "box" {
        return Err(p.error("region", "an open problem lies in a box of air (`box`)"));
    }
    let keep: Option<Keep> = p.choice("keep").optional()?;
    if region != "cavity" && keep.is_some() {
        return Err(p.error("keep", "chooses pieces of a cavity (`region`: `cavity`)"));
    }
    let mut cavity = None;
    let mut placed = Vec::new();
    if region == "cavity" {
        if p.has("margin") {
            return Err(p.error("margin", "a cavity is the air inside the bodies' own bounding box: it takes no margin"));
        }
        cavity = Some(enclosed_air(s, &mut bodies, &materials, keep.unwrap_or_default(), &material)?);
        // The metal bodies are the cavity's walls now; the others are placed in it.
        let metal = |b: &emcraft_io::Surface| material_of(&materials, &b.name) == crate::bodies::METAL;
        placed.extend(bodies.iter().filter(|b| metal(b)).map(|b| json!({"body": b.name, "material": crate::bodies::METAL, "as": "walls"})));
        let keep: Vec<bool> = bodies.iter().map(|b| !metal(b)).collect();
        bodies.retain(|b| !metal(b));
        let mut k = keep.iter();
        sources.retain(|_| k.next().copied().unwrap_or(false));
    } else if region == "box" {
        let margin = p.positive_length("margin").optional()?;
        s.model.set_solid(Solid { shape: air_box(p, &mut bodies, margin, thickness, quarter)?, material: material.clone() })?;
    } else {
        let k = bodies.iter().position(|b| b.name == region).ok_or_else(|| p.error("region", format!("names no body `{region}` (or `box`)")))?;
        let body = bodies.remove(k);
        let source = sources.remove(k);
        s.model.set_solid(Solid { shape: surface(body, &source, fit, &mut fitted), material: material.clone() })?;
    }
    for (b, source) in bodies.into_iter().zip(sources) {
        let m = material_of(&materials, &b.name).to_owned();
        let name = b.name.clone();
        let kind = crate::bodies::place(&mut s.model, surface(b, &source, fit, &mut fitted), Point3::ORIGIN, Axis::Z, &m)?;
        placed.push(json!({"body": name, "material": m, "as": kind}));
    }
    if let Some(t) = thickness {
        s.model.set_open(Some(emcraft_model::Open { thickness: t, reflection: None, shape: emcraft_model::OpenShape::Box }))?;
    }
    let bounds = s.model.bounds().unwrap_or_default();
    let unit = s.display.length.to_si(1.0);
    let pieces = cavity
        .map(|c: emcraft_mesh::cavity::Cavity| json!({"kept": c.kept.iter().map(|v| v / unit.powi(3)).collect::<Vec<_>>(), "dropped": c.dropped}));
    Ok(json!({
        "triangles": triangles,
        "faces": faces,
        "planes": fitted.plane,
        "curved": {"cylinder": fitted.cylinder, "sphere": fitted.sphere, "cone": fitted.cone, "torus": fitted.torus},
        "region": region,
        "pieces": pieces,
        "bodies": placed,
        "open": thickness.map(|t| s.show_length(t)),
        "volume": s.model.solid.as_ref().map(|x| x.shape.volume()),
        "bounds": bounds.corners().map(|c| c.map(|v| s.show_length(v))),
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
/// across x and y kept (`emcraft_mesh::cavity`), made the region; the bodies (all of them, the
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
/// one fits, `source` the triangles' faces before they were split by angle.
fn surface(b: emcraft_io::Surface, source: &[u32], fit: bool, fitted: &mut Fitted) -> Shape {
    let surfaces = if fit { emcraft_io::fit::fit_faces(&b, source) } else { Vec::new() };
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
    Ok(Shape::Box { size })
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

pub fn solid(s: &mut Session, p: &Params) -> Result<Value> {
    let shape = shape(p)?;
    if let Shape::Box { size } = shape
        && size.iter().any(|v| !(v > 0.0))
    {
        return Err(p.error("size", "must be positive"));
    }
    s.model.set_solid(Solid { shape, material: p.string("material").or("vacuum")?.to_owned() })?;
    Ok(json!({"volume": s.model.solid.as_ref().map(|x| x.shape.volume())}))
}

pub fn walls(s: &mut Session, p: &Params) -> Result<Value> {
    let sigma = p.number("conductivity").optional()?;
    s.model.set_wall_conductivity(sigma)?;
    Ok(json!({"conductivity": sigma}))
}

emcraft_macros::named_enum! {
    /// A plane's axis for `model.symmetry` (z is not one: the ports lie on z planes).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(crate) enum PlaneAxis {
        X = "x",
        Y = "y",
    }
}

emcraft_macros::named_serde!(PlaneAxis);

pub fn symmetry(s: &mut Session, p: &Params) -> Result<Value> {
    let clear = p.flag("clear").or(false)?;
    let axis = p.choice::<PlaneAxis>("axis").optional()?.map(|a| match a {
        PlaneAxis::X => Axis::X,
        PlaneAxis::Y => Axis::Y,
    });
    let mut planes = s.model.symmetry.clone();
    if clear {
        if p.has("kind") || p.has("at") {
            return Err(p.error("clear", "takes only `axis` (the plane to remove; all of them without it)"));
        }
        planes.retain(|q| axis.is_some_and(|a| q.axis != a));
    } else {
        let axis = axis.ok_or_else(|| p.error("axis", "is required (or `clear`)"))?;
        let kind: SymmetryKind = p.choice("kind").required()?;
        // The guide's middle by default: the only plane its walls are symmetric about.
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

pub fn port(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    let axis: Option<Axis> = p.choice("axis").optional()?;
    let face = p
        .lengths("face")
        .and_then(|f| match f[..] {
            [at, u0, v0, a, b] => Ok(PortFace { axis: axis.unwrap_or_default(), at, u0, v0, a, b }),
            _ => Err(p.error("face", "must be [at, u0, v0, a, b]")),
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
        reference: p.number("impedance").optional()?,
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
        "delta": [l.from_si(m.delta.x), l.from_si(m.delta.y), l.from_si(m.delta.z)],
        "gap": m.gap.map(|g| l.from_si(g)),
        "angle": m.angle,
        "unit": l.symbol(),
    }))
}

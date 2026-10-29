//! 3MF packages (3MF Core Specification 1.3): the mesh objects of the build items, placed by
//! their transforms and scaled to metres. Triangles may carry SolidCraft's face tags
//! (`ft:face`, namespace [`FACE_TAGS_NS`]); untagged objects are one face each.
//!
//! Objects made of components (Core §4.2: `<components><component objectid transform/>`) are
//! flattened: each component's object placed by the product of the transforms down to it
//! (Core §3.3: row vectors, `p' = p·M`, so a component's transform applies before its
//! parent's), its faces renumbered after the faces before it, so every placed face stays a
//! face of its own. The production extension's `p:path` (3MF Production Extension 1.1 §2.1)
//! on a build item or component names the model part the object lives in; those parts are
//! read too. Component trees are bounded in depth, in the parts they reach and in the
//! placements they make, and a component that refers back to an object above it is refused.
//!
//! Ported from FieldCraft's `fieldcraft-geometry::tagged_mesh` (MIT OR Apache-2.0, see
//! ATTRIBUTION.md), its limits kept: packages are untrusted, so the file, its parts, zip entries,
//! XML nesting, vertex and triangle counts and build items are capped, coordinates must be
//! finite and bounded, and every index is range-checked.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{Cursor, Read};

use quick_xml::NsReader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;

use emcraft_model::Point3;

use crate::{Error, MAX_COORD, Result, Surface};

/// Largest package read.
pub const MAX_FILE_BYTES: usize = 512 << 20;
/// Largest uncompressed part read (zip bombs stop here).
const MAX_PART: u64 = 1 << 30;
const MAX_ENTRIES: usize = 10_000;
/// Most vertices, and most triangles, in a file and in all the surfaces read from it.
pub const MAX_ELEMENTS: usize = crate::MAX_TRIANGLES;
const MAX_BUILD_ITEMS: usize = 10_000;
const MAX_XML_DEPTH: usize = 256;
/// Deepest nesting of components below a build item.
const MAX_COMPONENT_DEPTH: usize = 32;
/// Most objects visited while flattening every build item (a tree of empty objects that
/// doubles at each level stops here even though it places nothing).
const MAX_COMPONENT_VISITS: usize = 1_000_000;
/// Most model parts read from one package (the root and the ones `p:path` names).
const MAX_MODEL_PARTS: usize = 64;
/// Components in one object.
const MAX_COMPONENTS: usize = 100_000;
/// A transform whose determinant is below this fraction of its largest row length cubed
/// flattens the object (relative, so a uniform scale is never refused).
const MIN_RELATIVE_DET: f64 = 1e-12;

const CORE_NS: &str = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";
const PRODUCTION_NS: &str = "http://schemas.microsoft.com/3dmanufacturing/production/2015/06";
/// SolidCraft's face-tag namespace, version 1.
pub const FACE_TAGS_NS: &str = "https://github.com/bherbruck/solidcraft/formats/3mf-face-tags/2026/10";

fn invalid(msg: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("3MF: {msg}"))
}

fn bad<T>(msg: impl std::fmt::Display) -> Result<T> {
    Err(invalid(msg))
}

fn read_part(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Option<Vec<u8>>> {
    let want = name.trim_start_matches('/').to_ascii_lowercase();
    let Some(idx) = (0..zip.len())
        .find(|i| zip.name_for_index(*i).is_some_and(|n| n.is_ok_and(|n| n.replace('\\', "/").trim_start_matches('/').to_ascii_lowercase() == want)))
    else {
        return Ok(None);
    };
    let mut f = zip.by_index(idx).map_err(|e| invalid(format!("{name}: {e}")))?;
    // The declared size only sizes the first allocation (it may lie); the read itself is capped.
    let mut out = Vec::with_capacity(usize::try_from(f.size().min(16 << 20)).unwrap_or(0));
    (&mut f).take(MAX_PART + 1).read_to_end(&mut out).map_err(|e| invalid(format!("{name}: {e}")))?;
    if out.len() as u64 > MAX_PART {
        return bad(format!("{name} expands to more than 1 GB"));
    }
    Ok(Some(out))
}

/// Which namespace an element or attribute name is in.
#[derive(Clone, Copy, PartialEq)]
enum Ns {
    /// 3MF Core, or no namespace at all (accepted for lenience).
    Core,
    FaceTags,
    Production,
    Other,
}

fn ns_of(r: &ResolveResult) -> Ns {
    match r {
        ResolveResult::Bound(n) if n.as_ref() == CORE_NS => Ns::Core,
        ResolveResult::Bound(n) if n.as_ref() == FACE_TAGS_NS => Ns::FaceTags,
        ResolveResult::Bound(n) if n.as_ref() == PRODUCTION_NS => Ns::Production,
        ResolveResult::Unbound => Ns::Core,
        _ => Ns::Other,
    }
}

/// An element's attributes as (namespace, local name, value), borrowed from the event (values
/// are copied only when they hold entity references). Unprefixed attributes are Core.
type Attrs<'a> = Vec<(Ns, &'a str, Cow<'a, str>)>;

fn attrs<'a, R>(rd: &NsReader<R>, e: &'a BytesStart) -> Result<Attrs<'a>> {
    let mut out = Vec::with_capacity(4);
    for a in e.attributes() {
        let a = a.map_err(|err| invalid(format!("model XML: {err}")))?;
        let (ns, local) = rd.resolver().resolve_attribute(a.key);
        let value = a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map_err(|err| invalid(format!("model XML: {err}")))?;
        out.push((ns_of(&ns), local.into_inner(), value));
    }
    Ok(out)
}

fn get<'s>(a: &'s Attrs, ns: Ns, key: &str) -> Option<&'s str> {
    a.iter().find(|(n, k, _)| *n == ns && *k == key).map(|(_, _, v)| v.as_ref())
}

fn int(a: &Attrs, ns: Ns, key: &str, what: &str) -> Result<Option<u32>> {
    match get(a, ns, key) {
        None => Ok(None),
        Some(v) => v.trim().parse::<u32>().map(Some).map_err(|_| invalid(format!("{what} `{key}` is not a whole number: `{v}`"))),
    }
}

fn coord(a: &Attrs, key: &str) -> Result<f64> {
    let v = get(a, Ns::Core, key).ok_or_else(|| invalid(format!("vertex without `{key}`")))?;
    v.trim().parse::<f64>().ok().filter(|x| x.is_finite() && x.abs() <= MAX_COORD).ok_or_else(|| invalid(format!("bad coordinate `{v}`")))
}

/// Row-vector affine transform `[m00 m01 m02 m10 … m32]` (3MF order).
type Xf = [f64; 12];
const IDENTITY: Xf = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

fn parse_xf(s: Option<&str>) -> Result<Xf> {
    let Some(s) = s else { return Ok(IDENTITY) };
    let v: Vec<f64> = s
        .split_whitespace()
        .take(13)
        .map(|t| t.parse::<f64>().ok().filter(|x| x.is_finite() && x.abs() <= MAX_COORD))
        .collect::<Option<_>>()
        .ok_or_else(|| invalid(format!("bad transform `{s}`")))?;
    <Xf>::try_from(v.as_slice()).map_err(|_| invalid(format!("a transform has 12 numbers, got `{s}`")))
}

fn apply(m: &Xf, p: Point3) -> Point3 {
    Point3::new(
        p.x * m[0] + p.y * m[3] + p.z * m[6] + m[9],
        p.x * m[1] + p.y * m[4] + p.z * m[7] + m[10],
        p.x * m[2] + p.y * m[5] + p.z * m[8] + m[11],
    )
}

/// The transform doing `inner` first, then `outer` (row vectors: `p·A₁ + t₁`, then `·A₂ + t₂`).
fn compose(outer: &Xf, inner: &Xf) -> Xf {
    let mut m = [0.0; 12];
    for r in 0..4 {
        for c in 0..3 {
            let lin = (0..3).map(|k| inner[r * 3 + k] * outer[k * 3 + c]).sum::<f64>();
            m[r * 3 + c] = if r == 3 { lin + outer[9 + c] } else { lin };
        }
    }
    m
}

fn det(m: &Xf) -> f64 {
    m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6]) + m[2] * (m[3] * m[7] - m[4] * m[6])
}

/// Metres per 3MF model unit (3MF Core §3.4.1, the inch and foot exact by definition).
fn unit_m(u: &str) -> Option<f64> {
    Some(match u {
        "micron" => 1e-6,
        "millimeter" => 1e-3,
        "centimeter" => 1e-2,
        "inch" => 0.0254,
        "foot" => 0.3048,
        "meter" => 1.0,
        _ => return None,
    })
}

#[derive(Clone, Default)]
struct Object {
    name: String,
    positions: Vec<Point3>,
    triangles: Vec<[u32; 3]>,
    /// One per triangle, or empty when the object is untagged.
    tags: Vec<u32>,
    faces: Option<u32>,
    /// The components when the object is made of them.
    components: Vec<Component>,
}

/// A placement of an object: a component, or a build item.
#[derive(Clone)]
struct Component {
    id: u32,
    xf: Xf,
    /// The model part the object lives in (`p:path`), else the referring part.
    path: Option<String>,
}

/// A model part's key: its path without the leading slash, case folded (zip names are matched
/// so too).
fn part_key(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches('/').to_ascii_lowercase()
}

impl Object {
    /// Checks that need the whole object: tagging is all or nothing, indices exist.
    fn check(&self, id: u32) -> Result<()> {
        if !self.tags.is_empty() && self.tags.len() != self.triangles.len() {
            return bad(format!("object {id}: {} of {} triangles have a face tag", self.tags.len(), self.triangles.len()));
        }
        if !self.components.is_empty() && !self.triangles.is_empty() {
            return bad(format!("object {id} holds both a mesh and components"));
        }
        let n = self.positions.len();
        if let Some(v) = self.triangles.iter().flatten().find(|v| **v as usize >= n) {
            return bad(format!("object {id}: a triangle refers to vertex {v}, but there are {n}"));
        }
        Ok(())
    }
}

#[derive(Default)]
struct Model {
    unit: f64,
    objects: HashMap<u32, Object>,
    build: Vec<Component>,
    /// Vertices or triangles read, whichever more (counted against the limits across parts).
    elements: usize,
}

impl Model {
    fn add(&mut self, id: u32, o: Object) -> Result<()> {
        o.check(id)?;
        self.objects.insert(id, o);
        Ok(())
    }
}

fn parse_model(xml: &[u8]) -> Result<Model> {
    let mut rd = NsReader::from_reader(xml);
    rd.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut m = Model { unit: 1e-3, ..Default::default() };
    let mut depth = 0usize;
    let mut cur: Option<(u32, Object)> = None;
    let (mut vertices, mut triangles) = (0usize, 0usize);
    let mut seen_model = false;
    loop {
        buf.clear();
        let (ns, ev) = rd.read_resolved_event_into(&mut buf).map_err(|e| invalid(format!("model XML: {e}")))?;
        let ns = ns_of(&ns);
        let (e, empty) = match ev {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(end) => {
                depth = depth.saturating_sub(1);
                if ns == Ns::Core
                    && end.local_name().as_ref() == "object"
                    && let Some((id, o)) = cur.take()
                {
                    m.add(id, o)?;
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        if !empty {
            depth += 1;
            if depth > MAX_XML_DEPTH {
                return bad("XML nested too deeply");
            }
        }
        if ns != Ns::Core {
            continue;
        }
        let a = attrs(&rd, &e)?;
        match e.local_name().as_ref() {
            "model" => {
                seen_model = true;
                if let Some(u) = get(&a, Ns::Core, "unit") {
                    m.unit = unit_m(u).ok_or_else(|| invalid(format!("unknown unit `{u}`")))?;
                }
            }
            "object" => {
                if cur.is_some() {
                    return bad("an object inside an object");
                }
                let id = int(&a, Ns::Core, "id", "object")?.ok_or_else(|| invalid("object without id"))?;
                if m.objects.contains_key(&id) {
                    return bad(format!("two objects with id {id}"));
                }
                let faces = int(&a, Ns::FaceTags, "faces", "object")?;
                if faces == Some(0) {
                    return bad(format!("object {id} has ft:faces=\"0\""));
                }
                let o = Object { name: get(&a, Ns::Core, "name").unwrap_or_default().trim().to_string(), faces, ..Default::default() };
                if empty {
                    m.add(id, o)?;
                } else {
                    cur = Some((id, o));
                }
            }
            "vertex" => {
                let Some((_, o)) = cur.as_mut() else { return bad("vertex outside an object") };
                vertices += 1;
                if vertices > MAX_ELEMENTS {
                    return bad(format!("more than {MAX_ELEMENTS} vertices"));
                }
                o.positions.push(Point3::new(coord(&a, "x")?, coord(&a, "y")?, coord(&a, "z")?));
            }
            "triangle" => {
                let Some((id, o)) = cur.as_mut() else { return bad("triangle outside an object") };
                triangles += 1;
                if triangles > MAX_ELEMENTS {
                    return bad(format!("more than {MAX_ELEMENTS} triangles"));
                }
                let mut t = [0u32; 3];
                for (slot, k) in t.iter_mut().zip(["v1", "v2", "v3"]) {
                    *slot = int(&a, Ns::Core, k, "triangle")?.ok_or_else(|| invalid(format!("triangle without `{k}`")))?;
                }
                if let Some(f) = int(&a, Ns::FaceTags, "face", "triangle")? {
                    if let Some(n) = o.faces.filter(|n| f >= *n) {
                        return bad(format!("object {id}: face tag {f} is not below ft:faces=\"{n}\""));
                    }
                    o.tags.push(f);
                }
                o.triangles.push(t);
            }
            "component" => {
                let Some((id, o)) = cur.as_mut() else { return bad("component outside an object") };
                if o.components.len() >= MAX_COMPONENTS {
                    return bad(format!("object {id} has more than {MAX_COMPONENTS} components"));
                }
                let target = int(&a, Ns::Core, "objectid", "component")?.ok_or_else(|| invalid("component without objectid"))?;
                o.components.push(Component { id: target, xf: parse_xf(get(&a, Ns::Core, "transform"))?, path: path_attr(&a)? });
            }
            "item" => {
                if m.build.len() >= MAX_BUILD_ITEMS {
                    return bad(format!("more than {MAX_BUILD_ITEMS} build items"));
                }
                let target = int(&a, Ns::Core, "objectid", "build item")?.ok_or_else(|| invalid("build item without objectid"))?;
                m.build.push(Component { id: target, xf: parse_xf(get(&a, Ns::Core, "transform"))?, path: path_attr(&a)? });
            }
            _ => {}
        }
    }
    if !seen_model {
        return bad("no <model> element");
    }
    if cur.is_some() {
        return bad("the model ends inside an object");
    }
    m.elements = vertices.max(triangles);
    Ok(m)
}

/// The production extension's `p:path` (a model part of the same package), checked.
fn path_attr(a: &Attrs) -> Result<Option<String>> {
    let Some(p) = get(a, Ns::Production, "path") else { return Ok(None) };
    let p = p.trim();
    if p.is_empty() || p.len() > 1024 || p.split(['/', '\\']).any(|s| s == "..") {
        return bad(format!("bad p:path `{p}`"));
    }
    Ok(Some(p.to_string()))
}

/// The parsed model parts of a package by [`part_key`], and the root's key.
struct Package {
    root: String,
    parts: HashMap<String, Model>,
}

/// One build item being flattened into a surface.
#[derive(Default)]
struct Flat {
    name: Option<String>,
    positions: Vec<Point3>,
    triangles: Vec<[u32; 3]>,
    tags: Vec<u32>,
    /// The next face tag free.
    faces: u32,
}

/// Running counts over every build item (the caps on placements and visits).
#[derive(Default)]
struct Budget {
    elements: usize,
    visits: usize,
}

impl Package {
    /// Places object `id` of part `part` by `xf` into `out`, recursing into its components.
    /// `stack` holds the (part, object) pairs above it, so a cycle is found.
    fn place(&self, part: &str, id: u32, xf: &Xf, out: &mut Flat, stack: &mut Vec<(String, u32)>, budget: &mut Budget) -> Result<()> {
        budget.visits += 1;
        if budget.visits > MAX_COMPONENT_VISITS {
            return bad(format!("the components place objects more than {MAX_COMPONENT_VISITS} times"));
        }
        if stack.len() > MAX_COMPONENT_DEPTH {
            return bad(format!("components nested more than {MAX_COMPONENT_DEPTH} deep"));
        }
        if stack.iter().any(|(p, o)| p == part && *o == id) {
            return bad(format!("object {id} is a component of itself"));
        }
        let model = self.parts.get(part).ok_or_else(|| invalid(format!("no model part `{part}`")))?;
        let o = model.objects.get(&id).ok_or_else(|| invalid(format!("refers to missing object {id}")))?;
        if out.name.is_none() && !o.name.is_empty() {
            out.name = Some(o.name.clone());
        }
        if !o.components.is_empty() {
            stack.push((part.to_string(), id));
            for c in &o.components {
                let sub = c.path.as_deref().map_or_else(|| part.to_string(), part_key);
                self.place(&sub, c.id, &compose(xf, &c.xf), out, stack, budget)?;
            }
            stack.pop();
            return Ok(());
        }
        if !xf.iter().all(|c| c.is_finite() && c.abs() <= MAX_COORD) {
            return bad(format!("object {id}: the product of its transforms is out of range"));
        }
        let d = det(xf);
        let scale = (0..3).map(|r| (xf[r * 3].powi(2) + xf[r * 3 + 1].powi(2) + xf[r * 3 + 2].powi(2)).sqrt()).fold(0.0, f64::max);
        if !(d.abs() > MIN_RELATIVE_DET * scale.powi(3)) {
            return bad(format!("its transform is singular (it flattens object {id} to zero volume)"));
        }
        budget.elements = budget.elements.saturating_add(o.positions.len().max(o.triangles.len()));
        if budget.elements > MAX_ELEMENTS {
            return bad(format!("the build items place more than {MAX_ELEMENTS} vertices or triangles"));
        }
        let off = u32::try_from(out.positions.len()).map_err(|_| invalid("too many vertices"))?;
        for p in &o.positions {
            let q = apply(xf, *p).map(|c| c * model.unit);
            if !q.iter().all(|c| c.is_finite() && c.abs() <= MAX_COORD) {
                return bad(format!("object {id}: a placed vertex is out of range"));
            }
            out.positions.push(q);
        }
        for t in &o.triangles {
            let t = t.map(|v| v + off);
            out.triangles.push(if d < 0.0 { [t[0], t[2], t[1]] } else { t });
        }
        let base = out.faces;
        if o.tags.is_empty() {
            out.tags.extend(std::iter::repeat_n(base, o.triangles.len()));
            out.faces = base.saturating_add(u32::from(!o.triangles.is_empty()));
        } else {
            out.tags.extend(o.tags.iter().map(|t| base.saturating_add(*t)));
            let used = o.faces.unwrap_or(0).max(o.tags.iter().max().map_or(0, |t| t + 1));
            out.faces = base.saturating_add(used);
        }
        Ok(())
    }

    /// Build item `k` of the root as a surface.
    fn item(&self, k: usize, c: &Component, budget: &mut Budget) -> Result<Surface> {
        let part = c.path.as_deref().map_or_else(|| self.root.clone(), part_key);
        let mut out = Flat::default();
        self.place(&part, c.id, &c.xf, &mut out, &mut Vec::new(), budget)
            .map_err(|e| invalid(format!("build item {}: {}", k + 1, e.to_string().trim_start_matches("3MF: "))))?;
        if out.triangles.is_empty() {
            return bad(format!("build item {}: object {} has no triangles", k + 1, c.id));
        }
        let name = out.name.unwrap_or_else(|| format!("Mesh{}", k + 1));
        Ok(Surface { name, positions: out.positions, triangles: out.triangles, tags: out.tags })
    }
}

/// A 3MF package of `bodies` (metres), one object and build item each, named after them, in
/// millimetres, every triangle tagged with its face (SolidCraft's face tags, version 1).
pub fn write(bodies: &[Surface]) -> Result<Vec<u8>> {
    use std::fmt::Write as _;
    use std::io::Write as _;
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\" xmlns:ft=\"",
    );
    xml.push_str(FACE_TAGS_NS);
    xml.push_str("\">\n <resources>\n");
    let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;");
    for (k, b) in bodies.iter().enumerate() {
        let faces = b.tags.iter().max().map_or(1, |m| m + 1);
        let _ =
            writeln!(xml, "  <object id=\"{}\" type=\"model\" name=\"{}\" ft:faces=\"{faces}\">\n   <mesh>\n    <vertices>", k + 1, escape(&b.name));
        for p in &b.positions {
            let _ = writeln!(xml, "     <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>", p.x * 1e3, p.y * 1e3, p.z * 1e3);
        }
        xml.push_str("    </vertices>\n    <triangles>\n");
        for (t, tri) in b.triangles.iter().enumerate() {
            let _ = writeln!(
                xml,
                "     <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\" ft:face=\"{}\"/>",
                tri[0],
                tri[1],
                tri[2],
                b.tags.get(t).copied().unwrap_or(0)
            );
        }
        xml.push_str("    </triangles>\n   </mesh>\n  </object>\n");
    }
    xml.push_str(" </resources>\n <build>\n");
    for k in 0..bodies.len() {
        let _ = writeln!(xml, "  <item objectid=\"{}\"/>", k + 1);
    }
    xml.push_str(" </build>\n</model>\n");
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>";
    let types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/></Types>";
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [("[Content_Types].xml", types), ("_rels/.rels", rels), ("3D/3dmodel.model", xml.as_str())] {
        out.start_file(name, opts).map_err(|e| invalid(format!("writing the package: {e}")))?;
        out.write_all(body.as_bytes()).map_err(|e| invalid(format!("writing the package: {e}")))?;
    }
    Ok(out.finish().map_err(|e| invalid(format!("writing the package: {e}")))?.into_inner())
}

/// The surfaces of a 3MF package, one per build item, in metres.
pub fn read(bytes: &[u8]) -> Result<Vec<Surface>> {
    if bytes.len() > MAX_FILE_BYTES {
        return bad(format!("file too large ({} MB)", bytes.len() >> 20));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| invalid(format!("not a zip package: {e}")))?;
    if zip.len() > MAX_ENTRIES {
        return bad(format!("{} zip entries", zip.len()));
    }
    // The model part named by the package relationships, else the usual name.
    let mut root = "3D/3dmodel.model".to_string();
    if let Some(rels) = read_part(&mut zip, "_rels/.rels")? {
        let mut rd = NsReader::from_reader(rels.as_slice());
        let mut buf = Vec::new();
        loop {
            match rd.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.local_name().as_ref() == "Relationship" => {
                    let a = attrs(&rd, &e)?;
                    if get(&a, Ns::Core, "Type").is_some_and(|t| t.ends_with("/3dmodel"))
                        && let Some(t) = get(&a, Ns::Core, "Target")
                    {
                        root = t.trim_start_matches('/').to_string();
                        break;
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    let root = part_key(&root);
    let xml = read_part(&mut zip, &root)?.ok_or_else(|| invalid(format!("no model part `{root}`")))?;
    let m = parse_model(&xml)?;
    drop(xml);
    if m.build.is_empty() {
        return bad("no build items");
    }
    let unit = m.unit;
    let mut elements = m.elements;
    let mut pkg = Package { root: root.clone(), parts: HashMap::from([(root.clone(), m)]) };
    // Every part a build item or component names, read once each, in the order first named
    // (objects by id), so the work is the same on every run.
    let mut queue = vec![root];
    while let Some(key) = queue.pop() {
        let mut named: Vec<String> = Vec::new();
        if let Some(m) = pkg.parts.get(&key) {
            let mut ids: Vec<&u32> = m.objects.keys().collect();
            ids.sort_unstable();
            let comps = ids.into_iter().filter_map(|i| m.objects.get(i)).flat_map(|o| o.components.iter());
            named.extend(m.build.iter().chain(comps).filter_map(|c| c.path.as_deref().map(part_key)));
        }
        for p in named {
            if pkg.parts.contains_key(&p) {
                continue;
            }
            if pkg.parts.len() >= MAX_MODEL_PARTS {
                return bad(format!("more than {MAX_MODEL_PARTS} model parts"));
            }
            let xml = read_part(&mut zip, &p)?.ok_or_else(|| invalid(format!("no model part `{p}`")))?;
            let part = parse_model(&xml)?;
            if part.unit != unit {
                return bad(format!("not supported: model part `{p}` is in another unit than the root part"));
            }
            elements = elements.saturating_add(part.elements);
            if elements > MAX_ELEMENTS {
                return bad(format!("more than {MAX_ELEMENTS} vertices or triangles in the model parts"));
            }
            pkg.parts.insert(p.clone(), part);
            queue.push(p);
        }
    }
    let build = pkg.parts.get(&pkg.root).map(|m| m.build.clone()).unwrap_or_default();
    let mut budget = Budget::default();
    build.iter().enumerate().map(|(k, c)| pkg.item(k, c, &mut budget)).collect()
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    const HEAD: &str = r#"<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:ft="https://github.com/bherbruck/solidcraft/formats/3mf-face-tags/2026/10"><resources>"#;

    /// A package holding only a model part (no relationships: the usual name is used).
    pub(crate) fn package(model: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("3D/3dmodel.model", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(model.as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn tet(extra: &str) -> String {
        format!(
            "{HEAD}<object id=\"1\" name=\"Tet\"><mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"1\" y=\"0\" z=\"0\"/><vertex x=\"0\" y=\"1\" z=\"0\"/><vertex x=\"0\" y=\"0\" z=\"1\"/></vertices>\
             <triangles><triangle v1=\"0\" v2=\"2\" v3=\"1\" ft:face=\"0\"/><triangle v1=\"0\" v2=\"1\" v3=\"3\" ft:face=\"1\"/><triangle v1=\"1\" v2=\"2\" v3=\"3\" ft:face=\"2\"/><triangle v1=\"0\" v2=\"3\" v3=\"2\" ft:face=\"3\"/></triangles></mesh></object>\
             </resources><build><item objectid=\"1\"{extra}/></build></model>"
        )
    }

    fn volume(s: &Surface) -> f64 {
        s.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|v| s.positions[v as usize].to_vec());
                emcraft_model::geom::triple(a, b, c) / 6.0
            })
            .sum()
    }

    #[test]
    fn units_tags_and_transforms() {
        let s = read(&package(&tet(""))).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "Tet");
        assert_eq!(s[0].tags, [0, 1, 2, 3]);
        assert_eq!(s[0].positions[1], Point3::new(1e-3, 0.0, 0.0));
        // Centimetres, mirrored and moved: the winding stays outward.
        let cm = tet(" transform=\"-1 0 0 0 1 0 0 0 1 0 0 2\"").replace("millimeter", "centimeter");
        let s = read(&package(&cm)).unwrap();
        assert!((s[0].positions[1][0] + 0.01).abs() < 1e-15 && (s[0].positions[1][2] - 0.02).abs() < 1e-15);
        assert!((volume(&s[0]) - 1e-6 / 6.0).abs() < 1e-18, "{}", volume(&s[0]));
    }

    #[test]
    fn hostile_packages_are_refused() {
        for bad in [
            tet("").replace("v3=\"1\"", "v3=\"9\""),
            tet("").replace("x=\"1\"", "x=\"nan\""),
            tet("").replace("objectid=\"1\"", "objectid=\"7\""),
            tet(" transform=\"0 0 0 0 0 0 0 0 0 0 0 0\""),
            tet("").replace("millimeter", "furlong"),
            "<nothing/>".to_string(),
        ] {
            assert!(read(&package(&bad)).is_err(), "{bad}");
        }
        assert!(read(b"").is_err());
        assert!(read(b"PK\x03\x04garbage").is_err());
        let good = package(&tet(""));
        for cut in [10, good.len() / 2, good.len() - 3] {
            assert!(read(&good[..cut]).is_err(), "truncated at {cut}");
        }
    }

    const PROD: &str = "http://schemas.microsoft.com/3dmanufacturing/production/2015/06";

    /// A package of several model parts (the first is the root, found by its usual name).
    fn package_parts(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body) in parts {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    /// The unit tetrahedron as object `id`, its four triangles tagged 0..4.
    fn tet_object(id: u32) -> String {
        format!(
            "<object id=\"{id}\" name=\"Tet{id}\"><mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"1\" y=\"0\" z=\"0\"/><vertex x=\"0\" y=\"1\" z=\"0\"/><vertex x=\"0\" y=\"0\" z=\"1\"/></vertices>\
             <triangles><triangle v1=\"0\" v2=\"2\" v3=\"1\" ft:face=\"0\"/><triangle v1=\"0\" v2=\"1\" v3=\"3\" ft:face=\"1\"/><triangle v1=\"1\" v2=\"2\" v3=\"3\" ft:face=\"2\"/><triangle v1=\"0\" v2=\"3\" v3=\"2\" ft:face=\"3\"/></triangles></mesh></object>"
        )
    }

    /// An object made of components, each (object id, transform).
    fn assembly(id: u32, parts: &[(u32, &str)]) -> String {
        let comps: String = parts.iter().map(|(o, xf)| format!("<component objectid=\"{o}\" transform=\"{xf}\"/>")).collect();
        format!("<object id=\"{id}\" name=\"Assembly{id}\"><components>{comps}</components></object>")
    }

    fn model(resources: &str, build: &str) -> String {
        format!("{HEAD}{resources}</resources><build>{build}</build></model>")
    }

    #[test]
    fn components_are_flattened_with_their_transforms_composed() {
        // Two tetrahedra: one moved 10 mm along x, one mirrored in x; the whole lifted 2 mm.
        let res = format!("{}{}", tet_object(1), assembly(2, &[(1, "1 0 0 0 1 0 0 0 1 10 0 0"), (1, "-1 0 0 0 1 0 0 0 1 0 0 0")]));
        let s = read(&package(&model(&res, "<item objectid=\"2\" transform=\"1 0 0 0 1 0 0 0 1 0 0 2\"/>"))).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "Assembly2");
        assert_eq!(s[0].triangles.len(), 8);
        // Each placed tetrahedron keeps its four faces, renumbered after the ones before it.
        assert_eq!(s[0].tags, [0, 1, 2, 3, 4, 5, 6, 7]);
        assert!((s[0].positions[1] - Point3::new(11e-3, 0.0, 2e-3)).norm() < 1e-15);
        assert!((s[0].positions[5] - Point3::new(-1e-3, 0.0, 2e-3)).norm() < 1e-15);
        // Mirrored or not, both stay outward: twice the tetrahedron's volume.
        assert!((volume(&s[0]) - 2.0 * 1e-9 / 6.0).abs() < 1e-20, "{}", volume(&s[0]));

        // A component's transform applies before its parent's: a scale by 2 under a shift by
        // 1 takes x = 1 to 3 (the other order would give 4); a rotation by 90° about z under
        // it takes x = 1 to y = 2, nested a level deeper.
        let res = format!(
            "{}{}{}",
            tet_object(1),
            assembly(2, &[(1, "0 1 0 -1 0 0 0 0 1 0 0 0")]),
            assembly(3, &[(1, "2 0 0 0 2 0 0 0 2 0 0 0"), (2, "2 0 0 0 2 0 0 0 2 0 0 0")])
        );
        let s = read(&package(&model(&res, "<item objectid=\"3\" transform=\"1 0 0 0 1 0 0 0 1 1 0 0\"/>"))).unwrap();
        assert!((s[0].positions[1] - Point3::new(3e-3, 0.0, 0.0)).norm() < 1e-15, "{:?}", s[0].positions[1]);
        assert!((s[0].positions[5] - Point3::new(1e-3, 2e-3, 0.0)).norm() < 1e-15, "{:?}", s[0].positions[5]);
        assert!((volume(&s[0]) - 16.0 * 1e-9 / 6.0).abs() < 1e-19);
    }

    #[test]
    fn build_items_and_components_reach_other_model_parts() {
        let head = HEAD.replace("><resources>", &format!(" xmlns:p=\"{PROD}\"><resources>"));
        let other = format!("{HEAD}{}</resources><build/></model>", tet_object(7));
        let root = format!(
            "{head}<object id=\"2\" name=\"Pair\"><components><component objectid=\"7\" p:path=\"/3D/parts/tet.model\" transform=\"1 0 0 0 1 0 0 0 1 5 0 0\"/><component objectid=\"7\" p:path=\"/3D/parts/tet.model\"/></components></object></resources>\
             <build><item objectid=\"7\" p:path=\"/3D/parts/tet.model\"/><item objectid=\"2\"/></build></model>"
        );
        let s = read(&package_parts(&[("3D/3dmodel.model", &root), ("3D/parts/tet.model", &other)])).unwrap();
        assert_eq!(s.iter().map(|s| (s.name.as_str(), s.triangles.len())).collect::<Vec<_>>(), [("Tet7", 4), ("Pair", 8)]);
        assert!((s[1].positions[1] - Point3::new(6e-3, 0.0, 0.0)).norm() < 1e-15);
        // A part that is not in the package, or a path climbing out of it, is refused.
        assert!(read(&package_parts(&[("3D/3dmodel.model", &root)])).is_err());
        let climb = root.replace("/3D/parts/tet.model\"/>", "/3D/../tet.model\"/>");
        assert!(read(&package_parts(&[("3D/3dmodel.model", &climb), ("3D/parts/tet.model", &other)])).is_err());
        // Parts in another unit than the root are refused rather than guessed at.
        let cm = other.replace("millimeter", "centimeter");
        assert!(read(&package_parts(&[("3D/3dmodel.model", &root), ("3D/parts/tet.model", &cm)])).is_err());
    }

    #[test]
    fn hostile_component_trees_are_refused() {
        let item = "<item objectid=\"2\"/>";
        let id = "1 0 0 0 1 0 0 0 1 0 0 0";
        let mut cases = vec![
            // A cycle, and an object its own component.
            model(&format!("{}{}{}", tet_object(1), assembly(2, &[(3, id)]), assembly(3, &[(1, id), (2, id)])), item),
            model(&assembly(2, &[(2, id)]), item),
            // A missing component, a bad or non-finite transform, a flattening one.
            model(&assembly(2, &[(9, id)]), item),
            model(&format!("{}{}", tet_object(1), assembly(2, &[(1, "1 0 0 0 1 0 0 0 1 nan 0 0")])), item),
            model(&format!("{}{}", tet_object(1), assembly(2, &[(1, "1 0 0")])), item),
            model(&format!("{}{}", tet_object(1), assembly(2, &[(1, "1 0 0 0 1 0 0 0 0 0 0 0")])), item),
            // Scales that are each in range but whose product is not.
            model(
                &format!(
                    "{}{}{}",
                    tet_object(1),
                    assembly(3, &[(1, "1e5 0 0 0 1e5 0 0 0 1e5 0 0 0")]),
                    assembly(2, &[(3, "1e5 0 0 0 1e5 0 0 0 1e5 0 0 0")])
                ),
                item,
            ),
            // Components with nothing in them, and an object with both a mesh and components.
            model(&format!("<object id=\"1\"/>{}", assembly(2, &[(1, id)])), item),
            model(&tet_object(2).replace("</mesh>", "</mesh><components><component objectid=\"1\"/></components>"), item),
        ];
        // A chain deeper than the cap.
        let mut chain = tet_object(1);
        for k in 2..=(MAX_COMPONENT_DEPTH as u32 + 3) {
            chain.push_str(&assembly(k, &[(k - 1, id)]));
        }
        cases.push(model(&chain, &format!("<item objectid=\"{}\"/>", MAX_COMPONENT_DEPTH + 2)));
        // A tree that doubles at each level: within the depth cap, but placing 2²⁵ tetrahedra.
        let mut tree = tet_object(1);
        for k in 2..=26 {
            tree.push_str(&assembly(k, &[(k - 1, id), (k - 1, "1 0 0 0 1 0 0 0 1 1 0 0")]));
        }
        cases.push(model(&tree, "<item objectid=\"26\"/>"));
        // The same tree of empty objects places nothing, but still stops at the visit cap.
        cases.push(model(&tree.replace(&tet_object(1), "<object id=\"1\"/>"), "<item objectid=\"26\"/>"));
        for (k, c) in cases.iter().enumerate() {
            let e = read(&package(c));
            assert!(e.is_err(), "case {k} was read");
        }
        // A chain just within the cap reads.
        let mut ok = tet_object(1);
        for k in 2..=(MAX_COMPONENT_DEPTH as u32) {
            ok.push_str(&assembly(k, &[(k - 1, id)]));
        }
        assert_eq!(read(&package(&model(&ok, &format!("<item objectid=\"{MAX_COMPONENT_DEPTH}\"/>")))).unwrap()[0].triangles.len(), 4);
    }
}

//! 3MF packages (3MF Core Specification 1.3): the mesh objects of the build items, placed by
//! their transforms and scaled to metres. Triangles may carry SolidCraft's face tags
//! (`ft:face`, namespace [`FACE_TAGS_NS`]); untagged objects are one face each.
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
/// A transform whose determinant is below this fraction of its largest row length cubed
/// flattens the object (relative, so a uniform scale is never refused).
const MIN_RELATIVE_DET: f64 = 1e-12;

const CORE_NS: &str = "http://schemas.microsoft.com/3dmanufacturing/core/2015/02";
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
    Other,
}

fn ns_of(r: &ResolveResult) -> Ns {
    match r {
        ResolveResult::Bound(n) if n.as_ref() == CORE_NS => Ns::Core,
        ResolveResult::Bound(n) if n.as_ref() == FACE_TAGS_NS => Ns::FaceTags,
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
    components: bool,
}

impl Object {
    /// Checks that need the whole object: tagging is all or nothing, indices exist.
    fn check(&self, id: u32) -> Result<()> {
        if !self.tags.is_empty() && self.tags.len() != self.triangles.len() {
            return bad(format!("object {id}: {} of {} triangles have a face tag", self.tags.len(), self.triangles.len()));
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
    build: Vec<(u32, Xf)>,
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
                let Some((_, o)) = cur.as_mut() else { return bad("component outside an object") };
                o.components = true;
            }
            "item" => {
                if m.build.len() >= MAX_BUILD_ITEMS {
                    return bad(format!("more than {MAX_BUILD_ITEMS} build items"));
                }
                if get(&a, Ns::Other, "path").is_some() {
                    return bad("not supported yet: build items in other model parts (production extension)");
                }
                let target = int(&a, Ns::Core, "objectid", "build item")?.ok_or_else(|| invalid("build item without objectid"))?;
                m.build.push((target, parse_xf(get(&a, Ns::Core, "transform"))?));
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
    Ok(m)
}

/// Build item `k` as a surface: object `o` (checked) placed by `xf` and scaled to metres.
fn surface(mut o: Object, k: usize, id: u32, xf: &Xf, unit: f64) -> Result<Surface> {
    let d = det(xf);
    let scale = (0..3).map(|r| (xf[r * 3].powi(2) + xf[r * 3 + 1].powi(2) + xf[r * 3 + 2].powi(2)).sqrt()).fold(0.0, f64::max);
    if !(d.abs() > MIN_RELATIVE_DET * scale.powi(3)) {
        return bad(format!("build item {}: its transform is singular (it flattens object {id} to zero volume)", k + 1));
    }
    if o.components {
        return bad(format!("not supported yet: object {id} is made of components"));
    }
    if o.triangles.is_empty() {
        return bad(format!("object {id} has no triangles"));
    }
    for p in &mut o.positions {
        *p = apply(xf, *p).map(|c| c * unit);
        if !p.iter().all(|c| c.is_finite() && c.abs() <= MAX_COORD) {
            return bad(format!("object {id}: a placed vertex is out of range"));
        }
    }
    if d < 0.0 {
        o.triangles.iter_mut().for_each(|t| t.swap(1, 2));
    }
    if o.tags.is_empty() {
        o.tags = vec![0; o.triangles.len()];
    }
    let name = if o.name.is_empty() { format!("Mesh{}", k + 1) } else { o.name };
    Ok(Surface { name, positions: o.positions, triangles: o.triangles, tags: o.tags })
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
    let xml = read_part(&mut zip, &root)?.ok_or_else(|| invalid(format!("no model part `{root}`")))?;
    let mut m = parse_model(&xml)?;
    drop(xml);
    if m.build.is_empty() {
        return bad("no build items");
    }
    // An object's last placement takes it; earlier ones copy it. Every placement counts against
    // one budget, so placing a big object many times stops at the cap.
    let mut uses: HashMap<u32, usize> = HashMap::new();
    for (id, _) in &m.build {
        *uses.entry(*id).or_default() += 1;
    }
    let mut budget = 0usize;
    let mut out = Vec::with_capacity(m.build.len());
    for (k, (id, xf)) in m.build.iter().enumerate() {
        let left = uses.get_mut(id).map(|n| std::mem::replace(n, n.saturating_sub(1))).unwrap_or(0);
        let o = if left > 1 { m.objects.get(id).cloned() } else { m.objects.remove(id) };
        let o = o.ok_or_else(|| invalid(format!("build item {} refers to missing object {id}", k + 1)))?;
        budget = budget.saturating_add(o.positions.len().max(o.triangles.len()));
        if budget > MAX_ELEMENTS {
            return bad(format!("the build items place more than {MAX_ELEMENTS} vertices or triangles"));
        }
        out.push(surface(o, k, *id, xf, m.unit)?);
    }
    Ok(out)
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
}

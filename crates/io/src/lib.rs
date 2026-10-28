//! EMCraft's geometry import: closed triangulated surfaces read from STL (binary and ASCII,
//! [`stl`]) and 3MF packages ([`threemf`]), in metres, their triangles grouped into faces.
//!
//! Files are untrusted: sizes, counts and coordinates are capped and every index is checked;
//! every problem is an [`Error`], never a panic. Whether a surface bounds a solid (closed,
//! oriented, not self-intersecting) is checked by the model and the mesher, not here.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod fit;
pub mod solidcraft;
pub mod stl;
pub mod threemf;

use std::collections::HashMap;
use std::path::Path;

use emcraft_model::{Point3, Vec3};

/// Largest coordinate accepted, in metres (after units and transforms).
pub const MAX_COORD: f64 = 1e6;
/// Most triangles read from one file.
pub const MAX_TRIANGLES: usize = 5_000_000;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
    /// A file whose contents are wrong.
    #[error("{0}")]
    Invalid(String),
    /// A file whose extension names no format read here (its name).
    #[error("`{0}`: not an STL or 3MF file (by its extension)")]
    UnknownFormat(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A triangulated surface: positions (m), triangles and a face tag per triangle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Surface {
    pub name: String,
    pub positions: Vec<Point3>,
    pub triangles: Vec<[u32; 3]>,
    pub tags: Vec<u32>,
}

/// The file formats read here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Stl,
    ThreeMf,
}

impl Format {
    /// The format a file's name says, by its extension (any case).
    pub fn of(name: &str) -> Result<Format> {
        let ext = Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
        match ext.as_deref() {
            Some("stl") => Ok(Format::Stl),
            Some("3mf") => Ok(Format::ThreeMf),
            _ => Err(Error::UnknownFormat(name.to_owned())),
        }
    }
}

/// The last part of a path, `/` or `\` separated (a file chosen on another system keeps its
/// name).
fn file_name(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// Reads a surface from a file's bytes, by its extension (`stl`, `3mf`); STL lengths are in
/// `stl_unit` metres (STL carries no units). A 3MF package's build items become one surface.
pub fn read(name: &str, bytes: &[u8], stl_unit: f64) -> Result<Surface> {
    let mut all = Surface::default();
    for p in read_bodies(name, bytes, stl_unit)? {
        let off = all.positions.len() as u32;
        let base = all.tags.iter().map(|t| t + 1).max().unwrap_or(0);
        all.positions.extend(p.positions);
        all.triangles.extend(p.triangles.into_iter().map(|t| t.map(|v| v + off)));
        all.tags.extend(p.tags.into_iter().map(|t| t + base));
    }
    all.name = file_name(name).to_owned();
    Ok(all)
}

/// Reads a file's bodies, by its extension: a 3MF package's build items one body each (named
/// after their objects, SolidCraft's bodies), an STL file one body.
pub fn read_bodies(name: &str, bytes: &[u8], stl_unit: f64) -> Result<Vec<Surface>> {
    match Format::of(name)? {
        Format::Stl => Ok(vec![Surface { name: file_name(name).to_owned(), ..stl::read(bytes, stl_unit)? }]),
        Format::ThreeMf => threemf::read(bytes),
    }
}

/// Splits each face into the regions whose neighbouring triangles turn by less than
/// `angle_deg` (a surface's flat or gently curved patches, so its sharp edges stay edges when
/// it is meshed); returns the number of faces.
pub fn split_faces(s: &mut Surface, angle_deg: f64) -> u32 {
    let normal = |t: &[u32; 3]| {
        let [a, b, c] = t.map(|v| s.positions.get(v as usize).copied().unwrap_or(Point3::ORIGIN));
        (b - a).cross(c - a).normalized().unwrap_or(Vec3::ZERO)
    };
    let normals: Vec<Vec3> = s.triangles.iter().map(normal).collect();
    let mut by_edge: HashMap<[u32; 2], Vec<usize>> = HashMap::new();
    for (i, t) in s.triangles.iter().enumerate() {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            by_edge.entry([a.min(b), a.max(b)]).or_default().push(i);
        }
    }
    let cos = angle_deg.to_radians().cos();
    let mut tag = vec![u32::MAX; s.triangles.len()];
    let mut next = 0u32;
    for start in 0..s.triangles.len() {
        if tag[start] != u32::MAX {
            continue;
        }
        let old = s.tags.get(start).copied();
        tag[start] = next;
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            let t = s.triangles[i];
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                for &j in by_edge.get(&[a.min(b), a.max(b)]).into_iter().flatten() {
                    let (ni, nj) = (normals[i], normals[j]);
                    if tag[j] == u32::MAX && s.tags.get(j).copied() == old && ni.dot(nj) >= cos {
                        tag[j] = next;
                        stack.push(j);
                    }
                }
            }
        }
        next += 1;
    }
    s.tags = tag;
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_splits_into_its_six_faces() {
        let mut s = stl::tests::cube_surface(0.02, 0.01, 0.03);
        assert_eq!(split_faces(&mut s, 30.0), 6);
        // Every face's two triangles share a tag and differ from the others'.
        for f in 0..6 {
            assert_eq!(s.tags[2 * f], s.tags[2 * f + 1]);
        }
        let mut seen: Vec<u32> = s.tags.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 6);
        assert!(read("x.obj", b"", 1e-3).is_err());
    }

    #[test]
    fn bodies_keep_their_names_and_faces_through_a_package() {
        let mut a = stl::tests::cube_surface(0.02, 0.01, 0.03);
        split_faces(&mut a, 30.0);
        a.name = "Can & lid".into();
        let mut b = stl::tests::cube_surface(0.004, 0.004, 0.004);
        b.name = "Puck".into();
        let bytes = threemf::write(&[a.clone(), b]).unwrap();
        let back = read_bodies("part.3mf", &bytes, 1e-3).unwrap();
        assert_eq!(back.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Can & lid", "Puck"]);
        assert_eq!((back[0].triangles.clone(), back[0].tags.clone()), (a.triangles, a.tags));
        for (p, q) in back[0].positions.iter().zip(&a.positions) {
            assert!((*p - *q).norm() < 1e-15);
        }
        let stl = stl::write_binary(&stl::tests::cube_surface(0.01, 0.01, 0.01), 1e-3);
        assert_eq!(read_bodies("one.stl", &stl, 1e-3).unwrap().len(), 1);
    }
}

//! EMCraft's geometry import: closed triangulated surfaces read from STL (binary and ASCII,
//! [`stl`]) and 3MF packages ([`threemf`]), in metres, their triangles grouped into faces.
//!
//! Files are untrusted: sizes, counts and coordinates are capped and every index is checked;
//! every problem is an [`Error`], never a panic. Whether a surface bounds a solid (closed,
//! oriented, not self-intersecting) is checked by the model and the mesher, not here.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod stl;
pub mod threemf;

use std::collections::HashMap;

/// Largest coordinate accepted, in metres (after units and transforms).
pub const MAX_COORD: f64 = 1e6;
/// Most triangles read from one file.
pub const MAX_TRIANGLES: usize = 5_000_000;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A triangulated surface: positions (m), triangles and a face tag per triangle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Surface {
    pub name: String,
    pub positions: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub tags: Vec<u32>,
}

/// Reads a surface from a file's bytes, by its extension (`stl`, `3mf`); STL lengths are in
/// `stl_unit` metres (STL carries no units). A 3MF package's build items become one surface.
pub fn read(name: &str, bytes: &[u8], stl_unit: f64) -> Result<Surface> {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let mut s = match ext.as_str() {
        "stl" => stl::read(bytes, stl_unit)?,
        "3mf" => {
            let parts = threemf::read(bytes)?;
            let mut all = Surface::default();
            for p in parts {
                let off = all.positions.len() as u32;
                let base = all.tags.iter().map(|t| t + 1).max().unwrap_or(0);
                all.positions.extend(p.positions);
                all.triangles.extend(p.triangles.into_iter().map(|t| t.map(|v| v + off)));
                all.tags.extend(p.tags.into_iter().map(|t| t + base));
            }
            all
        }
        _ => return Err(Error::Invalid(format!("`{name}`: not an STL or 3MF file (by its extension)"))),
    };
    s.name = name.rsplit(['/', '\\']).next().unwrap_or(name).to_string();
    Ok(s)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Splits each face into the regions whose neighbouring triangles turn by less than
/// `angle_deg` (a surface's flat or gently curved patches, so its sharp edges stay edges when
/// it is meshed); returns the number of faces.
pub fn split_faces(s: &mut Surface, angle_deg: f64) -> u32 {
    let normal = |t: &[u32; 3]| {
        let p = t.map(|v| s.positions.get(v as usize).copied().unwrap_or([0.0; 3]));
        let n = cross(sub(p[1], p[0]), sub(p[2], p[0]));
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if l > 0.0 { n.map(|v| v / l) } else { [0.0; 3] }
    };
    let normals: Vec<[f64; 3]> = s.triangles.iter().map(normal).collect();
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
                    if tag[j] == u32::MAX && s.tags.get(j).copied() == old && ni[0] * nj[0] + ni[1] * nj[1] + ni[2] * nj[2] >= cos {
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
}

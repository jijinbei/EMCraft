//! STL: triangles as binary records (an 80-byte header, a count, 50 bytes per triangle) or as
//! ASCII `facet … vertex x y z … endfacet` text. The corners are welded where their coordinates
//! are equal (an STL file repeats them per triangle); the normals in the file are ignored (the
//! corner order gives the outward side).

use std::collections::HashMap;

use emcraft_model::Point3;

use crate::{Error, MAX_COORD, MAX_TRIANGLES, Result, Surface};

fn bad<T>(msg: impl std::fmt::Display) -> Result<T> {
    Err(Error::Invalid(format!("STL: {msg}")))
}

/// Reads binary or ASCII STL, its lengths in `unit` metres.
pub fn read(bytes: &[u8], unit: f64) -> Result<Surface> {
    if !(unit.is_finite() && unit > 0.0) {
        return bad(format!("the unit must be a positive length, got {unit}"));
    }
    let corners = if is_binary(bytes) { binary(bytes)? } else { ascii(bytes)? };
    if corners.is_empty() {
        return bad("no triangles");
    }
    let mut s = Surface::default();
    let mut index: HashMap<[u64; 3], u32> = HashMap::new();
    for tri in corners {
        let mut t = [0u32; 3];
        for (slot, p) in t.iter_mut().zip(tri) {
            let p = p.map(|c| c * unit);
            if !p.iter().all(|c| c.is_finite() && c.abs() <= MAX_COORD) {
                return bad("a coordinate is out of range");
            }
            // +0 and −0 are the same point.
            let key = p.map(|c| if c == 0.0 { 0u64 } else { c.to_bits() });
            *slot = *index.entry(key).or_insert_with(|| {
                s.positions.push(Point3::from(p));
                (s.positions.len() - 1) as u32
            });
        }
        // Triangles collapsed to a line or a point add nothing to a surface.
        if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
            s.triangles.push(t);
        }
    }
    s.tags = vec![0; s.triangles.len()];
    Ok(s)
}

/// Binary when the size is that of the count the header gives (ASCII files start with
/// `solid`, but some binary ones do too).
fn is_binary(bytes: &[u8]) -> bool {
    if bytes.len() < 84 {
        return false;
    }
    let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    n.checked_mul(50).and_then(|x| x.checked_add(84)) == Some(bytes.len())
}

fn binary(bytes: &[u8]) -> Result<Vec<[[f64; 3]; 3]>> {
    let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    if n > MAX_TRIANGLES {
        return bad(format!("{n} triangles; at most {MAX_TRIANGLES} are read"));
    }
    let f = |o: usize| f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]) as f64;
    Ok((0..n)
        .map(|k| {
            let o = 84 + 50 * k + 12;
            [0, 1, 2].map(|v| [f(o + 12 * v), f(o + 12 * v + 4), f(o + 12 * v + 8)])
        })
        .collect())
}

fn ascii(bytes: &[u8]) -> Result<Vec<[[f64; 3]; 3]>> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| Error::Invalid("STL: neither binary (the size does not match its count) nor ASCII text".into()))?;
    if !text.trim_start().starts_with("solid") {
        return bad("neither binary (the size does not match its count) nor ASCII (no `solid`)");
    }
    let mut out = Vec::new();
    let mut corners: Vec<[f64; 3]> = Vec::with_capacity(3);
    for (line, l) in text.lines().enumerate() {
        let mut w = l.split_whitespace();
        match w.next() {
            Some("vertex") => {
                let mut p = [0.0; 3];
                for c in &mut p {
                    *c = w
                        .next()
                        .and_then(|t| t.parse::<f64>().ok())
                        .ok_or_else(|| Error::Invalid(format!("STL: line {}: a vertex needs three numbers", line + 1)))?;
                }
                corners.push(p);
            }
            Some("endfacet") => {
                let [a, b, c] = corners[..] else {
                    return bad(format!("line {}: a facet has {} vertices, not 3", line + 1, corners.len()));
                };
                out.push([a, b, c]);
                corners.clear();
                if out.len() > MAX_TRIANGLES {
                    return bad(format!("more than {MAX_TRIANGLES} triangles"));
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// The surface as binary STL in `unit` metres (for tests and exports).
pub fn write_binary(s: &Surface, unit: f64) -> Vec<u8> {
    let mut out = vec![0u8; 80];
    out[..14].copy_from_slice(b"EMCraft binary");
    out.extend((s.triangles.len() as u32).to_le_bytes());
    for t in &s.triangles {
        out.extend([0u8; 12]);
        for &v in t {
            let p = s.positions.get(v as usize).copied().unwrap_or(Point3::ORIGIN);
            for c in p.iter() {
                out.extend(((c / unit) as f32).to_le_bytes());
            }
        }
        out.extend([0u8; 2]);
    }
    out
}

/// The surface as ASCII STL in `unit` metres.
pub fn write_ascii(s: &Surface, unit: f64) -> String {
    let mut out = String::from("solid emcraft\n");
    for t in &s.triangles {
        out.push_str(" facet normal 0 0 0\n  outer loop\n");
        for &v in t {
            let p = s.positions.get(v as usize).copied().unwrap_or(Point3::ORIGIN);
            out.push_str(&format!("   vertex {} {} {}\n", p.x / unit, p.y / unit, p.z / unit));
        }
        out.push_str("  endloop\n endfacet\n");
    }
    out.push_str("endsolid emcraft\n");
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A box [0, x] × [0, y] × [0, z], two outward triangles per face (faces x0, x1, y0, y1,
    /// z0, z1).
    pub(crate) fn cube_surface(x: f64, y: f64, z: f64) -> Surface {
        let mut s = Surface::default();
        for i in 0..8 {
            s.positions.push(Point3::new(if i & 1 == 0 { 0.0 } else { x }, if i & 2 == 0 { 0.0 } else { y }, if i & 4 == 0 { 0.0 } else { z }));
        }
        for q in [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]] {
            s.triangles.push([q[0], q[1], q[2]]);
            s.triangles.push([q[0], q[2], q[3]]);
        }
        s.tags = vec![0; 12];
        s
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
    fn binary_and_ascii_round_trip_with_units() {
        // Millimetre values exact in f32: 22.5 × 10 × 30 mm.
        let s = cube_surface(0.0225, 0.01, 0.03);
        for bytes in [write_binary(&s, 1e-3), write_ascii(&s, 1e-3).into_bytes()] {
            let r = read(&bytes, 1e-3).unwrap();
            assert_eq!(r.positions.len(), 8);
            assert_eq!(r.triangles.len(), 12);
            assert!((volume(&r) - 0.0225 * 0.01 * 0.03).abs() < 1e-15, "{}", volume(&r));
        }
    }

    #[test]
    fn hostile_files_are_refused() {
        assert!(read(b"", 1e-3).is_err());
        assert!(read(b"solid x\n facet\n  vertex 1 2\n endfacet\nendsolid", 1e-3).is_err());
        assert!(read(b"solid x\n facet\n  vertex 0 0 0\n  vertex 1 0 0\n endfacet\nendsolid", 1e-3).is_err());
        assert!(read(b"solid x\n facet\n  vertex 1e300 0 0\n vertex 0 1 0\n vertex 0 0 1\n endfacet\nendsolid", 1.0).is_err());
        assert!(read(b"\xff\xfe garbage", 1e-3).is_err());
        let mut lie = write_binary(&cube_surface(1.0, 1.0, 1.0), 1.0);
        lie.truncate(lie.len() - 7);
        assert!(read(&lie, 1.0).is_err());
        assert!(read(&write_binary(&cube_surface(1.0, 1.0, 1.0), 1.0), f64::NAN).is_err());
    }
}

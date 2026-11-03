//! PCB routing and filled copper, in SI metres. Independent of file formats and solvers.
use crate::error::{Result, invalid};
use serde::{Deserialize, Serialize};

pub const MAX_SEGMENTS: usize = 20_000;
pub const MAX_VERTICES: usize = 200_000;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub z: f64,
    pub eps_below: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub id: String,
    pub net: String,
    pub layer: String,
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub width: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Via {
    pub net: String,
    pub at: [f64; 2],
    pub layers: [String; 2],
    pub diameter: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Copper {
    pub net: String,
    pub layer: String,
    pub polygon: Vec<[f64; 2]>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub name: String,
    pub layers: Vec<Layer>,
    pub segments: Vec<Segment>,
    #[serde(default)]
    pub vias: Vec<Via>,
    #[serde(default)]
    pub copper: Vec<Copper>,
    #[serde(default)]
    pub notes: Vec<String>,
}
impl Board {
    /// Validate a board before storing it. Empty layers, unknown layers and non-finite geometry
    /// are refused. Coordinates and lengths are SI metres.
    /// ```
    /// let b=emcraft_model::pcb::Board{name:"empty".into(),layers:vec![],segments:vec![],vias:vec![],copper:vec![],notes:vec![]};
    /// assert!(b.check().is_err());
    /// ```
    pub fn check(&self) -> Result<()> {
        let bad = || invalid("PCB", "invalid or excessive geometry, stackup, names or coordinates");
        let name = |s: &str| !s.trim().is_empty() && s.chars().count() <= 128;
        let point = |p: [f64; 2]| p.iter().all(|v| v.is_finite() && v.abs() <= 10.0);
        let size = |v: f64| v.is_finite() && v > 0.0 && v <= 10.0;
        if !name(&self.name)
            || self.layers.len() < 2
            || self.layers.len() > 64
            || self.segments.is_empty()
            || self.segments.len() > MAX_SEGMENTS
            || self.vias.len() > MAX_SEGMENTS
            || self.copper.len() > 4096
            || self.notes.len() > 128
            || self.notes.iter().any(|n| n.len() > 1024)
        {
            return Err(bad());
        }
        for (i, l) in self.layers.iter().enumerate() {
            if !name(&l.name)
                || !l.z.is_finite()
                || l.z < 0.0
                || l.z > 0.1
                || !l.eps_below.is_finite()
                || !(1.0..=128.0).contains(&l.eps_below)
                || self.layers[..i].iter().any(|q| q.name == l.name || q.z >= l.z)
            {
                return Err(bad());
            }
        }
        let layer = |n: &str| self.layers.iter().any(|l| l.name == n);
        let mut ids = std::collections::BTreeSet::new();
        for s in &self.segments {
            if !name(&s.id)
                || !ids.insert(&s.id)
                || !name(&s.net)
                || !layer(&s.layer)
                || !point(s.start)
                || !point(s.end)
                || !size(s.width)
                || distance(s.start, s.end) < 1e-12
            {
                return Err(bad());
            }
        }
        for v in &self.vias {
            if !name(&v.net) || !point(v.at) || !size(v.diameter) || v.layers[0] == v.layers[1] || v.layers.iter().any(|l| !layer(l)) {
                return Err(bad());
            }
        }
        let total: usize = self.copper.iter().map(|p| p.polygon.len()).sum();
        if total > MAX_VERTICES {
            return Err(bad());
        }
        for c in &self.copper {
            if !name(&c.net) || !layer(&c.layer) || c.polygon.len() < 3 || !c.polygon.iter().all(|p| point(*p)) {
                return Err(bad());
            }
        }
        Ok(())
    }
    /// Convert all lengths together at a command/file boundary; call `check` after conversion.
    pub fn map_lengths(&mut self, mut convert: impl FnMut(f64) -> f64) {
        for l in &mut self.layers {
            l.z = convert(l.z);
        }
        for s in &mut self.segments {
            for p in [&mut s.start, &mut s.end] {
                for v in p {
                    *v = convert(*v);
                }
            }
            s.width = convert(s.width);
        }
        for v in &mut self.vias {
            for n in &mut v.at {
                *n = convert(*n);
            }
            v.diameter = convert(v.diameter);
        }
        for c in &mut self.copper {
            for p in &mut c.polygon {
                for n in p {
                    *n = convert(*n);
                }
            }
        }
    }
}
pub fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

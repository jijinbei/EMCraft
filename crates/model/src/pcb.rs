//! A printed circuit board: its copper layers, routed segments, vias and filled copper, in SI
//! metres. Independent of file formats (crates/io reads KiCad's) and of the solvers.

use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid};

/// The most routed segments a board may have.
pub const MAX_SEGMENTS: usize = 200_000;
/// The most filled-copper polygons (pours, pads) a board may have.
pub const MAX_COPPER: usize = 100_000;
/// The most polygon vertices a board's filled copper may have in all.
pub const MAX_VERTICES: usize = 2_000_000;

/// A copper layer: its name (as the source names it, e.g. `F.Cu`), its height z (m) and the
/// relative permittivity and loss tangent of the dielectric below it, down to the next layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub z: f64,
    pub eps_below: f64,
    /// tan δ of the dielectric below (0: lossless).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub loss_below: f64,
}

#[allow(clippy::trivially_copy_pass_by_ref, reason = "serde's skip_serializing_if passes a reference")]
fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

/// A straight routed segment of a net on a layer, `width` wide (m), from `start` to `end` (m).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub id: String,
    pub net: String,
    pub layer: String,
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub width: f64,
}

impl Segment {
    /// The segment's point at `end`: its start or its end (m).
    ///
    /// ```
    /// use emcraft_model::End;
    /// use emcraft_model::pcb::Segment;
    /// let g = Segment { id: "a".into(), net: "N".into(), layer: "F.Cu".into(), start: [0.0, 0.0], end: [0.01, 0.0], width: 2e-4 };
    /// assert_eq!(g.at(End::End), [0.01, 0.0]);
    /// ```
    pub fn at(&self, end: crate::End) -> [f64; 2] {
        match end {
            crate::End::Start => self.start,
            crate::End::End => self.end,
        }
    }
}

/// A through or blind via of a net at `at` (m), joining the two named layers: its pads
/// `diameter` across (m) and its plated hole `drill` (m; the pads' when not given).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Via {
    pub net: String,
    pub at: [f64; 2],
    pub layers: [String; 2],
    pub diameter: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drill: Option<f64>,
}

impl Via {
    /// The barrel's diameter (m): the drill, else the pads'.
    pub fn barrel(&self) -> f64 {
        self.drill.unwrap_or(self.diameter)
    }
}

/// Filled copper of a net on a layer: a polygon (m), even–odd.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Copper {
    pub net: String,
    pub layer: String,
    pub polygon: Vec<[f64; 2]>,
}

/// The part of a board the finite elements solve: the box over `lo`..`hi` (m) from `below` under
/// the lowest layer to `above` over the highest, closed by conducting walls (a shielded box: its
/// sides carry the ports).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub lo: [f64; 2],
    pub hi: [f64; 2],
    pub above: f64,
    pub below: f64,
}

/// The longest side of a region (m).
pub const MAX_REGION: f64 = 1.0;

/// A board: its layers (top down), routes, vias, filled copper and the notes its reader left
/// (what it skipped or could not represent).
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
    /// The region solved by the finite elements (`pcb.region`), if one is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Region>,
}

impl Board {
    /// The bounds (m) of the routes and filled copper, `None` for an empty board.
    ///
    /// ```
    /// use emcraft_model::pcb::{Board, Layer, Segment};
    /// let empty = Board { name: "empty".into(), layers: vec![], segments: vec![], vias: vec![], copper: vec![], notes: vec![], region: None };
    /// assert!(empty.bounds().is_none());
    /// let segment = Segment { id: "0".into(), net: "A".into(), layer: "F.Cu".into(), start: [0.0, 0.001], end: [0.01, 0.002], width: 2e-4 };
    /// let line = Board { segments: vec![segment], layers: vec![Layer { name: "F.Cu".into(), z: 0.0, eps_below: 4.2, loss_below: 0.0 }], ..empty };
    /// assert_eq!(line.bounds(), Some(([0.0, 0.001], [0.01, 0.002])));
    /// ```
    pub fn bounds(&self) -> Option<([f64; 2], [f64; 2])> {
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for p in self.segments.iter().flat_map(|s| [s.start, s.end]).chain(self.copper.iter().flat_map(|c| c.polygon.iter().copied())) {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        lo.iter().chain(&hi).all(|v| v.is_finite()).then_some((lo, hi))
    }

    /// Validate a board before storing it. Empty layers, unknown layers and non-finite geometry
    /// are refused. Coordinates and lengths are SI metres.
    /// ```
    /// let b=emcraft_model::pcb::Board{name:"empty".into(),layers:vec![],segments:vec![],vias:vec![],copper:vec![],notes:vec![],region:None};
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
            || self.copper.len() > MAX_COPPER
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
                || !(0.0..=1.0).contains(&l.loss_below)
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
            if !name(&v.net)
                || !point(v.at)
                || !size(v.diameter)
                || v.drill.is_some_and(|d| !size(d) || d > v.diameter)
                || v.layers[0] == v.layers[1]
                || v.layers.iter().any(|l| !layer(l))
            {
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
        if let Some(r) = &self.region {
            r.check()?;
        }
        Ok(())
    }

    /// Each layer's height over the lowest layer (m): the board's z runs down from its top.
    ///
    /// ```
    /// use emcraft_model::pcb::{Board, Layer};
    /// let layers = vec![Layer { name: "F.Cu".into(), z: 0.0, eps_below: 4.2, loss_below: 0.0 }, Layer { name: "B.Cu".into(), z: 1.6e-3, eps_below: 1.0, loss_below: 0.0 }];
    /// let b = Board { name: "b".into(), layers, segments: vec![], vias: vec![], copper: vec![], notes: vec![], region: None };
    /// assert_eq!(b.heights(), vec![1.6e-3, 0.0]);
    /// ```
    pub fn heights(&self) -> Vec<f64> {
        let bottom = self.layers.iter().map(|l| l.z).fold(f64::NEG_INFINITY, f64::max);
        self.layers.iter().map(|l| bottom - l.z).collect()
    }

    /// Converts every length (layers' z, segments' ends and widths, vias, copper) by `convert`, at
    /// a command's or a file's boundary; `check` the board after.
    ///
    /// ```
    /// use emcraft_model::pcb::{Board, Layer};
    /// let mut b = Board { name: "b".into(), layers: vec![Layer { name: "F.Cu".into(), z: 1.6, eps_below: 4.2, loss_below: 0.0 }], segments: vec![], vias: vec![], copper: vec![], notes: vec![], region: None };
    /// b.map_lengths(|mm| mm * 1e-3);
    /// assert_eq!(b.layers[0].z, 1.6e-3);
    /// ```
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
            v.drill = v.drill.map(&mut convert);
        }
        for c in &mut self.copper {
            for p in &mut c.polygon {
                for n in p {
                    *n = convert(*n);
                }
            }
        }
        if let Some(r) = &mut self.region {
            for v in r.lo.iter_mut().chain(&mut r.hi).chain([&mut r.above, &mut r.below]) {
                *v = convert(*v);
            }
        }
    }
}

impl Region {
    /// Refuses a region that is not finite, has no area, or is larger than [`MAX_REGION`] on a
    /// side or in height.
    ///
    /// ```
    /// use emcraft_model::pcb::Region;
    /// assert!(Region { lo: [0.0, 0.0], hi: [0.01, 0.005], above: 0.002, below: 0.0 }.check().is_ok());
    /// assert!(Region { lo: [0.0, 0.0], hi: [0.0, 0.005], above: 0.002, below: 0.0 }.check().is_err());
    /// assert!(Region { lo: [0.0, 0.0], hi: [0.01, 0.005], above: f64::NAN, below: 0.0 }.check().is_err());
    /// ```
    pub fn check(&self) -> Result<()> {
        let side = |a: f64, b: f64| a.is_finite() && b.is_finite() && b > a && b - a <= MAX_REGION;
        let height = |h: f64| h.is_finite() && (0.0..=MAX_REGION).contains(&h);
        if side(self.lo[0], self.hi[0]) && side(self.lo[1], self.hi[1]) && height(self.above) && height(self.below) {
            Ok(())
        } else {
            Err(invalid(
                "PCB region",
                format!("needs finite corners, the second above and right of the first, sides and air of at most {MAX_REGION} m"),
            ))
        }
    }
}

/// The distance between two points of the board's plane.
///
/// ```
/// assert_eq!(emcraft_model::pcb::distance([0.0, 0.0], [3.0, 4.0]), 5.0);
/// ```
pub fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

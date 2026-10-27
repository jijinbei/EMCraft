//! The rectangular waveguide along z: its sections and their apertures, and the planar circuit
//! inside it (dielectric layers, conducting strips and vias).

use serde::{Deserialize, Serialize};

use crate::Axis;
use crate::error::{Result, all_finite, invalid, near, positive};
use crate::geom::{Aabb, Point3};

/// A rectangular waveguide along z: x ∈ [0, a], y ∈ [0, b], z from 0 through its sections. A
/// section may narrow the broad wall to an aperture of its own (an H-plane step, an inductive
/// iris); the narrow wall b is the same throughout.
///
/// Planar circuits in a shielding box are guides too: dielectric `layers` fill it from y = 0 up
/// (a substrate, its whole width and length), and conducting `strips` of no thickness lie on
/// planes y = const (a microstrip on the substrate, a stripline between two layers), ending
/// anywhere inside the box (open stubs, a stub off a line); `vias` are conducting posts (to
/// ground, or through the whole height).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Waveguide {
    /// Broad wall width a (m), along x: the widest aperture.
    pub width: f64,
    /// Narrow wall height b (m), along y.
    pub height: f64,
    pub sections: Vec<Section>,
    /// Layers from y = 0 up; above the last, each section's own material.
    #[serde(default)]
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub strips: Vec<Strip>,
    /// Conducting posts across the layers (a via from the ground to a strip, a shorting pin).
    #[serde(default)]
    pub vias: Vec<Via>,
}

/// A length of the waveguide filled with one material, with its own aperture across x.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Section {
    /// Length along z (m).
    pub length: f64,
    /// The material's name.
    pub material: String,
    /// The aperture's width along x (m); the guide's width when `None`.
    #[serde(default)]
    pub width: Option<f64>,
    /// Where the aperture starts along x (m); centred in the guide when `None`.
    #[serde(default)]
    pub offset: Option<f64>,
    /// The aperture's height along y (m); the guide's height when `None` (an E-plane step or a
    /// capacitive iris when smaller).
    #[serde(default)]
    pub height: Option<f64>,
    /// Where the aperture starts along y (m); centred when `None`.
    #[serde(default)]
    pub offset_y: Option<f64>,
}

/// A dielectric layer across the whole guide.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// Thickness along y (m).
    pub thickness: f64,
    pub material: String,
}

/// A perfectly conducting strip of no thickness: [x0, x0 + width] × [z0, z1] on the plane y.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Strip {
    pub y: f64,
    pub x0: f64,
    pub width: f64,
    pub z0: f64,
    pub z1: f64,
}

/// A perfectly conducting rectangular post: [x0, x0 + wx] × [y0, y1] × [z0, z0 + wz], solid
/// metal (the mesh leaves its cells out, so its faces are conducting walls). From y0 = 0 it is
/// grounded; ending on a strip's plane under the strip, it joins the two (a via); from y = 0 to
/// the height it shorts the guide's planes (a shorting pin through a stripline).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Via {
    pub x0: f64,
    pub z0: f64,
    pub wx: f64,
    pub wz: f64,
    pub y0: f64,
    pub y1: f64,
}

/// An interval along one axis: from `start`, `width` long (m).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: f64,
    pub width: f64,
}

impl Span {
    pub const fn new(start: f64, width: f64) -> Span {
        Span { start, width }
    }

    pub fn end(self) -> f64 {
        self.start + self.width
    }

    /// Whether `v` lies strictly inside.
    pub fn contains(self, v: f64) -> bool {
        v > self.start && v < self.end()
    }

    /// Whether `o` lies inside, within `tol`.
    pub fn holds(self, o: Span, tol: f64) -> bool {
        self.start <= o.start + tol && o.end() <= self.end() + tol
    }

    /// Whether the two are the same within `tol`.
    pub fn same(self, o: Span, tol: f64) -> bool {
        near([self.start, self.width], [o.start, o.width], tol)
    }
}

impl Section {
    /// A full-aperture section `length` (m) long of `material`.
    pub fn new(length: f64, material: &str) -> Section {
        Section { length, material: material.to_owned(), ..Default::default() }
    }

    /// Checks section `n` (counted from 1) on its own: a positive length and aperture, finite
    /// offsets, a material `known` names.
    fn check(&self, n: usize, known: &impl Fn(&str) -> Result<()>) -> Result<()> {
        positive(&format!("length of section {n}"), self.length)?;
        known(&self.material)?;
        if let Some(a) = self.width {
            positive(&format!("aperture width of section {n}"), a)?;
        }
        if let Some(o) = self.offset {
            all_finite(format!("aperture offset of section {n}"), &[o])?;
        }
        if let Some(h) = self.height {
            positive(&format!("aperture height of section {n}"), h)?;
        }
        if let Some(o) = self.offset_y {
            all_finite(format!("aperture y offset of section {n}"), &[o])?;
        }
        Ok(())
    }
}

impl Layer {
    /// Checks layer `n` (counted from 1): a positive thickness, a material `known` names.
    fn check(&self, n: usize, known: &impl Fn(&str) -> Result<()>) -> Result<()> {
        positive(&format!("thickness of layer {n}"), self.thickness)?;
        known(&self.material)
    }
}

/// The guide's box with the tolerances its circuit is placed to.
struct Room {
    width: f64,
    height: f64,
    length: f64,
    /// Across x.
    tol: f64,
    /// Across y.
    toly: f64,
}

impl Room {
    fn of(w: &Waveguide) -> Room {
        Room { width: w.width, height: w.height, length: w.length(), tol: 1e-9 * w.width, toly: 1e-9 * w.height }
    }

    /// Whether [x0, x0 + wx] lies within the width.
    fn holds_x(&self, x0: f64, wx: f64) -> bool {
        !(x0 < -self.tol || x0 + wx > self.width + self.tol)
    }

    /// Whether [z0, z1] lies within the length.
    fn holds_z(&self, z0: f64, z1: f64) -> bool {
        !(z0 < -1e-9 * self.length || z1 > self.length * (1.0 + 1e-9))
    }
}

impl Strip {
    /// Checks strip `n` (counted from 1) lies inside the guide.
    fn check(&self, n: usize, room: &Room) -> Result<()> {
        let what = || format!("strip {n}");
        all_finite(what(), &[self.y, self.x0, self.width, self.z0, self.z1])?;
        positive(&format!("width of strip {n}"), self.width)?;
        if !(self.y > room.toly && self.y < room.height - room.toly) {
            return Err(invalid(what(), "must lie inside the guide (0 < y < height)"));
        }
        if !(room.holds_x(self.x0, self.width) && room.holds_z(self.z0, self.z1) && self.z1 > self.z0) {
            return Err(invalid(what(), "must lie within the guide's width and length, z0 < z1"));
        }
        Ok(())
    }

    /// The strip mirrored about the plane `axis` = `span` / 2 (x or y).
    pub fn mirrored(self, axis: Axis, span: f64) -> Strip {
        match axis {
            Axis::X => Strip { x0: span - self.x0 - self.width, ..self },
            _ => Strip { y: span - self.y, ..self },
        }
    }

    /// Whether the two are the same within `tol`.
    pub fn near(&self, o: &Strip, tol: f64) -> bool {
        near([self.y, self.x0, self.width, self.z0, self.z1], [o.y, o.x0, o.width, o.z0, o.z1], tol)
    }
}

impl Via {
    /// The post as a box.
    pub fn bounds(&self) -> Aabb {
        Aabb::new(Point3::new(self.x0, self.y0, self.z0), Point3::new(self.x0 + self.wx, self.y1, self.z0 + self.wz))
    }

    /// Checks via `n` (counted from 1) runs inside the guide and leaves its ports' faces open.
    fn check(&self, n: usize, room: &Room) -> Result<()> {
        let what = || format!("via {n}");
        all_finite(what(), &[self.x0, self.z0, self.wx, self.wz, self.y0, self.y1])?;
        positive(&format!("x width of via {n}"), self.wx)?;
        positive(&format!("z width of via {n}"), self.wz)?;
        let toly = room.toly;
        if !(self.y0 >= -toly && self.y1 <= room.height + toly && self.y1 > self.y0 + toly) {
            return Err(invalid(what(), "must run within the guide's height, 0 ≤ y0 < y1 ≤ height"));
        }
        if !(room.holds_x(self.x0, self.wx) && room.holds_z(self.z0, self.z0 + self.wz)) {
            return Err(invalid(what(), "must lie within the guide's width and length"));
        }
        let (z1, length) = (self.z0 + self.wz, room.length);
        let across = self.wx >= room.width - room.tol && self.y0 <= toly && self.y1 >= room.height - toly;
        if across && (self.z0 <= 1e-9 * length || z1 >= length * (1.0 - 1e-9)) {
            return Err(invalid(what(), "fills a port's whole face: end the guide there instead"));
        }
        Ok(())
    }

    /// The post mirrored about the plane `axis` = `span` / 2 (x or y).
    pub fn mirrored(self, axis: Axis, span: f64) -> Via {
        match axis {
            Axis::X => Via { x0: span - self.x0 - self.wx, ..self },
            _ => Via { y0: span - self.y1, y1: span - self.y0, ..self },
        }
    }

    /// Whether the two are the same within `tol`.
    pub fn near(&self, o: &Via, tol: f64) -> bool {
        near([self.x0, self.z0, self.wx, self.wz, self.y0, self.y1], [o.x0, o.z0, o.wx, o.wz, o.y0, o.y1], tol)
    }
}

impl Waveguide {
    pub fn length(&self) -> f64 {
        self.sections.iter().map(|s| s.length).sum()
    }

    /// Section i's aperture across x.
    pub fn aperture(&self, i: usize) -> Option<Span> {
        let s = self.sections.get(i)?;
        let w = s.width.unwrap_or(self.width);
        Some(Span::new(s.offset.unwrap_or((self.width - w) / 2.0), w))
    }

    /// Section i's aperture across y.
    pub fn aperture_y(&self, i: usize) -> Option<Span> {
        let s = self.sections.get(i)?;
        let h = s.height.unwrap_or(self.height);
        Some(Span::new(s.offset_y.unwrap_or((self.height - h) / 2.0), h))
    }

    /// The whole width, [0, a].
    pub fn across_x(&self) -> Span {
        Span::new(0.0, self.width)
    }

    /// The whole height, [0, b].
    pub fn across_y(&self) -> Span {
        Span::new(0.0, self.height)
    }

    /// Whether every section has the full width (no H-plane change).
    pub fn full_width(&self) -> bool {
        (0..self.sections.len()).all(|i| self.aperture(i).is_some_and(|a| a.same(self.across_x(), 1e-12 * self.width)))
    }

    /// Whether every section has the full height (no E-plane change).
    pub fn full_height(&self) -> bool {
        (0..self.sections.len()).all(|i| self.aperture_y(i).is_some_and(|a| a.same(self.across_y(), 1e-12 * self.height)))
    }

    /// Whether every section has the full cross-section.
    pub fn uniform(&self) -> bool {
        self.full_width() && self.full_height()
    }

    /// Whether the guide is a plain one: no layers, no strips, no vias.
    pub fn plain(&self) -> bool {
        self.layers.is_empty() && self.strips.is_empty() && self.vias.is_empty()
    }

    /// The layer holding height y (the tops of the layers, bottom up), `None` above them.
    pub fn layer_at(&self, y: f64) -> Option<&Layer> {
        self.layers.iter().zip(self.layer_tops()).find(|(_, top)| y < *top).map(|(l, _)| l)
    }

    /// The planes y = const between layers.
    pub fn layer_tops(&self) -> Vec<f64> {
        running_sum(self.layers.iter().map(|l| l.thickness))
    }

    /// The z coordinates of the section boundaries, from 0 to the length.
    pub fn interfaces(&self) -> Vec<f64> {
        std::iter::once(0.0).chain(running_sum(self.sections.iter().map(|s| s.length))).collect()
    }

    /// Checks the whole guide: its box, every section (with materials `known` names), layer,
    /// strip and via, and that neighbouring apertures nest.
    pub fn check(&self, known: impl Fn(&str) -> Result<()>) -> Result<()> {
        positive("waveguide width", self.width)?;
        positive("waveguide height", self.height)?;
        if self.sections.is_empty() {
            return Err(invalid("waveguide", "needs at least one section"));
        }
        let room = Room::of(self);
        for (i, s) in self.sections.iter().enumerate() {
            s.check(i + 1, &known)?;
            if !self.across_x().holds(self.aperture(i).unwrap_or(self.across_x()), room.tol) {
                return Err(invalid(format!("section {}", i + 1), "its aperture must lie within the guide's width"));
            }
            if !self.across_y().holds(self.aperture_y(i).unwrap_or(self.across_y()), room.toly) {
                return Err(invalid(format!("section {}", i + 1), "its aperture must lie within the guide's height"));
            }
        }
        for (i, l) in self.layers.iter().enumerate() {
            l.check(i + 1, &known)?;
        }
        if self.layers.iter().map(|l| l.thickness).sum::<f64>() > self.height + room.toly {
            return Err(invalid("layers", "are thicker than the guide is high"));
        }
        let uniform = self.uniform();
        for (i, s) in self.strips.iter().enumerate() {
            s.check(i + 1, &room)?;
            if !uniform {
                return Err(invalid(format!("strip {}", i + 1), "not supported yet: strips in a guide whose aperture changes"));
            }
        }
        for (i, v) in self.vias.iter().enumerate() {
            v.check(i + 1, &room)?;
            if !uniform {
                return Err(invalid(format!("via {}", i + 1), "not supported yet: vias in a guide whose aperture changes"));
            }
        }
        if !self.layers.is_empty() && !uniform {
            return Err(invalid("layers", "not supported yet: layers in a guide whose aperture changes"));
        }
        // Neighbouring apertures must nest: one rectangle holds the other (steps, irises).
        let rect = |k: usize| self.aperture(k).zip(self.aperture_y(k));
        let holds = |(ax, ay): (Span, Span), (bx, by): (Span, Span)| ax.holds(bx, room.tol) && ay.holds(by, room.toly);
        for i in 1..self.sections.len() {
            if let (Some(p), Some(q)) = (rect(i - 1), rect(i))
                && !holds(p, q)
                && !holds(q, p)
            {
                return Err(invalid(
                    format!("sections {} and {}", i, i + 1),
                    "not supported yet: apertures that overlap only in part (one must hold the other)",
                ));
            }
        }
        Ok(())
    }
}

/// The running totals of `values`, in order.
fn running_sum(values: impl Iterator<Item = f64>) -> Vec<f64> {
    values
        .scan(0.0, |total, v| {
            *total += v;
            Some(*total)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sec(width: Option<f64>, offset: Option<f64>) -> Section {
        Section { length: 0.01, material: "vacuum".into(), width, offset, ..Default::default() }
    }

    fn guide(sections: Vec<Section>) -> Waveguide {
        Waveguide { width: 0.02, height: 0.01, sections, layers: Vec::new(), strips: Vec::new(), vias: Vec::new() }
    }

    fn vacuum_only(name: &str) -> Result<()> {
        if name == "vacuum" { Ok(()) } else { Err(crate::ModelError::Unknown { kind: "material", name: name.into() }) }
    }

    #[test]
    fn apertures_must_fit_and_nest() {
        let w = guide(vec![sec(None, None), sec(Some(0.01), None), sec(None, None)]);
        w.check(vacuum_only).unwrap();
        assert_eq!(w.aperture(1), Some(Span::new(0.005, 0.01)));
        assert!(!w.uniform());
        assert!(guide(vec![sec(Some(0.03), None)]).check(vacuum_only).is_err());
        assert!(guide(vec![sec(Some(0.01), Some(0.0)), sec(Some(0.01), Some(0.008))]).check(vacuum_only).is_err());
        assert!(guide(vec![sec(Some(0.01), Some(f64::NAN))]).check(vacuum_only).is_err());
        guide(vec![sec(Some(0.012), Some(0.0)), sec(Some(0.008), Some(0.002))]).check(vacuum_only).unwrap();
    }

    #[test]
    fn layers_stack_and_sections_meet() {
        let mut w = guide(vec![sec(None, None), Section::new(0.02, "vacuum")]);
        w.layers = vec![Layer { thickness: 0.002, material: "vacuum".into() }, Layer { thickness: 0.003, material: "vacuum".into() }];
        assert_eq!(w.interfaces(), vec![0.0, 0.01, 0.03]);
        assert_eq!(w.layer_tops(), vec![0.002, 0.005]);
        assert_eq!(w.layer_at(0.001).map(|l| l.thickness), Some(0.002));
        assert_eq!(w.layer_at(0.004).map(|l| l.thickness), Some(0.003));
        assert!(w.layer_at(0.006).is_none());
        w.check(vacuum_only).unwrap();
        w.layers.push(Layer { thickness: 0.01, material: "vacuum".into() });
        assert!(w.check(vacuum_only).is_err(), "thicker than the guide");
        w.layers[2] = Layer { thickness: 0.001, material: "glass".into() };
        assert!(matches!(w.check(vacuum_only), Err(crate::ModelError::Unknown { .. })));
    }

    #[test]
    fn strips_and_vias_lie_inside_the_guide() {
        let mut w = guide(vec![sec(None, None)]);
        let strip = Strip { y: 0.002, x0: 0.002, width: 0.004, z0: 0.0, z1: 0.01 };
        let via = Via { x0: 0.002, z0: 0.004, wx: 0.001, wz: 0.001, y0: 0.0, y1: 0.002 };
        w.strips = vec![strip];
        w.vias = vec![via];
        w.check(vacuum_only).unwrap();
        for bad in [Strip { y: 0.0, ..strip }, Strip { z1: 0.0, ..strip }, Strip { x0: 0.018, ..strip }, Strip { width: f64::NAN, ..strip }] {
            w.strips = vec![bad];
            assert!(w.check(vacuum_only).is_err(), "{bad:?}");
        }
        w.strips.clear();
        let across = Via { x0: 0.0, z0: 0.0, wx: 0.02, wz: 0.001, y0: 0.0, y1: 0.01 };
        for bad in [Via { y1: 0.0, ..via }, Via { z0: 0.0095, ..via }, Via { wx: 0.0, ..via }, across] {
            w.vias = vec![bad];
            assert!(w.check(vacuum_only).is_err(), "{bad:?}");
        }
        assert!(via.mirrored(Axis::X, 0.02).near(&Via { x0: 0.017, ..via }, 1e-15));
        assert!(via.mirrored(Axis::Y, 0.01).near(&Via { y0: 0.008, y1: 0.01, ..via }, 1e-15));
        assert!(strip.mirrored(Axis::X, 0.02).near(&Strip { x0: 0.014, ..strip }, 1e-15));
    }
}

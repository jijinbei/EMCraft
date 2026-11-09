//! Lumped circuit elements: a resistor, inductor and capacitor across a gap, in series or in
//! parallel — a passive load on the field (a chip resistor bridging two traces, a tuning
//! capacitor across a gap), not a port.

use num_complex::Complex64 as C;

use crate::units::angular;
use serde::{Deserialize, Serialize};

use crate::error::{ModelError, Result, invalid, named};
use crate::geom::{Point3, Vec3};
use crate::model::Model;

/// The most lumped elements a model may hold.
pub const MAX_ELEMENTS: usize = 256;

emcraft_macros::named_enum! {
    /// How an element's parts are connected.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum Arrangement {
        /// R + jωL + 1/(jωC), the parts left out (0) dropping from the sum.
        #[default]
        Series = "series",
        /// 1/(1/R + 1/(jωL) + jωC), the parts left out (0) dropping from the sum.
        Parallel = "parallel",
    }
}

emcraft_macros::named_serde!(Arrangement);

/// A lumped element across the gap from `from` to `to` (m): resistance (Ω), inductance (H) and
/// capacitance (F), 0 for a part it does not have, in series or in parallel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LumpedElement {
    pub name: String,
    pub from: Point3,
    pub to: Point3,
    #[serde(default)]
    pub resistance: f64,
    #[serde(default)]
    pub inductance: f64,
    #[serde(default)]
    pub capacitance: f64,
    #[serde(default)]
    pub arrangement: Arrangement,
    /// The current spread evenly across a sheet from `−across` to `+across` about the gap's line
    /// (m; across the line) — a chip's width — instead of along the line itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub across: Option<Vec3>,
}

impl LumpedElement {
    /// Checks the element's own settings: finite parts ≥ 0, at least one of them (a series
    /// element of none would be a short, a parallel one an open circuit), finite ends that
    /// differ, a sheet across the gap's line.
    pub fn check(&self) -> Result<()> {
        let parts = [self.resistance, self.inductance, self.capacitance];
        if !parts.iter().all(|x| x.is_finite() && *x >= 0.0) {
            return Err(invalid("lumped element", "its resistance, inductance and capacitance must be finite numbers ≥ 0"));
        }
        if parts.iter().all(|x| *x == 0.0) {
            return Err(invalid("lumped element", "give it a resistance, an inductance or a capacitance"));
        }
        if !(self.from.is_finite() && self.to.is_finite() && self.from.distance(self.to) > 0.0) {
            return Err(invalid("lumped element", "its ends must be finite and differ"));
        }
        if let Some(a) = self.across {
            let along = self.to - self.from;
            if !(a.is_finite() && a.norm() > 0.0 && a.dot(along).abs() <= 1e-9 * a.norm() * along.norm()) {
                return Err(invalid("lumped element sheet", "must be finite and across the gap's line"));
            }
        }
        Ok(())
    }

    /// The element's admittance (S) at `frequency` (Hz): zero for an open circuit (a series
    /// capacitor at no frequency, say), never infinite for the parts [`Self::check`] allows
    /// except a series resistor of 0 Ω with nothing else, which it refuses.
    ///
    /// ```
    /// use emcraft_model::{Arrangement, LumpedElement, Point3};
    /// let r = LumpedElement { name: "R1".into(), from: Point3::ORIGIN, to: Point3::new(0.0, 0.0, 1e-3), resistance: 100.0, inductance: 0.0, capacitance: 0.0, arrangement: Arrangement::Series, across: None };
    /// assert!((r.admittance(1e9).re - 0.01).abs() < 1e-15);
    /// // 1 pF in parallel with it: jωC more.
    /// let rc = LumpedElement { capacitance: 1e-12, arrangement: Arrangement::Parallel, ..r.clone() };
    /// assert!((rc.admittance(1e9).im - 2.0 * std::f64::consts::PI * 1e9 * 1e-12).abs() < 1e-15);
    /// assert!(LumpedElement { resistance: 0.0, ..r }.check().is_err());
    /// ```
    pub fn admittance(&self, frequency: f64) -> C {
        let w = angular(frequency);
        let (r, l, c) = (self.resistance, self.inductance, self.capacitance);
        match self.arrangement {
            Arrangement::Series => {
                let z = C::new(r, if l > 0.0 { w * l } else { 0.0 }) + if c > 0.0 { C::new(0.0, -1.0 / (w * c)) } else { C::new(0.0, 0.0) };
                if z.norm() > 0.0 { 1.0 / z } else { C::new(0.0, 0.0) }
            }
            Arrangement::Parallel => {
                let g = if r > 0.0 { 1.0 / r } else { 0.0 };
                let b = if c > 0.0 { w * c } else { 0.0 } - if l > 0.0 { 1.0 / (w * l) } else { 0.0 };
                C::new(g, b)
            }
        }
    }
}

impl Model {
    /// Adds a lumped element after checking it: a unique name (among ports and elements), its
    /// own settings, its gap inside the geometry.
    pub fn add_element(&mut self, e: LumpedElement) -> Result<()> {
        named("element name", &e.name)?;
        if self.has_port_named(&e.name) || self.elements.iter().any(|x| x.name == e.name) {
            return Err(ModelError::Duplicate { kind: "element", name: e.name });
        }
        if self.elements.len() >= MAX_ELEMENTS {
            return Err(invalid("lumped element", format!("at most {MAX_ELEMENTS}")));
        }
        e.check()?;
        if let Some(b) = self.bounds()
            && !(b.contains(e.from) && b.contains(e.to))
        {
            return Err(invalid("lumped element", "must lie inside the geometry"));
        }
        self.elements.push(e);
        Ok(())
    }

    /// Takes the element named `name` out of the model.
    pub fn remove_element(&mut self, name: &str) -> Option<LumpedElement> {
        let i = self.elements.iter().position(|x| x.name == name)?;
        Some(self.elements.remove(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn element(arrangement: Arrangement, r: f64, l: f64, c: f64) -> LumpedElement {
        LumpedElement {
            name: "e".into(),
            from: Point3::ORIGIN,
            to: Point3::new(0.0, 0.0, 1e-3),
            resistance: r,
            inductance: l,
            capacitance: c,
            arrangement,
            across: None,
        }
    }

    #[test]
    fn series_and_parallel_resonate_where_lc_says() {
        // Series and parallel RLC resonate at f₀ = 1/(2π√(LC)) (Pozar, Microwave Engineering, 4th
        // ed., §6.1).
        let (l, c): (f64, f64) = (1e-9, 1e-12);
        let f0 = 1.0 / (2.0 * PI * (l * c).sqrt());
        // In series a short at resonance (only R left), in parallel an open (only G left).
        let s = element(Arrangement::Series, 10.0, l, c).admittance(f0);
        assert!((s - C::new(0.1, 0.0)).norm() < 1e-9, "{s}");
        let p = element(Arrangement::Parallel, 10.0, l, c).admittance(f0);
        assert!((p - C::new(0.1, 0.0)).norm() < 1e-9, "{p}");
        // A lone inductor.
        let x = element(Arrangement::Series, 0.0, l, 0.0).admittance(1e9);
        assert!((x - C::new(0.0, -1.0 / (2.0 * PI * 1e9 * l))).norm() < 1e-12);
    }

    #[test]
    fn hostile_elements_are_refused() {
        assert!(element(Arrangement::Series, -1.0, 0.0, 0.0).check().is_err());
        assert!(element(Arrangement::Series, f64::NAN, 0.0, 0.0).check().is_err());
        assert!(element(Arrangement::Parallel, 0.0, 0.0, 0.0).check().is_err());
        let mut e = element(Arrangement::Series, 50.0, 0.0, 0.0);
        e.to = e.from;
        assert!(e.check().is_err());
        e.to = Point3::new(0.0, 0.0, 1e-3);
        e.across = Some(Vec3::new(0.0, 0.0, 1e-3));
        assert!(e.check().is_err(), "a sheet along the gap");
    }
}

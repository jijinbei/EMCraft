//! Mesh-wide quality: statistics per metric, and the validity gate meshers run on their output.

use crate::compat::{ElementKind, check_coordinates};

use super::{JacobianCheck, Metric, Validity, element_validity};
use crate::{Error, Result};

/// The volume gate's tolerance: the mesh's volume matches the boundary's within this fraction.
pub const VOLUME_TOLERANCE: f64 = 1e-9; // unit-factors: relative to the volume

/// Statistics of one metric over the measured elements.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricStats {
    pub metric: Metric,
    /// Elements that have this metric.
    pub count: usize,
    pub min: f64,
    pub max: f64,
    /// Mean of the finite values (`None` when there are none).
    pub mean: Option<f64>,
    /// The element with the worst value (the first one on ties).
    pub worst: u32,
    /// Counts over [`Metric::range`] in equal bins; values outside the range fall in the end bins.
    pub histogram: Vec<usize>,
}

/// Quality of a set of elements.
#[derive(Debug, Clone, PartialEq)]
pub struct QualityReport {
    /// Elements measured (solids and shells).
    pub measured: usize,
    /// Elements of kinds with no quality metrics (rods, beams), by kind.
    pub skipped: Vec<(ElementKind, usize)>,
    /// Solids proven valid.
    pub valid: usize,
    pub invalid: Vec<u32>,
    pub undetermined: Vec<u32>,
    /// Total signed volume of the solids and total area of the shells.
    pub volume: f64,
    pub area: f64,
    /// One entry per metric present, in [`Metric`] declaration order.
    pub metrics: Vec<MetricStats>,
}

/// An element that fails a quality limit, or is not proven valid.
#[derive(Debug, Clone, PartialEq)]
pub struct Marked {
    pub label: u32,
    pub kind: ElementKind,
    /// The mean of its nodes.
    pub centroid: [f64; 3],
    /// The certified validity, for solids.
    pub validity: Option<Validity>,
    /// Each limit it fails: the metric, its value and the limit.
    pub failed: Vec<(Metric, f64, f64)>,
    /// Its scaled Jacobian, when it has one (the worst-first order).
    pub scaled_jacobian: Option<f64>,
}

impl Marked {
    /// Not proven valid (inverted or undetermined).
    pub fn is_invalid(&self) -> bool {
        matches!(self.validity, Some(Validity::Invalid | Validity::Undetermined))
    }
}

/// What the validity gate checks besides element validity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GateOptions {
    /// The volume the mesh must fill: the boundary's enclosed volume ([`enclosed_volume`]).
    pub expected_volume: Option<f64>,
    /// Relative tolerance on the volume. Curved (quadratic) meshes of a faceted boundary need a
    /// looser one than linear meshes.
    pub volume_tolerance: f64,
}

impl Default for GateOptions {
    fn default() -> Self {
        Self { expected_volume: None, volume_tolerance: VOLUME_TOLERANCE }
    }
}

/// The result of the validity gate.
#[derive(Debug, Clone, PartialEq)]
pub struct GateReport {
    /// Solid elements checked (other kinds are ignored).
    pub checked: usize,
    pub invalid: Vec<u32>,
    pub undetermined: Vec<u32>,
    /// Total signed volume of the solids.
    pub volume: f64,
    pub options: GateOptions,
}

impl GateReport {
    /// Whether the volume matches the expected one (true when none is expected).
    pub fn volume_matches(&self) -> bool {
        self.options.expected_volume.is_none_or(|v| (self.volume - v).abs() <= self.options.volume_tolerance * v.abs())
    }

    /// Every solid is proven valid and the volume matches.
    pub fn passed(&self) -> bool {
        self.invalid.is_empty() && self.undetermined.is_empty() && self.volume_matches()
    }

    /// The report, or [`Error::Failed`] describing why the gate failed.
    pub fn into_result(self) -> Result<Self> {
        if self.passed() {
            return Ok(self);
        }
        let mut why: Vec<crate::message::Message> = Vec::new();
        let list = |v: &[u32]| v.iter().take(10).map(u32::to_string).collect::<Vec<_>>().join(", ");
        if !self.invalid.is_empty() {
            why.push(crate::message![format!("{} inverted or degenerate elements ({})", self.invalid.len(), list(&self.invalid))]);
        }
        if !self.undetermined.is_empty() {
            why.push(crate::message![format!(
                "{} elements whose validity could not be proven ({})",
                self.undetermined.len(),
                list(&self.undetermined)
            )]);
        }
        if let (false, Some(v)) = (self.volume_matches(), self.options.expected_volume) {
            why.push(crate::message![
                "mesh volume ",
                crate::message::volume(self.volume),
                " differs from the boundary's ",
                crate::message::volume(v)
            ]);
        }
        let mut message = crate::message!["mesh validity gate: "];
        for (i, w) in why.into_iter().enumerate() {
            if i > 0 {
                message.0.push("; ".into());
            }
            message.0.extend(w.0);
        }
        Err(Error::failed(message))
    }
}

/// Accumulates the gate's report one element at a time.
struct Gate(GateReport);

impl Gate {
    fn new(options: &GateOptions) -> Result<Self> {
        if !(options.volume_tolerance >= 0.0 && options.volume_tolerance.is_finite()) || options.expected_volume.is_some_and(|v| !v.is_finite()) {
            return Err(Error::Invalid("gate options must be finite, with a non-negative tolerance".into()));
        }
        Ok(Gate(GateReport { checked: 0, invalid: Vec::new(), undetermined: Vec::new(), volume: 0.0, options: *options }))
    }

    fn add(&mut self, label: u32, kind: ElementKind, xyz: &[[f64; 3]]) -> Result<()> {
        if kind.dimension() != 3 {
            return Ok(());
        }
        self.add_check(label, &element_validity(kind, xyz)?);
        Ok(())
    }

    fn add_check(&mut self, label: u32, check: &JacobianCheck) {
        let r = &mut self.0;
        r.checked += 1;
        r.volume += check.volume;
        match check.validity {
            Validity::Valid => {}
            Validity::Invalid => r.invalid.push(label),
            Validity::Undetermined => r.undetermined.push(label),
        }
    }
}

/// [`validity_gate_elements`] over certified checks already made, for a mesher that measured its
/// elements with [`super::element_quality`] (which certifies them too) and should not certify
/// them twice: `(label, check)` per solid element.
pub fn validity_gate_checks<'a, I>(checks: I, options: &GateOptions) -> Result<GateReport>
where
    I: IntoIterator<Item = (u32, &'a JacobianCheck)>,
{
    let mut gate = Gate::new(options)?;
    for (label, check) in checks {
        gate.add_check(label, check);
    }
    Ok(gate.0)
}

/// The validity gate every mesher runs on its output before it reaches the model: each solid
/// element's Jacobian determinant is proven positive everywhere, and the elements' total volume
/// equals the volume the boundary encloses. Elements come as `(label, kind, node coordinates in
/// node order)`, straight from the mesher's arrays; other than solids are ignored. Call
/// [`GateReport::into_result`] to turn a failure into an error.
pub fn validity_gate_elements<I, E>(elements: I, options: &GateOptions) -> Result<GateReport>
where
    I: IntoIterator<Item = (u32, ElementKind, E)>,
    E: AsRef<[[f64; 3]]>,
{
    let mut gate = Gate::new(options)?;
    for (label, kind, xyz) in elements {
        gate.add(label, kind, xyz.as_ref())?;
    }
    Ok(gate.0)
}

/// The signed volume enclosed by a closed triangle surface (positive when the triangles' right-
/// hand normals point outwards), by the divergence theorem.
pub fn enclosed_volume(points: &[[f64; 3]], triangles: &[[usize; 3]]) -> Result<f64> {
    check_coordinates(points.as_flattened())?;
    let o = points.first().copied().unwrap_or([0.0; 3]);
    let mut six_v = 0.0;
    for t in triangles {
        let p = |i: usize| points.get(i).map(|p| super::shape::sub(*p, o)).ok_or_else(|| Error::Invalid(format!("triangle {t:?}: no point {i}")));
        six_v += super::shape::det(p(t[0])?, p(t[1])?, p(t[2])?);
    }
    Ok(six_v / 6.0)
}

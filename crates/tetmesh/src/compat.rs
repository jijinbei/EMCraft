//! What the ported meshing code took from FieldCraft's model crate: element kinds, the Jacobian
//! admissibility rule, coordinate checks, size caps, number formatting and a no-op progress
//! channel. Adapted from FieldCraft's `crates/model` (MIT OR Apache-2.0), see ATTRIBUTION.md.

/// The largest number of nodes and of elements a mesh may have.
pub const MAX_NODES: usize = 50_000_000;
pub const MAX_ELEMENTS: usize = 50_000_000;
/// A cap on hostile coordinates (m).
pub const MAX_COORD: f64 = 1e9;

/// Every coordinate a finite number within ±[`MAX_COORD`].
pub fn check_coordinates(xyz: &[f64]) -> crate::Result<()> {
    match xyz.iter().find(|c| !(c.abs() <= MAX_COORD)) {
        Some(c) => Err(crate::Error::Invalid(format!("coordinate {c} is not a finite number within ±{MAX_COORD}"))),
        None => Ok(()),
    }
}

/// The smallest normalized Jacobian of an admissible element.
pub const MIN_NORMALIZED_JACOBIAN: f64 = 1e-12;

/// `det J / (|J₁| |J₂| |J₃|)`, in [−1, 1]; 0 for a degenerate or non-finite matrix.
pub fn normalized_jacobian(j: &[[f64; 3]; 3]) -> f64 {
    let [a, b, c] = j;
    let det = a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]);
    let norms: f64 = j.iter().map(|r| (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt()).product();
    let q = det / norms;
    if q.is_finite() { q.clamp(-1.0, 1.0) } else { 0.0 }
}

pub fn admissible_jacobian(j: &[[f64; 3]; 3]) -> bool {
    normalized_jacobian(j) > MIN_NORMALIZED_JACOBIAN
}

/// The Jacobian of a linear tetrahedron: its edges from the first corner.
pub fn tet4_jacobian(xyz: &[[f64; 3]; 4]) -> [[f64; 3]; 3] {
    let [o, a, b, c] = xyz;
    let e = |p: &[f64; 3]| [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
    [e(a), e(b), e(c)]
}

/// The physical quantity of a number in a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantity {
    Length,
    Area,
    Volume,
}

impl Quantity {
    pub fn si_symbol(self) -> &'static str {
        match self {
            Quantity::Length => "m",
            Quantity::Area => "m²",
            Quantity::Volume => "m³",
        }
    }
}

/// A short number: six significant figures, scientific outside 10⁻⁴..10⁷.
pub fn format_number(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return if v == 0.0 { "0".to_string() } else { v.to_string() };
    }
    let exp = v.abs().log10().floor();
    if (-4.0..7.0).contains(&exp) {
        let decimals = (5.0 - exp).max(0.0) as usize;
        let s = format!("{v:.decimals$}");
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
    } else {
        let s = format!("{v:.5e}");
        match s.split_once('e') {
            Some((mantissa, e)) => {
                let m = if mantissa.contains('.') { mantissa.trim_end_matches('0').trim_end_matches('.') } else { mantissa };
                format!("{m}e{e}")
            }
            None => s,
        }
    }
}

/// Progress reporting: EMCraft's meshing runs to the end (no cancellation yet).
pub mod progress {
    pub fn check() -> crate::Result<()> {
        Ok(())
    }
    pub fn stage(_: &str, _: Option<f64>, _: &str) {}
    pub fn count(_: f64) {}
}

/// The element kinds of a mesh (FieldCraft's table; EMCraft meshes tetrahedra).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ElementKind {
    /// Two-node axial member (truss).
    Rod2,
    /// Three-node axial member (quadratic truss, straight or curved).
    Rod3,
    /// Two-node beam.
    Beam2,
    /// Three-node triangle (shell or membrane).
    Tri3,
    /// Six-node triangle.
    Tri6,
    /// Four-node quadrilateral.
    Quad4,
    /// Eight-node quadrilateral.
    Quad8,
    /// Four-node tetrahedron.
    Tet4,
    /// Ten-node tetrahedron.
    Tet10,
    /// Six-node wedge (pentahedron).
    Wedge6,
    /// Five-node pyramid: a quadrilateral base and an apex; the transition between a hexahedron's
    /// face and tetrahedra.
    Pyramid5,
    /// Eight-node hexahedron.
    Hex8,
    /// Twenty-node hexahedron.
    Hex20,
}

impl ElementKind {
    pub const ALL: [ElementKind; 13] = [
        ElementKind::Rod2,
        ElementKind::Rod3,
        ElementKind::Beam2,
        ElementKind::Tri3,
        ElementKind::Tri6,
        ElementKind::Quad4,
        ElementKind::Quad8,
        ElementKind::Tet4,
        ElementKind::Tet10,
        ElementKind::Wedge6,
        ElementKind::Pyramid5,
        ElementKind::Hex8,
        ElementKind::Hex20,
    ];

    pub fn node_count(self) -> usize {
        match self {
            ElementKind::Rod2 | ElementKind::Beam2 => 2,
            ElementKind::Rod3 | ElementKind::Tri3 => 3,
            ElementKind::Quad4 | ElementKind::Tet4 => 4,
            ElementKind::Pyramid5 => 5,
            ElementKind::Tri6 | ElementKind::Wedge6 => 6,
            ElementKind::Quad8 | ElementKind::Hex8 => 8,
            ElementKind::Tet10 => 10,
            ElementKind::Hex20 => 20,
        }
    }

    /// Corner nodes: the first ones in the node list (the rest are mid-side nodes).
    pub fn corner_count(self) -> usize {
        match self {
            ElementKind::Rod2 | ElementKind::Rod3 | ElementKind::Beam2 => 2,
            ElementKind::Tri3 | ElementKind::Tri6 => 3,
            ElementKind::Quad4 | ElementKind::Quad8 | ElementKind::Tet4 | ElementKind::Tet10 => 4,
            ElementKind::Pyramid5 => 5,
            ElementKind::Wedge6 => 6,
            ElementKind::Hex8 | ElementKind::Hex20 => 8,
        }
    }

    /// Whether the element has mid-side nodes.
    pub fn is_quadratic(self) -> bool {
        self.node_count() > self.corner_count()
    }

    /// Topological dimension: 1 for line elements, 2 for shells, 3 for solids.
    pub fn dimension(self) -> u8 {
        match self {
            ElementKind::Rod2 | ElementKind::Rod3 | ElementKind::Beam2 => 1,
            ElementKind::Tri3 | ElementKind::Tri6 | ElementKind::Quad4 | ElementKind::Quad8 => 2,
            ElementKind::Tet4 | ElementKind::Tet10 | ElementKind::Wedge6 | ElementKind::Pyramid5 | ElementKind::Hex8 | ElementKind::Hex20 => 3,
        }
    }

    /// The name used in commands and `.fieldcraft` files (`hex8`, `tet10`, …).
    pub fn name(self) -> &'static str {
        match self {
            ElementKind::Rod2 => "rod2",
            ElementKind::Rod3 => "rod3",
            ElementKind::Beam2 => "beam2",
            ElementKind::Tri3 => "tri3",
            ElementKind::Tri6 => "tri6",
            ElementKind::Quad4 => "quad4",
            ElementKind::Quad8 => "quad8",
            ElementKind::Tet4 => "tet4",
            ElementKind::Tet10 => "tet10",
            ElementKind::Wedge6 => "wedge6",
            ElementKind::Pyramid5 => "pyramid5",
            ElementKind::Hex8 => "hex8",
            ElementKind::Hex20 => "hex20",
        }
    }

    pub fn from_name(name: &str) -> Option<ElementKind> {
        ElementKind::ALL.into_iter().find(|k| k.name() == name)
    }

    /// The faces, numbered from 1 in this order, as positions in the element's node list: the
    /// corners, then (quadratic kinds) the mid-side nodes in the same cyclic order.
    ///
    /// - Solids: each face's corners run counter-clockwise seen from outside, so the right-hand
    ///   normal points out of the element. Hexahedra: 1 is the face of nodes 1-2-3-4, 2 the face
    ///   of 5-6-7-8, then the four sides along edges 1-2, 2-3, 3-4 and 4-1. Tetrahedra: 1 is
    ///   1-2-3, then the faces opposite node 3, node 1 and node 2. Wedges: the two triangles, then
    ///   the sides along edges 1-2, 2-3 and 3-1. Pyramids: 1 is the base 1-2-3-4, then the
    ///   triangles on edges 1-2, 2-3, 3-4 and 4-1 with the apex 5.
    /// - Shells: face 1 is the side the element's right-hand normal points to, face 2 the other.
    /// - Line elements have none.
    pub fn faces(self) -> &'static [&'static [usize]] {
        match self {
            ElementKind::Rod2 | ElementKind::Rod3 | ElementKind::Beam2 => &[],
            ElementKind::Tri3 => &[&[0, 1, 2], &[0, 2, 1]],
            ElementKind::Tri6 => &[&[0, 1, 2, 3, 4, 5], &[0, 2, 1, 5, 4, 3]],
            ElementKind::Quad4 => &[&[0, 1, 2, 3], &[0, 3, 2, 1]],
            ElementKind::Quad8 => &[&[0, 1, 2, 3, 4, 5, 6, 7], &[0, 3, 2, 1, 7, 6, 5, 4]],
            ElementKind::Tet4 => &[&[0, 2, 1], &[0, 1, 3], &[1, 2, 3], &[0, 3, 2]],
            ElementKind::Tet10 => &[&[0, 2, 1, 6, 5, 4], &[0, 1, 3, 4, 8, 7], &[1, 2, 3, 5, 9, 8], &[0, 3, 2, 7, 9, 6]],
            ElementKind::Wedge6 => &[&[0, 2, 1], &[3, 4, 5], &[0, 1, 4, 3], &[1, 2, 5, 4], &[2, 0, 3, 5]],
            ElementKind::Pyramid5 => &[&[0, 3, 2, 1], &[0, 1, 4], &[1, 2, 4], &[2, 3, 4], &[3, 0, 4]],
            ElementKind::Hex8 => &[&[0, 3, 2, 1], &[4, 5, 6, 7], &[0, 1, 5, 4], &[1, 2, 6, 5], &[2, 3, 7, 6], &[3, 0, 4, 7]],
            ElementKind::Hex20 => &[
                &[0, 3, 2, 1, 11, 10, 9, 8],
                &[4, 5, 6, 7, 12, 13, 14, 15],
                &[0, 1, 5, 4, 8, 17, 12, 16],
                &[1, 2, 6, 5, 9, 18, 13, 17],
                &[2, 3, 7, 6, 10, 19, 14, 18],
                &[3, 0, 4, 7, 11, 16, 15, 19],
            ],
        }
    }

    /// The edges, numbered from 1 in this order, as positions in the element's node list: the two
    /// corners, then the mid-side node for quadratic kinds. The order follows the mid-side nodes
    /// (see [`ElementKind`]); linear kinds use the same order.
    pub fn edges(self) -> &'static [&'static [usize]] {
        match self {
            ElementKind::Rod2 | ElementKind::Beam2 => &[&[0, 1]],
            ElementKind::Rod3 => &[&[0, 1, 2]],
            ElementKind::Tri3 => &[&[0, 1], &[1, 2], &[2, 0]],
            ElementKind::Tri6 => &[&[0, 1, 3], &[1, 2, 4], &[2, 0, 5]],
            ElementKind::Quad4 => &[&[0, 1], &[1, 2], &[2, 3], &[3, 0]],
            ElementKind::Quad8 => &[&[0, 1, 4], &[1, 2, 5], &[2, 3, 6], &[3, 0, 7]],
            ElementKind::Tet4 => &[&[0, 1], &[1, 2], &[2, 0], &[0, 3], &[1, 3], &[2, 3]],
            ElementKind::Tet10 => &[&[0, 1, 4], &[1, 2, 5], &[2, 0, 6], &[0, 3, 7], &[1, 3, 8], &[2, 3, 9]],
            ElementKind::Wedge6 => &[&[0, 1], &[1, 2], &[2, 0], &[3, 4], &[4, 5], &[5, 3], &[0, 3], &[1, 4], &[2, 5]],
            ElementKind::Pyramid5 => &[&[0, 1], &[1, 2], &[2, 3], &[3, 0], &[0, 4], &[1, 4], &[2, 4], &[3, 4]],
            ElementKind::Hex8 => &[&[0, 1], &[1, 2], &[2, 3], &[3, 0], &[4, 5], &[5, 6], &[6, 7], &[7, 4], &[0, 4], &[1, 5], &[2, 6], &[3, 7]],
            ElementKind::Hex20 => &[
                &[0, 1, 8],
                &[1, 2, 9],
                &[2, 3, 10],
                &[3, 0, 11],
                &[4, 5, 12],
                &[5, 6, 13],
                &[6, 7, 14],
                &[7, 4, 15],
                &[0, 4, 16],
                &[1, 5, 17],
                &[2, 6, 18],
                &[3, 7, 19],
            ],
        }
    }
}

/// How firmly a mesher keeps a seed's count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SeedConstraint {
    /// Exactly the seeded count, or the mesher refuses.
    #[default]
    Exact,
    /// At least the seeded count.
    Min,
    /// A goal the mesher may change.
    Free,
}

impl SeedConstraint {
    pub fn name(self) -> &'static str {
        match self {
            SeedConstraint::Exact => "exact",
            SeedConstraint::Min => "min",
            SeedConstraint::Free => "free",
        }
    }
}

/// Which end of the edge gets the small elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiasToward {
    Start,
    End,
    Both,
}

/// Geometric grading along the edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bias {
    /// The largest element over the smallest (at least 1).
    pub ratio: f64,
    pub toward: BiasToward,
}

/// The most divisions a seed may ask for.
pub const MAX_SEED_COUNT: u32 = 100_000;

pub mod seeds {
    use super::{Bias, BiasToward, MAX_SEED_COUNT};

    /// `n + 1` node positions in [0, 1] for `n` divisions graded by `bias` (uniform without one).
    pub fn spacing(n: u32, bias: Option<Bias>) -> Vec<f64> {
        let n = n.clamp(1, MAX_SEED_COUNT) as usize;
        let lengths: Vec<f64> = match bias {
            Some(b) if b.ratio > 1.0 && n > 1 => match b.toward {
                BiasToward::Start | BiasToward::End => {
                    let q = b.ratio.powf(1.0 / (n - 1) as f64);
                    let grow: Vec<f64> = (0..n).map(|i| q.powi(i as i32)).collect();
                    if b.toward == BiasToward::Start { grow } else { grow.into_iter().rev().collect() }
                }
                BiasToward::Both => {
                    let half = n.div_ceil(2);
                    let q = if half > 1 { b.ratio.powf(1.0 / (half - 1) as f64) } else { 1.0 };
                    (0..n).map(|i| q.powi(i.min(n - 1 - i) as i32)).collect()
                }
            },
            _ => vec![1.0; n],
        };
        let total: f64 = lengths.iter().sum();
        let mut out = Vec::with_capacity(n + 1);
        let mut at = 0.0;
        out.push(0.0);
        for l in lengths.iter().take(n - 1) {
            at += l / total;
            out.push(at);
        }
        out.push(1.0);
        out
    }
}

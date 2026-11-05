//! Periodic boundaries: opposite faces of a unit cell tied by Floquet's phase.
//!
//! **The condition.** A cell periodic with period L_d along axis d carries fields with
//! E(r + L_d ê_d) = e^{−jψ_d} E(r) (J.-M. Jin, *The Finite Element Method in Electromagnetics*,
//! 3rd ed., Wiley 2014, the chapter on periodic structures; the master/slave elimination as in
//! D. T. McGrath, V. P. Pyati, *Phased array antenna analysis with the hybrid finite element
//! method*, IEEE Trans. Antennas Propag. 42(12):1625–1630, 1994). On the mesh, every unknown
//! on a face x_d = L_d (a slave: an edge or a face whose nodes all lie there) is the master's on
//! the opposite face, x_s = σ e^{−jψ·s} x_m, s the periods between them (1 per axis crossed;
//! an edge along a corner of the cell crosses two) and σ = ±1 the edges' orientations (edges
//! run from the lower global node; a translate may run the other way). With T the map from the
//! kept unknowns to all of them, x = T x_r.
//!
//! **The test space.** The boundary terms on the two faces cancel only when the test functions
//! carry the conjugate phase, W(r + L) = e^{+jψ} W(r): the Galerkin system is
//! Tᴴ A T x_r = Tᴴ b. A is complex symmetric, but with a complex phase Tᴴ A T is not — for a
//! lossless cell it is Hermitian, and in general neither: T̄ ≠ T unless e^{−jψ} is real
//! (ψ = 0 or π). No choice of unknowns keeps it complex symmetric (the Bloch operator at ψ is the
//! transpose of the one at −ψ, by reciprocity), so the system is factored by the unsymmetric
//! multifrontal LU ([`crate::lu`]) on its symmetric pattern, not by the LDLᵀ.
//!
//! The reduction is applied to the assembled matrix, entry by entry: A's entry (i, j) adds
//! σ_iσ_j e^{−jψ·(s_j − s_i)} a_ij to the kept pair (R(i), R(j)). Everything else — the volume
//! and wall matrices, the ports' faces — is assembled as without the condition, and the fields
//! are read from x = T x_r as without it. Unknowns whose master or slave lies on a conducting
//! wall are dropped (tangential E = 0 there and, by the condition, on the other face).
//!
//! **Meshes.** The opposite faces must be translates node for node. The structured waveguide
//! mesh is one by construction (the same grid lines on both faces, every cell split the same
//! way); an unstructured solid is refused (its mesher does not make periodic surface meshes
//! yet). At order 2 the translates must keep the faces' node order (they do on the structured
//! mesh), since a face's two functions depend on it.

use std::collections::HashMap;

use emcraft_fem::Order;
use emcraft_mesh::Topology;
use emcraft_model::{Mesh, Periodic, Point3, Rotation};
use num_complex::Complex64 as C;

use super::{Dofs, Result, Slot, invalid};
use crate::sparse::Csr;

/// Where a full unknown goes: its kept unknown, its orientation and the periods to its master.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tied {
    reduced: usize,
    sign: f64,
    shift: [i8; 3],
}

/// The periodic reduction x = T x_r of a mesh's unknowns.
#[derive(Clone, Debug)]
pub struct Tie {
    /// Per full unknown, `None` when dropped.
    map: Vec<Option<Tied>>,
    /// Kept unknowns.
    pub reduced: usize,
    /// A point per kept unknown, for the ordering.
    pub points: Vec<Point3>,
    /// The cell's periods (m) along x, y, z (0 where not periodic).
    pub periods: [f64; 3],
    /// The axes tied.
    pub axes: [bool; 3],
    /// The kept pattern and, for each entry of the full pattern, its entry there (none when
    /// dropped) with its phase's period difference and sign product.
    pattern: Csr,
    entries: Vec<(Slot, [i8; 3], f64)>,
    /// Each node's master node and the periods to it.
    nodes: Vec<(u32, [i8; 3])>,
}

/// The faces of the cell: per axis, (low, high) coordinates of the mesh's extent.
fn extent(mesh: &Mesh) -> [(f64, f64); 3] {
    let mut out = [(f64::INFINITY, f64::NEG_INFINITY); 3];
    for p in &mesh.nodes {
        for (d, o) in out.iter_mut().enumerate() {
            o.0 = o.0.min(p[d]);
            o.1 = o.1.max(p[d]);
        }
    }
    out
}

/// Whether a boundary face (its nodes) lies on a face of the cell tied by `periodic`.
pub(super) fn on_tied_face(mesh: &Mesh, periodic: &Periodic, nodes: &[u32; 3]) -> bool {
    let ext = extent(mesh);
    periodic.axes().into_iter().any(|d| {
        let (lo, hi) = ext[d];
        let tol = 1e-9 * (hi - lo).abs().max(f64::MIN_POSITIVE);
        [lo, hi].iter().any(|&c| nodes.iter().all(|n| mesh.nodes.get(*n as usize).is_some_and(|p| (p[d] - c).abs() <= tol)))
    })
}

/// Whether a boundary face (its nodes) lies on one of a sector's two cut faces: tied, not a wall.
pub(super) fn on_cut_face(mesh: &Mesh, rot: &Rotation, nodes: &[u32; 3]) -> bool {
    let size = extent(mesh).iter().map(|(a, b)| b - a).fold(0.0f64, f64::max).max(f64::MIN_POSITIVE);
    // As the partners are matched (the mesher rounds a node by about 1e-7 of the size).
    let tol = 1e-6 * size;
    [rot.start(), rot.start() + rot.angle()].iter().any(|&angle| {
        nodes.iter().all(|n| {
            mesh.nodes.get(*n as usize).is_some_and(|p| {
                let (x, y) = (p.x - rot.centre[0], p.y - rot.centre[1]);
                (-x * angle.sin() + y * angle.cos()).abs() <= tol && x * angle.cos() + y * angle.sin() >= -tol
            })
        })
    })
}

/// The phase factor e^{−jψ·s}.
fn phase(psi: [f64; 3], s: [i8; 3]) -> C {
    let a: f64 = (0..3).map(|d| psi[d] * s[d] as f64).sum();
    C::new(a.cos(), -a.sin())
}

/// The nodes' partners across the tied faces: per axis, each node on the high face's partner
/// on the low face.
struct Partners {
    partner: [Vec<Slot>; 3],
    on_high: [Vec<bool>; 3],
}

impl Partners {
    /// The partners of `mesh`'s nodes along `axes`, and the cell's periods (0 where not
    /// periodic).
    fn new(mesh: &Mesh, axes: [bool; 3]) -> Result<(Partners, [f64; 3])> {
        let ext = extent(mesh);
        let mut periods = [0.0; 3];
        let mut partner: [Vec<Slot>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        let mut on_high: [Vec<bool>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for d in (0..3).filter(|&d| axes[d]) {
            let (lo, hi) = ext[d];
            let span = hi - lo;
            if !(span > 0.0 && span.is_finite()) {
                return Err(invalid("periodic boundaries", "the mesh has no extent along a periodic axis"));
            }
            periods[d] = span;
            let tol = 1e-9 * span;
            let size = ext.iter().map(|(a, b)| b - a).fold(0.0f64, f64::max);
            let q = 1e-7 * size;
            let key = |p: &Point3| -> [i64; 2] {
                let o: Vec<usize> = (0..3).filter(|&e| e != d).collect();
                [(p[o[0]] / q).round() as i64, (p[o[1]] / q).round() as i64]
            };
            let low: HashMap<[i64; 2], u32> =
                mesh.nodes.iter().enumerate().filter(|(_, p)| (p[d] - lo).abs() <= tol).map(|(i, p)| (key(p), i as u32)).collect();
            let mut part = vec![Slot::NONE; mesh.nodes.len()];
            let mut high = vec![false; mesh.nodes.len()];
            for (i, p) in mesh.nodes.iter().enumerate() {
                if (p[d] - hi).abs() <= tol {
                    let m = low.get(&key(p)).copied().ok_or_else(|| {
                        invalid(
                            "periodic boundaries",
                            format!(
                                "the mesh's faces at {} = {lo} and {hi} m do not match node for node (periodic boundaries need opposite faces that are translates: the structured waveguide mesh)",
                                ["x", "y", "z"][d]
                            ),
                        )
                    })?;
                    part[i] = Slot::from(m);
                    high[i] = true;
                }
            }
            if low.len() != high.iter().filter(|h| **h).count() {
                return Err(invalid("periodic boundaries", format!("the mesh's faces across {} have different nodes", ["x", "y", "z"][d])));
            }
            partner[d] = part;
            on_high[d] = high;
        }
        Ok((Partners { partner, on_high }, periods))
    }

    /// The partners of a sector's nodes ([`Rotation`]): each node on the second cut face (the
    /// axis included) and its image on the first, turned back a sector — a node on the axis is
    /// its own. The one "period" is the turn, along slot 0.
    fn rotational(mesh: &Mesh, rot: &Rotation) -> Result<Partners> {
        let size = extent(mesh).iter().map(|(a, b)| b - a).fold(0.0f64, f64::max).max(f64::MIN_POSITIVE);
        // The mesher's own coordinates round a node by about 1e-7 of the size: images are matched
        // to 1e-6 of it, far below any element.
        let (tol, q) = (1e-6 * size, 1e-6 * size);
        let on = |angle: f64, p: &Point3| {
            let (x, y) = (p.x - rot.centre[0], p.y - rot.centre[1]);
            (-x * angle.sin() + y * angle.cos()).abs() <= tol && x * angle.cos() + y * angle.sin() >= -tol
        };
        let (a0, a1) = (rot.start(), rot.start() + rot.angle());
        let key = |p: Point3| [(p.x / q).round() as i64, (p.y / q).round() as i64, (p.z / q).round() as i64];
        let first: HashMap<[i64; 3], u32> = mesh.nodes.iter().enumerate().filter(|(_, p)| on(a0, p)).map(|(i, p)| (key(*p), i as u32)).collect();
        let mut part = vec![Slot::NONE; mesh.nodes.len()];
        let mut high = vec![false; mesh.nodes.len()];
        for (i, p) in mesh.nodes.iter().enumerate() {
            if !on(a1, p) {
                continue;
            }
            let back = rot.turned_by(*p, -rot.angle());
            // The node within q of the image (its key's neighbours too: rounding may part them).
            let k = key(back);
            let found = (-1..=1)
                .flat_map(|i| (-1..=1).flat_map(move |j| (-1..=1).map(move |l| [k[0] + i, k[1] + j, k[2] + l])))
                .filter_map(|kk| first.get(&kk).copied())
                .find(|&m| mesh.nodes.get(m as usize).is_some_and(|a| (*a - back).norm() <= q));
            let m = found.ok_or_else(|| {
                invalid("rotational symmetry", "the sector's cut faces do not match node for node (the mesh must be made with the rotation set)")
            })?;
            part[i] = Slot::from(m);
            high[i] = true;
        }
        let empty = || (Vec::new(), Vec::new());
        let (p1, h1) = empty();
        let (p2, h2) = empty();
        Ok(Partners { partner: [part, p1, p2], on_high: [high, h1, h2] })
    }

    /// The periods from node `n`'s master: 1 along each axis whose high face it lies on.
    fn shift_of(&self, n: u32) -> [i8; 3] {
        [0, 1, 2].map(|d| self.on_high[d].get(n as usize).is_some_and(|h| *h) as i8)
    }

    /// Node `n` moved back by the periods `s`.
    fn translate(&self, n: u32, s: [i8; 3]) -> u32 {
        let mut n = Slot::from(n);
        for d in 0..3 {
            if s[d] == 1 {
                n = n.get().and_then(|i| self.partner[d].get(i)).copied().unwrap_or(Slot::NONE);
            }
        }
        n.0
    }
}

/// A slot's master slot, its orientation relative to it, and the periods to it.
#[derive(Clone, Copy)]
struct Master {
    slot: usize,
    sign: f64,
    shift: [i8; 3],
}

/// Each slot's master (itself when it lies on no high face): an edge's or a face's translate on
/// the low faces.
fn masters(topo: &Topology, dofs: &Dofs, partners: &Partners) -> Result<Vec<Master>> {
    let edge_id: HashMap<[u32; 2], u32> = topo.edges.iter().enumerate().map(|(i, e)| (*e, i as u32)).collect();
    let face_id: HashMap<[u32; 3], u32> = topo.faces.iter().enumerate().map(|(i, f)| (*f, i as u32)).collect();
    let (pe, pf) = (dofs.order.per_edge(), dofs.order.per_face());
    let total = dofs.edges * pe + dofs.faces * pf;
    let mut master: Vec<Master> = (0..total).map(|g| Master { slot: g, sign: 1.0, shift: [0i8; 3] }).collect();
    for (e, &[a, b]) in topo.edges.iter().enumerate() {
        let (sa, sb) = (partners.shift_of(a), partners.shift_of(b));
        let s = [0, 1, 2].map(|d| sa[d].min(sb[d]));
        if s == [0; 3] {
            continue;
        }
        let (ma, mb) = (partners.translate(a, s), partners.translate(b, s));
        let key = [ma.min(mb), ma.max(mb)];
        let m =
            *edge_id.get(&key).ok_or_else(|| invalid("periodic boundaries", "an edge on a periodic face has no translate on the opposite face"))?;
        let sign = if ma < mb { 1.0 } else { -1.0 };
        if sign < 0.0 && dofs.order == Order::Two {
            return Err(invalid("periodic boundaries", "not supported yet: order 2 on a mesh whose translates reverse an edge (use order 1)"));
        }
        for j in 0..pe {
            // The Whitney function turns with its edge, the edge's gradient ∇(λ_iλ_j) does not.
            master[e * pe + j] = Master { slot: m as usize * pe + j, sign: if j == 0 { sign } else { 1.0 }, shift: s };
        }
    }
    if pf > 0 {
        for (f, nodes) in topo.faces.iter().enumerate() {
            let sh = nodes.map(|n| partners.shift_of(n));
            let s = [0, 1, 2].map(|d| sh.iter().map(|x| x[d]).min().unwrap_or(0));
            if s == [0; 3] {
                continue;
            }
            let image = nodes.map(|n| partners.translate(n, s));
            let mut key = image;
            key.sort_unstable();
            if key != image {
                return Err(invalid(
                    "periodic boundaries",
                    "not supported yet: order 2 on a mesh whose translates reorder a face's nodes (use order 1)",
                ));
            }
            let m = *face_id
                .get(&key)
                .ok_or_else(|| invalid("periodic boundaries", "a face on a periodic face has no translate on the opposite face"))?;
            for j in 0..pf {
                master[dofs.edges * pe + f * pf + j] = Master { slot: dofs.edges * pe + m as usize * pf + j, sign: 1.0, shift: s };
            }
        }
    }
    Ok(master)
}

/// Each full unknown's slot among the kept ones (`None` when dropped), and a point per kept
/// unknown (its master's). With `self_tied_vanish` an unknown that is its own image a period
/// on (an edge on a rotation's axis, turned onto itself) is dropped: x = e^{−jψ} x with ψ ≠ 0.
fn reduction(dofs: &Dofs, master: &[Master], self_tied_vanish: bool) -> Result<(Vec<Option<Tied>>, Vec<Point3>)> {
    let total = master.len();
    // A master slot is dead when any slot tied to it is eliminated (a wall on either face).
    let mut dead = vec![false; total];
    for (g, m) in master.iter().enumerate() {
        if dofs.unknown_of(g).is_none() || (self_tied_vanish && m.slot == g && m.shift != [0; 3]) {
            dead[m.slot] = true;
        }
    }
    let mut reduced_of = vec![Slot::NONE; total];
    let mut points = Vec::new();
    let mut map = vec![None; dofs.unknowns];
    for (g, &Master { slot: m, sign, shift }) in master.iter().enumerate() {
        let Some(u) = dofs.unknown_of(g) else { continue };
        if dead[m] {
            continue;
        }
        let reduced = match reduced_of[m].get() {
            Some(r) => r,
            None => {
                let mu = dofs.unknown_of(m).ok_or_else(|| invalid("periodic boundaries", "a master unknown was eliminated"))?;
                let r = points.len();
                reduced_of[m] = Slot::new(r);
                points.push(dofs.points.get(mu).copied().unwrap_or(Point3::ORIGIN));
                r
            }
        };
        map[u] = Some(Tied { reduced, sign, shift });
    }
    Ok((map, points))
}

/// The kept pattern — every pair of kept unknowns some full entry couples — and, for each entry
/// of the full `pattern`, its entry there (none when dropped) with its phase's period difference
/// and sign product.
fn kept_pattern(pattern: &Csr, map: &[Option<Tied>], reduced: usize) -> Result<(Csr, Vec<(Slot, [i8; 3], f64)>)> {
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); reduced];
    for i in 0..pattern.n {
        let Some(si) = map[i] else { continue };
        for &j in &pattern.col[pattern.row(i)] {
            if let Some(sj) = map[j] {
                rows[si.reduced].push(sj.reduced);
            }
        }
    }
    let mut kept = Csr::builder(reduced);
    for (i, mut r) in rows.into_iter().enumerate() {
        r.push(i);
        r.sort_unstable();
        r.dedup();
        kept.row(r.into_iter().map(|j| (j, C::new(0.0, 0.0))));
    }
    let kept = kept.finish();
    let mut entries = Vec::with_capacity(pattern.col.len());
    for i in 0..pattern.n {
        for k in pattern.row(i) {
            let j = pattern.col[k];
            let e = match (map[i], map[j]) {
                (Some(si), Some(sj)) => {
                    let (r, c) = (si.reduced, sj.reduced);
                    let row = &kept.col[kept.row(r)];
                    let at = row.binary_search(&c).map_err(|_| invalid("periodic boundaries", "an entry outside the kept pattern"))?;
                    (Slot::new(kept.ptr[r] + at), [0, 1, 2].map(|d| sj.shift[d] - si.shift[d]), si.sign * sj.sign)
                }
                _ => (Slot::NONE, [0; 3], 0.0),
            };
            entries.push(e);
        }
    }
    Ok((kept, entries))
}

impl Tie {
    /// The reduction of `dofs` on `mesh` for the axes `periodic` ties; `pattern` is the full
    /// system's pattern.
    pub(super) fn new(mesh: &Mesh, topo: &Topology, dofs: &Dofs, periodic: &Periodic, pattern: &Csr) -> Result<Tie> {
        let axes = [periodic.x, periodic.y, periodic.z];
        let (partners, periods) = Partners::new(mesh, axes)?;
        let master = masters(topo, dofs, &partners)?;
        let (map, points) = reduction(dofs, &master, false)?;
        let reduced = points.len();
        let (pattern, entries) = kept_pattern(pattern, &map, reduced)?;
        let nodes = (0..mesh.nodes.len() as u32)
            .map(|n| {
                let s = partners.shift_of(n);
                (partners.translate(n, s), s)
            })
            .collect();
        Ok(Tie { map, reduced, points, periods, axes, pattern, entries, nodes })
    }

    /// The reduction of a sector's unknowns ([`Rotation`]): each on the second cut face tied to
    /// its image on the first a turn back, the one phase (slot 0 of the phases) ψ = 2πm/n; the
    /// unknowns on the axis that turn onto themselves dropped unless m = 0. Order 1 only (order
    /// 2's face functions mix under the turn's reordering of a face's nodes).
    pub(super) fn rotational(mesh: &Mesh, topo: &Topology, dofs: &Dofs, rot: &Rotation, pattern: &Csr) -> Result<Tie> {
        if dofs.order != Order::One {
            return Err(invalid("rotational symmetry", "not supported yet: order 2 on a sector (use order 1)"));
        }
        let partners = Partners::rotational(mesh, rot)?;
        let master = masters(topo, dofs, &partners)?;
        let (map, points) = reduction(dofs, &master, rot.m != 0)?;
        let reduced = points.len();
        let (pattern, entries) = kept_pattern(pattern, &map, reduced)?;
        let nodes = (0..mesh.nodes.len() as u32)
            .map(|n| {
                let s = partners.shift_of(n);
                (partners.translate(n, s), s)
            })
            .collect();
        Ok(Tie { map, reduced, points, periods: [rot.angle(), 0.0, 0.0], axes: [true, false, false], pattern, entries, nodes })
    }

    /// Node `n`'s master node and the periods from it.
    pub fn node_master(&self, n: usize) -> Option<(u32, [i8; 3])> {
        self.nodes.get(n).copied()
    }

    /// The kept unknown of full unknown `u` when `u` is its own master (no periods to it).
    pub fn master_of(&self, u: usize) -> Option<usize> {
        self.map.get(u).copied().flatten().filter(|s| s.shift == [0; 3] && s.sign > 0.0).map(|s| s.reduced)
    }

    /// The kept pattern (Tᴴ A T's).
    pub fn pattern(&self) -> &Csr {
        &self.pattern
    }

    /// The phase factor e^{−jψ·s}.
    pub fn phase(psi: [f64; 3], s: [i8; 3]) -> C {
        phase(psi, s)
    }

    /// The phases per period ψ_d (rad) of a wave whose transverse wavevector is (k_x, k_y), and
    /// `psi_z` along z.
    pub fn phases(&self, kx: f64, ky: f64, psi_z: f64) -> [f64; 3] {
        [kx * self.periods[0], ky * self.periods[1], if self.axes[2] { psi_z } else { 0.0 }]
    }

    /// The coefficient c of full unknown `u` (x_u = c x_r), `None` when dropped.
    pub fn coefficient(&self, u: usize, psi: [f64; 3]) -> Option<(usize, C)> {
        self.map.get(u).copied().flatten().map(|s| (s.reduced, phase(psi, s.shift) * s.sign))
    }

    /// Tᴴ A T for a matrix `a` on the full pattern (values `val`).
    pub fn reduce(&self, val: &[C], psi: [f64; 3]) -> Csr {
        let mut out = self.pattern.clone();
        for (&(e, s, sign), v) in self.entries.iter().zip(val) {
            if let Some(e) = e.get() {
                out.val[e] += phase(psi, s) * sign * *v;
            }
        }
        out
    }

    /// Tᴴ v for a test-side vector over the full unknowns.
    pub fn reduce_test(&self, v: &[(usize, C)], psi: [f64; 3]) -> Vec<(usize, C)> {
        Self::gather(v.iter().filter_map(|&(u, x)| self.coefficient(u, psi).map(|(r, c)| (r, c.conj() * x))))
    }

    /// Tᵀ v for a trial-side vector (a functional of x = T x_r).
    pub fn reduce_trial(&self, v: &[(usize, C)], psi: [f64; 3]) -> Vec<(usize, C)> {
        Self::gather(v.iter().filter_map(|&(u, x)| self.coefficient(u, psi).map(|(r, c)| (r, c * x))))
    }

    fn gather(items: impl Iterator<Item = (usize, C)>) -> Vec<(usize, C)> {
        let mut m: std::collections::BTreeMap<usize, C> = std::collections::BTreeMap::new();
        for (r, x) in items {
            *m.entry(r).or_default() += x;
        }
        m.into_iter().collect()
    }

    /// x = T x_r over the full unknowns.
    pub fn expand(&self, xr: &[C], psi: [f64; 3]) -> Vec<C> {
        (0..self.map.len()).map(|u| self.coefficient(u, psi).map_or(C::new(0.0, 0.0), |(r, c)| c * xr.get(r).copied().unwrap_or_default())).collect()
    }
}

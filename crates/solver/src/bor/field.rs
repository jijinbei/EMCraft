//! A body of revolution's field on its meridian half-plane for a wave into a port.

use super::ports::solved;
use super::*;

/// The field of a body of revolution on its meridian half-plane: (E_r, E_z, E_φ) at φ = 0 on an
/// `nx` × `ny` grid over the profile's box (r across, z up), none outside it.
#[derive(Clone, Debug, PartialEq)]
pub struct HalfPlaneField {
    pub r: (f64, f64),
    pub z: (f64, f64),
    pub nx: usize,
    pub ny: usize,
    pub e: Vec<Option<[C; 3]>>,
    /// The ports' first modes' S, as [`s_parameters`] gives them.
    pub point: BorSweepPoint,
}

/// The most grid points of a [`HalfPlaneField`] along a side.
pub const MAX_FIELD_SIDE: usize = 1024;

/// The field in order `m` at `frequency` for a unit wave of port `excitation`'s first mode in
/// (as [`s_parameters`] solves it), on a grid of `[nx, ny]` points: in each triangle, E_t from the edges'
/// Whitney functions and E_φ = j v / r from the nodes' (the mode's own amplitude scale).
pub fn field(p: &Profile, m: u32, ports: &[BorPort], frequency: f64, excitation: usize, [nx, ny]: [usize; 2]) -> Result<HalfPlaneField> {
    if !(2..=MAX_FIELD_SIDE).contains(&nx) || !(2..=MAX_FIELD_SIDE).contains(&ny) {
        return Err(invalid("grid", format!("2 to {MAX_FIELD_SIDE} points a side")));
    }
    let super::ports::Solved { point, dofs: d, solutions: xs } = solved(p, m, ports, frequency, None)?;
    let x = xs.get(excitation).ok_or_else(|| invalid("excitation", "no such port"))?;
    let (r0, r1, z0, z1) = p.points.iter().fold((f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY), |b, q| {
        (b.0.min(q[0]), b.1.max(q[0]), b.2.min(q[1]), b.3.max(q[1]))
    });
    let at = |i: usize, j: usize| [r0 + (r1 - r0) * i as f64 / (nx - 1) as f64, z0 + (z1 - z0) * j as f64 / (ny - 1) as f64];
    let mut e: Vec<Option<[C; 3]>> = crate::memory::filled(nx * ny, None, "a field picture's samples")?;
    for t in &p.triangles {
        let q = t.map(|v| p.points.get(v as usize).copied().unwrap_or_default());
        let det = (q[1][0] - q[0][0]) * (q[2][1] - q[0][1]) - (q[2][0] - q[0][0]) * (q[1][1] - q[0][1]);
        if det.abs() <= 0.0 {
            continue;
        }
        let g = [0, 1, 2].map(|i| {
            let (j, k) = ((i + 1) % 3, (i + 2) % 3);
            [(q[j][1] - q[k][1]) / det, (q[k][0] - q[j][0]) / det]
        });
        // The grid points in the triangle's box, then those inside it.
        let (lo, hi) = (
            [q[0][0].min(q[1][0]).min(q[2][0]), q[0][1].min(q[1][1]).min(q[2][1])],
            [q[0][0].max(q[1][0]).max(q[2][0]), q[0][1].max(q[1][1]).max(q[2][1])],
        );
        let index = |v: f64, a: f64, b: f64, n: usize| ((v - a) / (b - a).max(f64::MIN_POSITIVE) * (n - 1) as f64).clamp(0.0, (n - 1) as f64);
        let (i0, i1) = (index(lo[0], r0, r1, nx).ceil() as usize, index(hi[0], r0, r1, nx).floor() as usize);
        let (j0, j1) = (index(lo[1], z0, z1, ny).ceil() as usize, index(hi[1], z0, z1, ny).floor() as usize);
        for j in j0..=j1.min(ny - 1) {
            for i in i0..=i1.min(nx - 1) {
                let pt = at(i, j);
                let lam = [0, 1, 2].map(|k| {
                    let (a, b) = (q[(k + 1) % 3], q[(k + 2) % 3]);
                    ((b[0] - a[0]) * (pt[1] - a[1]) - (pt[0] - a[0]) * (b[1] - a[1])) / det
                });
                if lam.iter().any(|&l| l < -1e-12) {
                    continue;
                }
                let mut et = [C::new(0.0, 0.0); 2];
                let mut v = C::new(0.0, 0.0);
                for k in 0..3 {
                    let (a, b) = (t[k], t[(k + 1) % 3]);
                    if let Some(&u) = d.edge.get(&[a.min(b), a.max(b)]) {
                        let (ii, jj) = if a < b { (k, (k + 1) % 3) } else { ((k + 1) % 3, k) };
                        let c = x.get(u).copied().unwrap_or_default();
                        for dim in 0..2 {
                            et[dim] += c * (lam[ii] * g[jj][dim] - lam[jj] * g[ii][dim]);
                        }
                    }
                    if let Some(Some(u)) = d.node.get(t[k] as usize) {
                        v += x.get(*u).copied().unwrap_or_default() * lam[k];
                    }
                }
                let ephi = if pt[0] > 1e-12 * r1.max(1e-300) { C::new(0.0, 1.0) * v / pt[0] } else { C::new(0.0, 0.0) };
                e[j * nx + i] = Some([et[0], et[1], ephi]);
            }
        }
    }
    Ok(HalfPlaneField { r: (r0, r1), z: (z0, z1), nx, ny, e, point })
}

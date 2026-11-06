//! Bodies of revolution in the time domain: the fields of one azimuthal order m on the meridian
//! half-plane's (r, z) Yee grid (Y. Chen, R. Mittra, P. Harms, *Finite-difference time-domain
//! algorithm for solving Maxwell's equations in rotationally symmetric geometries*, IEEE Trans.
//! Microwave Theory Tech. 44(6):832–839, 1996; A. Taflove, S. C. Hagness, *Computational
//! Electrodynamics*, 3rd ed., Artech House 2005, ch. 12).
//!
//! **The fields.** With E_r, E_z, H_φ ∝ cos mφ and E_φ, H_r, H_z ∝ sin mφ the six components are
//! real and ∂_φ is ±m on them: ε∂ₜE_r = m H_z/r − ∂_z H_φ, ε∂ₜE_φ = ∂_z H_r − ∂_r H_z,
//! ε∂ₜE_z = (∂_r(r H_φ) − m H_r)/r, μ∂ₜH_r = m E_z/r + ∂_z E_φ, μ∂ₜH_φ = ∂_r E_z − ∂_z E_r,
//! μ∂ₜH_z = −(∂_r(r E_φ) + m E_r)/r. On the grid (cells Δr × Δz, the cell (i, j) spanning
//! [iΔr, (i+1)Δr] × [z₀ + jΔz, z₀ + (j+1)Δz]): E_r at (i+½, j), E_φ at (i, j), E_z at (i, j+½),
//! H_r at (i, j+½), H_φ at (i+½, j+½), H_z at (i+½, j).
//!
//! **The axis.** For m = 0, E_φ = H_r = 0 there and E_z follows Ampère's law round a disc of
//! radius Δr/2: ε∂ₜE_z = 4H_φ(½)/Δr. For m = 1, E_z = 0 there; E_φ takes ∂_r H_z from the field
//! at −Δr/2, which is the one at +Δr/2 turned by π, (−1)^m H_z(½), and H_r takes E_z/r → ∂_r E_z,
//! E_z(1)/Δr. For m ≥ 2 every field on the axis is zero.
//!
//! **Walls and materials.** Each cell holds the model's filling at its middle or a conductor;
//! an E component with a conductor cell beside it is zero (the walls staircased along the grid),
//! the others take the mean ε of the cells round them, an H component the mean μ.
//!
//! **Stability.** c Δt ≤ Δ/√2 for m = 0 and Δ/(m + 1) for m > 0 on square cells (Chen et al.)
//! away from the axis; the axis' disc (its 4/Δr) makes it stiffer there, and the grid ran away at
//! 0.98 of that bound and held at 0.75: Δt = 0.7 √2/(c (m + 1 or √2) √(1/Δr² + 1/Δz²)), c the
//! fastest speed in the grid.
//!
//! **Resonances.** A Gaussian-modulated pulse covering the band rings the cavity from a soft
//! source of each family's components off its symmetry lines; the ringing at three probes, after
//! the pulse, is Fourier-transformed with a Hann window on a grid four times finer than 1/T, and
//! each peak of the probes' summed power above a thousandth of the strongest is placed by a
//! parabola through its log-power and its neighbours'.

use std::f64::consts::PI;

use emcraft_model::units::{C0, EPS0, MU0};

use crate::{FdtdError, MAX_CELLS, MAX_STEPS, Result, invalid};

/// A body of revolution's meridian half-plane as cells: `nr` × `nz` of Δr × Δz from (0, z0), r
/// fastest, each the (εr, μr) filling it or a conductor.
#[derive(Clone, Debug, PartialEq)]
pub struct BorGrid {
    pub nr: usize,
    pub nz: usize,
    pub dr: f64,
    pub dz: f64,
    pub z0: f64,
    pub cells: Vec<Option<(f64, f64)>>,
}

/// The resonances a ring-down found (Hz, lowest first), the steps run and the step (s).
#[derive(Clone, Debug, PartialEq)]
pub struct Ringdown {
    pub frequencies: Vec<f64>,
    pub steps: usize,
    pub dt: f64,
}

/// The most periods of the band's middle a ring-down may run.
pub const MAX_PERIODS: f64 = 20_000.0;

impl BorGrid {
    fn cell(&self, i: isize, j: isize) -> Option<(f64, f64)> {
        if i < 0 || j < 0 || i as usize >= self.nr || j as usize >= self.nz {
            return None;
        }
        self.cells.get(j as usize * self.nr + i as usize).copied().flatten()
    }

    /// The mean ε of `around`, or none when one of them is a conductor.
    fn eps(&self, around: &[(isize, isize)]) -> Option<f64> {
        let mut sum = 0.0;
        for &(i, j) in around {
            sum += self.cell(i, j)?.0;
        }
        Some(sum / around.len() as f64)
    }

    /// The mean μ of `around` (a conductor counted as 1).
    fn mu(&self, around: &[(isize, isize)]) -> f64 {
        around.iter().map(|&(i, j)| self.cell(i, j).map_or(1.0, |c| c.1)).sum::<f64>() / around.len() as f64
    }
}

/// A field on a grid of `w` × `h` points, with its update coefficient per point (zero: held at
/// zero).
struct Field {
    w: usize,
    v: Vec<f64>,
    c: Vec<f64>,
}

impl Field {
    fn new(w: usize, h: usize) -> Field {
        Field { w, v: vec![0.0; w * h], c: vec![0.0; w * h] }
    }

    fn at(&self, i: usize, j: usize) -> f64 {
        self.v[j * self.w + i]
    }
}

/// Rings `grid`'s cavity in order `m` over the band [`start`, `stop`] (Hz) for `periods` periods of
/// its middle, and finds its resonances there.
///
/// ```
/// use emcraft_fdtd::bor::{BorGrid, ring};
///
/// // A pillbox of radius 10 mm and 15 mm long in 0.5 mm cells: TM010 at c x₀₁/(2π a).
/// let (nr, nz) = (20, 30);
/// let grid = BorGrid { nr, nz, dr: 5e-4, dz: 5e-4, z0: 0.0, cells: vec![Some((1.0, 1.0)); nr * nz] };
/// let r = ring(&grid, 0, (10e9, 12.5e9), 300.0)?;
/// let tm010 = 2.404825557695773 * 299_792_458.0 / (2.0 * std::f64::consts::PI * 0.01);
/// assert!(r.frequencies.iter().any(|f| (f / tm010 - 1.0).abs() < 2e-3));
/// assert!(ring(&grid, 0, (12e9, 10e9), 300.0).is_err());
/// # Ok::<(), emcraft_fdtd::FdtdError>(())
/// ```
pub fn ring(grid: &BorGrid, m: u32, (start, stop): (f64, f64), periods: f64) -> Result<Ringdown> {
    let (nr, nz) = (grid.nr, grid.nz);
    if nr < 2
        || nz < 2
        || grid.cells.len() != nr * nz
        || !(grid.dr > 0.0 && grid.dz > 0.0 && grid.dr.is_finite() && grid.dz.is_finite() && grid.z0.is_finite())
    {
        return Err(invalid("grid", "needs at least 2 × 2 cells of finite positive sides, one filling per cell"));
    }
    let cells = (nr + 1) * (nz + 1);
    if cells > MAX_CELLS {
        return Err(FdtdError::TooLarge { cells, limit: MAX_CELLS });
    }
    if m > 64 {
        return Err(invalid("order", "m from 0 to 64"));
    }
    if !(start > 0.0 && stop > start && stop.is_finite()) {
        return Err(invalid("band", "needs 0 < start < stop"));
    }
    if !(10.0..=MAX_PERIODS).contains(&periods) {
        return Err(invalid("periods", format!("10 to {MAX_PERIODS}")));
    }
    if grid.cells.iter().flatten().any(|&(e, u)| !(e.is_finite() && u.is_finite() && e > 0.0 && u > 0.0)) {
        return Err(invalid("grid", "every filling needs finite εr > 0 and μr > 0"));
    }
    let (dr, dz) = (grid.dr, grid.dz);
    let slowest = grid.cells.iter().flatten().map(|&(e, u)| e * u).fold(f64::INFINITY, f64::min).min(1.0);
    let speed = C0 / slowest.sqrt();
    let k = if m == 0 { 2f64.sqrt() } else { f64::from(m + 1) };
    let dt = 0.7 * 2f64.sqrt() / (k * speed * (1.0 / (dr * dr) + 1.0 / (dz * dz)).sqrt());
    let mid = 0.5 * (start + stop);
    let steps = (periods / (mid * dt)).ceil() as usize;
    if steps > MAX_STEPS {
        return Err(invalid("periods", format!("{steps} steps are more than {MAX_STEPS}: fewer periods or a larger cell")));
    }
    let mf = f64::from(m);
    let r = |i: f64| i * dr;
    let (ni, nj) = (nr as isize, nz as isize);
    // The fields and their coefficients Δt/ε or Δt/μ (zero where held).
    let mut er = Field::new(nr, nz + 1);
    let mut ep = Field::new(nr + 1, nz + 1);
    let mut ez = Field::new(nr + 1, nz);
    let mut hr = Field::new(nr + 1, nz);
    let mut hp = Field::new(nr, nz);
    let mut hz = Field::new(nr, nz + 1);
    for j in 0..=nj {
        for i in 0..ni {
            if let Some(e) = grid.eps(&[(i, j - 1), (i, j)]) {
                er.c[j as usize * nr + i as usize] = dt / (EPS0 * e);
            }
            hz.c[j as usize * nr + i as usize] = dt / (MU0 * grid.mu(&[(i, j - 1), (i, j)]));
        }
        for i in 0..=ni {
            // On the axis only the cells beside it (r ≥ 0).
            let around: Vec<(isize, isize)> = if i == 0 { vec![(0, j - 1), (0, j)] } else { vec![(i - 1, j - 1), (i, j - 1), (i - 1, j), (i, j)] };
            if let Some(e) = grid.eps(&around)
                && (i > 0 || m == 1)
            {
                ep.c[j as usize * (nr + 1) + i as usize] = dt / (EPS0 * e);
            }
        }
    }
    for j in 0..nj {
        for i in 0..=ni {
            let around: Vec<(isize, isize)> = if i == 0 { vec![(0, j)] } else { vec![(i - 1, j), (i, j)] };
            if let Some(e) = grid.eps(&around)
                && (i > 0 || m == 0)
            {
                ez.c[j as usize * (nr + 1) + i as usize] = dt / (EPS0 * e);
            }
            if i > 0 || m == 1 {
                hr.c[j as usize * (nr + 1) + i as usize] = dt / (MU0 * grid.mu(&around));
            }
        }
        for i in 0..ni {
            hp.c[j as usize * nr + i as usize] = dt / (MU0 * grid.mu(&[(i, j)]));
        }
    }
    // The source and probes: points off the grid's symmetry lines, at the nearest live components.
    let near = |f: &Field, h: usize, fr: f64, fz: f64| -> Option<usize> {
        let (ti, tj) = ((fr * f.w as f64) as usize, (fz * h as f64) as usize);
        (0..f.v.len()).filter(|&k| f.c[k] != 0.0).min_by_key(|&k| {
            let (i, j) = (k % f.w, k / f.w);
            i.abs_diff(ti).pow(2) + j.abs_diff(tj).pow(2)
        })
    };
    let spots = [(0.37, 0.29), (0.61, 0.73), (0.83, 0.47)];
    let source: Vec<(usize, usize)> = [(0usize, &er, nz + 1), (1, &ep, nz + 1), (2, &ez, nz)]
        .iter()
        .filter_map(|&(which, f, h)| near(f, h, spots[0].0, spots[0].1).map(|k| (which, k)))
        .collect();
    let probes: Vec<(usize, usize)> = spots
        .iter()
        .flat_map(|&(a, b)| [(0usize, near(&er, nz + 1, a, b)), (1, near(&ep, nz + 1, a, b)), (2, near(&ez, nz, a, b))])
        .filter_map(|(w, k)| k.map(|k| (w, k)))
        .collect();
    if source.is_empty() || probes.is_empty() {
        return Err(invalid("grid", "holds no field of this order: the body is all conductor"));
    }
    let band = stop - start;
    let tau = 4.0 / (PI * band);
    let t0 = 4.0 * tau;
    let pulse = |t: f64| (-((t - t0) / tau).powi(2)).exp() * (2.0 * PI * mid * (t - t0)).sin();
    let quiet = ((t0 + 4.0 * tau) / dt).ceil() as usize;
    let mut record: Vec<Vec<f64>> = vec![Vec::with_capacity(steps.saturating_sub(quiet)); probes.len()];
    for n in 0..steps {
        // H from n − ½ to n + ½.
        for j in 0..nz {
            for i in 0..nr {
                let k = j * nr + i;
                // μ∂ₜH_φ = −(∂_z E_r − ∂_r E_z).
                let curl = (er.at(i, j + 1) - er.at(i, j)) / dz - (ez.at(i + 1, j) - ez.at(i, j)) / dr;
                hp.v[k] -= hp.c[k] * curl;
            }
            for i in 0..=nr {
                let k = j * (nr + 1) + i;
                if hr.c[k] == 0.0 {
                    continue;
                }
                let ez_over_r = if i == 0 { ez.at(1, j) / dr } else { ez.at(i, j) / r(i as f64) };
                let curl = mf * ez_over_r + (ep.at(i, j + 1) - ep.at(i, j)) / dz;
                hr.v[k] += hr.c[k] * curl;
            }
        }
        for j in 0..=nz {
            for i in 0..nr {
                let k = j * nr + i;
                let rh = r(i as f64 + 0.5);
                let curl = (r((i + 1) as f64) * ep.at(i + 1, j) - r(i as f64) * ep.at(i, j)) / (rh * dr) + mf * er.at(i, j) / rh;
                hz.v[k] -= hz.c[k] * curl;
            }
        }
        // E from n to n + 1.
        for j in 0..=nz {
            for i in 0..nr {
                let k = j * nr + i;
                if er.c[k] == 0.0 {
                    continue;
                }
                let rh = r(i as f64 + 0.5);
                let below = if j > 0 { hp.at(i, j - 1) } else { 0.0 };
                let above = if j < nz { hp.at(i, j) } else { 0.0 };
                er.v[k] += er.c[k] * (mf * hz.at(i, j) / rh - (above - below) / dz);
            }
            for i in 0..=nr {
                let k = j * (nr + 1) + i;
                if ep.c[k] == 0.0 {
                    continue;
                }
                let hr_below = if j > 0 { hr.at(i, j - 1) } else { 0.0 };
                let hr_above = if j < nz { hr.at(i, j) } else { 0.0 };
                let dhz = if i == 0 {
                    // H_z at −Δr/2 is (−1)^m that at +Δr/2.
                    (1.0 - if m.is_multiple_of(2) { 1.0 } else { -1.0 }) * hz.at(0, j) / dr
                } else {
                    let right = if i < nr { hz.at(i, j) } else { 0.0 };
                    (right - hz.at(i - 1, j)) / dr
                };
                ep.v[k] += ep.c[k] * ((hr_above - hr_below) / dz - dhz);
            }
        }
        for j in 0..nz {
            for i in 0..=nr {
                let k = j * (nr + 1) + i;
                if ez.c[k] == 0.0 {
                    continue;
                }
                let curl = if i == 0 {
                    4.0 * hp.at(0, j) / dr
                } else {
                    let ri = r(i as f64);
                    let right = if i < nr { r(i as f64 + 0.5) * hp.at(i, j) } else { 0.0 };
                    let left = r(i as f64 - 0.5) * hp.at(i - 1, j);
                    (right - left) / (ri * dr) - mf * hr.at(i, j) / ri
                };
                ez.v[k] += ez.c[k] * curl;
            }
        }
        let s = pulse((n as f64 + 1.0) * dt);
        for &(w, k) in &source {
            match w {
                0 => er.v[k] += s,
                1 => ep.v[k] += s,
                _ => ez.v[k] += s,
            }
        }
        if n >= quiet {
            for (rec, &(w, k)) in record.iter_mut().zip(&probes) {
                rec.push(match w {
                    0 => er.v[k],
                    1 => ep.v[k],
                    _ => ez.v[k],
                });
            }
        }
    }
    let frequencies = peaks(&record, dt, (start, stop));
    Ok(Ringdown { frequencies, steps, dt })
}

/// The peaks in [`start`, `stop`] of the records' summed power spectrum (Hann-windowed, on a grid
/// four times finer than 1/T), each placed by a parabola through its log-power and its
/// neighbours', those above a thousandth of the strongest.
fn peaks(records: &[Vec<f64>], dt: f64, (start, stop): (f64, f64)) -> Vec<f64> {
    let n = records.first().map_or(0, Vec::len);
    if n < 16 {
        return Vec::new();
    }
    let span = n as f64 * dt;
    let df = 1.0 / (4.0 * span);
    let count = (((stop - start) / df).ceil() as usize + 1).min(200_000);
    let window: Vec<f64> = (0..n).map(|k| 0.5 - 0.5 * (2.0 * PI * k as f64 / (n - 1) as f64).cos()).collect();
    let power: Vec<f64> = (0..count)
        .map(|q| {
            let f = start + q as f64 * df;
            let (c, s) = ((2.0 * PI * f * dt).cos(), (2.0 * PI * f * dt).sin());
            records
                .iter()
                .map(|rec| {
                    // The phasor e^{−j2πf k Δt} by rotation, renormalised now and then.
                    let (mut re, mut im) = (0.0, 0.0);
                    let (mut wr, mut wi) = (1.0f64, 0.0f64);
                    for (k, (&x, &w)) in rec.iter().zip(&window).enumerate() {
                        re += w * x * wr;
                        im -= w * x * wi;
                        let (a, b) = (wr * c - wi * s, wr * s + wi * c);
                        (wr, wi) = (a, b);
                        if k % 1024 == 1023 {
                            let norm = wr.hypot(wi);
                            wr /= norm;
                            wi /= norm;
                        }
                    }
                    re * re + im * im
                })
                .sum()
        })
        .collect();
    let strongest = power.iter().copied().fold(0.0, f64::max);
    let mut out = Vec::new();
    for q in 1..count.saturating_sub(1) {
        let (a, b, c) = (power[q - 1], power[q], power[q + 1]);
        if b > a && b >= c && b > 1e-3 * strongest && a > 0.0 && c > 0.0 {
            let (la, lb, lc) = (a.ln(), b.ln(), c.ln());
            let den = la - 2.0 * lb + lc;
            let shift = if den < 0.0 { 0.5 * (la - lc) / den } else { 0.0 };
            out.push(start + (q as f64 + shift.clamp(-0.5, 0.5)) * df);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pillbox(cell: f64) -> BorGrid {
        let (nr, nz) = ((0.01 / cell).round() as usize, (0.015 / cell).round() as usize);
        BorGrid { nr, nz, dr: 0.01 / nr as f64, dz: 0.015 / nz as f64, z0: 0.0, cells: vec![Some((1.0, 1.0)); nr * nz] }
    }

    /// The pillbox's f = c/(2π) √((x/a)² + (pπ/d)²).
    fn exact(x: f64, p: f64) -> f64 {
        C0 / (2.0 * PI) * ((x / 0.01f64).powi(2) + (p * PI / 0.015).powi(2)).sqrt()
    }

    /// Each wanted frequency within `tol` of a peak found.
    fn found(r: &Ringdown, want: &[f64], tol: f64) {
        for w in want {
            let best = r.frequencies.iter().map(|f| (f / w - 1.0).abs()).fold(f64::INFINITY, f64::min);
            assert!(best < tol, "{w} Hz: nearest peak {best:.2e} off ({:?})", r.frequencies);
        }
    }

    /// **Setup**: a vacuum pillbox, a = 10 mm, d = 15 mm, perfectly conducting, in 0.25 mm cells.
    /// **Closed form**: TM010 and TM011 (x₀₁ = 2.404826, m = 0), TE111 (x′₁₁ = 1.841184) and TM110
    /// (x₁₁ = 3.831706, m = 1) (Pozar, *Microwave Engineering*, §6.3; Abramowitz–Stegun 9.5).
    #[test]
    #[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
    fn a_pillbox_rings_at_its_closed_form_resonances() {
        let g = pillbox(2.5e-4);
        found(&ring(&g, 0, (10e9, 16e9), 400.0).unwrap(), &[exact(2.404_825_557_695_773, 0.0), exact(2.404_825_557_695_773, 1.0)], 1e-3);
        found(&ring(&g, 1, (12e9, 19e9), 400.0).unwrap(), &[exact(1.841_183_781_340_659, 1.0), exact(3.831_705_970_207_512, 0.0)], 1e-3);
    }
}

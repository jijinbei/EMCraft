//! A plane wave on the open box by the total-field/scattered-field split (A. Taflove, S. C.
//! Hagness, *Computational Electrodynamics*, 3rd ed., Artech House 2005, ch. 5): inside a box
//! round the bodies the grid holds the total field, outside it the scattered field alone, and on
//! the box's faces the curls that reach across are corrected by the incident field there. The
//! wave travels along a grid axis, so its incident field comes from a one-dimensional Yee grid
//! along that axis with the same cells, time step and CPML: its numerical dispersion is the
//! three-dimensional grid's along the axis exactly, and the incident wave does not leak into the
//! scattered-field region. The Huygens box lies outside the split, in the scattered field, so
//! the far field and the power it gives are the scattered wave's, per unit of incident field.

use emcraft_model::units::{EPS0, MU0};
use emcraft_model::{Aabb, Model, Source};

use crate::open::{BoxCoefficients, BoxGrid};
use crate::yee::{Cpml, Fields, index, lattices};
use crate::{FdtdError, Result, invalid};

/// A correction on the split's faces: field `id` of component `comp` gains `factor` times the
/// one-dimensional grid's value at `k`.
struct Correction {
    comp: usize,
    id: usize,
    factor: f64,
    k: usize,
}

/// The plane wave: its one-dimensional grid along the axis of travel and the corrections on the
/// split's faces.
pub(crate) struct PlaneWave {
    /// E along the axis of travel (at its nodes) and H (at its half cells), with the CPML's
    /// auxiliary variables.
    e1: Vec<f64>,
    h1: Vec<f64>,
    pe: Vec<f64>,
    ph: Vec<f64>,
    cpml_e: Vec<Cpml>,
    cpml_h: Vec<Cpml>,
    h: f64,
    dt: f64,
    /// The node of the soft source, and where the incident field is read.
    source: usize,
    probe: usize,
    on_e: Vec<Correction>,
    on_h: Vec<Correction>,
}

/// The plane wave driving `model`, as (axis of travel, travelling up the axis, E's axis, E's
/// sign); `None` without one.
pub(crate) fn of(model: &Model) -> Result<Option<(usize, bool, usize, f64)>> {
    let waves: Vec<_> = model
        .sources
        .iter()
        .filter_map(|s| if let Source::PlaneWave { direction, polarization } = s { Some((*direction, *polarization)) } else { None })
        .collect();
    let [(k, e)] = waves[..] else {
        return if waves.is_empty() { Ok(None) } else { Err(FdtdError::Unsupported("more than one plane wave in the time domain".into())) };
    };
    if model.sources.len() > 1 || !model.lumped.is_empty() {
        return Err(FdtdError::Unsupported("a plane wave with other sources or lumped ports in the time domain (a plane wave alone)".into()));
    }
    let along = |v: emcraft_model::Vec3| {
        let a = v.dominant_axis();
        let n = v.norm();
        ((0..3).all(|q| q == a || v[q].abs() <= 1e-9 * n)).then_some((a, v[a].signum()))
    };
    let (Some((a, up)), Some((p, sign))) = (along(k), along(e)) else {
        return Err(FdtdError::Unsupported(
            "a plane wave off the grid's axes in the time domain (its direction and polarization along x, y or z)".into(),
        ));
    };
    if a == p {
        return Err(invalid("plane wave", "its polarization must lie across its direction"));
    }
    Ok(Some((a, up > 0.0, p, sign)))
}

impl PlaneWave {
    /// The wave travelling along `axis` (up it when `up`) with E along `pol` of sign `e_sign`, on
    /// the uniform `grid` with the CPML `cpml` (E's, H's) along the axis, its split on the box of
    /// nodes `lo`..`hi`.
    pub(crate) fn new(
        (axis, up, pol, e_sign): (usize, bool, usize, f64),
        grid: &BoxGrid,
        cpml: (&[Cpml], &[Cpml]),
        (lo, hi): ([usize; 3], [usize; 3]),
        coef: &BoxCoefficients,
    ) -> Result<PlaneWave> {
        let BoxGrid { n, hs, npml, .. } = *grid;
        let dt = coef.dt;
        let na = n[axis];
        let source = if up { npml + 2 } else { na - npml - 2 };
        if (up && source + 1 >= lo[axis]) || (!up && source <= hi[axis] + 1) {
            return Err(invalid("plane wave", "no room between the layer and the bodies for its source: enlarge the box"));
        }
        // H's axis and sign: â × ê along the third axis.
        let q = 3 - axis - pol;
        let h_sign = e_sign * if pol == (axis + 1) % 3 { 1.0 } else { -1.0 };
        let [e_dims, h_dims] = lattices(n);
        let mut on_e = Vec::new();
        let mut on_h = Vec::new();
        for face in 0..3 {
            for (side, bound) in [(-1.0, lo[face]), (1.0, hi[face])] {
                // E on the face, tangential: its curl's H across the face lies outside.
                for comp in (0..3).filter(|&c| c != face) {
                    let (u, v) = ((comp + 1) % 3, (comp + 2) % 3);
                    // ∂H_v/∂u (face ⟂ u) or −∂H_u/∂v (face ⟂ v); only H_q is incident.
                    let (hc, sign) = if face == u { (v, side) } else { (u, -side) };
                    if hc != q {
                        continue;
                    }
                    let w = 3 - comp - face;
                    for t in lo[comp]..hi[comp] {
                        for r in lo[w]..=hi[w] {
                            let mut idx = [0usize; 3];
                            idx[face] = bound;
                            idx[comp] = t;
                            idx[w] = r;
                            let id = index(e_dims[comp], idx[0], idx[1], idx[2]);
                            let outside = if side < 0.0 { bound - 1 } else { bound };
                            let k = if axis == face { outside } else { idx[axis] };
                            let c = coef.ce[comp].get(id).copied().unwrap_or(0.0);
                            on_e.push(Correction { comp, id, factor: c * sign * h_sign / hs[face], k });
                        }
                    }
                }
                // H just outside the face, tangential: its curl's E on the face is total.
                for comp in (0..3).filter(|&c| c != face) {
                    let (u, v) = ((comp + 1) % 3, (comp + 2) % 3);
                    // ∂E_v/∂u (face ⟂ u) or −∂E_u/∂v (face ⟂ v); only E_pol is incident.
                    let (ec, sign) = if face == u { (v, -side) } else { (u, side) };
                    if ec != pol {
                        continue;
                    }
                    let w = 3 - comp - face;
                    let outside = if side < 0.0 { bound - 1 } else { bound };
                    for t in lo[comp]..=hi[comp] {
                        for r in lo[w]..hi[w] {
                            let mut idx = [0usize; 3];
                            idx[face] = outside;
                            idx[comp] = t;
                            idx[w] = r;
                            let id = index(h_dims[comp], idx[0], idx[1], idx[2]);
                            let k = if axis == face { bound } else { idx[axis] };
                            let c = coef.ch[comp].get(id).copied().unwrap_or(0.0);
                            on_h.push(Correction { comp, id, factor: c * sign * e_sign / hs[face], k });
                        }
                    }
                }
            }
        }
        // The split's faces in vacuum: the corrections assume its plain, lossless update.
        let (ve, vh) = (dt / EPS0, dt / MU0);
        let off = |x: f64, v: f64| (x / v - 1.0).abs() > 1e-9;
        if on_e.iter().any(|c| off(coef.ce[c.comp][c.id], ve) || coef.dispersion[c.comp].has(c.id))
            || on_h.iter().any(|c| off(coef.ch[c.comp][c.id], vh))
        {
            return Err(invalid(
                "plane wave",
                "the total-field box round the bodies must lie in vacuum: enlarge the box or keep the bodies inside it",
            ));
        }
        let probe = (lo[axis] + hi[axis]) / 2;
        Ok(PlaneWave {
            e1: vec![0.0; na + 1],
            h1: vec![0.0; na],
            pe: vec![0.0; na + 1],
            ph: vec![0.0; na],
            cpml_e: cpml.0.to_vec(),
            cpml_h: cpml.1.to_vec(),
            h: hs[axis],
            dt,
            source,
            probe,
            on_e,
            on_h,
        })
    }

    /// The one-dimensional H from n − ½ to n + ½, then the corrections of E on the split's faces
    /// (E just updated to n + 1 with H at n + ½).
    pub(crate) fn after_e(&mut self, f: &Fields) {
        let (h, dt) = (self.h, self.dt);
        for i in 0..self.h1.len() {
            let d = (self.e1[i + 1] - self.e1[i]) / h;
            self.ph[i] = self.cpml_h[i].psi(self.ph[i], d);
            self.h1[i] -= dt / MU0 * (d + self.ph[i]);
        }
        for c in &self.on_e {
            let e = &f.e[c.comp];
            e.set(c.id, e.get(c.id) + c.factor * self.h1.get(c.k).copied().unwrap_or(0.0));
        }
    }

    /// The one-dimensional E from n to n + 1 with the source `s` added; returns the incident E
    /// at the probe (the centre of the split), the spectrum the results are per unit of.
    pub(crate) fn step_e(&mut self, s: f64) -> f64 {
        let (h, dt) = (self.h, self.dt);
        let n = self.e1.len() - 1;
        for i in 1..n {
            let d = (self.h1[i] - self.h1[i - 1]) / h;
            self.pe[i] = self.cpml_e[i].psi(self.pe[i], d);
            self.e1[i] -= dt / EPS0 * (d + self.pe[i]);
        }
        if let Some(e) = self.e1.get_mut(self.source) {
            *e += s;
        }
        self.e1.get(self.probe).copied().unwrap_or(0.0)
    }

    /// The corrections of H outside the split's faces for the next step, with E at n + 1: H at
    /// n + ½ gains them before it is updated (the update is linear and these faces lossless).
    pub(crate) fn correct_h(&self, f: &Fields) {
        for c in &self.on_h {
            let x = &f.h[c.comp];
            x.set(c.id, x.get(c.id) + c.factor * self.e1.get(c.k).copied().unwrap_or(0.0));
        }
    }
}

/// The split's box of nodes round `bodies` (m) on `grid`'s cells: a cell clear of them on every
/// side, inside its absorbing layer with room for the Huygens box.
pub(crate) fn split_box(bodies: Aabb, grid: &BoxGrid) -> Result<([usize; 3], [usize; 3])> {
    let BoxGrid { n, hs, npml, .. } = *grid;
    let mut lo = [0usize; 3];
    let mut hi = [0usize; 3];
    for d in 0..3 {
        let a = (bodies.min[d] / hs[d]).floor() - 1.0;
        let b = (bodies.max[d] / hs[d]).ceil() + 1.0;
        if !(a >= (npml + 4) as f64 && b <= (n[d] - npml - 4) as f64) {
            return Err(invalid("plane wave", "the bodies reach too near the layer for the total-field box and the Huygens box: enlarge the box"));
        }
        lo[d] = a as usize;
        hi[d] = b as usize;
    }
    Ok((lo, hi))
}

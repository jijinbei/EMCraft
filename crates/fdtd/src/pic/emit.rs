//! Particles emitted from a conducting wall: a given current, or as many as the field at the wall
//! draws (space-charge limited).
//!
//! **Space-charge-limited emission by Gauss's law** (C. K. Birdsall, A. B. Langdon, *Plasma
//! Physics via Computer Simulation*, 1991, §16-5): each step a node of the cathode emits the
//! charge that, once past the first cell, brings the normal field just off the wall to zero. A
//! charge Q (C) leaving the wall through a node's area A changes E_n there by −σQ/(ε₀A) once it
//! is a cell away (σ = +1 on the low wall, −1 on the high one, the current's running sum through
//! the first edge); particles still in the first cell, a fraction s of it out, have done s of
//! that. So the node emits Q = σε₀A E_n − Σ q w (1 − s) S, S their weight on the node across, when
//! Q has the particles' sign: none when the field pushes them back.
//!
//! The emitted particles start on the wall at the step's time, with the momentum of the energy
//! asked for along the wall's normal, spread across each node's area by the R₂ sequence (M.
//! Roberts, *The unreasonable effectiveness of quasirandom sequences*, 2018: the golden ratio's
//! two-dimensional analogue), so that the same run gives the same particles.

use emcraft_model::units::EPS0;

use super::grid::{Grid, Wall};
use super::{Particle, Population};
use crate::{Result, invalid};

/// How much a wall emits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Emission {
    /// As much as the field at the wall draws: the normal field brought to zero.
    Limited,
    /// A current density (A/m², its size).
    Current(f64),
}

/// A conducting wall emitting a population's particles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Emitter {
    /// The wall: normal to `axis`, at its high end or its low one.
    pub axis: usize,
    pub high: bool,
    /// The population emitted (an index of [`super::Pic::populations`]).
    pub population: usize,
    pub emission: Emission,
    /// The particles' kinetic energy as they leave (J).
    pub energy: f64,
    /// Macro-particles from each node a step.
    pub per_node: usize,
}

/// The most macro-particles a node emits a step.
pub const MAX_PER_NODE: usize = 64;

/// The R₂ sequence's steps.
const R2: [f64; 2] = [0.754_877_666_246_692_7, 0.569_840_290_998_053_2];

impl Emitter {
    /// The emitter checked against the grid and the populations.
    pub(crate) fn check(&self, grid: &Grid, populations: usize) -> Result<()> {
        if self.axis > 2 || grid.axes[self.axis].wall != Wall::Conductor {
            return Err(invalid("emitter", "emits from a conducting wall: its axis must be one closed by walls"));
        }
        if self.population >= populations {
            return Err(invalid("emitter", format!("population {} of {populations}", self.population)));
        }
        if !(self.energy.is_finite() && self.energy >= 0.0) {
            return Err(invalid("emitter", "the energy must be a finite number ≥ 0"));
        }
        if self.per_node == 0 || self.per_node > MAX_PER_NODE {
            return Err(invalid("emitter", format!("1 to {MAX_PER_NODE} particles from a node a step")));
        }
        if let Emission::Current(j) = self.emission
            && !(j.is_finite() && j >= 0.0)
        {
            return Err(invalid("emitter", "the current density must be a finite number ≥ 0"));
        }
        Ok(())
    }

    /// The axes across the wall.
    fn across(&self) -> [usize; 2] {
        [(self.axis + 1) % 3, (self.axis + 2) % 3]
    }

    /// The wall's nodes that emit (not those on the walls across it), with the index of their
    /// normal field on E's lattice.
    fn nodes(&self, grid: &Grid) -> Vec<([usize; 2], usize)> {
        let [b, c] = self.across();
        let (ab, ac) = (grid.axes[b], grid.axes[c]);
        let lattice = grid.e(self.axis);
        let edge = if self.high { grid.axes[self.axis].cells - 1 } else { 0 };
        (0..ac.points(true))
            .flat_map(|pc| (0..ab.points(true)).map(move |pb| [pb, pc]))
            .filter(|&[pb, pc]| !ab.on_wall(pb) && !ac.on_wall(pc))
            .map(|[pb, pc]| {
                let mut p = [0; 3];
                (p[self.axis], p[b], p[c]) = (edge, pb, pc);
                ([pb, pc], lattice.at(p))
            })
            .collect()
    }

    /// The charge (C) of `population`'s particles still in the first cell off the wall, by
    /// what they have yet to do to each node's field: Σ q w (1 − s) S.
    fn pending(&self, grid: &Grid, population: &Population) -> Vec<f64> {
        let [b, c] = self.across();
        let nodes = [grid.axes[b].points(true), grid.axes[c].points(true)];
        let mut pending = vec![0.0; nodes[0] * nodes[1]];
        let axis = grid.axes[self.axis];
        for p in &population.particles {
            let out = if self.high { (axis.length() - p.x[self.axis]) / axis.h } else { p.x[self.axis] / axis.h };
            if !(0.0..1.0).contains(&out) {
                continue;
            }
            let q = population.species.charge * p.w * (1.0 - out);
            let (sb, sc) = (p.x[b] / grid.axes[b].h, p.x[c] / grid.axes[c].h);
            let (fb, fc) = (sb.floor(), sc.floor());
            for (ob, wb) in [(0, 1.0 - (sb - fb)), (1, sb - fb)] {
                for (oc, wc) in [(0, 1.0 - (sc - fc)), (1, sc - fc)] {
                    let (Some(pb), Some(pc)) = (grid.axes[b].wrap(fb as isize + ob, true), grid.axes[c].wrap(fc as isize + oc, true)) else {
                        continue;
                    };
                    if let Some(v) = pending.get_mut(pb + nodes[0] * pc) {
                        *v += q * wb * wc;
                    }
                }
            }
        }
        pending
    }

    /// The particles emitted this step, given the normal field `e` (V/m, E's lattice along the
    /// axis), `dt` (s) and the count of particles emitted so far (the R₂ sequence's place).
    pub(crate) fn emit(&self, grid: &Grid, e: &[f64], population: &Population, dt: f64, count: &mut u64) -> Vec<Particle> {
        let [b, c] = self.across();
        let area = grid.axes[b].h * grid.axes[c].h;
        let q = population.species.charge;
        let sigma = if self.high { -1.0 } else { 1.0 };
        let pending = match self.emission {
            Emission::Limited => self.pending(grid, population),
            Emission::Current(_) => Vec::new(),
        };
        let row = grid.axes[b].points(true);
        let inward = std::array::from_fn(|d| if d == self.axis { sigma } else { 0.0 });
        let u = population.species.momentum(self.energy, inward);
        let wall = if self.high { grid.axes[self.axis].length() } else { 0.0 };
        let mut out = Vec::new();
        for ([pb, pc], id) in self.nodes(grid) {
            let charge = match self.emission {
                Emission::Limited => sigma * EPS0 * area * e.get(id).copied().unwrap_or(0.0) - pending.get(pb + row * pc).copied().unwrap_or(0.0),
                Emission::Current(j) => q.signum() * j * area * dt,
            };
            if !(charge * q > 0.0 && charge.is_finite()) {
                continue;
            }
            let w = charge / (q * self.per_node as f64);
            for _ in 0..self.per_node {
                *count += 1;
                let n = *count as f64;
                let off = R2.map(|r| (0.5 + n * r).fract() - 0.5);
                let mut x = [0.0; 3];
                x[self.axis] = wall;
                for (d, (p, o)) in [(b, (pb, off[0])), (c, (pc, off[1]))] {
                    let a = grid.axes[d];
                    let at = (p as f64 + o) * a.h;
                    x[d] = if a.wall == Wall::Periodic { at.rem_euclid(a.length()) } else { at.clamp(0.0, a.length()) };
                }
                out.push(Particle { x, u, w });
            }
        }
        out
    }
}

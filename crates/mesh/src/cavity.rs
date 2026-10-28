//! The air a part's metal encloses, as the region the fields fill: CAD users draw the metal of a
//! waveguide component (a block with a channel milled through it, irises left standing), not the
//! air inside it.
//!
//! The rule: the region is the bodies' bounding box less the bodies, split into its connected
//! pieces, and a piece is kept when the metal encloses it across x and y — it reaches none of the
//! box's four side faces (x or y lowest or highest). A piece may reach the box's lowest and
//! highest z faces: a channel open at both ends, where the ports go. A piece that reaches a side
//! face is air around the part, there only because the part is not a box (a flange's corners, a
//! step in its outline), and is dropped. Every kept piece stays ([`Keep::Enclosed`]), or the
//! largest alone ([`Keep::Largest`]: a part with blind screw holes beside its channel, say). A
//! metal body floating in a kept piece (a post standing free) is a hole in it.
//!
//! The box is the bodies' extremes exactly, so the part's outer faces lie in its faces: each
//! body's faces flush with the box's or with an earlier body's are moved out of the air first
//! (`crate::flush`), and the points the Boolean leaves inside the straight creases collapsed
//! (`crate::crease`), so the result is the air's own faces, the box's only where the air reaches
//! it.

use emcraft_model::{Combine, Point3, Vec3};

use crate::csg::{Op, Surface, combine_simplified};
use crate::flush::{Gaps, moved};
use crate::{MeshError, Result};

/// Which of the enclosed pieces of air are kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Keep {
    /// Every piece of air the metal encloses across x and y.
    #[default]
    Enclosed,
    /// The largest of them.
    Largest,
}

/// The enclosed air: its closed surface (outward from the air; the box's faces tagged 0..6 —
/// x low, x high, y low, y high, z low, z high — the bodies' after them, each body's tags
/// following the last's), the volume of each piece kept and how many pieces were dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct Cavity {
    pub surface: Surface,
    pub kept: Vec<f64>,
    pub dropped: usize,
}

fn fail(why: impl Into<String>) -> MeshError {
    MeshError::Invalid { what: "cavity".into(), why: why.into() }
}

const NO_AIR: &str = "the bodies enclose no air: every piece of air between them reaches a side (x or y) of their bounding box — is the part closed around its channel?";

/// The most bodies a cavity is cut from.
pub const MAX_BODIES: usize = 1000;

/// The box [lo, hi]: 12 outward triangles, tags x low, x high, y low, y high, z low, z high.
fn bounding_box(lo: Point3, hi: Point3) -> Surface {
    let positions = (0..8).map(|i| Point3::new([lo.x, hi.x][i & 1], [lo.y, hi.y][(i >> 1) & 1], [lo.z, hi.z][(i >> 2) & 1])).collect();
    let quads = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
    let triangles = quads.iter().flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect();
    Surface { positions, triangles, tags: (0..12).map(|i| i / 2).collect() }
}

/// The air the `bodies` (name, closed outward surface) enclose: see the module notes.
pub fn cavity(bodies: &[(String, Surface)], keep: Keep) -> Result<Cavity> {
    if bodies.is_empty() {
        return Err(fail("no metal body to take the air from"));
    }
    if bodies.len() > MAX_BODIES {
        return Err(fail(format!("{} bodies; the limit is {MAX_BODIES}", bodies.len())));
    }
    let all = bodies.iter().flat_map(|(_, s)| s.positions.iter());
    let (lo, hi) = all
        .fold(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]), |(lo, hi), p| ([0, 1, 2].map(|d| lo[d].min(p[d])), [0, 1, 2].map(|d| hi[d].max(p[d]))));
    let extent = (0..3).map(|d| hi[d] - lo[d]).fold(f64::INFINITY, f64::min);
    if !(extent > 0.0 && extent.is_finite()) {
        return Err(fail("the bodies span no volume"));
    }
    let scale = (0..3).map(|d| hi[d] - lo[d]).fold(0.0f64, f64::max);
    // Moved faces end up outside the air whatever the step; a small one cannot reach through a
    // thin wall into air beyond it. Faces within a millionth of the part's size are flush: well
    // above the bodies' moves below.
    let gaps = Gaps { step: 1e-4 * scale, flush: 1e-6 * scale, near: 0.0, mm: 1e3 };
    let mut air = bounding_box(Point3::from(lo), Point3::from(hi));
    for (k, (name, body)) in bodies.iter().enumerate() {
        let positions = moved(name, &body.positions, &body.triangles, &air.positions, &air.triangles, Combine::Subtract, gaps)?;
        // General position: each body moved by its own fraction of a ten-millionth of the
        // part's size (the fractional parts of multiples of irrational numbers), so that no
        // corner of one lies exactly on an edge or a diagonal of another — far below any mesh,
        // far above rounding.
        let jitter = Vec3::new(0.618_033_988_75, 0.414_213_562_37, 0.732_050_807_57).map(|v| ((k + 1) as f64 * v).fract() * 1e-7 * scale);
        let positions = positions.into_iter().map(|p| p + jitter).collect();
        let tool = Surface { positions, triangles: body.triangles.clone(), tags: body.tags.clone() };
        let base = air.tags.iter().map(|t| t + 1).max().unwrap_or(0).max(6);
        air = match combine_simplified(&air, &tool, Op::Difference, base) {
            Ok(a) => a,
            Err(MeshError::Invalid { why, .. }) if why == "nothing is left" => return Err(fail(NO_AIR)),
            Err(e) => return Err(fail(format!("taking `{name}` from the box: {e}"))),
        };
    }
    pieces(air, keep)
}

/// A connected part of a surface: its triangles, signed volume and whether it reaches a side
/// face of the box.
struct Shell {
    triangles: Vec<usize>,
    volume: f64,
    side: bool,
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// The surface's shells (triangles joined by edges), in the order of their first triangle.
fn shells(s: &Surface) -> Vec<Shell> {
    let mut parent: Vec<usize> = (0..s.triangles.len()).collect();
    let mut by_edge: std::collections::HashMap<[u32; 2], usize> = std::collections::HashMap::new();
    for (i, t) in s.triangles.iter().enumerate() {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            match by_edge.get(&[a.min(b), a.max(b)]) {
                Some(&j) => {
                    let (x, y) = (find(&mut parent, i), find(&mut parent, j));
                    if x != y {
                        parent[x.max(y)] = x.min(y);
                    }
                }
                None => {
                    by_edge.insert([a.min(b), a.max(b)], i);
                }
            }
        }
    }
    let mut out: Vec<Shell> = Vec::new();
    let mut index: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    for (i, t) in s.triangles.iter().enumerate() {
        let r = find(&mut parent, i);
        let k = *index.entry(r).or_insert_with(|| {
            out.push(Shell { triangles: Vec::new(), volume: 0.0, side: false });
            out.len() - 1
        });
        let p = |v: u32| s.positions.get(v as usize).copied().unwrap_or(Point3::ORIGIN).to_vec();
        out[k].triangles.push(i);
        out[k].volume += emcraft_model::geom::triple(p(t[0]), p(t[1]), p(t[2])) / 6.0;
        out[k].side |= s.tags.get(i).is_some_and(|tag| *tag < 4);
    }
    out
}

/// The winding number of a shell about `p`.
fn winding(s: &Surface, shell: &Shell, p: Point3) -> f64 {
    let corner = |v: u32| s.positions.get(v as usize).copied().unwrap_or(Point3::ORIGIN) - p;
    shell.triangles.iter().filter_map(|&t| s.triangles.get(t)).map(|t| emcraft_model::solid_angle(t.map(corner))).sum::<f64>()
        / (4.0 * std::f64::consts::PI)
}

/// The kept pieces of the box less the bodies.
fn pieces(air: Surface, keep: Keep) -> Result<Cavity> {
    let shells = shells(&air);
    // Each hole (a shell enclosing metal, negative volume) belongs to the smallest piece of air
    // around it.
    let pieces: Vec<usize> = (0..shells.len()).filter(|&k| shells[k].volume > 0.0).collect();
    let mut owner: Vec<Option<usize>> = vec![None; shells.len()];
    for (k, sh) in shells.iter().enumerate() {
        if sh.volume > 0.0 {
            owner[k] = Some(k);
            continue;
        }
        let Some(&t) = sh.triangles.first() else { continue };
        let Some(&v) = air.triangles.get(t).map(|t| &t[0]) else { continue };
        let p = air.positions.get(v as usize).copied().unwrap_or(Point3::ORIGIN);
        owner[k] = pieces
            .iter()
            .copied()
            .filter(|&q| winding(&air, &shells[q], p) > 0.5)
            .min_by(|&a, &b| shells[a].volume.total_cmp(&shells[b].volume).then(a.cmp(&b)));
    }
    let enclosed: Vec<usize> = pieces.iter().copied().filter(|&q| !shells[q].side).collect();
    let volume = |q: usize| -> f64 { shells.iter().zip(&owner).filter(|(_, o)| **o == Some(q)).map(|(s, _)| s.volume).sum() };
    let kept: Vec<usize> = match keep {
        Keep::Enclosed => enclosed.clone(),
        Keep::Largest => enclosed.iter().copied().max_by(|&a, &b| volume(a).total_cmp(&volume(b)).then(b.cmp(&a))).into_iter().collect(),
    };
    if kept.is_empty() {
        return Err(fail(NO_AIR));
    }
    let mut take = vec![false; air.triangles.len()];
    for (sh, o) in shells.iter().zip(&owner) {
        if o.is_some_and(|q| kept.contains(&q)) {
            for &t in &sh.triangles {
                take[t] = true;
            }
        }
    }
    let mut index = vec![u32::MAX; air.positions.len()];
    let mut surface = Surface::default();
    for (t, tri) in air.triangles.iter().enumerate() {
        if !take[t] {
            continue;
        }
        let mut ids = [0u32; 3];
        for (id, &v) in ids.iter_mut().zip(tri) {
            let slot = index.get_mut(v as usize).ok_or_else(|| fail("a triangle has a missing corner"))?;
            if *slot == u32::MAX {
                surface.positions.push(air.positions.get(v as usize).copied().unwrap_or(Point3::ORIGIN));
                *slot = (surface.positions.len() - 1) as u32;
            }
            *id = *slot;
        }
        surface.triangles.push(ids);
        surface.tags.push(air.tags.get(t).copied().unwrap_or(0));
    }
    Ok(Cavity { surface, kept: kept.iter().map(|&q| volume(q)).collect(), dropped: pieces.len() - kept.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(lo: [f64; 3], hi: [f64; 3]) -> Surface {
        bounding_box(Point3::from(lo), Point3::from(hi))
    }

    fn volume(s: &Surface) -> f64 {
        s.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|v| s.positions[v as usize].to_vec());
                emcraft_model::geom::triple(a, b, c) / 6.0
            })
            .sum()
    }

    /// The waveguide block of the experiments: a 32.86 × 20.16 × 32 mm block, the 22.86 × 10.16
    /// channel through it along z, two iris plates (5.43 × 10.16 × 2 mm) against its side walls —
    /// here as three bodies: two halves of the block and the plates on their own.
    fn block() -> Vec<(String, Surface)> {
        let (a, b, l) = (22.86, 10.16, 32.0);
        vec![
            ("Left".into(), cube([-5.0, -5.0, 0.0], [0.0, b + 5.0, l])),
            ("Right".into(), cube([a, -5.0, 0.0], [a + 5.0, b + 5.0, l])),
            ("Floor".into(), cube([0.0, -5.0, 0.0], [a, 0.0, l])),
            ("Roof".into(), cube([0.0, b, 0.0], [a, b + 5.0, l])),
            ("IrisL".into(), cube([0.0, 0.0, 15.0], [5.43, b, 17.0])),
            ("IrisR".into(), cube([17.43, 0.0, 15.0], [a, b, 17.0])),
        ]
    }

    #[test]
    fn a_channel_through_a_block_is_the_region() {
        let c = cavity(&block(), Keep::Enclosed).unwrap();
        let (a, b, l) = (22.86, 10.16, 32.0);
        let exact = a * b * l - 2.0 * 5.43 * b * 2.0;
        assert!((volume(&c.surface) - exact).abs() < 1e-6 * exact, "{}", volume(&c.surface));
        assert_eq!(c.kept.len(), 1);
        assert!((c.kept[0] - exact).abs() < 1e-6 * exact);
        // The ends: the box's z faces, the channel's opening only.
        let ends: f64 = c
            .surface
            .triangles
            .iter()
            .zip(&c.surface.tags)
            .filter(|(_, t)| **t == 4 || **t == 5)
            .map(|(t, _)| {
                let [p, q, r] = t.map(|v| c.surface.positions[v as usize]);
                0.5 * (q - p).cross(r - p).norm()
            })
            .sum();
        assert!((ends - 2.0 * a * b).abs() < 1e-6 * a * b, "{ends}");
        // No side face of the box is left; the surface is the air's own, and few triangles.
        assert!(c.surface.tags.iter().all(|t| *t >= 4));
        assert!(c.surface.triangles.len() < 100, "{}", c.surface.triangles.len());
    }

    #[test]
    fn outside_air_is_dropped_and_free_posts_are_holes() {
        // An L-shaped outline enclosing nothing: the corner's air reaches the sides.
        let mut bodies: Vec<(String, Surface)> =
            vec![("Block".into(), cube([0.0, 0.0, 0.0], [10.0, 10.0, 20.0])), ("Lug".into(), cube([0.0, 10.0, 0.0], [4.0, 14.0, 20.0]))];
        let e = cavity(&bodies, Keep::Enclosed).unwrap_err().to_string();
        assert!(e.contains("enclose no air"), "{e}");
        // A solid block: no air at all.
        assert!(cavity(&bodies[..1], Keep::Enclosed).is_err());
        // A shell around a channel, a lug standing out of its side (the air beside the lug is
        // dropped), the channel holding a free post (a hole in the air).
        bodies = vec![
            ("Left".into(), cube([-2.0, -2.0, 0.0], [0.0, 6.0, 20.0])),
            ("Right".into(), cube([8.0, -2.0, 0.0], [10.0, 6.0, 20.0])),
            ("Floor".into(), cube([0.0, -2.0, 0.0], [8.0, 0.0, 20.0])),
            ("Roof".into(), cube([0.0, 4.0, 0.0], [8.0, 6.0, 20.0])),
            ("Lug".into(), cube([10.0, -2.0, 0.0], [12.0, 1.0, 20.0])),
            ("Post".into(), cube([3.0, 1.0, 9.0], [5.0, 3.0, 11.0])),
        ];
        let c = cavity(&bodies, Keep::Enclosed).unwrap();
        let exact = 8.0 * 4.0 * 20.0 - 8.0;
        assert!((volume(&c.surface) - exact).abs() < 1e-6 * exact, "{}", volume(&c.surface));
        assert_eq!((c.kept.len(), c.dropped), (1, 1));
        // Two channels side by side (4 and 10 wide): both kept, or the wider alone.
        bodies = vec![
            ("Left".into(), cube([-2.0, -2.0, 0.0], [0.0, 6.0, 20.0])),
            ("Right".into(), cube([16.0, -2.0, 0.0], [18.0, 6.0, 20.0])),
            ("Floor".into(), cube([0.0, -2.0, 0.0], [16.0, 0.0, 20.0])),
            ("Roof".into(), cube([0.0, 4.0, 0.0], [16.0, 6.0, 20.0])),
            ("Middle".into(), cube([4.0, 0.0, 0.0], [6.0, 4.0, 20.0])),
        ];
        // The wall first, the floor after it: the floor's top lies partly against the wall and
        // partly under the channels' air — refused, not guessed.
        let mut reordered = bodies.clone();
        reordered.rotate_right(1);
        let e = cavity(&reordered, Keep::Enclosed).unwrap_err().to_string();
        assert!(e.contains("partly against"), "{e}");
        let both = cavity(&bodies, Keep::Enclosed).unwrap();
        assert_eq!(both.kept.len(), 2);
        assert!((volume(&both.surface) - 1120.0).abs() < 1e-3, "{}", volume(&both.surface));
        let wide = cavity(&bodies, Keep::Largest).unwrap();
        assert_eq!((wide.kept.len(), wide.dropped), (1, 1));
        assert!((volume(&wide.surface) - 800.0).abs() < 1e-3, "{}", volume(&wide.surface));
    }
}

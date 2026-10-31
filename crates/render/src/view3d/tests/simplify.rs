use super::*;
use crate::Rgb;

fn len(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// A sphere of radius `r` as a scan reads it: `n` × 2`n` quads, each triangle's corners its own
/// points (the same position repeated where triangles meet).
fn sphere(r: f64, n: u32) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let at = |i: u32, j: u32| {
        let (t, p) = (std::f64::consts::PI * f64::from(i) / f64::from(n), std::f64::consts::TAU * f64::from(j) / f64::from(2 * n));
        let (st, ct, sp, cp) = (t.sin(), t.cos(), p.sin(), p.cos());
        // The poles exactly, and the seam the same as the start, so corners weld.
        let (st, sp, cp) = (if i == 0 || i == n { 0.0 } else { st }, if j == 2 * n { 0.0 } else { sp }, if j == 2 * n { 1.0 } else { cp });
        [r * st * cp, r * st * sp, r * ct]
    };
    let (mut pos, mut tris) = (Vec::new(), Vec::new());
    let mut tri = |a, b, c| {
        let k = pos.len() as u32;
        pos.extend([a, b, c]);
        tris.push([k, k + 1, k + 2]);
    };
    for i in 0..n {
        for j in 0..2 * n {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            if i > 0 {
                tri(a, c, d);
            }
            if i + 1 < n {
                tri(a, b, c);
            }
        }
    }
    (pos, tris)
}

fn volume(l: &Level) -> f64 {
    l.triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| l.positions[i as usize]);
            dot(a, cross(b, c)) / 6.0
        })
        .sum()
}

/// Each side of a closed surface is shared by exactly two triangles, once each way.
fn closed(l: &Level) -> bool {
    let mut sides: Vec<(u32, u32)> = l.triangles.iter().flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]).collect();
    sides.sort_unstable();
    let n = sides.len();
    sides.dedup();
    sides.len() == n && sides.iter().all(|(a, b)| sides.binary_search(&(*b, *a)).is_ok())
}

/// A finely meshed sphere comes down to each target, closed and wound as before, its points
/// near the sphere and its volume kept; the same mesh simplifies the same way.
#[test]
fn a_sphere_comes_down_to_each_target_closed_and_near_the_sphere() {
    let (pos, tris) = sphere(50.0, 100);
    let full = tris.len();
    let made = levels(&pos, &tris, &[full / 4, full / 16, full / 64]);
    assert_eq!(made.len(), 3);
    let exact = 4.0 / 3.0 * std::f64::consts::PI * 50.0f64.powi(3);
    for (l, target) in made.iter().zip([full / 4, full / 16, full / 64]) {
        assert!(l.triangles.len() <= target, "{} > {target}", l.triangles.len());
        assert!(closed(l), "closed and consistently wound");
        assert!(l.positions.iter().all(|p| (len(*p) - 50.0).abs() < 1.0), "near the sphere");
        assert!((volume(l) - exact).abs() < exact * 0.02, "volume {} vs {exact}", volume(l));
    }
    let again = levels(&pos, &tris, &[full / 4, full / 16, full / 64]);
    assert_eq!(made, again);
}

/// A flat sheet comes down to a few triangles, flat, and its open outline stays put: the same
/// area.
#[test]
fn a_flat_sheet_keeps_its_outline() {
    let n = 40u32;
    let at = |i: u32, j: u32| [f64::from(i), f64::from(j) * 0.5, 0.0];
    let (mut pos, mut tris) = (Vec::new(), Vec::new());
    for i in 0..n {
        for j in 0..n {
            for t in [[at(i, j), at(i + 1, j), at(i + 1, j + 1)], [at(i, j), at(i + 1, j + 1), at(i, j + 1)]] {
                let k = pos.len() as u32;
                pos.extend(t);
                tris.push([k, k + 1, k + 2]);
            }
        }
    }
    let made = levels(&pos, &tris, &[tris.len() / 16]);
    let l = made.first().unwrap();
    assert!(l.triangles.len() <= tris.len() / 16);
    assert!(l.positions.iter().all(|p| p[2] == 0.0));
    let area: f64 = l
        .triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| l.positions[i as usize]);
            len(cross(sub(b, a), sub(c, a))) / 2.0
        })
        .sum();
    assert!((area - 800.0).abs() < 1e-6, "the sheet is still whole: {area}");
}

/// Input is hostile: points that are not finite, indices out of range, triangles with a
/// repeated corner and empty meshes are left out, never a panic.
#[test]
fn hostile_meshes_never_panic() {
    let p = |x: f64, y: f64| [x, y, 0.0];
    assert!(levels(&[], &[], &[1]).is_empty());
    assert!(levels(&[p(0.0, 0.0)], &[[0, 0, 0]], &[1]).is_empty());
    assert!(levels(&[p(f64::NAN, 0.0), p(1.0, 0.0), p(0.0, 1.0)], &[[0, 1, 2]], &[0]).is_empty());
    let (pos, mut tris) = sphere(5.0, 12);
    tris.push([0, 1, u32::MAX]);
    tris.push([2, 2, 3]);
    let made = levels(&pos, &tris, &[usize::MAX, 100, 0]);
    assert!(made.iter().all(|l| l.triangles.iter().flatten().all(|i| (*i as usize) < l.positions.len())));
    let mut s = Scene::default();
    s.tri([f64::NAN; 3], [1.0; 3], [0.0; 3], Rgb(0, 0, 0), 255);
    assert!(coarser(&s, 0).is_none());
}

/// A scene comes down group by group: each colour, opacity and tag kept on its own triangles,
/// small groups and lines untouched, and the same scene gives the same coarser scene.
#[test]
fn a_scene_comes_down_group_by_group() {
    let (pos, tris) = sphere(1.0, 60);
    let mut s = Scene::default();
    for (k, colour) in [Rgb(200, 0, 0), Rgb(0, 200, 0)].into_iter().enumerate() {
        let shift = k as f64 * 3.0;
        s.tagged(k as u32 + 1, |s| {
            for t in &tris {
                let [a, b, c] = t.map(|i| {
                    let p = pos[i as usize];
                    [p[0] + shift, p[1], p[2]]
                });
                s.tri(a, b, c, colour, 255);
            }
        });
    }
    s.tagged(9, |s| s.cuboid([10.0; 3], [11.0; 3], Rgb(0, 0, 200), 128));
    s.line([0.0; 3], [1.0; 3], Rgb(1, 1, 1), 255);
    let c = coarser(&s, s.tris.len() / 10).unwrap();
    assert!(c.tris.len() <= s.tris.len() / 10 + 20, "{} of {}", c.tris.len(), s.tris.len());
    assert_eq!(c.lines, s.lines);
    for (tag, colour, x) in [(1, Rgb(200, 0, 0), 0.0), (2, Rgb(0, 200, 0), 3.0)] {
        let mine: Vec<&Tri> = c.tris.iter().filter(|t| t.tag == tag).collect();
        assert!(mine.len() > 100 && mine.iter().all(|t| t.color == colour && t.alpha == 255));
        // Each sphere stays where it was.
        assert!(mine.iter().flat_map(|t| t.p).all(|p| (len([p[0] - x, p[1], p[2]]) - 1.0).abs() < 0.05));
    }
    let small: Vec<Tri> = c.tris.iter().filter(|t| t.tag == 9).copied().collect();
    assert_eq!(small, s.tris.iter().filter(|t| t.tag == 9).copied().collect::<Vec<_>>());
    assert_eq!(coarser(&s, s.tris.len() / 10), Some(c));
}

/// Timing, not a check (`cargo test --release -p emcraft-render --lib simplify_timing --
/// --ignored --nocapture`): a scene of about 200 000 triangles brought down to 40 000.
#[test]
#[ignore = "timing: cargo test --release -p emcraft-render --lib simplify_timing -- --ignored --nocapture"]
fn simplify_timing() {
    let (pos, tris) = sphere(1.0, 316);
    let mut s = Scene::default();
    s.tagged(1, |s| {
        for t in &tris {
            let [a, b, c] = t.map(|i| pos[i as usize]);
            s.tri(a, b, c, Rgb(9, 9, 9), 255);
        }
    });
    let started = std::time::Instant::now();
    let c = coarser(&s, 40_000).map_or(0, |c| c.tris.len());
    eprintln!("sphere: {} triangles to {c} in {:.0} ms", s.tris.len(), started.elapsed().as_secs_f64() * 1e3);
    // Flat walls, where every collapse inside a face costs nothing: a box of 6 × 2 × 129².
    let mut s = Scene::default();
    let n = 129;
    for k in 0..3 {
        let (i, j) = ((k + 1) % 3, (k + 2) % 3);
        for side in [0.0, 1.0] {
            for a in 0..n {
                for b in 0..n {
                    let at = |u: usize, v: usize| {
                        let mut p = [side; 3];
                        (p[i], p[j]) = (u as f64 / n as f64, v as f64 / n as f64);
                        p
                    };
                    s.tagged(1, |s| s.quad([at(a, b), at(a + 1, b), at(a + 1, b + 1), at(a, b + 1)], Rgb(9, 9, 9), 255));
                }
            }
        }
    }
    let started = std::time::Instant::now();
    let c = coarser(&s, 40_000).map_or(0, |c| c.tris.len());
    eprintln!("box: {} triangles to {c} in {:.0} ms", s.tris.len(), started.elapsed().as_secs_f64() * 1e3);
}

use super::super::{Projection, StandardView, hits};
use super::*;
use crate::Rgb;

/// A small deterministic generator (the tests need no crate for randomness).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
    fn pick<T: Copy>(&mut self, of: &[T]) -> T {
        of[((self.next() * of.len() as f64) as usize).min(of.len() - 1)]
    }
}

/// A scene as the app draws one: boxes of tagged quads (walls, some see-through, some
/// untagged), a grid of squares on z = 0 whose shared edges and corners tie, random triangles
/// above, and a few triangles that are not finite.
fn scene(rng: &mut Lcg) -> Scene {
    let mut s = Scene::default();
    let n = 24;
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (f64::from(i), f64::from(j));
            let alpha = if (i + j) % 3 == 0 { 120 } else { 255 };
            s.tagged(1 + (i * n + j) % 7, |s| {
                s.quad([[x, y, 0.0], [x + 1.0, y, 0.0], [x + 1.0, y + 1.0, 0.0], [x, y + 1.0, 0.0]], Rgb(1, 2, 3), alpha)
            });
        }
    }
    for k in 0..12u32 {
        let lo = [rng.range(0.0, 20.0), rng.range(0.0, 20.0), rng.range(0.5, 10.0)];
        let hi = [lo[0] + rng.range(0.5, 4.0), lo[1] + rng.range(0.5, 4.0), lo[2] + rng.range(0.5, 4.0)];
        let alpha = rng.pick(&[255u8, 255, 199, 200, 60]);
        s.tagged(rng.pick(&[0u32, 10 + k, 10 + k]), |s| s.cuboid(lo, hi, Rgb(4, 5, 6), alpha));
    }
    // The same box twice: equal distances, the lower index wins.
    s.tagged(30, |s| s.cuboid([2.0, 2.0, 12.0], [5.0, 5.0, 14.0], Rgb(7, 7, 7), 255));
    s.tagged(31, |s| s.cuboid([2.0, 2.0, 12.0], [5.0, 5.0, 14.0], Rgb(7, 7, 7), 255));
    for _ in 0..600 {
        let c = [rng.range(0.0, 24.0), rng.range(0.0, 24.0), rng.range(1.0, 20.0)];
        let mut p = || [c[0] + rng.range(-1.5, 1.5), c[1] + rng.range(-1.5, 1.5), c[2] + rng.range(-1.5, 1.5)];
        let (a, b, q) = (p(), p(), p());
        let tag = rng.pick(&[0u32, 40, 41, 42]);
        let alpha = rng.pick(&[255u8, 128, 200]);
        s.tagged(tag, |s| s.tri(a, b, q, Rgb(8, 8, 8), alpha));
    }
    s.tagged(50, |s| {
        s.tri([f64::NAN, 0.0, 0.0], [1.0, 0.0, 5.0], [0.0, 1.0, 5.0], Rgb(0, 0, 0), 255);
        s.tri([f64::INFINITY, 0.0, 5.0], [1.0, 0.0, 5.0], [0.0, 1.0, 5.0], Rgb(0, 0, 0), 255);
    });
    s
}

/// The index answers every pixel as the scan of every triangle does: the same nearest hit and
/// nearest solid hit, the same triangle where two tie, on many cameras and pixels (seeded).
#[test]
fn indexed_hits_answer_as_the_scan_does() {
    let mut rng = Lcg(0x5eed);
    let s = scene(&mut rng);
    let index = SceneIndex::new(&s);
    let (w, h) = (320.0, 240.0);
    let mut cams = Vec::new();
    for v in [StandardView::Top, StandardView::Front, StandardView::Right, StandardView::Iso] {
        for p in [Projection::Perspective, Projection::Orthographic] {
            let mut c = Camera { projection: p, ..Camera::default() };
            c.standard(v);
            c.fit([0.0, 0.0, 0.0], [24.0, 24.0, 20.0], w / h);
            cams.push(c);
        }
    }
    for _ in 0..16 {
        let mut c = Camera { projection: rng.pick(&[Projection::Perspective, Projection::Orthographic]), ..Camera::default() };
        c.fit([0.0, 0.0, 0.0], [24.0, 24.0, 20.0], w / h);
        (c.yaw, c.pitch) = (rng.range(-3.2, 3.2), rng.range(-1.5, 1.5));
        c.distance *= rng.range(0.3, 2.0);
        cams.push(c);
    }
    let (mut hit_some, mut ties) = (0, 0);
    for (k, c) in cams.iter().enumerate() {
        for i in 0..400 {
            // Every few pixels on a whole or half pixel (the top views look down grid lines).
            let (x, y) = if i % 3 == 0 {
                ((i * 7 % 320) as f64, (i * 13 % 240) as f64 * 0.5)
            } else {
                (rng.range(-10.0, w + 10.0), rng.range(-10.0, h + 10.0))
            };
            let want = hits(&s, c, x, y, w, h);
            assert_eq!(index.hits(&s, c, x, y, w, h), want, "camera {k} pixel ({x}, {y})");
            assert_eq!(index.pick(&s, c, x, y, w, h), super::super::pick(&s, c, x, y, w, h));
            if let Some((near, _)) = want {
                hit_some += 1;
                // A tie on the grid: another triangle as near.
                let (o, d) = c.ray(x, y, w, h).unwrap_or_default();
                let at = |i: usize| hit(o, d, &s.tris[i]);
                let best = at(near.index);
                ties += usize::from(s.tris.iter().enumerate().any(|(j, t)| j != near.index && t.tag != 0 && at(j) == best));
            }
        }
    }
    assert!(hit_some > 1500, "many pixels hit: {hit_some}");
    assert!(ties > 10, "ties were tried: {ties}");
}

/// The triangles near a plane include every triangle lying in it.
#[test]
fn triangles_near_a_plane_include_all_in_it() {
    let mut rng = Lcg(3);
    let s = scene(&mut rng);
    let index = SceneIndex::new(&s);
    assert_eq!(index.bounds(), s.bounds());
    let n = [0.0, 0.0, 1.0];
    let near = index.near_plane(&s, n, 0.0, 1e-9).unwrap();
    let flat: Vec<usize> = s.tris.iter().enumerate().filter(|(_, t)| t.tag != 0 && t.p.iter().all(|p| p[2].abs() <= 1e-9)).map(|(i, _)| i).collect();
    assert!(flat.len() >= 2 * 24 * 24);
    assert!(flat.iter().all(|i| near.binary_search(i).is_ok()), "every flat triangle is found");
    assert!(near.len() < flat.len() + 100, "and few others: {} of {}", near.len(), s.tris.len());
    assert!(near.windows(2).all(|w| w[0] < w[1]), "in order, once each");
    // Hostile planes and another scene: no answer, no panic.
    assert!(index.near_plane(&s, [f64::NAN, 0.0, 0.0], 0.0, 1.0).is_none());
    assert!(index.near_plane(&Scene::default(), n, 0.0, 1.0).is_none());
}

/// An index of another scene answers as the scan of the scene asked about; empty scenes and
/// hostile cameras have no hits.
#[test]
fn another_scene_and_hostile_input_are_scanned_or_refused() {
    let mut rng = Lcg(11);
    let s = scene(&mut rng);
    let stale = SceneIndex::new(&Scene::default());
    let mut c = Camera::default();
    c.fit([0.0, 0.0, 0.0], [24.0, 24.0, 20.0], 1.0);
    for (x, y) in [(50.0, 50.0), (30.0, 70.0)] {
        assert_eq!(stale.hits(&s, &c, x, y, 100.0, 100.0), hits(&s, &c, x, y, 100.0, 100.0));
    }
    assert!(SceneIndex::new(&Scene::default()).hits(&Scene::default(), &c, 50.0, 50.0, 100.0, 100.0).is_none());
    let index = SceneIndex::new(&s);
    for (x, y) in [(f64::NAN, 1.0), (f64::INFINITY, 0.0), (1e308, -1e308)] {
        assert_eq!(index.hits(&s, &c, x, y, 100.0, 100.0), hits(&s, &c, x, y, 100.0, 100.0));
    }
}

/// Timing, not a check (`cargo test --release -p emcraft-render --lib bvh_timing -- --ignored
/// --nocapture`): a picked ray on a scene of about 200 000 triangles, scanned and indexed.
#[test]
#[ignore = "timing: cargo test --release -p emcraft-render --lib bvh_timing -- --ignored --nocapture"]
fn bvh_timing() {
    let mut s = Scene::default();
    // A finely meshed closed box: 6 faces of 129 × 129 squares, ~200k triangles.
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
                    s.tagged(1, |s| s.quad([at(a, b), at(a + 1, b), at(a + 1, b + 1), at(a, b + 1)], Rgb(1, 1, 1), 255));
                }
            }
        }
    }
    let mut c = Camera::default();
    c.fit([0.0; 3], [1.0; 3], 1.0);
    let rays = 200;
    let started = std::time::Instant::now();
    let index = SceneIndex::new(&s);
    let build = started.elapsed().as_secs_f64() * 1e3;
    let time = |f: &dyn Fn(f64, f64) -> Option<(Hit, Option<Hit>)>| {
        let started = std::time::Instant::now();
        for r in 0..rays {
            std::hint::black_box(f(20.0 + (r % 60) as f64, 20.0 + (r / 60) as f64 * 15.0));
        }
        started.elapsed().as_secs_f64() * 1e3 / rays as f64
    };
    let scan = time(&|x, y| hits(&s, &c, x, y, 100.0, 100.0));
    let indexed = time(&|x, y| index.hits(&s, &c, x, y, 100.0, 100.0));
    eprintln!("{} triangles: index built in {build:.1} ms; a pick {scan:.3} ms scanned, {indexed:.4} ms indexed", s.tris.len());
}

//! The axisymmetric PIC against closed forms: a pillbox's TM010, Gauss's law kept by moving
//! rings, a bunch's loss factor through a pillbox, a ring beam's space-charge depression.

use std::f64::consts::PI;

use emcraft_model::units::C0;
use emcraft_particles::{ELECTRON, Pusher, Species};

use super::*;
use crate::pic::{Pulse, Wall};

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn pillbox(h: f64) -> (AxiPic, f64, f64) {
    let (a, l) = (0.01f64, 0.01306f64);
    let g = AxiGrid::new((a / h).round() as usize, (l / h).round() as usize, h, l / (l / h).round(), Wall::Conductor).unwrap();
    let dt = g.time_step(0.95);
    (AxiPic::new(g, dt, Pusher::Boris).unwrap(), a, l)
}

#[test]
fn a_pillbox_rings_at_tm010() {
    // E_z = J₀(x₀₁r/a) in a closed cylinder rings at x₀₁c/(2πa) (Pozar §6.4): 11.4743 GHz for
    // 10 mm; 1.1e-4 low on 0.25 mm cells.
    use emcraft_theory::bessel::j;
    let (mut pic, a, l) = pillbox(0.25e-3);
    let dt = pic.dt();
    let x01 = 2.404_825_557_695_773;
    pic.set(2, |r, _| j(0, x01 * r / a));
    let mut ups = Vec::new();
    let mut last = pic.field_at(0.0, 0.5 * l).0[2];
    for n in 0..((5.0 / 11.47e9) / dt) as usize {
        pic.step().unwrap();
        let now = pic.field_at(0.0, 0.5 * l).0[2];
        if last < 0.0 && now >= 0.0 {
            ups.push((n as f64 + last / (last - now)) * dt);
        }
        last = now;
    }
    let f = (ups.len() - 1) as f64 / (ups[ups.len() - 1] - ups[0]);
    let want = x01 * C0 / (2.0 * PI * a);
    assert!((f / want - 1.0).abs() < 3e-4, "{f:e} Hz against {want:e}");
}

#[rstest::rstest]
#[case::closed(Wall::Conductor)]
#[case::periodic(Wall::Periodic)]
fn rings_keep_gauss_law_to_rounding(#[case] wall: Wall) {
    // Electron and positron rings up to γ ≈ 2 in every direction, a static B_z, a metal block
    // and the walls: the flux form ε₀∮E·dA − Q stays where it started at every node.
    let g = AxiGrid::new(12, 30, 1e-3, 0.8e-3, wall).unwrap();
    let mut pic = AxiPic::new(g, g.time_step(0.9), Pusher::Vay).unwrap();
    pic.set_metal(|r, z| r > 0.009 && (z - 0.012).abs() < 0.002);
    let mut rnd = Lcg(5);
    let positron = Species { charge: -ELECTRON.charge, mass: ELECTRON.mass };
    for species in [ELECTRON, positron] {
        let rings = (0..300)
            .map(|_| Ring { r: 0.011 * rnd.next(), z: 0.024 * rnd.next(), u: [(); 3].map(|()| 2.0 * C0 * (2.0 * rnd.next() - 1.0)), w: 1e6 })
            .collect();
        pic.add(Rings { species, rings }).unwrap();
    }
    pic.set(5, |_, _| 0.2);
    let before = pic.gauss();
    let scale = before.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    for _ in 0..150 {
        pic.step().unwrap();
    }
    let drift = before.iter().zip(pic.gauss()).fold(0.0f64, |m, (x, y)| m.max((x - y).abs()));
    assert!(drift < 1e-12 * scale, "{drift:e} of {scale:e}");
    assert!(pic.rings() < 600, "some reach the walls");
}

#[test]
fn a_bunch_on_the_axis_leaves_the_pillboxs_loss_factor() {
    // As the Cartesian grid's test, with the cylinder exact: q² Σ k_n e^{−(ω_nσ_t)²} over the
    // TM0np modes (Wilson, SLAC-PUB-4547; emcraft_theory::wake), 5.805 V/pC; 0.05 % on 0.5 mm.
    let (mut pic, a, l) = pillbox(0.5e-3);
    let dt = pic.dt();
    let k = pic.add(Rings { species: ELECTRON, rings: Vec::new() }).unwrap();
    let sigma = 2e-3 / C0;
    let pulse = Pulse::Gaussian { charge: 1e-9, t0: 5.0 * sigma, sigma };
    pic.inject(Injection { high: false, population: k, pulse, inner: 0.0, radius: 0.0, energy: 1e10 * -ELECTRON.charge, per_step: 4 }).unwrap();
    let mut q = 0.0;
    for _ in 0..((10.0 * sigma + l / C0) / dt) as usize + 10 {
        q += pic.step().unwrap().emitted;
    }
    let got = pic.energy().0 / (q * q);
    let want = emcraft_theory::wake::Pillbox::new(a, l).loss_factor(1.0, sigma).unwrap();
    assert!((got / want - 1.0).abs() < 2e-3, "{got:e} V/C against {want:e}");
}

#[test]
fn a_ring_beam_sinks_by_its_space_charge_and_stops_at_the_limit() {
    // Bogdankevich & Rukhadze (Sov. Phys. Usp. 14:163, 1971): γ₀ = 1.25 on a 6 mm ring in a 10 mm
    // tube held by 10 T; at 0.7 of the limiting current it sinks to γ_b (2 % deeper here, the
    // ring 0.5 mm thick), and at 1.3 of it no more than about the limit passes. Faster beams
    // (γ₀ ≳ 1.5) drive the grid's numerical Cherenkov instability at this time step.
    use emcraft_theory::plasma::AnnularBeam;
    let (r, rb, l, h) = (0.01f64, 0.006, 0.08, 0.5e-3);
    let beam = AnnularBeam { gamma0: 1.25, ratio: r / rb, charge: ELECTRON.charge, mass: ELECTRON.mass };
    let il = beam.limiting().unwrap();
    let run = |current: f64| {
        let g = AxiGrid::new((r / h).round() as usize, (l / h).round() as usize, h, h, Wall::Conductor).unwrap();
        let dt = g.time_step(0.95);
        let mut pic = AxiPic::new(g, dt, Pusher::Boris).unwrap();
        pic.set(5, |_, _| 10.0);
        let k = pic.add(Rings { species: ELECTRON, rings: Vec::new() }).unwrap();
        let pulse = Pulse::Steady { current, rise: 0.2e-9 };
        pic.inject(Injection {
            high: false,
            population: k,
            pulse,
            inner: rb - 0.25e-3,
            radius: rb + 0.25e-3,
            energy: 0.25 * ELECTRON.rest_energy(),
            per_step: 16,
        })
        .unwrap();
        let steps = (2e-9 / dt) as usize;
        let (mut out, mut sum, mut n) = (0.0, 0.0, 0usize);
        for s in 0..steps {
            let m = pic.step().unwrap();
            if s >= steps / 2 {
                out += m.collected[2][1];
                if s % 10 == 0 {
                    for p in pic.populations()[k].rings.iter().filter(|p| p.z > l / 3.0 && p.z < 2.0 * l / 3.0) {
                        sum += emcraft_particles::gamma(p.u);
                        n += 1;
                    }
                }
            }
        }
        (sum / n.max(1) as f64, out.abs() / ((steps - steps / 2) as f64 * dt))
    };
    let want = beam.depressed(0.7 * il).unwrap();
    let (g, passed) = run(0.7 * il);
    assert!(((1.25 - g) / (1.25 - want) - 1.0).abs() < 0.04, "γ_b {g} against {want}");
    assert!((passed / (0.7 * il) - 1.0).abs() < 0.02);
    let (_, over) = run(1.3 * il);
    assert!(over < 1.0 * il && over > 0.8 * il, "{over} A past the limit {il}");
}

#[test]
fn hostile_axisymmetric_settings_are_refused() {
    assert!(AxiGrid::new(0, 10, 1e-3, 1e-3, Wall::Conductor).is_err());
    assert!(AxiGrid::new(10, 10, f64::NAN, 1e-3, Wall::Conductor).is_err());
    assert!(AxiGrid::new(100_000, 100_000, 1e-3, 1e-3, Wall::Conductor).is_err());
    let g = AxiGrid::new(10, 20, 1e-3, 1e-3, Wall::Conductor).unwrap();
    for dt in [0.0, f64::NAN, g.time_step(1.2)] {
        assert!(AxiPic::new(g, dt, Pusher::Boris).is_err());
    }
    let mut pic = AxiPic::new(g, g.time_step(0.9), Pusher::Boris).unwrap();
    let one = |r: f64, z: f64, w: f64| Rings { species: ELECTRON, rings: vec![Ring { r, z, u: [0.0; 3], w }] };
    assert!(pic.add(one(0.02, 0.01, 1.0)).is_err() && pic.add(one(0.005, -1.0, 1.0)).is_err() && pic.add(one(0.005, 0.01, -1.0)).is_err());
    let k = pic.add(one(0.005, 0.01, 1.0)).unwrap();
    let base = Injection {
        high: false,
        population: k,
        pulse: Pulse::Steady { current: 1.0, rise: 0.0 },
        inner: 0.0,
        radius: 0.005,
        energy: 1e-15,
        per_step: 4,
    };
    for bad in [
        Injection { population: 7, ..base },
        Injection { inner: 0.006, ..base },
        Injection { radius: 0.5, ..base },
        Injection { per_step: 0, ..base },
    ] {
        assert!(pic.inject(bad).is_err(), "{bad:?}");
    }
    assert!(pic.absorb(true, 0).is_err() && pic.absorb(true, 25).is_err());
    let periodic = AxiGrid::new(10, 20, 1e-3, 1e-3, Wall::Periodic).unwrap();
    let mut p = AxiPic::new(periodic, periodic.time_step(0.9), Pusher::Boris).unwrap();
    assert!(p.absorb(false, 4).is_err());
    // A field beyond physics drops the ring rather than poisoning the grid.
    pic.set(2, |_, _| f64::INFINITY);
    assert_eq!(pic.step().unwrap().lost, 1);
}

/// The slow-wave structure of the BWO tests: a tube 10 mm inside its teeth, grooves 5 mm wide to
/// 13 mm, period 10 mm (grooves at the period's first half).
const TEETH: f64 = 0.010;
const GROOVES: f64 = 0.013;
const PERIOD: f64 = 0.010;

fn corrugated(r: f64, s: f64) -> bool {
    r > if (s.rem_euclid(1.0) - 0.25).abs() < 0.25 { GROOVES } else { TEETH }
}

/// A TM01 mode of the corrugated tube: its Bloch phase per period (rad), frequency (Hz), Pierce
/// impedance of its backward harmonic k = (2π − ψ)/d at each asked radius (Ω), group velocity.
struct ColdMode {
    psi: f64,
    f: f64,
    impedance: Vec<f64>,
    vg: f64,
}

/// The structure's cold TM01 band on a periodic stretch of `periods` (cells of `h`), at the
/// Bloch phases 2πm/periods for `modes`: E_z = J₀(x₀₁r/a) cos(kz) let ring, its strongest line
/// over 8–15 GHz found, then every component's phasor there accumulated; the stored energy
/// W = ¼∫(ε₀|Ê|² + |B̂|²/μ₀) dV, half of it each of the two waves ±ψ; the harmonic's amplitude E
/// the spatial transform of Ê_z along z; v_g = dω/dk from the neighbouring phases; then
/// K = E²/(2k²P), P = v_g (W/2)/L (Pierce 1950, ch. II).
fn cold_band(h: f64, periods: usize, modes: std::ops::RangeInclusive<usize>, radii: &[f64]) -> Vec<ColdMode> {
    use emcraft_model::units::{EPS0, MU0};
    use emcraft_theory::bessel::j;
    use num_complex::Complex64 as Z;
    let lz = periods as f64 * PERIOD;
    let nz = (lz / h).round() as usize;
    let g = AxiGrid::new((GROOVES / h).round() as usize, nz, h, lz / nz as f64, Wall::Periodic).unwrap();
    let dt = g.time_step(0.95);
    let x01 = 2.404_825_557_695_773;
    let zero = Z::new(0.0, 0.0);
    let mut out: Vec<(f64, f64, Vec<f64>, f64)> = Vec::new();
    for m in modes {
        let k = 2.0 * PI * m as f64 / lz;
        let start = || {
            let mut pic = AxiPic::new(g, dt, Pusher::Boris).unwrap();
            pic.set(2, |r, z| if r < TEETH { j(0, x01 * r / TEETH) * (k * z).cos() } else { 0.0 });
            pic.set_metal(|r, z| corrugated(r, z / PERIOD));
            pic
        };
        let mut pic = start();
        let freqs: Vec<f64> = (0..=700).map(|i| 8e9 + 0.01e9 * i as f64).collect();
        let mut sums = vec![zero; freqs.len()];
        for _ in 0..(30e-9 / dt) as usize {
            pic.step().unwrap();
            let (e, t) = (pic.field_at(0.009, 0.3 * PERIOD).0[2], pic.time());
            for (s, f) in sums.iter_mut().zip(&freqs) {
                *s += Z::from_polar(e, -2.0 * PI * f * t);
            }
        }
        let f = sums.iter().zip(&freqs).fold((0.0, 0.0), |a, (s, f)| if s.norm() > a.0 { (s.norm(), *f) } else { a }).1;
        let mut pic = start();
        let mut ph: [Vec<Z>; 6] = std::array::from_fn(|w| vec![zero; pic.lats[w].len()]);
        let steps = (40e-9 / dt) as usize;
        for _ in 0..steps {
            pic.step().unwrap();
            let rot = Z::from_polar(2.0 / steps as f64, -2.0 * PI * f * pic.time());
            let fields = [&pic.f.er, &pic.f.ez, &pic.f.et, &pic.f.br, &pic.f.bz, &pic.f.bt];
            for (p, v) in ph.iter_mut().zip(fields) {
                for (a, b) in p.iter_mut().zip(v.iter()) {
                    *a += rot * b;
                }
            }
        }
        let mut w = 0.0;
        for (which, p) in ph.iter().enumerate() {
            let lat = pic.lats[which];
            let coefficient = if which < 3 { 0.25 * EPS0 } else { 0.25 / MU0 };
            for kk in 0..lat.nz {
                for i in 0..lat.nr {
                    let r = (i as f64 + if lat.r_node { 0.0 } else { 0.5 }) * h;
                    let ring = if lat.r_node && i == 0 { PI * 0.25 * h * h } else { 2.0 * PI * r * h };
                    w += coefficient * p[lat.at(i, kk)].norm_sqr() * ring * g.dz;
                }
            }
        }
        let kb = 2.0 * PI / PERIOD - k;
        let lez = pic.lats[EZ];
        let amplitudes = radii
            .iter()
            .map(|rb| {
                let i = (rb / h).round() as usize;
                (0..lez.nz).map(|kk| ph[EZ][lez.at(i, kk)] * Z::from_polar(g.dz / lz, -kb * (kk as f64 + 0.5) * g.dz)).sum::<Z>().norm()
            })
            .collect();
        out.push((k * PERIOD, f, amplitudes, w));
    }
    let n = out.len();
    (0..n)
        .map(|m| {
            let (psi, f, ref a, w) = out[m];
            let (lo, hi) = (m.saturating_sub(1), (m + 1).min(n - 1));
            let vg = 2.0 * PI * (out[hi].1 - out[lo].1) / ((out[hi].0 - out[lo].0) / PERIOD);
            let kb = (2.0 * PI - psi) / PERIOD;
            let p = vg.abs() * 0.5 * w / lz;
            ColdMode { psi, f, impedance: a.iter().map(|a| a * a / (2.0 * kb * kb * p)).collect(), vg }
        })
        .collect()
}

/// A BWO in a body of revolution: `periods` of the corrugated tube between 20 mm of smooth tube
/// at the grooves' radius each end (matched: far above its cut-off) and a 10-cell absorber; a
/// ring beam (7.5–8.5 mm) of `current` A at 100 kV held by 2 T through the cathode end. Each ns,
/// the strongest line of E_z over 10–15 GHz near the structure's cathode end (the window's mean,
/// the beam's own static field, taken out) and its frequency.
fn bwo(periods: usize, current: f64, time: f64, h: f64) -> Vec<(f64, f64)> {
    use num_complex::Complex64 as Z;
    let (layer, buffer) = (10usize, 0.02f64);
    let cells = periods as f64;
    let lz = cells * PERIOD + 2.0 * (layer as f64 * h + buffer);
    let nz = (lz / h).round() as usize;
    let g = AxiGrid::new((GROOVES / h).round() as usize, nz, h, lz / nz as f64, Wall::Conductor).unwrap();
    let dt = g.time_step(0.95);
    let mut pic = AxiPic::new(g, dt, Pusher::Boris).unwrap();
    let z0 = layer as f64 * h + buffer;
    pic.set_metal(|r, z| {
        let s = (z - z0) / PERIOD;
        if s > 0.0 && s < cells { corrugated(r, s) } else { r > GROOVES }
    });
    pic.absorb(false, layer).unwrap();
    pic.absorb(true, layer).unwrap();
    pic.set(5, |_, _| 2.0);
    let k = pic.add(Rings { species: ELECTRON, rings: Vec::new() }).unwrap();
    let pulse = Pulse::Steady { current, rise: 1e-9 };
    pic.inject(Injection { high: false, population: k, pulse, inner: 0.0075, radius: 0.0085, energy: 1e5 * -ELECTRON.charge, per_step: 8 }).unwrap();
    let (pr, pz) = (0.009, z0 + 0.25 * PERIOD);
    let window = (1e-9 / dt) as usize;
    let freqs: Vec<f64> = (0..=100).map(|i| 10e9 + 0.05e9 * i as f64).collect();
    let zero = Z::new(0.0, 0.0);
    let (mut sums, mut basis, mut mean) = (vec![zero; freqs.len()], vec![zero; freqs.len()], 0.0);
    let mut out = Vec::new();
    for n in 0..(time / dt) as usize {
        pic.step().unwrap();
        let (e, t) = (pic.field_at(pr, pz).0[2], pic.time());
        mean += e;
        for ((s, b), f) in sums.iter_mut().zip(basis.iter_mut()).zip(&freqs) {
            let w = Z::from_polar(1.0, -2.0 * PI * f * t);
            *s += w * e;
            *b += w;
        }
        if (n + 1) % window == 0 {
            let m = mean / window as f64;
            let lines = sums.iter().zip(&basis).zip(&freqs).map(|((s, b), f)| ((s - b * m).norm() / window as f64, *f));
            out.push(lines.fold((0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a }));
            sums.iter_mut().chain(basis.iter_mut()).for_each(|s| *s = zero);
            mean = 0.0;
        }
    }
    out
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn a_backward_wave_oscillator_starts_near_johnsons_current() {
    // Johnson (Proc. IRE 43:684, 1955) with Pierce's space charge (Branch & Mihran's reduction for
    // a thin ring) and the relativistic C³ (Levush et al. 1992): the tube's own cold band and
    // impedance on this grid give the synchronism of the 100 kV beam with the backward harmonic
    // and the start current of 24 periods. Below it (0.6 of it) nothing grows in 100 ns; at 2.5
    // times it oscillates at Johnson's frequency, the band where v_p = v₀/(1 + bC). The measured
    // start lies at 1.4–1.9 times Johnson's here (9–12 A against 6.3 A; 12 periods ≈ 100 A
    // against 65 A, the L⁻³ scaling kept to 15 %): the ends still reflect, which moves it
    // (Levush et al.; with the drift tubes at the teeth's radius, near their cut-off, the start
    // fell to 5 A).
    use emcraft_theory::bwo::{Oscillator, Ring, johnson_start};
    let h = 0.5e-3;
    let band = cold_band(h, 18, 4..=6, &[0.0075, 0.008, 0.0085]);
    let gamma0 = 1.0 + 1e5 / ELECTRON.rest_energy() * -ELECTRON.charge;
    let v0 = C0 * (1.0 - 1.0 / (gamma0 * gamma0)).sqrt();
    // Where the band f(ψ) meets the beam line f = v₀(2π − ψ)/(2πd) (shifted by 1 + bC), linearly
    // between the cold phases; the impedance averaged over the ring's area (Simpson, r-weighted).
    let meet = |shift: f64| -> (f64, f64) {
        let line = |psi: f64| v0 / shift * (2.0 * PI - psi) / (2.0 * PI * PERIOD);
        let pair = band.windows(2).find(|w| (w[0].f - line(w[0].psi)) * (w[1].f - line(w[1].psi)) <= 0.0).unwrap();
        let (a, b) = (&pair[0], &pair[1]);
        let t = (line(a.psi) - a.f) / ((b.f - a.f) - (line(b.psi) - line(a.psi)));
        let mean = |m: &ColdMode| (7.5 * m.impedance[0] + 32.0 * m.impedance[1] + 8.5 * m.impedance[2]) / 48.0;
        (a.f + t * (b.f - a.f), (mean(a).ln() + t * (mean(b).ln() - mean(a).ln())).exp())
    };
    let (f_sync, impedance) = meet(1.0);
    let ring = Ring { radius: 0.008, thickness: 0.001, tube: TEETH };
    let o =
        Oscillator { impedance, length: 24.0 * PERIOD, frequency: f_sync, gamma0, charge: ELECTRON.charge, mass: ELECTRON.mass, ring: Some(ring) };
    let start = o.start_current().unwrap();
    let start_c = (start * -ELECTRON.charge * impedance / (2.0 * gamma0.powi(3) * ELECTRON.mass * v0 * v0)).cbrt();
    let b = johnson_start(o.space_charge(start).unwrap()).unwrap().b;
    let (f_johnson, _) = meet(1.0 + b * start_c);
    assert!(band.iter().all(|m| m.vg > 0.0) && (5.0..8.0).contains(&start), "start {start} A, K {impedance} Ω, f {f_sync}");
    let [below, above] = std::thread::scope(|s| [0.6, 2.5].map(|x| s.spawn(move || bwo(24, x * start, 100e-9, h))).map(|t| t.join().unwrap()));
    // The strongest line over a stretch of ns, and its frequency.
    let peak = |r: &[(f64, f64)], from: usize, to: usize| r[from..to].iter().copied().fold((0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a });
    let noise = peak(&below, 10, 30).0;
    assert!(peak(&below, 80, 100).0 < 3.0 * noise, "below the start: {below:?}");
    let (top, f) = peak(&above, 0, 100);
    assert!(top > 100.0 * noise, "above the start: {above:?}");
    assert!((f / f_johnson - 1.0).abs() < 0.01, "{} GHz against Johnson's {} GHz", f * 1e-9, f_johnson * 1e-9);
    println!(
        "start {start:.2} A (K {impedance:.3} Ω, sync {:.3} GHz), oscillating at {:.3} GHz against Johnson's {:.3} GHz, {top:.2e} V/m over noise {noise:.2e}",
        f_sync * 1e-9,
        f * 1e-9,
        f_johnson * 1e-9
    );
}

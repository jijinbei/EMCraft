//! The particle-in-cell loop against what is known exactly: the Yee grid's own dispersion in
//! vacuum, charge conservation to rounding, a cold plasma's oscillation at ω_p and a planar
//! diode's space-charge-limited current (Child–Langmuir, at any speed: Jory & Trivelpiece).

use std::f64::consts::PI;

use emcraft_model::units::C0;
use emcraft_particles::{ELECTRON, Pusher, Species};
use emcraft_theory::plasma::{child_langmuir_relativistic, plasma_frequency};

use super::*;

fn axis(cells: usize, h: f64, wall: Wall) -> Axis {
    Axis { cells, h, wall }
}

/// A deterministic sequence in [0, 1) (a linear congruential generator's top bits).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[test]
fn a_standing_wave_in_vacuum_rings_at_the_yee_grids_frequency() {
    // E_z = sin(kx) between periodic faces, no particles: Eⁿ = cos(nθ) sin(kx) exactly, with
    // sin(θ/2) = (cΔt/h) sin(kh/2), the leapfrog's dispersion (Taflove & Hagness 2005, §4.6).
    let (n, h) = (16, 1e-3);
    let g = Grid::new([axis(n, h, Wall::Periodic), axis(1, h, Wall::Periodic), axis(1, h, Wall::Periodic)]).unwrap();
    let dt = g.time_step(0.9);
    let mut pic = Pic::new(g, dt, Pusher::Boris).unwrap();
    let k = 2.0 * PI / (n as f64 * h);
    pic.set_e(2, |x| (k * x[0]).sin());
    let theta = 2.0 * (C0 * dt / h * (k * h / 2.0).sin()).asin();
    for _ in 0..300 {
        pic.step().unwrap();
    }
    for i in 0..n {
        let x = [i as f64 * h, 0.0, 0.5 * h];
        let want = (300.0 * theta).cos() * (k * x[0]).sin();
        assert!((pic.e_at(x)[2] - want).abs() < 1e-10, "node {i}: {} against {want}", pic.e_at(x)[2]);
    }
}

#[rstest::rstest]
#[case::raw(0)]
#[case::smoothed(2)]
fn charge_is_conserved_at_every_node_to_rounding(#[case] smoothing: usize) {
    // Electrons and positrons in all directions up to γ ≈ 3 in a static B, through periodic
    // faces along x and z and onto conducting walls along y: ε₀∇·E − ρ stays where it started.
    let g = Grid::new([axis(5, 1e-3, Wall::Periodic), axis(4, 1.3e-3, Wall::Conductor), axis(6, 0.8e-3, Wall::Periodic)]).unwrap();
    let mut pic = Pic::new(g, g.time_step(0.95), Pusher::Vay).unwrap();
    let positron = Species { charge: -ELECTRON.charge, mass: ELECTRON.mass };
    let mut r = Lcg(7);
    let lengths = g.lengths();
    for species in [ELECTRON, positron] {
        let particles =
            (0..200).map(|_| Particle { x: lengths.map(|l| l * r.next()), u: [(); 3].map(|()| 3.0 * C0 * (2.0 * r.next() - 1.0)), w: 1e6 }).collect();
        pic.add(Population { species, particles }).unwrap();
    }
    pic.set_b(2, |_| 0.1);
    pic.smooth(smoothing);
    let before = pic.gauss();
    let scale = before.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let mut collected = 0.0;
    for _ in 0..200 {
        let moved = pic.step().unwrap();
        collected += moved.collected[1].iter().map(|q| q.abs()).sum::<f64>();
    }
    let after = pic.gauss();
    let drift = before.iter().zip(&after).fold(0.0f64, |m, (a, b)| m.max((a - b).abs()));
    assert!(collected > 0.0 && pic.particles() < 400, "some reach the walls");
    assert!(drift < 1e-11 * scale, "Gauss's law moved by {drift:e} of {scale:e}");
}

#[test]
fn a_cold_plasma_oscillates_at_the_plasma_frequency() {
    // Electrons on a neutralising background, their velocities perturbed as sin(kx): E_x rings at
    // ω_p (Birdsall & Langdon 1991, §4-2), whatever k — here ω_pΔt = 0.05, kh = 2π/32.
    let density = 1e16;
    let wp = plasma_frequency(density, ELECTRON.charge, ELECTRON.mass);
    let dt = 0.05 / wp;
    let h = 1.05 * C0 * dt * 3f64.sqrt();
    let n = 32;
    let g = Grid::new([axis(n, h, Wall::Periodic), axis(1, h, Wall::Periodic), axis(1, h, Wall::Periodic)]).unwrap();
    let mut pic = Pic::new(g, dt, Pusher::Boris).unwrap();
    let k = 2.0 * PI / g.lengths()[0];
    let pop = Population::uniform(&g, ELECTRON, density, [16, 1, 1], |x| [1e4 * (k * x[0]).sin(), 0.0, 0.0]).unwrap();
    pic.add(pop).unwrap();
    pic.neutralise();
    let probe = [0.25 * g.lengths()[0], 0.0, 0.0];
    let mut crossings = Vec::new();
    let mut last = pic.e_at(probe)[0];
    for s in 1..=1300 {
        pic.step().unwrap();
        let now = pic.e_at(probe)[0];
        if last != 0.0 && now.signum() != last.signum() {
            crossings.push((s as f64 - 1.0 + last / (last - now)) * dt);
        }
        last = now;
    }
    let (first, end) = (crossings.first().copied().unwrap_or(0.0), crossings.last().copied().unwrap_or(0.0));
    let w = PI * (crossings.len() - 1) as f64 / (end - first);
    assert!(crossings.len() >= 15);
    assert!((w / wp - 1.0).abs() < 0.01, "ω = {w:e} against ω_p = {wp:e}");
    // The grid's own: charge assigned and the field gathered with the first-order shape, each
    // sinc²(kh/2) (Birdsall & Langdon 1991, §8-6), Gauss's difference k → (2/h) sin(kh/2), and
    // the leapfrog's sin(ωΔt/2) = ω_pΔt/2: 0.23 % low.
    let x = k * h / 2.0;
    let sinc = x.sin() / x;
    let grid = wp * sinc * sinc * (x / x.sin()).sqrt() * (2.0 / (wp * dt) * (0.5 * wp * dt).asin());
    assert!((w / grid - 1.0).abs() < 2e-4, "ω = {w:e} against the grid's {grid:e}");
}

/// The current density (A/m²) a planar diode of `n` cells draws at the anode, averaged over the
/// last `tail` of `steps` steps: 1 cm, 100 kV held by the outer circuit, a cathode at z = 0
/// emitting electrons at rest as the field allows.
fn diode(n: usize, steps: usize, tail: usize) -> f64 {
    let (v, d) = (1e5, 1e-2);
    let h = d / n as f64;
    let g = Grid::new([axis(1, h, Wall::Periodic), axis(1, h, Wall::Periodic), axis(n, h, Wall::Conductor)]).unwrap();
    let dt = g.time_step(0.95);
    let mut pic = Pic::new(g, dt, Pusher::Boris).unwrap();
    pic.set_e(2, |_| -v / d);
    pic.hold(2).unwrap();
    let electrons = pic.add(Population::new(ELECTRON)).unwrap();
    pic.emit(Emitter { axis: 2, high: false, population: electrons, emission: Emission::Limited, energy: 0.0, per_node: 4 }).unwrap();
    let mut anode = 0.0;
    for s in 0..steps {
        let moved = pic.step().unwrap();
        if s >= steps - tail {
            anode += moved.collected[2][1];
        }
    }
    assert!((pic.line_integral(2) + v).abs() < 1e-9 * v, "the voltage held");
    anode.abs() / (h * h * tail as f64 * dt)
}

#[test]
fn a_planar_diode_draws_the_space_charge_limited_current() {
    // The current settles at Jory & Trivelpiece's relativistic Child–Langmuir current (0.98 of
    // Child's at 100 kV across 1 cm), approached as h^{2/3}: the density near a cathode
    // emitting at rest grows as z^{−2/3}. At 40 and 80 cells it is 5.9 % and 3.7 % high, and
    // the two extrapolated to h → 0 at that order are within 0.1 %.
    let want = child_langmuir_relativistic(1e5, 1e-2, ELECTRON.charge, ELECTRON.mass).unwrap();
    let (coarse, fine) = (diode(40, 2000, 500) / want, diode(80, 3500, 1000) / want);
    let r = 2f64.powf(2.0 / 3.0);
    let limit = (r * fine - coarse) / (r - 1.0);
    assert!(coarse > fine && fine > 1.0 && fine < 1.05, "{coarse} then {fine} of the limited current");
    assert!((limit - 1.0).abs() < 1e-3, "extrapolated to {limit}");
}

#[test]
fn hostile_settings_are_refused_and_nothing_panics() {
    let a = axis(4, 1e-3, Wall::Periodic);
    let z = axis(4, 1e-3, Wall::Conductor);
    assert!(Grid::new([a, a, axis(4, f64::NAN, Wall::Periodic)]).is_err());
    assert!(Grid::new([axis(100_000, 1e-3, Wall::Periodic), axis(100_000, 1e-3, Wall::Periodic), a]).is_err());
    let g = Grid::new([a, a, z]).unwrap();
    for dt in [0.0, -1.0, f64::NAN, f64::INFINITY, g.time_step(1.5)] {
        assert!(Pic::new(g, dt, Pusher::Boris).is_err(), "{dt}");
    }
    let mut pic = Pic::new(g, g.time_step(0.9), Pusher::Boris).unwrap();
    let one = |x: V3, u: V3, w: f64| Population { species: ELECTRON, particles: vec![Particle { x, u, w }] };
    assert!(pic.add(one([0.0, 0.0, 5e-3], [0.0; 3], 1.0)).is_err(), "outside the walls");
    assert!(pic.add(one([0.0, 0.0, 1e-3], [f64::NAN, 0.0, 0.0], 1.0)).is_err());
    assert!(pic.add(one([0.0, 0.0, 1e-3], [0.0; 3], -1.0)).is_err());
    assert!(Population::uniform(&g, ELECTRON, f64::NAN, [1, 1, 1], |_| [0.0; 3]).is_err());
    assert!(Population::uniform(&g, ELECTRON, 1.0, [0, 1, 1], |_| [0.0; 3]).is_err());
    assert!(Population::uniform(&g, ELECTRON, 1.0, [100_000, 100_000, 1], |_| [0.0; 3]).is_err());
    // Periodic positions come back into the box.
    let i = pic.add(one([-1e-3, 9e-3, 1e-3], [0.0; 3], 1.0)).unwrap();
    assert!((pic.populations()[i].particles[0].x[0] - 3e-3).abs() < 1e-15);
    let base = Emitter { axis: 2, high: true, population: i, emission: Emission::Limited, energy: 0.0, per_node: 1 };
    for bad in [
        Emitter { axis: 0, ..base },
        Emitter { axis: 7, ..base },
        Emitter { population: 9, ..base },
        Emitter { energy: f64::NAN, ..base },
        Emitter { per_node: 0, ..base },
        Emitter { per_node: MAX_PER_NODE + 1, ..base },
        Emitter { emission: Emission::Current(-1.0), ..base },
    ] {
        assert!(pic.emit(bad).is_err(), "{bad:?}");
    }
    assert!(pic.hold(0).is_err() && pic.hold(5).is_err());
    // A field beyond any physics drops the particle, said, rather than poisoning the grid.
    pic.set_e(2, |_| f64::INFINITY);
    let moved = pic.step().unwrap();
    assert_eq!((moved.lost, pic.particles()), (1, 0));
}

#[test]
fn a_bunch_through_a_pillbox_leaves_its_loss_factor() {
    // A 1 nC bunch, σ_z = 2 mm, at 10 GeV along the axis of a perfectly conducting pillbox (10 mm
    // by 13.06 mm, a staircase of 0.5 mm cells) entering through one end wall and leaving through
    // the other: the energy left in the cavity is q² Σ k_n e^{−(ω_n σ_t)²} over its TM0np modes
    // (Wilson 1989; emcraft_theory::wake), 5.81 V/pC — within 0.2 % at this cell, 0.07 % at 0.25 mm.
    let (a, l, sz) = (0.01, 0.01306, 2e-3);
    let h = 0.5e-3;
    let across = |len: f64| axis((len / h).round() as usize, len / (len / h).round(), Wall::Conductor);
    let g = Grid::new([across(2.0 * a), across(2.0 * a), across(l)]).unwrap();
    let dt = g.time_step(0.95);
    let mut pic = Pic::new(g, dt, Pusher::Boris).unwrap();
    assert!(pic.set_metal(|x| (x[0] - a).powi(2) + (x[1] - a).powi(2) > a * a));
    let k = pic.add(Population::new(ELECTRON)).unwrap();
    let sigma = sz / C0;
    let bunch = Bunch {
        axis: 2,
        high: false,
        population: k,
        pulse: Pulse::Gaussian { charge: 1e-9, t0: 5.0 * sigma, sigma },
        centre: [a, a],
        radius: 0.0,
        inner: 0.0,
        energy: 1e10 * -ELECTRON.charge,
        per_step: 8,
    };
    pic.inject(bunch).unwrap();
    let before = pic.gauss();
    let steps = ((10.0 * sigma + l / C0) / dt).ceil() as usize + 10;
    let mut q = 0.0;
    for _ in 0..steps {
        q += pic.step().unwrap().emitted;
    }
    assert_eq!(pic.particles(), 0, "the bunch has gone");
    assert!((q / -1e-9 - 1.0).abs() < 1e-6, "{q} C sent");
    let k = pic.energy().field / (q * q);
    let want = emcraft_theory::wake::Pillbox::new(a, l).loss_factor(1.0, sigma).unwrap();
    assert!((k / want - 1.0).abs() < 4e-3, "{k:e} V/C against {want:e}");
    // Gauss's law held off the metal while the bunch went through.
    let drift = before.iter().zip(pic.gauss()).fold(0.0f64, |m, (x, y)| m.max((x - y).abs()));
    assert!(drift < 1e-12 * 1e-9 / g.cell_volume(), "{drift:e}");
    // A bunch through a periodic face, or with no length, is refused.
    let bad = Bunch { pulse: Pulse::Gaussian { charge: 1e-9, t0: 0.0, sigma: 0.0 }, ..bunch };
    assert!(pic.inject(bad).is_err() && pic.inject(Bunch { axis: 9, ..bunch }).is_err());
}

#[test]
fn a_bunch_through_a_dielectric_pillbox_leaves_the_dielectric_modes_loss_factor() {
    // The pillbox filled with εr = 2.25: its modes at c/1.5, their stored energy with ε₀εr, the
    // bunch at c above the Cherenkov speed (Wangler §11.6; emcraft_theory::wake::Pillbox) —
    // 9.41 V/pC against 5.81 empty; 0.9 % high at 0.5 mm cells, 0.25 % at 0.25 mm. Gauss's law
    // holds as ∇·(εr E) off the metal.
    let (a, l, eps) = (0.01, 0.01306, 2.25);
    let h = 0.5e-3;
    let across = |len: f64| axis((len / h).round() as usize, len / (len / h).round(), Wall::Conductor);
    let g = Grid::new([across(2.0 * a), across(2.0 * a), across(l)]).unwrap();
    let dt = g.time_step(0.95);
    let mut pic = Pic::new(g, dt, Pusher::Boris).unwrap();
    assert!(pic.set_media(|x| if (x[0] - a).powi(2) + (x[1] - a).powi(2) > a * a { Medium::Metal } else { Medium::Dielectric(eps) }));
    let k = pic.add(Population::new(ELECTRON)).unwrap();
    let sigma = 2e-3 / C0;
    pic.inject(Bunch {
        axis: 2,
        high: false,
        population: k,
        pulse: Pulse::Gaussian { charge: 1e-12, t0: 5.0 * sigma, sigma },
        centre: [a, a],
        radius: 0.0,
        inner: 0.0,
        energy: 1e10 * -ELECTRON.charge,
        per_step: 8,
    })
    .unwrap();
    let before = pic.gauss();
    let mut q = 0.0;
    for _ in 0..((10.0 * sigma + l / C0) / dt).ceil() as usize + 10 {
        q += pic.step().unwrap().emitted;
    }
    let k = pic.energy().field / (q * q);
    let want = emcraft_theory::wake::Pillbox { radius: a, length: l, eps }.loss_factor(1.0, sigma).unwrap();
    assert!((k / want - 1.0).abs() < 0.015, "{k:e} V/C against {want:e}");
    let drift = before.iter().zip(pic.gauss()).fold(0.0f64, |m, (x, y)| m.max((x - y).abs()));
    assert!(drift < 1e-12 * 1e-12 / g.cell_volume(), "{drift:e}");
}

/// A ring beam of `current` (A) at γ₀ = 1.25 through a grounded drift tube 5 mm in radius (the
/// ring at 3 mm, a cell thick) and 30 mm long in a 10 T axial field, 0.5 mm cells: the mean γ
/// in its middle third and the current reaching the far end over the run's second half (A).
fn drift_tube(current: f64) -> (f64, f64) {
    let (r, rb, l, h) = (5e-3, 3e-3, 30e-3, 0.5e-3);
    let across = |len: f64| axis((len / h).round() as usize, len / (len / h).round(), Wall::Conductor);
    let g = Grid::new([across(2.0 * r), across(2.0 * r), across(l)]).unwrap();
    let dt = g.time_step(0.95);
    let mut pic = Pic::new(g, dt, Pusher::Boris).unwrap();
    pic.set_metal(|x| (x[0] - r).powi(2) + (x[1] - r).powi(2) > r * r);
    pic.set_b(2, |_| 10.0);
    let k = pic.add(Population::new(ELECTRON)).unwrap();
    let pulse = Pulse::Steady { current, rise: 0.1e-9 };
    let energy = 0.25 * ELECTRON.rest_energy();
    pic.inject(Bunch { axis: 2, high: false, population: k, pulse, centre: [r, r], radius: rb + 0.5 * h, inner: rb - 0.5 * h, energy, per_step: 16 })
        .unwrap();
    let steps = (0.8e-9 / dt) as usize;
    let mut out = 0.0;
    for n in 0..steps {
        let m = pic.step().unwrap();
        if n >= steps / 2 {
            out += m.collected[2][1];
        }
    }
    let mid: Vec<f64> =
        pic.populations()[k].particles.iter().filter(|p| p.x[2] > l / 3.0 && p.x[2] < 2.0 * l / 3.0).map(|p| emcraft_particles::gamma(p.u)).collect();
    (mid.iter().sum::<f64>() / mid.len().max(1) as f64, out.abs() / ((steps - steps / 2) as f64 * dt))
}

#[test]
fn a_ring_beam_in_a_drift_tube_sinks_by_its_space_charge() {
    // Bogdankevich & Rukhadze (Sov. Phys. Usp. 14:163, 1971; Miller 1982, §3.2): the beam slows
    // to γ_b with I = I₀(γ₀ − γ_b)β_b/(2 ln(R/r_b)); at 0.7 of the limiting current
    // I₀(γ₀^{2/3} − 1)^{3/2}/(2 ln(R/r_b)) = 1.07 kA it all passes and γ_b = 1.1616 (1.3 % off
    // here; above the limit, oracle pic-limiting-current). At γ₀ = 1.25 the beam (β = 0.6) is slower than the
    // grid's shortest waves: no numerical Cherenkov drag.
    let beam = emcraft_theory::plasma::AnnularBeam { gamma0: 1.25, ratio: 5.0 / 3.0, charge: ELECTRON.charge, mass: ELECTRON.mass };
    let limit = beam.limiting().unwrap();
    let want = beam.depressed(0.7 * limit).unwrap();
    let (g, passed) = drift_tube(0.7 * limit);
    assert!(((1.25 - g) / (1.25 - want) - 1.0).abs() < 0.05, "γ_b {g} against {want}");
    assert!((passed / (0.7 * limit) - 1.0).abs() < 0.03, "{passed} A of {}", 0.7 * limit);
}

/// The growth rate (1/s) of the fundamental of E_x along a periodic box one wavelength 2π/k long,
/// 32 cells, `per` macro-particles a cell for each population of (density m⁻³, velocity m/s,
/// seed): fitted as `growth_rate` fits |E₁|², from e⁻⁶ to e⁻² of its peak amplitude.
fn growth(k: f64, streams: &[(f64, f64, f64)], per: usize, time: f64) -> f64 {
    let cells = 32;
    let h = 2.0 * PI / k / cells as f64;
    let g = Grid::new([axis(cells, h, Wall::Periodic), axis(1, 10.0 * h, Wall::Periodic), axis(1, 10.0 * h, Wall::Periodic)]).unwrap();
    let dt = g.time_step(0.95);
    let mut pic = Pic::new(g, dt, Pusher::Boris).unwrap();
    for &(n, v, seed) in streams {
        pic.add(Population::uniform(&g, ELECTRON, n, [per, 1, 1], |x| [v + seed * (k * x[0]).sin(), 0.0, 0.0]).unwrap()).unwrap();
    }
    pic.neutralise();
    let (mut t, mut w) = (Vec::new(), Vec::new());
    for _ in 0..(time / dt) as usize {
        pic.step().unwrap();
        let (c, s) = (0..cells).fold((0.0, 0.0), |(c, s), i| {
            let x = (i as f64 + 0.5) * h;
            let e = pic.e_at([x, 0.0, 0.0])[0];
            (c + e * (k * x).cos(), s + e * (k * x).sin())
        });
        t.push(pic.time());
        w.push(c * c + s * s);
    }
    growth_rate(&t, &w).unwrap_or(0.0)
}

#[test]
fn two_streams_grow_at_the_cold_dispersions_rate() {
    // Two cold streams at ±v₀ grow fastest, at ω_b/2 where k v₀ = (√3/2) ω_b; a weak beam (1 %
    // of the plasma's density) through a plasma at rest at k v₀ = ω_p grows at
    // ≈ (√3/2)(n_b/2n_p)^{1/3} ω_p, the cube root of Pierce's gain — each against the largest
    // imaginary root of 1 = Σ ω_s²/(ω − k v_s)² (Birdsall & Langdon §5-6; emcraft_theory::plasma).
    // β = 0.1 (its relativistic shift below 1 %): 2.7 % and 0.5 % here.
    let (n, v0) = (1e16, 3e7);
    let wb = plasma_frequency(n, ELECTRON.charge, ELECTRON.mass);
    let k = 3f64.sqrt() / 2.0 * wb / v0;
    let want = emcraft_theory::plasma::two_stream_growth(wb, v0, wb, -v0, k).unwrap();
    let got = growth(k, &[(n, v0, 1e-3 * v0), (n, -v0, 0.0)], 16, 16.0 / want);
    assert!((got / want - 1.0).abs() < 0.04, "{got:e} against {want:e}");
    // The weak beam at k v_b = ω_p.
    let vb = wb / k;
    let weak = plasma_frequency(0.01 * n, ELECTRON.charge, ELECTRON.mass);
    let want = emcraft_theory::plasma::two_stream_growth(wb, 0.0, weak, vb, k).unwrap();
    let got = growth(k, &[(n, 0.0, 0.0), (0.01 * n, vb, 1e-4 * vb)], 16, 22.0 / want);
    assert!((got / want - 1.0).abs() < 0.03, "{got:e} against {want:e}");
}

#[test]
fn an_open_box_lets_a_pulse_out() {
    // A divergence-free pulse (E the grid's own curl of a Gaussian, so no charge is implied) at
    // the centre of a box whose six faces are open (CPML, Roden & Gedney 2000): once it has
    // crossed the box, 1e-8 of its energy is left; with the walls closed, all of it.
    let (n, h, layer) = (32, 1e-3, 8);
    let left = |open: bool| {
        let g = Grid::new([axis(n, h, Wall::Conductor), axis(n, h, Wall::Conductor), axis(n, h, Wall::Conductor)]).unwrap();
        let dt = g.time_step(0.95);
        let mut pic = Pic::new(g, dt, Pusher::Boris).unwrap();
        for a in 0..3 {
            for high in [false, true] {
                if open {
                    pic.absorb(a, high, layer).unwrap();
                }
            }
        }
        let (c, w) = (0.5 * n as f64 * h, 2.5 * h);
        let pot = |x: f64, y: f64, z: f64| (-((x - c).powi(2) + (y - c).powi(2) + (z - c).powi(2)) / (w * w)).exp();
        pic.set_e(0, |x| (pot(x[0], x[1] + 0.5 * h, x[2]) - pot(x[0], x[1] - 0.5 * h, x[2])) / h);
        pic.set_e(1, |x| -(pot(x[0] + 0.5 * h, x[1], x[2]) - pot(x[0] - 0.5 * h, x[1], x[2])) / h);
        let e0 = pic.energy().field;
        for _ in 0..((2.5 * n as f64 * h / C0) / dt) as usize {
            pic.step().unwrap();
        }
        pic.energy().field / e0
    };
    let (open, closed) = (left(true), left(false));
    assert!(open < 1e-6 && closed > 0.97, "{open:e} left open, {closed} closed");
    // A layer as thick as the axis is refused.
    let g = Grid::new([axis(8, h, Wall::Conductor), axis(8, h, Wall::Periodic), axis(8, h, Wall::Conductor)]).unwrap();
    let mut pic = Pic::new(g, g.time_step(0.9), Pusher::Boris).unwrap();
    assert!(pic.absorb(0, false, 8).is_err() && pic.absorb(1, false, 2).is_err() && pic.absorb(7, true, 2).is_err());
    assert!(pic.absorb(0, false, 4).is_ok() && pic.absorb(0, true, 4).is_err());
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn a_guide_ending_in_absorbers_lets_its_waves_out() {
    // TM01 waves (Pozar §3.4: E_z ∝ J₀(x₀₁r/a)) of a circular guide 10 mm in radius (1 mm cells), above cut-off
    // (kz 500/m ± 100), launched both ways from its middle into CPML at both ends: 2 ns later
    // 1e-6 of their electric energy is left (the B frozen in the staircase's faces by the
    // perfect conductor counts none of it).
    use emcraft_theory::bessel::j;
    let (a, l, h, layer) = (0.01, 0.16, 1e-3, 12);
    let across = |len: f64| axis((len / h).round() as usize, len / (len / h).round(), Wall::Conductor);
    let g = Grid::new([across(2.0 * a), across(2.0 * a), across(l)]).unwrap();
    let dt = g.time_step(0.95);
    let mut pic = Pic::new(g, dt, Pusher::Boris).unwrap();
    pic.set_metal(|x| (x[0] - a).powi(2) + (x[1] - a).powi(2) > a * a);
    pic.absorb(2, false, layer).unwrap();
    pic.absorb(2, true, layer).unwrap();
    let x01 = 2.404_825_557_695_773;
    let pot = |x: f64, y: f64, z: f64| {
        let r = ((x - a).powi(2) + (y - a).powi(2)).sqrt();
        if r >= a { 0.0 } else { j(0, x01 * r / a) * (-((z - 0.5 * l) / 0.02).powi(2)).exp() * (500.0 * z).cos() }
    };
    let open = |x: [f64; 3]| (x[0] - a).powi(2) + (x[1] - a).powi(2) < (a - h).powi(2);
    pic.set_b(0, |x| if open(x) { (pot(x[0], x[1] + 0.5 * h, x[2]) - pot(x[0], x[1] - 0.5 * h, x[2])) / h } else { 0.0 });
    pic.set_b(1, |x| if open(x) { -(pot(x[0] + 0.5 * h, x[1], x[2]) - pot(x[0] - 0.5 * h, x[1], x[2])) / h } else { 0.0 });
    let mut most = 0.0f64;
    for _ in 0..(2e-9 / dt) as usize {
        pic.step().unwrap();
        most = most.max(pic.energy().electric);
    }
    assert!(pic.energy().electric < 1e-5 * most, "{:e} of {most:e}", pic.energy().electric);
}

#[test]
fn a_run_gives_the_same_bits_on_any_number_of_threads() {
    // The fields by planes and the particles by chunks on the threads, the currents added in the
    // particles' order: one thread and many agree to the bit.
    let run = || {
        let g = Grid::new([axis(6, 1e-3, Wall::Periodic), axis(5, 1.2e-3, Wall::Conductor), axis(40, 0.9e-3, Wall::Conductor)]).unwrap();
        let mut pic = Pic::new(g, g.time_step(0.9), Pusher::Vay).unwrap();
        pic.absorb(2, true, 6).unwrap();
        pic.set_b(1, |_| 0.05);
        let mut r = Lcg(11);
        let lengths = g.lengths();
        let particles = (0..3000)
            .map(|_| Particle { x: lengths.map(|l| l * (0.05 + 0.9 * r.next())), u: [(); 3].map(|()| C0 * (2.0 * r.next() - 1.0)), w: 1e5 })
            .collect();
        pic.add(Population { species: ELECTRON, particles }).unwrap();
        for _ in 0..60 {
            pic.step().unwrap();
        }
        let e = pic.energy();
        (e.field.to_bits(), e.kinetic.to_bits(), pic.particles(), pic.gauss().iter().map(|g| g.to_bits()).collect::<Vec<_>>())
    };
    let one = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap().install(run);
    let many = rayon::ThreadPoolBuilder::new().num_threads(7).build().unwrap().install(run);
    assert!(one == many, "one thread and seven differ");
}

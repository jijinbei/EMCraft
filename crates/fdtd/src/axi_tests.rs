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

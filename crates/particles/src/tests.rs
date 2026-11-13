//! The pushes against motions known in closed form, and the runs' ends.

use std::f64::consts::PI;

use emcraft_model::units::C0;

use crate::push::{cross, dot};
use crate::*;

const Z: V3 = [0.0, 0.0, 1.0];

fn long_line() -> Line {
    Line { origin: [0.0, 0.0, -1e3], axis: Z, length: 2e3 }
}

/// Where the particle is after exactly `steps` steps (its last point), and that point's time.
fn after(run: &Trajectory, steps: usize) -> (V3, f64) {
    assert_eq!((run.end, run.steps), (End::Steps, steps));
    let last = run.points.last().copied().unwrap_or(Point { t: f64::NAN, x: [f64::NAN; 3], u: [0.0; 3] });
    (last.x, last.t)
}

/// A uniform electric field: hyperbolic motion. An electron at rest in E = −E₀ẑ gains
/// momentum p = eE₀t and moves z = (mc²/eE₀)(√(1 + (eE₀t/mc)²) − 1) (Landau & Lifshitz, *The
/// Classical Theory of Fields*, §20). One nanosecond in 1 MV/m takes it to γ ≈ 1.17.
#[test]
fn a_uniform_electric_field_gives_hyperbolic_motion() {
    let e0 = 1e6;
    let field = |_: V3, _: f64| Sample::Field([0.0, 0.0, -e0], [0.0; 3]);
    let (steps, dt) = (10_000, 1e-13);
    for pusher in [Pusher::Boris, Pusher::Vay] {
        let run =
            track(ELECTRON, Start { x: [0.0; 3], u: [0.0; 3], t: 0.0 }, &field, long_line(), Settings { pusher, dt, max_steps: steps, every: 100 })
                .unwrap();
        let (x, t) = after(&run, steps);
        let (q, mc) = (-ELECTRON.charge, ELECTRON.mass * C0);
        let exact = mc * C0 / (q * e0) * ((1.0 + (q * e0 * t / mc).powi(2)).sqrt() - 1.0);
        assert!((x[2] / exact - 1.0).abs() < 1e-7, "{pusher:?}: {} against {exact}", x[2]);
        assert!(x[0] == 0.0 && x[1] == 0.0);
    }
}

/// A uniform magnetic field: a circle of radius r = γmv/(eB) at ω = eB/(γm) (Jackson,
/// *Classical Electrodynamics*, 3rd ed., §12.2), |u| kept to rounding. An electron at γ = 3
/// moving along x in B = 0.1 ẑ circles about (0, r) — the force −e v×B points to +y.
#[test]
fn a_uniform_magnetic_field_gives_a_circle_at_the_cyclotron_frequency() {
    let b0 = 0.1;
    let u = [C0 * (9.0f64 - 1.0).sqrt(), 0.0, 0.0];
    let g = gamma(u);
    let (q, m) = (-ELECTRON.charge, ELECTRON.mass);
    let omega = q * b0 / (g * m);
    let r = g * m * (u[0] / g) / (q * b0);
    let field = |_: V3, _: f64| Sample::Field([0.0; 3], [0.0, 0.0, b0]);
    let dt = 0.01 / omega;
    let steps = 100_000;
    let run =
        track(ELECTRON, Start { x: [0.0; 3], u, t: 0.0 }, &field, long_line(), Settings { pusher: Pusher::Boris, dt, max_steps: steps, every: 1 })
            .unwrap();
    let (_, _) = after(&run, steps);
    assert!((gamma(run.u) / g - 1.0).abs() < 1e-12, "|u| kept");
    // Every point on the circle, to the leapfrog's (ωΔt)²/4 (its half step's offset: 2.5e-5).
    let worst = run.points.iter().map(|p| ((p.x[0].powi(2) + (p.x[1] - r).powi(2)).sqrt() / r - 1.0).abs()).fold(0.0, f64::max);
    assert!(worst < 2.6e-5, "radius off by {worst:e}");
    // The angle turned in all the steps against ωt.
    let turns = |p: &Point| (-(p.x[0])).atan2(p.x[1] - r);
    let mut angle = 0.0;
    for w in run.points.windows(2) {
        let d = turns(&w[1]) - turns(&w[0]);
        angle += d - 2.0 * PI * (d / (2.0 * PI)).round();
    }
    let t = dt * steps as f64;
    assert!((angle.abs() / (omega * t) - 1.0).abs() < 2e-5, "{} against {}", angle.abs(), omega * t);
}

/// Crossed fields, E below cB: the particle drifts at E×B/B², exactly so on average, whatever
/// its charge and start (in the frame moving at the drift the electric field vanishes and the
/// motion is a circle; Jackson §12.3). E = 10 kV/m ŷ, B = 0.01 T ẑ: 1 Mm/s along x.
#[test]
fn crossed_fields_drift_at_e_cross_b_over_b_squared() {
    let (e0, b0) = (1e4, 0.01);
    let field = |_: V3, _: f64| Sample::Field([0.0, e0, 0.0], [0.0, 0.0, b0]);
    let omega = -ELECTRON.charge * b0 / ELECTRON.mass;
    let dt = 0.02 / omega;
    let steps = 314_159;
    let run = track(
        ELECTRON,
        Start { x: [0.0; 3], u: [0.0; 3], t: 0.0 },
        &field,
        long_line(),
        Settings { pusher: Pusher::Boris, dt, max_steps: steps, every: 1000 },
    )
    .unwrap();
    let (x, t) = after(&run, steps);
    let drift = cross([0.0, e0, 0.0], [0.0, 0.0, b0]);
    let want = drift[0] / (b0 * b0);
    // A cycloid's radius over the distance drifted: the average to a part in a thousand.
    assert!((x[0] / t / want - 1.0).abs() < 1e-3, "{} against {want}", x[0] / t);
}

/// An electron in a plane wave, exactly (Landau & Lifshitz, *The Classical Theory of Fields*,
/// §48): with A = A₀ sin ωξ x̂, ξ = t − z/c, and the electron at rest where A = 0, its
/// canonical momentum gives pₓ = −qA, γ − p_z/(mc) = 1, so dx/dξ = pₓ/m and dz/dξ = p_z/m:
/// x = (ca/ω)(cos ωξ − 1) and z = (ca²/2)(ξ/2 − sin 2ωξ/(4ω)), a = qA₀/(mc). At a = 1 it is
/// relativistic: z runs ahead by a quarter of ξ c.
#[test]
fn an_electron_in_a_plane_wave_moves_as_landau_and_lifshitz_say() {
    let omega = 2.0 * PI * 1e9;
    let (q, m) = (ELECTRON.charge, ELECTRON.mass);
    let a0 = m * C0 / q.abs();
    let a = q * a0 / (m * C0);
    let field = move |x: V3, t: f64| {
        let ex = -a0 * omega * (omega * (t - x[2] / C0)).cos();
        Sample::Field([ex, 0.0, 0.0], [0.0, ex / C0, 0.0])
    };
    let period = 2.0 * PI / omega;
    let dt = period / 4000.0;
    let steps = 5 * 4000;
    for pusher in [Pusher::Boris, Pusher::Vay] {
        let run =
            track(ELECTRON, Start { x: [0.0; 3], u: [0.0; 3], t: 0.0 }, &field, long_line(), Settings { pusher, dt, max_steps: steps, every: 50 })
                .unwrap();
        let scale = C0 / omega;
        for p in run.points.iter().skip(1) {
            let xi = p.t - p.x[2] / C0;
            let x = C0 * a / omega * ((omega * xi).cos() - 1.0);
            let z = C0 * a * a / 2.0 * (xi / 2.0 - (2.0 * omega * xi).sin() / (4.0 * omega));
            assert!((p.x[0] - x).abs() < 1e-4 * scale && (p.x[2] - z).abs() < 1e-4 * scale, "{pusher:?} at t {}: {:?} against {x}, {z}", p.t, p.x);
        }
    }
}

/// Fields whose forces cancel (E = −v×B) leave a particle on its line. Vay's push does so at any
/// γ, to rounding; Boris's leaves it, the more the faster it goes (Vay, Phys. Plasmas
/// 15:056701, 2008, §II). At γ = 100 in 1 T, over 2000 steps of a tenth of the rest frame's
/// cyclotron period over γ.
#[test]
fn vays_push_keeps_a_particle_in_cancelling_fields_on_its_line_where_boris_does_not() {
    let u = [C0 * (100.0f64 * 100.0 - 1.0).sqrt(), 0.0, 0.0];
    let v = velocity(u);
    let b = [0.0, 0.0, 1.0];
    let vxb = cross(v, b);
    let e = [-vxb[0], -vxb[1], -vxb[2]];
    let field = move |_: V3, _: f64| Sample::Field(e, b);
    let omega = -ELECTRON.charge * 1.0 / ELECTRON.mass;
    let dt = 0.1 * 2.0 * PI / omega;
    let line = Line { origin: [-1.0, 0.0, 0.0], axis: [1.0, 0.0, 0.0], length: 1e9 };
    let off = |pusher| {
        let run = track(ELECTRON, Start { x: [0.0; 3], u, t: 0.0 }, &field, line, Settings { pusher, dt, max_steps: 2000, every: 1 }).unwrap();
        run.points.iter().map(|p| p.x[1].hypot(p.x[2])).fold(0.0, f64::max) / run.points.last().map_or(1.0, |p| p.x[0])
    };
    let (vay, boris) = (off(Pusher::Vay), off(Pusher::Boris));
    assert!(vay < 1e-12, "Vay: {vay:e}");
    assert!(boris > 1e3 * vay.max(1e-15), "Boris: {boris:e}");
}

/// A run ends where it should: through the exit plane, back behind the entry plane, on a wall
/// (with where), when its steps run out, or when a field stops being a number; and a run
/// asked wrongly is refused before it starts.
#[test]
fn a_run_ends_where_it_should_and_a_wrong_one_is_refused() {
    let mev = 1e6 * -ELECTRON.charge;
    let forward = ELECTRON.momentum(mev, Z);
    let line = Line { origin: [0.0; 3], axis: Z, length: 0.1 };
    let settings = Settings { pusher: Pusher::Boris, dt: 1e-12, max_steps: 10_000, every: 1 };
    let free = |_: V3, _: f64| Sample::Free;
    assert_eq!(track(ELECTRON, Start { x: [0.0; 3], u: forward, t: 0.0 }, &free, line, settings).unwrap().end, End::Through);
    let backward = ELECTRON.momentum(mev, [0.0, 0.0, -1.0]);
    assert_eq!(track(ELECTRON, Start { x: [0.0; 3], u: backward, t: 0.0 }, &free, line, settings).unwrap().end, End::Back);
    let walled = |x: V3, _: f64| if x[2] > 0.05 { Sample::Wall } else { Sample::Free };
    let lost = track(ELECTRON, Start { x: [0.0; 3], u: forward, t: 0.0 }, &walled, line, settings).unwrap();
    assert!(matches!(lost.end, End::Lost(at) if at[2] > 0.05 && at[2] < 0.051), "{:?}", lost.end);
    let slow = Settings { max_steps: 3, ..settings };
    assert_eq!(track(ELECTRON, Start { x: [0.0; 3], u: forward, t: 0.0 }, &free, line, slow).unwrap().end, End::Steps);
    let wild = |_: V3, _: f64| Sample::Field([f64::NAN; 3], [0.0; 3]);
    assert_eq!(track(ELECTRON, Start { x: [0.0; 3], u: forward, t: 0.0 }, &wild, line, settings).unwrap().end, End::NotFinite);
    for bad in [
        Settings { dt: 0.0, ..settings },
        Settings { dt: f64::NAN, ..settings },
        Settings { max_steps: 0, ..settings },
        Settings { max_steps: MAX_STEPS + 1, ..settings },
    ] {
        assert!(track(ELECTRON, Start { x: [0.0; 3], u: forward, t: 0.0 }, &free, line, bad).is_err(), "{bad:?}");
    }
    assert!(track(ELECTRON, Start { x: [0.0; 3], u: forward, t: 0.0 }, &free, Line { length: -1.0, ..line }, settings).is_err());
    assert!(track(ELECTRON, Start { x: [0.0; 3], u: forward, t: 0.0 }, &free, Line { axis: [0.0, 0.0, 2.0], ..line }, settings).is_err());
    assert!(track(ELECTRON, Start { x: [f64::INFINITY, 0.0, 0.0], u: forward, t: 0.0 }, &free, line, settings).is_err());
    assert!(dot(forward, forward) > 0.0);
}

//! Ports whose modes come from the cross-section solver (`emcraft_solver::portmode`): against
//! the closed forms where those apply, and on cross-sections they do not cover.

mod common;

use common::*;
use emcraft_fem::Order;
use emcraft_model::{Axis, Combine, End, Feature, Material, Model, Point3, Port, Shape, Solid};
use emcraft_solver::sweep::SweepOptions;
use emcraft_solver::{Complex64 as C, Driven};

/// An impedance (Ω), checked.
fn ohms(v: f64) -> emcraft_model::Ohms {
    emcraft_model::Ohms::new(v).unwrap()
}

/// A length (m), checked.
fn metres(v: f64) -> emcraft_model::Metres {
    emcraft_model::Metres::new(v).unwrap()
}

/// The high-pass filter with its ports' modes solved numerically.
fn numeric_filter() -> Model {
    let mut m = guide(&FILTER, 0.004, 4);
    m.ports.iter_mut().for_each(|p| p.mode_solver = emcraft_model::ModeSolver::Numeric);
    m
}

#[test]
fn numerical_port_modes_give_the_closed_forms_s_parameters() {
    // The filter's S against the exact cascade, as with the closed-form TE10, and against the
    // closed-form ports' own S: the modes differ only by the cross-section's discretisation.
    let numeric = Driven::new(&numeric_filter(), Order::Two).unwrap();
    let analytic = Driven::new(&guide(&FILTER, 0.004, 4), Order::Two).unwrap();
    for f in [5.0e9, 6.5e9, 8.0e9] {
        let (rn, ra) = (numeric.solve(f).unwrap(), analytic.solve(f).unwrap());
        let (s11, s21) = line(&FILTER).solve(f).unwrap().s_power();
        assert!((rn.s[1][0] - s21).norm() < 4e-3 && (rn.s[0][0] - s11).norm() < 4e-3, "{f}: {} {}", rn.s[1][0], s21);
        for (p, q) in [(0, 0), (1, 0), (1, 1)] {
            assert!((rn.s[p][q] - ra.s[p][q]).norm() < 1e-3, "{f}: S{}{} {} {}", p + 1, q + 1, rn.s[p][q], ra.s[p][q]);
        }
    }
    // Below the fill's TE10 cut-off (4.64 GHz) the first mode is evanescent: refused.
    assert!(numeric.solve(4.0e9).is_err());
}

/// A coaxial line: PTFE (εr = 2.1) between conductors of radii 1 and 3.5 mm, `length` long,
/// the inner conductor a Boolean difference through the whole length.
fn coax(length: f64, size: f64) -> Model {
    let mut m = Model::new();
    m.add_material(Material::new("ptfe", 2.1, 1.0).unwrap()).unwrap();
    m.set_solid(Solid { shape: Shape::Cylinder { radius: metres(0.0035), length: metres(length) }, material: "ptfe".into() }).unwrap();
    m.add_feature(Feature {
        combine: Combine::Subtract,
        shape: Shape::Cylinder { radius: metres(0.001), length: metres(length) },
        at: Point3::ORIGIN,
        axis: Axis::Z,
    })
    .unwrap();
    m.mesh = Some(emcraft_mesh::solid::solid(&m, metres(size)).unwrap().into());
    for (name, end) in [("1", End::Start), ("2", End::End)] {
        m.add_port(Port::new(name, end).with_modes(2)).unwrap();
    }
    m
}

#[test]
fn a_coaxial_line_transmits_its_tem_wave() {
    // No closed form covers the annulus, so its ports take the numerical modes: TEM first,
    // S21 = e^{−jk√εr L}, nothing reflected. The cross-section's triangles bend with the
    // volume's onto the conductors (straight ones, a polygon in a curved guide, reflect 5e-3):
    // |S11| 6e-5, the phase within 7e-5 rad at 2 mm over 10 mm; 2e-5 and 2e-5 over the 4 mm
    // at 2.5 mm cells here, half the time (the accuracy at length is the oracle's coaxial-line).
    let length = 0.004;
    let d = Driven::new(&coax(length, 0.0025), Order::Two).unwrap();
    let f = 3.0e9;
    let r = d.solve(f).unwrap();
    let beta = 2.0 * std::f64::consts::PI * f / emcraft_model::units::C0 * 2.1f64.sqrt();
    let exact = C::from_polar(1.0, -beta * length);
    assert!(r.s[0][0].norm() < 2e-4, "{}", r.s[0][0]);
    assert!((r.s[1][0] - exact).norm() < 3e-4, "{} {exact}", r.s[1][0]);
    // Lossless (the coaxial template's invariants are a heavy test of the engine's).
    assert!((r.s[0][0].norm_sqr() + r.s[1][0].norm_sqr() - 1.0).abs() < 1e-10);
}

/// WR-90 `length` long with a layer of εr `er` filling it from y = 0 to `d`, ports at both ends.
fn slab_guide(length: f64, d: f64, er: f64, size: f64) -> Model {
    let mut m = Model::new();
    m.add_material(Material::new("slab", er, 1.0).unwrap()).unwrap();
    let w = emcraft_model::Waveguide {
        width: metres(A),
        height: metres(B),
        sections: vec![emcraft_model::Section::new(metres(length), "vacuum")],
        layers: vec![emcraft_model::Layer { thickness: metres(d), material: "slab".into() }],
        strips: Vec::new(),
        vias: Vec::new(),
    };
    m.set_waveguide(w).unwrap();
    for (name, end) in [("1", End::Start), ("2", End::End)] {
        m.add_port(Port::new(name, end).with_modes(4)).unwrap();
    }
    m.mesh = Some(emcraft_mesh::waveguide(&m, metres(size)).unwrap().into());
    m
}

#[test]
fn a_slab_loaded_guide_transmits_its_hybrid_mode() {
    // The layered guide has no closed-form port modes; the numerical LSM₁₀ (TE10-like, with an
    // axial E) must pass with Collin's β and not reflect.
    let (length, d, er) = (0.02, B / 2.0, 2.25);
    let dr = Driven::new(&slab_guide(length, d, er, 0.002), Order::Two).unwrap();
    let f = 10e9;
    let k0 = 2.0 * std::f64::consts::PI * f / emcraft_model::units::C0;
    let beta = emcraft_theory::slab::slab_loaded_modes(
        &emcraft_theory::slab::SlabGuide { a: A, b: B, d, eps_r: er },
        emcraft_theory::slab::ModeSearch { k0, lowest: 0.0, count: 1 },
    )[0]
    .sqrt();
    let r = dr.solve(f).unwrap();
    let exact = C::from_polar(1.0, -beta * length);
    // 2 mm cells: |S11| 1e-5, S21 within 1e-4.
    assert!(r.s[0][0].norm() < 1e-4, "{}", r.s[0][0]);
    assert!((r.s[1][0] - exact).norm() < 5e-4, "{} {exact}", r.s[1][0]);
}

/// A stripline: a box a × b filled with εr, a strip `w` wide across its middle (y = b/2), the
/// whole `length` long.
fn stripline(a: f64, b: f64, w: f64, length: f64, er: f64, size: f64) -> Model {
    let mut m = Model::new();
    m.add_material(Material::new("fill", er, 1.0).unwrap()).unwrap();
    let wg = emcraft_model::Waveguide {
        width: metres(a),
        height: metres(b),
        sections: vec![emcraft_model::Section::new(metres(length), "fill")],
        layers: Vec::new(),
        strips: vec![emcraft_model::Strip { y: b / 2.0, x0: (a - w) / 2.0, width: metres(w), z0: 0.0, z1: length }],
        vias: Vec::new(),
    };
    m.set_waveguide(wg).unwrap();
    for (name, end) in [("1", End::Start), ("2", End::End)] {
        m.add_port(Port::new(name, end).with_modes(2)).unwrap();
    }
    m.mesh = Some(emcraft_mesh::waveguide(&m, metres(size)).unwrap().into());
    m
}

#[test]
fn a_stripline_carries_tem_at_the_speed_of_light_in_its_fill() {
    // A strip across the middle of a filled box: TEM, β = k√εr whatever the cross-section. The
    // strip's edges are singular (E ∝ r^{−1/2}): graded to a twentieth of the size there, |S11|
    // 1.5e-4 and the phase within 5e-5 rad at 1.5 mm (graded to a quarter, as irises are: 9e-4
    // and 7e-4, falling only as h).
    let (a, b, w, length, er) = (0.006, 0.003, 0.0015, 0.006, 2.2);
    let dr = Driven::new(&stripline(a, b, w, length, er, 0.0015), Order::Two).unwrap();
    let f = 5e9;
    let beta = 2.0 * std::f64::consts::PI * f / emcraft_model::units::C0 * er.sqrt();
    let r = dr.solve(f).unwrap();
    let exact = C::from_polar(1.0, -beta * length);
    assert!(r.s[0][0].norm() < 5e-4, "{}", r.s[0][0]);
    assert!((r.s[1][0] - exact).norm() < 5e-4, "{} {exact}", r.s[1][0]);
}

/// A microstrip in a shielding box `a` × `b`: a substrate `h` thick of εr, a strip `w` wide
/// centred on it, `length` long.
fn microstrip(a: f64, b: f64, h: f64, w: f64, er: f64, length: f64, size: f64) -> Model {
    let mut m = Model::new();
    m.add_material(Material::new("substrate", er, 1.0).unwrap()).unwrap();
    let wg = emcraft_model::Waveguide {
        width: metres(a),
        height: metres(b),
        sections: vec![emcraft_model::Section::new(metres(length), "vacuum")],
        layers: vec![emcraft_model::Layer { thickness: metres(h), material: "substrate".into() }],
        strips: vec![emcraft_model::Strip { y: h, x0: (a - w) / 2.0, width: metres(w), z0: 0.0, z1: length }],
        vias: Vec::new(),
    };
    m.set_waveguide(wg).unwrap();
    for (name, end) in [("1", End::Start), ("2", End::End)] {
        m.add_port(Port::new(name, end).with_modes(2)).unwrap();
    }
    m.mesh = Some(emcraft_mesh::waveguide(&m, metres(size)).unwrap().into());
    m
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn a_microstrip_has_hammerstad_and_jensens_effective_permittivity() {
    // Heavy: three fine cross-sections. In the gate, the 50 Ω strip's port mode is
    // strip_lines_have_their_closed_form_impedances; its ε_eff is the oracle's microstrip-eps-eff.
    // Alumina (εr = 9.8, h = 0.635 mm) at 1 GHz (quasi-static) in a box far from the strip (the
    // cover 38 substrates up, the side walls 12 strip widths away each side): the quasi-TEM
    // mode's (β/k₀)² against the fit (itself within 0.2 %), for a narrow, a 50 Ω and a wide
    // strip. Measured −0.24 %, −0.20 %, +0.30 % (with the cover at 9 h, −0.5 to −0.9 %: a cover
    // adds capacitance in the air and lowers ε_eff).
    let (h, er): (f64, f64) = (0.000635, 9.8);
    let k0 = 2.0 * std::f64::consts::PI * 1e9 / emcraft_model::units::C0;
    for u in [0.4f64, 0.945, 3.0] {
        let w = u * h;
        let dr = Driven::new(&microstrip((24.0 * w).max(0.012), 0.024, h, w, er, 0.0005, 0.0006), Order::Two).unwrap();
        let emcraft_solver::driven::PortModes::Numeric(n) = &dr.ports[0].modes else { panic!("closed-form port on a microstrip") };
        let eps = n.section.modes(k0, 1).unwrap()[0].beta2 / (k0 * k0);
        let fit = emcraft_theory::microstrip::eps_eff(u, er);
        assert!((eps / fit - 1.0).abs() < 5e-3, "{u}: {eps} {fit}");
    }
}

#[test]
fn strip_lines_have_their_closed_form_impedances() {
    // Power and current on the ports' cross-sections against Cohn's exact stripline impedance
    // (side walls three plate spacings from the strip) and Hammerstad–Jensen's microstrip fit:
    // measured −0.23 % and −0.58 %.
    let k0 = |f: f64| 2.0 * std::f64::consts::PI * f / emcraft_model::units::C0;
    let z_of = |m: &Model, f: f64| {
        let d = Driven::new(m, Order::Two).unwrap();
        let emcraft_solver::driven::PortModes::Numeric(n) = &d.ports[0].modes else { panic!("closed-form port") };
        let modes = n.section.modes(k0(f), 1).unwrap();
        n.section.impedance(&modes[0], k0(f)).unwrap().unwrap()
    };
    let (b, w, er) = (0.003, 0.0024, 2.2);
    let z = z_of(&stripline(w + 6.0 * b, b, w, 0.001, er, 0.001), 2e9);
    let cohn = emcraft_theory::stripline::z0(w, b, er);
    assert!((z / cohn - 1.0).abs() < 5e-3, "{z} {cohn}");
    let (h, u, er) = (0.000635, 0.945, 9.8);
    let z = z_of(&microstrip(24.0 * u * h, 0.024, h, u * h, er, 0.0005, 0.0006), 1e9);
    let fit = emcraft_theory::microstrip::z0(u, er);
    assert!((z / fit - 1.0).abs() < 1e-2, "{z} {fit}");
}

/// Two strips `w` wide a gap `s` apart, centred midway between planes `b` apart in εr = 2.2,
/// the side walls `side` from the strips, under the symmetry `planes`.
fn coupled_strips(w: f64, s: f64, b: f64, side: f64, planes: &[emcraft_model::Symmetry]) -> Model {
    let a = 2.0 * (side + w) + s;
    let mut m = stripline(a, b, w, 0.001, 2.2, 0.002);
    let wg = m.waveguide.clone().unwrap();
    let strip = |x0: f64| emcraft_model::Strip { y: b / 2.0, x0, width: metres(w), z0: 0.0, z1: 0.001 };
    m.set_waveguide(emcraft_model::Waveguide { strips: vec![strip(side), strip(side + w + s)], ..wg }).unwrap();
    m.set_symmetry(planes.to_vec()).unwrap();
    m.mesh = Some(emcraft_mesh::waveguide(&m, metres(0.001)).unwrap().into());
    m
}

#[test]
fn coupled_strips_have_cohns_even_and_odd_impedances() {
    // The two TEM modes of coupled strips share β = k√εr, so the eigensolver returns some basis
    // of them; split by the conductors' currents they are the even and the odd mode, against
    // Cohn's exact Z0e and Z0o (within 0.3 % and 0.4 % at 1 mm cells). Under a magnetic wall between
    // the strips only the even mode is left, under an electric wall only the odd one, the same
    // impedances from the half; and below a magnetic wall through the strips' plane, the same
    // again from the half's half strips.
    let (w, s, b) = (0.0015, 0.0005, 0.002);
    let (ze, zo) = emcraft_theory::coupled::z0_even_odd(w, s, b, 2.2).unwrap();
    let k0 = 2.0 * std::f64::consts::PI * 2e9 / emcraft_model::units::C0;
    let a = 2.0 * (0.006 + w) + s;
    let plane = |axis, at, kind| emcraft_model::Symmetry { axis, at, kind };
    let (mag, ele) = (emcraft_model::SymmetryKind::Magnetic, emcraft_model::SymmetryKind::Electric);
    let impedances = |planes: &[emcraft_model::Symmetry], count: usize| {
        let m = coupled_strips(w, s, b, 0.006, planes);
        let d = Driven::new(&m, Order::Two).unwrap();
        let emcraft_solver::driven::PortModes::Numeric(n) = &d.ports[0].modes else { panic!("closed-form port") };
        let modes = n.section.modes_split(k0, count).unwrap();
        let z: Vec<f64> = modes.iter().map(|md| n.section.impedance_whole(md, k0, planes).unwrap().unwrap()).collect();
        // The currents: equal on the two strips for the even mode, opposite for the odd one.
        if n.section.conductors().len() == 2 {
            let (ie, _) = n.section.currents(&modes[0], k0).unwrap();
            let (io, _) = n.section.currents(&modes[1], k0).unwrap();
            assert!((ie[0] / ie[1] - 1.0).norm() < 1e-6 && (io[0] / io[1] + 1.0).norm() < 1e-6, "{ie:?} {io:?}");
        }
        z
    };
    let z = impedances(&[], 2);
    assert!((z[0] / ze - 1.0).abs() < 3e-3 && (z[1] / zo - 1.0).abs() < 4e-3, "{z:?} {ze} {zo}");
    let half_y = impedances(&[plane(Axis::Y, b / 2.0, mag)], 2);
    assert!((half_y[0] / z[0] - 1.0).abs() < 1e-3 && (half_y[1] / z[1] - 1.0).abs() < 1e-3, "{half_y:?} {z:?}");
    let even = impedances(&[plane(Axis::X, a / 2.0, mag)], 1);
    let odd = impedances(&[plane(Axis::X, a / 2.0, ele)], 1);
    assert!((even[0] / z[0] - 1.0).abs() < 1e-3 && (odd[0] / z[1] - 1.0).abs() < 1e-3, "{even:?} {odd:?} {z:?}");
}

#[test]
fn s_parameters_referred_to_50_ohms_follow_the_line_equations() {
    // The coaxial line (Z₀ = η₀ ln 3.5/(2π√2.1) = 51.8 Ω) between 50 Ω references: a mismatched
    // line, S11 = Γ(1 − e^{−2jβL})/(1 − Γ²e^{−2jβL}), S21 = (1 − Γ²)e^{−jβL}/(1 − Γ²e^{−2jβL}),
    // Γ = (Z₀ − 50)/(Z₀ + 50). A 4 mm line at 2.5 mm cells, for speed (10 mm at 2 mm: the
    // oracle's coaxial-line-50-ohm).
    let length = 0.004;
    let mut m = coax(length, 0.0025);
    m.ports.iter_mut().for_each(|p| p.reference = Some(ohms(50.0)));
    let d = Driven::new(&m, Order::Two).unwrap();
    let f = 6e9;
    let r = d.solve(f).unwrap();
    let eta0 = emcraft_model::units::MU0 * emcraft_model::units::C0;
    let z0 = eta0 * 3.5f64.ln() / (2.0 * std::f64::consts::PI * 2.1f64.sqrt());
    let g = (z0 - 50.0) / (z0 + 50.0);
    let beta = 2.0 * std::f64::consts::PI * f / emcraft_model::units::C0 * 2.1f64.sqrt();
    let (e1, e2) = (C::from_polar(1.0, -beta * length), C::from_polar(1.0, -2.0 * beta * length));
    let den = 1.0 - g * g * e2;
    let (s11, s21) = (g * (1.0 - e2) / den, (1.0 - g * g) * e1 / den);
    let zl = r.impedances[0].unwrap();
    assert!((zl / z0 - 1.0).abs() < 1e-3, "{zl} {z0}");
    assert!((r.s[0][0] - s11).norm() < 1e-3 && (r.s[1][0] - s21).norm() < 1e-3);
    assert!((r.s[0][0].norm_sqr() + r.s[1][0].norm_sqr() - 1.0).abs() < 1e-9);
    // A reference on a port without a line impedance (a waveguide) is refused.
    let mut wg = guide(&FILTER, 0.004, 2);
    wg.ports[0].reference = Some(ohms(50.0));
    assert!(Driven::new(&wg, Order::One).unwrap().solve(8e9).is_err());
}

#[test]
fn a_fast_sweep_matches_the_full_one() {
    // The high-pass filter over 5–9 GHz in 21 points: the reduced-basis sweep against a full solve
    // at every point, with fewer factorisations, and fewer still with each one's moments.
    let d = Driven::new(&guide(&FILTER, 0.004, 4), Order::Two).unwrap();
    let freqs: Vec<f64> = (0..21).map(|i| 5e9 + 4e9 * i as f64 / 20.0).collect();
    let fast = emcraft_solver::sweep::fast_sweep(&d, &freqs, &Default::default()).unwrap();
    let solved = fast.full.iter().filter(|f| **f).count();
    // Without the moments each factorization adds only its fields.
    let plain = emcraft_solver::sweep::fast_sweep(&d, &freqs, &SweepOptions { moments: 0, ..Default::default() }).unwrap();
    let plain_solved = plain.full.iter().filter(|f| **f).count();
    let mut worst: f64 = 0.0;
    for (f, r) in freqs.iter().zip(&fast.results) {
        let exact = d.solve(*f).unwrap();
        for p in 0..2 {
            for q in 0..2 {
                worst = worst.max((r.s[p][q] - exact.s[p][q]).norm());
            }
        }
    }
    // Measured: 2 full solves with the moments (the band's ends), 6 without; |ΔS| 3.6e-9 (6e-11
    // with 6 solves: the error goes about as the residual squared, 1e-4 at most).
    assert!(worst < 1e-8, "{worst:e}");
    assert!(solved <= 3 && solved < plain_solved, "{solved} {plain_solved}");
    // Each result says how it was solved: the band's ends in full, the rest mostly in the basis.
    assert_eq!(fast.results.iter().map(|r| r.full).collect::<Vec<_>>(), fast.full);
    assert!(fast.full[0] && fast.full[20]);
    assert!(d.solve(7e9).unwrap().full);
}

#[test]
fn a_fast_sweep_reports_its_full_solves_and_residuals() {
    use emcraft_solver::SolveError;
    use emcraft_solver::sweep::{SweepStep, fast_sweep, fast_sweep_each};
    let d = Driven::new(&guide(&FILTER, 0.004, 4), Order::One).unwrap();
    let freqs: Vec<f64> = (0..11).map(|i| 5e9 + 4e9 * i as f64 / 10.0).collect();
    let o = Default::default();
    let mut steps: Vec<SweepStep> = Vec::new();
    let fast = fast_sweep_each::<SolveError>(&d, &freqs, &o, |s| {
        steps.push(s);
        Ok(())
    })
    .unwrap();
    let solved = fast.full.iter().filter(|f| **f).count();
    // Every step: the cap, the count of full solves never falling and never past what was made.
    assert!(steps.iter().all(|s| s.max_full == 11 && s.tolerance == 1e-4 && s.full <= solved));
    assert!(steps.windows(2).all(|w| w[0].full <= w[1].full));
    // The ends first (no residual yet), then passes in the basis that report the last residual.
    assert_eq!((steps[0].full, steps[0].residual), (0, None));
    assert!(steps.iter().any(|s| s.pass_total == 11 - 2 && s.pass_done == 0));
    let last = steps.last().unwrap();
    assert!(last.full == solved && last.residual.is_some_and(|r| r > 0.0), "{last:?}");
    // The same results as without the reports, and a refusal from `each` stops the sweep.
    let plain = fast_sweep(&d, &freqs, &o).unwrap();
    assert_eq!(plain.full, fast.full);
    assert!(fast_sweep_each(&d, &freqs, &o, |s| if s.full >= 2 { Err(SolveError::NotReady("stop".into())) } else { Ok(()) }).is_err());
}

#[test]
#[ignore = "benchmark: cargo test --release -p emcraft-solver --test ports factor_benchmark -- --ignored --nocapture"]
fn factor_benchmark() {
    // The coaxial line's system at 3 GHz, factored five times: the fastest numeric factorization.
    let d = Driven::new(&coax(0.01, 0.002), Order::Two).unwrap();
    let best = (0..5).map(|_| d.solve(3e9).unwrap().timings.factor).fold(f64::INFINITY, f64::min);
    eprintln!("numeric factorization: best of 5 {best:.3} s ({} unknowns)", d.dofs.unknowns);
}

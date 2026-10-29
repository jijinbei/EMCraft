//! Ports across several materials, one of them lossy (hybrid lossy modes): a stripline of two
//! lossy plies (TEM, γ = jk₀√(εr(1 − j tan δ)) exactly), a guide loaded with a lossy slab
//! (Collin's LSM₁₀ in complex arithmetic) and a shielded microstrip on a lossy substrate
//! (|S21| = e^{−αL} with the port's own α, reciprocal, passive), each γ evaluated here apart
//! from the solver's.

use std::f64::consts::PI;

use emcraft_fem::Order;
use emcraft_model::units::C0;
use emcraft_model::{End, Layer, Material, Model, Port, Section, Strip, Terminals, Waveguide};
use emcraft_solver::{Complex64 as C, Driven};

/// A box `a` × `b`, `length` long: `layers` from the floor up, the rest `fill`; `strips` across
/// it at (y, x0, width); a port at each end.
fn boxed(
    materials: Vec<Material>,
    a: f64,
    b: f64,
    length: f64,
    fill: &str,
    layers: Vec<(f64, &str)>,
    strips: &[(f64, f64, f64)],
    size: f64,
) -> Model {
    let mut m = Model::new();
    for mat in materials {
        m.add_material(mat).unwrap();
    }
    m.set_waveguide(Waveguide {
        width: a,
        height: b,
        sections: vec![Section::new(length, fill)],
        layers: layers.into_iter().map(|(thickness, material)| Layer { thickness, material: material.into() }).collect(),
        strips: strips.iter().map(|&(y, x0, width)| Strip { y, x0, width, z0: 0.0, z1: length }).collect(),
        vias: Vec::new(),
    })
    .unwrap();
    for (name, end) in [("1", End::Start), ("2", End::End)] {
        m.add_port(Port::new(name, end).with_modes(2)).unwrap();
    }
    m.mesh = Some(emcraft_mesh::waveguide(&m, size).unwrap());
    m
}

/// γ = jk₀√(εr(1 − j tan δ)), Re γ ≥ 0: a TEM wave in the lossy dielectric.
fn tem_gamma(f: f64, eps_r: f64, tan_d: f64) -> C {
    let k0 = 2.0 * PI * f / C0;
    let g = C::new(0.0, k0) * C::new(eps_r, -eps_r * tan_d).sqrt();
    if g.re < 0.0 { -g } else { g }
}

/// A stripline 6 × 3 mm, a 1.5 mm strip across its middle, the dielectric below the strip and
/// above it two materials of the same εr = 4.4, tan δ = 0.02 (a core and a prepreg).
fn two_ply_stripline(length: f64) -> Model {
    let core = Material::new("core", 4.4, 1.0).unwrap().with_loss_tangent(0.02).unwrap();
    let prepreg = Material::new("prepreg", 4.4, 1.0).unwrap().with_loss_tangent(0.02).unwrap();
    boxed(vec![core, prepreg], 0.006, 0.003, length, "prepreg", vec![(0.0015, "core")], &[(0.0015, 0.00225, 0.0015)], 0.0015)
}

#[test]
fn a_lossy_two_ply_stripline_carries_its_exact_tem_wave() {
    // Two materials in the cross-section, so the hybrid lossy solver; one dielectric in effect,
    // so the wave is TEM with γ exactly that of the dielectric whatever the shape.
    let length = 0.006;
    let m = two_ply_stripline(length);
    for f in [5e9, 10e9] {
        let exact = tem_gamma(f, 4.4, 0.02);
        let modes = emcraft_solver::driven::port_modes(&m, Order::Two, f).unwrap();
        for (_, list) in &modes {
            let g = list[0].gamma.unwrap();
            assert!((g - exact).norm() < 1e-6 * exact.norm(), "{f}: {g} {exact}");
        }
    }
    let d = Driven::new(&m, Order::Two).unwrap();
    for f in [5e9, 10e9] {
        let r = d.solve(f).unwrap();
        let exact = (-tem_gamma(f, 4.4, 0.02) * length).exp();
        let (s11, s21) = (r.s[0][0], r.s[1][0]);
        // As the lossless stripline (`stripline` oracle): |S11| ~1e-4, the phase within 0.003°.
        assert!(s11.norm() < 1e-3, "{f}: {s11}");
        assert!((s21 - exact).norm() < 1e-3, "{f}: {s21} {exact}");
        assert!((r.s[0][1] - s21).norm() < 1e-9, "{f}: reciprocal");
        assert!(s11.norm_sqr() + s21.norm_sqr() < 1.0);
    }
}

#[test]
fn a_guide_with_a_lossy_slab_transmits_collins_complex_mode() {
    // WR-90 with εr = 2.25, tan δ = 0.02 filling its lower half, 20 mm: S21 = e^{−γL} with the
    // LSM₁₀'s complex γ from the LSE/LSM equations in complex arithmetic.
    let (a, b, length) = (0.02286, 0.01016, 0.02);
    let slab = Material::new("slab", 2.25, 1.0).unwrap().with_loss_tangent(0.02).unwrap();
    let m = boxed(vec![slab], a, b, length, "vacuum", vec![(b / 2.0, "slab")], &[], 0.002);
    let d = Driven::new(&m, Order::Two).unwrap();
    let f = 10e9;
    let k0 = 2.0 * PI * f / C0;
    let beta2 = emcraft_theory::slab::slab_loaded_modes_lossy(a, b, b / 2.0, C::new(2.25, -0.045), k0, 0.0, 1)[0];
    let g = emcraft_theory::slab::gamma_of(beta2);
    let r = d.solve(f).unwrap();
    let exact = (-g * length).exp();
    // As the lossless slab (`slab-loaded-guide`): |S11| ~2e-5, S21 within 5e-4.
    assert!(r.s[0][0].norm() < 2e-4, "{}", r.s[0][0]);
    assert!((r.s[1][0] - exact).norm() < 5e-4, "{} {exact}", r.s[1][0]);
    assert!((r.s[0][1] - r.s[1][0]).norm() < 1e-9);
    // α read back from |S21| within 1 %.
    let alpha = -r.s[1][0].norm().ln() / length;
    assert!((alpha / g.re - 1.0).abs() < 0.01, "{alpha} {}", g.re);
}

#[test]
fn a_shielded_lossy_microstrip_decays_by_its_own_attenuation() {
    // FR-4 (εr = 4.4, tan δ = 0.02, 0.8 mm) under air in a 6 × 4 mm box, a 1.5 mm strip,
    // 10 mm long: matched, reciprocal, passive, and |S21| = e^{−αL} with the port mode's α.
    let fr4 = Material::new("fr4", 4.4, 1.0).unwrap().with_loss_tangent(0.02).unwrap();
    let length = 0.01;
    let m = boxed(vec![fr4], 0.006, 0.004, length, "vacuum", vec![(0.0008, "fr4")], &[(0.0008, 0.00225, 0.0015)], 0.001);
    let f = 5e9;
    let modes = emcraft_solver::driven::port_modes(&m, Order::Two, f).unwrap();
    let g = modes[0].1[0].gamma.unwrap();
    // Quasi-TEM: ε_eff between 1 and εr, α > 0 below the filled line's.
    let k0 = 2.0 * PI * f / C0;
    let eps_eff = -(g * g).re / (k0 * k0);
    assert!(eps_eff > 2.5 && eps_eff < 4.4 && g.re > 0.0 && g.re < tem_gamma(f, 4.4, 0.02).re, "{g} {eps_eff}");
    let d = Driven::new(&m, Order::Two).unwrap();
    let r = d.solve(f).unwrap();
    let (s11, s21) = (r.s[0][0], r.s[1][0]);
    assert!(s11.norm() < 5e-3, "{s11}");
    assert!((s21 - (-g * length).exp()).norm() < 5e-3, "{s21} {}", (-g * length).exp());
    assert!((r.s[0][1] - s21).norm() < 1e-9);
    assert!(s11.norm_sqr() + s21.norm_sqr() < 1.0);
}

#[test]
fn a_terminal_port_in_a_lossy_medium_is_refused() {
    // Two strips on FR-4: terminal ports would need power-orthogonal modes.
    let fr4 = Material::new("fr4", 4.4, 1.0).unwrap().with_loss_tangent(0.02).unwrap();
    let mut m = boxed(vec![fr4], 0.008, 0.004, 0.005, "vacuum", vec![(0.0008, "fr4")], &[(0.0008, 0.002, 0.001), (0.0008, 0.005, 0.001)], 0.001);
    m.ports.iter_mut().for_each(|p| p.terminals = Terminals::Auto);
    let e = Driven::new(&m, Order::One).unwrap_err();
    assert!(e.to_string().contains("not supported yet") && e.to_string().contains("terminal"), "{e}");
    // Modal, it is solved.
    m.ports.iter_mut().for_each(|p| p.terminals = Terminals::Modal);
    assert!(Driven::new(&m, Order::One).is_ok());
}

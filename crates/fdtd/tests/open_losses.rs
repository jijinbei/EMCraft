//! Lossy, magnetic and dispersive materials in the time domain's open problems: a Hertzian
//! dipole at the centre of a sphere, against the exact spherical-wave solution written out here
//! (the dipole radiates the TM₀₁ spherical wave alone; the sphere's surface is where it meets
//! free space).

use std::f64::consts::PI;

use emcraft_fdtd::open::{OpenOptions, radiate};
use emcraft_model::{Complex64 as C, Inclusion, LumpedPort, Material, Model, Open, Point3, Pole, Shape, Solid, Source, Vec3};

const J: C = C::new(0.0, 1.0);

/// ψ(x) = x j₁(x) and its derivative.
fn psi(x: C) -> (C, C) {
    (x.sin() / x - x.cos(), x.cos() / x - x.sin() / (x * x) + x.sin())
}

/// ζ(x) = x h₁⁽²⁾(x) = x (j₁ − j y₁) (outgoing for e^{jωt}) and its derivative.
fn zeta(x: C) -> (C, C) {
    let (p, dp) = psi(x);
    // χ(x) = x y₁(x) = −cos x/x − sin x.
    let (c, dc) = (-x.cos() / x - x.sin(), x.sin() / x + x.cos() / (x * x) - x.cos());
    (p - J * c, dp - J * dc)
}

/// The power a z dipole of moment I·l at the centre of a sphere of radius `b` and relative ε, μ
/// radiates, over its power in free space: |T|². A dipole in a medium of wavenumber k has
/// r H_φ = −(j k I·l sin θ/4π) ζ(kr) (C. A. Balanis, *Advanced Engineering Electromagnetics*,
/// 2nd ed., Wiley 2012, §6.8, the field of a Hertzian dipole), so with free space's amplitude
/// as unit, inside r H_φ = (k₁/k₀) ζ(k₁r) + B ψ(k₁r) and outside T ζ(k₀r); r H_φ and
/// r E_θ ∝ (k/ε) (r H_φ)′ in the argument are continuous at r = b.
fn sphere_power(f: f64, b: f64, eps: C, mu: C) -> f64 {
    let k0 = 2.0 * PI * f / emcraft_model::units::C0;
    let k1 = k0 * (eps * mu).sqrt();
    let (y1, a1) = (k1 / eps, k1 / k0);
    let ((z0, dz0), (z1, dz1), (p1, dp1)) = (zeta(C::new(k0 * b, 0.0)), zeta(k1 * b), psi(k1 * b));
    // B p1 − T z0 = −a1 z1, y1 B dp1 − k0 T dz0 = −y1 a1 dz1: Cramer's rule.
    let det = -k0 * p1 * dz0 + y1 * z0 * dp1;
    let t = a1 * y1 * (z1 * dp1 - p1 * dz1) / det;
    t.norm_sqr()
}

const CENTRE: Point3 = Point3::new(0.025, 0.025, 0.025);

/// A 50 mm box lined by 10 mm of layer, a z dipole at its centre inside a sphere of `material`
/// 10 mm in radius.
fn sphere_model(material: Material) -> Model {
    let mut m = Model::new();
    let name = material.name.clone();
    m.add_material(material).unwrap();
    m.set_solid(Solid { shape: Shape::Box { size: Vec3::from([0.05; 3]) }, material: "vacuum".into() }).unwrap();
    m.set_open(Some(Open { thickness: 0.01, reflection: None, shape: emcraft_model::OpenShape::Box })).unwrap();
    m.add_inclusion(Inclusion { shape: Shape::Sphere { radius: 0.01 }, at: CENTRE, material: Some(name) }).unwrap();
    m.add_source(Source::Dipole { at: CENTRE, direction: Vec3::new(0.0, 0.0, 1.0), length: 1e-3, current: 1.0 }).unwrap();
    m
}

/// The radiated power over the exact at 8 and 12 GHz (losses matched at 10 GHz), less one.
fn sphere_errors(material: Material, cell: f64, eps: impl Fn(f64) -> C, mu: impl Fn(f64) -> C) -> Vec<f64> {
    let freqs = [8e9, 12e9];
    let r = radiate(&sphere_model(material), &OpenOptions::new(cell), &freqs).unwrap();
    assert!(r.converged);
    freqs
        .iter()
        .zip(&r.radiated)
        .map(|(&f, &p)| {
            let free = emcraft_theory::radiation::dipole_power(1e-3, f);
            let exact = sphere_power(f, 0.01, eps(f), mu(f)) * free;
            eprintln!("{f:e}: {p:e} against {exact:e} ({:.3} of free space)", exact / free);
            p / exact - 1.0
        })
        .collect()
}

/// εr 3, μr 1.5, tan δ = tan δm = 0.1: as conductivities matched at 10 GHz.
fn lossy() -> (Material, impl Fn(f64) -> C, impl Fn(f64) -> C) {
    let m = Material::new("lossy", 3.0, 1.5).unwrap().with_loss_tangent(0.1).unwrap().with_magnetic_loss_tangent(0.1).unwrap();
    (m, |f: f64| C::new(3.0, -3.0 * 0.1 * 10e9 / f), |f: f64| C::new(1.5, -1.5 * 0.1 * 10e9 / f))
}

#[test]
fn a_dipole_in_a_lossy_magnetic_sphere_radiates_as_the_spherical_wave_solution() {
    let (m, eps, mu) = lossy();
    // 1 mm cells (λ/12 in the sphere at 12 GHz), the sphere staircased: measured −0.6 % and
    // +2.0 % (+1.5 % and +11 % at 2 mm).
    let errors = sphere_errors(m, 1e-3, eps, mu);
    assert!(errors.iter().all(|e| e.abs() < 0.04), "{errors:?}");
}

#[test]
fn the_sphere_solution_is_free_space_without_a_sphere() {
    let p = sphere_power(10e9, 0.01, C::new(1.0, 0.0), C::new(1.0, 0.0));
    assert!((p - 1.0).abs() < 1e-12, "{p}");
    // A small sphere screens the dipole by 3/(ε + 2) (quasi-statics), its power by the square.
    let small = sphere_power(1e8, 0.001, C::new(4.0, 0.0), C::new(1.0, 0.0));
    assert!((small - 0.25).abs() < 1e-3, "{small}");
    // A lossy sphere absorbs.
    assert!(sphere_power(10e9, 0.01, C::new(3.0, -0.6), C::new(1.0, 0.0)) < sphere_power(10e9, 0.01, C::new(3.0, 0.0), C::new(1.0, 0.0)));
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn spheres_converge_on_finer_grids() {
    let (m, eps, mu) = lossy();
    let errors = sphere_errors(m, 0.7e-3, eps, mu);
    eprintln!("{errors:?}");
    assert!(errors.iter().all(|e| e.abs() < 0.02), "{errors:?}");
    let tau = 1.0 / (2.0 * PI * 20e9);
    let debye = Material::new("debye", 2.0, 1.0).unwrap().with_dispersion(vec![Pole::Debye { delta_eps: 1.0, tau }]).unwrap();
    let errors = sphere_errors(debye, 1e-3, |f| C::new(2.0, 0.0) + 1.0 / C::new(1.0, 2.0 * PI * f * tau), |_| C::new(1.0, 0.0));
    assert!(errors.iter().all(|e| e.abs() < 0.03), "{errors:?}");
}

#[test]
fn lumped_ports_in_lossy_materials_and_lossy_walls_are_refused() {
    let (material, _, _) = lossy();
    let mut m = sphere_model(material);
    m.sources.clear();
    let gap = LumpedPort { name: "1".into(), from: CENTRE, to: Point3::new(0.025, 0.025, 0.027), impedance: 50.0, voltage: [1.0, 0.0], radius: 0.0 };
    m.add_lumped_port(gap).unwrap();
    assert!(matches!(radiate(&m, &OpenOptions::new(2e-3), &[10e9]), Err(emcraft_fdtd::FdtdError::Unsupported(_))));
    let (material, _, _) = lossy();
    let mut m = sphere_model(material);
    m.wall_conductivity = Some(5.8e7);
    assert!(matches!(radiate(&m, &OpenOptions::new(2e-3), &[10e9]), Err(emcraft_fdtd::FdtdError::Unsupported(_))));
    // A loss tangent read unchecked from a file.
    let (material, _, _) = lossy();
    let mut m = sphere_model(material);
    m.materials[1].loss_tangent = f64::INFINITY;
    assert!(radiate(&m, &OpenOptions::new(2e-3), &[10e9]).is_err());
}

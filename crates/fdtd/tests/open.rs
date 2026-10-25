//! The time domain's open problems against closed forms: a current element's radiated power
//! and pattern, and short gaps' mutual impedance (the oracle case time-domain-gaps holds the
//! fine-grid numbers).

use emcraft_fdtd::open::{OpenOptions, radiate};
use emcraft_model::{LumpedPort, Model, Open, Point3, Shape, Solid, Source, Vec3};

fn open_box(size: f64, pml: f64) -> Model {
    let mut m = Model::new();
    m.set_solid(Solid { shape: Shape::Box { size: Vec3::from([size; 3]) }, material: "vacuum".into() }).unwrap();
    m.set_open(Some(Open { thickness: pml, reflection: None })).unwrap();
    m
}

fn gap(name: &str, x: f64, z: f64, v: f64) -> LumpedPort {
    LumpedPort {
        name: name.into(),
        from: Point3::new(x, 0.02, z),
        to: Point3::new(x, 0.02, z + 0.002),
        impedance: 1e9,
        voltage: [v, 0.0],
        radius: 0.0,
    }
}

#[test]
fn a_coarse_current_element_radiates_and_bad_inputs_are_refused() {
    // 2 mm cells (λ/15), a 40 mm box: the plumbing, fast.
    let f = 10e9;
    let mut m = open_box(0.04, 0.01);
    m.add_source(Source::Dipole { at: Point3::new(0.02, 0.02, 0.02), direction: Vec3::new(0.0, 0.0, 1.0), length: 1e-3, current: 1.0 }).unwrap();
    let r = radiate(&m, &OpenOptions::new(2e-3), &[f]).unwrap();
    let exact = emcraft_theory::radiation::dipole_power(1e-3, f);
    assert!(r.converged && (r.radiated[0] / exact - 1.0).abs() < 0.1, "{} {exact}", r.radiated[0]);
    let d = r.directivity(0, std::f64::consts::FRAC_PI_2, 0.3).unwrap();
    assert!((d / 1.5 - 1.0).abs() < 0.1, "{d}");
    // Along no grid axis; a phase; no sources; bad options.
    let mut tilted = open_box(0.04, 0.01);
    tilted.add_source(Source::Dipole { at: Point3::new(0.02, 0.02, 0.02), direction: Vec3::new(1.0, 0.0, 1.0), length: 1e-3, current: 1.0 }).unwrap();
    assert!(radiate(&tilted, &OpenOptions::new(2e-3), &[f]).is_err());
    let mut phased = open_box(0.04, 0.01);
    phased.add_lumped_port(LumpedPort { voltage: [0.0, 1.0], ..gap("1", 0.02, 0.02, 1.0) }).unwrap();
    assert!(radiate(&phased, &OpenOptions::new(2e-3), &[f]).is_err());
    assert!(radiate(&open_box(0.04, 0.01), &OpenOptions::new(2e-3), &[f]).is_err());
    assert!(radiate(&m, &OpenOptions::new(-1.0), &[f]).is_err());
    assert!(radiate(&m, &OpenOptions::new(2e-3), &[]).is_err());
    assert!(radiate(&m, &OpenOptions::new(2e-3), &[f64::NAN]).is_err());
}

#[test]
#[ignore = "heavy: cargo xtask test --heavy (CI heavy job)"]
fn a_current_element_radiates_as_a_hertzian_dipole() {
    let mut m = open_box(0.06, 0.015);
    m.add_source(Source::Dipole { at: Point3::new(0.03, 0.03, 0.03), direction: Vec3::new(0.0, 0.0, 1.0), length: 1e-3, current: 1.0 }).unwrap();
    let f = 10e9;
    let r = radiate(&m, &OpenOptions::new(1e-3), &[f]).unwrap();
    // Measured +0.27 %, and the directivity within 0.4 % down to 20° from the axis.
    let exact = emcraft_theory::radiation::dipole_power(1e-3, f);
    assert!((r.radiated[0] / exact - 1.0).abs() < 0.01);
    for t in [90.0f64, 45.0, 20.0] {
        let d = r.directivity(0, t.to_radians(), 0.3).unwrap();
        assert!((d / (1.5 * t.to_radians().sin().powi(2)) - 1.0).abs() < 0.01, "{t}: {d}");
    }
}

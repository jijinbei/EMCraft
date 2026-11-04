//! Plane waves in the time domain: the total-field/scattered-field split on the open box.

use emcraft_fdtd::open::{OpenOptions, radiate};
use emcraft_model::units::ETA0;
use emcraft_model::{Inclusion, LumpedPort, Material, Model, Open, Point3, Shape, Solid, Source, Vec3};

fn metres(v: f64) -> emcraft_model::Metres {
    emcraft_model::Metres::new(v).unwrap()
}

/// A 40 mm box lined by a 8 mm layer, a sphere of radius 4 mm and εr `eps` in its middle, a
/// plane wave along `direction` polarised along `polarization`.
fn lit_sphere(eps: f64, direction: Vec3, polarization: Vec3) -> Model {
    let mut m = Model::new();
    m.add_material(Material::new("faint", eps, 1.0).unwrap()).unwrap();
    m.set_solid(Solid { shape: Shape::Box { size: emcraft_model::BoxSize::new(Vec3::new(0.04, 0.04, 0.04)).unwrap() }, material: "vacuum".into() })
        .unwrap();
    m.add_inclusion(Inclusion { shape: Shape::Sphere { radius: metres(0.004) }, at: Point3::new(0.02, 0.02, 0.02), material: Some("faint".into()) })
        .unwrap();
    m.set_open(Some(Open { thickness: metres(0.008), reflection: None, shape: emcraft_model::OpenShape::Box })).unwrap();
    m.add_source(Source::PlaneWave { direction, polarization }).unwrap();
    m
}

/// The Rayleigh–Gans cross-section of a faint sphere of radius `a` and εr `eps` at `f`:
/// σ = (8π/3) k⁴ a⁶ ((εr − 1)/(εr + 2))² (Bohren & Huffman 1983, §5.2).
fn rayleigh(f: f64, a: f64, eps: f64) -> f64 {
    let k = 2.0 * std::f64::consts::PI * f / emcraft_model::units::C0;
    8.0 * std::f64::consts::PI / 3.0 * k.powi(4) * a.powi(6) * ((eps - 1.0) / (eps + 2.0)).powi(2)
}

#[test]
fn a_faint_sphere_scatters_as_rayleigh_says_whichever_way_the_wave_comes() {
    // Along +z and −x: the split lets no incident wave out, so what reaches the Huygens box is
    // the sphere's.
    for (k, e) in [(Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0)), (Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0))] {
        let r = radiate(&lit_sphere(1.01, k, e), &OpenOptions::new(1e-3), &[3e9, 4e9, 5e9]).unwrap();
        assert!(r.converged);
        let sigma = r.radiated[1] * 2.0 * ETA0;
        let exact = rayleigh(4e9, 0.004, 1.01);
        assert!((sigma / exact - 1.0).abs() < 0.08, "{k:?}: {sigma} against {exact}");
    }
}

#[test]
fn plane_waves_the_split_cannot_hold_are_refused() {
    let o = OpenOptions::new(1e-3);
    let off_axis = lit_sphere(1.01, Vec3::new(0.0, 0.6, 0.8), Vec3::new(1.0, 0.0, 0.0));
    assert!(matches!(radiate(&off_axis, &o, &[4e9]), Err(emcraft_fdtd::FdtdError::Unsupported(_))));
    let mut with_port = lit_sphere(1.01, Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0));
    let gap = LumpedPort {
        name: "1".into(),
        from: Point3::new(0.02, 0.02, 0.012),
        to: Point3::new(0.02, 0.02, 0.013),
        impedance: emcraft_model::Ohms::new(50.0).unwrap(),
        voltage: [1.0, 0.0],
        radius: 0.0,
        across: None,
    };
    with_port.add_lumped_port(gap).unwrap();
    assert!(matches!(radiate(&with_port, &o, &[4e9]), Err(emcraft_fdtd::FdtdError::Unsupported(_))));
    // A sphere filling the box leaves no room for the split.
    let mut big = Model::new();
    big.set_solid(Solid { shape: Shape::Box { size: emcraft_model::BoxSize::new(Vec3::new(0.03, 0.03, 0.03)).unwrap() }, material: "vacuum".into() })
        .unwrap();
    big.add_inclusion(Inclusion { shape: Shape::Sphere { radius: metres(0.0065) }, at: Point3::new(0.015, 0.015, 0.015), material: None }).unwrap();
    big.set_open(Some(Open { thickness: metres(0.008), reflection: None, shape: emcraft_model::OpenShape::Box })).unwrap();
    big.add_source(Source::PlaneWave { direction: Vec3::new(0.0, 0.0, 1.0), polarization: Vec3::new(1.0, 0.0, 0.0) }).unwrap();
    assert!(radiate(&big, &o, &[4e9]).is_err());
}

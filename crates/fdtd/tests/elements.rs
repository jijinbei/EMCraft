//! Lumped elements in the time domain: on a port's own gap, its admittance gains theirs.

use emcraft_fdtd::open::{OpenOptions, radiate};
use emcraft_model::{Arrangement, Complex64 as C, LumpedElement, LumpedPort, Model, Open, Point3, Shape, Solid, Vec3};

fn metres(v: f64) -> emcraft_model::Metres {
    emcraft_model::Metres::new(v).unwrap()
}

const FROM: Point3 = Point3::new(0.015, 0.015, 0.014);
const TO: Point3 = Point3::new(0.015, 0.015, 0.016);

/// A 2 mm gap fed by a 50 Ω port standing free in a 30 mm open box, with `element` across it.
fn gap(element: Option<(f64, f64, f64, Arrangement)>) -> C {
    let mut m = Model::new();
    m.set_solid(Solid { shape: Shape::Box { size: emcraft_model::BoxSize::new(Vec3::new(0.03, 0.03, 0.03)).unwrap() }, material: "vacuum".into() })
        .unwrap();
    m.set_open(Some(Open { thickness: metres(0.006), reflection: None, shape: emcraft_model::OpenShape::Box })).unwrap();
    m.add_lumped_port(LumpedPort {
        name: "p".into(),
        from: FROM,
        to: TO,
        impedance: emcraft_model::Ohms::new(50.0).unwrap(),
        voltage: [1.0, 0.0],
        radius: 0.0,
        across: None,
    })
    .unwrap();
    if let Some((resistance, inductance, capacitance, arrangement)) = element {
        m.add_element(LumpedElement { name: "e".into(), from: FROM, to: TO, resistance, inductance, capacitance, arrangement, across: None })
            .unwrap();
    }
    let r = radiate(&m, &OpenOptions::new(1e-3), &[2e9, 3e9, 4e9]).unwrap();
    assert!(r.converged);
    r.ports[0].impedance[1]
}

#[test]
fn an_element_on_a_ports_gap_adds_its_admittance() {
    let bare = gap(None);
    let w = 2.0 * std::f64::consts::PI * 3e9;
    for (e, y) in [
        ((100.0, 0.0, 0.0, Arrangement::Series), C::new(0.01, 0.0)),
        ((200.0, 0.0, 0.5e-12, Arrangement::Parallel), C::new(0.005, w * 0.5e-12)),
        ((0.0, 5e-9, 0.0, Arrangement::Series), C::new(0.0, -1.0 / (w * 5e-9))),
    ] {
        let loaded = gap(Some(e));
        let expected = 1.0 / (1.0 / bare + y);
        // The leapfrog's own time steps: the capacitor's and inductor's currents are differences
        // and sums over a step (second order in ω Δt).
        assert!((loaded - expected).norm() < 2e-3 * expected.norm(), "{e:?}: {loaded} against {expected} (bare {bare})");
    }
}

#[test]
fn elements_the_time_domain_cannot_hold_are_refused() {
    let mut m = Model::new();
    m.set_solid(Solid { shape: Shape::Box { size: emcraft_model::BoxSize::new(Vec3::new(0.03, 0.03, 0.03)).unwrap() }, material: "vacuum".into() })
        .unwrap();
    m.set_open(Some(Open { thickness: metres(0.006), reflection: None, shape: emcraft_model::OpenShape::Box })).unwrap();
    m.add_lumped_port(LumpedPort {
        name: "p".into(),
        from: FROM,
        to: TO,
        impedance: emcraft_model::Ohms::new(50.0).unwrap(),
        voltage: [1.0, 0.0],
        radius: 0.0,
        across: None,
    })
    .unwrap();
    m.add_element(LumpedElement {
        name: "rlc".into(),
        from: FROM,
        to: TO,
        resistance: 10.0,
        inductance: 1e-9,
        capacitance: 1e-12,
        arrangement: Arrangement::Series,
        across: None,
    })
    .unwrap();
    assert!(matches!(radiate(&m, &OpenOptions::new(1e-3), &[3e9]), Err(emcraft_fdtd::FdtdError::Unsupported(_))));
    // On a gap of its own beside the port: a load the port sees through the air between them.
    m.elements.clear();
    let (a, b) = (Point3::new(0.018, 0.015, 0.014), Point3::new(0.018, 0.015, 0.016));
    m.add_element(LumpedElement {
        name: "r".into(),
        from: a,
        to: b,
        resistance: 10.0,
        inductance: 0.0,
        capacitance: 0.0,
        arrangement: Arrangement::Series,
        across: None,
    })
    .unwrap();
    let near = radiate(&m, &OpenOptions::new(1e-3), &[2e9, 3e9, 4e9]).unwrap();
    assert!(near.ports[0].impedance.iter().all(|z| z.re.is_finite() && z.im.is_finite()));
}

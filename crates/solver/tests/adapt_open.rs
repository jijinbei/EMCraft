//! The residual indicator of open problems (`emcraft_solver::adapt::open_indicator`): the
//! perfectly matched layer and a current element's neighbourhood left out, a plane wave's
//! incident field and lifted conductor values taken as the scattered field's data.

use emcraft_fem::Order;
use emcraft_mesh::solid::{Sizes, solid_sized};
use emcraft_model::{Inclusion, Model, Open, OpenShape, Point3, Shape, Solid, Source, Vec3};
use emcraft_solver::Driven;
use emcraft_solver::adapt::open_indicator;

const SIDE: f64 = 0.04;
const LAYER: f64 = 0.01;
const F: f64 = 10.0e9;

fn open_box() -> Model {
    let mut m = Model::new();
    m.set_solid(Solid { shape: Shape::Box { size: Vec3::new(SIDE, SIDE, SIDE) }, material: "vacuum".into() }).unwrap();
    m
}

fn meshed(mut m: Model, size: f64) -> Model {
    m.set_open(Some(Open { thickness: LAYER, reflection: None, shape: OpenShape::Box })).unwrap();
    m.mesh = Some(solid_sized(&m, &Sizes { default: size, per_material: Vec::new() }, true).unwrap());
    m
}

/// Whether a point lies in the box's layer.
fn in_layer(p: Point3) -> bool {
    (0..3).any(|k| p[k] < LAYER || p[k] > SIDE - LAYER)
}

#[test]
fn the_layer_and_a_dipoles_neighbourhood_are_left_out() {
    let mut m = open_box();
    let at = Point3::new(0.0201, 0.0202, 0.0199);
    m.add_source(Source::Dipole { at, direction: Vec3::unit(2), length: 0.001, current: 1.0 }).unwrap();
    let m = meshed(m, 0.006);
    let d = Driven::open(&m, Order::One).unwrap();
    let r = d.solve_open(&m, F).unwrap();
    assert!(r.lifted.is_empty(), "no plane wave, nothing lifted");
    let eta = open_indicator(&d, &m, &r).unwrap();
    let mesh = m.mesh.as_ref().unwrap();
    assert_eq!(eta.len(), mesh.tets.len());
    assert!(eta.iter().all(|e| e.is_finite() && *e >= 0.0));
    // The layer (by the solver's rule: the centroid's depth) carries nothing.
    for t in 0..mesh.tets.len() {
        if in_layer(mesh.centroid(t).unwrap()) {
            assert_eq!(eta[t], 0.0, "tetrahedron {t} in the layer");
        }
    }
    // Nor do the dipole's tetrahedron and those sharing a corner with it.
    let (home, _) = emcraft_mesh::locate(mesh, at).unwrap();
    assert_eq!(eta[home], 0.0);
    let corners = mesh.tets[home];
    let patch: Vec<usize> = (0..mesh.tets.len()).filter(|&t| mesh.tets[t].iter().any(|v| corners.contains(v))).collect();
    assert!(patch.len() > 4 && patch.iter().all(|&t| eta[t] == 0.0));
    // The rest of the physical domain does.
    let inside = (0..mesh.tets.len()).filter(|&t| !in_layer(mesh.centroid(t).unwrap()) && !patch.contains(&t)).collect::<Vec<_>>();
    assert!(!inside.is_empty() && inside.iter().filter(|&&t| eta[t] > 0.0).count() == inside.len());
    // Deterministic, bit for bit.
    let again = open_indicator(&d, &m, &d.solve_open(&m, F).unwrap()).unwrap();
    assert!(eta.iter().zip(&again).all(|(a, b)| a.to_bits() == b.to_bits()));
}

#[test]
fn a_conductors_lifted_trace_is_the_scattered_fields_data() {
    let mut m = open_box();
    m.add_inclusion(Inclusion { shape: Shape::Sphere { radius: 0.004 }, at: Point3::new(0.02, 0.02, 0.02), material: None }).unwrap();
    m.add_source(Source::PlaneWave { direction: Vec3::unit(2), polarization: Vec3::unit(0) }).unwrap();
    let m = meshed(m, 0.005);
    let d = Driven::open(&m, Order::One).unwrap();
    let r = d.solve_open(&m, F).unwrap();
    assert!(!r.lifted.is_empty() && r.lifted.windows(2).all(|w| w[0].0 < w[1].0), "the conductor's values, ascending");
    let eta = open_indicator(&d, &m, &r).unwrap();
    // Without the lifted values the elements on the sphere would see a scattered field whose
    // trace is zero there instead of −n × E_inc: a jump the solution does not have.
    let mut bare = r.clone();
    bare.lifted.clear();
    let wrong = open_indicator(&d, &m, &bare).unwrap();
    let mesh = m.mesh.as_ref().unwrap();
    let near = |e: &[f64]| {
        (0..mesh.tets.len()).filter(|&t| mesh.centroid(t).unwrap().distance(Point3::new(0.02, 0.02, 0.02)) < 0.007).map(|t| e[t]).sum::<f64>()
    };
    let (right, without) = (near(&eta), near(&wrong));
    // Measured 30 times.
    assert!(without > 10.0 * right, "{right:e} {without:e}");
}

#[test]
fn a_dielectric_scatterers_incident_field_is_its_source() {
    let mut m = open_box();
    m.add_material(emcraft_model::Material::new("glass", 4.0, 1.0).unwrap()).unwrap();
    m.add_inclusion(Inclusion { shape: Shape::Sphere { radius: 0.004 }, at: Point3::new(0.02, 0.02, 0.02), material: Some("glass".into()) }).unwrap();
    m.add_source(Source::PlaneWave { direction: Vec3::unit(2), polarization: Vec3::unit(0) }).unwrap();
    let m = meshed(m, 0.006);
    let d = Driven::open(&m, Order::Two).unwrap();
    let r = d.solve_open(&m, F).unwrap();
    assert!(r.lifted.is_empty(), "no conductor");
    let eta = open_indicator(&d, &m, &r).unwrap();
    // Leaving the incident field out would make the solution look wrong where it is right: the
    // indicator taken as an unforced problem's is larger in and round the sphere (second-order
    // elements, whose own residual is small next to the missing source).
    let mut m2 = m.clone();
    m2.sources.clear();
    let unforced = open_indicator(&d, &m2, &r).unwrap();
    let mesh = m.mesh.as_ref().unwrap();
    let near = |e: &[f64]| {
        (0..mesh.tets.len()).filter(|&t| mesh.centroid(t).unwrap().distance(Point3::new(0.02, 0.02, 0.02)) < 0.005).map(|t| e[t]).sum::<f64>()
    };
    // Measured 143 times.
    assert!(near(&unforced) > 30.0 * near(&eta), "{:e} {:e}", near(&eta), near(&unforced));
}

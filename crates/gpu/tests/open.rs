//! The open box on a device against the CPU's threads: a current element and two lumped gaps
//! (ports and sources, the CPML on every face, the Huygens box's transforms).

use std::time::Instant;

use emcraft_fdtd::open::{OpenOptions, OpenRun, radiate, radiate_with};
use emcraft_gpu::{Backend, Gpu, Precision, available};
use emcraft_model::{Inclusion, LumpedPort, Material, Model, Open, Point3, Pole, Shape, Solid, Source, Vec3};

fn open_box(size: f64, pml: f64) -> Model {
    let mut m = Model::new();
    m.set_solid(Solid { shape: Shape::Box { size: Vec3::from([size; 3]) }, material: "vacuum".into() }).unwrap();
    m.set_open(Some(Open { thickness: pml, reflection: None, shape: emcraft_model::OpenShape::Box })).unwrap();
    m
}

/// A dipole at the centre and two gaps beside it.
fn antennas(size: f64) -> Model {
    let mut m = open_box(size, 0.01);
    let c = size / 2.0;
    m.add_source(Source::Dipole { at: Point3::new(c, c, c), direction: Vec3::new(0.0, 0.0, 1.0), length: 1e-3, current: 1.0 }).unwrap();
    for (name, x) in [("1", c - 0.006), ("2", c + 0.006)] {
        let gap = LumpedPort {
            name: name.into(),
            from: Point3::new(x, c, c - 0.002),
            to: Point3::new(x, c, c + 0.002),
            impedance: 50.0,
            voltage: [1.0, 0.0],
            radius: 0.0,
        };
        m.add_lumped_port(gap).unwrap();
    }
    m
}

/// The antennas with the dipole inside a sphere of a lossy, magnetically lossy, dispersive
/// material: the slotted edges, the impressed current on them, the H faces' μ and decay.
fn loaded(size: f64) -> Model {
    let mut m = antennas(size);
    let poles = vec![Pole::Debye { delta_eps: 1.0, tau: 1e-11 }];
    let x = Material::new("x", 2.0, 1.5)
        .unwrap()
        .with_loss_tangent(0.1)
        .unwrap()
        .with_magnetic_loss_tangent(0.1)
        .unwrap()
        .with_dispersion(poles)
        .unwrap();
    m.add_material(x).unwrap();
    let c = size / 2.0;
    m.add_inclusion(Inclusion { shape: Shape::Sphere { radius: 0.004 }, at: Point3::new(c, c, c), material: Some("x".into()) }).unwrap();
    m
}

/// The largest difference of the ports' impedances and the radiated powers, relative.
fn worst(a: &OpenRun, b: &OpenRun) -> f64 {
    let mut w: f64 = 0.0;
    for (p, q) in a.ports.iter().zip(&b.ports) {
        for (x, y) in p.impedance.iter().zip(&q.impedance) {
            w = w.max((x - y).norm() / y.norm());
        }
    }
    for (x, y) in a.radiated.iter().zip(&b.radiated) {
        w = w.max((x - y).abs() / y.abs());
    }
    let fx = a.far_field(0, 1.2, 0.3).unwrap();
    let fy = b.far_field(0, 1.2, 0.3).unwrap();
    w.max(((fx.0 - fy.0).norm() + (fx.1 - fy.1).norm()) / (fy.0.norm() + fy.1.norm()))
}

fn on(m: &Model, o: &OpenOptions, freqs: &[f64], backend: Backend, precision: Precision) -> OpenRun {
    let t = Instant::now();
    let r = radiate_with(m, o, freqs, Some(&Gpu { backend, precision })).unwrap();
    eprintln!("{backend} {precision}: {} steps in {:.2?}", r.steps, t.elapsed());
    r
}

/// `m` on CubeCL's CPU runtime in `precision` against the threads, cut short (the runtime
/// launches slowly: 400 steps, two batches, of a coarse box). The largest difference, and
/// whether the steps were the same. Each precision is a test of its own, side by side in the
/// gate.
fn gap(m: &Model, precision: Precision) -> (f64, bool) {
    let freqs = [9e9, 11e9];
    let o = OpenOptions { max_steps: 400, ..OpenOptions::new(2e-3) };
    let cpu = radiate(m, &o, &freqs).unwrap();
    let r = on(m, &o, &freqs, Backend::Cpu, precision);
    (worst(&r, &cpu), r.steps == cpu.steps)
}

#[test]
fn the_cpu_backend_matches_the_threads_in_the_open() {
    let (w, same_steps) = gap(&antennas(0.04), Precision::Double);
    assert!(same_steps);
    assert!(w < 1e-8, "{w:e}");
}

#[test]
fn the_cpu_backend_in_single_precision_matches_the_threads_in_the_open_to_its_precision() {
    let (w, _) = gap(&antennas(0.04), Precision::Single);
    assert!(w < 1e-3, "{w:e}");
}

#[test]
fn lossy_dispersive_and_magnetic_materials_in_the_open_match_the_threads() {
    let (w, _) = gap(&loaded(0.04), Precision::Double);
    assert!(w < 1e-8, "{w:e}");
}

#[test]
fn lossy_dispersive_and_magnetic_materials_in_single_precision_match_the_threads_to_its_precision() {
    let (w, _) = gap(&loaded(0.04), Precision::Single);
    assert!(w < 1e-3, "{w:e}");
}

#[test]
#[ignore = "needs an NVIDIA GPU: cargo test -p emcraft-gpu --features cuda -- --ignored"]
fn cuda_matches_the_threads_in_the_open() {
    let cuda = available().into_iter().find(|a| a.backend == Backend::Cuda).unwrap();
    assert!(cuda.problem.is_none(), "{cuda:?}");
    let (m, freqs) = (antennas(0.05), [9e9, 11e9]);
    let o = OpenOptions::new(1e-3);
    let t = Instant::now();
    let cpu = radiate(&m, &o, &freqs).unwrap();
    eprintln!("threads: {} steps in {:.2?}", cpu.steps, t.elapsed());
    let double = on(&m, &o, &freqs, Backend::Cuda, Precision::Double);
    assert_eq!(double.steps, cpu.steps);
    assert!(worst(&double, &cpu) < 1e-8, "{:e}", worst(&double, &cpu));
    let single = on(&m, &o, &freqs, Backend::Cuda, Precision::Single);
    assert!(worst(&single, &cpu) < 1e-3, "{:e}", worst(&single, &cpu));
    assert_eq!(on(&m, &o, &freqs, Backend::Cuda, Precision::Single), single);
    // Lossy, dispersive and magnetic materials to the end.
    let m = loaded(0.05);
    let cpu = radiate(&m, &o, &freqs).unwrap();
    let double = on(&m, &o, &freqs, Backend::Cuda, Precision::Double);
    assert!(worst(&double, &cpu) < 1e-8, "{:e}", worst(&double, &cpu));
    let single = on(&m, &o, &freqs, Backend::Cuda, Precision::Single);
    assert!(worst(&single, &cpu) < 1e-3, "{:e}", worst(&single, &cpu));
}

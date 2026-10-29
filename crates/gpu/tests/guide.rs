//! The device leapfrog against the CPU's threads on the high-pass filter: double precision to
//! rounding, single to its precision, the same bits twice.

use std::time::Instant;

use emcraft_fdtd::{Options, TdResult, s_parameters, s_parameters_with};
use emcraft_gpu::{Backend, Gpu, Precision, available};
use std::f64::consts::PI;

use emcraft_model::{End, Material, Model, Pole, Port, Section, Waveguide};

fn guide(materials: Vec<Material>, sections: Vec<Section>) -> Model {
    let mut m = Model::new();
    for x in materials {
        m.add_material(x).unwrap();
    }
    m.set_waveguide(Waveguide { width: 0.02286, height: 0.01016, sections, layers: Vec::new(), strips: Vec::new(), vias: Vec::new() }).unwrap();
    m.add_port(Port::new("1", End::Start).with_modes(1)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(1)).unwrap();
    m
}

fn filter() -> Model {
    let sections = vec![Section::new(0.015, "ptfe"), Section::new(0.02, "vacuum"), Section::new(0.015, "ptfe")];
    guide(vec![Material::new("ptfe", 2.0, 1.0).unwrap()], sections)
}

/// A slab with a Debye and a Lorentz pole (the CPU's `dispersive_slab`).
fn dispersive_slab() -> Model {
    let poles = vec![Pole::Debye { delta_eps: 0.6, tau: 1.0 / (2.0 * PI * 20e9) }, Pole::Lorentz { delta_eps: 0.4, frequency: 16e9, damping: 1.5e9 }];
    let disp = Material::new("disp", 2.0, 1.0).unwrap().with_dispersion(poles).unwrap();
    guide(vec![disp], vec![Section::new(0.01, "vacuum"), Section::new(0.02, "disp"), Section::new(0.01, "vacuum")])
}

const FREQS: [f64; 3] = [5.5e9, 7.5e9, 8.5e9];

fn worst(a: &TdResult, b: &TdResult) -> f64 {
    (0..FREQS.len()).map(|i| (a.s11[i] - b.s11[i]).norm().max((a.s21[i] - b.s21[i]).norm())).fold(0.0, f64::max)
}

fn on(backend: Backend, precision: Precision, o: &Options) -> TdResult {
    on_model(&filter(), &FREQS, backend, precision, o)
}

fn on_model(m: &Model, freqs: &[f64], backend: Backend, precision: Precision, o: &Options) -> TdResult {
    let t = Instant::now();
    let r = s_parameters_with(m, o, freqs, Some(&Gpu { backend, precision })).unwrap();
    eprintln!("{backend} {precision}: {} + {} steps in {:.2?}", r.steps, r.reference_steps, t.elapsed());
    r
}

#[test]
fn the_cpu_backend_matches_the_threads() {
    // CubeCL's CPU runtime launches slowly (some 3 ms a step here): a run cut short at 520 steps
    // (into the second batch, the pulse through both planes), the transforms compared as they
    // stand. Bit-for-bit repeats are checked on CUDA.
    let o = Options { max_steps: 520, ..Options::new(0.003) };
    let cpu = s_parameters(&filter(), &o, &FREQS).unwrap();
    let double = on(Backend::Cpu, Precision::Double, &o);
    // The same steps; the arithmetic in another order (fused multiply–adds, the projections'
    // tree): rounding apart.
    assert_eq!((double.steps, double.reference_steps), (cpu.steps, cpu.reference_steps));
    assert!(worst(&double, &cpu) < 1e-9, "{:e}", worst(&double, &cpu));
    let single = on(Backend::Cpu, Precision::Single, &o);
    assert!(worst(&single, &cpu) < 1e-4, "{:e}", worst(&single, &cpu));
    assert!(cpu.s21.iter().all(|s| s.norm() > 0.1), "{cpu:?}");
}

#[test]
fn dispersive_edges_advance_their_poles_on_the_device() {
    // The auxiliary equations on the device against the threads', cut short as above (2 mm:
    // the Lorentz pole's explicit update needs ω₀Δt < 0.5).
    let (m, freqs) = (dispersive_slab(), [8e9, 12e9]);
    let o = Options { max_steps: 520, ..Options::new(0.002) };
    let cpu = s_parameters(&m, &o, &freqs).unwrap();
    let double = on_model(&m, &freqs, Backend::Cpu, Precision::Double, &o);
    let worst = (0..2).map(|i| (double.s11[i] - cpu.s11[i]).norm().max((double.s21[i] - cpu.s21[i]).norm())).fold(0.0, f64::max);
    assert!(worst < 1e-9, "{worst:e}");
    assert!(cpu.s21.iter().all(|s| s.norm() > 0.1), "{cpu:?}");
}

/// Every loss at once: a lossy dielectric (a conductivity), a magnetically lossy ferrite and
/// walls of finite conductivity.
fn lossy_guide() -> Model {
    let lossy = Material::new("lossy", 2.2, 1.0).unwrap().with_loss_tangent(0.05).unwrap();
    let ferrite = Material::new("ferrite", 1.5, 2.0).unwrap().with_magnetic_loss_tangent(0.05).unwrap();
    let sections = vec![Section::new(0.01, "vacuum"), Section::new(0.01, "lossy"), Section::new(0.01, "ferrite"), Section::new(0.01, "vacuum")];
    let mut m = guide(vec![lossy, ferrite], sections);
    m.set_wall_conductivity(Some(1e6)).unwrap();
    m
}

#[test]
fn losses_and_lossy_walls_on_the_device_match_the_threads() {
    // Cut short as above: the walls' edges, their branches of surface current, the lossy edges
    // and the magnetic faces' decay against the threads'.
    let (m, freqs) = (lossy_guide(), [8e9, 12e9]);
    let o = Options { max_steps: 520, ..Options::new(0.003) };
    let cpu = s_parameters(&m, &o, &freqs).unwrap();
    let double = on_model(&m, &freqs, Backend::Cpu, Precision::Double, &o);
    let gap = |r: &TdResult| (0..2).map(|i| (r.s11[i] - cpu.s11[i]).norm().max((r.s21[i] - cpu.s21[i]).norm())).fold(0.0, f64::max);
    assert!(gap(&double) < 1e-9, "{:e}", gap(&double));
    let single = on_model(&m, &freqs, Backend::Cpu, Precision::Single, &o);
    assert!(gap(&single) < 1e-4, "{:e}", gap(&single));
    assert!(cpu.s21.iter().all(|s| s.norm() > 0.05), "{cpu:?}");
}

#[test]
fn every_built_backend_reports_what_it_runs() {
    let list = available();
    assert_eq!(list.iter().map(|a| a.backend).collect::<Vec<_>>(), Backend::ALL);
    let cpu = &list[0];
    assert_eq!((cpu.precisions.as_slice(), &cpu.problem), (&Precision::ALL[..], &None));
    for a in &list {
        assert!(a.precisions.is_empty() == a.problem.is_some(), "{a:?}");
    }
}

/// On a machine with CUDA (`cargo test -p emcraft-gpu --features cuda -- --ignored`).
#[test]
#[ignore = "needs an NVIDIA GPU: cargo test -p emcraft-gpu --features cuda -- --ignored"]
fn cuda_matches_the_threads() {
    let cuda = available().into_iter().find(|a| a.backend == Backend::Cuda).unwrap();
    assert!(cuda.problem.is_none(), "{cuda:?}");
    for cell in [0.003, 0.001] {
        let t = Instant::now();
        let cpu = s_parameters(&filter(), &Options::new(cell), &FREQS).unwrap();
        eprintln!("threads: {} + {} steps in {:.2?}", cpu.steps, cpu.reference_steps, t.elapsed());
        let o = Options::new(cell);
        let double = on(Backend::Cuda, Precision::Double, &o);
        assert!(worst(&double, &cpu) < 1e-9, "{cell}: {:e}", worst(&double, &cpu));
        // Single precision's 6e-8 grown over some 25 000 steps: well inside the scheme's own
        // 1e-2.
        let single = on(Backend::Cuda, Precision::Single, &o);
        assert!(worst(&single, &cpu) < 1e-4, "{cell}: {:e}", worst(&single, &cpu));
        assert_eq!(on(Backend::Cuda, Precision::Single, &o), single);
    }
    // The dispersive slab to the end.
    let (m, freqs) = (dispersive_slab(), [8e9, 10e9, 12e9]);
    let cpu = s_parameters(&m, &Options::new(0.0015), &freqs).unwrap();
    for precision in Precision::ALL {
        let r = on_model(&m, &freqs, Backend::Cuda, precision, &Options::new(0.0015));
        let worst = (0..3).map(|i| (r.s11[i] - cpu.s11[i]).norm().max((r.s21[i] - cpu.s21[i]).norm())).fold(0.0, f64::max);
        assert!(worst < if precision == Precision::Double { 1e-9 } else { 1e-4 }, "{precision}: {worst:e}");
    }
    // Every loss to the end.
    let (m, freqs) = (lossy_guide(), [8e9, 10e9, 12e9]);
    let cpu = s_parameters(&m, &Options::new(0.0015), &freqs).unwrap();
    for precision in Precision::ALL {
        let r = on_model(&m, &freqs, Backend::Cuda, precision, &Options::new(0.0015));
        let worst = (0..3).map(|i| (r.s11[i] - cpu.s11[i]).norm().max((r.s21[i] - cpu.s21[i]).norm())).fold(0.0, f64::max);
        assert!(worst < if precision == Precision::Double { 1e-9 } else { 1e-4 }, "lossy {precision}: {worst:e}");
    }
}

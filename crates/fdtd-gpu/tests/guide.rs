//! The device leapfrog against the CPU's threads on the high-pass filter: double precision to
//! rounding, single to its precision, the same bits twice.

use std::time::Instant;

use emcraft_fdtd::{Options, TdResult, s_parameters, s_parameters_with};
use emcraft_fdtd_gpu::{Backend, Gpu, Precision, available};
use emcraft_model::{End, Material, Model, Port, Section, Waveguide};

fn filter() -> Model {
    let mut m = Model::new();
    m.add_material(Material::new("ptfe", 2.0, 1.0).unwrap()).unwrap();
    let sections = vec![Section::new(0.015, "ptfe"), Section::new(0.02, "vacuum"), Section::new(0.015, "ptfe")];
    m.set_waveguide(Waveguide { width: 0.02286, height: 0.01016, sections, layers: Vec::new(), strips: Vec::new(), vias: Vec::new() }).unwrap();
    m.add_port(Port::new("1", End::Start).with_modes(1)).unwrap();
    m.add_port(Port::new("2", End::End).with_modes(1)).unwrap();
    m
}

const FREQS: [f64; 3] = [5.5e9, 7.5e9, 8.5e9];

fn worst(a: &TdResult, b: &TdResult) -> f64 {
    (0..FREQS.len()).map(|i| (a.s11[i] - b.s11[i]).norm().max((a.s21[i] - b.s21[i]).norm())).fold(0.0, f64::max)
}

fn on(backend: Backend, precision: Precision, o: &Options) -> TdResult {
    let t = Instant::now();
    let r = s_parameters_with(&filter(), o, &FREQS, Some(&Gpu { backend, precision })).unwrap();
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
fn every_built_backend_reports_what_it_runs() {
    let list = available();
    assert_eq!(list.iter().map(|a| a.backend).collect::<Vec<_>>(), Backend::ALL);
    let cpu = &list[0];
    assert_eq!((cpu.precisions.as_slice(), &cpu.problem), (&Precision::ALL[..], &None));
    for a in &list {
        assert!(a.precisions.is_empty() == a.problem.is_some(), "{a:?}");
    }
}

/// On a machine with CUDA (`cargo test -p emcraft-fdtd-gpu --features cuda -- --ignored`).
#[test]
#[ignore = "needs an NVIDIA GPU: cargo test -p emcraft-fdtd-gpu --features cuda -- --ignored"]
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
}

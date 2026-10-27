//! EMCraft's time-domain leapfrog on a GPU: the waveguide run's [`Plan`] stepped by kernels
//! written in Rust with CubeCL (docs/adr/0002-gpu.md), on CUDA or on the CPU's SIMD, in single
//! or double precision.
//!
//! **What runs where.** The CPU prepares the plan, transforms the projections and decides when
//! to stop, as for its own threads (`emcraft_fdtd`); the device holds the fields and runs four
//! kernels a step: H, E, the source and the two projections. The projections are kept on the
//! device and read back every [`BATCH`] steps, so the host waits on the device once a batch.
//!
//! **Determinism.** Every field value is written by one unit, from values of the previous half
//! step, and each projection is summed in a fixed order (each unit a fixed stride, then a tree
//! of fixed pairs): the same plan on the same device and precision gives the same result bit for
//! bit. Devices and precisions differ in rounding (fused multiply–adds, single precision): their
//! results agree with the CPU's to the precision, not bit for bit.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::fmt;
use std::ops::ControlFlow;
use std::panic::{AssertUnwindSafe, catch_unwind};

use cubecl::prelude::*;
use cubecl::server::Handle;
use cubecl::std::tensor::TensorHandle;
use emcraft_fdtd::{FdtdError, Plan, Result, Stepper};

mod kernels;

/// Steps between reads of the projections.
pub const BATCH: usize = 500;

/// Where the kernels run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// The CPU, through CubeCL's SIMD compiler (always built; for tests and machines without a
    /// GPU).
    Cpu,
    /// An NVIDIA GPU (the `cuda` feature).
    Cuda,
}

impl Backend {
    pub const ALL: [Backend; 2] = [Backend::Cpu, Backend::Cuda];

    /// The name commands use.
    pub fn name(self) -> &'static str {
        match self {
            Backend::Cpu => "cpu",
            Backend::Cuda => "cuda",
        }
    }

    pub fn parse(s: &str) -> Option<Backend> {
        Backend::ALL.into_iter().find(|b| b.name() == s)
    }

    /// Whether this build carries the backend.
    pub fn built(self) -> bool {
        match self {
            Backend::Cpu => true,
            Backend::Cuda => cfg!(feature = "cuda"),
        }
    }

    fn device(self) -> Result<cubecl::Device> {
        match self {
            Backend::Cpu => Ok(cubecl::Device::Cpu(Default::default())),
            #[cfg(feature = "cuda")]
            Backend::Cuda => Ok(cubecl::Device::Cuda(Default::default())),
            #[cfg(not(feature = "cuda"))]
            Backend::Cuda => Err(FdtdError::Unsupported("CUDA in this build (built without the cuda feature)".into())),
        }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The floating-point type of the fields on the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    /// f32: half the memory and traffic of double; ample for S-parameters to 1e-4.
    Single,
    /// f64: for checking single against.
    Double,
}

impl Precision {
    pub const ALL: [Precision; 2] = [Precision::Single, Precision::Double];

    pub fn name(self) -> &'static str {
        match self {
            Precision::Single => "single",
            Precision::Double => "double",
        }
    }

    pub fn parse(s: &str) -> Option<Precision> {
        Precision::ALL.into_iter().find(|p| p.name() == s)
    }
}

impl fmt::Display for Precision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A device's backend and the precisions it can run.
#[derive(Clone, Debug, PartialEq)]
pub struct Available {
    pub backend: Backend,
    /// Empty when the backend cannot run here (with the reason).
    pub precisions: Vec<Precision>,
    pub problem: Option<String>,
}

/// The backends of this build and what each can run on this machine.
pub fn available() -> Vec<Available> {
    Backend::ALL
        .into_iter()
        .map(|backend| match guarded(|| probe(backend)) {
            Ok(precisions) => Available { backend, precisions, problem: None },
            Err(e) => Available { backend, precisions: Vec::new(), problem: Some(e.to_string()) },
        })
        .collect()
}

/// The precisions `backend` supports: a tiny kernel run in each.
fn probe(backend: Backend) -> Result<Vec<Precision>> {
    let client = backend.device()?.client();
    let mut ok = Vec::new();
    for p in Precision::ALL {
        let works = match p {
            Precision::Single => kernels::probe::<f32>(&client),
            Precision::Double => kernels::probe::<f64>(&client),
        };
        if works {
            ok.push(p);
        }
    }
    if ok.is_empty() {
        return Err(FdtdError::Device(format!("{backend} ran no test kernel")));
    }
    Ok(ok)
}

/// Runs `f`, turning a panic inside the device's runtime into an error.
fn guarded<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|panic| {
        let why = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
        Err(FdtdError::Device(format!("the runtime stopped: {why}")))
    })
}

/// A [`Stepper`] on a CubeCL device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gpu {
    pub backend: Backend,
    pub precision: Precision,
}

impl Stepper for Gpu {
    fn leapfrog(&self, plan: &Plan, max_steps: usize, observe: &mut dyn FnMut(usize, [f64; 2]) -> ControlFlow<()>) -> Result<usize> {
        if plan.dispersive {
            return Err(FdtdError::Unsupported(format!("dispersive materials on {} (run on the CPU's threads)", self.backend)));
        }
        let device = self.backend.device()?;
        guarded(|| {
            let client = device.client();
            match self.precision {
                Precision::Single => Run::<f32>::new(&client, plan)?.go(&client, plan, max_steps, observe),
                Precision::Double => Run::<f64>::new(&client, plan)?.go(&client, plan, max_steps, observe),
            }
        })
    }
}

/// A floating-point type the kernels run in.
pub(crate) trait Real: Float + CubeElement + cubecl::ScalarArgType + LaunchArg<RuntimeArg = Self> {
    const KIND: cubecl::ir::FloatKind;
    fn of(x: f64) -> Self;
    fn wide(self) -> f64;
}

impl Real for f32 {
    const KIND: cubecl::ir::FloatKind = cubecl::ir::FloatKind::F32;
    fn of(x: f64) -> f32 {
        x as f32
    }
    fn wide(self) -> f64 {
        self as f64
    }
}

impl Real for f64 {
    const KIND: cubecl::ir::FloatKind = cubecl::ir::FloatKind::F64;
    fn of(x: f64) -> f64 {
        x
    }
    fn wide(self) -> f64 {
        self
    }
}

/// An array on the device.
#[derive(Clone)]
pub(crate) struct Buf<F> {
    handle: Handle,
    len: usize,
    _f: std::marker::PhantomData<F>,
}

impl<F: Real> Buf<F> {
    fn new(client: &Client, xs: &[f64]) -> Buf<F> {
        let v: Vec<F> = xs.iter().map(|&x| F::of(x)).collect();
        Buf { handle: client.create_from_slice(F::as_bytes(&v)), len: v.len(), _f: std::marker::PhantomData }
    }

    fn zeros(client: &Client, len: usize) -> Buf<F> {
        Buf::new(client, &vec![0.0; len])
    }

    /// The array as a kernel argument; its length is the array's own.
    pub(crate) fn arg(&self) -> BufferArg {
        TensorHandle::new_contiguous([self.len], self.handle.clone(), cubecl::ir::ElemType::Float(F::KIND)).binding().into_buffer_arg()
    }

    fn read(&self, client: &Client) -> Result<Vec<f64>> {
        let bytes = client.read_one(self.handle.clone()).map_err(|e| FdtdError::Device(format!("{e:?}")))?;
        Ok(F::from_bytes(&bytes).iter().map(|x| x.wide()).collect())
    }
}

/// The run's arrays on the device.
struct Run<F> {
    e: [Buf<F>; 3],
    h: [Buf<F>; 3],
    ce: [Buf<F>; 3],
    ch: [Buf<F>; 3],
    psi_e: [Buf<F>; 2],
    psi_h: [Buf<F>; 2],
    primal: [Buf<F>; 3],
    dual: [Buf<F>; 3],
    cpml_e: Buf<F>,
    cpml_h: Buf<F>,
    source: Buf<F>,
    weights: Buf<F>,
    /// Each step's two projections, a batch of them.
    record: Buf<F>,
}

/// The most units a projection's cube uses.
const REDUCE: u32 = 256;

impl<F: Real> Run<F> {
    fn new(client: &Client, p: &Plan) -> Result<Run<F>> {
        let [nx, ny, nz] = p.n;
        let plane = (nx + 1) * ny;
        let lens_e = [nx * (ny + 1) * (nz + 1), (nx + 1) * ny * (nz + 1), (nx + 1) * (ny + 1) * nz];
        let lens_h = [(nx + 1) * ny * nz, nx * (ny + 1) * nz, nx * ny * (nz + 1)];
        let shapes_agree = p.e.iter().zip(lens_e).all(|(c, l)| c.len() == l)
            && p.h.iter().zip(lens_h).all(|(c, l)| c.len() == l)
            && p.primal.iter().zip(p.n).all(|(d, n)| d.len() == n)
            && p.dual.iter().zip(p.n).all(|(d, n)| d.len() == n + 1)
            && p.cpml_e.len() == nz + 1
            && p.cpml_h.len() == nz
            && p.source.len() == plane
            && p.weights.iter().all(|w| w.len() == plane)
            && p.source_plane <= nz
            && p.planes.iter().all(|&k| k <= nz);
        if !shapes_agree {
            return Err(FdtdError::Invalid { what: "plan".into(), why: "its arrays do not match its grid".into() });
        }
        // Indices on the device are 32-bit.
        if lens_e.iter().chain(&lens_h).any(|&l| l >= u32::MAX as usize / 2) {
            return Err(FdtdError::TooLarge { cells: nx * ny * nz, limit: emcraft_fdtd::MAX_CELLS });
        }
        let flat = |c: &[[f64; 2]]| c.iter().flatten().copied().collect::<Vec<f64>>();
        let weights: Vec<f64> = p.weights.iter().flatten().copied().collect();
        Ok(Run {
            e: lens_e.map(|l| Buf::zeros(client, l)),
            h: lens_h.map(|l| Buf::zeros(client, l)),
            ce: p.e.each_ref().map(|c| Buf::new(client, c)),
            ch: p.h.each_ref().map(|c| Buf::new(client, c)),
            psi_e: [Buf::zeros(client, lens_e[0]), Buf::zeros(client, lens_e[1])],
            psi_h: [Buf::zeros(client, lens_h[0]), Buf::zeros(client, lens_h[1])],
            primal: p.primal.each_ref().map(|d| Buf::new(client, d)),
            dual: p.dual.each_ref().map(|d| Buf::new(client, d)),
            cpml_e: Buf::new(client, &flat(&p.cpml_e)),
            cpml_h: Buf::new(client, &flat(&p.cpml_h)),
            source: Buf::new(client, &p.source),
            weights: Buf::new(client, &weights),
            record: Buf::zeros(client, 2 * BATCH),
        })
    }

    /// Steps in batches, handing each batch's projections to `observe` in order.
    fn go(&self, client: &Client, p: &Plan, max_steps: usize, observe: &mut dyn FnMut(usize, [f64; 2]) -> ControlFlow<()>) -> Result<usize> {
        let [nx, ny, nz] = p.n.map(|n| n as u32);
        let nodes = (nx + 1) * (ny + 1) * (nz + 1);
        let plane = (nx + 1) * ny;
        let cubes = |units: u32| CubeCount::Static(units.div_ceil(256), 1, 1);
        // A power of two the device's cubes hold.
        let most = REDUCE.min(client.properties().hardware.max_units_per_cube).max(1);
        let reduce = 1 << (31 - most.leading_zeros());
        let mut n = 0;
        while n < max_steps {
            let batch = BATCH.min(max_steps - n);
            for b in 0..batch {
                let [ex, ey, ez] = &self.e;
                let [hx, hy, hz] = &self.h;
                let [cex, cey, cez] = &self.ce;
                let [chx, chy, chz] = &self.ch;
                let [dx, dy, dz] = &self.primal;
                let [dxd, dyd, dzd] = &self.dual;
                kernels::step_h::launch::<F>(
                    client,
                    cubes(nodes),
                    CubeDim::new_1d(256),
                    hx.arg(),
                    hy.arg(),
                    hz.arg(),
                    ex.arg(),
                    ey.arg(),
                    ez.arg(),
                    chx.arg(),
                    chy.arg(),
                    chz.arg(),
                    self.psi_h[0].arg(),
                    self.psi_h[1].arg(),
                    dx.arg(),
                    dy.arg(),
                    dz.arg(),
                    self.cpml_h.arg(),
                    nx,
                    ny,
                    nz,
                );
                kernels::step_e::launch::<F>(
                    client,
                    cubes(nodes),
                    CubeDim::new_1d(256),
                    ex.arg(),
                    ey.arg(),
                    ez.arg(),
                    hx.arg(),
                    hy.arg(),
                    hz.arg(),
                    cex.arg(),
                    cey.arg(),
                    cez.arg(),
                    self.psi_e[0].arg(),
                    self.psi_e[1].arg(),
                    dxd.arg(),
                    dyd.arg(),
                    dzd.arg(),
                    self.cpml_e.arg(),
                    nx,
                    ny,
                    nz,
                );
                kernels::inject::launch::<F>(
                    client,
                    cubes(plane),
                    CubeDim::new_1d(256),
                    ey.arg(),
                    cey.arg(),
                    self.source.arg(),
                    F::of(p.source_at(n + b)),
                    plane,
                    p.source_plane as u32,
                );
                kernels::project::launch::<F>(
                    client,
                    CubeCount::Static(2, 1, 1),
                    CubeDim::new_1d(reduce),
                    ey.arg(),
                    self.weights.arg(),
                    self.record.arg(),
                    plane,
                    p.planes[0] as u32,
                    p.planes[1] as u32,
                    b as u32,
                    reduce as usize,
                );
            }
            let record = self.record.read(client)?;
            for b in 0..batch {
                let v = [record.get(2 * b), record.get(2 * b + 1)].map(|v| v.copied().unwrap_or(f64::NAN));
                if observe(n + b, v).is_break() {
                    return Ok(n + b + 1);
                }
            }
            n += batch;
        }
        Ok(n)
    }
}

//! The devices: which backends this build carries, what each runs on this machine, and a guard
//! that turns a panic inside a runtime into an error.

use std::panic::{AssertUnwindSafe, catch_unwind};

use emcraft_fdtd::{FdtdError, Result};

use crate::kernels;

emcraft_macros::named_enum! {
    /// Where the kernels run; its id is the name commands use.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Backend {
        /// The CPU, through CubeCL's SIMD compiler (the `cubecl-cpu` feature: for tests and
        /// machines without a GPU; the apps leave it out).
        Cpu = "cpu",
        /// An NVIDIA GPU (the `cuda` feature).
        Cuda = "cuda",
    }
}

impl Backend {
    /// Whether this build carries the backend.
    pub fn built(self) -> bool {
        match self {
            Backend::Cpu => cfg!(feature = "cubecl-cpu"),
            Backend::Cuda => cfg!(feature = "cuda"),
        }
    }

    pub(crate) fn device(self) -> Result<cubecl::Device> {
        match self {
            #[cfg(feature = "cubecl-cpu")]
            Backend::Cpu => Ok(cubecl::Device::Cpu(Default::default())),
            #[cfg(not(feature = "cubecl-cpu"))]
            Backend::Cpu => Err(FdtdError::Unsupported("the GPU kernels on the CPU in this build (built without the cubecl-cpu feature)".into())),
            #[cfg(feature = "cuda")]
            Backend::Cuda => Ok(cubecl::Device::Cuda(Default::default())),
            #[cfg(not(feature = "cuda"))]
            Backend::Cuda => Err(FdtdError::Unsupported("CUDA in this build (built without the cuda feature)".into())),
        }
    }
}

emcraft_macros::named_enum! {
    /// The floating-point type of the fields on the device.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum Precision {
        /// f32: half the memory and traffic of double; ample for S-parameters to 1e-4.
        #[default]
        Single = "single",
        /// f64: for checking single against.
        Double = "double",
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
pub(crate) fn guarded<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|panic| {
        let why = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
        Err(FdtdError::Device(format!("the runtime stopped: {why}")))
    })
}

/// The largest power of two at most `most` units that a cube of `client`'s device holds (a CPU
/// runtime's cube holds as many units as the machine has cores).
pub(crate) fn cube_units(client: &cubecl::prelude::Client, most: u32) -> u32 {
    let most = most.min(client.properties().hardware.max_units_per_cube).max(1);
    1 << (31 - most.leading_zeros())
}

//! The devices: which backends this build carries, what each runs on this machine, and a guard
//! that turns a panic inside a runtime into an error.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use emcraft_fdtd::{FdtdError, Result};

use crate::kernels;

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

    pub(crate) fn device(self) -> Result<cubecl::Device> {
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
pub(crate) fn guarded<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|panic| {
        let why = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
        Err(FdtdError::Device(format!("the runtime stopped: {why}")))
    })
}

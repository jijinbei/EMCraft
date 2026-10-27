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

use std::ops::ControlFlow;

use emcraft_fdtd::{FdtdError, Plan, Result, Stepper};

mod buffer;
mod device;
mod guide;
mod kernels;

pub use device::{Available, Backend, Precision, available};

use crate::device::guarded;
use crate::guide::Run;

/// Steps between reads of the projections.
pub const BATCH: usize = 500;

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

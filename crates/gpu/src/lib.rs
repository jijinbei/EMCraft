//! EMCraft's solvers on a GPU, through kernels written in Rust with CubeCL
//! (docs/adr/0002-gpu.md), on CUDA or on the CPU's SIMD: the time domain's runs (the guide's
//! [`Plan`] and the open box's [`OpenPlan`]) in single or double precision, and the
//! factorization's large fronts ([`Fronts`]) in complex double.
//!
//! **What runs where.** The CPU prepares the plan, transforms the projections (or the ports'
//! voltages and currents) and decides when to stop, as for its own threads (`emcraft_fdtd`);
//! the device holds the fields and runs the step's kernels, the values the host needs kept on
//! it and read back once a batch ([`BATCH`] steps for the guide), so the host waits on the
//! device once a batch.
//!
//! **Determinism.** Every field value is written by one unit, from values of the previous half
//! step, and each projection is summed in a fixed order (each unit a fixed stride, then a tree
//! of fixed pairs): the same plan on the same device and precision gives the same result bit for
//! bit. Devices and precisions differ in rounding (fused multiply–adds, single precision): their
//! results agree with the CPU's to the precision, not bit for bit.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::ops::ControlFlow;

use emcraft_fdtd::{OpenBatch, OpenPlan, OpenStepper, Plan, Result, Stepper};

mod buffer;
mod device;
mod front;
mod guide;
mod kernels;
mod open;

pub use device::{Available, Backend, Precision, available};
pub use front::{Fronts, MAX_ROWS, MIN_ROWS};

use crate::device::guarded;
use crate::guide::Run;
use crate::open::OpenRun;

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

impl OpenStepper for Gpu {
    fn radiate(&self, plan: &OpenPlan, max_steps: usize, observe: &mut dyn FnMut(OpenBatch<'_>) -> ControlFlow<()>) -> Result<usize> {
        let device = self.backend.device()?;
        guarded(|| {
            let client = device.client();
            match self.precision {
                Precision::Single => OpenRun::<f32>::new(&client, plan)?.go(&client, plan, max_steps, observe),
                Precision::Double => OpenRun::<f64>::new(&client, plan)?.go(&client, plan, max_steps, observe),
            }
        })
    }
}

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
mod lattice;
mod open;

pub use device::{Available, Backend, Precision, available};
pub use front::{Fronts, MAX_ROWS, MIN_ROWS};

use cubecl::prelude::Client;

use crate::buffer::Real;
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

/// Work run on a device in the float type the precision picks.
trait OnDevice {
    type Out;
    fn run<F: Real>(self, client: &Client) -> Result<Self::Out>;
}

impl Gpu {
    /// Runs `job` on the device in the chosen precision, a panic in the runtime an error.
    fn dispatch<J: OnDevice>(&self, job: J) -> Result<J::Out> {
        let device = self.backend.device()?;
        guarded(|| {
            let client = device.client();
            match self.precision {
                Precision::Single => job.run::<f32>(&client),
                Precision::Double => job.run::<f64>(&client),
            }
        })
    }
}

/// A guide's leapfrog.
struct Guide<'a, 'o> {
    plan: &'a Plan,
    max_steps: usize,
    observe: &'o mut dyn FnMut(usize, [f64; 2]) -> ControlFlow<()>,
}

impl OnDevice for Guide<'_, '_> {
    type Out = usize;
    fn run<F: Real>(self, client: &Client) -> Result<usize> {
        Run::<F>::new(client, self.plan)?.go(client, self.plan, self.max_steps, self.observe)
    }
}

/// An open box's leapfrog.
struct Open<'a, 'o> {
    plan: &'a OpenPlan,
    max_steps: usize,
    observe: &'o mut dyn FnMut(OpenBatch<'_>) -> ControlFlow<()>,
}

impl OnDevice for Open<'_, '_> {
    type Out = usize;
    fn run<F: Real>(self, client: &Client) -> Result<usize> {
        OpenRun::<F>::new(client, self.plan)?.go(client, self.plan, self.max_steps, self.observe)
    }
}

impl Stepper for Gpu {
    fn leapfrog(&self, plan: &Plan, max_steps: usize, observe: &mut dyn FnMut(usize, [f64; 2]) -> ControlFlow<()>) -> Result<usize> {
        self.dispatch(Guide { plan, max_steps, observe })
    }
}

impl OpenStepper for Gpu {
    fn radiate(&self, plan: &OpenPlan, max_steps: usize, observe: &mut dyn FnMut(OpenBatch<'_>) -> ControlFlow<()>) -> Result<usize> {
        self.dispatch(Open { plan, max_steps, observe })
    }
}

//! The waveguide run on a device: the plan's arrays uploaded once, four kernels a step, the
//! projections read back a batch at a time.

use std::ops::ControlFlow;

use cubecl::prelude::*;
use emcraft_fdtd::{FdtdError, Plan, Result};

use crate::BATCH;
use crate::buffer::{Buf, Real, Xyz};
use crate::kernels::{self, CellsLaunch, ZCpmlLaunch};

/// One half step's arrays: the CPML's ψ of the x and y components and its (b, a) along z.
struct Cpml<F> {
    psi: [Buf<F>; 2],
    ba: Buf<F>,
}

impl<F: Real> Cpml<F> {
    fn new(client: &Client, lens: [usize; 2], ba: &[[f64; 2]]) -> Cpml<F> {
        let flat: Vec<f64> = ba.iter().flatten().copied().collect();
        Cpml { psi: lens.map(|l| Buf::zeros(client, l)), ba: Buf::new(client, &flat) }
    }

    fn arg(&self) -> ZCpmlLaunch<F> {
        ZCpmlLaunch::new(self.psi[0].arg(), self.psi[1].arg(), self.ba.arg())
    }
}

/// The run's arrays on the device.
pub(crate) struct Run<F> {
    n: [u32; 3],
    e: Xyz<F>,
    h: Xyz<F>,
    /// dt/ε and dt/μ.
    ce: Xyz<F>,
    ch: Xyz<F>,
    cpml_e: Cpml<F>,
    cpml_h: Cpml<F>,
    primal: Xyz<F>,
    dual: Xyz<F>,
    source: Buf<F>,
    /// Both planes' projection weights, one after the other.
    weights: Buf<F>,
    /// Each step's two projections, a batch of them.
    record: Buf<F>,
}

/// The most units a projection's cube uses.
const REDUCE: u32 = 256;

/// Units per cube of the field kernels.
const UNITS: u32 = 256;

impl<F: Real> Run<F> {
    pub(crate) fn new(client: &Client, p: &Plan) -> Result<Run<F>> {
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
        let weights: Vec<f64> = p.weights.iter().flatten().copied().collect();
        Ok(Run {
            n: p.n.map(|n| n as u32),
            e: Xyz::zeros(client, lens_e),
            h: Xyz::zeros(client, lens_h),
            ce: Xyz::new(client, &p.e),
            ch: Xyz::new(client, &p.h),
            cpml_e: Cpml::new(client, [lens_e[0], lens_e[1]], &p.cpml_e),
            cpml_h: Cpml::new(client, [lens_h[0], lens_h[1]], &p.cpml_h),
            primal: Xyz::new(client, &p.primal),
            dual: Xyz::new(client, &p.dual),
            source: Buf::new(client, &p.source),
            weights: Buf::new(client, &weights),
            record: Buf::zeros(client, 2 * BATCH),
        })
    }

    fn cells(&self) -> CellsLaunch {
        let [nx, ny, nz] = self.n;
        CellsLaunch::new(nx, ny, nz)
    }

    /// One step, `b` its slot in the batch's record.
    fn step(&self, client: &Client, p: &Plan, n: usize, b: usize, reduce: u32) {
        let [nx, ny, nz] = self.n;
        let nodes = (nx + 1) * (ny + 1) * (nz + 1);
        let plane = (nx + 1) * ny;
        let cubes = |units: u32| CubeCount::Static(units.div_ceil(UNITS), 1, 1);
        let dim = CubeDim::new_1d(UNITS);
        kernels::step_h::launch::<F>(
            client,
            cubes(nodes),
            dim,
            self.h.arg(),
            self.e.arg(),
            self.ch.arg(),
            self.cpml_h.arg(),
            self.primal.arg(),
            self.cells(),
        );
        kernels::step_e::launch::<F>(
            client,
            cubes(nodes),
            dim,
            self.e.arg(),
            self.h.arg(),
            self.ce.arg(),
            self.cpml_e.arg(),
            self.dual.arg(),
            self.cells(),
        );
        let (ey, cey) = (&self.e.0[1], &self.ce.0[1]);
        kernels::inject::launch::<F>(
            client,
            cubes(plane),
            dim,
            ey.arg(),
            cey.arg(),
            self.source.arg(),
            F::of(p.source_at(n)),
            plane,
            p.source_plane as u32,
        );
        let [k1, k2] = p.planes.map(|k| k as u32);
        let width = CubeDim::new_1d(reduce);
        kernels::project::launch::<F>(
            client,
            CubeCount::Static(2, 1, 1),
            width,
            ey.arg(),
            self.weights.arg(),
            self.record.arg(),
            plane,
            k1,
            k2,
            b as u32,
            reduce as usize,
        );
    }

    /// Steps in batches, handing each batch's projections to `observe` in order.
    pub(crate) fn go(
        &self,
        client: &Client,
        p: &Plan,
        max_steps: usize,
        observe: &mut dyn FnMut(usize, [f64; 2]) -> ControlFlow<()>,
    ) -> Result<usize> {
        // A power of two the device's cubes hold.
        let most = REDUCE.min(client.properties().hardware.max_units_per_cube).max(1);
        let reduce = 1 << (31 - most.leading_zeros());
        let mut n = 0;
        while n < max_steps {
            let batch = BATCH.min(max_steps - n);
            for b in 0..batch {
                self.step(client, p, n + b, b, reduce);
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

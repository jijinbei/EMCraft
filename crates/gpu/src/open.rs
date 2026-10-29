//! The open box's run on a device: the plan's arrays uploaded once, five kernels a step (H, the
//! ports' E before the step, E, the sources, the Huygens box's transforms), the ports' voltages
//! and currents and the box's transforms read back a batch at a time.

use std::ops::ControlFlow;

use cubecl::prelude::*;
use emcraft_fdtd::{FdtdError, OpenBatch, OpenPlan, Result};
use num_complex::Complex64;

use crate::buffer::{Buf, Ints, Real, Xyz};
use crate::kernels::open::{self as kernels, PsiLaunch, SamplesLaunch, SourcesLaunch, StepsLaunch, TransformsLaunch};
use crate::lattice::Lattice;
use crate::slots::{Decay, Slots};

/// Each component's two auxiliary variables.
struct Psi<F>([Buf<F>; 6]);

impl<F: Real> Psi<F> {
    fn new(client: &Client, lens: [usize; 3]) -> Psi<F> {
        Psi([0, 0, 1, 1, 2, 2].map(|a| Buf::zeros(client, lens[a])))
    }

    fn arg(&self) -> PsiLaunch<F> {
        let [x0, x1, y0, y1, z0, z1] = &self.0;
        PsiLaunch::new(x0.arg(), x1.arg(), y0.arg(), y1.arg(), z0.arg(), z1.arg())
    }
}

/// The ports' edges and the current elements on the device; one-element stand-ins where there
/// are none.
struct Sources<F> {
    edge_axis: Ints,
    edge_id: Ints,
    k: Buf<F>,
    old: Buf<F>,
    start: Ints,
    port: Buf<F>,
    current_axis: Ints,
    current_id: Ints,
    current: Buf<F>,
    ports: u32,
    currents: u32,
    edges: u32,
}

/// `xs`, or a single `fill` when empty (a device array is never empty).
fn or_one<T: Clone>(xs: Vec<T>, fill: T) -> Vec<T> {
    if xs.is_empty() { vec![fill] } else { xs }
}

impl<F: Real> Sources<F> {
    fn new(client: &Client, p: &OpenPlan) -> Result<Sources<F>> {
        let small = |x: usize| u32::try_from(x).map_err(|_| FdtdError::Invalid { what: "plan".into(), why: "an index does not fit 32 bits".into() });
        let (mut axis, mut id, mut k, mut start, mut port) = (Vec::new(), Vec::new(), Vec::new(), vec![0u32], Vec::new());
        for pp in &p.ports {
            for e in &pp.edges {
                axis.push(small(pp.axis)?);
                id.push(small(e.id)?);
                k.extend([e.ca, e.cb, e.cv]);
            }
            start.push(small(id.len())?);
            port.extend([pp.r_edge, pp.sign, pp.voltage, p.hs.get(pp.axis).copied().unwrap_or(0.0)]);
        }
        let edges = id.len();
        let current_axis = p.currents.iter().map(|c| small(c.axis)).collect::<Result<Vec<_>>>()?;
        let current_id = p.currents.iter().map(|c| small(c.id)).collect::<Result<Vec<_>>>()?;
        let current: Vec<f64> = p.currents.iter().map(|c| c.coefficient).collect();
        Ok(Sources {
            edge_axis: Ints::new(client, &or_one(axis, 0)),
            edge_id: Ints::new(client, &or_one(id, 0)),
            k: Buf::new(client, &or_one(k, 0.0)),
            old: Buf::zeros(client, edges.max(1)),
            start: Ints::new(client, &start),
            port: Buf::new(client, &or_one(port, 0.0)),
            current_axis: Ints::new(client, &or_one(current_axis, 0)),
            current_id: Ints::new(client, &or_one(current_id, 0)),
            current: Buf::new(client, &or_one(current, 0.0)),
            ports: small(p.ports.len())?,
            currents: small(p.currents.len())?,
            edges: small(edges)?,
        })
    }

    fn arg(&self) -> SourcesLaunch<F> {
        SourcesLaunch::new(
            self.edge_axis.arg(),
            self.edge_id.arg(),
            self.k.arg(),
            self.old.arg(),
            self.start.arg(),
            self.port.arg(),
            self.current_axis.arg(),
            self.current_id.arg(),
            self.current.arg(),
            self.ports,
            self.currents,
        )
    }
}

/// The Huygens box's samples and running transforms.
struct Huygens {
    axis: Ints,
    plane: Ints,
    iu: Ints,
    iv: Ints,
    count: u32,
    e: Buf<f64>,
    h: Buf<f64>,
}

impl Huygens {
    fn new(client: &Client, p: &OpenPlan) -> Result<Huygens> {
        let small = |x: usize| u32::try_from(x).map_err(|_| FdtdError::Invalid { what: "plan".into(), why: "a sample does not fit 32 bits".into() });
        let column = |f: fn(&emcraft_fdtd::SamplePlan) -> usize| -> Result<Ints> {
            Ok(Ints::new(client, &or_one(p.samples.iter().map(|s| small(f(s))).collect::<Result<Vec<_>>>()?, 0)))
        };
        let len = 6 * p.samples.len() * p.frequencies.len();
        Ok(Huygens {
            axis: column(|s| s.axis)?,
            plane: column(|s| s.plane)?,
            iu: column(|s| s.iu)?,
            iv: column(|s| s.iv)?,
            count: small(p.samples.len())?,
            e: Buf::zeros(client, len.max(1)),
            h: Buf::zeros(client, len.max(1)),
        })
    }
}

/// The run's arrays on the device.
pub(crate) struct OpenRun<F> {
    grid: Lattice,
    e: Xyz<F>,
    h: Xyz<F>,
    ce: Xyz<F>,
    ch: Xyz<F>,
    poles: Slots<F>,
    decay: Decay<F>,
    psi_e: Psi<F>,
    psi_h: Psi<F>,
    cpml_e: Xyz<F>,
    cpml_h: Xyz<F>,
    sources: Sources<F>,
    huygens: Huygens,
    /// Each step's (voltage, current) of each port, a batch of them.
    record: Buf<F>,
}

impl<F: Real> OpenRun<F> {
    pub(crate) fn new(client: &Client, p: &OpenPlan) -> Result<OpenRun<F>> {
        let grid = Lattice::new(client, p.n)?;
        let (lens_e, lens_h) = (grid.e, grid.h);
        let agree = p.ce.iter().zip(lens_e).all(|(c, l)| c.len() == l)
            && p.ch.iter().zip(lens_h).all(|(c, l)| c.len() == l)
            && Slots::<F>::fit(p.poles.as_ref(), lens_e)
            && Decay::<F>::fit(&p.h_decay, lens_h)
            && p.cpml_e.iter().zip(p.n).all(|(c, n)| c.len() == n + 1)
            && p.cpml_h.iter().zip(p.n).all(|(c, n)| c.len() == n)
            && p.ports.iter().all(|q| q.axis < 3 && q.edges.iter().all(|e| e.id < lens_e[q.axis]) && !q.edges.is_empty())
            && p.currents.iter().all(|c| c.axis < 3 && c.id < lens_e[c.axis])
            && p.samples
                .iter()
                .all(|s| s.axis < 3 && s.plane >= 1 && s.plane < p.n[s.axis] && s.iu + 1 < p.n[(s.axis + 1) % 3] && s.iv + 1 < p.n[(s.axis + 2) % 3])
            && p.batch > 0;
        if !agree {
            return Err(FdtdError::Invalid { what: "plan".into(), why: "its arrays do not match its grid".into() });
        }
        let profile = |c: &[Vec<[f64; 2]>; 3]| c.each_ref().map(|c| c.iter().flatten().copied().collect::<Vec<f64>>());
        Ok(OpenRun {
            grid,
            e: Xyz::zeros(client, lens_e),
            h: Xyz::zeros(client, lens_h),
            ce: Xyz::new(client, &p.ce),
            ch: Xyz::new(client, &p.ch),
            poles: Slots::new(client, p.poles.as_ref(), p.dt),
            decay: Decay::new(client, &p.h_decay),
            psi_e: Psi::new(client, lens_e),
            psi_h: Psi::new(client, lens_h),
            cpml_e: Xyz::new(client, &profile(&p.cpml_e)),
            cpml_h: Xyz::new(client, &profile(&p.cpml_h)),
            sources: Sources::new(client, p)?,
            huygens: Huygens::new(client, p)?,
            record: Buf::zeros(client, (2 * p.batch * p.ports.len()).max(1)),
        })
    }

    /// One step, `b` its slot in the batch's record and `phasors` the batch's.
    fn step(&self, client: &Client, p: &OpenPlan, n: usize, b: usize, phasors: &Buf<f64>) {
        let (nodes, dim) = self.grid.over_nodes();
        let steps = || StepsLaunch::new(F::of(p.hs[0]), F::of(p.hs[1]), F::of(p.hs[2]));
        kernels::step_h::launch::<F>(
            client,
            nodes.clone(),
            dim,
            self.h.arg(),
            self.e.arg(),
            self.psi_h.arg(),
            self.cpml_h.arg(),
            steps(),
            self.ch.arg(),
            self.grid.cells(),
            self.decay.h.arg(),
            self.decay.any,
        );
        if self.sources.edges > 0 {
            let (edges, dim) = self.grid.over(self.sources.edges);
            kernels::remember::launch::<F>(client, edges, dim, self.e.arg(), self.sources.arg(), self.sources.edges);
        }
        kernels::step_e::launch::<F>(
            client,
            nodes,
            dim,
            self.e.arg(),
            self.h.arg(),
            self.ce.arg(),
            self.psi_e.arg(),
            self.cpml_e.arg(),
            steps(),
            self.grid.cells(),
            self.poles.arg(),
            self.poles.any,
        );
        let single = CubeCount::Static(1, 1, 1);
        kernels::drive::launch::<F>(
            client,
            single,
            CubeDim::new_1d(1),
            self.e.arg(),
            self.ce.arg(),
            self.sources.arg(),
            self.record.arg(),
            F::of(p.source_at(n)),
            b as u32,
            self.poles.arg(),
            self.poles.any,
        );
        let hy = &self.huygens;
        let samples = SamplesLaunch::new(hy.axis.arg(), hy.plane.arg(), hy.iu.arg(), hy.iv.arg(), hy.count);
        let transforms = TransformsLaunch::new(phasors.arg(), hy.e.arg(), hy.h.arg(), p.frequencies.len() as u32);
        let (across, dim) = self.grid.over(hy.count);
        kernels::transform::launch::<F>(client, across, dim, self.e.arg(), self.h.arg(), samples, transforms, b as u32, self.grid.cells());
    }

    /// Steps in batches of `p.batch`, handing each to `observe`.
    pub(crate) fn go(
        &self,
        client: &Client,
        p: &OpenPlan,
        max_steps: usize,
        observe: &mut dyn FnMut(OpenBatch<'_>) -> ControlFlow<()>,
    ) -> Result<usize> {
        let ports = p.ports.len();
        let mut n = 0;
        while n < max_steps {
            let batch = p.batch.min(max_steps - n);
            let phasors = Buf::<f64>::new(client, &or_one(phasors(p, n, batch), 0.0));
            for b in 0..batch {
                self.step(client, p, n + b, b, &phasors);
            }
            let record = self.record.read(client)?;
            let vi: Vec<[f64; 2]> = (0..batch * ports).map(|q| [0, 1].map(|c| record.get(2 * q + c).copied().unwrap_or(f64::NAN))).collect();
            let (e, h) = (self.huygens.e.read(client)?, self.huygens.h.read(client)?);
            let flow = observe(OpenBatch { first: n, steps: batch, ports: &vi, e: &e, h: &h });
            n += batch;
            if flow.is_break() {
                break;
            }
        }
        Ok(n)
    }
}

/// The phasors Δt e^{−jωt} of steps `first..first + count`: per step, per frequency, (re, im)
/// at E's time (n + 1) Δt, then at H's (n + ½) Δt.
fn phasors(p: &OpenPlan, first: usize, count: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(4 * count * p.frequencies.len());
    for n in first..first + count {
        let (t_e, t_h) = ((n as f64 + 1.0) * p.dt, (n as f64 + 0.5) * p.dt);
        for &f in &p.frequencies {
            let w = 2.0 * std::f64::consts::PI * f;
            let (pe, ph) = (Complex64::from_polar(p.dt, -w * t_e), Complex64::from_polar(p.dt, -w * t_h));
            out.extend([pe.re, pe.im, ph.re, ph.im]);
        }
    }
    out
}

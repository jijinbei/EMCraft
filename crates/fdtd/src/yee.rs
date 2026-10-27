//! The machinery both time-domain solvers share: the six staggered field components of the
//! Yee cell, the convolutional PML's grading (Roden & Gedney 2000), the Gaussian-modulated
//! pulse, the stability limit of the time step, running Fourier transforms, and the threads
//! that step the fields slab by slab.
//!
//! **Threads.** Each thread owns a slab of z planes for the whole run and updates only the
//! values in it; barriers order the H half step, the E half step and the serial work of thread
//! 0 (sources, transforms, checks). Every value is computed by one thread in one fixed order,
//! so the result does not depend on how many threads there are.

use std::f64::consts::PI;
use std::ops::{ControlFlow, Range};
use std::sync::Barrier;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use emcraft_model::units::{C0, EPS0, ETA0};
use num_complex::Complex64 as C;

/// A field component's array on its own lattice, row-major in x. Values are stored as atomics
/// so the threads of a run can share them without `unsafe`: each thread writes only its own
/// slabs of z, and the barriers between half steps order every write before the reads of the
/// next half step (relaxed loads and stores compile to plain moves).
pub(crate) struct Field {
    dims: [usize; 3],
    v: Vec<AtomicU64>,
}

impl Field {
    /// Zeros on a lattice of `dims` points.
    pub(crate) fn new(dims: [usize; 3]) -> Field {
        Field { dims, v: (0..dims[0] * dims[1] * dims[2]).map(|_| AtomicU64::new(0)).collect() }
    }

    /// Zeros on the same lattice (a CPML's auxiliary variables).
    pub(crate) fn zeros_like(&self) -> Field {
        Field::new(self.dims)
    }

    pub(crate) fn dims(&self) -> [usize; 3] {
        self.dims
    }

    /// The index of point (i, j, k).
    #[inline]
    pub(crate) fn at(&self, i: usize, j: usize, k: usize) -> usize {
        i + self.dims[0] * (j + self.dims[1] * k)
    }

    #[inline]
    pub(crate) fn get(&self, id: usize) -> f64 {
        f64::from_bits(self.v[id].load(Ordering::Relaxed))
    }

    #[inline]
    pub(crate) fn set(&self, id: usize, x: f64) {
        self.v[id].store(x.to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn len(&self) -> usize {
        self.v.len()
    }
}

/// The six components on a grid of `n` cells: E_a on the primal edges along axis a (a half
/// cell along a, nodes across), H_a on the dual edges (nodes along a, half cells across).
pub(crate) struct Fields {
    pub(crate) e: [Field; 3],
    pub(crate) h: [Field; 3],
}

impl Fields {
    pub(crate) fn new(n: [usize; 3]) -> Fields {
        let along = |a: usize, on: usize, off: usize| -> [usize; 3] { [0, 1, 2].map(|d| n[d] + if d == a { on } else { off }) };
        Fields { e: [0, 1, 2].map(|a| Field::new(along(a, 0, 1))), h: [0, 1, 2].map(|a| Field::new(along(a, 1, 0))) }
    }
}

/// A CPML's update of one auxiliary variable: ψⁿ⁺¹ = b ψⁿ + a ∂ (b = 1, a = 0 outside it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Cpml {
    pub(crate) b: f64,
    pub(crate) a: f64,
}

impl Cpml {
    /// No absorption.
    pub(crate) const NONE: Cpml = Cpml { b: 1.0, a: 0.0 };

    /// The new ψ from the old and the derivative it absorbs.
    #[inline]
    pub(crate) fn psi(self, psi: f64, derivative: f64) -> f64 {
        self.b * psi + self.a * derivative
    }
}

/// The CPML's polynomial grading: σ rises as the cube of the depth to 0.8 (m + 1)/(η₀ h), the
/// complex frequency shift α falls linearly from 5 % of 2π f_c ε₀ (Roden & Gedney 2000).
pub(crate) struct Grading {
    sigma_max: f64,
    alpha_max: f64,
    dt: f64,
}

/// The grading's polynomial order.
const ORDER: f64 = 3.0;

impl Grading {
    /// The grading of a layer of cells of `h` for a time step `dt`, centred on `fc`.
    pub(crate) fn new(h: f64, dt: f64, fc: f64) -> Grading {
        Grading { sigma_max: 0.8 * (ORDER + 1.0) / (ETA0 * h), alpha_max: 2.0 * PI * fc * EPS0 * 0.05, dt }
    }

    /// The coefficients at depth `d` into the layer, a fraction of its thickness (≤ 0 outside).
    pub(crate) fn at(&self, d: f64) -> Cpml {
        if d <= 0.0 {
            return Cpml::NONE;
        }
        let sigma = self.sigma_max * d.powf(ORDER);
        let alpha = self.alpha_max * (1.0 - d);
        let b = (-(sigma + alpha) * self.dt / EPS0).exp();
        let a = if sigma > 0.0 { sigma / (sigma + alpha) * (b - 1.0) } else { 0.0 };
        Cpml { b, a }
    }
}

/// The largest stable time step (times `courant`) on a grid whose smallest steps are `h`.
pub(crate) fn time_step(courant: f64, h: [f64; 3]) -> f64 {
    courant / (C0 * (1.0 / h[0].powi(2) + 1.0 / h[1].powi(2) + 1.0 / h[2].powi(2)).sqrt())
}

/// A Gaussian-modulated sine: e^{−((t − t₀)/τ)²} sin(2π f_c (t − t₀)).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pulse {
    pub(crate) t0: f64,
    pub(crate) tau: f64,
    pub(crate) fc: f64,
}

impl Pulse {
    #[inline]
    pub(crate) fn at(&self, t: f64) -> f64 {
        (-((t - self.t0) / self.tau).powi(2)).exp() * (2.0 * PI * self.fc * (t - self.t0)).sin()
    }
}

/// Running Fourier transforms Σₙ vₙ e^{−jω tₙ} of `N` signals sampled every step (tₙ = n Δt
/// from n = 1), the phasors advanced by multiplication each step.
pub(crate) struct RunningDft<const N: usize> {
    advance: Vec<C>,
    phasor: Vec<C>,
    sums: [Vec<C>; N],
}

impl<const N: usize> RunningDft<N> {
    pub(crate) fn new(freqs: &[f64], dt: f64) -> RunningDft<N> {
        let advance: Vec<C> = freqs.iter().map(|f| C::from_polar(1.0, -2.0 * PI * f * dt)).collect();
        RunningDft { phasor: advance.clone(), sums: std::array::from_fn(|_| vec![C::new(0.0, 0.0); freqs.len()]), advance }
    }

    /// Adds the next sample of each signal.
    pub(crate) fn push(&mut self, values: [f64; N]) {
        for (f, (p, a)) in self.phasor.iter_mut().zip(&self.advance).enumerate() {
            for (sum, v) in self.sums.iter_mut().zip(values) {
                sum[f] += *p * v;
            }
            *p *= a;
        }
    }

    /// The transforms so far, one per signal.
    pub(crate) fn sums(&self) -> &[Vec<C>; N] {
        &self.sums
    }

    pub(crate) fn into_sums(self) -> [Vec<C>; N] {
        self.sums
    }
}

/// The two half steps of a leapfrog over a slab of z planes.
pub(crate) trait Kernel: Sync {
    /// H from n − ½ to n + ½ on the planes `ks`.
    fn step_h(&self, ks: Range<usize>);
    /// E from n to n + 1 on the planes `ks`.
    fn step_e(&self, ks: Range<usize>);
}

/// The most threads a run uses.
const MAX_THREADS: usize = 16;

/// Leapfrogs `kernel` over `planes` z planes for up to `max_steps` steps; after each step
/// thread 0 runs `after(step)` alone (sources, transforms, checks), which breaks to stop.
/// Returns the number of steps taken.
pub(crate) fn leapfrog(kernel: &impl Kernel, planes: usize, max_steps: usize, mut after: impl FnMut(usize) -> ControlFlow<()>) -> usize {
    if max_steps == 0 {
        return 0;
    }
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(MAX_THREADS).min(planes / 4).max(1);
    let slabs: Vec<Range<usize>> = (0..threads).map(|t| t * planes / threads..(t + 1) * planes / threads).collect();
    let barrier = Barrier::new(threads);
    let done = AtomicBool::new(false);
    let half_steps = |ks: &Range<usize>| {
        kernel.step_h(ks.clone());
        barrier.wait();
        kernel.step_e(ks.clone());
        barrier.wait();
    };
    let mut steps = 0;
    std::thread::scope(|scope| {
        for ks in slabs.iter().skip(1) {
            let (half_steps, barrier, done) = (&half_steps, &barrier, &done);
            scope.spawn(move || {
                loop {
                    half_steps(ks);
                    // Thread 0 runs `after`.
                    barrier.wait();
                    if done.load(Ordering::Relaxed) {
                        break;
                    }
                }
            });
        }
        let ks0 = slabs.first().cloned().unwrap_or(0..0);
        for n in 0..max_steps {
            half_steps(&ks0);
            steps = n + 1;
            if after(n).is_break() || n + 1 == max_steps {
                done.store(true, Ordering::Relaxed);
            }
            barrier.wait();
            if done.load(Ordering::Relaxed) {
                break;
            }
        }
    });
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_staggered() {
        let f = Fields::new([2, 3, 4]);
        assert_eq!(f.e.each_ref().map(Field::dims), [[2, 4, 5], [3, 3, 5], [3, 4, 4]]);
        assert_eq!(f.h.each_ref().map(Field::dims), [[3, 3, 4], [2, 4, 4], [2, 3, 5]]);
    }

    #[test]
    fn the_leapfrog_takes_every_step_on_every_plane() {
        struct Count(Vec<AtomicU64>);
        impl Kernel for Count {
            fn step_h(&self, ks: Range<usize>) {
                for k in ks {
                    self.0[k].fetch_add(1, Ordering::Relaxed);
                }
            }
            fn step_e(&self, _: Range<usize>) {}
        }
        let c = Count((0..40).map(|_| AtomicU64::new(0)).collect());
        assert_eq!(leapfrog(&c, 40, 7, |_| ControlFlow::Continue(())), 7);
        assert!(c.0.iter().all(|n| n.load(Ordering::Relaxed) == 7));
        assert_eq!(leapfrog(&c, 40, 7, |n| if n == 2 { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }), 3);
        assert_eq!(leapfrog(&c, 40, 0, |_| ControlFlow::Continue(())), 0);
    }
}

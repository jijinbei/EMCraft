//! The dispersive and lossy edges' slots and terms on the device (`emcraft_fdtd::Poles`), and
//! the H faces' decay with magnetic losses, shared by the guide's run and the open box's.

use cubecl::prelude::*;

use crate::buffer::{Buf, Ints, Real, Xyz};
use crate::kernels::PolesLaunch;

/// The slotted edges, their terms and the terms' states; one-element stand-ins when there are
/// none (the kernels, compiled without them, do not read them).
pub(crate) struct Slots<F> {
    slot: [Ints; 3],
    eps: Buf<F>,
    loss: Buf<F>,
    start: Ints,
    kind: Ints,
    k: Buf<F>,
    p: Buf<F>,
    prev: Buf<F>,
    dt: f64,
    pub(crate) any: bool,
}

impl<F: Real> Slots<F> {
    pub(crate) fn new(client: &Client, poles: Option<&emcraft_fdtd::Poles>, dt: f64) -> Slots<F> {
        match poles {
            Some(p) => {
                let k: Vec<f64> = p.k.iter().flatten().copied().collect();
                Slots {
                    slot: p.slot.each_ref().map(|s| Ints::new(client, s)),
                    eps: Buf::new(client, &p.eps),
                    loss: Buf::new(client, &p.loss),
                    start: Ints::new(client, &p.start),
                    kind: Ints::new(client, &p.kind),
                    k: Buf::new(client, &k),
                    p: Buf::zeros(client, p.kind.len().max(1)),
                    prev: Buf::zeros(client, p.kind.len().max(1)),
                    dt,
                    any: true,
                }
            }
            None => {
                let none = Ints::new(client, &[u32::MAX]);
                let zero = || Buf::zeros(client, 1);
                Slots {
                    slot: [none.clone(), none.clone(), none],
                    eps: zero(),
                    loss: zero(),
                    start: Ints::new(client, &[0, 0]),
                    kind: Ints::new(client, &[0]),
                    k: zero(),
                    p: zero(),
                    prev: zero(),
                    dt,
                    any: false,
                }
            }
        }
    }

    pub(crate) fn arg(&self) -> PolesLaunch<F> {
        let [x, y, z] = &self.slot;
        PolesLaunch::new(
            x.arg(),
            y.arg(),
            z.arg(),
            self.eps.arg(),
            self.loss.arg(),
            self.start.arg(),
            self.kind.arg(),
            self.k.arg(),
            self.p.arg(),
            self.prev.arg(),
            F::of(self.dt),
        )
    }
}

/// The H faces' decay with magnetic losses (one-element stand-ins without).
pub(crate) struct Decay<F> {
    pub(crate) h: Xyz<F>,
    pub(crate) any: bool,
}

impl<F: Real> Decay<F> {
    pub(crate) fn new(client: &Client, decay: &[Vec<f64>; 3]) -> Decay<F> {
        if decay.iter().all(Vec::is_empty) {
            return Decay { h: Xyz::new(client, &[vec![1.0], vec![1.0], vec![1.0]]), any: false };
        }
        Decay { h: Xyz::new(client, decay), any: true }
    }
}

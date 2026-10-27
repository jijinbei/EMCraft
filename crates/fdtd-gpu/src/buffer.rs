//! Arrays on a device: the float types the kernels run in and typed buffers made into kernel
//! arguments without `unsafe` (each argument has its array's own length).

use std::marker::PhantomData;

use cubecl::prelude::*;
use cubecl::server::Handle;
use cubecl::std::tensor::TensorHandle;
use emcraft_fdtd::{FdtdError, Result};

use crate::kernels::XyzLaunch;

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
    _f: PhantomData<F>,
}

impl<F: Real> Buf<F> {
    pub(crate) fn new(client: &Client, xs: &[f64]) -> Buf<F> {
        let v: Vec<F> = xs.iter().map(|&x| F::of(x)).collect();
        Buf { handle: client.create_from_slice(F::as_bytes(&v)), len: v.len(), _f: PhantomData }
    }

    pub(crate) fn zeros(client: &Client, len: usize) -> Buf<F> {
        Buf::new(client, &vec![0.0; len])
    }

    /// The array as a kernel argument; its length is the array's own.
    pub(crate) fn arg(&self) -> BufferArg {
        TensorHandle::new_contiguous([self.len], self.handle.clone(), cubecl::ir::ElemType::Float(F::KIND)).binding().into_buffer_arg()
    }

    pub(crate) fn read(&self, client: &Client) -> Result<Vec<f64>> {
        let bytes = client.read_one(self.handle.clone()).map_err(|e| FdtdError::Device(format!("{e:?}")))?;
        Ok(F::from_bytes(&bytes).iter().map(|x| x.wide()).collect())
    }
}

/// An array of indices on the device.
#[derive(Clone)]
pub(crate) struct Ints {
    handle: Handle,
    len: usize,
}

impl Ints {
    pub(crate) fn new(client: &Client, xs: &[u32]) -> Ints {
        Ints { handle: client.create_from_slice(u32::as_bytes(xs)), len: xs.len() }
    }

    pub(crate) fn arg(&self) -> BufferArg {
        TensorHandle::new_contiguous([self.len], self.handle.clone(), cubecl::ir::ElemType::UInt(cubecl::ir::UIntKind::U32))
            .binding()
            .into_buffer_arg()
    }
}

/// One array per axis: a field's three components, their coefficients, the grid's steps.
pub(crate) struct Xyz<F>(pub(crate) [Buf<F>; 3]);

impl<F: Real> Xyz<F> {
    pub(crate) fn new(client: &Client, arrays: &[Vec<f64>; 3]) -> Xyz<F> {
        Xyz(arrays.each_ref().map(|a| Buf::new(client, a)))
    }

    pub(crate) fn zeros(client: &Client, lens: [usize; 3]) -> Xyz<F> {
        Xyz(lens.map(|l| Buf::zeros(client, l)))
    }

    pub(crate) fn arg(&self) -> XyzLaunch<F> {
        let [x, y, z] = &self.0;
        XyzLaunch::new(x.arg(), y.arg(), z.arg())
    }
}

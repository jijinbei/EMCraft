//! The field's colour map on its plane, evaluated by the GPU: the plane's phasors uploaded once
//! (the real and imaginary parts of E or H at each sample), each frame only the phase and the
//! colour scale. The fragment shader does what `scene3d::field::field_plane` does on the CPU —
//! cells of `step` samples a side coloured by the mean of their corners' values at the phase,
//! cells with a corner outside the mesh left out, the diverging map for a component and the
//! sequential one for |E| or |H| — so the two pictures agree.
//!
//! The scene keeps the plane as one quad (its tag, for picking and the outline); the packer marks
//! its triangles ([`Packed::with_field`](super::Packed::with_field)) and the shader colours them.

use emcraft_render::colormap;

/// The plane's phasors and where it lies.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldGrid {
    /// What the grid was made from (the samples and the component), to upload it once.
    pub id: u64,
    pub nx: usize,
    pub ny: usize,
    /// Samples per cell along each side (as the CPU's cells).
    pub step: usize,
    /// The component: 0, 1, 2 for x, y, z; 3 for the vector's length.
    pub axis: u32,
    /// Real and imaginary parts at each sample (row 0 at the plane's low v side), NaN outside
    /// the mesh.
    pub re: Vec<[f64; 3]>,
    pub im: Vec<[f64; 3]>,
    /// The plane's corner at (u, v) = (0, 0) and its corners at (1, 0) and (0, 1) (m).
    pub corner: [f64; 3],
    pub u_end: [f64; 3],
    pub v_end: [f64; 3],
}

/// Most cells along the field plane's longer side (`scene3d::field`'s).
pub const FIELD_CELLS: usize = 120;

impl FieldGrid {
    /// The grid of `phasors` (from `PlaneSamples::phasors`), `nx` × `ny`, for `axis`, over the
    /// plane from `corner` to `u_end` and `v_end`.
    pub fn new(
        id: u64,
        nx: usize,
        ny: usize,
        axis: u32,
        phasors: &[Option<([f64; 3], [f64; 3])>],
        corner: [f64; 3],
        u_end: [f64; 3],
        v_end: [f64; 3],
    ) -> FieldGrid {
        let nan = [f64::NAN; 3];
        let (re, im) = (0..nx * ny).map(|k| phasors.get(k).copied().flatten().unwrap_or((nan, nan))).unzip();
        FieldGrid { id, nx, ny, step: nx.max(ny).div_ceil(FIELD_CELLS).max(1), axis: axis.min(3), re, im, corner, u_end, v_end }
    }

    /// The value at sample `k` at the phase with cos φ = `c`, sin φ = `s`: Re(F e^{jφ}).
    pub fn value(&self, k: usize, c: f64, s: f64) -> f64 {
        let (Some(re), Some(im)) = (self.re.get(k), self.im.get(k)) else { return f64::NAN };
        let v: [f64; 3] = std::array::from_fn(|i| re[i] * c - im[i] * s);
        match self.axis {
            3 => (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt(),
            a => v[a as usize],
        }
    }

    /// The largest |value| at the phase (the CPU's colour scale for one plane).
    pub fn peak(&self, phase: f64) -> f64 {
        let (s, c) = phase.sin_cos();
        (0..self.nx * self.ny).map(|k| self.value(k, c, s)).filter(|v| v.is_finite()).fold(0.0, |a, b| a.max(b.abs()))
    }

    /// The textures' texels: real parts (xyz, w 1 inside the mesh) and imaginary parts.
    pub fn texels(&self) -> (Vec<u8>, Vec<u8>) {
        let mut re = Vec::with_capacity(16 * self.re.len());
        let mut im = Vec::with_capacity(16 * self.im.len());
        for (r, i) in self.re.iter().zip(&self.im) {
            let inside = r.iter().chain(i).all(|v| v.is_finite());
            let f = |v: f64| if inside && v.is_finite() { v as f32 } else { 0.0 };
            for v in [f(r[0]), f(r[1]), f(r[2]), if inside { 1.0 } else { 0.0 }] {
                re.extend_from_slice(&v.to_le_bytes());
            }
            for v in [f(i[0]), f(i[1]), f(i[2]), 0.0] {
                im.extend_from_slice(&v.to_le_bytes());
            }
        }
        (re, im)
    }
}

/// The field at one phase, for one frame.
#[derive(Clone, Debug)]
pub struct FieldFrame {
    pub grid: std::sync::Arc<FieldGrid>,
    pub phase: f64,
    /// The value drawn at full colour.
    pub peak: f64,
}

/// Bytes of the field's uniform (`scene.wgsl`'s `F`).
pub const FIELD_BYTES: usize = 10 * 16;

impl FieldFrame {
    /// The uniform for a scene packed round `origin`: the plane's frame, the grid, the phase,
    /// the scale and the colour map's five stops.
    pub fn bytes(&self, origin: [f64; 3]) -> Vec<u8> {
        let g = &self.grid;
        let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        // u = (p − corner) · U with U = e / |e|², so the far corner reads 1.
        let dual = |e: [f64; 3]| {
            let n = e[0] * e[0] + e[1] * e[1] + e[2] * e[2];
            if n > 0.0 && n.is_finite() { e.map(|v| v / n) } else { [0.0; 3] }
        };
        let o = sub(g.corner, origin);
        let (u, v) = (dual(sub(g.u_end, g.corner)), dual(sub(g.v_end, g.corner)));
        let (s, c) = self.phase.sin_cos();
        let inv = if self.peak > 0.0 && self.peak.is_finite() { 1.0 / self.peak } else { 0.0 };
        let signed = g.axis < 3;
        let stops: Vec<[f64; 4]> = [-1.0, -0.5, 0.0, 0.5, 1.0]
            .iter()
            .map(|&t| if signed { colormap::diverging(t) } else { colormap::sequential(0.5 * (t + 1.0)) })
            .map(|c| [f64::from(c.0) / 255.0, f64::from(c.1) / 255.0, f64::from(c.2) / 255.0, 1.0])
            .collect();
        let mut out = Vec::with_capacity(FIELD_BYTES);
        let rows: Vec<[f64; 4]> = [
            [o[0], o[1], o[2], 0.0],
            [u[0], u[1], u[2], 0.0],
            [v[0], v[1], v[2], 0.0],
            [g.nx as f64, g.ny as f64, g.step as f64, f64::from(g.axis)],
            [c, s, inv, if signed { 1.0 } else { 0.0 }],
        ]
        .into_iter()
        .chain(stops)
        .collect();
        for row in &rows {
            for x in row {
                let x = *x as f32;
                out.extend_from_slice(&(if x.is_finite() { x } else { 0.0 }).to_le_bytes());
            }
        }
        out
    }
}

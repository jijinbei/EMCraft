//! The materials of the tetrahedra: each one's complex permittivity εr(1 − j tan δ) and
//! permeability μr(1 − j tan δ_m), and for a dispersive material (Debye, Lorentz or Drude poles)
//! its εr(ω) at each frequency instead.

use emcraft_model::{Material, Mesh, Model};
use num_complex::Complex64 as C;

use super::{Result, Slot, invalid};

#[derive(Clone, Debug)]
pub(crate) struct Media {
    /// Per tetrahedron, εr(1 − j tan δ) (for a dispersive one, see `dispersive_of`).
    eps: Vec<C>,
    /// Per tetrahedron, μr(1 − j tan δ_m).
    mu: Vec<C>,
    /// Per tetrahedron, its material's index into `dispersive` (none if not dispersive).
    dispersive_of: Vec<Slot>,
    /// The dispersive materials, in the model's order (deterministic).
    dispersive: Vec<Material>,
}

impl Media {
    pub(crate) fn new(model: &Model, mesh: &Mesh) -> Result<Media> {
        let n = mesh.tets.len();
        let mut index = vec![Slot::NONE; model.materials.len()];
        let mut dispersive = Vec::new();
        for (i, m) in model.materials.iter().enumerate().filter(|(_, m)| m.is_dispersive()) {
            index[i] = Slot::new(dispersive.len());
            dispersive.push(m.clone());
        }
        let (mut eps, mut mu, mut dispersive_of) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
        for t in 0..n {
            let k = mesh.tet_material.get(t).copied();
            let m = k.and_then(|k| model.materials.get(k as usize)).ok_or_else(|| invalid("mesh", format!("tetrahedron {t} has no material")))?;
            eps.push(C::new(m.eps_r.get(), -m.eps_r.get() * m.loss_tangent.get()));
            mu.push(m.mu_c());
            dispersive_of.push(k.and_then(|k| index.get(k as usize)).copied().unwrap_or(Slot::NONE));
        }
        Ok(Media { eps, mu, dispersive_of, dispersive })
    }

    /// The dispersive materials: those whose permittivity depends on the frequency.
    pub(crate) fn dispersive_materials(&self) -> &[Material] {
        &self.dispersive
    }

    /// Tetrahedron `t`'s dispersive material (an index into [`Media::dispersive_materials`]),
    /// if it has one.
    pub(crate) fn dispersive_of(&self, t: usize) -> Option<usize> {
        self.dispersive_of.get(t).and_then(|d| d.get())
    }

    /// Tetrahedron `t`'s εr(1 − j tan δ), whatever the frequency (a dispersive material's
    /// static fields).
    pub(crate) fn eps(&self, t: usize) -> C {
        self.eps.get(t).copied().unwrap_or(C::new(1.0, 0.0))
    }

    /// Tetrahedron `t`'s complex relative permittivity at `frequency` (Hz).
    pub(crate) fn eps_at(&self, t: usize, frequency: f64) -> C {
        match self.dispersive_of(t).and_then(|d| self.dispersive.get(d)) {
            Some(m) => m.eps_at(frequency),
            None => self.eps(t),
        }
    }

    /// Tetrahedron `t`'s complex relative permeability μr(1 − j tan δ_m).
    pub(crate) fn mu(&self, t: usize) -> C {
        self.mu.get(t).copied().unwrap_or(C::new(1.0, 0.0))
    }

    /// Whether any tetrahedron's permeability has losses.
    pub(crate) fn magnetically_lossy(&self) -> bool {
        self.mu.iter().any(|m| m.im != 0.0)
    }
}

/// s/μ for a complex μ: s/μr exactly (a real quotient) when the permeability is lossless.
pub(crate) fn per_mu(s: f64, mu: C) -> C {
    if mu.im == 0.0 { C::new(s / mu.re, 0.0) } else { s / mu }
}

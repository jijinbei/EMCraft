//! `mesh.generate`: the structured waveguide grid or the unstructured mesh of a solid.

use emcraft_model::units::C0;
use serde_json::{Map, Value, json};

use crate::{Change, Params, Result, Session};

/// The element sizes asked for: one default, and per material (by index) when sized by
/// wavelength.
pub(crate) struct Sizes {
    pub default: f64,
    pub per_material: Vec<Option<f64>>,
}

impl Sizes {
    /// `per_wavelength` elements per wavelength at `frequency` (the band's top by default) in
    /// each material, at most `size`; or `size` alone.
    pub(crate) fn read(s: &Session, p: &Params) -> Result<Sizes> {
        let size = p.positive_length("size").optional()?;
        let Some(n) = p.number("per_wavelength").check(|n| (1.0..=1000.0).contains(n), "between 1 and 1000").optional()? else {
            let default = p.positive_length("size").required_or("or `per_wavelength` with `frequency`")?;
            return Ok(Sizes { default, per_material: Vec::new() });
        };
        let f = crate::args::frequency_or_band_top(s, p, "frequency", "the frequency the mesh must resolve, or the band: project.frequency")?;
        let per_material: Vec<Option<f64>> = s
            .model
            .materials
            .iter()
            .map(|m| {
                let h = C0 / (f * m.index_at(f)) / n;
                Some(size.map_or(h, |c| c.min(h)))
            })
            .collect();
        let default = per_material.first().copied().flatten().unwrap_or(f64::NAN);
        Ok(Sizes { default, per_material })
    }

    fn by_wavelength(&self) -> bool {
        !self.per_material.is_empty()
    }

    /// A structured grid's one size: the smallest the sections' materials ask for.
    pub(crate) fn grid_size(&self, s: &Session) -> Result<f64> {
        match &s.model.waveguide {
            Some(w) if self.by_wavelength() => w.sections.iter().try_fold(f64::INFINITY, |h, sec| {
                let (m, _) = s.model.material(&sec.material)?;
                Ok::<_, crate::EngineError>(h.min(self.per_material.get(m).copied().flatten().unwrap_or(self.default)))
            }),
            _ => Ok(self.default),
        }
    }

    /// Each material's size, in display units, for the reply.
    fn reply(&self, s: &Session) -> Value {
        let sizes: Map<String, Value> =
            s.model.materials.iter().zip(&self.per_material).filter_map(|(m, h)| Some((m.name.clone(), json!(s.show_length((*h)?))))).collect();
        Value::Object(sizes)
    }
}

/// A structured grid's options at `size`: its grading from the parameters.
pub(crate) fn grid_options(p: &Params, size: f64) -> Result<emcraft_mesh::Options> {
    let defaults = emcraft_mesh::Options::new(size);
    Ok(emcraft_mesh::Options {
        edge: p.number("edge").or(defaults.edge)?,
        growth: p.number("growth").or(defaults.growth)?,
        strip_edge: p.number("strip_edge").or(defaults.strip_edge)?,
        strip_end: p.number("strip_end").optional()?,
        size_y: p.positive_length("size_y").optional()?,
        ..defaults
    })
}

pub fn generate(s: &mut Session, p: &Params) -> Result<Value> {
    let sizes = Sizes::read(s, p)?;
    if s.model.waveguide.is_none() && s.model.solid.is_some() {
        let curved = p.flag("curved").or(true)?;
        let solid_sizes = emcraft_mesh::solid::Sizes { default: sizes.default, per_material: sizes.per_material.clone() };
        let mesh = emcraft_mesh::solid::solid_sized(&s.model, &solid_sizes, curved)?;
        let mut reply = json!({"nodes": mesh.nodes.len(), "tetrahedra": mesh.tets.len(), "unstructured": true, "curved_edges": mesh.curved.len()});
        if sizes.by_wavelength() {
            reply["sizes"] = sizes.reply(s);
        }
        s.model.mesh = Some(mesh.into());
        s.results.invalidate(Change::Geometry);
        return Ok(reply);
    }
    let size = sizes.grid_size(s)?;
    let mesh = emcraft_mesh::waveguide_with(&s.model, &grid_options(p, size)?)?;
    let mut reply = json!({"nodes": mesh.nodes.len(), "tetrahedra": mesh.tets.len()});
    if sizes.by_wavelength() {
        reply["size"] = json!(s.show_length(size));
    }
    s.model.mesh = Some(mesh.into());
    s.results.invalidate(Change::Geometry);
    Ok(reply)
}

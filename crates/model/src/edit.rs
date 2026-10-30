//! Changing and removing the model's items after they were added: materials, waveguide and
//! lumped ports, sources, features and inclusions.
//!
//! An item is changed by taking it out and adding its new form through the setter that adds
//! items of its kind, so it is checked against everything else in the model exactly as a new
//! one would be (unique names, a port per end, inside the solid, apart from the other
//! inclusions, …); it then goes back to its place in its list, so the ports keep their order in
//! S and the features theirs. A change that is refused leaves the model as it was.
//!
//! ```
//! use emcraft_model::{End, Model, Port, Section, Waveguide};
//!
//! let mut m = Model::new();
//! let guide = Waveguide { width: 0.02286, height: 0.01016, sections: vec![Section::new(0.02, "vacuum")], layers: vec![], strips: vec![], vias: vec![] };
//! m.set_waveguide(guide)?;
//! m.add_port(Port::new("1", End::Start))?;
//! m.add_port(Port::new("2", End::End))?;
//! // Port 1 renamed and given one mode: it stays first.
//! m.replace_port(0, Port::new("in", End::Start).with_modes(1))?;
//! assert_eq!((m.ports[0].name.as_str(), m.ports[0].modes), ("in", 1));
//! // Refused: both ports at one end. Nothing changed.
//! assert!(m.replace_port(0, Port::new("in", End::End)).is_err());
//! assert_eq!(m.ports[0].end, End::Start);
//! // A material in use is not removed.
//! assert!(m.remove_material("vacuum").is_err());
//! # Ok::<(), emcraft_model::ModelError>(())
//! ```

use std::sync::Arc;

use crate::error::{ModelError, Result, invalid, named};
use crate::material::Material;
use crate::model::Model;
use crate::open::{LumpedPort, Source};
use crate::port::Port;
use crate::shape::{Feature, Inclusion};

/// Where a material is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialUse {
    /// The waveguide's section of this index.
    Section(usize),
    /// The waveguide's layer of this index.
    Layer(usize),
    /// The solid: the region the fields fill.
    Region,
    /// The inclusion of this index.
    Inclusion(usize),
}

/// An error naming an item index `i` of a list of `n` that does not exist.
fn no_item(kind: &'static str, i: usize, n: usize) -> ModelError {
    invalid(kind, format!("there is no {kind} {} (the model has {n})", i + 1))
}

/// Takes item `i` of `list(m)` out, adds `new` with `add` (which checks it against the rest of
/// the model and pushes it last) and moves it back to `i`; on a refusal the old item goes back
/// where it was.
fn replace_in<T>(
    m: &mut Model,
    i: usize,
    new: T,
    kind: &'static str,
    list: fn(&mut Model) -> &mut Vec<T>,
    add: fn(&mut Model, T) -> Result<()>,
) -> Result<()> {
    let n = list(m).len();
    if i >= n {
        return Err(no_item(kind, i, n));
    }
    let old = list(m).remove(i);
    match add(m, new) {
        Ok(()) => {
            let items = list(m);
            if let Some(added) = items.pop() {
                items.insert(i.min(items.len()), added);
            }
            Ok(())
        }
        Err(e) => {
            let items = list(m);
            items.insert(i.min(items.len()), old);
            Err(e)
        }
    }
}

impl Model {
    /// Where the material named `name` is used: sections, layers, the solid and inclusions.
    pub fn material_uses(&self, name: &str) -> Vec<MaterialUse> {
        let mut uses = Vec::new();
        if let Some(w) = &self.waveguide {
            uses.extend(w.sections.iter().enumerate().filter(|(_, s)| s.material == name).map(|(i, _)| MaterialUse::Section(i)));
            uses.extend(w.layers.iter().enumerate().filter(|(_, l)| l.material == name).map(|(i, _)| MaterialUse::Layer(i)));
        }
        if self.solid.as_ref().is_some_and(|s| s.material == name) {
            uses.push(MaterialUse::Region);
        }
        uses.extend(
            self.inclusions.iter().enumerate().filter(|(_, inc)| inc.material.as_deref() == Some(name)).map(|(i, _)| MaterialUse::Inclusion(i)),
        );
        uses
    }

    /// Removes the material named `name`, which nothing may use; vacuum, the material other
    /// settings fall back on, stays. The mesh's materials are renumbered.
    pub fn remove_material(&mut self, name: &str) -> Result<Material> {
        let (i, _) = self.material(name)?;
        if name == Material::vacuum().name {
            return Err(invalid("material `vacuum`", "is built in: every model keeps it"));
        }
        if !self.material_uses(name).is_empty() {
            return Err(invalid(format!("material `{name}`"), "is in use"));
        }
        let gone = self.materials.remove(i);
        if let Some(mesh) = &self.mesh {
            let index = i as u32;
            self.mesh = if mesh.tet_material.contains(&index) {
                None
            } else {
                let mut renumbered = (**mesh).clone();
                renumbered.tet_material.iter_mut().filter(|k| **k > index).for_each(|k| *k -= 1);
                Some(Arc::new(renumbered))
            };
        }
        Ok(gone)
    }

    /// Replaces material `i` with `m` (checked again); a new name must be unique and is given
    /// to everything that used the old one.
    pub fn replace_material(&mut self, i: usize, m: Material) -> Result<()> {
        m.checked()?;
        let old = self.materials.get(i).map(|x| x.name.clone()).ok_or_else(|| no_item("material", i, self.materials.len()))?;
        if m.name != old {
            named("material name", &m.name)?;
            if old == Material::vacuum().name {
                return Err(invalid("material `vacuum`", "is built in: it keeps its name"));
            }
            if self.materials.iter().any(|x| x.name == m.name) {
                return Err(ModelError::Duplicate { kind: "material", name: m.name });
            }
            self.rename_uses(&old, &m.name);
        }
        self.materials[i] = m;
        Ok(())
    }

    /// Points everything that uses material `old` at `new`.
    fn rename_uses(&mut self, old: &str, new: &str) {
        for u in self.material_uses(old) {
            let slot = match u {
                MaterialUse::Section(k) => self.waveguide.as_mut().and_then(|w| w.sections.get_mut(k)).map(|s| &mut s.material),
                MaterialUse::Layer(k) => self.waveguide.as_mut().and_then(|w| w.layers.get_mut(k)).map(|l| &mut l.material),
                MaterialUse::Region => self.solid.as_mut().map(|s| &mut s.material),
                MaterialUse::Inclusion(k) => self.inclusions.get_mut(k).and_then(|inc| inc.material.as_mut()),
            };
            if let Some(slot) = slot {
                *slot = new.to_owned();
            }
        }
    }

    /// Takes waveguide port `i` out of the model.
    pub fn remove_port(&mut self, i: usize) -> Result<Port> {
        if i >= self.ports.len() {
            return Err(no_item("port", i, self.ports.len()));
        }
        Ok(self.ports.remove(i))
    }

    /// Replaces waveguide port `i` with `p`, checked as [`Model::add_port`] checks a new one; it
    /// keeps its place.
    pub fn replace_port(&mut self, i: usize, p: Port) -> Result<()> {
        replace_in(self, i, p, "port", |m| &mut m.ports, Model::add_port)
    }

    /// Takes lumped port `i` out of the model.
    pub fn remove_lumped_port(&mut self, i: usize) -> Result<LumpedPort> {
        if i >= self.lumped.len() {
            return Err(no_item("lumped port", i, self.lumped.len()));
        }
        Ok(self.lumped.remove(i))
    }

    /// Replaces lumped port `i` with `p`, checked as [`Model::add_lumped_port`] checks a new one.
    pub fn replace_lumped_port(&mut self, i: usize, p: LumpedPort) -> Result<()> {
        replace_in(self, i, p, "lumped port", |m| &mut m.lumped, Model::add_lumped_port)
    }

    /// Takes source `i` out of the model.
    pub fn remove_source(&mut self, i: usize) -> Result<Source> {
        if i >= self.sources.len() {
            return Err(no_item("source", i, self.sources.len()));
        }
        Ok(self.sources.remove(i))
    }

    /// Replaces source `i` with `s`, checked as [`Model::add_source`] checks a new one.
    pub fn replace_source(&mut self, i: usize, s: Source) -> Result<()> {
        replace_in(self, i, s, "source", |m| &mut m.sources, Model::add_source)
    }

    /// Replaces feature `i` with `f`, checked as [`Model::add_feature`] checks a new one; it keeps
    /// its place in the order features combine in. The mesh is dropped.
    pub fn replace_feature(&mut self, i: usize, f: Feature) -> Result<()> {
        replace_in(self, i, f, "feature", |m| &mut m.features, Model::add_feature)
    }

    /// Replaces inclusion `i` with `inc`, checked as [`Model::add_inclusion`] checks a new one.
    /// The mesh is dropped.
    pub fn replace_inclusion(&mut self, i: usize, inc: Inclusion) -> Result<()> {
        replace_in(self, i, inc, "inclusion", |m| &mut m.inclusions, Model::add_inclusion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Axis, Combine, End, Point3, Section, Shape, Solid, Vec3, Waveguide};

    fn guide(m: &mut Model) {
        m.add_material(Material::new("ptfe", 2.1, 1.0).unwrap()).unwrap();
        m.add_material(Material::new("glass", 4.0, 1.0).unwrap()).unwrap();
        let sections = vec![Section::new(0.01, "vacuum"), Section::new(0.01, "ptfe")];
        m.set_waveguide(Waveguide { width: 0.02, height: 0.01, sections, layers: vec![], strips: vec![], vias: vec![] }).unwrap();
    }

    #[test]
    fn materials_in_use_stay_and_renames_follow() {
        let mut m = Model::new();
        guide(&mut m);
        assert_eq!(m.material_uses("ptfe"), vec![MaterialUse::Section(1)]);
        assert!(m.remove_material("ptfe").is_err());
        assert!(m.remove_material("vacuum").is_err());
        assert!(matches!(m.remove_material("nothing"), Err(ModelError::Unknown { .. })));
        // Renamed: the section follows; a name taken is refused, and so is a bad material.
        let (i, ptfe) = m.material("ptfe").map(|(i, x)| (i, x.clone())).unwrap();
        m.replace_material(i, Material { name: "teflon".into(), ..ptfe.clone() }).unwrap();
        assert_eq!(m.waveguide.as_ref().unwrap().sections[1].material, "teflon");
        assert!(matches!(m.replace_material(i, Material { name: "glass".into(), ..ptfe.clone() }), Err(ModelError::Duplicate { .. })));
        assert!(m.replace_material(i, Material { eps_r: f64::NAN, ..ptfe.clone() }).is_err());
        assert!(m.replace_material(0, Material { name: "air".into(), ..Material::vacuum() }).is_err());
        assert!(m.replace_material(9, ptfe).is_err());
        // An unused one goes, and the mesh's materials after it move down.
        m.materials.push(Material::new("last", 3.0, 1.0).unwrap());
        m.mesh = Some(Arc::new(crate::Mesh { tet_material: vec![0, 1, 3], ..Default::default() }));
        m.remove_material("glass").unwrap();
        assert_eq!(m.mesh.as_ref().unwrap().tet_material, vec![0, 1, 2]);
    }

    #[test]
    fn items_are_replaced_in_place_or_not_at_all() {
        let mut m = Model::new();
        m.set_solid(Solid { shape: Shape::Box { size: Vec3::new(0.04, 0.04, 0.04) }, material: "vacuum".into() }).unwrap();
        let sphere = |x: f64| Inclusion { shape: Shape::Sphere { radius: 0.003 }, at: Point3::new(x, 0.02, 0.02), material: None };
        m.add_inclusion(sphere(0.01)).unwrap();
        m.add_inclusion(sphere(0.03)).unwrap();
        // Onto the other one: refused, both stay as they were.
        assert!(m.replace_inclusion(0, sphere(0.029)).is_err());
        assert_eq!(m.inclusions, vec![sphere(0.01), sphere(0.03)]);
        m.replace_inclusion(0, sphere(0.015)).unwrap();
        assert_eq!(m.inclusions, vec![sphere(0.015), sphere(0.03)]);
        assert!(m.replace_inclusion(2, sphere(0.02)).is_err());
        let post = |x: f64| Feature {
            combine: Combine::Subtract,
            shape: Shape::Cylinder { radius: 0.001, length: 0.04 },
            at: Point3::new(x, 0.0, 0.005),
            axis: Axis::Y,
        };
        m.add_feature(post(0.005)).unwrap();
        m.add_feature(post(0.035)).unwrap();
        m.replace_feature(0, post(0.006)).unwrap();
        assert_eq!(m.features, vec![post(0.006), post(0.035)]);
        assert!(m.replace_feature(1, post(1.0)).is_err(), "apart from the solid");
        assert_eq!(m.features[1], post(0.035));
        // Ports keep their order; a source outside is refused.
        m.add_port(Port::new("a", End::Start)).unwrap();
        m.add_port(Port::new("b", End::End)).unwrap();
        assert!(m.replace_port(1, Port::new("a", End::End)).is_err(), "a name taken");
        m.replace_port(0, Port::new("c", End::Start)).unwrap();
        assert_eq!(m.ports.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["c", "b"]);
        assert_eq!(m.remove_port(0).unwrap().name, "c");
        assert!(m.remove_port(5).is_err());
        let dipole = |x: f64| Source::Dipole { at: Point3::new(x, 0.02, 0.02), direction: Vec3::unit(2), length: 0.001, current: 1.0 };
        m.add_source(dipole(0.02)).unwrap();
        assert!(m.replace_source(0, dipole(1.0)).is_err());
        m.replace_source(0, dipole(0.021)).unwrap();
        assert_eq!(m.remove_source(0).unwrap(), dipole(0.021));
        let gap = |name: &str| LumpedPort {
            name: name.into(),
            from: Point3::new(0.02, 0.02, 0.019),
            to: Point3::new(0.02, 0.02, 0.021),
            impedance: 50.0,
            voltage: [1.0, 0.0],
            radius: 0.0,
        };
        m.add_lumped_port(gap("L1")).unwrap();
        assert!(m.replace_lumped_port(0, gap("b")).is_err(), "a waveguide port's name");
        m.replace_lumped_port(0, gap("L2")).unwrap();
        assert_eq!(m.remove_lumped_port(0).unwrap().name, "L2");
        assert!(m.remove_lumped_port(0).is_err());
    }
}

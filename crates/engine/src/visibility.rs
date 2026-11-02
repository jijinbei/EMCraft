//! Display objects shared by material and geometry eye controls. Visibility changes no physics.
use crate::{EngineError, Params, Result, Session};
use emcraft_model::{Model, Point3};
use serde_json::{Value, json};

/// A geometry tree object, identified independently of the viewport's GPU tags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryObject {
    Guide,
    Solid,
    Section(usize),
    Layer(usize),
    Strip(usize),
    Via(usize),
    Feature(usize),
    Inclusion(usize),
}
impl GeometryObject {
    pub fn key(self) -> String {
        match self {
            Self::Guide => "guide".into(),
            Self::Solid => "solid".into(),
            Self::Section(i) => format!("section:{i}"),
            Self::Layer(i) => format!("layer:{i}"),
            Self::Strip(i) => format!("strip:{i}"),
            Self::Via(i) => format!("via:{i}"),
            Self::Feature(i) => format!("feature:{i}"),
            Self::Inclusion(i) => format!("inclusion:{i}"),
        }
    }
    pub fn parse(key: &str) -> Option<Self> {
        match key {
            "guide" => Some(Self::Guide),
            "solid" => Some(Self::Solid),
            _ => {
                let (kind, index) = key.split_once(':')?;
                let i = index.parse::<usize>().ok()?;
                match kind {
                    "section" => Some(Self::Section(i)),
                    "layer" => Some(Self::Layer(i)),
                    "strip" => Some(Self::Strip(i)),
                    "via" => Some(Self::Via(i)),
                    "feature" => Some(Self::Feature(i)),
                    "inclusion" => Some(Self::Inclusion(i)),
                    _ => None,
                }
            }
        }
    }
    pub fn exists(self, m: &Model) -> bool {
        match self {
            Self::Guide => m.waveguide.is_some(),
            Self::Solid => m.solid.is_some(),
            Self::Section(i) => m.waveguide.as_ref().is_some_and(|w| i < w.sections.len()),
            Self::Layer(i) => m.waveguide.as_ref().is_some_and(|w| i < w.layers.len()),
            Self::Strip(i) => m.waveguide.as_ref().is_some_and(|w| i < w.strips.len()),
            Self::Via(i) => m.waveguide.as_ref().is_some_and(|w| i < w.vias.len()),
            Self::Feature(i) => i < m.features.len(),
            Self::Inclusion(i) => i < m.inclusions.len(),
        }
    }
    /// Whether a displayed point belongs to this object (SI metres). Sections include their
    /// wall shell, so hiding a section also hides the shell outside its material volume.
    pub fn contains(self, m: &Model, p: Point3) -> bool {
        match self {
            Self::Guide => m.waveguide.is_some(),
            Self::Solid => m.solid.is_some(),
            Self::Section(i) => m
                .waveguide
                .as_ref()
                .and_then(|w| w.sections.get(i).map(|s| (w.sections[..i].iter().map(|s| s.length.get()).sum::<f64>(), s.length.get())))
                .is_some_and(|(z, l)| p.z >= z - 1e-10 && p.z <= z + l + 1e-10),
            Self::Layer(i) => m
                .waveguide
                .as_ref()
                .and_then(|w| w.layers.get(i).map(|s| (w.layers[..i].iter().map(|s| s.thickness.get()).sum::<f64>(), s.thickness.get())))
                .is_some_and(|(y, l)| p.y >= y - 1e-10 && p.y <= y + l + 1e-10),
            Self::Feature(i) => m.features.get(i).is_some_and(|s| s.contains(p)),
            Self::Inclusion(i) => m.inclusions.get(i).is_some_and(|s| s.contains(p)),
            Self::Strip(i) => m
                .waveguide
                .as_ref()
                .and_then(|w| w.strips.get(i))
                .is_some_and(|s| p.x >= s.x0 && p.x <= s.x0 + s.width && p.z >= s.z0 && p.z <= s.z1 && (p.y - s.y).abs() < 1e-10),
            Self::Via(i) => m
                .waveguide
                .as_ref()
                .and_then(|w| w.vias.get(i))
                .is_some_and(|s| p.x >= s.x0 && p.x <= s.x0 + s.wx && p.y >= s.y0 && p.y <= s.y1 && p.z >= s.z0 && p.z <= s.z0 + s.wz),
        }
    }
}
/// A material or geometry row's common visibility target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisplayObject {
    Material(String),
    Geometry(GeometryObject),
}
impl DisplayObject {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Material(_) => "material",
            Self::Geometry(_) => "geometry",
        }
    }
    pub fn name(&self) -> String {
        match self {
            Self::Material(s) => s.clone(),
            Self::Geometry(g) => g.key(),
        }
    }
}
impl crate::views::Views {
    /// Whether a display object is shown. For example, the default vacuum is visible:
    /// ```
    /// use emcraft_engine::{views::Views, visibility::DisplayObject};
    /// assert!(Views::default().visible(&DisplayObject::Material("vacuum".into())));
    /// ```
    pub fn visible(&self, object: &DisplayObject) -> bool {
        match object {
            DisplayObject::Material(n) => !self.hidden_materials.contains(n),
            DisplayObject::Geometry(g) => !self.hidden_geometry.contains(&g.key()),
        }
    }
}
pub fn set(s: &mut Session, object: DisplayObject, visible: bool) -> Result<Value> {
    let name = object.name();
    let hidden = match &object {
        DisplayObject::Material(n) => {
            s.model().material(n)?;
            &mut s.views.hidden_materials
        }
        DisplayObject::Geometry(g) => {
            if !g.exists(s.model()) {
                return Err(EngineError::failed("no such geometry object"));
            }
            &mut s.views.hidden_geometry
        }
    };
    if name.chars().count() > 64 || (!visible && hidden.len() >= 4096 && !hidden.contains(&name)) {
        return Err(EngineError::failed("display object limit exceeded"));
    }
    match visible {
        true => {
            hidden.remove(&name);
        }
        false => {
            hidden.insert(name.clone());
        }
    }
    Ok(json!({"kind":object.kind(),"name":name,"visible":visible}))
}
pub fn command(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    let object = match p.string("kind").required()? {
        "material" => DisplayObject::Material(name.into()),
        "geometry" => DisplayObject::Geometry(GeometryObject::parse(name).ok_or_else(|| EngineError::failed("invalid geometry object"))?),
        _ => return Err(EngineError::failed("kind must be material or geometry")),
    };
    set(s, object, p.flag("visible").required()?)
}

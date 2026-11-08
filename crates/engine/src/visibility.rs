//! Display objects shared by material and geometry eye controls. Visibility changes no physics.
use crate::{EngineError, Params, Result, Session};
use emcraft_model::{Axis, Model, Point3};
use serde_json::{Value, json};

/// A geometry tree object, identified independently of the viewport's GPU tags. Written in
/// project files and commands as its key (`section:2`, `symmetry:z`), read back checked.
///
/// ```
/// use emcraft_engine::visibility::GeometryObject;
/// use emcraft_model::Axis;
/// assert_eq!(GeometryObject::Symmetry(Axis::Z).key(), "symmetry:z");
/// assert_eq!(GeometryObject::parse("section:2"), Some(GeometryObject::Section(2)));
/// assert_eq!(GeometryObject::parse("symmetry:w"), None);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum GeometryObject {
    Guide,
    Solid,
    Section(usize),
    Layer(usize),
    Strip(usize),
    Via(usize),
    Feature(usize),
    Inclusion(usize),
    /// The symmetry plane across an axis (a model has one a axis at most).
    Symmetry(Axis),
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
            Self::Symmetry(a) => format!("symmetry:{}", a.id()),
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        match key {
            "guide" => Some(Self::Guide),
            "solid" => Some(Self::Solid),
            _ => {
                let (kind, index) = key.split_once(':')?;
                if kind == "symmetry" {
                    return Axis::parse(index).map(Self::Symmetry);
                }
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
            Self::Guide => m.waveguide().is_some(),
            Self::Solid => m.domain.solid.is_some(),
            Self::Section(i) => m.waveguide().is_some_and(|w| i < w.sections.len()),
            Self::Layer(i) => m.waveguide().is_some_and(|w| i < w.layers.len()),
            Self::Strip(i) => m.waveguide().is_some_and(|w| i < w.strips.len()),
            Self::Via(i) => m.waveguide().is_some_and(|w| i < w.vias.len()),
            Self::Feature(i) => i < m.features().len(),
            Self::Inclusion(i) => i < m.inclusions().len(),
            Self::Symmetry(a) => m.domain.symmetry.iter().any(|s| s.axis == a),
        }
    }

    /// Whether a displayed point belongs to this object (SI metres). Sections include their
    /// wall shell, so hiding a section also hides the shell outside its material volume.
    pub fn contains(self, m: &Model, p: Point3) -> bool {
        match self {
            Self::Guide => m.waveguide().is_some(),
            Self::Solid => m.domain.solid.is_some(),
            Self::Section(i) => m
                .waveguide()
                .as_ref()
                .and_then(|w| w.sections.get(i).map(|s| (w.sections[..i].iter().map(|s| s.length.get()).sum::<f64>(), s.length.get())))
                .is_some_and(|(z, l)| p.z >= z - 1e-10 && p.z <= z + l + 1e-10),
            Self::Layer(i) => m
                .waveguide()
                .as_ref()
                .and_then(|w| w.layers.get(i).map(|s| (w.layers[..i].iter().map(|s| s.thickness.get()).sum::<f64>(), s.thickness.get())))
                .is_some_and(|(y, l)| p.y >= y - 1e-10 && p.y <= y + l + 1e-10),
            Self::Feature(i) => m.features().get(i).is_some_and(|s| s.contains(p)),
            Self::Inclusion(i) => m.inclusions().get(i).is_some_and(|s| s.contains(p)),
            // A plane hides only its own sheet, nothing of the model.
            Self::Symmetry(_) => false,
            Self::Strip(i) => m
                .waveguide()
                .as_ref()
                .and_then(|w| w.strips.get(i))
                .is_some_and(|s| p.x >= s.x0 && p.x <= s.x0 + s.width && p.z >= s.z0 && p.z <= s.z1 && (p.y - s.y).abs() < 1e-10),
            Self::Via(i) => m
                .waveguide()
                .as_ref()
                .and_then(|w| w.vias.get(i))
                .is_some_and(|s| p.x >= s.x0 && p.x <= s.x0 + s.wx && p.y >= s.y0 && p.y <= s.y1 && p.z >= s.z0 && p.z <= s.z0 + s.wz),
        }
    }
}

/// Where item `i` of `old` went in `new`: where it still is, else the same item (the same
/// occurrence of equal ones) where it moved to, else the same place when the count is unchanged
/// (it was edited); `None` when it is gone.
fn moved<T: PartialEq>(old: &[T], new: &[T], i: usize) -> Option<usize> {
    let item = old.get(i)?;
    if new.get(i) == Some(item) {
        return Some(i);
    }
    let nth = old[..i].iter().filter(|o| *o == item).count();
    new.iter().enumerate().filter(|(_, n)| *n == item).nth(nth).map(|(j, _)| j).or_else(|| (old.len() == new.len()).then_some(i))
}

/// A list of the model's waveguide (empty without one).
fn parts<T>(m: &Model, list: fn(&emcraft_model::Waveguide) -> &Vec<T>) -> &[T] {
    m.waveguide().map_or(&[], |w| list(w).as_slice())
}

impl GeometryObject {
    /// This object after the model changed from `before` to `after`: an indexed object follows
    /// its own item (a body deleted before it moves it down one), `None` when it is gone.
    ///
    /// ```
    /// use emcraft_engine::visibility::GeometryObject;
    /// let mut s = emcraft_engine::Session::new();
    /// s.execute("geometry.waveguide", &serde_json::json!({"width": 20, "height": 10, "sections": [{"length": 10, "material": "vacuum"}, {"length": 20, "material": "vacuum"}]})).unwrap();
    /// let before = s.model().clone();
    /// s.execute("geometry.waveguide", &serde_json::json!({"width": 20, "height": 10, "sections": [{"length": 20, "material": "vacuum"}]})).unwrap();
    /// assert_eq!(GeometryObject::Section(1).follow(&before, s.model()), Some(GeometryObject::Section(0)));
    /// assert_eq!(GeometryObject::Section(0).follow(&before, s.model()), None);
    /// ```
    pub fn follow(self, before: &Model, after: &Model) -> Option<Self> {
        match self {
            Self::Guide | Self::Solid | Self::Symmetry(_) => self.exists(after).then_some(self),
            Self::Section(i) => moved(parts(before, |w| &w.sections), parts(after, |w| &w.sections), i).map(Self::Section),
            Self::Layer(i) => moved(parts(before, |w| &w.layers), parts(after, |w| &w.layers), i).map(Self::Layer),
            Self::Strip(i) => moved(parts(before, |w| &w.strips), parts(after, |w| &w.strips), i).map(Self::Strip),
            Self::Via(i) => moved(parts(before, |w| &w.vias), parts(after, |w| &w.vias), i).map(Self::Via),
            Self::Feature(i) => moved(before.features(), after.features(), i).map(Self::Feature),
            Self::Inclusion(i) => moved(before.inclusions(), after.inclusions(), i).map(Self::Inclusion),
        }
    }
}

impl crate::views::Views {
    /// The hidden geometry carried across a change of the model (see [`GeometryObject::follow`]).
    pub(crate) fn follow(&mut self, before: &Model, after: &Model) {
        self.hidden_geometry = std::mem::take(&mut self.hidden_geometry).into_iter().filter_map(|g| g.follow(before, after)).collect();
    }
}

impl TryFrom<String> for GeometryObject {
    type Error = String;

    fn try_from(key: String) -> std::result::Result<Self, String> {
        GeometryObject::parse(&key).ok_or_else(|| format!("no geometry object `{}`", crate::text::clip(&key, 64)))
    }
}

impl From<GeometryObject> for String {
    fn from(g: GeometryObject) -> String {
        g.key()
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
            DisplayObject::Geometry(g) => !self.hidden_geometry.contains(g),
        }
    }
}

pub fn set(s: &mut Session, object: DisplayObject, visible: bool) -> Result<Value> {
    let name = object.name();
    let full = |n: usize| n >= MAX_HIDDEN;
    match &object {
        DisplayObject::Material(n) => {
            s.model().material(n)?;
            let hidden = &mut s.views.hidden_materials;
            if name.chars().count() > 64 || (!visible && full(hidden.len()) && !hidden.contains(n)) {
                return Err(EngineError::failed("display object limit exceeded"));
            }
            if visible {
                hidden.remove(n)
            } else {
                hidden.insert(n.clone())
            };
        }
        DisplayObject::Geometry(g) => {
            if !g.exists(s.model()) {
                return Err(EngineError::failed("no such geometry object"));
            }
            let hidden = &mut s.views.hidden_geometry;
            if !visible && full(hidden.len()) && !hidden.contains(g) {
                return Err(EngineError::failed("display object limit exceeded"));
            }
            if visible {
                hidden.remove(g)
            } else {
                hidden.insert(*g)
            };
        }
    }
    Ok(json!({"kind": object.kind(), "name": name, "visible": visible}))
}

/// The most objects a view hides of each kind.
pub const MAX_HIDDEN: usize = 4096;

/// `view.visibility`: shows or hides a material's region or a geometry object, the display only
/// (the model, mesh and results stay as they are).
///
/// ```
/// let mut s = emcraft_engine::Session::new();
/// let hide = |name: &str| serde_json::json!({"kind": "material", "name": name, "visible": false});
/// s.execute("view.visibility", &hide("vacuum")).unwrap();
/// assert!(s.views().hidden_materials.contains("vacuum"));
/// assert!(s.execute("view.visibility", &hide("missing")).is_err());
/// ```
pub fn command(s: &mut Session, p: &Params) -> Result<Value> {
    let name = p.string("name").required()?;
    let object = match p.string("kind").required()? {
        "material" => DisplayObject::Material(name.into()),
        "geometry" => DisplayObject::Geometry(GeometryObject::parse(name).ok_or_else(|| EngineError::failed("invalid geometry object"))?),
        _ => return Err(EngineError::failed("kind must be material or geometry")),
    };
    set(s, object, p.flag("visible").required()?)
}

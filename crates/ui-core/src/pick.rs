//! What a click in the 3D view names: a part of the model by its kind and index, and the tag
//! its triangles carry so a pick finds it.

/// What a picked triangle belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Pick {
    /// The waveguide's walls (once meshed).
    Guide,
    Material(usize),
    Section(usize),
    Layer(usize),
    Strip(usize),
    Solid,
    Feature(usize),
    Inclusion(usize),
    Port(usize),
    /// A field plane in the Results step's view.
    Plane(usize),
    Via(usize),
    /// A lumped port's marker.
    Lumped(usize),
    /// A board's net's copper (an index into [`board::nets`]).
    Net(usize),
    /// An imported part's body round the region (an index into `Model::imported`).
    Part(usize),
}

impl Pick {
    pub fn display_object(self, m: &emcraft_model::Model) -> Option<emcraft_engine::visibility::DisplayObject> {
        use emcraft_engine::visibility::{DisplayObject as O, GeometryObject as G};
        Some(match self {
            Self::Material(i) => O::Material(m.materials.get(i)?.name.clone()),
            Self::Guide => O::Geometry(G::Guide),
            Self::Solid => O::Geometry(G::Solid),
            Self::Section(i) => O::Geometry(G::Section(i)),
            Self::Layer(i) => O::Geometry(G::Layer(i)),
            Self::Strip(i) => O::Geometry(G::Strip(i)),
            Self::Via(i) => O::Geometry(G::Via(i)),
            Self::Feature(i) => O::Geometry(G::Feature(i)),
            Self::Inclusion(i) => O::Geometry(G::Inclusion(i)),
            _ => return None,
        })
    }

    /// The triangle tag (0 is "nothing").
    pub fn tag(self) -> u32 {
        let (kind, i) = match self {
            Pick::Guide => (1, 0),
            Pick::Material(i) => (12, i),
            Pick::Section(i) => (2, i),
            Pick::Layer(i) => (3, i),
            Pick::Strip(i) => (4, i),
            Pick::Solid => (5, 0),
            Pick::Feature(i) => (6, i),
            Pick::Inclusion(i) => (7, i),
            Pick::Port(i) => (8, i),
            Pick::Plane(i) => (9, i),
            Pick::Via(i) => (10, i),
            Pick::Lumped(i) => (11, i),
            Pick::Net(i) => (13, i),
            Pick::Part(i) => (14, i),
        };
        (kind << 16) | (i.min(0xFFFF) as u32)
    }

    pub fn from_tag(tag: u32) -> Option<Pick> {
        let i = (tag & 0xFFFF) as usize;
        Some(match tag >> 16 {
            1 => Pick::Guide,
            12 => Pick::Material(i),
            2 => Pick::Section(i),
            3 => Pick::Layer(i),
            4 => Pick::Strip(i),
            5 => Pick::Solid,
            6 => Pick::Feature(i),
            7 => Pick::Inclusion(i),
            8 => Pick::Port(i),
            9 => Pick::Plane(i),
            10 => Pick::Via(i),
            11 => Pick::Lumped(i),
            13 => Pick::Net(i),
            14 => Pick::Part(i),
            _ => return None,
        })
    }
}

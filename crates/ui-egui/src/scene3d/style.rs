//! The 3D view's colours and how opaque each kind of face is in each overlay.

use emcraft_model::Model;
use emcraft_render::Rgb;

use super::Overlay;
use super::surface::Kind;

pub(super) const METAL: Rgb = Rgb(170, 178, 190);
pub(super) const METAL_DARK: Rgb = Rgb(120, 126, 136);
pub(super) const PORT: Rgb = Rgb(240, 140, 40);
pub(super) const PORT_SOFT: Rgb = Rgb(235, 180, 120);
pub(super) const EDGE: Rgb = Rgb(40, 46, 56);
pub(super) const PML: Rgb = Rgb(150, 110, 210);
/// The rim under field arrows.
pub(super) const RIM: Rgb = Rgb(245, 245, 245);
pub(super) const SOURCE: Rgb = Rgb(220, 40, 60);
/// The colour of a periodic boundary's faces.
pub(super) const PERIODIC: Rgb = Rgb(170, 90, 200);
/// The colour picked things turn towards.
pub(super) const HIGHLIGHT: Rgb = Rgb(30, 120, 255);

/// A material's colour: vacuum pale blue, the others from a fixed palette by their index.
pub fn material_rgb(model: &Model, index: usize) -> Rgb {
    const PALETTE: [Rgb; 6] = [Rgb(214, 150, 60), Rgb(90, 170, 110), Rgb(170, 110, 200), Rgb(60, 150, 190), Rgb(200, 100, 110), Rgb(150, 150, 70)];
    match model.materials.get(index) {
        Some(m) if m.eps_r <= 1.0 + 1e-12 && m.mu_r <= 1.0 + 1e-12 => Rgb(175, 200, 232),
        _ => PALETTE[index.saturating_sub(1) % PALETTE.len()],
    }
}

/// A symmetry plane's colour: an electric wall blue, a magnetic one green.
pub(super) fn symmetry_rgb(kind: emcraft_model::SymmetryKind) -> Rgb {
    match kind {
        emcraft_model::SymmetryKind::Electric => Rgb(70, 120, 220),
        emcraft_model::SymmetryKind::Magnetic => Rgb(60, 170, 110),
    }
}

/// A boundary face's colour and opacity in an overlay, `port` whether a port lies on it. Air
/// is all but invisible and walls see-through, so what lies inside shows; conductors inside
/// are solid metal.
pub(super) fn wall_style(m: &Model, overlay: Overlay, port: bool, material: usize, kind: Kind) -> (Rgb, u8) {
    match (overlay, kind) {
        (Overlay::Ports, _) if port => (PORT, 255),
        (Overlay::Plain | Overlay::Mesh, _) if port => (PORT_SOFT, 255),
        (Overlay::Boundaries, _) if port => (PORT_SOFT, 200),
        (Overlay::Materials, _) if port => (material_rgb(m, material), 200),
        (Overlay::Field, Kind::Conductor) => (METAL_DARK, 200),
        (Overlay::Field, _) => (METAL, if kind == Kind::Air { 0 } else { 40 }),
        (_, Kind::Conductor) => (METAL_DARK, 255),
        (Overlay::Materials, Kind::Air) => (material_rgb(m, material), 30),
        (_, Kind::Air) => (METAL, if overlay == Overlay::Mesh { 0 } else { 16 }),
        (Overlay::Plain, Kind::Wall) => (METAL, 110),
        (Overlay::Mesh, Kind::Wall) => (Rgb(214, 220, 228), 130),
        (Overlay::Materials, Kind::Wall) => (material_rgb(m, material), 120),
        (Overlay::Ports, Kind::Wall) => (METAL, 55),
        (Overlay::Boundaries, Kind::Wall) => (METAL_DARK, 110),
    }
}

/// The opacity of a face that is no wall (a symmetry plane, a periodic boundary) in an
/// overlay: shaded when boundaries are the subject, else no more than half see-through.
pub(super) fn plane_alpha(m: &Model, overlay: Overlay, material: usize) -> u8 {
    let (_, alpha) = wall_style(m, overlay, false, material, Kind::Wall);
    if overlay == Overlay::Boundaries { 200 } else { alpha.min(120) }
}

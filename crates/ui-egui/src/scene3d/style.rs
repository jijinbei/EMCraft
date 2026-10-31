//! The 3D view's colours, the current theme's ([`crate::theme::Scene`]), and how opaque each
//! kind of face is in each overlay.

use emcraft_model::Model;
use emcraft_render::Rgb;

use super::Overlay;
use super::surface::Kind;
use crate::theme::Scene;

/// The current theme's 3D colours.
pub(super) fn sc() -> Scene {
    crate::theme::scene()
}

/// A material's colour: vacuum pale blue, the others from the theme's palette by their index.
pub fn material_rgb(model: &Model, index: usize) -> Rgb {
    let s = sc();
    match model.materials.get(index) {
        Some(m) if m.eps_r.get() <= 1.0 + 1e-12 && m.mu_r.get() <= 1.0 + 1e-12 => s.vacuum,
        _ => s.materials[index.saturating_sub(1) % s.materials.len()],
    }
}

/// A symmetry plane's colour: an electric wall blue, a magnetic one green.
pub(super) fn symmetry_rgb(kind: emcraft_model::SymmetryKind) -> Rgb {
    match kind {
        emcraft_model::SymmetryKind::Electric => sc().symmetry_electric,
        emcraft_model::SymmetryKind::Magnetic => sc().symmetry_magnetic,
    }
}

/// A boundary face's colour and opacity in an overlay, `port` whether a port lies on it. Air
/// is all but invisible and walls see-through, so what lies inside shows; conductors inside
/// are solid metal.
pub(super) fn wall_style(m: &Model, overlay: Overlay, port: bool, material: usize, kind: Kind) -> (Rgb, u8) {
    let s = sc();
    match (overlay, kind) {
        (Overlay::Ports, _) if port => (s.port, 255),
        (Overlay::Plain | Overlay::Mesh, _) if port => (s.port_soft, 255),
        (Overlay::Boundaries, _) if port => (s.port_soft, 200),
        (Overlay::Materials, _) if port => (material_rgb(m, material), 200),
        (Overlay::Field, Kind::Conductor) => (s.metal_dark, 200),
        (Overlay::Field, _) => (s.metal, if kind == Kind::Air { 0 } else { 40 }),
        (_, Kind::Conductor) => (s.metal_dark, 255),
        (Overlay::Materials, Kind::Air) => (material_rgb(m, material), 30),
        (_, Kind::Air) => (s.metal, if overlay == Overlay::Mesh { 0 } else { 16 }),
        (Overlay::Plain, Kind::Wall) => (s.metal, 110),
        (Overlay::Mesh, Kind::Wall) => (s.mesh_wall, 130),
        (Overlay::Materials, Kind::Wall) => (material_rgb(m, material), 120),
        (Overlay::Ports, Kind::Wall) => (s.metal, 55),
        (Overlay::Boundaries, Kind::Wall) => (s.metal_dark, 110),
    }
}

/// The opacity of a face that is no wall (a symmetry plane, a periodic boundary) in an
/// overlay: shaded when boundaries are the subject, else no more than half see-through.
pub(super) fn plane_alpha(m: &Model, overlay: Overlay, material: usize) -> u8 {
    let (_, alpha) = wall_style(m, overlay, false, material, Kind::Wall);
    if overlay == Overlay::Boundaries { 200 } else { alpha.min(120) }
}

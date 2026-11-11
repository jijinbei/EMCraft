//! The 3D view's navigation ([`emcraft_ui_core::navigation`]) on egui's keys: the modifiers held,
//! the digits of the standard views and the arrows.

use emcraft_render::view3d::StandardView;
pub use emcraft_ui_core::navigation::*;

/// egui's modifiers as the navigation reads them (Ctrl is also Cmd on macOS).
pub fn keys(m: egui::Modifiers) -> NavKeys {
    NavKeys { shift: m.shift, ctrl: m.ctrl || m.command || m.mac_cmd, alt: m.alt }
}

/// The standard views on Ctrl+1…7, as SolidCraft's.
pub const DIGIT_VIEWS: [(egui::Key, StandardView); 7] = [
    (egui::Key::Num1, StandardView::Front),
    (egui::Key::Num2, StandardView::Back),
    (egui::Key::Num3, StandardView::Left),
    (egui::Key::Num4, StandardView::Right),
    (egui::Key::Num5, StandardView::Top),
    (egui::Key::Num6, StandardView::Bottom),
    (egui::Key::Num7, StandardView::Iso),
];

/// The arrows: each key's turn toward the screen's right and up.
pub const ARROWS: [(egui::Key, f64, f64); 4] =
    [(egui::Key::ArrowLeft, -1.0, 0.0), (egui::Key::ArrowRight, 1.0, 0.0), (egui::Key::ArrowUp, 0.0, 1.0), (egui::Key::ArrowDown, 0.0, -1.0)];

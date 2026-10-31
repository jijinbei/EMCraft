//! How the mouse, the wheel, the trackpad and the keys move the 3D view's camera, the same way
//! as in the sibling app SolidCraft (`crates/ui-egui/src/prefs.rs` and `navigation.rs`, MIT OR
//! Apache-2.0; ATTRIBUTION.md), so the two apps feel alike: its mouse mappings (presets), its
//! wheel and pinch zoom toward the cursor, two-finger pan, Ctrl+left drag orbiting in every
//! mapping, Shift snapping an orbit to the nearest of the cube's views, the arrow keys (15°, with
//! Shift 90°; held, a steady turn) and Ctrl+1…7 for the standard views.
//!
//! The mappings' names only say which other program's mouse they follow (NOTICE).

use emcraft_render::view3d::StandardView;

/// What a drag does to the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavMode {
    Orbit,
    Pan,
    Zoom,
}

/// A mouse button that can drag the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavButton {
    Left,
    Middle,
    Right,
}

/// Modifier keys a binding needs held (others may be held too).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct NavKeys {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl NavKeys {
    const NONE: NavKeys = NavKeys { shift: false, ctrl: false, alt: false };
    const SHIFT: NavKeys = NavKeys { shift: true, ctrl: false, alt: false };
    const CTRL: NavKeys = NavKeys { shift: false, ctrl: true, alt: false };
    const ALT: NavKeys = NavKeys { shift: false, ctrl: false, alt: true };
    const SHIFT_ALT: NavKeys = NavKeys { shift: true, ctrl: false, alt: true };

    /// All of these keys are held (Ctrl is Cmd on macOS).
    fn held(self, m: egui::Modifiers) -> bool {
        (!self.shift || m.shift) && (!self.ctrl || m.ctrl || m.command) && (!self.alt || m.alt)
    }
}

/// One binding: dragging `button` with `keys` held does `mode`.
#[derive(Clone, Copy, Debug)]
pub struct NavBinding {
    pub button: NavButton,
    pub keys: NavKeys,
    pub mode: NavMode,
}

const fn bind(button: NavButton, keys: NavKeys, mode: NavMode) -> NavBinding {
    NavBinding { button, keys, mode }
}

/// A mouse mapping: its bindings, most specific first (the first whose button is dragged with
/// all its keys held wins).
pub struct NavPreset {
    pub id: &'static str,
    pub label: &'static str,
    /// What the mouse does, for the settings page.
    pub help: &'static str,
    /// The wheel zooms the other way.
    pub reverse_zoom: bool,
    /// A middle double-click fits (else it goes home).
    pub middle_double_fits: bool,
    pub bindings: &'static [NavBinding],
}

pub const OPENSCAD: &str = "openscad";
pub const TOUCHPAD: &str = "touchpad";
pub const FUSION: &str = "fusion";

use NavButton::{Left, Middle, Right};
use NavMode::{Orbit, Pan, Zoom};

/// The mouse mappings, SolidCraft's. The first is the default.
pub const NAV_PRESETS: &[NavPreset] = &[
    NavPreset {
        id: FUSION,
        label: "Fusion",
        help: "Middle drag pans, Shift+middle orbits, right drag orbits",
        reverse_zoom: false,
        middle_double_fits: false,
        bindings: &[bind(Middle, NavKeys::SHIFT, Orbit), bind(Middle, NavKeys::NONE, Pan), bind(Right, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "solidworks",
        label: "SolidWorks",
        help: "Middle drag orbits, Ctrl+middle pans, Shift+middle zooms",
        reverse_zoom: true,
        middle_double_fits: true,
        bindings: &[bind(Middle, NavKeys::CTRL, Pan), bind(Middle, NavKeys::SHIFT, Zoom), bind(Middle, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "inventor",
        label: "Inventor",
        help: "Middle drag pans, Shift+middle orbits, Ctrl+middle zooms, right drag orbits",
        reverse_zoom: false,
        middle_double_fits: false,
        bindings: &[
            bind(Middle, NavKeys::SHIFT, Orbit),
            bind(Middle, NavKeys::CTRL, Zoom),
            bind(Middle, NavKeys::NONE, Pan),
            bind(Right, NavKeys::NONE, Orbit),
        ],
    },
    NavPreset {
        id: "alias",
        label: "Alias",
        help: "Shift+Alt+left drag orbits, Shift+Alt+middle pans, Shift+Alt+right zooms; middle drag also pans and right drag orbits",
        reverse_zoom: false,
        middle_double_fits: false,
        bindings: &[
            bind(Left, NavKeys::SHIFT_ALT, Orbit),
            bind(Middle, NavKeys::SHIFT_ALT, Pan),
            bind(Right, NavKeys::SHIFT_ALT, Zoom),
            bind(Middle, NavKeys::CTRL, Zoom),
            bind(Middle, NavKeys::NONE, Pan),
            bind(Right, NavKeys::NONE, Orbit),
        ],
    },
    NavPreset {
        id: "maya",
        label: "Maya",
        help: "Alt+left drag orbits, Alt+middle pans, Alt+right zooms; middle drag also pans",
        reverse_zoom: false,
        middle_double_fits: false,
        bindings: &[
            bind(Left, NavKeys::ALT, Orbit),
            bind(Middle, NavKeys::ALT, Pan),
            bind(Right, NavKeys::ALT, Zoom),
            bind(Middle, NavKeys::NONE, Pan),
        ],
    },
    NavPreset {
        id: "blender",
        label: "Blender",
        help: "Middle drag orbits, Shift+middle pans, Ctrl+middle zooms; right drag leaves the camera alone",
        reverse_zoom: false,
        middle_double_fits: false,
        bindings: &[bind(Middle, NavKeys::SHIFT, Pan), bind(Middle, NavKeys::CTRL, Zoom), bind(Middle, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "tinkercad",
        label: "Tinkercad",
        help: "Right drag orbits, middle or Shift+right drag pans",
        reverse_zoom: false,
        middle_double_fits: false,
        bindings: &[bind(Right, NavKeys::SHIFT, Pan), bind(Right, NavKeys::NONE, Orbit), bind(Middle, NavKeys::NONE, Pan)],
    },
    NavPreset {
        id: OPENSCAD,
        label: "OpenSCAD",
        help: "Left drag orbits; right or middle drag pans, Shift+right or Shift+middle drag zooms",
        reverse_zoom: false,
        middle_double_fits: false,
        bindings: &[
            bind(Left, NavKeys::NONE, Orbit),
            bind(Right, NavKeys::SHIFT, Zoom),
            bind(Middle, NavKeys::SHIFT, Zoom),
            bind(Right, NavKeys::NONE, Pan),
            bind(Middle, NavKeys::NONE, Pan),
        ],
    },
    NavPreset {
        id: "hfss",
        label: "HFSS",
        help: "Alt+left drag pans, Shift+Alt+left zooms, Ctrl+left drag orbits",
        reverse_zoom: false,
        middle_double_fits: false,
        // Ctrl+left orbit is the drag every mapping has.
        bindings: &[bind(Left, NavKeys::SHIFT_ALT, Zoom), bind(Left, NavKeys::ALT, Pan)],
    },
    NavPreset {
        id: TOUCHPAD,
        label: "Touchpad",
        help: "Hold Shift and move the pointer to pan; hold Alt and move it to orbit. No button is needed. Scrolling zooms.",
        reverse_zoom: false,
        middle_double_fits: false,
        bindings: &[bind(Middle, NavKeys::NONE, Pan), bind(Right, NavKeys::NONE, Orbit)],
    },
];

impl NavPreset {
    /// What dragging `button` with `mods` held does, if anything.
    pub fn mode(&self, button: NavButton, mods: egui::Modifiers) -> Option<NavMode> {
        self.bindings.iter().find(|b| b.button == button && b.keys.held(mods)).map(|b| b.mode)
    }

    /// What moving the pointer with no button down does (the touchpad mapping: Shift pans, Alt
    /// orbits; exactly that key held).
    pub fn motion_mode(&self, mods: egui::Modifiers) -> Option<NavMode> {
        if self.id != TOUCHPAD || mods.ctrl || mods.command || mods.mac_cmd {
            return None;
        }
        match (mods.shift, mods.alt) {
            (true, false) => Some(Pan),
            (false, true) => Some(Orbit),
            _ => None,
        }
    }

    /// Two-finger scroll pans, and with Shift orbits (the default mapping's trackpad).
    pub fn shift_scroll_orbits(&self) -> bool {
        self.id == FUSION
    }

    /// Two-finger scroll pans (not for the touchpad mapping, where scrolling zooms).
    pub fn scroll_pans(&self) -> bool {
        self.id != TOUCHPAD
    }
}

/// The mapping with this id (the default for an unknown one).
pub fn preset(id: &str) -> &'static NavPreset {
    NAV_PRESETS.iter().find(|p| p.id == id).unwrap_or(&NAV_PRESETS[0])
}

emcraft_macros::named_enum! {
    /// What an orbit turns about.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum OrbitAround {
        /// The view's centre (its target): what Pan moves and Fit frames.
        #[default]
        ViewCentre = "view_centre" => "View centre",
        /// The model's origin, kept still on screen.
        Origin = "origin" => "Origin",
        /// The model point under the cursor when the drag began (over empty space, the middle of
        /// what is shown).
        Cursor = "cursor" => "Under the cursor",
    }
}

emcraft_macros::named_enum! {
    /// The view's orientation control in its corner.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum Navigator {
        /// A cube: its faces and corners turn to that view, a drag on it orbits.
        #[default]
        Cube = "cube" => "Cube",
        /// The six signed axes.
        Axes = "axes" => "Axes",
    }
}

/// How fast the wheel zooms: the camera's distance changes by e^(−0.0015) a point scrolled.
pub const WHEEL_ZOOM_PER_POINT: f64 = 0.0015;
/// How fast a zoom drag zooms: e^(0.01) a point dragged down.
pub const DRAG_ZOOM_PER_POINT: f64 = 0.01;
/// The arrow keys' step, and with Shift.
pub const KEY_STEP_DEGREES: [f64; 2] = [15.0, 90.0];
/// A key press shorter than this is one animated step; held longer, the view turns steadily.
pub const TAP_SECONDS: f64 = 0.15;
/// How fast a held arrow turns the view (Shift: twice as fast).
pub const HOLD_DEGREES_PER_SECOND: f64 = 90.0;
/// Ctrl+arrows pan this share of the view's height.
pub const KEY_PAN_SHARE: f32 = 0.1;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_mapping_is_solidcrafts_and_ids_are_distinct() {
        let p = preset("nonsense");
        assert_eq!(p.id, FUSION);
        let none = egui::Modifiers::NONE;
        assert_eq!(p.mode(Right, none), Some(Orbit));
        assert_eq!(p.mode(Middle, none), Some(Pan));
        assert_eq!(p.mode(Middle, egui::Modifiers::SHIFT), Some(Orbit));
        assert_eq!(p.mode(Left, none), None);
        let mut ids: Vec<&str> = NAV_PRESETS.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), NAV_PRESETS.len());
        assert_eq!(preset("blender").mode(Middle, none), Some(Orbit));
        assert_eq!(preset(TOUCHPAD).motion_mode(egui::Modifiers::ALT), Some(Orbit));
        assert_eq!(preset(FUSION).motion_mode(egui::Modifiers::ALT), None);
    }
}

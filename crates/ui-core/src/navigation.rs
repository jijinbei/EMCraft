//! How the mouse, the wheel, the trackpad and the keys move the 3D view's camera, the same way
//! as in the sibling app SolidCraft (`crates/ui-egui/src/prefs.rs` and `navigation.rs`, MIT OR
//! Apache-2.0; ATTRIBUTION.md), so the two apps feel alike: its mouse mappings (presets), its
//! wheel and pinch zoom toward the cursor, two-finger pan, Ctrl+left drag orbiting in every
//! mapping, Shift snapping an orbit to the nearest of the cube's views, the arrow keys (15°, with
//! Shift 90°; held, a steady turn) and Ctrl+1…7 for the standard views; what an orbit turns
//! about and the orientation control. Each window reads its own keys into [`NavKeys`].
//!
//! The mappings' names only say which other program's mouse they follow (NOTICE).

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
emcraft_macros::named_serde!(OrbitAround);

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
emcraft_macros::named_serde!(Navigator);

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

/// Modifier keys, as a binding needs them held (others may be held too) and as a window reports
/// them held (Ctrl is also Cmd on macOS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct NavKeys {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl NavKeys {
    pub const NONE: NavKeys = NavKeys { shift: false, ctrl: false, alt: false };
    pub const SHIFT: NavKeys = NavKeys { shift: true, ctrl: false, alt: false };
    pub const CTRL: NavKeys = NavKeys { shift: false, ctrl: true, alt: false };
    pub const ALT: NavKeys = NavKeys { shift: false, ctrl: false, alt: true };
    const SHIFT_ALT: NavKeys = NavKeys { shift: true, ctrl: false, alt: true };

    /// All of these keys are held in `m`.
    fn held(self, m: NavKeys) -> bool {
        (!self.shift || m.shift) && (!self.ctrl || m.ctrl) && (!self.alt || m.alt)
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
    pub help: crate::lang::Text,
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
use emcraft_render::view3d::{Camera, StandardView};

/// The mouse mappings, SolidCraft's. The first is the default.
pub const NAV_PRESETS: &[NavPreset] = &[
    NavPreset {
        id: FUSION,
        label: "Fusion",
        help: t!("Middle drag pans, Shift+middle orbits, right drag orbits"),
        reverse_zoom: false,
        middle_double_fits: false,
        bindings: &[bind(Middle, NavKeys::SHIFT, Orbit), bind(Middle, NavKeys::NONE, Pan), bind(Right, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "solidworks",
        label: "SolidWorks",
        help: t!("Middle drag orbits, Ctrl+middle pans, Shift+middle zooms"),
        reverse_zoom: true,
        middle_double_fits: true,
        bindings: &[bind(Middle, NavKeys::CTRL, Pan), bind(Middle, NavKeys::SHIFT, Zoom), bind(Middle, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "inventor",
        label: "Inventor",
        help: t!("Middle drag pans, Shift+middle orbits, Ctrl+middle zooms, right drag orbits"),
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
        help: t!("Shift+Alt+left drag orbits, Shift+Alt+middle pans, Shift+Alt+right zooms; middle drag also pans and right drag orbits"),
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
        help: t!("Alt+left drag orbits, Alt+middle pans, Alt+right zooms; middle drag also pans"),
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
        help: t!("Middle drag orbits, Shift+middle pans, Ctrl+middle zooms; right drag leaves the camera alone"),
        reverse_zoom: false,
        middle_double_fits: false,
        bindings: &[bind(Middle, NavKeys::SHIFT, Pan), bind(Middle, NavKeys::CTRL, Zoom), bind(Middle, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "tinkercad",
        label: "Tinkercad",
        help: t!("Right drag orbits, middle or Shift+right drag pans"),
        reverse_zoom: false,
        middle_double_fits: false,
        bindings: &[bind(Right, NavKeys::SHIFT, Pan), bind(Right, NavKeys::NONE, Orbit), bind(Middle, NavKeys::NONE, Pan)],
    },
    NavPreset {
        id: OPENSCAD,
        label: "OpenSCAD",
        help: t!("Left drag orbits; right or middle drag pans, Shift+right or Shift+middle drag zooms"),
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
        help: t!("Alt+left drag pans, Shift+Alt+left zooms, Ctrl+left drag orbits"),
        reverse_zoom: false,
        middle_double_fits: false,
        // Ctrl+left orbit is the drag every mapping has.
        bindings: &[bind(Left, NavKeys::SHIFT_ALT, Zoom), bind(Left, NavKeys::ALT, Pan)],
    },
    NavPreset {
        id: TOUCHPAD,
        label: "Touchpad",
        help: t!("Hold Shift and move the pointer to pan; hold Alt and move it to orbit. No button is needed. Scrolling zooms."),
        reverse_zoom: false,
        middle_double_fits: false,
        bindings: &[bind(Middle, NavKeys::NONE, Pan), bind(Right, NavKeys::NONE, Orbit)],
    },
];

impl NavPreset {
    /// What dragging `button` with `mods` held does, if anything.
    pub fn mode(&self, button: NavButton, mods: NavKeys) -> Option<NavMode> {
        self.bindings.iter().find(|b| b.button == button && b.keys.held(mods)).map(|b| b.mode)
    }

    /// What moving the pointer with no button down does (the touchpad mapping: Shift pans, Alt
    /// orbits; exactly that key held).
    pub fn motion_mode(&self, mods: NavKeys) -> Option<NavMode> {
        if self.id != TOUCHPAD || mods.ctrl {
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

/// The standard views on Ctrl+1…7, in their digits' order (as SolidCraft's).
pub const DIGIT_VIEWS: [StandardView; 7] =
    [StandardView::Front, StandardView::Back, StandardView::Left, StandardView::Right, StandardView::Top, StandardView::Bottom, StandardView::Iso];

/// A box (m): its low and high corners.
pub type Bounds = ([f64; 3], [f64; 3]);

/// A view change asked of the 3D view, animated from the camera it starts from to
/// [`Goal::camera`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Goal {
    /// A standard view's direction, keeping the framing.
    View(StandardView),
    /// The isometric view, framing the model.
    Home,
    /// The model framed, keeping the direction.
    Fit,
    /// A box (m) seen from a standard view, framed (facing the field's plane).
    Face([f64; 3], [f64; 3], StandardView),
    /// Turned toward the screen's right and up by degrees (arrow keys, the cube's arrows).
    Turn { right: f64, up: f64, degrees: f64 },
    /// Looking from a direction (the view cube's faces and corners), keeping the framing.
    From([f64; 3]),
}

impl Goal {
    /// The goal a standard view's button asks for: the isometric view also frames the model.
    pub fn standard(v: StandardView) -> Goal {
        if v == StandardView::Iso { Goal::Home } else { Goal::View(v) }
    }

    /// Where the change ends, starting from `from`, framing `frame` in a picture of `aspect`
    /// (width over height) where it frames.
    ///
    /// ```
    /// use emcraft_render::view3d::{Camera, StandardView};
    /// use emcraft_ui_core::navigation::Goal;
    /// let frame = ([0.0; 3], [0.02, 0.01, 0.05]);
    /// let fitted = Goal::Fit.camera(Camera::default(), frame, 1.5);
    /// assert_eq!(fitted.yaw, Camera::default().yaw);
    /// let top = Goal::View(StandardView::Top).camera(fitted, frame, 1.5);
    /// assert_eq!(top.distance, fitted.distance);
    /// // A turn by no number of degrees leaves the camera as it was.
    /// let c = Goal::Turn { right: 1.0, up: 0.0, degrees: f64::NAN }.camera(fitted, frame, 1.5);
    /// assert_eq!(c, fitted);
    /// ```
    pub fn camera(self, from: Camera, frame: Bounds, aspect: f64) -> Camera {
        let mut to = from;
        match self {
            Goal::View(view) => to.standard(view),
            Goal::Home => {
                to.standard(StandardView::Iso);
                to.fit(frame.0, frame.1, aspect);
            }
            Goal::Fit => to.fit(frame.0, frame.1, aspect),
            Goal::Face(lo, hi, view) => {
                to.standard(view);
                to.fit(lo, hi, aspect);
            }
            Goal::Turn { right, up, degrees } => to = to.stepped(right, up, degrees),
            Goal::From(dir) => to = to.looking_from(dir),
        }
        to.sanitized()
    }
}

/// The distances (m) a zoom keeps within, for a model of box `bounds`.
///
/// ```
/// let [near, far] = emcraft_ui_core::navigation::zoom_limits(([0.0; 3], [3.0, 4.0, 0.0]));
/// assert!((near - 5e-4).abs() < 1e-12 && (far - 5e3).abs() < 1e-9);
/// // A box of no size (or not a number) zooms as one of 1 m.
/// assert_eq!(emcraft_ui_core::navigation::zoom_limits(([f64::NAN; 3], [0.0; 3])), [1e-4, 1e3]);
/// ```
pub fn zoom_limits((lo, hi): Bounds) -> [f64; 2] {
    let size = (0..3).map(|i| (hi[i] - lo[i]).powi(2)).sum::<f64>().sqrt();
    let size = if size.is_finite() && size > 0.0 { size } else { 1.0 };
    [size * 1e-4, size * 1e3]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_mapping_is_solidcrafts_and_ids_are_distinct() {
        let p = preset("nonsense");
        assert_eq!(p.id, FUSION);
        let none = NavKeys::NONE;
        assert_eq!(p.mode(Right, none), Some(Orbit));
        assert_eq!(p.mode(Middle, none), Some(Pan));
        assert_eq!(p.mode(Middle, NavKeys::SHIFT), Some(Orbit));
        assert_eq!(p.mode(Left, none), None);
        let mut ids: Vec<&str> = NAV_PRESETS.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), NAV_PRESETS.len());
        assert_eq!(preset("blender").mode(Middle, none), Some(Orbit));
        assert_eq!(preset(TOUCHPAD).motion_mode(NavKeys::ALT), Some(Orbit));
        assert_eq!(preset(FUSION).motion_mode(NavKeys::ALT), None);
    }
}

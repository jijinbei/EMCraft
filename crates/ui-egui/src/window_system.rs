//! Which window system the desktop window opens on, and whether files dropped on it arrive.
//!
//! On Linux under a Wayland desktop, winit's Wayland backend delivers no dropped files, so the
//! window opens on X11 (XWayland) where there is one, and a drop works as on every other
//! system. `EMCRAFT_WINDOW=wayland` keeps it on Wayland (no drops then: the window then says
//! nothing of dropping); `EMCRAFT_WINDOW=x11` asks for X11.

use std::sync::atomic::{AtomicBool, Ordering};

static DROPS: AtomicBool = AtomicBool::new(true);

/// Whether a file dropped on the window reaches it.
pub fn drops_files() -> bool {
    DROPS.load(Ordering::Relaxed)
}

/// What to open on, from the environment: X11 unless Wayland is asked for or there is no X
/// server (`(x11, wayland_there)`).
///
/// ```
/// use emcraft_ui_egui::window_system::choose;
///
/// // GNOME on Wayland with XWayland: X11, for drops.
/// assert_eq!(choose(None, true, true), (true, true));
/// assert_eq!(choose(Some("wayland"), true, true), (false, true));
/// // Wayland alone.
/// assert_eq!(choose(None, false, true), (false, true));
/// ```
pub fn choose(asked: Option<&str>, x_server: bool, wayland: bool) -> (bool, bool) {
    let x11 = match asked {
        Some("wayland") => false,
        Some("x11") => true,
        _ => x_server,
    };
    (x11, wayland)
}

/// The event loop's hook that opens the window on the chosen system (Linux only).
#[cfg(target_os = "linux")]
pub fn hook() -> Option<eframe::EventLoopBuilderHook> {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let (x11, wayland) = choose(var("EMCRAFT_WINDOW").as_deref(), var("DISPLAY").is_some(), var("WAYLAND_DISPLAY").is_some());
    DROPS.store(x11 || !wayland, Ordering::Relaxed);
    if !x11 {
        return None;
    }
    Some(Box::new(|b| {
        use winit::platform::x11::EventLoopBuilderExtX11;
        b.with_x11();
    }))
}

#[cfg(not(target_os = "linux"))]
pub fn hook() -> Option<eframe::EventLoopBuilderHook> {
    None
}

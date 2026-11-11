//! The themes ([`emcraft_ui_core::theme`]) in egui: the current theme's tokens in egui's colours
//! ([`Tokens`]), the frames and shadows drawn with them, and egui's visuals and text styles from
//! them. The current theme belongs to the thread drawing the frame ([`set`], [`current`]).

use std::cell::RefCell;
use std::rc::Rc;

use egui::{Color32, Stroke};
use emcraft_engine::theme::{Base, Colour};
use emcraft_render::{Palette, Rgb};

pub use emcraft_ui_core::theme::{Emphasis, MAX_THEMES, SCENE_NAMES, Scene, TOKEN_NAMES, Theme, ThemeKey, Themes, default_dir};

/// The design tokens in egui's colours.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tokens(pub emcraft_ui_core::theme::Tokens<Color32>);

impl std::ops::Deref for Tokens {
    type Target = emcraft_ui_core::theme::Tokens<Color32>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A theme's colour as egui's.
pub fn colour32(c: Colour) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// `th`'s tokens in egui's colours.
pub fn tokens_of(th: &Theme) -> Tokens {
    Tokens(th.tokens.map(colour32))
}

impl Tokens {
    /// The tokens of the current theme.
    pub fn get() -> Tokens {
        CURRENT.with_borrow(|(_, t)| *t)
    }

    /// The soft shadow under floating cards.
    pub fn shell_shadow(&self) -> egui::Shadow {
        egui::Shadow { offset: [0, 2], blur: 8, spread: 0, color: self.app_bar.gamma_multiply(0.18) }
    }

    /// A floating card's frame: `fill`, the border, rounded corners and the shell shadow.
    pub fn card(&self, fill: Color32) -> egui::Frame {
        egui::Frame::NONE.fill(fill).stroke(Stroke::new(1.0, self.border)).corner_radius(self.radius_lg).shadow(self.shell_shadow())
    }
}

/// An egui colour as a picture's (its alpha dropped).
pub fn rgb(c: Color32) -> Rgb {
    Rgb(c.r(), c.g(), c.b())
}

/// A picture's colour as egui's.
pub fn color32(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// WCAG contrast of two opaque sRGB colours ([`Rgb::contrast_ratio`]).
pub fn contrast_ratio(a: Color32, b: Color32) -> f64 {
    rgb(a).contrast_ratio(rgb(b))
}

/// The built-in theme `base`, or greys with why.
fn builtin_or_fallback(base: Base) -> Theme {
    Theme::builtin(base).unwrap_or_else(|_| Theme::fallback(base))
}

thread_local! {
    /// The theme belongs to the thread drawing the frame, so tests that draw apps side by side
    /// on their own threads keep their own; its tokens kept in egui's colours.
    static CURRENT: RefCell<(Rc<Theme>, Tokens)> = RefCell::new({
        let th = Rc::new(builtin_or_fallback(Base::Light));
        let t = tokens_of(&th);
        (th, t)
    });
}

/// The current theme.
pub fn current() -> Rc<Theme> {
    CURRENT.with_borrow(|(t, _)| Rc::clone(t))
}

/// Switches themes (the visuals follow on the next [`apply`]).
pub fn set(theme: Rc<Theme>) {
    CURRENT.with_borrow_mut(|(t, tokens)| {
        if !Rc::ptr_eq(t, &theme) {
            *tokens = tokens_of(&theme);
            *t = theme;
        }
    });
}

/// Whether the current theme is dark (its base).
pub fn is_dark() -> bool {
    CURRENT.with_borrow(|(t, _)| t.is_dark())
}

/// The current theme's picture palette.
pub fn palette() -> Palette {
    CURRENT.with_borrow(|(t, _)| t.palette)
}

/// The current theme's 3D colours.
pub fn scene() -> Scene {
    CURRENT.with_borrow(|(t, _)| t.scene)
}

/// The current theme's colours for the 3D view's scene.
pub fn scene_colours() -> crate::scene3d::Colours {
    CURRENT.with_borrow(|(t, _)| crate::scene3d::Colours::of(t))
}

/// What cached pictures were drawn in.
pub fn key() -> ThemeKey {
    CURRENT.with_borrow(|(t, _)| t.key())
}

fn visuals(th: &Theme) -> egui::Visuals {
    let t = &tokens_of(th);
    let mut v = if th.is_dark() { egui::Visuals::dark() } else { egui::Visuals::light() };
    if th.fallback {
        return v;
    }
    v.panel_fill = t.panel;
    v.window_fill = t.dialog_body;
    v.window_stroke = Stroke::new(1.0, t.dialog_border);
    v.extreme_bg_color = t.control_fill;
    v.faint_bg_color = t.panel_header;
    v.override_text_color = Some(t.text);
    v.widgets.inactive.weak_bg_fill = t.panel_header;
    v.widgets.inactive.bg_fill = t.panel_header;
    v.widgets.active.weak_bg_fill = t.accent_soft;
    v.selection.bg_fill = t.accent_soft;
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    // Fields and buttons show their edge on the white of a card.
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.control_border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.hovered.bg_fill = t.hover;
    v.widgets.open.weak_bg_fill = t.panel_header;
    v.window_corner_radius = egui::CornerRadius::same(t.radius_xl);
    v.window_shadow = t.shell_shadow();
    v.popup_shadow = t.shell_shadow();
    v
}

/// Applies the current theme to the context's style.
pub fn apply(ctx: &egui::Context) {
    let th = current();
    ctx.set_theme(if th.is_dark() { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.set_visuals(visuals(&th));
    let t = tokens_of(&th);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(t.space_6, t.space_4);
        s.spacing.button_padding = egui::vec2(t.space_6, 3.0);
        s.spacing.interact_size.y = t.control_height - 6.0;
        // egui's own text styles follow the tokens, so plain labels and buttons size with them.
        use egui::{FontFamily, TextStyle};
        for (style, size, family) in [
            (TextStyle::Small, t.font_small, FontFamily::Proportional),
            (TextStyle::Body, t.font_body, FontFamily::Proportional),
            (TextStyle::Button, t.font_button, FontFamily::Proportional),
            (TextStyle::Heading, t.font_heading, FontFamily::Proportional),
            (TextStyle::Monospace, t.font_body - 0.5, FontFamily::Monospace),
        ] {
            s.text_styles.insert(style, egui::FontId::new(size, family));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_theme_is_the_threads_own() {
        let themes = Themes::load(None);
        set(themes.get("dark"));
        assert!(is_dark() && Tokens::get() == tokens_of(&themes.get("dark")) && key().name == "dark");
        assert_eq!(palette(), themes.get("dark").palette);
        std::thread::spawn(|| assert!(!is_dark())).join().unwrap();
        set(themes.get("light"));
        assert!(!is_dark());
        // Each copy of a theme has its own key.
        assert_ne!(Themes::load(None).get("light").key(), themes.get("light").key());
    }
}

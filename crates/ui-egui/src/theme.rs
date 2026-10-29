//! Colours, sizes and widget style: a light theme (the default) and a dark one. Every colour the
//! UI uses comes from [`Tokens`]; the palettes and the token names are SolidCraft's
//! (`crates/ui-egui/src/theme.rs`, `assets/styles/ui/palettes.rs`), reduced to what EMCraft uses,
//! with EMCraft's own per-tab accents.

use std::cell::Cell;

use egui::{Color32, Stroke};

use crate::ribbon::RibbonTab;

thread_local! {
    /// The theme belongs to the thread drawing the frame, so tests that draw apps side by side
    /// on their own threads keep their own.
    static DARK: Cell<bool> = const { Cell::new(false) };
}

pub fn is_dark() -> bool {
    DARK.get()
}

/// Switches themes (the visuals follow on the next [`apply`]).
pub fn set_dark(dark: bool) {
    DARK.set(dark);
}

/// The design tokens of one theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub space_2: f32,
    pub space_4: f32,
    pub space_6: f32,
    pub space_8: f32,
    pub space_12: f32,
    pub space_16: f32,
    pub radius_sm: u8,
    pub radius_md: u8,
    pub radius_lg: u8,
    pub radius_xl: u8,
    pub font_small: f32,
    pub font_body: f32,
    pub font_label: f32,
    pub font_button: f32,
    pub font_heading: f32,
    /// Height of form controls and rows.
    pub control_height: f32,
    pub control_radius: u8,
    /// Width of a form's label column.
    pub form_label_width: f32,
    pub form_header_height: f32,
    pub form_footer_height: f32,
    /// Outer width of a dialog card.
    pub form_card_width: f32,
    pub app_bar: Color32,
    pub app_bar_text: Color32,
    pub app_bar_hover: Color32,
    pub toolbar: Color32,
    pub panel: Color32,
    pub panel_header: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub on_accent: Color32,
    pub selected_ink: Color32,
    pub selected_bg: Color32,
    pub hover: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub success: Color32,
    pub form_ink: Color32,
    pub form_dim: Color32,
    pub form_error: Color32,
    pub form_error_fill: Color32,
    pub dialog_body: Color32,
    pub dialog_footer: Color32,
    pub dialog_rule: Color32,
    pub dialog_border: Color32,
    pub control_fill: Color32,
    pub control_border: Color32,
    pub viewport_top: Color32,
    pub viewport_bottom: Color32,
    pub icon: Color32,
    /// Per-tab accents (fill, ink on light grounds, soft tint): geometry, materials, ports and
    /// sources, mesh, simulation, results.
    pub tab_model: [Color32; 3],
    pub tab_materials: [Color32; 3],
    pub tab_ports: [Color32; 3],
    pub tab_mesh: [Color32; 3],
    pub tab_simulate: [Color32; 3],
    pub tab_results: [Color32; 3],
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Tokens {
    pub const fn light() -> Tokens {
        Tokens {
            space_2: 2.0,
            space_4: 4.0,
            space_6: 6.0,
            space_8: 8.0,
            space_12: 12.0,
            space_16: 16.0,
            radius_sm: 4,
            radius_md: 6,
            radius_lg: 8,
            radius_xl: 10,
            font_small: 11.0,
            font_body: 12.0,
            font_label: 12.5,
            font_button: 13.0,
            font_heading: 14.0,
            control_height: 26.0,
            control_radius: 6,
            form_label_width: 96.0,
            form_header_height: 40.0,
            form_footer_height: 48.0,
            form_card_width: 380.0,
            app_bar: rgb(0x343840),
            app_bar_text: rgb(0xe6eaf2),
            app_bar_hover: Color32::from_rgba_premultiplied(28, 28, 28, 28),
            toolbar: rgb(0xf6f7f9),
            panel: rgb(0xfafbfc),
            panel_header: rgb(0xeceef2),
            border: rgb(0xd0d4dc),
            text: rgb(0x262a32),
            text_dim: rgb(0x6e7480),
            accent: rgb(0x2678da),
            accent_soft: rgb(0xd6e6fa),
            on_accent: Color32::WHITE,
            selected_ink: rgb(0x1a507f),
            selected_bg: rgb(0xe8f1fa),
            hover: rgb(0xe2eaf6),
            error: rgb(0xd63636),
            warning: rgb(0xde961e),
            success: rgb(0x276b2c),
            form_ink: rgb(0x1b2430),
            form_dim: rgb(0x4a5868),
            form_error: rgb(0xa1342c),
            form_error_fill: rgb(0xfcefed),
            dialog_body: rgb(0xffffff),
            dialog_footer: rgb(0xf4f6f9),
            dialog_rule: rgb(0xe6eaf0),
            dialog_border: rgb(0xd5dbe3),
            control_fill: rgb(0xffffff),
            control_border: rgb(0xc3cbd6),
            viewport_top: rgb(0xf0f3f7),
            viewport_bottom: rgb(0xcbd2dd),
            icon: rgb(0x3c4452),
            tab_model: [rgb(0x2678da), rgb(0x1a507f), rgb(0xd6e6fa)],
            tab_materials: [rgb(0xa45000), rgb(0xa45000), rgb(0xfbebd8)],
            tab_ports: [rgb(0x057f45), rgb(0x057f45), rgb(0xedf7f1)],
            tab_mesh: [rgb(0xe0b200), rgb(0x8a6d00), rgb(0xfff8dc)],
            tab_simulate: [rgb(0x7349cd), rgb(0x7349cd), rgb(0xeee8fa)],
            tab_results: [rgb(0xa8326e), rgb(0xa8326e), rgb(0xf8e6ef)],
        }
    }

    pub const fn dark() -> Tokens {
        let mut t = Tokens::light();
        t.app_bar = rgb(0x1c1f26);
        t.toolbar = rgb(0x272b34);
        t.panel = rgb(0x272b34);
        t.panel_header = rgb(0x2f343e);
        t.border = rgb(0x3f4552);
        t.text = rgb(0xdee2e8);
        t.text_dim = rgb(0x949ba8);
        t.accent = rgb(0x3a88ee);
        t.accent_soft = rgb(0x2a466e);
        t.selected_ink = rgb(0x9cc7f5);
        t.selected_bg = rgb(0x2a466e);
        t.hover = rgb(0x363d4a);
        t.error = rgb(0xf06460);
        t.warning = rgb(0xecaa46);
        t.success = rgb(0x6cc070);
        t.form_ink = rgb(0xdee2e8);
        t.form_dim = rgb(0xb4bbc8);
        t.form_error = rgb(0xf3908a);
        t.form_error_fill = rgb(0x4a2a2a);
        t.dialog_body = rgb(0x3b4453);
        t.dialog_footer = rgb(0x2f343e);
        t.dialog_rule = rgb(0x353b47);
        t.dialog_border = rgb(0x485363);
        t.control_fill = rgb(0x272b34);
        t.control_border = rgb(0x4a5262);
        t.viewport_top = rgb(0x2e3440);
        t.viewport_bottom = rgb(0x2e3440);
        t.icon = rgb(0xd0d6e2);
        t.tab_model = [rgb(0x3a88ee), rgb(0x9cc7f5), rgb(0x2a466e)];
        t.tab_materials = [rgb(0xb26a14), rgb(0xe0914a), rgb(0x45331e)];
        t.tab_ports = [rgb(0x128d45), rgb(0x46be78), rgb(0x1f3b2b)];
        t.tab_mesh = [rgb(0xc6a800), rgb(0xd4b726), rgb(0x41391a)];
        t.tab_simulate = [rgb(0x8a6fcc), rgb(0xb39bee), rgb(0x372f4f)];
        t.tab_results = [rgb(0xc8588f), rgb(0xe074ae), rgb(0x402336)];
        t
    }

    /// The tokens of the current theme.
    pub fn get() -> Tokens {
        if is_dark() { Tokens::dark() } else { Tokens::light() }
    }

    /// A ribbon tab's accent: (fill, ink, soft).
    pub fn tab(&self, tab: RibbonTab) -> [Color32; 3] {
        match tab {
            RibbonTab::Model => self.tab_model,
            RibbonTab::Materials => self.tab_materials,
            RibbonTab::PortsSources => self.tab_ports,
            RibbonTab::Mesh => self.tab_mesh,
            RibbonTab::Simulate => self.tab_simulate,
            RibbonTab::Results => self.tab_results,
        }
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

/// WCAG contrast of two opaque sRGB colours.
pub fn contrast_ratio(a: Color32, b: Color32) -> f64 {
    let luminance = |c: Color32| {
        let linear = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
    };
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn visuals(dark: bool) -> egui::Visuals {
    let t = if dark { Tokens::dark() } else { Tokens::light() };
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
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
    let dark = is_dark();
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.set_visuals(visuals(dark));
    let t = Tokens::get();
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(t.space_6, t.space_4);
        s.spacing.button_padding = egui::vec2(t.space_6, 3.0);
        s.spacing.interact_size.y = t.control_height - 6.0;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Text and tab inks are readable on their grounds in both themes.
    #[test]
    fn inks_hold_wcag_contrast_in_both_themes() {
        for t in [Tokens::light(), Tokens::dark()] {
            assert!(contrast_ratio(t.text, t.panel) >= 7.0);
            assert!(contrast_ratio(t.form_ink, t.dialog_body) >= 7.0);
            assert!(contrast_ratio(t.text_dim, t.panel) >= 3.0);
            assert!(contrast_ratio(t.on_accent, t.accent) >= 3.0);
            assert!(contrast_ratio(t.form_error, t.form_error_fill) >= 4.5);
            for tab in RibbonTab::ALL {
                let [_, ink, soft] = t.tab(tab);
                assert!(contrast_ratio(ink, soft) >= 4.5, "{tab:?}");
                assert!(contrast_ratio(ink, t.toolbar) >= 4.5, "{tab:?}");
            }
        }
    }
}

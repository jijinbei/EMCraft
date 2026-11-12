//! The window's look from the theme ([`emcraft_ui_core::theme`]): its tokens in iced's colours,
//! iced's theme seeded from them, and the styles of the window's parts — buttons, fields,
//! choices, cards and their headers, the canvas' ground, the application bar — each a function
//! of the tokens, so every theme (built-in, tinted or the user's) dresses the iced window as it
//! dresses the egui one.

use std::rc::Rc;

use emcraft_engine::theme::Colour;
use emcraft_ui_core::theme::Theme;
use iced::widget::{button as b, container, pick_list, text_input};
use iced::{Background, Border, Color, Shadow};

/// The design tokens in iced's colours.
pub type Tokens = emcraft_ui_core::theme::Tokens<Color>;

/// A theme's colour as iced's.
pub fn colour(c: Colour) -> Color {
    Color::from_rgba8(c.r, c.g, c.b, f32::from(c.a) / 255.0)
}

/// A picture's colour as iced's.
pub fn rgb(c: emcraft_render::Rgb) -> Color {
    Color::from_rgb8(c.0, c.1, c.2)
}

/// The theme the window is dressed in, and its tokens in iced's colours.
#[derive(Clone, Debug)]
pub struct Look {
    pub theme: Rc<Theme>,
    pub tokens: Tokens,
}

impl Look {
    pub fn of(theme: Rc<Theme>) -> Look {
        let tokens = theme.tokens.map(colour);
        Look { theme, tokens }
    }

    /// iced's own theme seeded from the tokens: what the parts styled here leave to it (menus,
    /// scroll bars, the window's ground).
    pub fn iced(&self) -> iced::Theme {
        let t = &self.tokens;
        let seed = iced::theme::palette::Seed {
            background: t.panel,
            text: t.text,
            primary: t.accent,
            success: t.success,
            warning: t.warning,
            danger: t.error,
        };
        iced::Theme::custom(self.theme.name.clone(), seed)
    }
}

fn border(c: Color, radius: u8) -> Border {
    Border { color: c, width: 1.0, radius: f32::from(radius).into() }
}

/// A button: the primary one filled with the accent, the others on the controls' fill.
pub fn button(t: Tokens, primary: bool) -> impl Fn(&iced::Theme, b::Status) -> b::Style {
    move |_, status| {
        let (fill, ink, edge) = if primary { (t.accent, t.on_accent, t.accent) } else { (t.control_fill, t.text, t.control_border) };
        let fill = match status {
            b::Status::Hovered | b::Status::Pressed if !primary => t.hover,
            b::Status::Hovered | b::Status::Pressed => t.selected_ink,
            b::Status::Disabled => t.panel_header,
            b::Status::Active => fill,
        };
        b::Style {
            background: Some(Background::Color(fill)),
            text_color: ink,
            border: border(edge, t.control_radius),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// A button that is only its text (a mark on a card, a link's ×), lit when hovered.
pub fn quiet(t: Tokens) -> impl Fn(&iced::Theme, b::Status) -> b::Style {
    move |_, status| {
        let ink = if matches!(status, b::Status::Hovered | b::Status::Pressed) { t.accent } else { t.text_dim };
        b::Style { background: None, text_color: ink, border: Border::default(), shadow: Shadow::default(), snap: true }
    }
}

/// A browser row: its text on nothing, the hover's fill under the pointer, the selection's when
/// `selected`.
pub fn row(t: Tokens, selected: bool) -> impl Fn(&iced::Theme, b::Status) -> b::Style {
    move |_, status| {
        let fill = match (selected, status) {
            (true, _) => Some(t.selected_bg),
            (false, b::Status::Hovered | b::Status::Pressed) => Some(t.hover),
            _ => None,
        };
        b::Style {
            background: fill.map(Background::Color),
            text_color: if selected { t.selected_ink } else { t.text },
            border: Border { radius: 4.0.into(), ..Border::default() },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// A text field: the controls' fill and edge, the accent's edge while typed in.
pub fn field(t: Tokens) -> impl Fn(&iced::Theme, text_input::Status) -> text_input::Style {
    move |_, status| {
        let edge = match status {
            text_input::Status::Focused { .. } => t.accent,
            text_input::Status::Hovered => t.text_dim,
            text_input::Status::Active | text_input::Status::Disabled => t.control_border,
        };
        text_input::Style {
            background: Background::Color(t.control_fill),
            border: border(edge, t.radius_sm),
            placeholder: t.text_dim,
            value: t.form_ink,
            selection: t.accent_soft,
        }
    }
}

/// A choice among names: as a field, its arrow dim.
pub fn choice(t: Tokens) -> impl Fn(&iced::Theme, pick_list::Status) -> pick_list::Style {
    move |_, status| {
        let edge = match status {
            pick_list::Status::Opened { .. } => t.accent,
            pick_list::Status::Hovered => t.text_dim,
            pick_list::Status::Active | pick_list::Status::Disabled => t.control_border,
        };
        pick_list::Style {
            text_color: t.form_ink,
            placeholder_color: t.text_dim,
            handle_color: t.text_dim,
            background: Background::Color(t.control_fill),
            border: border(edge, t.radius_sm),
        }
    }
}

/// A node's card: a dialog's body on the chain, the panel's header off it.
pub fn card(t: Tokens, on_chain: bool) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| container::Style {
        text_color: Some(t.form_ink),
        background: Some(Background::Color(if on_chain { t.dialog_body } else { t.panel_header })),
        border: border(t.border, t.radius_lg),
        shadow: Shadow { color: Color { a: 0.18, ..t.app_bar }, offset: iced::Vector::new(0.0, 2.0), blur_radius: 8.0 },
        snap: true,
    }
}

/// A card's header: the accent's soft tint on the chain, the border's grey off it.
pub fn header(t: Tokens, on_chain: bool) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| {
        let r = f32::from(t.radius_lg);
        container::Style {
            text_color: Some(t.text),
            background: Some(Background::Color(if on_chain { t.accent_soft } else { t.border })),
            border: Border { radius: iced::border::Radius { top_left: r, top_right: r, bottom_left: 0.0, bottom_right: 0.0 }, ..Border::default() },
            ..container::Style::default()
        }
    }
}

/// The canvas' ground: the panel, its edge.
pub fn ground(t: Tokens) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| container::Style { background: Some(Background::Color(t.panel)), border: border(t.border, t.radius_lg), ..container::Style::default() }
}

/// The application bar.
pub fn app_bar(t: Tokens) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| container::Style { text_color: Some(t.app_bar_text), background: Some(Background::Color(t.app_bar)), ..container::Style::default() }
}

/// The window's ground under everything: the ribbon's.
pub fn window(t: Tokens) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| container::Style { text_color: Some(t.text), background: Some(Background::Color(t.toolbar)), ..container::Style::default() }
}

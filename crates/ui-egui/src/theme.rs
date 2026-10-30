//! Themes: the colours of the window ([`Tokens`]), of the 3D view ([`Scene`]) and of pictures
//! ([`Palette`]), and the sizes and widget style that go with them. The colours are data: the
//! built-in themes are the files `crates/engine/themes/*.json` (`light`, `dark` and the one-hue
//! ones, compiled in, read by [`emcraft_engine::theme`]), and the user's own themes are files of the same format in
//! `<config dir>/themes/*.json` (docs/themes.md), each overriding its base's colours by name.
//! The token names and the palettes' structure are SolidCraft's (`crates/ui-egui/src/theme.rs`),
//! reduced to what EMCraft uses, with EMCraft's own per-tab accents.
//!
//! A theme that cannot be read (bad JSON, a bad colour, an unknown name, text that fails WCAG
//! contrast) is left out and reported; should a built-in one ever fail, egui's own visuals stand
//! in. The current theme belongs to the thread drawing the frame ([`set`], [`current`]).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use egui::{Color32, Stroke};
use emcraft_engine::theme::{Base, ThemeFile};
use emcraft_render::{Palette, Rgb};

use crate::ribbon::RibbonTab;

/// The design tokens of one theme: sizes (here) and colours (from its file).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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
    /// The app bar's button of the page that is open.
    pub app_bar_active: Color32,
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
    /// A status that asks for another look (a stale step).
    pub info: Color32,
    /// The marks drawn on a status's disc.
    pub on_status: Color32,
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
    /// The 3D view's axes (x, y, z) and the letters on them.
    pub axis_x: Color32,
    pub axis_y: Color32,
    pub axis_z: Color32,
    pub axis_label: Color32,
    /// Per-tab accents (fill, ink on light grounds, soft tint): geometry, materials, ports and
    /// sources, boundaries, mesh, simulation, results.
    pub tab_model: [Color32; 3],
    pub tab_materials: [Color32; 3],
    pub tab_ports: [Color32; 3],
    pub tab_boundaries: [Color32; 3],
    pub tab_mesh: [Color32; 3],
    pub tab_simulate: [Color32; 3],
    pub tab_results: [Color32; 3],
}

/// The 3D view's colours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scene {
    /// Walls, and conductors inside.
    pub metal: Rgb,
    pub metal_dark: Rgb,
    pub port: Rgb,
    pub port_soft: Rgb,
    pub edge: Rgb,
    /// The open boundary's layer.
    pub pml: Rgb,
    /// The rim under field arrows.
    pub rim: Rgb,
    pub source: Rgb,
    /// A periodic boundary's faces.
    pub periodic: Rgb,
    /// What picked things turn towards.
    pub highlight: Rgb,
    pub vacuum: Rgb,
    /// The other materials, by their index (cycled).
    pub materials: [Rgb; 6],
    pub symmetry_electric: Rgb,
    pub symmetry_magnetic: Rgb,
    /// Walls in the mesh overlay.
    pub mesh_wall: Rgb,
    /// A layered guide's strips.
    pub strip: Rgb,
    /// A feature that intersects the solid.
    pub intersect: Rgb,
}

/// Generates the names of a colour set's colours, as theme files write them, from one table:
/// `"name" => field` or `"name" => field[index]`.
macro_rules! colour_names {
    ($set:ident: $colour:ty, $names:ident { $($name:literal => $field:ident $([$i:literal])?),* $(,)? }) => {
        /// The names of the colours, as theme files write them.
        pub const $names: &[&str] = &[$($name),*];

        impl $set {
            /// The colour named `name`.
            pub fn colour(&self, name: &str) -> Option<$colour> {
                match name {
                    $($name => Some(self.$field $([$i])?),)*
                    _ => None,
                }
            }

            /// The colour named `name`, to change.
            pub fn colour_mut(&mut self, name: &str) -> Option<&mut $colour> {
                match name {
                    $($name => Some(&mut self.$field $([$i])?),)*
                    _ => None,
                }
            }
        }
    };
}

colour_names!(Tokens: Color32, TOKEN_NAMES {
    "app_bar" => app_bar, "app_bar_text" => app_bar_text, "app_bar_hover" => app_bar_hover, "app_bar_active" => app_bar_active,
    "toolbar" => toolbar,
    "panel" => panel, "panel_header" => panel_header, "border" => border, "text" => text, "text_dim" => text_dim,
    "accent" => accent, "accent_soft" => accent_soft, "on_accent" => on_accent, "selected_ink" => selected_ink,
    "selected_bg" => selected_bg, "hover" => hover, "error" => error, "warning" => warning, "success" => success,
    "info" => info, "on_status" => on_status, "form_ink" => form_ink, "form_dim" => form_dim, "form_error" => form_error,
    "form_error_fill" => form_error_fill, "dialog_body" => dialog_body, "dialog_footer" => dialog_footer,
    "dialog_rule" => dialog_rule, "dialog_border" => dialog_border, "control_fill" => control_fill,
    "control_border" => control_border, "viewport_top" => viewport_top, "viewport_bottom" => viewport_bottom, "icon" => icon,
    "axis_x" => axis_x, "axis_y" => axis_y, "axis_z" => axis_z, "axis_label" => axis_label,
    "tab_model_fill" => tab_model[0], "tab_model_ink" => tab_model[1], "tab_model_soft" => tab_model[2],
    "tab_materials_fill" => tab_materials[0], "tab_materials_ink" => tab_materials[1], "tab_materials_soft" => tab_materials[2],
    "tab_ports_fill" => tab_ports[0], "tab_ports_ink" => tab_ports[1], "tab_ports_soft" => tab_ports[2],
    "tab_boundaries_fill" => tab_boundaries[0], "tab_boundaries_ink" => tab_boundaries[1], "tab_boundaries_soft" => tab_boundaries[2],
    "tab_mesh_fill" => tab_mesh[0], "tab_mesh_ink" => tab_mesh[1], "tab_mesh_soft" => tab_mesh[2],
    "tab_simulate_fill" => tab_simulate[0], "tab_simulate_ink" => tab_simulate[1], "tab_simulate_soft" => tab_simulate[2],
    "tab_results_fill" => tab_results[0], "tab_results_ink" => tab_results[1], "tab_results_soft" => tab_results[2],
});

colour_names!(Scene: Rgb, SCENE_NAMES {
    "metal" => metal, "metal_dark" => metal_dark, "port" => port, "port_soft" => port_soft, "edge" => edge, "pml" => pml,
    "rim" => rim, "source" => source, "periodic" => periodic, "highlight" => highlight, "vacuum" => vacuum,
    "material_1" => materials[0], "material_2" => materials[1], "material_3" => materials[2],
    "material_4" => materials[3], "material_5" => materials[4], "material_6" => materials[5],
    "symmetry_electric" => symmetry_electric, "symmetry_magnetic" => symmetry_magnetic, "mesh_wall" => mesh_wall,
    "strip" => strip, "intersect" => intersect,
});

impl Tokens {
    /// The sizes every theme shares; the colours left transparent for a theme to fill.
    pub fn sizes() -> Tokens {
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
            ..Tokens::default()
        }
    }

    /// The tokens of the current theme.
    pub fn get() -> Tokens {
        CURRENT.with_borrow(|t| t.tokens)
    }

    /// A ribbon tab's accent: (fill, ink, soft).
    pub fn tab(&self, tab: RibbonTab) -> [Color32; 3] {
        match tab {
            RibbonTab::Model => self.tab_model,
            // The walls take the materials' accent: they were under MATERIALS before.
            RibbonTab::Materials => self.tab_materials,
            RibbonTab::Boundaries => self.tab_boundaries,
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

    /// The first pair of colours below the contrast it needs (WCAG), as a sentence.
    pub fn contrast_problem(&self) -> Option<String> {
        let mut pairs = vec![
            ("text", self.text, "panel", self.panel, 7.0),
            ("form_ink", self.form_ink, "dialog_body", self.dialog_body, 7.0),
            ("text_dim", self.text_dim, "panel", self.panel, 3.0),
            ("on_accent", self.on_accent, "accent", self.accent, 3.0),
            ("form_error", self.form_error, "form_error_fill", self.form_error_fill, 4.5),
        ];
        for tab in RibbonTab::ALL {
            let [_, ink, soft] = self.tab(tab);
            pairs.push(("a tab's ink", ink, "its soft tint", soft, 4.5));
            pairs.push(("a tab's ink", ink, "toolbar", self.toolbar, 4.5));
        }
        pairs.into_iter().find_map(|(a, ca, b, cb, least)| {
            let r = contrast_ratio(ca, cb);
            (r < least).then(|| format!("`{a}` on `{b}` has a contrast of {r:.2}, below {least}"))
        })
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

/// The most user themes loaded.
pub const MAX_THEMES: usize = 32;
/// The most directory entries looked at for them.
const MAX_ENTRIES: usize = 4096;

/// Each theme built gets its own number, so pictures drawn in an older copy of a theme (before
/// its file was read again) are drawn again.
static SERIAL: AtomicU64 = AtomicU64::new(1);

/// A theme: its name, the built-in theme it starts from, and its colours.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    pub description: String,
    pub base: Base,
    pub tokens: Tokens,
    pub scene: Scene,
    pub palette: Palette,
    /// egui's own visuals stand in (a built-in theme could not be read).
    pub fallback: bool,
    serial: u64,
}

/// What a cached picture was drawn in: the theme's name and which copy of it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ThemeKey {
    pub name: String,
    serial: u64,
}

/// `file`'s `ui` and `scene` colours over `tokens` and `scene`; refused at the first unknown
/// name, with the names there are.
fn overlay(file: &ThemeFile, tokens: &mut Tokens, scene: &mut Scene) -> Result<(), String> {
    for (k, c) in &file.ui {
        let slot = tokens.colour_mut(k).ok_or_else(|| format!("`ui` has no token `{k}` (see docs/themes.md for the names)"))?;
        *slot = Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
    }
    for (k, c) in &file.scene {
        let slot = scene.colour_mut(k).ok_or_else(|| format!("`scene` has no colour `{k}` (it has {})", SCENE_NAMES.join(", ")))?;
        *slot = c.rgb();
    }
    Ok(())
}

impl Theme {
    fn new(file: &ThemeFile, tokens: Tokens, scene: Scene) -> Theme {
        Theme {
            name: file.name.clone(),
            description: file.description.clone(),
            base: file.base,
            tokens,
            scene,
            palette: file.picture,
            fallback: false,
            serial: SERIAL.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// A built-in theme from its file, which names every colour.
    pub fn builtin(base: Base) -> Result<Theme, String> {
        let file = base.theme().map_err(|e| e.to_string())?;
        let missing: Vec<&str> = TOKEN_NAMES
            .iter()
            .filter(|k| !file.ui.contains_key(**k))
            .chain(SCENE_NAMES.iter().filter(|k| !file.scene.contains_key(**k)))
            .copied()
            .collect();
        if !missing.is_empty() {
            return Err(format!("the built-in theme `{base}` lacks {}", missing.join(", ")));
        }
        let (mut tokens, mut scene) = (Tokens::sizes(), Scene::default());
        overlay(file, &mut tokens, &mut scene)?;
        let t = Theme::new(file, tokens, scene);
        t.readable()?;
        Ok(t)
    }

    /// A user's theme: `file`'s colours over its base's (`base`).
    pub fn user(file: &ThemeFile, base: &Theme) -> Result<Theme, String> {
        let (mut tokens, mut scene) = (base.tokens, base.scene);
        overlay(file, &mut tokens, &mut scene)?;
        let t = Theme::new(file, tokens, scene);
        t.readable()?;
        Ok(t)
    }

    /// The last resort when the built-in theme `base` cannot be read: egui's own visuals, greys
    /// for the 3D view, black on white for pictures.
    pub fn fallback(base: Base) -> Theme {
        let v = if base.is_dark() { egui::Visuals::dark() } else { egui::Visuals::light() };
        let mut tokens = Tokens::sizes();
        for name in TOKEN_NAMES {
            if let Some(c) = tokens.colour_mut(name) {
                *c = if name.contains("ink") || name.contains("text") || name.contains("error") { v.text_color() } else { v.panel_fill };
            }
        }
        (tokens.accent, tokens.border, tokens.hover) = (v.selection.bg_fill, v.widgets.noninteractive.bg_stroke.color, v.widgets.hovered.bg_fill);
        let grey = Rgb::BLACK.mix(Rgb::WHITE, 0.6);
        let mut scene = Scene { edge: Rgb::BLACK, ..Scene::default() };
        for name in SCENE_NAMES.iter().filter(|n| **n != "edge") {
            if let Some(c) = scene.colour_mut(name) {
                *c = grey;
            }
        }
        Theme {
            name: base.id().to_owned(),
            description: String::new(),
            base,
            tokens,
            scene,
            palette: Palette::fallback(),
            fallback: true,
            serial: SERIAL.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Refused when text would be hard to read: the window's inks and the pictures' (WCAG).
    pub fn readable(&self) -> Result<(), String> {
        match self.tokens.contrast_problem().or_else(|| self.palette.contrast_problem().map(|p| format!("`picture`: {p}"))) {
            Some(p) => Err(p),
            None => Ok(()),
        }
    }

    pub fn is_dark(&self) -> bool {
        self.base.is_dark()
    }

    pub fn key(&self) -> ThemeKey {
        ThemeKey { name: self.name.clone(), serial: self.serial }
    }
}

/// The built-in theme `base`, or the fallback with why.
fn builtin_or_fallback(base: Base) -> (Theme, Option<String>) {
    match Theme::builtin(base) {
        Ok(t) => (t, None),
        Err(e) => (Theme::fallback(base), Some(e)),
    }
}

thread_local! {
    /// The theme belongs to the thread drawing the frame, so tests that draw apps side by side
    /// on their own threads keep their own.
    static CURRENT: RefCell<Rc<Theme>> = RefCell::new(Rc::new(builtin_or_fallback(Base::Light).0));
}

/// The current theme.
pub fn current() -> Rc<Theme> {
    CURRENT.with_borrow(Rc::clone)
}

/// Switches themes (the visuals follow on the next [`apply`]).
pub fn set(theme: Rc<Theme>) {
    CURRENT.with_borrow_mut(|t| {
        if !Rc::ptr_eq(t, &theme) {
            *t = theme;
        }
    });
}

/// Whether the current theme is dark (its base).
pub fn is_dark() -> bool {
    CURRENT.with_borrow(|t| t.is_dark())
}

/// The current theme's picture palette.
pub fn palette() -> Palette {
    CURRENT.with_borrow(|t| t.palette)
}

/// The current theme's 3D colours.
pub fn scene() -> Scene {
    CURRENT.with_borrow(|t| t.scene)
}

/// What cached pictures were drawn in.
pub fn key() -> ThemeKey {
    CURRENT.with_borrow(|t| t.key())
}

/// The themes there are: the built-in ones, then the user's (by file name), and what was wrong
/// with the files left out.
#[derive(Clone, Debug)]
pub struct Themes {
    pub list: Vec<Rc<Theme>>,
    pub problems: Vec<String>,
    /// Where the user's themes were read from.
    pub dir: Option<PathBuf>,
}

impl Default for Themes {
    fn default() -> Self {
        Themes::load(None)
    }
}

/// Where user themes live: the `themes` folder of the configuration directory.
pub fn default_dir() -> Option<PathBuf> {
    crate::prefs::config_dir().map(|d| d.join("themes"))
}

impl Themes {
    /// The built-in themes and the user's in `dir` (none without one).
    pub fn load(dir: Option<&Path>) -> Themes {
        let mut out = Themes { list: Vec::new(), problems: Vec::new(), dir: dir.map(Path::to_path_buf) };
        for base in Base::ALL {
            let (t, problem) = builtin_or_fallback(base);
            if let Some(p) = problem {
                out.problems.push(format!("{p}; using egui's own colours"));
            }
            out.list.push(Rc::new(t));
        }
        if let Some(dir) = dir {
            out.read_dir(dir);
        }
        out
    }

    /// The user's themes in `dir`: its `.json` files by name, at most [`MAX_THEMES`].
    fn read_dir(&mut self, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut files: Vec<PathBuf> = entries
            .take(MAX_ENTRIES)
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json") && p.is_file())
            .collect();
        files.sort();
        if files.len() > MAX_THEMES {
            self.problems.push(format!("{} theme files in {}: the first {MAX_THEMES} are read", files.len(), dir.display()));
            files.truncate(MAX_THEMES);
        }
        for path in files {
            let shown = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            match self.read_file(&path) {
                Ok(t) => self.list.push(Rc::new(t)),
                Err(e) => self.problems.push(format!("theme {shown} left out: {e}")),
            }
        }
    }

    fn read_file(&self, path: &Path) -> Result<Theme, String> {
        let file = emcraft_engine::theme::parse(&emcraft_engine::theme::read_text(&path.to_string_lossy()).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if self.find(&file.name).is_some() {
            return Err(format!("a theme named `{}` is already loaded", file.name));
        }
        let base = self.find(file.base.id()).ok_or_else(|| format!("no base theme `{}`", file.base))?;
        Theme::user(&file, &base)
    }

    /// The theme named `name`.
    pub fn find(&self, name: &str) -> Option<Rc<Theme>> {
        self.list.iter().find(|t| t.name == name).cloned()
    }

    /// The theme named `name`, or the light one when there is none such.
    pub fn get(&self, name: &str) -> Rc<Theme> {
        self.find(name).or_else(|| self.list.first().cloned()).unwrap_or_else(|| Rc::new(builtin_or_fallback(Base::Light).0))
    }

    pub fn names(&self) -> Vec<&str> {
        self.list.iter().map(|t| t.name.as_str()).collect()
    }
}

fn visuals(th: &Theme) -> egui::Visuals {
    let t = &th.tokens;
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
    let t = th.tokens;
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(t.space_6, t.space_4);
        s.spacing.button_padding = egui::vec2(t.space_6, 3.0);
        s.spacing.interact_size.y = t.control_height - 6.0;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every built-in theme names every token and scene colour, no unknown one, and keep their
    /// text and the pictures' ink readable: the fallback is never needed.
    #[test]
    fn built_in_themes_are_whole_and_hold_wcag_contrast() {
        let themes = Themes::load(None);
        assert_eq!(themes.problems, Vec::<String>::new());
        assert_eq!(themes.names(), Base::IDS);
        for base in Base::ALL {
            let file = base.theme().unwrap();
            let names = |m: &std::collections::BTreeMap<String, _>| m.keys().cloned().collect::<Vec<String>>();
            let mut want: Vec<String> = TOKEN_NAMES.iter().map(|s| (*s).to_owned()).collect();
            want.sort();
            assert_eq!(names(&file.ui), want, "{base}: the ui tokens");
            let mut want: Vec<String> = SCENE_NAMES.iter().map(|s| (*s).to_owned()).collect();
            want.sort();
            assert_eq!(names(&file.scene), want, "{base}: the scene colours");
            let t = Theme::builtin(base).unwrap();
            assert!(!t.fallback && t.readable().is_ok());
            // Every colour is set (a transparent one would be a token the file forgot).
            for name in TOKEN_NAMES {
                assert_ne!(t.tokens.colour(name), Some(Color32::TRANSPARENT), "{base}: {name}");
            }
            assert!(contrast_ratio(t.tokens.text, t.tokens.panel) >= 7.0);
            assert!(t.palette.ink.contrast_ratio(t.palette.ground) >= 4.5);
            assert_eq!(t.is_dark(), base.is_dark());
        }
        assert_eq!(TOKEN_NAMES.len(), 59);
        assert_eq!(Tokens::sizes().colour("text"), Some(Color32::TRANSPARENT));
        assert_eq!(Tokens::sizes().colour("nope"), None);
        // The fallback stands in with readable text.
        for base in Base::ALL {
            let f = Theme::fallback(base);
            assert!(f.fallback && contrast_ratio(f.tokens.text, f.tokens.panel) >= 4.5, "{base}");
        }
    }

    /// User themes from a folder: each over its base, the bad ones left out with their reason.
    #[test]
    fn user_themes_load_from_a_folder_and_bad_ones_are_reported() {
        let dir = std::env::temp_dir().join(format!("emcraft-themes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
        write(
            "a-night.json",
            r##"{"name": "Night", "base": "dark", "ui": {"accent": "#2f7fd8"}, "scene": {"port": "#ff9a3c"}, "picture": {"ground": "#20242c"}}"##,
        );
        write("b-paper.json", r##"{"name": "Paper", "ui": {"panel": "#fffdf5"}}"##);
        write("c-broken.json", "{ not json");
        write("d-unknown.json", r##"{"name": "Odd", "ui": {"acent": "#4a9cf0"}}"##);
        write("e-faint.json", r##"{"name": "Faint", "ui": {"text": "#d0d0d0"}}"##);
        write("f-again.json", r##"{"name": "Night"}"##);
        write("g-scene.json", r##"{"name": "Odd scene", "scene": {"glow": "#ffffff"}}"##);
        write("notes.txt", "not a theme");
        let themes = Themes::load(Some(&dir));
        assert_eq!(themes.names()[Base::COUNT..], ["Night", "Paper"]);
        let night = themes.find("Night").unwrap();
        let dark = themes.find("dark").unwrap();
        assert!(night.is_dark());
        assert_eq!(night.tokens.accent, Color32::from_rgba_unmultiplied(0x2f, 0x7f, 0xd8, 0xff));
        assert_eq!((night.tokens.text, night.scene.metal), (dark.tokens.text, dark.scene.metal), "the rest is the base's");
        assert_eq!(night.scene.port, Rgb(0xff, 0x9a, 0x3c));
        assert_eq!(night.palette.ground, Rgb(0x20, 0x24, 0x2c));
        assert!(!themes.find("Paper").unwrap().is_dark());
        let problems = themes.problems.join("\n");
        assert_eq!(themes.problems.len(), 5, "{problems}");
        for want in ["c-broken.json left out: not JSON", "no token `acent`", "`text` on `panel` has a contrast", "already loaded", "no colour `glow`"]
        {
            assert!(problems.contains(want), "{want}: {problems}");
        }
        // Missing names fall back to the light theme; a missing folder gives the built-ins.
        assert_eq!(themes.get("gone").name, "light");
        assert_eq!(Themes::load(Some(&dir.join("missing"))).names(), Base::IDS);
        // Too many files: the first ones are read, the rest reported.
        for i in 0..MAX_THEMES + 3 {
            write(&format!("z{i:03}.json"), &format!(r#"{{"name": "z{i}"}}"#));
        }
        let many = Themes::load(Some(&dir));
        assert_eq!(many.list.len(), Base::COUNT + 2 + (MAX_THEMES - 7));
        assert!(many.problems.iter().any(|p| p.contains("the first 32 are read")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The example theme shipped in docs/themes loads without a problem.
    #[test]
    fn the_documented_example_theme_loads() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/themes");
        let themes = Themes::load(Some(&dir));
        assert_eq!(themes.problems, Vec::<String>::new());
        let night = themes.find("Night shift").unwrap();
        assert!(night.is_dark() && night.tokens.accent != themes.get("dark").tokens.accent);
    }

    #[test]
    fn the_current_theme_is_the_threads_own() {
        let themes = Themes::load(None);
        set(themes.get("dark"));
        assert!(is_dark() && Tokens::get() == themes.get("dark").tokens && key().name == "dark");
        assert_eq!(palette(), themes.get("dark").palette);
        std::thread::spawn(|| assert!(!is_dark())).join().unwrap();
        set(themes.get("light"));
        assert!(!is_dark());
        // Each copy of a theme has its own key.
        assert_ne!(Themes::load(None).get("light").key(), themes.get("light").key());
    }
}

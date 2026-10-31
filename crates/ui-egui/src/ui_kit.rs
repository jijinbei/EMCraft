//! The few tokenised controls EMCraft's cards share, after SolidCraft's UI kit
//! (`crates/ui-egui/src/ui_kit/`: the card header, aligned label rows, footer actions, the error
//! band and the progress bar), much reduced. Each publishes a scenario handle where a test or an
//! agent needs to reach it.

use egui::{Align2, Color32, FontId, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

use crate::handle::{Fixed, Handle, Part};
use crate::lang::{Shown, tr};
use crate::theme::Tokens;
use crate::{UiExt, scenario};

/// A card's title band: glyph, title, optional dock toggle and a close button. Returns
/// (close clicked, dock clicked).
pub fn header(ui: &mut Ui, glyph: crate::glyphs::Glyph, title: impl Shown, docked: Option<bool>, accent: Color32) -> (bool, bool) {
    let t = &Tokens::get();
    let title = title.shown();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.form_header_height), Sense::hover());
    let icon = Rect::from_center_size(pos2(rect.left() + t.space_12 + 10.0, rect.center().y), vec2(20.0, 20.0));
    crate::glyphs::paint(ui.painter(), icon, glyph, t.icon, accent);
    let close = Rect::from_center_size(pos2(rect.right() - t.space_8 - 13.0, rect.center().y), vec2(26.0, 26.0));
    let close_r = icon_button(ui, close, Mark::Close, "Close", Fixed::DialogClose);
    let mut right = close.left();
    let mut dock_clicked = false;
    if let Some(docked) = docked {
        let dock = Rect::from_center_size(pos2(close.left() - t.space_4 - 13.0, rect.center().y), vec2(26.0, 26.0));
        let r = icon_button(ui, dock, if docked { Mark::Undock } else { Mark::Dock }, if docked { "Float" } else { "Dock" }, Fixed::DialogDock);
        dock_clicked = r.clicked();
        right = dock.left();
    }
    let title_rect = Rect::from_min_max(pos2(icon.right() + t.space_8, rect.top()), pos2(right, rect.bottom()));
    ui.painter().with_clip_rect(title_rect).text(
        pos2(title_rect.left(), title_rect.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(t.font_heading),
        t.form_ink,
    );
    let tr = ui.interact(title_rect, ui.id().with("title"), Sense::hover());
    tr.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, title));
    ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, t.dialog_rule));
    (close_r.clicked(), dock_clicked)
}

/// The mark an icon button draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Close,
    Dock,
    Undock,
}

/// A small square button with a drawn mark, published as `handle`.
pub fn icon_button(ui: &mut Ui, r: Rect, mark: Mark, label: &str, handle: Fixed) -> Response {
    let t = &Tokens::get();
    let label = tr(label);
    let resp = ui.hit(r, handle.into(), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    if resp.hovered() {
        ui.painter().rect_filled(r, t.radius_sm, t.hover);
    }
    let s = Stroke::new(1.4, t.form_dim);
    let c = r.center();
    let d = 4.5;
    match mark {
        Mark::Close => cross(ui.painter(), c, d, s),
        Mark::Dock => {
            ui.painter().rect_stroke(Rect::from_center_size(c, vec2(12.0, 10.0)), 1.0, s, egui::StrokeKind::Middle);
            ui.painter().line_segment([c + vec2(2.0, -5.0), c + vec2(2.0, 5.0)], s);
        }
        Mark::Undock => {
            ui.painter().rect_stroke(Rect::from_center_size(c + vec2(-1.5, 1.5), vec2(9.0, 8.0)), 1.0, s, egui::StrokeKind::Middle);
            ui.painter().rect_stroke(Rect::from_center_size(c + vec2(2.0, -2.0), vec2(9.0, 8.0)), 1.0, s, egui::StrokeKind::Middle);
        }
    }
    resp.on_hover_text(label)
}

/// A cross of half-side `d` about `c`.
fn cross(p: &egui::Painter, c: egui::Pos2, d: f32, s: Stroke) {
    p.line_segment([c + vec2(-d, -d), c + vec2(d, d)], s);
    p.line_segment([c + vec2(d, -d), c + vec2(-d, d)], s);
}

/// A small close mark in `r` (a control the caller senses), on the hover tint when `hovered`.
pub fn close_mark(ui: &Ui, r: Rect, hovered: bool) {
    let t = &Tokens::get();
    if hovered {
        ui.painter().rect_filled(r, t.radius_sm, t.hover);
    }
    cross(ui.painter(), r.center(), 3.5, Stroke::new(1.3, t.form_dim));
}

/// A form row: the label in the label column, the control after it. The label's text is the
/// row's accessible name.
pub fn row<R>(ui: &mut Ui, label: impl Shown, required: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = &Tokens::get();
    let label = label.shown();
    ui.horizontal(|ui| {
        ui.set_min_height(t.control_height);
        let (r, _) = ui.allocate_exact_size(vec2(t.form_label_width, t.control_height), Sense::hover());
        let text = if required { format!("{label} *") } else { label.to_owned() };
        // A parameter with a symbol (`eps_r`: ε_r) shows it set as mathematics at the column's end.
        let mut name = r;
        if let Some(symbol) = crate::math::parameter_symbol(label) {
            let g = ui.painter().layout_job(crate::math::job(symbol, t.font_label, t.form_ink));
            let at = pos2(r.right() - t.space_6 - g.size().x, r.center().y - g.size().y / 2.0);
            name.max.x = at.x - t.space_4;
            ui.painter().galley(at, g, t.form_ink);
        }
        ui.painter().with_clip_rect(name).text(
            pos2(r.left(), r.center().y),
            Align2::LEFT_CENTER,
            &text,
            FontId::proportional(t.font_label),
            t.form_dim,
        );
        let lr = ui.interact(r, ui.id().with(("row_label", label)), Sense::hover());
        lr.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
        add(ui)
    })
    .inner
}

/// A caption: small dim wrapping text.
pub fn caption(ui: &mut Ui, text: impl Shown) {
    small(ui, text, Tokens::get().text_dim);
}

/// Small text in `color`.
pub fn small(ui: &mut Ui, text: impl Shown, color: Color32) -> Response {
    let t = &Tokens::get();
    ui.label(egui::RichText::new(text.shown()).size(t.font_small).color(color))
}

/// A section heading in small capitals with a hairline after it.
pub fn section(ui: &mut Ui, text: impl Shown) {
    let t = &Tokens::get();
    let text = text.shown();
    ui.add_space(t.space_4);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    let g = ui.painter().layout_no_wrap(text.to_uppercase(), FontId::proportional(t.font_small), t.text_dim);
    let w = g.size().x;
    ui.painter().galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, t.text_dim);
    ui.painter().hline((r.left() + w + t.space_6)..=r.right(), r.center().y, Stroke::new(1.0, t.dialog_rule));
}

/// A full-width error band.
pub fn error_band(ui: &mut Ui, text: impl Shown) {
    let t = &Tokens::get();
    egui::Frame::NONE.fill(t.form_error_fill).corner_radius(t.radius_sm).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let r = ui.label(egui::RichText::new(text.shown()).size(t.font_body).color(t.form_error));
        scenario::publish_rect(Fixed::DialogError, r.rect);
    });
}

/// A card's button: secondary, or `primary` in the accent colour.
pub fn styled(label: impl Shown, primary: bool) -> egui::Button<'static> {
    let t = &Tokens::get();
    let text = egui::RichText::new(label.shown()).size(t.font_button).color(if primary { t.on_accent } else { t.form_ink });
    let b = egui::Button::new(text).min_size(vec2(72.0, t.control_height)).corner_radius(t.control_radius);
    if primary { b.fill(t.accent) } else { b.fill(t.control_fill).stroke(Stroke::new(1.0, t.control_border)) }
}

/// A card's button ([`styled`]) whose click stays with its card, published as `handle`.
pub fn button(ui: &mut Ui, label: impl Shown, primary: bool, handle: Fixed) -> Response {
    ui.named(styled(label, primary), handle.into())
}

/// A determinate (`Some`) or running (`None`) progress bar.
pub fn progress(ui: &mut Ui, fraction: Option<f64>, width: f32) -> Response {
    let t = &Tokens::get();
    let (r, resp) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
    ui.painter().rect_filled(r, 3, t.dialog_rule);
    match fraction {
        Some(f) => {
            let w = r.width() * f.clamp(0.0, 1.0) as f32;
            ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(w, r.height())), 3, t.accent);
        }
        None => {
            // A running segment sliding along the bar.
            let time = ui.input(|i| i.time);
            let phase = (time * 0.8).fract() as f32;
            let seg = r.width() * 0.3;
            let x0 = r.left() + (r.width() + seg) * phase - seg;
            let s = Rect::from_min_max(pos2(x0.max(r.left()), r.top()), pos2((x0 + seg).min(r.right()), r.bottom()));
            if s.is_positive() {
                ui.painter().rect_filled(s, 3, t.accent);
            }
            ui.ctx().request_repaint();
        }
    }
    scenario::publish_rect(Fixed::Progress, r);
    resp
}

/// A periodic animation played like a video: playing or paused, at a speed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Player {
    pub playing: bool,
    /// Periods per [`Player::PERIOD`] seconds.
    pub speed: f64,
}

impl Default for Player {
    /// Plays from the start, as a video opens.
    fn default() -> Player {
        Player { playing: true, speed: 1.0 }
    }
}

impl Player {
    /// Seconds a period takes at speed 1.
    pub const PERIOD: f64 = 3.0;
    pub const SPEEDS: [f64; 4] = [0.25, 0.5, 1.0, 2.0];

    /// The phase (radians) `dt` seconds later.
    pub fn advance(self, phase: f64, dt: f64) -> f64 {
        (phase + dt * self.speed * std::f64::consts::TAU / Player::PERIOD).rem_euclid(std::f64::consts::TAU)
    }
}

/// A video player's controls for a phase (radians): play or pause, a timeline over one period
/// that seeks (and pauses) when dragged, and the speed. Handles `{handle}:play`, `{handle}` (the
/// timeline) and `{handle}:speed`.
pub fn player(ui: &mut Ui, player: &mut Player, phase: &mut f64, handle: Fixed) {
    // One piece in a row that wraps.
    wrap_before(ui, PLAYER_WIDTH);
    ui.horizontal(|ui| player_parts(ui, player, phase, handle));
}

/// The player's width: play, the timeline, the phase and the speed.
const PLAYER_WIDTH: f32 = 380.0;

/// In a row that wraps, a new row unless `width` more fits in this one (a group of widgets
/// that a row would otherwise run past the card's edge with).
pub fn wrap_before(ui: &mut Ui, width: f32) {
    if ui.available_size_before_wrap().x < width && ui.cursor().min.x > ui.max_rect().min.x + 1.0 {
        ui.end_row();
    }
}

fn player_parts(ui: &mut Ui, player: &mut Player, phase: &mut f64, handle: Fixed) {
    use std::f64::consts::TAU;
    let t = &Tokens::get();
    // Play / pause.
    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 22.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, t.radius_sm, t.hover);
    }
    crate::glyphs::paint(
        ui.painter(),
        r.shrink(3.0),
        if player.playing { crate::glyphs::Glyph::Pause } else { crate::glyphs::Glyph::Play },
        t.icon,
        t.accent,
    );
    scenario::publish_target(Handle::Part(handle, Part::Play), r);
    let tip = tr(if player.playing { "Pause" } else { "Play" });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    if resp.on_hover_text(tip).clicked() {
        player.playing = !player.playing;
    }
    // The timeline: one period, the played part in the accent.
    let (r, resp) = ui.allocate_exact_size(vec2(200.0, 22.0), Sense::click_and_drag());
    let track = Rect::from_center_size(r.center(), vec2(r.width() - 10.0, 4.0));
    let p = ui.painter();
    p.rect_filled(track, 2, t.border);
    let f = (phase.rem_euclid(TAU) / TAU) as f32;
    p.rect_filled(Rect::from_min_size(track.min, vec2(track.width() * f, track.height())), 2, t.accent);
    let knob = pos2(track.left() + track.width() * f, track.center().y);
    p.circle_filled(knob, if resp.hovered() || resp.dragged() { 6.0 } else { 5.0 }, t.accent);
    scenario::publish_target(handle, r);
    if let Some(q) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        *phase = f64::from(((q.x - track.left()) / track.width()).clamp(0.0, 1.0)) * TAU;
        player.playing = false;
    }
    ui.label(egui::RichText::new(format!("ωt {:>3.0}°", phase.rem_euclid(TAU).to_degrees())).size(t.font_small).color(t.text_dim).monospace());
    // Speed.
    let r = egui::ComboBox::from_id_salt((handle.id(), "speed")).width(52.0).selected_text(format!("{}×", player.speed)).show_ui(ui, |ui| {
        for s in Player::SPEEDS {
            ui.selectable_value(&mut player.speed, s, format!("{s}×"));
        }
    });
    scenario::publish_target(Handle::Part(handle, Part::Speed), r.response.rect);
}

/// A slider whose track shows on the panels' colours: the track in the controls' border colour,
/// the part up to the handle in the accent.
pub fn slider(ui: &mut Ui, value: &mut f64, range: std::ops::RangeInclusive<f64>, text: impl Into<egui::WidgetText>) -> Response {
    let t = &Tokens::get();
    ui.scope(|ui| {
        let v = ui.visuals_mut();
        v.widgets.inactive.bg_fill = t.border;
        v.selection.bg_fill = t.accent;
        v.slider_trailing_fill = true;
        ui.spacing_mut().slider_width = 200.0;
        ui.add(egui::Slider::new(value, range).text(text))
    })
    .inner
}

//! Labels set as mathematics: TeX-like text (`$\varepsilon_r$ = 2.2`, `$\tan\delta_m$`,
//! `$S_{21}$`) laid out as an egui [`LayoutJob`] with real subscripts and superscripts — smaller
//! and lowered or raised — and Greek letters, so the window's labels read like the typeset
//! formulas of the documentation instead of `εr` on the line.
//!
//! The TeX is read by [`emcraft_render::math::parse`] (the subset the pictures' labels use:
//! scripts, fractions, roots, Greek and the usual symbols; *The TeXbook*, ch. 16–18); this
//! module only sets the nodes in egui's text. Text the window writes in plain Unicode (`εr 2.1,
//! μr 1`, `S11`, `tan δm`, `f_c`, `5.8e7`) is turned into that TeX by [`texify`], so a label
//! keeps its plain text for handles and tests and shows the rich form.
//!
//! ```
//! use emcraft_ui_egui::math;
//! // Plain text in, TeX out: the symbols the window writes become scripts.
//! assert_eq!(math::texify("vacuum: εr 1, μr 1"), "vacuum: $\\varepsilon_{r}$ 1, $\\mu_{r}$ 1");
//! assert_eq!(math::texify("|S21| at f_c"), "|$S_{21}$| at $f_{c}$");
//! // Identifiers, prices and unknown words stay as they are.
//! assert_eq!(math::texify("`loss_tangent` costs $5"), "`loss_tangent` costs \\$5");
//! // Any string lays out: hostile input makes a job, never a panic.
//! let job = math::job("$x^{{{{", 13.0, egui::Color32::WHITE);
//! assert!(!job.sections.is_empty());
//! ```

use std::borrow::Cow;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Color32, FontId, Response};
use emcraft_render::math::Node;

/// A script's size against its base's (TeX's script style is 70 %).
const SCRIPT: f32 = 0.72;
/// The smallest size a nested script takes, against the label's.
const SMALLEST: f32 = 0.5;

/// Where a run of text sits: on the line, lowered or raised.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    Line,
    Sub,
    Sup,
}

/// `tex` (plain text with `$…$` math) set at `size` points in `color`.
pub fn job(tex: &str, size: f32, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    set(&mut job, &emcraft_render::math::parse(tex), size, size, Place::Line, color);
    if job.sections.is_empty() {
        job.append("", 0.0, format(size, size, Place::Line, color));
    }
    job
}

/// Plain text (as the window writes it) set as mathematics: [`job`] of [`texify`].
pub fn rich(plain: &str, size: f32, color: Color32) -> LayoutJob {
    job(&texify(plain), size, color)
}

/// A label of plain text set as mathematics, its plain text the accessible name.
pub fn label(ui: &mut egui::Ui, plain: &str, size: f32, color: Color32) -> Response {
    let r = ui.label(rich(plain, size, color));
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, plain));
    r
}

/// Plain text set as mathematics and painted with `align` at `pos`, clipped to `clip`; the
/// rectangle it took.
pub fn paint(painter: &egui::Painter, pos: egui::Pos2, align: egui::Align2, plain: &str, size: f32, color: Color32) -> egui::Rect {
    let galley = painter.layout_job(rich(plain, size, color));
    let rect = align.anchor_size(pos, galley.size());
    painter.galley(rect.min, galley, color);
    rect
}

/// `plain` on hover, set as mathematics when it holds any (else egui's plain tooltip).
pub fn tip(resp: Response, plain: &str) -> Response {
    match texify(plain) {
        Cow::Borrowed(_) => resp.on_hover_text(plain),
        Cow::Owned(tex) => resp.on_hover_ui(|ui| {
            let size = ui.style().text_styles.get(&egui::TextStyle::Body).map_or(13.0, |f| f.size);
            ui.label(job(&tex, size, ui.visuals().text_color()));
        }),
    }
}

fn format(size: f32, base: f32, place: Place, color: Color32) -> TextFormat {
    let mut f = TextFormat { font_id: FontId::proportional(size), color, ..TextFormat::default() };
    match place {
        Place::Line => {}
        // The top of the line: a superscript's top meets its base's.
        Place::Sup => f.valign = Align::TOP,
        // The bottom of the line with a short line box, so the glyphs drop below the base's
        // baseline by about a tenth of its size.
        Place::Sub => {
            f.valign = Align::BOTTOM;
            f.line_height = Some((size * 1.15 - base * 0.2).max(size * 0.5));
        }
    }
    f
}

/// Appends `nodes` at `size` (the label's is `base`) in `place`.
fn set(job: &mut LayoutJob, nodes: &[Node], size: f32, base: f32, place: Place, color: Color32) {
    for n in nodes {
        match n {
            Node::Text(s) => job.append(s, 0.0, format(size, base, place, color)),
            Node::Space(w) if *w > 0.1 => job.append(" ", 0.0, format(size, base, place, color)),
            Node::Space(_) => {}
            Node::Scripts { base: b, sup, sub } => {
                set(job, b, size, base, place, color);
                let small = (size * SCRIPT).max(base * SMALLEST);
                if let Some(s) = sub {
                    set(job, s, small, base, Place::Sub, color);
                }
                if let Some(s) = sup {
                    set(job, s, small, base, Place::Sup, color);
                }
            }
            Node::Frac { num, den } => {
                grouped(job, num, size, base, place, color);
                job.append("/", 0.0, format(size, base, place, color));
                grouped(job, den, size, base, place, color);
            }
            Node::Sqrt(body) => {
                job.append("√", 0.0, format(size, base, place, color));
                grouped(job, body, size, base, place, color);
            }
        }
    }
}

/// `nodes`, in parentheses unless they are one character.
fn grouped(job: &mut LayoutJob, nodes: &[Node], size: f32, base: f32, place: Place, color: Color32) {
    let single = matches!(nodes, [Node::Text(s)] if s.chars().count() == 1);
    if !single {
        job.append("(", 0.0, format(size, base, place, color));
    }
    set(job, nodes, size, base, place, color);
    if !single {
        job.append(")", 0.0, format(size, base, place, color));
    }
}

pub use emcraft_ui_core::math::{material, parameter_symbol, texify};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_are_smaller_and_lowered_or_raised() {
        let j = job("$\\varepsilon_r$ = 2 $x^2$", 14.0, Color32::WHITE);
        let texts: Vec<(&str, f32, Align)> =
            j.sections.iter().map(|s| (&j.text[s.byte_range.start.0..s.byte_range.end.0], s.format.font_id.size, s.format.valign)).collect();
        assert_eq!(texts[0], ("ε", 14.0, Align::BOTTOM));
        assert_eq!(texts[1].0, "r");
        assert!(texts[1].1 < 14.0 && j.sections[1].format.line_height.is_some(), "a subscript: smaller, its line box short");
        let sup = texts.iter().find(|t| t.0 == "2" && t.2 == Align::TOP);
        assert!(sup.is_some_and(|s| s.1 < 14.0), "{texts:?}");
    }

    #[test]
    fn hostile_text_lays_out() {
        let long = "εr_".repeat(5000);
        for s in ["", "$", "$$", "_", "S1", "1e", "1e9999", "\\$", "$\\frac{$", long.as_str(), "ε∞∞"] {
            let _ = texify(s);
            let _ = rich(s, 12.0, Color32::WHITE);
        }
    }
}

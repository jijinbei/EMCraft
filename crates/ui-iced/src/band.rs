//! The Setup step's band as a small diagram, drawn by a canvas from the shared rules'
//! [`Diagram`]: a frequency axis with its ticks, the band shaded with the solve's frequencies as
//! ticks in it, and the cut-offs near it as dashed lines (a port's first mode in the warning
//! colour: below it no power enters), labelled on two staggered rows.

use emcraft_ui_core::steps::band::Diagram;
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, LineDash, Path, Stroke};
use iced::{Color, Pixels, Point, Rectangle, Renderer, Size, Theme, alignment};

use crate::app::Message;
use crate::look::Tokens;

/// Most ticks drawn for the solve's frequencies (beyond, they fill the band).
const MAX_SOLVE_TICKS: usize = 400;

/// The diagram, the tokens it is drawn in and the frequency unit's symbol.
pub struct Band<'a> {
    pub diagram: Diagram,
    pub tokens: Tokens,
    pub unit: &'a str,
}

fn label(content: String, at: Point, ink: Color, size: f32, x: iced::widget::text::Alignment) -> canvas::Text {
    canvas::Text { content, position: at, color: ink, size: Pixels(size), align_x: x, align_y: alignment::Vertical::Top, ..canvas::Text::default() }
}

impl canvas::Program<Message> for Band<'_> {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: Cursor) -> Vec<Geometry> {
        let (t, d) = (self.tokens, &self.diagram);
        let mut frame = Frame::new(renderer, bounds.size());
        let ((lo, hi), (a, z)) = (d.span, d.band);
        let (left, right) = (4.0, bounds.width - 4.0);
        let x = |f: f64| left + (((f - lo) / (hi - lo)) as f32).clamp(0.0, 1.0) * (right - left);
        let base = 46.0;
        let line = |w: f32, ink: Color| Stroke::default().with_width(w).with_color(ink);
        let band = (x(z) - x(a)).max(1.0);
        frame.fill_rectangle(Point::new(x(a), base - 16.0), Size::new(band, 16.0), t.accent_soft);
        if d.solves.len() > MAX_SOLVE_TICKS {
            frame.fill_rectangle(Point::new(x(a), base - 13.0), Size::new(band, 10.0), Color { a: 0.6, ..t.accent });
        } else {
            for f in d.solves.iter().filter(|f| f.is_finite()) {
                frame.stroke(&Path::line(Point::new(x(*f), base - 13.0), Point::new(x(*f), base)), line(1.0, t.accent));
            }
        }
        frame.stroke(&Path::line(Point::new(left, base), Point::new(right, base)), line(1.0, t.text_dim));
        let small = t.font_small;
        for v in &d.ticks {
            frame.stroke(&Path::line(Point::new(x(*v), base), Point::new(x(*v), base + 3.0)), line(1.0, t.text_dim));
            let text = emcraft_ui_core::dialogs::short_number(*v);
            frame.fill_text(label(text, Point::new(x(*v), base + 4.0), t.text_dim, small, iced::widget::text::Alignment::Center));
        }
        frame.fill_text(label(self.unit.to_owned(), Point::new(right, base + 4.0), t.text_dim, small, iced::widget::text::Alignment::Right));
        for (i, c) in d.cuts.iter().enumerate() {
            let ink = if c.first { t.warning } else { t.text_dim };
            let top = 1.0 + 12.0 * (i % 2) as f32;
            let xf = x(c.at);
            let dashed = Stroke { line_dash: LineDash { segments: &[3.0, 2.0], offset: 0 }, ..line(1.2, ink) };
            frame.stroke(&Path::line(Point::new(xf, top + 2.0), Point::new(xf, base)), dashed);
            let (align, at) = if xf > bounds.width / 2.0 {
                (iced::widget::text::Alignment::Right, xf - 3.0)
            } else {
                (iced::widget::text::Alignment::Left, xf + 3.0)
            };
            frame.fill_text(label(c.label.clone(), Point::new(at, top), ink, small - 1.0, align));
        }
        vec![frame.into_geometry()]
    }
}

//! The command glyphs in the iced window: the shared marks ([`emcraft_ui_core::glyph_marks`])
//! drawn by a canvas at the size it is given, in the theme's ink and accent.

use emcraft_ui_core::glyph::Glyph;
use emcraft_ui_core::glyph_marks::{self, Color32, Ink, Mark, Pos2, SIZE, Stroke};
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, LineDash, Path};
use iced::{Color, Element, Pixels, Point, Rectangle, Renderer, Size, Theme, Vector, Widget, alignment};

use crate::app::Message;

/// A glyph in two colours.
pub struct Icon {
    pub glyph: Glyph,
    pub ink: Color,
    pub accent: Color,
}

/// `glyph` `side` points square, in `ink` and `accent`.
pub fn view<'a>(glyph: Glyph, side: f32, ink: Color, accent: Color) -> Element<'a, Message> {
    iced::widget::canvas(Icon { glyph, ink, accent }).width(side).height(side).boxed()
}

impl Icon {
    fn colour(&self, c: Color32) -> Color {
        let base = match c.ink {
            Ink::Ink => self.ink,
            Ink::Accent => self.accent,
        };
        Color { a: base.a * c.alpha.clamp(0.0, 1.0), ..base }
    }

    fn stroke(&self, s: Stroke, scale: f32) -> canvas::Stroke<'static> {
        canvas::Stroke::default().with_width(s.width * scale).with_color(self.colour(s.color))
    }
}

fn polyline(points: &[Pos2], closed: bool, at: impl Fn(Pos2) -> Point) -> Path {
    Path::new(|b| {
        if let Some((first, rest)) = points.split_first() {
            b.move_to(at(*first));
            rest.iter().for_each(|p| b.line_to(at(*p)));
            if closed {
                b.close();
            }
        }
    })
}

impl canvas::Program<Message> for Icon {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let k = bounds.width.min(bounds.height) / SIZE;
        let off = Vector::new((bounds.width - SIZE * k) / 2.0, (bounds.height - SIZE * k) / 2.0);
        let at = |p: Pos2| Point::new(p.x * k + off.x, p.y * k + off.y);
        for m in glyph_marks::marks(self.glyph) {
            match m {
                Mark::Line { points, closed, stroke } => frame.stroke(&polyline(&points, closed, at), self.stroke(stroke, k)),
                Mark::Dashed { points, stroke, dash, gap } => {
                    let segments = [dash * k, gap * k];
                    let s = canvas::Stroke { line_dash: LineDash { segments: &segments, offset: 0 }, ..self.stroke(stroke, k) };
                    frame.stroke(&polyline(&points, false, at), s);
                }
                Mark::Fill { points, fill, stroke } => {
                    let path = polyline(&points, true, at);
                    frame.fill(&path, self.colour(fill));
                    if stroke.width > 0.0 {
                        frame.stroke(&path, self.stroke(stroke, k));
                    }
                }
                Mark::Circle { centre, radius, fill, stroke } => {
                    let path = Path::circle(at(centre), radius * k);
                    if let Some(f) = fill {
                        frame.fill(&path, self.colour(f));
                    }
                    if let Some(s) = stroke {
                        frame.stroke(&path, self.stroke(s, k));
                    }
                }
                Mark::Ellipse { centre, radii, stroke } => {
                    let c = at(centre);
                    let path = Path::new(|b| {
                        (0..=32).map(|i| std::f32::consts::TAU * i as f32 / 32.0).enumerate().for_each(|(i, a)| {
                            let p = Point::new(c.x + radii.x * k * a.cos(), c.y + radii.y * k * a.sin());
                            if i == 0 { b.move_to(p) } else { b.line_to(p) }
                        });
                    });
                    frame.stroke(&path, self.stroke(stroke, k));
                }
                Mark::Rect { rect, rounding, fill, stroke } => {
                    let (a, b) = (at(rect.min), at(rect.max));
                    let path = Path::rounded_rectangle(a, Size::new(b.x - a.x, b.y - a.y), (rounding * k).into());
                    if let Some(f) = fill {
                        frame.fill(&path, self.colour(f));
                    }
                    if let Some(s) = stroke {
                        frame.stroke(&path, self.stroke(s, k));
                    }
                }
                Mark::Text { at: p, text, size, color } => frame.fill_text(canvas::Text {
                    content: text,
                    position: at(p),
                    color: self.colour(color),
                    size: Pixels(size * k),
                    align_x: iced::widget::text::Alignment::Center,
                    align_y: alignment::Vertical::Center,
                    ..canvas::Text::default()
                }),
            }
        }
        vec![frame.into_geometry()]
    }
}

/// An eye: open when what it names is shown, struck through when hidden.
pub fn eye<'a>(visible: bool, ink: Color) -> Element<'a, Message> {
    iced::widget::canvas(Eye { visible, ink }).width(22).height(18).boxed()
}

struct Eye {
    visible: bool,
    ink: Color,
}

impl canvas::Program<Message> for Eye {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let c = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let at = |x: f32, y: f32| Point::new(c.x + x, c.y + y);
        let outline = Path::new(|b| {
            b.move_to(at(-8.0, 0.0));
            [(-4.0, -4.0), (0.0, -5.0), (4.0, -4.0), (8.0, 0.0), (4.0, 4.0), (0.0, 5.0), (-4.0, 4.0)]
                .into_iter()
                .for_each(|(x, y)| b.line_to(at(x, y)));
            b.close();
        });
        let line = |w: f32| canvas::Stroke::default().with_width(w).with_color(self.ink);
        frame.stroke(&outline, line(1.2));
        frame.fill(&Path::circle(c, 2.0), self.ink);
        if !self.visible {
            frame.stroke(&Path::line(at(-7.0, 6.0), at(7.0, -6.0)), line(1.4));
        }
        vec![frame.into_geometry()]
    }
}

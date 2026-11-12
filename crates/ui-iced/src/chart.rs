//! A chart drawn as vectors by the window's GPU: the engine's chart laid out as shapes at the
//! widget's size ([`emcraft_render::chart::drawing`], the same shapes the CPU draws pictures
//! from) and each shape given to iced's canvas, which tessellates and draws it on the GPU — sharp
//! at any size and drawn again only when the chart or the size changes.

use emcraft_render::Rgb;
use emcraft_render::palette::Palette;
use emcraft_render::shapes::{Align, Shape};
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, LineCap, Path, Stroke};
use iced::widget::text::{Alignment, LineHeight};
use iced::{Color, Pixels, Point, Rectangle, Renderer, Size, Theme, Vector, alignment};

use crate::app::Message;

/// A label's em for its size (as [`emcraft_render::text`] sets DejaVu Sans: 9 pixels a scale).
const EM_PER_SCALE: f32 = 9.0;

/// The face the labels are set in: the pictures' own (loaded with the window,
/// [`emcraft_render::text::face_bytes`]), so they take the room the layout gave them; what it
/// lacks comes from the window's fonts.
pub const FACE: iced::Font = iced::Font::new("DejaVu Sans");

/// The chart (kept, or made for the frame) and the palette it is drawn in.
pub struct Plot<'a> {
    pub chart: std::borrow::Cow<'a, emcraft_render::chart::Chart>,
    pub palette: &'a Palette,
}

fn colour(c: Rgb, alpha: f32) -> Color {
    Color { a: alpha, ..Color::from_rgb8(c.0, c.1, c.2) }
}

fn point(p: [f64; 2]) -> Point {
    Point::new(p[0] as f32, p[1] as f32)
}

fn align(a: Align) -> Alignment {
    match a {
        Align::Left => Alignment::Left,
        Align::Center => Alignment::Center,
        Align::Right => Alignment::Right,
    }
}

/// Draws `shape` on `frame`.
fn put(frame: &mut Frame, shape: &Shape) {
    match shape {
        Shape::Line { a, b, width, color } => {
            let stroke = Stroke { width: *width as f32, line_cap: LineCap::Butt, ..Stroke::default() }.with_color(colour(*color, 1.0));
            frame.stroke(&Path::line(point(*a), point(*b)), stroke);
        }
        Shape::Triangle { p, color } => frame.fill(&polygon(p), colour(*color, 1.0)),
        Shape::Rect { at, size, color } => frame.fill_rectangle(point(*at), Size::new(size[0] as f32, size[1] as f32), colour(*color, 1.0)),
        Shape::Band { edge, clip, color, alpha } => {
            let region = Rectangle::new(point([clip[0], clip[1]]), Size::new(clip[2] as f32, clip[3] as f32));
            frame.with_clip(region, |f| {
                // The clip moves the origin to its corner.
                let shifted: Vec<[f64; 2]> = edge.iter().map(|p| [p[0] - clip[0], p[1] - clip[1]]).collect();
                f.fill(&polygon(&shifted), colour(*color, *alpha as f32));
            });
        }
        Shape::Text { at, text, scale, color, align: a, up } => {
            // Its TeX ($S_{21}$) in Unicode (S₂₁).
            let content = emcraft_render::math::to_unicode(text);
            let label = |position: Point| canvas::Text {
                content: content.clone(),
                font: FACE,
                position,
                color: colour(*color, 1.0),
                size: Pixels(EM_PER_SCALE * (*scale).clamp(1, 16) as f32),
                line_height: LineHeight::Relative(1.0),
                align_x: align(*a),
                align_y: alignment::Vertical::Top,
                ..canvas::Text::default()
            };
            if *up {
                frame.with_save(|f| {
                    f.translate(Vector::new(at[0] as f32, at[1] as f32));
                    f.rotate(-std::f32::consts::FRAC_PI_2);
                    f.fill_text(label(Point::ORIGIN));
                });
            } else {
                frame.fill_text(label(point(*at)));
            }
        }
    }
}

/// A closed polygon through `pts`.
fn polygon(pts: &[[f64; 2]]) -> Path {
    Path::new(|b| {
        if let Some((first, rest)) = pts.split_first() {
            b.move_to(point(*first));
            rest.iter().for_each(|p| b.line_to(point(*p)));
            b.close();
        }
    })
}

/// `chart` drawn on `frame` at `size` in `palette`: its ground and its shapes.
pub fn paint(frame: &mut Frame, chart: &emcraft_render::chart::Chart, size: Size, palette: &Palette) {
    let (w, h) = (size.width.max(1.0) as usize, size.height.max(1.0) as usize);
    let d = emcraft_render::chart::drawing(chart, w, h, palette);
    frame.fill_rectangle(Point::ORIGIN, size, colour(d.ground, 1.0));
    d.shapes.iter().for_each(|s| put(frame, s));
}

impl canvas::Program<Message> for Plot<'_> {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        paint(&mut frame, &self.chart, bounds.size(), self.palette);
        vec![frame.into_geometry()]
    }
}

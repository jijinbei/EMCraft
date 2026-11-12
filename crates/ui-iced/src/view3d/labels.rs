//! The scene's labels over the 3D view: each where the shared rules place it
//! ([`scene3d::placed`]), on a ground of the panel's colour, dimmed when the face it names looks
//! away, with a leader line to the point it stands off from; and the selection box while it is
//! dragged (by window a solid outline, by crossing a dashed one in another colour). A canvas laid
//! over the shader widget; it lets every event through to the view under it.

use emcraft_render::view3d::Camera;
use emcraft_ui_core::scene3d::{self, Label};
use emcraft_ui_core::selection::BoxSel;
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{Color, Pixels, Rectangle, Renderer, Size, Theme, alignment};

use crate::app::Message;
use crate::geom::{point, size as size_of, vector};
use crate::look::{self, Tokens};

/// A character's width as a share of the font's size, for the ground under a label (the canvas
/// does not measure text).
const CHAR_W: f32 = 0.56;

/// The labels seen through `camera` and the selection box, in the theme's tokens.
pub struct Labels<'a> {
    pub labels: &'a [Label],
    pub camera: Camera,
    pub boxsel: Option<BoxSel>,
    pub tokens: Tokens,
}

/// The selection box `b` on `frame`.
fn boxed(frame: &mut Frame, t: Tokens, b: BoxSel) {
    let ink = if b.crossing { t.success } else { t.accent };
    let path = Path::rectangle(point(b.rect.min), size_of_rect(b));
    frame.fill(&path, Color { a: 0.12, ..ink });
    let dash = if b.crossing { &[6.0, 4.0][..] } else { &[][..] };
    let stroke = Stroke { line_dash: canvas::LineDash { segments: dash, offset: 0 }, ..Stroke::default().with_width(1.2).with_color(ink) };
    frame.stroke(&path, stroke);
}

fn size_of_rect(b: BoxSel) -> Size {
    Size::new(b.rect.width(), b.rect.height())
}

impl canvas::Program<Message> for Labels<'_> {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: Cursor) -> Vec<Geometry> {
        let t = self.tokens;
        let mut frame = Frame::new(renderer, bounds.size());
        let size = t.font_small;
        for l in scene3d::placed(self.labels, &self.camera, size_of(bounds.size())) {
            let ink = if l.away { t.text_dim } else { look::rgb(l.label.color) };
            let at = point(l.at);
            if let Some(a) = l.anchor {
                frame.stroke(&Path::line(at, point(a)), Stroke::default().with_width(1.0).with_color(ink));
                frame.fill(&Path::circle(point(a), 2.5), ink);
            }
            if !l.away {
                let ground = Size::new(l.label.text.chars().count() as f32 * size * CHAR_W + 8.0, size + 6.0);
                let corner = at - vector(emcraft_ui_core::geom::Vector::new(ground.width / 2.0, ground.height / 2.0));
                frame.fill(&Path::rounded_rectangle(corner, ground, 3.0.into()), Color { a: 0.85, ..t.panel });
            }
            frame.fill_text(canvas::Text {
                content: l.label.text.clone(),
                position: at,
                color: ink,
                size: Pixels(size),
                align_x: iced::widget::text::Alignment::Center,
                align_y: alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }
        if let Some(b) = self.boxsel {
            boxed(&mut frame, t, b);
        }
        vec![frame.into_geometry()]
    }
}

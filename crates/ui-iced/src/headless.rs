//! The window drawn without a window (`ui.render`): its view laid out and drawn by iced's CPU
//! renderer (tiny-skia), no display or GPU needed; the 3D view, which the window draws with the
//! GPU in iced's shader widget, drawn offscreen on the process's own device into a picture laid
//! in its place (where there is no adapter at all, it says so).

use iced_runtime::core::renderer::{Headless, Style};
use iced_runtime::user_interface::{Cache, UserInterface};

use crate::app::{App, Message};

/// The largest side drawn (points), and the scale pictures are drawn at.
const MAX_SIDE: f32 = 8192.0;
pub const SCALE: f32 = 1.0;

/// The window as it is now, `size` points wide and high, drawn on the CPU: its pixels (RGBA rows)
/// and their size.
pub fn render(app: &mut App, size: (f32, f32)) -> Result<(u32, u32, Vec<u8>), String> {
    let (w, h) = size;
    if !(w.is_finite() && h.is_finite() && (1.0..=MAX_SIDE).contains(&w) && (1.0..=MAX_SIDE).contains(&h)) {
        return Err(format!("a picture's sides are 1 to {MAX_SIDE} points"));
    }
    let settings = iced_runtime::core::renderer::Settings {
        font: iced::Font::default(),
        text_size: iced::Pixels(16.0),
        line_height: iced::widget::text::LineHeight::default(),
        metrics_hinting: false,
    };
    let mut renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(settings, Some("tiny-skia"))).ok_or("no CPU renderer")?;
    // The 3D view drawn into a picture while the view is built.
    app.headless = true;
    let theme = app.theme();
    // Laid out for the picture's size (the columns' widths follow the window's), then as it was.
    let window = std::mem::replace(&mut app.window, size);
    let element = app.view();
    let mut ui: UserInterface<'_, Message, iced::Theme, iced::Renderer> =
        UserInterface::build(element, iced::Size::new(w, h), Cache::default(), &mut renderer);
    let mut bus = iced_runtime::core::shell::Bus::new();
    let redraw = iced::Event::Window(iced::window::Event::RedrawRequested(std::time::Instant::now()));
    let _ = ui.update(
        &iced_runtime::core::window::Headless,
        &iced_runtime::core::shell::Waker::noop(),
        &[redraw],
        iced::mouse::Cursor::Unavailable,
        &mut renderer,
        &mut bus,
    );
    let base = iced::theme::Base::base(&theme);
    ui.draw(&mut renderer, &theme, &Style { text_color: base.text_color }, iced::mouse::Cursor::Unavailable);
    // Drawn while the interface lives: its texts are drawn from the paragraphs it holds.
    let (pw, ph) = ((w * SCALE).round() as u32, (h * SCALE).round() as u32);
    let rgba = renderer.screenshot(iced::Size::new(pw, ph), SCALE, base.background_color);
    drop(ui);
    app.headless = false;
    app.window = window;
    Ok((pw, ph, rgba))
}

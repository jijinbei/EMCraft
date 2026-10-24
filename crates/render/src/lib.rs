//! EMCraft pictures without a GPU: a 2D canvas with anti-aliased lines, arrows and a bitmap
//! font ([`canvas`], [`font`]), colour maps ([`colormap`]), field plots on planes — a colour
//! map with vector arrows over it ([`plot`]) — and line charts ([`chart`]), written as PNG. The
//! desktop app shows the same pictures, so headless and on-screen views agree.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod axes;
pub mod canvas;
pub mod chart;
pub mod colormap;
pub mod font;
pub mod plot;

pub use axes::Axis;
pub use canvas::{Canvas, Rgb};

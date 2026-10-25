//! EMCraft pictures without a GPU: a 2D canvas with anti-aliased lines, arrows and a bitmap
//! font ([`canvas`], [`font`]), colour maps ([`colormap`]), field plots on planes — a colour
//! map with vector arrows over it ([`plot`]) —, line charts ([`chart`]) and 3D views of triangles and
//! lines with a depth buffer ([`view3d`]), written as PNG. The
//! desktop app shows the same pictures, so headless and on-screen views agree.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod axes;
pub mod canvas;
pub mod chart;
pub mod colormap;
pub mod font;
pub mod plot;
pub mod view3d;

pub use axes::Axis;
pub use canvas::{Canvas, Rgb};

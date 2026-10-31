//! EMCraft pictures without a GPU: a 2D canvas with anti-aliased lines, arrows and text in
//! DejaVu Sans ([`canvas`], [`text`]; the bitmap [`font`] as a fallback), colour maps ([`colormap`]), the themes' colours of pictures ([`palette`]), field plots on planes — a colour
//! map with vector arrows over it ([`plot`]) —, line charts ([`chart`]), written as PNG; and the 3D
//! view's scene, camera, sections and picking ([`view3d`]), which the desktop app's GPU draws. The
//! desktop app shows the same 2D pictures, so headless and on-screen views agree.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod axes;
pub mod canvas;
pub mod chart;
pub mod colormap;
pub mod font;
pub mod icon;
pub mod math;
pub mod palette;
pub mod plot;
pub mod text;
pub mod view3d;

pub use axes::Axis;
pub use canvas::{Canvas, Rgb, apng};
pub use palette::{Palette, Pen};

//! The colours pictures are drawn in: a [`Palette`] per theme (the ground, the inks, the grid,
//! the series' colours) and the [`Pen`] a chart's series or a plot's arrows are drawn with — a
//! colour by its role, resolved against the palette when the picture is drawn, so one chart
//! draws light or dark. The colour maps of fields ([`crate::colormap`]) are scientific maps and
//! stay the same in every theme; what is drawn over them (arrows, their halo, section lines,
//! metal) is chosen to read on them.
//!
//! The palettes themselves are data: the built-in themes' files (`crates/engine/themes/`,
//! read by `emcraft_engine::theme`) and the user's own; [`Palette::fallback`] is the last resort.

use crate::Rgb;

/// What a series or a set of arrows is drawn with: a colour by its role in the palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pen {
    /// The palette's series colour `i`, cycled.
    Series(u8),
    /// Series colour `i` mixed toward the ink: solved points over a reference curve drawn in
    /// the same series' colour.
    Strong(u8),
    /// Series colour `i` mixed toward the ground: a reference drawn behind the solved points.
    Faded(u8),
    /// From series colour 0 (at 0) to series colour 1 (at 1): a quantity along a sweep.
    Ramp(f64),
    /// Grey: a guide (the light line, a closed form behind the solved points).
    Muted,
    Ink,
    /// The electric field's arrows.
    EArrows,
    /// The magnetic field's arrows.
    HArrows,
    /// A colour of its own, the same in every theme (one a person picked for a curve).
    Custom(Rgb),
}

/// The number of series colours.
pub const SERIES: usize = 6;

/// How far [`Pen::Strong`] and [`Pen::Faded`] move a series colour toward the ink or the ground.
const SHADE: f64 = 0.45;

/// A picture's colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// The picture's background.
    pub ground: Rgb,
    /// Text, axes, frames.
    pub ink: Rgb,
    /// Subtitles and the labels of reference lines.
    pub ink_dim: Rgb,
    /// A chart's grid.
    pub grid: Rgb,
    /// A chart's dashed reference lines (cut-off frequencies).
    pub mark: Rgb,
    /// Section boundaries over a field plot.
    pub line: Rgb,
    /// Points of a field plot outside the domain (conductors).
    pub metal: Rgb,
    /// The rim round arrows, so they read on any colour.
    pub halo: Rgb,
    /// [`Pen::Muted`].
    pub muted: Rgb,
    /// [`Pen::EArrows`].
    pub e_arrows: Rgb,
    /// [`Pen::HArrows`].
    pub h_arrows: Rgb,
    /// [`Pen::Series`]: blue, red, green, purple, orange, grey.
    pub series: [Rgb; SERIES],
}

/// Generates the palette's names for its single colours (theme files name them), from one list.
macro_rules! named_colours {
    ($($name:ident),* $(,)?) => {
        /// The names of the palette's single colours (its `series` aside), as theme files write
        /// them.
        pub const COLOURS: &[&str] = &[$(stringify!($name)),*];

        impl Palette {
            /// The single colour named `name` (one of [`COLOURS`]).
            pub fn colour(&self, name: &str) -> Option<Rgb> {
                match name {
                    $(stringify!($name) => Some(self.$name),)*
                    _ => None,
                }
            }

            /// The single colour named `name`, to change.
            pub fn colour_mut(&mut self, name: &str) -> Option<&mut Rgb> {
                match name {
                    $(stringify!($name) => Some(&mut self.$name),)*
                    _ => None,
                }
            }
        }
    };
}

named_colours!(ground, ink, ink_dim, grid, mark, line, metal, halo, muted, e_arrows, h_arrows);

impl Palette {
    /// The last resort when no theme can be read (the built-in themes are files, read by
    /// `emcraft_engine::theme`): black on white, greys for the rest.
    pub fn fallback() -> Palette {
        let grey = |t: f64| Rgb::BLACK.mix(Rgb::WHITE, t);
        Palette {
            ground: Rgb::WHITE,
            ink: Rgb::BLACK,
            ink_dim: grey(0.35),
            grid: grey(0.9),
            mark: grey(0.5),
            line: grey(0.25),
            metal: grey(0.6),
            halo: Rgb::WHITE,
            muted: grey(0.5),
            e_arrows: Rgb::BLACK,
            h_arrows: grey(0.3),
            series: [Rgb::BLACK, grey(0.3), grey(0.45), grey(0.15), grey(0.55), grey(0.4)],
        }
    }

    /// Series colour `i`, cycled.
    fn series(&self, i: u8) -> Rgb {
        self.series[usize::from(i) % SERIES]
    }

    /// The colour of `pen` in this palette.
    pub fn pen(&self, pen: Pen) -> Rgb {
        match pen {
            Pen::Series(i) => self.series(i),
            Pen::Strong(i) => self.series(i).mix(self.ink, SHADE),
            Pen::Faded(i) => self.series(i).mix(self.ground, SHADE),
            Pen::Ramp(t) => self.series[0].mix(self.series[1], if t.is_finite() { t } else { 0.0 }),
            Pen::Muted => self.muted,
            Pen::Ink => self.ink,
            Pen::EArrows => self.e_arrows,
            Pen::HArrows => self.h_arrows,
            Pen::Custom(c) => c,
        }
    }

    /// The pairs of colours that must stay readable on each other, with the least WCAG contrast
    /// each needs: the ink on the ground (4.5, text) and the dim ink on the ground (3, large or
    /// secondary text).
    pub fn contrast_pairs(&self) -> [(&'static str, Rgb, &'static str, Rgb, f64); 2] {
        [("ink", self.ink, "ground", self.ground, 4.5), ("ink_dim", self.ink_dim, "ground", self.ground, 3.0)]
    }

    /// The first pair of [`Palette::contrast_pairs`] below its contrast, as a sentence.
    pub fn contrast_problem(&self) -> Option<String> {
        self.contrast_pairs().into_iter().find_map(|(a, ca, b, cb, least)| {
            let r = ca.contrast_ratio(cb);
            (r < least).then(|| format!("`{a}` on `{b}` has a contrast of {r:.2}, below {least}"))
        })
    }
}

impl Default for Palette {
    fn default() -> Self {
        Palette::fallback()
    }
}

impl Rgb {
    /// WCAG contrast of two opaque sRGB colours: 1 (the same) to 21 (black on white).
    pub fn contrast_ratio(self, other: Rgb) -> f64 {
        let luminance = |c: Rgb| {
            let linear = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * linear(c.0) + 0.7152 * linear(c.1) + 0.0722 * linear(c.2)
        };
        let (a, b) = (luminance(self), luminance(other));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// `#rrggbb`.
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// Palettes for this crate's tests (the built-in ones are files, read by the engine): dark ink
/// on white, and light ink on a dark ground.
#[cfg(test)]
pub(crate) mod samples {
    use crate::Rgb;

    use super::Palette;

    pub(crate) fn light() -> Palette {
        Palette {
            ground: Rgb::WHITE,
            ink: Rgb(30, 30, 30),
            ink_dim: Rgb(90, 90, 90),
            grid: Rgb(225, 225, 225),
            mark: Rgb(120, 120, 120),
            line: Rgb(60, 60, 60),
            metal: Rgb(150, 150, 155),
            halo: Rgb::WHITE,
            muted: Rgb(125, 125, 132),
            e_arrows: Rgb(20, 20, 20),
            h_arrows: Rgb(0, 120, 60),
            series: [Rgb(30, 90, 200), Rgb(200, 60, 40), Rgb(40, 150, 90), Rgb(150, 70, 180), Rgb(210, 140, 30), Rgb(60, 60, 60)],
        }
    }

    pub(crate) fn dark() -> Palette {
        Palette {
            ground: Rgb(39, 43, 52),
            ink: Rgb(222, 226, 232),
            ink_dim: Rgb(160, 167, 180),
            grid: Rgb(58, 64, 76),
            series: [Rgb(90, 150, 240), Rgb(240, 110, 90), Rgb(90, 200, 130), Rgb(190, 130, 230), Rgb(240, 180, 70), Rgb(175, 180, 190)],
            ..light()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pens_resolve_against_the_palette() {
        let p = samples::light();
        assert_eq!(p.pen(Pen::Series(0)), Rgb(30, 90, 200));
        assert_eq!(p.pen(Pen::Series(7)), p.pen(Pen::Series(1)), "cycled");
        assert_eq!(p.pen(Pen::Ramp(0.0)), p.series[0]);
        assert_eq!(p.pen(Pen::Ramp(1.0)), p.series[1]);
        assert_eq!(p.pen(Pen::Ramp(f64::NAN)), p.series[0]);
        assert_eq!(p.pen(Pen::Ramp(-3.0)), p.series[0]);
        // Strong is darker than its series on white, faded lighter; the other way in the dark.
        let sum = |c: Rgb| u32::from(c.0) + u32::from(c.1) + u32::from(c.2);
        assert!(sum(p.pen(Pen::Strong(0))) < sum(p.series[0]) && sum(p.pen(Pen::Faded(0))) > sum(p.series[0]));
        let d = samples::dark();
        assert!(sum(d.pen(Pen::Strong(0))) > sum(d.series[0]) && sum(d.pen(Pen::Faded(0))) < sum(d.series[0]));
        assert_eq!((p.pen(Pen::Ink), p.pen(Pen::Muted), p.pen(Pen::EArrows), p.pen(Pen::HArrows)), (p.ink, p.muted, p.e_arrows, p.h_arrows));
    }

    #[test]
    fn named_colours_and_contrast() {
        let mut p = samples::light();
        assert_eq!(COLOURS.len(), 11);
        for name in COLOURS {
            assert!(p.colour(name).is_some(), "{name}");
        }
        assert_eq!(p.colour("series"), None);
        if let Some(c) = p.colour_mut("grid") {
            *c = Rgb(1, 2, 3);
        }
        assert_eq!(p.grid, Rgb(1, 2, 3));
        assert!((Rgb::BLACK.contrast_ratio(Rgb::WHITE) - 21.0).abs() < 1e-9);
        assert!((Rgb(9, 9, 9).contrast_ratio(Rgb(9, 9, 9)) - 1.0).abs() < 1e-12);
        for q in [samples::light(), samples::dark(), Palette::fallback()] {
            assert_eq!(q.contrast_problem(), None, "{q:?}");
        }
        p.ink = p.ground;
        assert!(p.contrast_problem().is_some_and(|m| m.contains("`ink` on `ground`")));
        assert_eq!(Rgb(30, 90, 200).hex(), "#1e5ac8");
    }
}

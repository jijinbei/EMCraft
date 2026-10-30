//! The inductive-iris band-pass filter's sections: the geometry the `iris-bandpass` template
//! builds through `geometry.iris_filter`, with as many irises as it is given.

use emcraft_model::Section;

/// Most irises of an iris filter.
pub const MAX_IRISES: usize = 32;

/// The sections of an inductive-iris filter along z: a feed, then `irises` irises (apertures
/// centred across x) with a cavity between each two, then a feed. The first and last irises are
/// `end_width` wide and the cavities next to them `cavity` long; the ones between are
/// `inner_width` wide and `inner_cavity` long (the end ones' when left out). Lengths in metres.
///
/// ```
/// use emcraft_templates::{IrisFilter, iris_sections};
/// let f = IrisFilter { irises: 3, iris_length: 2e-3, end_width: 12e-3, inner_width: 9e-3, cavity: 15e-3, inner_cavity: 16e-3, feed: 10e-3, material: "vacuum".into() };
/// let s = iris_sections(&f);
/// // feed, iris, cavity, iris, cavity, iris, feed
/// assert_eq!(s.len(), 7);
/// assert_eq!(s.iter().filter_map(|x| x.width).collect::<Vec<_>>(), [12e-3, 9e-3, 12e-3]);
/// // with three irises, both cavities touch an end iris
/// assert_eq!([s[2].length, s[4].length], [15e-3, 15e-3]);
/// ```
pub fn iris_sections(f: &IrisFilter) -> Vec<Section> {
    let section = |length: f64, width: Option<f64>| Section { length, material: f.material.clone(), width, ..Section::default() };
    let mut out = vec![section(f.feed, None)];
    for k in 0..f.irises {
        let end = k == 0 || k + 1 == f.irises;
        out.push(section(f.iris_length, Some(if end { f.end_width } else { f.inner_width })));
        if k + 1 < f.irises {
            // The cavity after iris k: an end one when it touches the first or the last iris.
            let end = k == 0 || k + 2 == f.irises;
            out.push(section(if end { f.cavity } else { f.inner_cavity }, None));
        }
    }
    out.push(section(f.feed, None));
    out
}

/// An inductive-iris filter's dimensions ([`iris_sections`]); lengths in metres.
#[derive(Clone, Debug, PartialEq)]
pub struct IrisFilter {
    pub irises: usize,
    pub iris_length: f64,
    pub end_width: f64,
    pub inner_width: f64,
    pub cavity: f64,
    pub inner_cavity: f64,
    pub feed: f64,
    pub material: String,
}

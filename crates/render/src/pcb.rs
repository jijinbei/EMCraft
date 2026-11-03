//! PCB top-view template previews. The same even-odd spans fill the interactive workspace.
use crate::{Canvas, Rgb};
use emcraft_model::pcb::{Board, MAX_VERTICES};

/// Even-odd polygon spans at `y`, in the input coordinates' units. Invalid/excessive input
/// yields no spans. Repeated bridge edges allow filled polygons to describe holes.
/// ```
/// let spans=emcraft_render::pcb::spans(&[[0.,0.],[2.,0.],[2.,1.],[0.,1.]],0.5);
/// assert_eq!(spans,vec![[0.,2.]]);
/// assert!(emcraft_render::pcb::spans(&[],f64::NAN).is_empty());
/// ```
pub fn spans(poly: &[[f64; 2]], y: f64) -> Vec<[f64; 2]> {
    if !y.is_finite() || poly.len() > MAX_VERTICES || poly.iter().flatten().any(|v| !v.is_finite()) {
        return Vec::new();
    }
    let mut xs = Vec::new();
    for (a, b) in poly.iter().zip(poly.iter().cycle().skip(1)).take(poly.len()) {
        if (a[1] > y) != (b[1] > y) {
            xs.push(a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
        }
    }
    xs.sort_by(f64::total_cmp);
    xs.as_chunks::<2>().0.to_vec()
}

/// Draw a checked board into a bounded top view (1..2048 pixels per side), without a GPU.
/// Colours are supplied by the active theme; all board lengths are SI metres.
/// ```
/// use emcraft_render::{Rgb,pcb::preview};
/// let b=emcraft_model::pcb::Board{name:"empty".into(),layers:vec![],segments:vec![],vias:vec![],copper:vec![],notes:vec![]};
/// assert!(preview(&b,[200,100],Rgb::BLACK,Rgb::WHITE,Rgb::WHITE).is_err());
/// ```
pub fn preview(board: &Board, size: [usize; 2], bg: Rgb, copper: Rgb, signal: Rgb) -> Result<Canvas, String> {
    board.check().map_err(|e| e.to_string())?;
    let [w, h] = size;
    if w == 0 || h == 0 || w > 2048 || h > 2048 || board.copper.iter().map(|c| c.polygon.len()).sum::<usize>().saturating_mul(h) > 10_000_000 {
        return Err("PCB preview size or geometry exceeds drawing limits".into());
    }
    let (lo, hi) = board.bounds().ok_or("empty PCB")?;
    let scale = ((w as f64 * 0.88) / (hi[0] - lo[0]).max(1e-12)).min((h as f64 * 0.88) / (hi[1] - lo[1]).max(1e-12));
    let at = |p: [f64; 2]| [(p[0] - (lo[0] + hi[0]) / 2.) * scale + w as f64 / 2., (p[1] - (lo[1] + hi[1]) / 2.) * scale + h as f64 / 2.];
    let mut canvas = Canvas::new(w, h, bg);
    for c in &board.copper {
        let points: Vec<_> = c.polygon.iter().copied().map(at).collect();
        for y in 0..h {
            for [a, b] in spans(&points, y as f64 + 0.5) {
                canvas.rect(a, y as f64, b - a, 1., copper);
            }
        }
    }
    for s in &board.segments {
        canvas.line(at(s.start), at(s.end), (s.width * scale).max(2.), signal);
    }
    Ok(canvas)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_edges_keep_the_saved_copper_hole_open() {
        let poly = [[0., 0.], [4., 0.], [4., 4.], [0., 4.], [0., 0.], [1., 1.], [1., 3.], [3., 3.], [3., 1.], [1., 1.], [0., 0.]];
        assert_eq!(spans(&poly, 2.), vec![[0., 1.], [3., 4.]]);
        assert!(spans(&poly, 5.).is_empty());
    }
}

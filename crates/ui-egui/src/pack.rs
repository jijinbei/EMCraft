//! Packing a view's blocks round its main picture, decided, not simulated: each block says the
//! size it wants (its width, its height), the main picture its least size, and [`arrange`] tries
//! the few ways they can go — the blocks in a column beside the main one (as many as fit its
//! height, the rest under the main one), or all under it — and keeps the one that leaves the
//! least room empty. The same sizes always give the same rectangles, so a window keeps its places
//! and a test can say where everything goes.

use egui::{Rect, Vec2, pos2, vec2};

/// The gap between blocks (points).
pub const GAP: f32 = 10.0;

/// Where everything goes, in the area's coordinates (its top left at the origin): the main
/// picture, each block in the order given, and the height it all takes (more than the area's
/// when it has to scroll).
#[derive(Clone, Debug, PartialEq)]
pub struct Arrangement {
    pub main: Rect,
    pub blocks: Vec<Rect>,
    pub height: f32,
    /// What the blocks fill of their rectangles: the sizes they wanted.
    content: f32,
}

impl Arrangement {
    /// The area left empty: the arrangement's extent less the main picture and what the blocks
    /// fill (a block given more width than it wants leaves the rest empty).
    pub fn empty(&self, width: f32) -> f32 {
        (width * self.height - self.main.area() - self.content).max(0.0)
    }
}

/// What blocks of these sizes fill.
fn content(wants: &[Vec2]) -> f32 {
    wants.iter().map(|w| w.x * w.y).sum()
}

/// The blocks (`wants`: each one's width and height) round a main picture of at least `least`
/// in an area of `area`: the arrangement leaving the least room empty, beside before under on a
/// tie. The main picture takes the room the blocks leave.
///
/// ```
/// use egui::vec2;
/// use emcraft_ui_egui::pack::arrange;
/// // A wide area: the two blocks stack beside the main picture, which takes the rest.
/// let a = arrange(vec2(1200.0, 600.0), vec2(400.0, 260.0), &[vec2(300.0, 120.0), vec2(300.0, 200.0)]);
/// assert_eq!(a.blocks[0].left(), a.blocks[1].left());
/// assert!(a.main.right() <= a.blocks[0].left() && a.main.height() == 600.0);
/// // A narrow one: everything under the main picture.
/// let a = arrange(vec2(500.0, 600.0), vec2(400.0, 260.0), &[vec2(300.0, 120.0)]);
/// assert!(a.blocks[0].top() >= a.main.bottom());
/// ```
pub fn arrange(area: Vec2, least: Vec2, wants: &[Vec2]) -> Arrangement {
    let under = all_under(area, least, wants);
    match beside(area, least, wants) {
        Some(b) if b.empty(area.x) <= under.empty(area.x) => b,
        _ => under,
    }
}

/// Every block under the main picture, full width, the main picture taking the height left.
fn all_under(area: Vec2, least: Vec2, wants: &[Vec2]) -> Arrangement {
    let stacked: f32 = wants.iter().map(|w| w.y + GAP).sum();
    let main_h = (area.y - stacked).max(least.y);
    let main = Rect::from_min_size(pos2(0.0, 0.0), vec2(area.x, main_h));
    let mut y = main.bottom();
    let blocks = wants
        .iter()
        .map(|w| {
            let r = Rect::from_min_size(pos2(0.0, y + GAP), vec2(area.x, w.y));
            y = r.bottom();
            r
        })
        .collect();
    Arrangement { main, blocks, height: y.max(area.y), content: content(wants) }
}

/// The blocks in a column beside the main picture as wide as the widest of them, as many as fit
/// the area's height in order, the rest under the main picture; none when the main picture would
/// be narrower than it may.
fn beside(area: Vec2, least: Vec2, wants: &[Vec2]) -> Option<Arrangement> {
    let column = wants.iter().map(|w| w.x).fold(0.0, f32::max);
    let main_w = area.x - column - GAP;
    if wants.is_empty() || main_w < least.x {
        return None;
    }
    let (mut y, mut rects, mut left): (f32, Vec<Option<Rect>>, Vec<usize>) = (-GAP, vec![None; wants.len()], Vec::new());
    for (k, w) in wants.iter().enumerate() {
        if left.is_empty() && y + GAP + w.y <= area.y {
            rects[k] = Some(Rect::from_min_size(pos2(main_w + GAP, y + GAP), vec2(column, w.y)));
            y += GAP + w.y;
        } else {
            left.push(k);
        }
    }
    // What does not fit beside goes under the main picture, in the main picture's width.
    let below: f32 = left.iter().map(|&k| wants[k].y + GAP).sum();
    let main_h = (area.y - below).max(least.y);
    let main = Rect::from_min_size(pos2(0.0, 0.0), vec2(main_w, main_h));
    let mut y2 = main.bottom();
    for &k in &left {
        let r = Rect::from_min_size(pos2(0.0, y2 + GAP), vec2(main_w, wants[k].y));
        y2 = r.bottom();
        rects[k] = Some(r);
    }
    let blocks: Vec<Rect> = rects.into_iter().map(|r| r.unwrap_or(Rect::NOTHING)).collect();
    let height = blocks.iter().map(|r| r.bottom()).fold(main.bottom(), f32::max).max(area.y);
    Some(Arrangement { main, blocks, height, content: content(wants) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlap(a: &Rect, b: &Rect) -> bool {
        a.intersect(*b).area() > 1e-3 && a.is_positive() && b.is_positive()
    }

    /// Any arrangement: nothing overlaps, nothing runs past the area's width, every block gets
    /// at least the height it wants and the main picture its least size.
    fn sound(area: Vec2, least: Vec2, wants: &[Vec2], a: &Arrangement) {
        let all: Vec<&Rect> = std::iter::once(&a.main).chain(&a.blocks).collect();
        for (i, r) in all.iter().enumerate() {
            assert!(r.right() <= area.x + 1e-3 && r.left() >= -1e-3, "{r:?} past {area:?}");
            for s in &all[i + 1..] {
                assert!(!overlap(r, s), "{r:?} over {s:?}");
            }
        }
        for (r, w) in a.blocks.iter().zip(wants) {
            assert!((r.height() - w.y).abs() < 1e-3);
        }
        assert!(a.main.width() >= least.x - 1e-3 || area.x < least.x);
        assert!(a.main.height() >= least.y - 1e-3);
    }

    #[test]
    fn every_size_packs_soundly_and_the_same_sizes_the_same_way() {
        let least = vec2(360.0, 260.0);
        let blocks = [vec2(440.0, 120.0), vec2(440.0, 230.0), vec2(300.0, 40.0)];
        for w in (300..=2000).step_by(50) {
            for h in (200..=1200).step_by(100) {
                let area = vec2(w as f32, h as f32);
                let a = arrange(area, least, &blocks);
                sound(area, least, &blocks, &a);
                assert_eq!(a, arrange(area, least, &blocks));
            }
        }
    }

    #[test]
    fn a_wide_card_puts_blocks_beside_and_fills_its_height() {
        let area = vec2(1200.0, 600.0);
        let a = arrange(area, vec2(360.0, 260.0), &[vec2(440.0, 120.0), vec2(440.0, 230.0), vec2(300.0, 40.0)]);
        // All three fit beside (120 + 230 + 40 and the gaps): the main picture is the card's height.
        assert!(a.blocks.iter().all(|b| b.left() > a.main.right()));
        assert_eq!((a.main.height(), a.height), (600.0, 600.0));
        // Little is left empty: under a third of the card (the column's foot).
        assert!(a.empty(area.x) < area.x * area.y / 3.0, "{}", a.empty(area.x));
    }

    #[test]
    fn what_does_not_fit_beside_goes_under_the_main_picture() {
        let a = arrange(vec2(1200.0, 400.0), vec2(360.0, 200.0), &[vec2(440.0, 300.0), vec2(440.0, 150.0)]);
        assert!(a.blocks[0].left() > a.main.right());
        assert!(a.blocks[1].top() >= a.main.bottom() && a.blocks[1].right() <= a.main.right() + 1e-3);
    }

    #[test]
    fn a_narrow_card_stacks_and_scrolls() {
        let a = arrange(vec2(600.0, 400.0), vec2(360.0, 260.0), &[vec2(440.0, 300.0)]);
        assert!(a.blocks[0].top() >= a.main.bottom());
        assert!(a.height > 400.0);
    }
}

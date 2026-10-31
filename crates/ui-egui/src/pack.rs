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
    place(area, least, wants, area.x, Column::default())
}

/// The blocks in a column beside the main picture as wide as the widest of them, as many as fit
/// the area's height in order, the rest under the main picture; none when the main picture would
/// be narrower than it may.
fn beside(area: Vec2, least: Vec2, wants: &[Vec2]) -> Option<Arrangement> {
    let width = wants.iter().map(|w| w.x).fold(0.0, f32::max);
    let main_w = area.x - width - GAP;
    if wants.is_empty() || main_w < least.x {
        return None;
    }
    let mut y = -GAP;
    let count = wants
        .iter()
        .take_while(|w| {
            let fits = y + GAP + w.y <= area.y;
            y += GAP + w.y;
            fits
        })
        .count();
    Some(place(area, least, wants, main_w, Column { count, width }))
}

/// The column beside the main picture: how many of the blocks, first first, go in it and its width.
#[derive(Clone, Copy, Default)]
struct Column {
    count: usize,
    width: f32,
}

/// The column's blocks right of a main picture `main_w` wide, the rest under it in its width;
/// the main picture takes the height those under it leave.
fn place(area: Vec2, least: Vec2, wants: &[Vec2], main_w: f32, column: Column) -> Arrangement {
    let (beside, under) = wants.split_at(column.count.min(wants.len()));
    let below: f32 = under.iter().map(|w| w.y + GAP).sum();
    let main = Rect::from_min_size(pos2(0.0, 0.0), vec2(main_w, (area.y - below).max(least.y)));
    let mut blocks: Vec<Rect> = stack(beside, main_w + GAP, -GAP, column.width).collect();
    blocks.extend(stack(under, 0.0, main.bottom(), main_w));
    let height = blocks.iter().map(|r| r.bottom()).fold(main.bottom(), f32::max).max(area.y);
    Arrangement { main, blocks, height, content: content(wants) }
}

/// Blocks of these heights stacked down from `top` at `x`, each `width` wide, a gap above each.
fn stack(wants: &[Vec2], x: f32, top: f32, width: f32) -> impl Iterator<Item = Rect> + '_ {
    wants.iter().scan(top, move |y, w| {
        let r = Rect::from_min_size(pos2(x, *y + GAP), vec2(width, w.y));
        *y = r.bottom();
        Some(r)
    })
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

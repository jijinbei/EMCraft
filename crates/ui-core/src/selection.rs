//! What is selected in the 3D view and box selection, as in SolidCraft (`crates/ui-egui/src/
//! selection.rs` and `viewport.rs`'s `box_select`, MIT OR Apache-2.0; ATTRIBUTION.md): a plain
//! left drag draws a rectangle that selects by window when dragged left to right (the parts
//! wholly inside) or by crossing right to left (the parts it touches); Shift adds to what is
//! selected; a click with Shift or Ctrl toggles one part. A part is the triangles of one
//! [`Pick`] as the view projects them. The pure parts are unit tested; the view uses them.

use std::collections::BTreeMap;

use crate::geom::{Point, Rect};
use emcraft_render::view3d::{Camera, Scene};

use crate::pick::Pick;

/// The most triangles a box selection looks at (a mesh's boundary is far below it).
const MAX_TRIANGLES: usize = 4_000_000;

/// A rectangle dragged in the view selects by window (dragged left to right: the parts wholly
/// inside) or by crossing (right to left: the parts touching it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxSel {
    pub rect: Rect,
    pub crossing: bool,
}

impl BoxSel {
    /// The box dragged from `start` to `end`.
    ///
    /// ```
    /// use emcraft_ui_core::selection::BoxSel;
    /// let b = BoxSel::from_drag(emcraft_ui_core::geom::Point::new(100.0, 10.0), emcraft_ui_core::geom::Point::new(20.0, 80.0));
    /// assert!(b.crossing && b.rect.width() == 80.0);
    /// ```
    pub fn from_drag(start: Point, end: Point) -> BoxSel {
        BoxSel { rect: Rect::from_two_pos(start, end), crossing: end.x < start.x }
    }

    /// Whether the polyline (screen points) counts as selected.
    pub fn takes(&self, pts: &[Point]) -> bool {
        if pts.is_empty() {
            return false;
        }
        if !self.crossing {
            return pts.iter().all(|p| self.rect.contains(*p));
        }
        pts.iter().any(|p| self.rect.contains(*p)) || pts.windows(2).any(|w| segment_hits_rect(w[0], w[1], self.rect))
    }

    /// Whether a triangle (screen points) counts: by window all its corners inside, by crossing
    /// any part of it touching the box (an edge through it, or the box inside the triangle).
    pub fn takes_triangle(&self, t: [Point; 3]) -> bool {
        if !self.crossing {
            return self.takes(&t);
        }
        self.takes(&[t[0], t[1], t[2], t[0]]) || inside_triangle(self.rect.center(), t)
    }
}

/// Does the segment a–b cross the rectangle's boundary or lie inside it?
pub fn segment_hits_rect(a: Point, b: Point, r: Rect) -> bool {
    if r.contains(a) || r.contains(b) {
        return true;
    }
    let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()];
    (0..4).any(|i| segments_cross(a, b, c[i], c[(i + 1) % 4]))
}

fn orient(a: Point, b: Point, c: Point) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn segments_cross(p: Point, q: Point, r: Point, s: Point) -> bool {
    let (d1, d2, d3, d4) = (orient(r, s, p), orient(r, s, q), orient(p, q, r), orient(p, q, s));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}

/// Whether `p` lies inside (or on) the triangle `t`, either winding.
fn inside_triangle(p: Point, t: [Point; 3]) -> bool {
    let (a, b, c) = (orient(t[0], t[1], p), orient(t[1], t[2], p), orient(t[2], t[0], p));
    (a >= 0.0 && b >= 0.0 && c >= 0.0) || (a <= 0.0 && b <= 0.0 && c <= 0.0)
}

/// The parts the box `bx` (points from the view's top left) selects in `scene` seen by `camera`
/// in a `w` × `h` view, in tag order (the same scene and box give the same list). By window a
/// part is taken when every triangle of it is inside (one corner behind the eye keeps it out);
/// by crossing when any triangle touches the box. Field planes and material display regions are not separately box-selected model parts.
pub fn boxed(scene: &Scene, camera: &Camera, w: f64, h: f64, bx: BoxSel) -> Vec<Pick> {
    // Per tag: whether every triangle so far is in (window) or any touches (crossing).
    let mut parts: BTreeMap<u32, bool> = BTreeMap::new();
    for t in scene.tris.iter().take(MAX_TRIANGLES) {
        if t.tag == 0 {
            continue;
        }
        let corners = t.p.map(|p| camera.project(p, w, h).map(|q| Point::new(q[0] as f32, q[1] as f32)));
        let took = match corners {
            [Some(a), Some(b), Some(c)] if [a, b, c].iter().all(|p| p.is_finite()) => bx.takes_triangle([a, b, c]),
            _ => false,
        };
        let e = parts.entry(t.tag).or_insert(!bx.crossing);
        *e = if bx.crossing { *e || took } else { *e && took };
    }
    parts
        .into_iter()
        .filter(|(_, on)| *on)
        .filter_map(|(tag, _)| Pick::from_tag(tag))
        .filter(|p| !matches!(p, Pick::Plane(_) | Pick::Material(_)))
        .collect()
}

/// What is selected — the one selection of the window: parts of the model (in the 3D view and
/// as their browser rows), or a browser row that has no part (a boundary, the mesh). Written
/// only through its methods, so the two never disagree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    parts: Vec<Pick>,
    row: Option<String>,
}

impl std::ops::Deref for Selection {
    type Target = [Pick];
    fn deref(&self) -> &[Pick] {
        &self.parts
    }
}

impl Selection {
    /// `parts`, the browser row of the last one `row` (its key), or a row with no part.
    pub fn of(parts: Vec<Pick>, row: Option<String>) -> Selection {
        Selection { parts, row }
    }

    /// The browser row selected (`section/text`): the last part's, or a row with no part.
    pub fn row(&self) -> Option<&str> {
        self.row.as_deref()
    }

    /// The only part selected, when one is.
    pub fn single(&self) -> Option<Pick> {
        match self.parts[..] {
            [p] => Some(p),
            _ => None,
        }
    }

    /// Whether nothing at all is selected.
    pub fn is_clear(&self) -> bool {
        self.parts.is_empty() && self.row.is_none()
    }

    pub fn clear(&mut self) {
        self.parts.clear();
        self.row = None;
    }

    /// `picks` added (in their order, each once).
    pub fn add(&mut self, picks: &[Pick]) {
        for p in picks {
            if !self.parts.contains(p) {
                self.parts.push(*p);
            }
        }
    }

    /// `pick` taken out when it is in, else added at the end.
    pub fn toggle(&mut self, pick: Pick) {
        if self.parts.contains(&pick) {
            self.parts.retain(|p| *p != pick);
        } else {
            self.parts.push(pick);
        }
    }
}

/// A click in the 3D view while no tool is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Click {
    /// One click on `pick` (`None`: empty space); `add`: Shift or Ctrl held.
    Single { pick: Option<Pick>, add: bool },
    /// A double click.
    Double { pick: Option<Pick> },
}

/// What a click asks of the view besides the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Then {
    Nothing,
    /// Fit the model to the view.
    Fit,
    /// Edit the part picked (its row's edit).
    Edit,
}

/// The selection's rules, one table: a click picks what is under it (Shift or Ctrl adds or
/// takes it away), on empty space clears; a double click on a part picks it alone to edit it,
/// on empty space clears and fits the view.
///
/// ```
/// use emcraft_ui_core::selection::{Click, Selection, Then, step};
/// use emcraft_ui_core::pick::Pick;
///
/// let mut s = Selection::default();
/// assert_eq!(step(&mut s, Click::Single { pick: Some(Pick::Guide), add: false }), Then::Nothing);
/// assert_eq!(&s[..], [Pick::Guide]);
/// step(&mut s, Click::Single { pick: Some(Pick::Port(0)), add: true });
/// assert_eq!(&s[..], [Pick::Guide, Pick::Port(0)]);
/// step(&mut s, Click::Single { pick: None, add: false });
/// assert!(s.is_clear());
/// assert_eq!(step(&mut s, Click::Double { pick: None }), Then::Fit);
/// ```
pub fn step(selection: &mut Selection, click: Click) -> Then {
    match click {
        Click::Double { pick: Some(p) } => {
            *selection = Selection::of(vec![p], None);
            Then::Edit
        }
        Click::Double { pick: None } => {
            selection.clear();
            Then::Fit
        }
        Click::Single { pick: Some(p), add: true } => {
            selection.row = None;
            selection.toggle(p);
            Then::Nothing
        }
        Click::Single { pick: None, add: true } => Then::Nothing,
        Click::Single { pick, add: false } => {
            *selection = Selection::of(pick.into_iter().collect(), None);
            Then::Nothing
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emcraft_render::Rgb;
    use emcraft_render::view3d::{Projection, StandardView};

    #[test]
    fn window_and_crossing_selection() {
        let inside = [Point::new(20.0, 20.0), Point::new(40.0, 30.0)];
        let partly = [Point::new(20.0, 20.0), Point::new(140.0, 30.0)];
        let through = [Point::new(-10.0, 50.0), Point::new(200.0, 50.0)];
        let away = [Point::new(300.0, 300.0), Point::new(320.0, 330.0)];
        // Left to right: window.
        let w = BoxSel::from_drag(Point::new(0.0, 0.0), Point::new(100.0, 100.0));
        assert!(!w.crossing);
        assert!(w.takes(&inside) && !w.takes(&partly) && !w.takes(&through) && !w.takes(&away));
        // Right to left: crossing.
        let c = BoxSel::from_drag(Point::new(100.0, 100.0), Point::new(0.0, 0.0));
        assert!(c.crossing);
        assert!(c.takes(&inside) && c.takes(&partly) && c.takes(&through) && !c.takes(&away));
        assert!(!c.takes(&[]) && !w.takes(&[]));
    }

    #[test]
    fn a_crossing_box_inside_a_large_triangle_touches_it() {
        let big = [Point::new(-500.0, -500.0), Point::new(900.0, -500.0), Point::new(-500.0, 900.0)];
        let c = BoxSel::from_drag(Point::new(60.0, 60.0), Point::new(40.0, 40.0));
        assert!(c.takes_triangle(big));
        let w = BoxSel::from_drag(Point::new(40.0, 40.0), Point::new(60.0, 60.0));
        assert!(!w.takes_triangle(big));
        // Wound the other way, the same.
        assert!(c.takes_triangle([big[0], big[2], big[1]]));
        assert!(!c.takes_triangle([Point::new(200.0, 200.0), Point::new(210.0, 200.0), Point::new(200.0, 210.0)]));
    }

    /// Two unit squares side by side in z = 0, ports 0 and 1, seen from the front.
    fn two_squares() -> (Scene, Camera) {
        let mut s = Scene::default();
        for (i, x0) in [(0usize, -2.0), (1, 1.0)] {
            s.tagged(Pick::Port(i).tag(), |s| {
                let (a, b, c, d) = ([x0, 0.0, 0.0], [x0 + 1.0, 0.0, 0.0], [x0 + 1.0, 1.0, 0.0], [x0, 1.0, 0.0]);
                s.tri(a, b, c, Rgb(200, 0, 0), 255);
                s.tri(a, c, d, Rgb(200, 0, 0), 255);
            });
        }
        // A field plane is never taken.
        s.tagged(Pick::Plane(0).tag(), |s| s.tri([-0.1, 0.1, 0.0], [0.1, 0.1, 0.0], [0.0, 0.2, 0.0], Rgb(0, 0, 0), 255));
        let mut cam = Camera { projection: Projection::Orthographic, ..Camera::default() };
        cam.standard(StandardView::Front);
        cam.fit([-2.0, 0.0, 0.0], [2.0, 1.0, 0.0], 1.0);
        (s, cam)
    }

    #[test]
    fn boxes_take_whole_parts_by_window_and_touched_ones_by_crossing() {
        let (s, cam) = two_squares();
        let (w, h) = (400.0, 400.0);
        let on = |p: [f64; 3]| cam.project(p, w, h).map_or(Point::ORIGIN, |q| Point::new(q[0] as f32, q[1] as f32));
        // Around the right square only, left to right: a window.
        let window = BoxSel::from_drag(on([0.9, 1.1, 0.0]), on([2.1, -0.1, 0.0]));
        assert!(!window.crossing);
        assert_eq!(boxed(&s, &cam, w, h, window), vec![Pick::Port(1)]);
        // Right to left from there into half the left square: a crossing.
        let crossing = BoxSel::from_drag(on([2.1, -0.1, 0.0]), on([-1.5, 1.1, 0.0]));
        assert!(crossing.crossing);
        assert_eq!(boxed(&s, &cam, w, h, crossing), vec![Pick::Port(0), Pick::Port(1)]);
        // A window that cuts both takes neither.
        let cut = BoxSel::from_drag(on([-1.5, 1.1, 0.0]), on([1.5, -0.1, 0.0]));
        assert!(boxed(&s, &cam, w, h, cut).is_empty());
        // Hostile: an empty scene, a zero-sized view.
        assert!(boxed(&Scene::default(), &cam, w, h, window).is_empty());
        assert!(boxed(&s, &cam, 0.0, f64::NAN, crossing).iter().all(|p| !matches!(p, Pick::Plane(_) | Pick::Material(_))));
    }

    #[test]
    fn adding_and_toggling_keep_the_order() {
        let mut sel = Selection::of(vec![Pick::Port(1)], None);
        sel.add(&[Pick::Port(0), Pick::Port(1), Pick::Solid]);
        assert_eq!(&sel[..], [Pick::Port(1), Pick::Port(0), Pick::Solid]);
        sel.toggle(Pick::Port(0));
        assert_eq!(&sel[..], [Pick::Port(1), Pick::Solid]);
        sel.toggle(Pick::Port(0));
        assert_eq!(&sel[..], [Pick::Port(1), Pick::Solid, Pick::Port(0)]);
    }
}

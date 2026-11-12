//! The 3D view's tools in the iced window: the shared rules ([`emcraft_ui_core::tools`]) find
//! what the pointer is over, decide what a click asks for and move the reference plane; this
//! does what they ask — a part selected, a command run or its dialog opened, a measurement
//! taken — and is drawn by [`crate::view3d::marks`].

use emcraft_ui_core::lang::tr;
use emcraft_ui_core::selection::Selection;
use emcraft_ui_core::tools::{Measured, Outcome, RefPlane, Sight, grips, reference};

use crate::app::App;
use crate::view3d::GripMove;

impl App {
    /// The view's size (points).
    fn view_size(&self) -> emcraft_ui_core::geom::Vector {
        emcraft_ui_core::geom::Vector::new(self.view3d.size.0, self.view3d.size.1)
    }

    /// What the pointer is over for the tool on (nothing while none is).
    pub(crate) fn tool_hover(&mut self) {
        let (camera, size) = (self.view3d.camera, self.view_size());
        let sight = self.view3d.shown.as_ref().map(|s| Sight { built: &s.built, generation: self.generation, camera: &camera, size });
        self.tools.hovered(self.session.model(), sight.as_ref(), self.tool, self.hover);
    }

    /// A click with a tool on: on empty space the tool goes off, as Escape does; else what it
    /// asks for is done.
    pub(crate) fn tool_click(&mut self) {
        let Some(tool) = self.tool.filter(|_| self.job.is_none()) else { return };
        if self.tools.hover.is_none() {
            self.tool = None;
            self.tools.reset(None);
            return;
        }
        let l = self.session.display().length;
        match self.tools.clicked(tool, self.session.model(), l) {
            Some(Outcome::Select(pick)) => {
                let row = emcraft_ui_core::browser::row_of(&self.session, &self.replies, pick).map(|(k, _)| k);
                self.selection = Selection::of(vec![pick], row);
            }
            Some(Outcome::Run(id, params)) => self.start(id, params),
            Some(Outcome::Dialog(id, params)) => match self.open(id, &params) {
                Ok(d) => self.dialog = Some(d),
                Err(e) => self.error = Some(e),
            },
            Some(Outcome::Refused(why)) => self.error = Some(tr(why).to_owned()),
            Some(Outcome::Measure(picks)) => {
                // A question of the model: answered at once, nothing changed.
                let params = emcraft_ui_core::tools::measure_params(l, picks);
                match self.session.execute("geometry.measure", &params) {
                    Ok(reply) => self.tools.measured = Some(Measured { picks, reply }),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            None => {}
        }
    }

    /// The selected port's reference plane (as dragged, while it is).
    pub(crate) fn reference(&self) -> Option<RefPlane> {
        reference::ref_plane(self.session.model(), &self.selection, self.tools.drag.as_ref())
    }

    /// The reference plane's grip on the view.
    pub(crate) fn grip_at(&self) -> Option<emcraft_ui_core::geom::Point> {
        self.reference().and_then(|r| reference::grip(&r, &self.view3d.camera, self.view_size()))
    }

    /// The selected body's grips to draw, the drag applied.
    pub(crate) fn body_grips(&self) -> Vec<grips::Mark> {
        grips::marks(self.session.model(), &self.selection, self.body_drag.as_ref(), self.session.display().length)
    }

    /// Every grip a press takes, on the view: the selected body's and the reference plane's.
    pub(crate) fn grip_points(&self) -> Vec<emcraft_ui_core::geom::Point> {
        let (camera, size) = (self.view3d.camera, self.view_size());
        let body = self.body_grips().into_iter().filter_map(move |m| emcraft_ui_core::tools::screen(&camera, size, m.at));
        body.chain(self.grip_at()).collect()
    }

    /// A grip dragged: a body's under the press (else the reference plane's) begun, moved, let go
    /// into one `geometry.body.edit` (or `model.port.edit {deembed}`).
    pub(crate) fn grip(&mut self, m: GripMove) {
        let (camera, size, l) = (self.view3d.camera, self.view_size(), self.session.display().length);
        match m {
            _ if self.job.is_some() => {}
            GripMove::Start(at) => {
                self.body_drag = grips::start(self.session.model(), &self.selection, &camera, size, at);
                if self.body_drag.is_none() {
                    self.tools.drag = self.reference().map(reference::start);
                }
            }
            GripMove::By(dx, dy) => {
                let d = emcraft_ui_core::geom::Vector::new(dx, dy);
                if let Some(bd) = self.body_drag.take() {
                    self.body_drag = Some(grips::dragged(bd, self.session.model(), l, &camera, size, d));
                } else if let (Some(r), Some(drag)) = (self.reference(), self.tools.drag.take()) {
                    self.tools.drag = Some(reference::dragged(drag, &r, self.session.model(), &camera, size, d));
                }
            }
            GripMove::End => self.let_go(),
        }
    }

    /// The grip let go: its edit run, when it moved something.
    fn let_go(&mut self) {
        let l = self.session.display().length;
        if let Some(bd) = self.body_drag.take() {
            if let Some(params) = grips::let_go(&bd, l) {
                self.run_now("geometry.body.edit", &params);
            }
        } else if let Some(d) = self.tools.drag.take()
            && let Some(params) = reference::let_go(&d, self.session.model(), l)
        {
            self.run_now("model.port.edit", &params);
        }
    }

    /// A key typed while a body's grip is held: the digits give its value exactly, Enter lets
    /// go with it (nothing while no grip is held).
    pub(crate) fn typed(&mut self, key: Option<char>) {
        match (self.body_drag.take(), key) {
            (Some(bd), Some('\n')) => {
                self.body_drag = Some(bd);
                self.let_go();
            }
            (Some(bd), key) => self.body_drag = Some(grips::typed(bd, key)),
            (None, _) => {}
        }
    }
}

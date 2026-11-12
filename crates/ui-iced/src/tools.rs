//! The 3D view's tools in the iced window: the shared rules ([`emcraft_ui_core::tools`]) find
//! what the pointer is over, decide what a click asks for and move the reference plane; this
//! does what they ask — a part selected, a command run or its dialog opened, a measurement
//! taken — and is drawn by [`crate::view3d::marks`].

use emcraft_ui_core::lang::tr;
use emcraft_ui_core::selection::Selection;
use emcraft_ui_core::tools::{Measured, Outcome, RefPlane, Sight, reference};

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
                Err(e) => self.fail(e),
            },
            Some(Outcome::Refused(why)) => self.fail(tr(why).to_owned()),
            Some(Outcome::Measure(picks)) => {
                // A question of the model: answered at once, nothing changed.
                let params = emcraft_ui_core::tools::measure_params(l, picks);
                match self.session.execute("geometry.measure", &params) {
                    Ok(reply) => self.tools.measured = Some(Measured { picks, reply }),
                    Err(e) => self.fail(e.to_string()),
                }
            }
            None => {}
        }
    }

    /// The selected port's reference plane (as dragged, while it is).
    pub(crate) fn reference(&self) -> Option<RefPlane> {
        reference::ref_plane(self.session.model(), &self.selection, self.tools.drag.as_ref())
    }

    /// The selected port and its reference plane's distance (display units, to a thousandth).
    pub(crate) fn reference_now(&self) -> Option<(usize, f64)> {
        self.reference().map(|r| (r.port, reference::rounded(self.session.display().length, r.deembed)))
    }

    /// The reference plane set to `value` (display units), when it is a number that moves it.
    pub(crate) fn set_reference(&mut self, value: Option<f64>) {
        let (Some(r), Some(x)) = (self.reference(), value.filter(|x| x.is_finite())) else { return };
        if reference::rounded(self.session.display().length, r.deembed) != x {
            self.run_now("model.port.edit", &serde_json::json!({"name": r.name, "deembed": x}));
        }
    }

    /// The reference plane's grip on the view.
    pub(crate) fn grip_at(&self) -> Option<emcraft_ui_core::geom::Point> {
        self.reference().and_then(|r| reference::grip(&r, &self.view3d.camera, self.view_size()))
    }

    /// The section's dimensions and the ports it meets, while it shows them.
    pub(crate) fn section_notes(&self) -> Option<(Vec<emcraft_ui_core::scene3d::section::Dim>, Vec<emcraft_ui_core::scene3d::section::PortMark>)> {
        let v = &self.view3d;
        let (cut, outline) = (v.section.filter(|_| v.section_dims)?, v.cut_outline()?);
        let l = self.session.display().length;
        Some(emcraft_ui_core::scene3d::section::notes(self.session.model(), l, self.session.driven_port(), cut, outline))
    }

    /// Every grip a press takes, on the view: the reference plane's (a shape has none, ADR 0010).
    pub(crate) fn grip_points(&self) -> Vec<emcraft_ui_core::geom::Point> {
        self.grip_at().into_iter().collect()
    }

    /// The reference plane's grip dragged: begun, moved, let go into one `model.port.edit
    /// {deembed}`.
    pub(crate) fn grip(&mut self, m: GripMove) {
        let (camera, size, l) = (self.view3d.camera, self.view_size(), self.session.display().length);
        match m {
            _ if self.job.is_some() => {}
            GripMove::Start(_) => self.tools.drag = self.reference().map(reference::start),
            GripMove::By(dx, dy) => {
                let d = emcraft_ui_core::geom::Vector::new(dx, dy);
                if let (Some(r), Some(drag)) = (self.reference(), self.tools.drag.take()) {
                    self.tools.drag = Some(reference::dragged(drag, &r, self.session.model(), &camera, size, d));
                }
            }
            GripMove::End => {
                if let Some(d) = self.tools.drag.take()
                    && let Some(params) = reference::let_go(&d, self.session.model(), l)
                {
                    self.run_now("model.port.edit", &params);
                }
            }
        }
    }
}

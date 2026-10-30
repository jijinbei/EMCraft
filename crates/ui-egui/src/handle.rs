//! The names scenarios and the control channel find things on screen by (`ui.handles`,
//! `ui.at`, a scenario's `{"click": name}`, docs/control-protocol.md): a [`Handle`] for each,
//! written as the text a test types — `tab:MODEL`, `toolbar:mesh.generate`, `view:field`,
//! `field:save:video`. The names that never change are the [`Fixed`] ones; the others carry
//! what they name (a tab, a command, a view, a row's text, an index).
//!
//! A handle is written straight into the frame's list of names (no text of its own is made
//! for it), and hashes as its text, so it also serves as a widget's id.

use std::fmt;
use std::hash::{Hash, Hasher};

use emcraft_engine::view::Plane;
use emcraft_render::view3d::StandardView;

use crate::View;
use crate::ribbon::RibbonTab;
use crate::steps::{Solver, Step};

emcraft_macros::named_enum! {
    /// A name that never changes: a button, a field, a chart or a card.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Fixed {
        // The application bar and its menus.
        BarStart = "bar:start",
        BarTitle = "bar:title",
        BarSettings = "bar:settings",
        MenuFile = "menu:file",
        MenuEdit = "menu:edit",
        EditUndo = "edit:undo",
        EditRedo = "edit:redo",
        FileNew = "file:new",
        FileOpen = "file:open",
        FileSave = "file:save",
        FileSaveAs = "file:save_as",
        FileExport = "file:export",
        FileStart = "file:start",
        // The file prompt, the question before changes are lost, the history's export.
        FilePath = "file:path",
        FileOk = "file:ok",
        FileCancel = "file:cancel",
        ConfirmSave = "confirm:save",
        ConfirmDiscard = "confirm:discard",
        ConfirmCancel = "confirm:cancel",
        ExportPath = "export:path",
        ExportOk = "export:ok",
        ExportCancel = "export:cancel",
        // The start page.
        HomeNew = "home:new",
        HomeOpen = "home:open",
        // The window's cards.
        Ribbon = "ribbon",
        Steps = "steps",
        StepPanel = "step_panel",
        Browser = "browser",
        Viewport = "viewport",
        Status = "status",
        Progress = "progress",
        StatusCancel = "status:cancel",
        // Command search.
        SearchField = "search:field",
        SearchPopup = "search:popup",
        // The dialog card.
        Dialog = "dialog",
        DialogOk = "dialog:ok",
        DialogCancel = "dialog:cancel",
        DialogClose = "dialog:close",
        DialogDock = "dialog:dock",
        DialogError = "dialog:error",
        // The step panels.
        IntentStart = "intent:start",
        IntentStop = "intent:stop",
        IntentApply = "intent:apply",
        ExcitationPreview = "excitation:preview",
        ExcitationPatterns = "excitation:patterns",
        SetupBand = "setup:band",
        SetupPoints = "setup:points",
        SetupCount = "setup:count",
        SetupFrequency = "setup:frequency",
        SetupOrder = "setup:order",
        RunStart = "run:start",
        RunError = "run:error",
        ResultsCheck = "results:check",
        ResultsTouchstone = "results:touchstone",
        ResultsTouchstonePath = "results:touchstone:path",
        // The result views.
        ChartSParameters = "chart:sparameters",
        ChartTdr = "chart:tdr",
        ChartDispersion = "chart:dispersion",
        ChartPattern = "chart:pattern",
        ChartStudy = "chart:study",
        PictureField = "picture:field",
        PicturePortMode = "picture:port_mode",
        TablePortModes = "table:port_modes",
        PortModesSolve = "port_modes:solve",
        PortModesFocus = "port_modes:focus",
        CompareRun = "compare:run",
        TdrPort = "tdr:port",
        PatternPhi = "pattern:phi",
        // The field view.
        FieldFrequency = "field:frequency",
        FieldAt = "field:at",
        FieldKeep = "field:keep",
        FieldBeside3d = "field:beside3d",
        FieldComponent = "field:component",
        FieldPhase = "field:phase",
        FieldLocator = "field:locator",
        FieldSpark = "field:spark",
        FieldSavePicture = "field:save:picture",
        FieldSaveVideo = "field:save:video",
        // The 3D view.
        View3d = "view3d",
        View3dSelected = "view3d:selected",
        View3dPlane = "view3d:plane",
        View3dFit = "view3d:fit",
        View3dFace = "view3d:face",
        View3dPlay = "view3d:play",
        View3dProjection = "view3d:projection",
    }
}

emcraft_macros::named_enum! {
    /// A part of a compound control (`field:phase:play`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Part {
        /// A video player's play button.
        Play = "play",
        /// A video player's speed.
        Speed = "speed",
    }
}

/// A name on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Handle<'a> {
    Fixed(Fixed),
    /// A part of a fixed control: `field:phase:play`.
    Part(Fixed, Part),
    /// A ribbon tab: `tab:MODEL`.
    Tab(RibbonTab),
    /// A ribbon button: `toolbar:mesh.generate`.
    Toolbar(&'a str),
    /// A ribbon panel's name under its buttons: `panel:MODEL:GEOMETRY`.
    Panel(RibbonTab, &'a str),
    /// A view's tab over the viewport: `view:field`.
    View(View),
    /// A browser row, by its text: `row:Port 1: Start, 4 modes`.
    Row(&'a str),
    /// A browser section's heading: `fold:Geometry`.
    Fold(&'a str),
    /// A step in the step bar: `step:mesh`.
    Step(Step),
    /// A step check's fix: `step:fix:0`.
    StepFix(usize),
    /// A search result: `search:row:mesh.generate`.
    SearchRow(&'a str),
    /// A sample's card on the start page: `sample:highpass`.
    Sample(&'a str),
    /// A recent project's card on the start page: `home:recent:0`.
    Recent(usize),
    /// A setting's choice: `settings:length:mm`.
    Setting(&'static str, &'a str),
    /// A dialog's field: `dialog:field:width`.
    DialogField(&'a str),
    /// A part of a vector field: `dialog:field:at:1`.
    DialogPart(&'a str, usize),
    /// A table field's cell: `dialog:cell:sections:0:length`.
    DialogCell(&'a str, usize, &'a str),
    /// A table field's Add: `dialog:add:sections`.
    DialogAdd(&'a str),
    /// A table row's remove: `dialog:remove:sections:1`.
    DialogRemove(&'a str, usize),
    /// A choice in a dialog's drop-down: `choice:material:vacuum`.
    Choice(&'a str, &'a str),
    /// The Setup step's solver: `setup:solver:frequency`.
    Solver(Solver),
    /// The Setup step's device: `setup:device:cpu`.
    Device(&'a str),
    /// The Setup step's precision: `setup:precision:double`.
    Precision(&'a str),
    /// A preflight finding's Go to: `preflight:blocks:0`, `preflight:warning:1`.
    Preflight(&'static str, usize),
    /// A refused Run's Go to: `run:refused:0`.
    Refused(usize),
    /// The Touchstone export's format: `results:touchstone:ri`.
    Touchstone(&'a str),
    /// A row of the Excitation step's mode table: `portmode:row:4`.
    PortModeRow(usize),
    /// A row of the Port modes view's table (the mode counted from 1): `port_modes:row:1:2`.
    PortModesRow(&'a str, usize),
    /// An eigenmode's Field button (counted from 1): `eigen:field:1`.
    EigenField(usize),
    /// The field view's plane: `field:plane:cross`.
    FieldPlane(Plane),
    /// A plane kept for the 3D view: `field:kept:0`.
    FieldKept(usize),
    /// A standard view in the 3D view's navigator: `view3d:std:front`.
    Standard(StandardView),
    /// The S-parameter a study's chart shows: `study:parameter:s21`.
    StudyParameter(&'a str),
    /// The variable a grid study is charted along: `study:along:a`.
    StudyAlong(&'a str),
    /// A held variable's value (by index): `study:at:b:1`.
    StudyAt(&'a str, usize),
    /// The TDR view's transform: `tdr:mode:auto`.
    TdrMode(&'a str),
}

impl From<Fixed> for Handle<'_> {
    fn from(f: Fixed) -> Self {
        Handle::Fixed(f)
    }
}

impl fmt::Display for Handle<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Handle::Fixed(x) => f.write_str(x.id()),
            Handle::Part(x, p) => write!(f, "{}:{}", x.id(), p.id()),
            Handle::Tab(t) => write!(f, "tab:{}", t.id()),
            Handle::Toolbar(id) => write!(f, "toolbar:{id}"),
            Handle::Panel(t, name) => write!(f, "panel:{}:{name}", t.id()),
            Handle::View(v) => write!(f, "view:{}", v.id()),
            Handle::Row(text) => write!(f, "row:{text}"),
            Handle::Fold(section) => write!(f, "fold:{section}"),
            Handle::Step(s) => write!(f, "step:{}", s.id()),
            Handle::StepFix(i) => write!(f, "step:fix:{i}"),
            Handle::SearchRow(id) => write!(f, "search:row:{id}"),
            Handle::Sample(name) => write!(f, "sample:{name}"),
            Handle::Recent(i) => write!(f, "home:recent:{i}"),
            Handle::Setting(group, name) => write!(f, "settings:{group}:{name}"),
            Handle::DialogField(name) => write!(f, "dialog:field:{name}"),
            Handle::DialogPart(name, k) => write!(f, "dialog:field:{name}:{k}"),
            Handle::DialogCell(name, row, column) => write!(f, "dialog:cell:{name}:{row}:{column}"),
            Handle::DialogAdd(name) => write!(f, "dialog:add:{name}"),
            Handle::DialogRemove(name, row) => write!(f, "dialog:remove:{name}:{row}"),
            Handle::Choice(param, value) => write!(f, "choice:{param}:{value}"),
            Handle::Solver(s) => write!(f, "setup:solver:{}", s.id()),
            Handle::Device(name) => write!(f, "setup:device:{name}"),
            Handle::Precision(p) => write!(f, "setup:precision:{p}"),
            Handle::Preflight(what, i) => write!(f, "preflight:{what}:{i}"),
            Handle::Refused(i) => write!(f, "run:refused:{i}"),
            Handle::Touchstone(format) => write!(f, "results:touchstone:{format}"),
            Handle::PortModeRow(row) => write!(f, "portmode:row:{row}"),
            Handle::PortModesRow(port, mode) => write!(f, "port_modes:row:{port}:{mode}"),
            Handle::EigenField(n) => write!(f, "eigen:field:{n}"),
            Handle::FieldPlane(p) => write!(f, "field:plane:{}", p.id()),
            Handle::FieldKept(k) => write!(f, "field:kept:{k}"),
            Handle::Standard(v) => write!(f, "view3d:std:{}", v.id()),
            Handle::StudyParameter(p) => write!(f, "study:parameter:{p}"),
            Handle::StudyAlong(name) => write!(f, "study:along:{name}"),
            Handle::StudyAt(name, i) => write!(f, "study:at:{name}:{i}"),
            Handle::TdrMode(mode) => write!(f, "tdr:mode:{mode}"),
        }
    }
}

/// Feeds text written to it into a hasher.
struct HashText<'a, H>(&'a mut H);

impl<H: Hasher> fmt::Write for HashText<'_, H> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.write(s.as_bytes());
        Ok(())
    }
}

/// A handle hashes as its text: two handles that read the same are the same widget.
impl Hash for Handle<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let _ = fmt::Write::write_fmt(&mut HashText(state), format_args!("{self}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_read_as_the_protocol_names_them() {
        let cases: [(Handle, &str); 12] = [
            (Handle::Tab(RibbonTab::PortsSources), "tab:PORTS & SOURCES"),
            (Handle::Toolbar("mesh.generate"), "toolbar:mesh.generate"),
            (Handle::View(View::PortModes), "view:port_modes"),
            (Fixed::FieldSaveVideo.into(), "field:save:video"),
            (Handle::Part(Fixed::FieldPhase, Part::Speed), "field:phase:speed"),
            (Handle::Row("Port 1: Start, 4 modes"), "row:Port 1: Start, 4 modes"),
            (Handle::Step(Step::Mesh), "step:mesh"),
            (Handle::DialogCell("sections", 0, "length"), "dialog:cell:sections:0:length"),
            (Handle::Solver(Solver::TimeDomain), "setup:solver:time_domain"),
            (Handle::FieldPlane(Plane::Cross), "field:plane:cross"),
            (Handle::Standard(StandardView::Front), "view3d:std:front"),
            (Handle::StudyAt("b", 2), "study:at:b:2"),
        ];
        for (h, text) in cases {
            assert_eq!(h.to_string(), text);
        }
    }

    #[test]
    fn a_handle_hashes_as_its_text() {
        let hash = |h: Handle| {
            let mut s = std::collections::hash_map::DefaultHasher::new();
            h.hash(&mut s);
            s.finish()
        };
        assert_eq!(hash(Handle::Toolbar("a.b")), hash(Handle::Toolbar("a.b")));
        assert_ne!(hash(Handle::Toolbar("a.b")), hash(Handle::Toolbar("a.c")));
        assert_eq!(Fixed::parse("field:save:video"), Some(Fixed::FieldSaveVideo));
    }
}

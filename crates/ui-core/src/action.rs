//! What a frame asks of the window: drawing reads the window and pushes []s into the
//! frame's []; once the frame is drawn the window performs each in turn. The control
//! channel and the keyboard shortcuts ask through the same actions, so a click, a request and a
//! key do a thing one way. The actions are the window's whatever draws it; performing them is
//! each window's own.

use emcraft_model::units::{Frequency, Length};
use emcraft_render::view3d::StandardView;
use serde_json::Value;

use crate::files::FileAction;
use crate::page::{Section, Tab};
use crate::pick::Pick;
use crate::ribbon::RibbonTab;
use crate::steps::ModeFocus;
use crate::view::View;
use emcraft_engine::workflow::{Analysis as Solver, Step};

/// Something a person (a click, a key), an agent or a test asks of the app.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Action {
    /// Runs an engine command: a quick one at once, a long one on the worker thread.
    /// Commands in order, each after the one before has finished (a long one on the worker);
    /// the first that fails stops the rest.
    RunAll(Vec<(String, Value)>),
    Run {
        id: String,
        params: Value,
    },
    /// Opens a command's dialog, `values` over its usual initial ones (a command without
    /// parameters runs at once).
    OpenDialog {
        id: String,
        values: Option<Value>,
    },
    /// OK on the open dialog: runs its command; refused, the dialog stays with the error.
    DialogOk,
    DialogCancel,
    /// Dialogs open docked at the right (`true`) or floating, the open one too.
    SetDock(bool),
    /// The viewport shows a view.
    SetView(View),
    /// A view's tab clicked: the view, its port modes solved first when it has none.
    ShowView(View),
    SetTab(RibbonTab),
    /// Opens a workflow step's panel (`None` closes it).
    OpenStep(Option<Step>),
    /// A check's fix: the step that owns `command`, with its dialog.
    Fix(String),
    /// A browser row (its key `section/text`) or parts in 3D selected (in order, the last the
    /// row's); none clears.
    Select {
        row: Option<String>,
        picks: Vec<Pick>,
    },
    /// A click in the 3D view while no tool is on (the selection's table: `crate::selection`).
    Click(crate::selection::Click),
    /// What a selection box took, added to the selection (`add`) or instead of it.
    SelectBox {
        picks: Vec<Pick>,
        add: bool,
    },
    /// The 3D view fitted to the model.
    Fit,
    /// Opens the edit dialog of the one part selected (its row's).
    EditSelected,
    /// Folds or unfolds a browser section.
    Fold(String),
    /// A row elsewhere points at a part of the 3D view: highlighted until the next frame.
    Hover(Pick),
    /// The port mode the Port modes view marks.
    FocusMode(ModeFocus),
    Undo,
    Redo,
    /// Cancels the running job.
    CancelJob,
    /// Closes a layer of the window (Escape closes the top one: `crate::ui_state`).
    Close(crate::layer::Layer),
    /// Opens the command search's list (its field typed in or focused).
    OpenSearch,
    /// Shows a page of the window (its tab).
    Show(Tab),
    /// Opens a template's card (its parameters and preview), or closes it (`None`).
    Template(Option<String>),
    /// The start page's purpose: the templates shown, what a project begun there is for.
    Purpose(Option<Solver>),
    /// Builds the project from a template with these values changed (`file.sample`).
    OpenTemplate(String, Value),
    /// Opens the settings at a section (`None`: their last, else their first) and shows them.
    OpenSettings(Option<Section>),
    /// Closes the settings, back to the page they were opened from.
    CloseSettings,
    Pref(Pref),
    File(FileAction),
    /// Opens an item's edit dialog on its current values (`project.values`).
    EditItem(crate::browser::Item),
    /// Deletes items, each by its command in turn (each undoable); a refusal stops there and
    /// shows as an error.
    DeleteItems(Vec<crate::browser::Item>),
    /// Lets an autosaved copy found on start go (it is deleted).
    DismissRecovery(usize),
    /// Asks where to export the history as a script.
    AskExport,
    /// Writes the history as a script to a path.
    ExportHistory(String),
    /// The Setup step's form confirmed (`project.setup`).
    ConfirmSetup,
    /// The Setup step's form filled with proposed settings (as `project.setup` takes them),
    /// to be checked and confirmed.
    ProposeSetup(Value),
    /// Run: the solver from the Setup step's fields, after the preflight.
    StartRun,
    /// The Excitation step's port-mode preview.
    PreviewPortModes,
    /// The Results step's passivity and reciprocity check.
    CheckResults,
    /// A system's chain read again after a change (`solve.system`, a moment): once per revision.
    AutoSolve,
    /// The Results step's Touchstone export, from its fields.
    ExportTouchstone,
    /// The device (and its precision) the solvers run on.
    SetDevice {
        device: emcraft_engine::Device,
        precision: emcraft_engine::Precision,
    },
    /// The 3D view from a standard side, fitted.
    StandardView(#[serde(with = "crate::action::named")] StandardView),
    /// The field's phase animation plays or pauses.
    SetPlaying(bool),
    /// A tool of the 3D view on (the ports' with the Excitation step and the model in 3D), or
    /// none.
    Tool(Option<crate::tools::Tool>),
    /// Starts drawing copper on the board with this pen, or stops (`None`).
    Pen(Option<crate::pen::Pen>),
    /// Measures between two picks (`geometry.measure`); the 3D view draws the reply.
    Measure([crate::tools::Snap; 2]),
    /// Opens the crash reports' card (each then marked seen).
    ShowReports,
    /// Deletes a crash report by its id.
    DeleteReport(String),
    /// Makes a report of the session as it is (an unexpected result with no crash), shown.
    ReportBug,
    /// A message in the message area.
    Message {
        text: String,
        error: bool,
    },
}

/// A preference set from the settings.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Pref {
    /// The theme, by name.
    Theme(String),
    /// The window's language, by id (`en`, `ja`).
    Language(String),
    /// A font file to show the language with (none: the system's fonts only).
    FontFile(Option<String>),
    /// Looks for the system's fonts again.
    SearchFonts,
    /// Reads the themes again (the user's from their folder).
    ReloadThemes,
    /// The 3D view's mouse mapping, by id.
    Navigation(String),
    OrbitAround(crate::navigation::OrbitAround),
    Navigator(crate::navigation::Navigator),
    /// What the GPU's view adds.
    GpuLook(crate::prefs::GpuLook),
    Length(Length),
    Frequency(Frequency),
    /// The SolidCraft program designs open in (empty: `solidcraft` on the PATH).
    SolidCraft(String),
    /// Drops a project from the recent list.
    ForgetRecent(usize),
    ClearRecent,
}

impl Action {
    /// Runs command `id` with `params`.
    pub fn run(id: &str, params: Value) -> Action {
        Action::Run { id: id.to_owned(), params }
    }

    /// Opens the dialog of command `id`.
    pub fn dialog(id: &str) -> Action {
        if id == "pcb.import" {
            return Action::File(crate::files::FileAction::PickPart);
        }
        Action::OpenDialog { id: id.to_owned(), values: None }
    }

    /// Opens the dialog of command `id` with `values` over its usual ones.
    pub fn dialog_with(id: &str, values: Value) -> Action {
        Action::OpenDialog { id: id.to_owned(), values: Some(values) }
    }

    /// An error in the message area.
    pub fn error(text: impl std::fmt::Display) -> Action {
        Action::Message { text: text.to_string(), error: true }
    }

    /// Whether the window puts what goes wrong in its message area when it performs the action. The others
    /// report it where it arises (a command's failure, by `run`) or show it in place (a
    /// dialog's refusal, under its field; a step's field errors).
    pub fn reports_errors(&self) -> bool {
        matches!(
            self,
            Action::OpenDialog { .. }
                | Action::EditItem(_)
                | Action::DeleteItems(_)
                | Action::ShowView(_)
                | Action::Fix(_)
                | Action::PreviewPortModes
                | Action::CheckResults
        )
    }
}

/// While a job runs: what changes the project or runs a command waits for it (one at a
/// time); what only looks, moves the view, selects, or opens and closes goes on. `apply`
/// refuses what waits, and `ui.act` draws its button greyed out, both by this one rule.
impl Action {
    /// Whether it waits while a job runs: what changes the project or runs a command (one at a
    /// time), not what only looks, moves the view, selects, or opens and closes. Every kind is
    /// named, so a new one is decided here when it is added.
    pub fn waits_for_job(&self) -> bool {
        match self {
            Action::RunAll(_)
            | Action::Run { .. }
            | Action::DialogOk
            | Action::Undo
            | Action::Redo
            | Action::OpenTemplate(..)
            | Action::EditItem(_)
            | Action::DeleteItems(_)
            | Action::ExportHistory(..)
            | Action::ConfirmSetup
            | Action::StartRun
            | Action::PreviewPortModes
            | Action::CheckResults
            | Action::AutoSolve
            | Action::ExportTouchstone
            | Action::SetDevice { .. }
            | Action::Measure(_)
            | Action::EditSelected => true,
            Action::File(f) => f.waits_for_job(),
            Action::OpenDialog { .. }
            | Action::DialogCancel
            | Action::SetDock(_)
            | Action::SetView(_)
            | Action::ShowView(_)
            | Action::SetTab(_)
            | Action::OpenStep(_)
            | Action::Fix(_)
            | Action::Select { .. }
            | Action::Click(_)
            | Action::SelectBox { .. }
            | Action::Fit
            | Action::Fold(_)
            | Action::Hover(_)
            | Action::FocusMode(_)
            | Action::CancelJob
            | Action::Close(_)
            | Action::OpenSearch
            | Action::Show(_)
            | Action::Template(_)
            | Action::Purpose(_)
            | Action::OpenSettings(_)
            | Action::CloseSettings
            | Action::Pref(_)
            | Action::DismissRecovery(_)
            | Action::AskExport
            | Action::ProposeSetup(_)
            | Action::StandardView(_)
            | Action::SetPlaying(_)
            | Action::Tool(_)
            | Action::Pen(_)
            | Action::Message { .. }
            | Action::ShowReports
            | Action::DeleteReport(_)
            | Action::ReportBug => false,
        }
    }
}

/// The actions a frame asked for, in order, and whether a job ran while it was drawn (what
/// waits for it is drawn greyed out, [`Action::waits_for_job`]).
#[derive(Debug, Default)]
pub struct Actions(Vec<Action>, bool);

impl Actions {
    /// A frame's actions, drawn while a job runs or not.
    pub fn during(busy: bool) -> Actions {
        Actions(Vec::new(), busy)
    }

    /// Whether `action` may be asked for now (it does not wait for a running job).
    pub fn allows(&self, action: &Action) -> bool {
        !(self.1 && action.waits_for_job())
    }

    pub fn push(&mut self, a: Action) {
        self.0.push(a);
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Action> {
        self.0.iter()
    }
}

impl Extend<Action> for Actions {
    fn extend<I: IntoIterator<Item = Action>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

impl IntoIterator for Actions {
    type Item = Action;
    type IntoIter = std::vec::IntoIter<Action>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// A named enum of another crate (the 3D view's standard sides) by its id.
pub mod named {
    use emcraft_macros::Named;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<T: Named, S: Serializer>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(v.id())
    }

    pub fn deserialize<'de, T: Named, D: Deserializer<'de>>(d: D) -> Result<T, D::Error> {
        let s = String::deserialize(d)?;
        T::parse(&s).ok_or_else(|| serde::de::Error::unknown_variant(&s, T::IDS))
    }
}

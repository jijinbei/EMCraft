//! The window's state and its transitions, apart from the drawing: what fills the window (the
//! page), what is open over it (one overlay at a time — a question, a template's card or the
//! search) and the dialog being filled. The drawing reads it and asks for a change with an
//! [`Action`]; [`UiState::update`] makes it, and nothing else does: the fields are private, the
//! drafts in them (a path being typed, a dialog's fields) the only thing the drawing edits.
//!
//! Escape closes the top layer ([`UiState::escape`]); a layer's way out is written once
//! ([`UiState::close`]). No egui here (a test keeps it so): the transitions are plain data,
//! checked by table in the tests below.

use crate::dialogs::Dialog;
use crate::files::{Answer, Ask, FileAction};
use crate::page::Pages;
use crate::template_dialog::TemplateDialog;
use crate::{Action, EmcraftApp};

emcraft_macros::named_enum! {
    /// A layer of the window, top first.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Layer {
        /// A question before work is lost, or a prompt for a path or a file's name.
        Ask = "ask",
        /// A template's card.
        Template = "template",
        /// The command search, open.
        Search = "search",
        /// The open dialog (floating, docked or in the step's panel).
        Dialog = "dialog",
        /// The settings page, shown.
        Settings = "settings",
        /// A route being drawn on the board (the pen).
        Pen = "pen",
        /// The 3D view's selection box, being dragged.
        Box = "box",
        /// The 3D view's tool, with what it picked.
        Tool = "tool",
        /// What is selected (in the 3D view and the browser).
        Selection = "selection",
    }
}

/// What is open over the window: one at a time.
pub enum Overlay {
    /// A question or a prompt (over everything: a card or the search open closes).
    Ask(Ask),
    /// A template's card, its fields being set.
    Template(Box<TemplateDialog>),
    /// The command search's list (its query is the ribbon's field).
    Search,
}

impl Overlay {
    /// Its layer.
    pub fn layer(&self) -> Layer {
        match self {
            Overlay::Ask(_) => Layer::Ask,
            Overlay::Template(_) => Layer::Template,
            Overlay::Search => Layer::Search,
        }
    }
}

/// Who has the keyboard when Escape is pressed: what decides whether the top layer takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyboard {
    /// No widget.
    Free,
    /// One of the open dialog's widgets.
    Dialog,
    /// A field somewhere else: Escape lets go of it, closing nothing below a question or the
    /// search (whose own fields they are).
    Elsewhere,
}

/// The window's state; see the module's docs.
#[derive(Default)]
pub struct UiState {
    /// What fills the window: [`Pages`] keeps its own transitions.
    pub page: Pages,
    overlay: Option<Overlay>,
    dialog: Option<Dialog>,
}

impl UiState {
    /// The window showing `page`, nothing open over it.
    pub fn at(page: Pages) -> UiState {
        UiState { page, overlay: None, dialog: None }
    }

    /// What is open over the window.
    pub fn overlay(&self) -> Option<&Overlay> {
        self.overlay.as_ref()
    }

    /// The question or prompt open.
    pub fn ask(&self) -> Option<&Ask> {
        match &self.overlay {
            Some(Overlay::Ask(a)) => Some(a),
            _ => None,
        }
    }

    /// The template's card open.
    pub fn template(&self) -> Option<&TemplateDialog> {
        match &self.overlay {
            Some(Overlay::Template(t)) => Some(t),
            _ => None,
        }
    }

    /// Whether the search's list is open.
    pub fn searching(&self) -> bool {
        matches!(self.overlay, Some(Overlay::Search))
    }

    /// The dialog open.
    pub fn dialog(&self) -> Option<&Dialog> {
        self.dialog.as_ref()
    }

    /// The open dialog's drafts: its fields, its error, where it sits (not whether it is open).
    pub fn dialog_mut(&mut self) -> Option<&mut Dialog> {
        self.dialog.as_mut()
    }

    /// The layers open, top first.
    pub fn layers(&self) -> impl Iterator<Item = Layer> {
        let over = self.overlay.as_ref().map(Overlay::layer);
        let dialog = self.dialog.is_some().then_some(Layer::Dialog);
        let settings = self.page.settings_shown().is_some().then_some(Layer::Settings);
        over.into_iter().chain(dialog).chain(settings)
    }

    /// The layer Escape closes, given the layers open below these ones (the 3D view's, top
    /// first) and who has the keyboard: the top one, unless a field it does not own has it.
    ///
    /// ```
    /// use emcraft_ui_egui::ui_state::{Keyboard, Layer, UiState};
    ///
    /// let s = UiState::default();
    /// assert_eq!(s.escape_target(&[Layer::Tool, Layer::Selection], Keyboard::Free), Some(Layer::Tool));
    /// // A field elsewhere lets go of the keyboard and closes nothing.
    /// assert_eq!(s.escape_target(&[Layer::Tool], Keyboard::Elsewhere), None);
    /// assert_eq!(s.escape_target(&[], Keyboard::Free), None);
    /// ```
    pub fn escape_target(&self, below: &[Layer], keyboard: Keyboard) -> Option<Layer> {
        let top = self.layers().chain(below.iter().copied()).next()?;
        let owned = match top {
            // Their own fields: Escape is theirs while one has the keyboard.
            Layer::Ask | Layer::Search => true,
            Layer::Dialog => keyboard != Keyboard::Elsewhere,
            _ => keyboard == Keyboard::Free,
        };
        owned.then_some(top)
    }

    /// Opens a question (a card or the search open closes: a question is answered first).
    pub fn set_ask(&mut self, ask: Ask) {
        self.overlay = Some(Overlay::Ask(ask));
    }

    /// The question open, closed.
    pub fn take_ask(&mut self) -> Option<Ask> {
        match self.overlay.take() {
            Some(Overlay::Ask(a)) => Some(a),
            other => {
                self.overlay = other;
                None
            }
        }
    }

    /// Opens a template's card, unless a question is open (it stays; the card is refused).
    pub fn open_template(&mut self, card: TemplateDialog) -> bool {
        if self.ask().is_some() {
            return false;
        }
        self.overlay = Some(Overlay::Template(Box::new(card)));
        true
    }

    /// Opens the search's list, unless a question or a card is open.
    pub fn open_search(&mut self) -> bool {
        match self.overlay {
            None | Some(Overlay::Search) => {
                self.overlay = Some(Overlay::Search);
                true
            }
            Some(_) => false,
        }
    }

    /// Opens `dialog` in place of the one open.
    pub fn open_dialog(&mut self, dialog: Dialog) {
        self.dialog = Some(dialog);
    }

    /// The dialog open, closed.
    pub fn take_dialog(&mut self) -> Option<Dialog> {
        self.dialog.take()
    }

    /// Makes the change `action` asks of the window's state: `None` when it is not one of its
    /// transitions (the app performs it), else what follows from it (the dialog's cancel, a
    /// prompt's answer), performed after.
    pub fn update(&mut self, action: &Action) -> Option<Vec<Action>> {
        let then = match action {
            Action::Close(layer @ (Layer::Ask | Layer::Template | Layer::Search | Layer::Dialog | Layer::Settings)) => self.close(*layer),
            Action::DialogCancel => {
                self.dialog = None;
                None
            }
            Action::Show(tab) => {
                self.page.show(*tab);
                None
            }
            Action::OpenSettings(section) => {
                self.page.open_settings(*section);
                None
            }
            Action::CloseSettings => {
                self.page.close_settings();
                None
            }
            Action::Template(None) => {
                self.close_overlay(Layer::Template);
                None
            }
            Action::OpenSearch => {
                self.open_search();
                None
            }
            _ => return None,
        };
        Some(then.into_iter().collect())
    }

    /// Closes `layer` when it is one of these (the 3D view's are the app's): what follows from
    /// closing it — a prompt closed is its answer, "no path"; the question before changes are
    /// lost closed is "Cancel" (answered with it still open); the dialog closed is its Cancel.
    pub fn close(&mut self, layer: Layer) -> Option<Action> {
        match layer {
            Layer::Ask => match self.take_ask() {
                Some(Ask::Path(p)) => Some(Action::File(FileAction::Chosen(p.purpose, None))),
                Some(Ask::Save(then)) => {
                    self.set_ask(Ask::Save(then));
                    Some(Action::File(FileAction::Confirm(Answer::Cancel)))
                }
                Some(Ask::Export(_)) | None => None,
            },
            Layer::Template | Layer::Search => {
                self.close_overlay(layer);
                None
            }
            Layer::Dialog => Some(Action::DialogCancel),
            Layer::Settings => {
                self.page.close_settings();
                None
            }
            Layer::Pen | Layer::Box | Layer::Tool | Layer::Selection => None,
        }
    }

    /// Closes the overlay when it is `layer`'s.
    fn close_overlay(&mut self, layer: Layer) {
        if self.overlay.as_ref().is_some_and(|o| o.layer() == layer) {
            self.overlay = None;
        }
    }
}

impl EmcraftApp {
    /// Draws what is open over the window: `draw` edits its drafts (a path typed, a card's
    /// fields) with the rest of the app to read; what is open stays as it was (closing it is an
    /// action).
    pub(crate) fn lend_overlay<R>(&mut self, draw: impl FnOnce(&Self, &mut Overlay) -> R) -> Option<R> {
        let mut o = self.ui_state.overlay.take()?;
        let r = draw(self, &mut o);
        self.ui_state.overlay = Some(o);
        Some(r)
    }

    /// Draws the open dialog (or where it would be): `draw` edits its fields with the rest of
    /// the app to read.
    pub(crate) fn lend_dialog<R>(&mut self, draw: impl FnOnce(&Self, Option<&mut Dialog>) -> R) -> R {
        let mut d = self.ui_state.dialog.take();
        let r = draw(self, d.as_mut());
        self.ui_state.dialog = d;
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::{Prompt, Purpose, Then};
    use crate::page::Tab;

    fn asking() -> UiState {
        let mut s = UiState::default();
        s.set_ask(Ask::Save(Then::New));
        s
    }

    /// Escape takes the top layer, a question over everything, and a field outside it keeps
    /// the key from the layers below a question or the search.
    #[rstest::rstest]
    #[case::question_first(asking(), &[Layer::Tool], Keyboard::Elsewhere, Some(Layer::Ask))]
    #[case::the_settings_over_the_view(UiState::at(Pages::at(Tab::Settings)), &[Layer::Selection], Keyboard::Free, Some(Layer::Settings))]
    #[case::a_field_elsewhere(UiState::at(Pages::at(Tab::Settings)), &[], Keyboard::Elsewhere, None)]
    #[case::the_view_below(UiState::default(), &[Layer::Box, Layer::Tool], Keyboard::Free, Some(Layer::Box))]
    fn escape_reaches_the_top_layer(#[case] s: UiState, #[case] below: &[Layer], #[case] k: Keyboard, #[case] want: Option<Layer>) {
        assert_eq!(s.escape_target(below, k), want);
    }

    /// One overlay at a time: a question closes the search; a card or the search does not open
    /// over a question; closing a prompt answers it with no path, the question before changes
    /// are lost with Cancel (it stays open for that answer).
    #[test]
    fn one_overlay_at_a_time_and_each_closes_its_own_way() {
        let mut s = UiState::default();
        assert!(s.open_search());
        s.set_ask(Ask::Path(Prompt { purpose: Purpose::Open, text: String::new() }));
        assert!(!s.searching() && !s.open_search());
        assert_eq!(s.close(Layer::Ask), Some(Action::File(FileAction::Chosen(Purpose::Open, None))));
        assert!(s.overlay().is_none());
        let mut s = asking();
        assert_eq!(s.close(Layer::Ask), Some(Action::File(FileAction::Confirm(Answer::Cancel))));
        assert!(s.ask().is_some());
        // Closing a layer that is not open changes nothing.
        let mut s = UiState::default();
        assert!(s.open_search());
        assert_eq!(s.update(&Action::Template(None)), Some(vec![]));
        assert!(s.searching());
    }

    /// No egui in the window's state: its transitions are plain data.
    #[test]
    fn the_windows_state_does_not_draw() {
        let src = include_str!("ui_state.rs");
        let word = ["eg", "ui"].concat();
        let path = format!("{word}::");
        // The crate's own name (`emcraft_ui_egui::`) is not the drawing library.
        let drawing = src.match_indices(&path).any(|(i, _)| !src[..i].ends_with('_'));
        assert!(!drawing, "ui_state.rs reaches for the drawing");
    }
}

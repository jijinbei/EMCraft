//! Escape on the iced window, as the egui window's: given to the top layer open
//! ([`emcraft_ui_core::layer::Layer`], top first) — a question, the template's card, the command
//! search, the dialog, the settings, the 3D view's tool (what it picked first, then the tool), the
//! selection — unless a field it does not own has the keyboard (Escape then only lets go of the
//! field). A grip being dragged lets go first.

use emcraft_ui_core::files::{Answer, FileAction};
use emcraft_ui_core::layer::Layer;

use crate::app::{App, Message};

/// The layers open now, top first.
pub fn stack(app: &App) -> Vec<Layer> {
    let open = |l: Layer| match l {
        Layer::Ask => app.asking.is_some(),
        Layer::Template => app.template.card.is_some(),
        Layer::Search => !app.search.is_empty(),
        Layer::Dialog => app.dialog.is_some(),
        Layer::Settings => app.settings,
        Layer::Tool => app.tool.is_some(),
        Layer::Selection => !app.selection.is_clear(),
        Layer::Reports => app.crashes.shown.is_some(),
        // The pen and the box being dragged are not the iced window's (the box is the 3D view's
        // own, let go there).
        Layer::Pen | Layer::Box => false,
    };
    Layer::ALL.into_iter().filter(|l| open(*l)).collect()
}

/// The layer Escape closes: the top one, unless a field took the key (`captured`) and the top
/// layer is not one whose own fields it holds (a question, the search, a card or a dialog over
/// the window: what is under them takes no keys).
pub fn target(app: &App, captured: bool) -> Option<Layer> {
    let top = *stack(app).first()?;
    let owned = matches!(top, Layer::Ask | Layer::Search | Layer::Template | Layer::Reports | Layer::Dialog);
    (!captured || owned).then_some(top)
}

/// What closes layer `l` (the tool and the selection are the window's own, closed there).
pub fn close(l: Layer) -> Option<Message> {
    match l {
        Layer::Ask => Some(Message::File(FileAction::Confirm(Answer::Cancel))),
        Layer::Template => Some(Message::Template(crate::template_dialog::TemplateAct::Open(None))),
        Layer::Search => Some(Message::Search(String::new())),
        Layer::Dialog => Some(Message::Dialog(crate::dialog::Edit::Cancel)),
        Layer::Settings => Some(Message::Settings(false)),
        Layer::Reports => Some(Message::Reports(crate::record::ReportAct::Show(false))),
        Layer::Tool | Layer::Selection | Layer::Pen | Layer::Box => None,
    }
}

//! Importing a part in the iced window, as the egui window's: a SolidCraft design, 3MF or STL
//! (and a KiCad board, through the PCB's own import) chosen with the system's file dialog or
//! dropped on the window opens `geometry.import`'s dialog on it, the geometry's step open; the
//! dialog shows above its fields what can be imported (the format the file is lit), Choose
//! file…, what the file holds (`geometry.import.inspect`: its bodies, size and units, a design's
//! parameters each editable into the dialog's `parameters`) and in a line what importing makes of
//! it.

use emcraft_ui_core::dialogs::Dialog;
use emcraft_ui_core::lang::{args, tr, trf};
use emcraft_ui_core::replies::ImportProbe;
use iced::widget::{Column, button, column, container, row, text, text_input};
use iced::{Alignment, Element, Fill, Widget};
use serde_json::{Map, Value, json};

use crate::app::{App, Message};
use crate::look;

/// The command the import dialog is for.
pub const ID: &str = "geometry.import";

/// The fields above the options: shown whether or not they are unfolded.
const ABOVE: &[&str] = &["path", "parameters"];

/// What a field of the dialog says, trimmed.
fn text_of<'d>(d: &'d Dialog, name: &str) -> &'d str {
    d.fields.iter().find(|f| f.name == name).map_or("", |f| f.text.trim())
}

/// What the probe of the dialog's file is keyed by: the path and the STL unit.
fn key(d: &Dialog) -> String {
    format!("{}\u{1f}{}", text_of(d, "path"), text_of(d, "unit"))
}

impl App {
    /// Part `path` imported: a KiCad board through the PCB's import; any other part through
    /// `geometry.import`'s dialog on it (the one open takes the path), the geometry's step open.
    pub(crate) fn import_part(&mut self, path: &str) {
        let ext = std::path::Path::new(path).extension().and_then(|s| s.to_str()).map(str::to_ascii_lowercase);
        let (id, values) = if ext.as_deref() == Some("kicad_pcb") { ("pcb.import", json!({"path": path})) } else { (ID, json!({"path": path})) };
        if let Some(d) = self.dialog.as_mut().filter(|d| d.command.id == id) {
            let _ = d.set("path", &json!(path));
            return;
        }
        if id == ID {
            self.open_step(Some(emcraft_engine::workflow::Step::Geometry));
        } else {
            self.page = emcraft_ui_core::view::View::Pcb;
        }
        match self.open(id, &values) {
            Ok(d) => self.dialog = Some(d),
            Err(e) => self.error = Some(e),
        }
    }

    /// What the open import dialog's file holds, read again when the file (or its unit)
    /// changed; none without the dialog or a file.
    pub(crate) fn probe_import(&mut self) {
        let Some(d) = self.dialog.as_ref().filter(|d| d.command.id == ID && !text_of(d, "path").is_empty()) else {
            self.replies.import = None;
            return;
        };
        let k = key(d);
        if self.replies.import.as_ref().is_some_and(|p| p.key == k) {
            return;
        }
        let mut params = json!({"path": text_of(d, "path")});
        if let Ok(u) = text_of(d, "unit").parse::<f64>() {
            params["unit"] = json!(u);
        }
        let reply = self
            .session
            .execute("geometry.import.inspect", &params)
            .map_err(|e| e.to_string())
            .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()));
        self.replies.import = Some(ImportProbe { key: k, reply });
    }

    /// A design's parameter typed: written into the dialog's `parameters` (name → number, or an
    /// expression the design reads; empty or as saved: left out).
    pub(crate) fn import_parameter(&mut self, name: String, value: String, saved: String) {
        let Some(field) = self.dialog.as_mut().and_then(|d| d.field_mut("parameters")) else { return };
        let mut set: Map<String, Value> =
            serde_json::from_str(field.text.trim()).ok().and_then(|v: Value| v.as_object().cloned()).unwrap_or_default();
        let v = value.trim();
        if v.is_empty() || v == saved {
            set.remove(&name);
        } else if let Ok(x) = emcraft_ui_core::dialogs::parse_number(v) {
            set.insert(name, json!(x));
        } else {
            set.insert(name, json!(v));
        }
        field.text = if set.is_empty() { String::new() } else { Value::Object(set).to_string() };
    }
}

/// The import dialog's handles: `dialog:pick` (Choose file…), `dialog:options`, `dialog:help`.
pub fn handles(app: &App) -> Vec<(String, Message)> {
    use crate::dialog::Edit;
    let Some(_) = app.dialog.as_ref().filter(|d| d.command.id == ID) else { return Vec::new() };
    [
        ("dialog:pick", Message::File(emcraft_ui_core::files::FileAction::PickPart)),
        ("dialog:options", Message::Dialog(Edit::Options)),
        ("dialog:help", Message::Dialog(Edit::Help)),
    ]
    .map(|(h, m)| (h.to_owned(), m))
    .into()
}

/// What the import dialog's file holds, as `ui.inspect` shows it: the probe's reply (format,
/// units, size, bodies, parameters) or `{"error": why}`; null without one.
pub fn inspect(app: &App, d: &Dialog) -> Value {
    match app.replies.import.as_ref().filter(|p| d.command.id == ID && p.key == key(d)).map(|p| &p.reply) {
        Some(Ok(r)) => serde_json::to_value(r).unwrap_or(Value::Null),
        Some(Err(e)) => json!({"error": e}),
        None => Value::Null,
    }
}

/// Design parameter `name`'s value as saved, as its row shows it.
pub fn saved(app: &App, name: &str) -> Option<String> {
    let r = app.replies.import.as_ref()?.reply.as_ref().ok()?;
    r.parameters.iter().find(|q| q.name == name)?.value.map(|x| emcraft_ui_core::dialogs::value_text(&json!(x)))
}

/// The import dialog's head: the formats (the file's lit), Choose file…, what the file holds and
/// what importing makes of it.
pub fn head<'a>(app: &'a App, d: &'a Dialog) -> Option<Element<'a, Message>> {
    if d.command.id != ID {
        return None;
    }
    let t = app.look.tokens;
    let probe = app.replies.import.as_ref().filter(|p| p.key == key(d));
    let reply = probe.and_then(|p| p.reply.as_ref().ok());
    let format = reply.map(|r| r.format.as_str());
    let chips = [
        ("solidcraft", emcraft_ui_core::glyph::Glyph::Feature, "SolidCraft", t!("parametric")),
        ("3mf", emcraft_ui_core::glyph::Glyph::Box, "3MF", t!("face tags kept")),
        ("stl", emcraft_ui_core::glyph::Glyph::Mesh, "STL", t!("one body")),
    ]
    .map(|(id, glyph, name, what)| {
        let lit = format == Some(id);
        let face = row![
            crate::icons::view(glyph, 22.0, t.text, t.accent),
            column![text(name).size(t.font_body), text(tr(what)).size(t.font_small).color(t.text_dim)]
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        container(face).padding(6).width(Fill).style(look::card(t, lit)).boxed()
    });
    let pick = button(
        row![crate::icons::view(emcraft_ui_core::glyph::Glyph::OpenFile, 18.0, t.icon, t.accent), text(tr(t!("Choose file…"))).size(t.font_button)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .style(look::button(t, text_of(d, "path").is_empty()))
    .on_press_maybe(app.job.is_none().then_some(Message::File(emcraft_ui_core::files::FileAction::PickPart)));
    let dropped = text(tr(t!("or drop it on the window"))).size(t.font_small).color(t.text_dim);
    let detected = match probe.map(|p| &p.reply) {
        Some(Ok(r)) => Some(detected(app, r)),
        Some(Err(e)) => Some(text(e.clone()).size(t.font_small).color(t.error).boxed()),
        None => None,
    };
    let parts = [row(chips).spacing(6).boxed(), row![pick, dropped].spacing(10).align_y(Alignment::Center).boxed()]
        .into_iter()
        .chain(detected)
        .chain(std::iter::once(outcome(app, d)));
    Some(Column::with_children(parts).spacing(8).boxed())
}

/// Which of dialog `d`'s fields its list shows: all, but for the import the path (and the
/// parameters, unless a design's are edited above) with the rest under Options.
pub fn shown<'d>(app: &App, d: &'d Dialog) -> impl Fn(&str) -> bool + 'd {
    let import = d.command.id == ID;
    let design =
        app.replies.import.as_ref().filter(|p| p.key == key(d)).and_then(|p| p.reply.as_ref().ok()).is_some_and(|r| r.format == "solidcraft");
    move |n: &str| !import || if ABOVE.contains(&n) { n == "path" || !design } else { d.options }
}

/// The import's Options: a ▸ that unfolds its other fields, ▾ that folds them; and ? beside it
/// that shows the command's description.
pub fn fold<'a>(app: &'a App, d: &Dialog) -> Option<Element<'a, Message>> {
    let t = app.look.tokens;
    (d.command.id == ID).then(|| {
        let mark = if d.options { "▾" } else { "▸" };
        let options = button(text(format!("{mark} {}", tr(t!("Options")))).size(t.font_body))
            .style(look::quiet(t))
            .on_press(Message::Dialog(crate::dialog::Edit::Options));
        let help = button(text("?").size(t.font_button)).style(look::button(t, d.help)).on_press(Message::Dialog(crate::dialog::Edit::Help));
        row![options, iced::widget::Space::new().width(Fill), help].align_y(Alignment::Center).boxed()
    })
}

/// The command's whole description, while ? is on.
pub fn help<'a>(app: &'a App, d: &Dialog) -> Option<Element<'a, Message>> {
    let t = app.look.tokens;
    (d.command.id == ID && d.help).then(|| text(emcraft_ui_core::lang::command_doc(ID).to_owned()).size(t.font_small).color(t.text_dim).boxed())
}

/// What the file holds: its bodies, size and units, and a design's parameters as fields.
fn detected<'a>(app: &'a App, r: &'a emcraft_engine::ImportInspection) -> Element<'a, Message> {
    let t = app.look.tokens;
    let l = app.session.display().length.symbol();
    let names: Vec<&str> = r.bodies.iter().map(|b| b.name.as_str()).collect();
    let more = if names.len() > 6 { "…" } else { "" };
    let count = if names.len() == 1 { tr(t!("1 body")).to_owned() } else { trf(t!("{n} bodies"), args!(n = names.len())) };
    let first = row![
        text(count).size(t.font_body),
        text(format!("{}{more}", names.iter().take(6).copied().collect::<Vec<_>>().join(", "))).size(t.font_small)
    ]
    .spacing(8);
    let size = r.size.map(|size| {
        let v: Vec<String> = size.iter().map(|x| emcraft_ui_core::dialogs::value_text(&json!((x * 1e6).round() / 1e6))).collect();
        let units = if r.units == "display" { tr(t!("in display units (STL has none)")) } else { tr(t!("units from the file")) };
        text(format!("{} {l} · {units}", v.join(" × "))).size(t.font_small).color(t.text_dim).boxed()
    });
    let set: Map<String, Value> = app
        .dialog
        .as_ref()
        .and_then(|d| d.fields.iter().find(|f| f.name == "parameters"))
        .and_then(|f| serde_json::from_str::<Value>(f.text.trim()).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    let params = r.parameters.iter().take(64).map(|q| {
        let saved = q.value.map(|x| emcraft_ui_core::dialogs::value_text(&json!(x))).unwrap_or_default();
        let now = set.get(&q.name).map_or_else(|| saved.clone(), emcraft_ui_core::dialogs::value_text);
        let (name, saved_too) = (q.name.clone(), saved.clone());
        let input = text_input("", now)
            .size(t.font_body)
            .width(110)
            .padding([3, 6])
            .style(look::field(t))
            .on_input(move |s| Message::ImportParameter(name.clone(), s, saved_too.clone()));
        row![text(q.name.clone()).size(t.font_small).width(140), input, text(q.unit.clone()).size(t.font_small).color(t.text_dim)]
            .spacing(6)
            .align_y(Alignment::Center)
            .boxed()
    });
    let heading = (!r.parameters.is_empty()).then(|| text(tr(t!("Parameters"))).size(t.font_small).color(t.text_dim).boxed());
    let body = Column::with_children([first.boxed()].into_iter().chain(size).chain(heading).chain(params)).spacing(4);
    container(body).padding(8).width(Fill).style(look::ground(t)).boxed()
}

/// What importing makes of the file, in a line, by the region chosen.
fn outcome<'a>(app: &'a App, d: &Dialog) -> Element<'a, Message> {
    let t = app.look.tokens;
    let open = d.fields.iter().any(|f| f.name == "open" && f.on);
    let line = match text_of(d, "region") {
        "cavity" => t!("Metal bodies become walls; the air they enclose is the region"),
        "box" | "" if open => t!("Metal bodies in a box of air lined by a PML"),
        "box" => t!("Metal bodies in a box of air: the air is the region"),
        "" => t!("Bodies are metal: the air they enclose is the region, else a box of air round them"),
        _ => t!("The named body is the region; the others metal in it"),
    };
    text(tr(line)).size(t.font_small).color(t.text).boxed()
}

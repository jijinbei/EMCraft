//! The Geometry step's parameters: the project's variables (a template's dimensions, a count
//! such as the iris filter's irises), each a field in the order the project defines them. Enter
//! changes one (`project.variable`), which rebuilds the model.

use super::*;

/// The variables as the history defines them, in order: name, value (a number or an
/// expression), unit.
fn defined(app: &EmcraftApp) -> Vec<(String, String, &'static str)> {
    let d = app.session.display();
    let mut out: Vec<(String, String, &'static str)> = Vec::new();
    for step in app.session.history().iter().filter(|s| s.command == "project.variable") {
        let Some(name) = step.param("name").and_then(Value::as_str) else { continue };
        let value = match step.param("value") {
            Some(Value::String(e)) => e.clone(),
            Some(v) => v.as_f64().map_or_else(String::new, fmt),
            None => String::new(),
        };
        let unit = match step.param("quantity").and_then(Value::as_str) {
            Some("length") => d.length.symbol(),
            Some("frequency") => d.frequency.symbol(),
            _ => "",
        };
        match out.iter_mut().find(|(n, _, _)| n == name) {
            Some(row) => *row = (name.to_owned(), value, unit),
            None => out.push((name.to_owned(), value, unit)),
        }
    }
    out
}

pub(super) fn parameters(app: &EmcraftApp, f: &mut Fields, ui: &mut egui::Ui, t: &Tokens, out: &mut Actions) {
    let rows = defined(app);
    if rows.is_empty() {
        return;
    }
    ui_kit::section(ui, "Parameters");
    let busy = app.busy();
    for (name, value, unit) in rows {
        // The field follows the project's value until it is edited.
        let (draft, seen) = f.parameters.entry(name.clone()).or_insert_with(|| (value.clone(), value.clone()));
        if *seen != value {
            (*draft, *seen) = (value.clone(), value.clone());
        }
        let enter = ui_kit::row(ui, Raw(&name), false, |ui| {
            let edit = egui::TextEdit::singleline(draft).desired_width((ui.available_width() - 44.0).max(60.0)).char_limit(64);
            let r = ui.add_enabled_ui(!busy, |ui| ui.named(edit, Handle::Parameter(&name))).inner;
            if !unit.is_empty() {
                ui_kit::small(ui, Raw(unit), t.text_dim);
            }
            r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
        });
        if enter && *draft != value {
            let text = draft.trim();
            // A number as a number; anything else goes as it is (an expression, or an error the
            // engine names).
            let given = text.parse::<f64>().ok().filter(|v| v.is_finite()).map_or_else(|| json!(text), |v| json!(v));
            out.push(Action::run("project.variable", json!({"name": name, "value": given})));
        }
    }
    ui.add_space(t.space_8);
}

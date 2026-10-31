//! Step 0, Intent: the kind of analysis (four cards, `project.analysis`) and the frequency band.

use super::*;
use crate::Hint;
use crate::glyphs::{self, Glyph};

/// Step 0's panel; `compact`: a dialog shares it, so the cards keep only their glyphs.
pub(super) fn intent(app: &EmcraftApp, f: &mut Fields, ui: &mut egui::Ui, fu: &str, compact: bool, t: &Tokens, out: &mut Actions) {
    ui_kit::section(ui, "Analysis");
    kinds(app, ui, compact, t, out);
    ui_kit::section(ui, "Frequency band");
    ui_kit::caption(ui, "Set once, early: it sizes the mesh (elements per wavelength at its top), the sweep and the port-mode preview.");
    let (e0, e1) = (f.error("start").map(str::to_owned), f.error("stop").map(str::to_owned));
    let enter = field(ui, "start", &mut f.start, fu, e0.as_deref(), Fixed::IntentStart)
        | field(ui, "stop", &mut f.stop, fu, e1.as_deref(), Fixed::IntentStop);
    let busy = app.busy();
    ui.act(!busy, ui_kit::styled("Apply", true), Fixed::IntentApply.into(), "", || Action::ApplyBand, out);
    if enter && !busy {
        out.push(Action::ApplyBand);
    }
    let d = app.session.display();
    kv(ui, "units", format!("{}, {}", d.length.symbol(), d.frequency.symbol()), t);
}

/// A kind of analysis on its card: its glyph, its short name and what it does (on hover).
pub fn kind_card(kind: Solver) -> KindCard {
    let (glyph, name, detail) = match kind {
        Solver::Frequency => (Glyph::KindSweep, "S-parameters", "S-parameters between ports, swept over the band (solve.frequency)"),
        Solver::Eigenmodes => (Glyph::KindResonator, "Eigenmodes", "A closed resonator's modes and Q; nothing drives it (solve.eigenmodes)"),
        Solver::Radiation => {
            (Glyph::KindAntenna, "Radiation", "An antenna or a scatterer in an open boundary: fields and far field (solve.radiation)")
        }
        Solver::TimeDomain => (Glyph::KindPulse, "Time domain", "A pulse over the band: S-parameters by finite integration (solve.time_domain)"),
    };
    KindCard { glyph, name, detail }
}

/// A kind of analysis as its card shows it: its glyph, its name and what it is for (both to be
/// translated).
#[derive(Clone, Copy, Debug)]
pub struct KindCard {
    pub glyph: Glyph,
    pub name: &'static str,
    pub detail: &'static str,
}

/// The four kinds of analysis as cards in a row, the job's own marked (`auto` while it is
/// inferred from the model; `compact`: glyphs only, the names on hover); a click chooses one
/// (`project.analysis`).
fn kinds(app: &EmcraftApp, ui: &mut egui::Ui, compact: bool, t: &Tokens, out: &mut Actions) {
    let m = app.session.model();
    let (current, chosen) = (m.analysis(), m.analysis);
    let busy = app.busy();
    let gap = 6.0;
    let w = ((ui.available_width() - 3.0 * gap) / 4.0).floor().max(56.0);
    let h = if compact { 32.0 } else { 62.0 };
    for row in Solver::ALL.chunks(4) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for &kind in row {
                let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
                let resp = ui.hit(r, Handle::Kind(kind), if busy { Sense::hover() } else { Sense::click() });
                let KindCard { glyph, name, detail } = kind_card(kind);
                let on = kind == current;
                let p = ui.painter();
                let fill = match (on, resp.hovered() && !busy) {
                    (true, _) => t.accent_soft,
                    (false, true) => t.hover,
                    (false, false) => t.panel_header,
                };
                let stroke = if on { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.border) };
                p.rect(r, t.radius_sm, fill, stroke, egui::StrokeKind::Inside);
                let ink = if on { t.text } else { t.icon };
                let (y, side) = if compact { (r.center().y, 24.0) } else { (r.top() + 23.0, 32.0) };
                glyphs::paint(p, Rect::from_center_size(pos2(r.center().x, y), vec2(side, side)), glyph, ink, t.accent);
                let font = FontId::proportional(t.font_small);
                if !compact {
                    p.text(
                        pos2(r.center().x, r.bottom() - 11.0),
                        egui::Align2::CENTER_CENTER,
                        tr(name),
                        font.clone(),
                        if on { t.text } else { t.text_dim },
                    );
                }
                if on && chosen.is_none() && !compact {
                    p.text(
                        r.right_top() + vec2(-4.0, 3.0),
                        egui::Align2::RIGHT_TOP,
                        tr("auto"),
                        FontId::proportional(t.font_small - 1.5),
                        t.text_dim,
                    );
                }
                let tip = format!("{}\n{}", tr(name), tr(detail));
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).hint(tip);
                if resp.clicked() && !busy && chosen != Some(kind) {
                    out.push(Action::run("project.analysis", json!({"kind": kind.id()})));
                }
            }
        });
    }
    ui.add_space(2.0);
}

/// Checks the band's fields one by one and sets the band (`project.frequency`).
pub fn apply_band(app: &mut EmcraftApp) {
    let f = &mut app.workflow.fields;
    let (a, z) = (positive(&f.start), positive(&f.stop));
    f.set_error("start", a.as_ref().err().cloned());
    f.set_error("stop", z.as_ref().err().cloned());
    let (Ok(a), Ok(z)) = (a, z) else { return };
    if z < a {
        f.set_error("stop", Some(crate::lang::tr("must not be below the start").into()));
        return;
    }
    if let Err(e) = app.run("project.frequency", json!({"start": a, "stop": z})) {
        app.workflow.fields.set_error("stop", Some(e.to_string()));
    }
}

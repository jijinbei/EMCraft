//! The far-field pattern: a cut (θ varying at an azimuth φ, or φ varying at a polar angle θ),
//! its angle, and the field's total or one polarization's share.

use emcraft_engine::view::{Cut, FarComponent, PatternView};

use super::*;
use crate::state::PatternSettings;

pub(super) struct FarField;

impl ChartView for FarField {
    type Key = SharedChart;
    const HANDLE: Fixed = Fixed::ChartPattern;
    const CHART: emcraft_engine::views::ChartId = emcraft_engine::views::ChartId::Pattern;
    const HEIGHT: Height = Height::new(0.9, 260.0, 700.0);

    /// The cut, its angle and the component.
    fn controls(_: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, _out: &mut Actions) -> Option<SharedChart> {
        let p = &mut st.views.pattern;
        ui.horizontal_wrapped(|ui| {
            for (cut, hint) in [(Cut::Phi, "θ varies at an azimuth φ"), (Cut::Theta, "φ varies at a polar angle θ")] {
                let r = ui.named(pill(tr(cut.label()), p.cut == cut, 10, PillSize::Small), Handle::PatternCut(cut.id())).hint(hint);
                if r.clicked() && p.cut != cut {
                    *p = PatternSettings { cut, angle: if cut == Cut::Theta { 90.0 } else { 0.0 }, ..*p };
                }
            }
            let (range, name) = match p.cut {
                Cut::Phi => (0.0..=360.0, "φ (°)"),
                Cut::Theta => (0.0..=180.0, "θ (°)"),
            };
            let mut angle = p.angle;
            let r = crate::ui_kit::slider(ui, &mut angle, range, name);
            scenario::publish_target(Fixed::PatternPhi, r.rect);
            if (angle - p.angle).abs() > 1e-9 {
                p.angle = angle;
            }
            ui.separator();
            for c in FarComponent::ALL {
                let r = ui.named(pill(tr(c.label()), p.component == c, 10, PillSize::Small), Handle::PatternComponent(c.id()));
                if r.hint("the total, or one polarization's share").clicked() {
                    p.component = c;
                }
            }
        });
        Some(SharedChart::Pattern(st.views.pattern))
    }

    fn chart(app: &EmcraftApp, key: &SharedChart) -> Result<emcraft_render::chart::Chart, String> {
        let SharedChart::Pattern(p) = key else { return Err("not a pattern".into()) };
        let view = PatternView { cut: p.cut, angle: p.angle, component: p.component, ..PatternView::default() };
        emcraft_engine::view::pattern_chart_in(&app.session, &view).map_err(|e| e.to_string())
    }
}

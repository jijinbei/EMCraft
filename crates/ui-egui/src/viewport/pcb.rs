//! Guided PCB diagnosis: board and stackup, route, conditions, diagnosis and results.
use super::*;
use crate::lang::{Raw, args};
use crate::ui_kit;
use emcraft_engine::pcb::{Code, Inspection, Stage, near};
use emcraft_model::units::{Time, Unit};
use std::collections::BTreeSet;

fn go(out: &mut Actions, stage: Stage) {
    out.push(Action::run("view.pcb", json!({"stage": stage.id()})));
}

/// The report of `net` under the panel's settings, while they are the ones it was made with.
fn report<'a>(app: &'a EmcraftApp, state: &crate::state::PcbUi, net: &str) -> Option<&'a Inspection> {
    app.replies.pcb.as_ref().filter(|r| {
        r.net == net
            && r.ground == state.ground.trim()
            && Some(r.reference) == state.reference.parse::<f64>().ok()
            && Some(r.stop) == state.stop.parse::<f64>().ok()
            && r.frequency_unit == app.session.display().frequency.symbol()
    })
}

/// The PCB-specific workflow stays above the workspace, as the FEM workflow does.
pub(crate) fn bar(app: &EmcraftApp, ui: &mut egui::Ui, out: &mut Actions) {
    let current = emcraft_engine::pcb::stage(&app.session);
    let loaded = app.session.model().pcb.is_some();
    let net =
        app.session.views().pcb_net.as_deref().filter(|n| app.session.model().pcb.as_ref().is_some_and(|b| b.segments.iter().any(|s| s.net == *n)));
    let ready = net.is_some_and(|n| report(app, &app.views.pcb, n).is_some());
    egui::Panel::top("pcb_steps").show_separator_line(false).frame(egui::Frame::NONE.fill(Tokens::get().viewport_top).inner_margin(6)).show(
        ui,
        |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.selectable_label(app.view == View::Home, tr("Home")).clicked() {
                    out.push(Action::ShowView(View::Home));
                }
                for (i, stage) in Stage::ALL.into_iter().enumerate() {
                    let enabled = match stage {
                        Stage::Board => true,
                        Stage::Net => loaded,
                        Stage::Settings | Stage::Inspect => loaded && net.is_some(),
                        Stage::Results => loaded && ready,
                    };
                    let done = match stage {
                        Stage::Board => loaded,
                        Stage::Net => net.is_some(),
                        _ => ready,
                    };
                    let t = Tokens::get();
                    let label = format!("{} {}", i + 1, shown(stage));
                    let galley =
                        ui.painter().layout_no_wrap(label.clone(), FontId::proportional(t.font_body), if enabled { t.text } else { t.text_dim });
                    let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 46., 30.), Sense::hover());
                    let r = ui.interact(
                        rect,
                        egui::Id::new(("pcb_step", stage.id())),
                        if enabled && !app.busy() { Sense::click() } else { Sense::hover() },
                    );
                    let active = app.view == View::Pcb && stage == current;
                    ui.painter().rect(
                        rect,
                        12.,
                        if active {
                            t.accent_soft
                        } else if r.hovered() {
                            t.hover
                        } else {
                            Color32::TRANSPARENT
                        },
                        Stroke::new(1., if active { t.accent } else { t.border }),
                        egui::StrokeKind::Inside,
                    );
                    crate::steps::mark(
                        ui.painter(),
                        pos2(rect.left() + 14., rect.center().y),
                        if done {
                            crate::steps::Status::Done
                        } else if enabled {
                            crate::steps::Status::Todo
                        } else {
                            crate::steps::Status::Waiting
                        },
                        &t,
                    );
                    ui.painter().galley(pos2(rect.left() + 28., rect.center().y - galley.size().y / 2.), galley, t.text);
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label.clone()));
                    scenario::publish_target(Handle::Choice("pcb_step", stage.id()), rect);
                    if r.clicked() {
                        go(out, stage);
                    }
                }
            });
        },
    );
}

/// The PCB window: the stage's panel on the left, the board in 3D filling the rest with its top
/// view as a minimap in the corner (a click on either picks a route).
pub(super) fn show(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let board = app.session.model().pcb.as_ref();
    let side = if board.is_some() { (area.width() * 0.36).clamp(300., 440.).min(area.width()) } else { area.width() };
    let panel = Rect::from_min_size(area.min, vec2(side, area.height()));
    let mut left = ui.new_child(egui::UiBuilder::new().max_rect(panel));
    egui::ScrollArea::vertical().id_salt("pcb_panel").auto_shrink(false).show(&mut left, |ui| stage_panel(app, st, ui, out));
    let Some(board) = board else { return };
    let view = Rect::from_min_max(pos2(panel.right() + 8., area.top()), area.max);
    if view.width() < 80. {
        return;
    }
    crate::view3d::show(app, &mut st.view3d, &mut st.field, ui, view, crate::scene3d::Overlay::Board, out);
    scenario::publish_target(Handle::Choice("pcb_control", "board3d"), view);
    // What the view offers is the stage's: drawing while the board is prepared, picking a
    // route while one is chosen, the coupling's illustrations once there are results.
    let stage = emcraft_engine::pcb::stage(&app.session);
    if stage != Stage::Board {
        st.views.pcb.drawing = None;
    }
    let illustrated = stage == Stage::Results && super::pcb_coupling::ready(app);
    super::pcb_coupling::frame_view(app, illustrated, &mut st.views.pcb.illustration, &mut st.view3d, view);
    match stage {
        Stage::Board => draw(app, &mut st.views.pcb, &st.view3d, ui, view, board, out),
        Stage::Net => pick_route(app, st, board, out),
        Stage::Results if illustrated => {
            super::pcb_coupling::mode_bar(app, ui, view, out);
            let illustration = &app.session.views().pcb_illustration;
            if illustration.illustrated.is_some() && illustration.playing {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
        Stage::Settings | Stage::Inspect | Stage::Results => {}
    }
    // The top view, small, in the corner.
    let minimap = minimap(view);
    let selected = app.session.views().pcb_net.clone().unwrap_or_default();
    let ground = st.views.pcb.ground.clone();
    let result = report(app, &st.views.pcb, &selected).cloned();
    let mut corner = ui.new_child(egui::UiBuilder::new().max_rect(minimap));
    draw_board(app, &mut st.views.pcb, &mut corner, out, &selected, &ground, result.as_ref(), minimap.height());
}

/// A net clicked in 3D becomes the route.
fn pick_route(app: &EmcraftApp, st: &mut Parts, board: &emcraft_model::pcb::Board, out: &mut Actions) {
    let Some(crate::scene3d::Pick::Net(i)) = st.view3d.single() else { return };
    let nets = crate::scene3d::board::nets(board);
    let Some(net) = nets.get(i).filter(|n| board.segments.iter().any(|s| &s.net == *n)) else { return };
    if app.session.views().pcb_net.as_deref() == Some(net.as_str()) || app.busy() {
        return;
    }
    st.view3d.selected.clear();
    st.views.pcb.attempted = None;
    out.push(Action::run("view.pcb", json!({"net": net, "stage": Stage::Net.id()})));
}

/// The stage's heading, guidance and controls, then the stackup or, with results, the report.
fn stage_panel(app: &EmcraftApp, st: &mut Parts, ui: &mut egui::Ui, out: &mut Actions) {
    let stage = emcraft_engine::pcb::stage(&app.session);
    let state = &mut st.views.pcb;
    let board = app.session.model().pcb.as_ref();
    let selected = app.session.views().pcb_net.as_deref().filter(|n| board.is_some_and(|b| b.segments.iter().any(|s| s.net == *n))).unwrap_or("");
    let result = report(app, state, selected);
    ui.heading(shown(stage));
    ui_kit::caption(
        ui,
        match stage {
            Stage::Board => "1. Import a PCB or open a template, then check its copper layers and substrate thickness.",
            Stage::Net => {
                "2. Click the trace in the board view, or choose its net from the list. Orange is the selected route; green is reference copper."
            }
            Stage::Settings => "3. Set the reference net, source/load impedance and maximum frequency. The stackup below comes from the board.",
            Stage::Inspect => "4. Review the selected route and conditions, then run the diagnosis.",
            Stage::Results => "5. Red markers locate missing reference copper. Review the findings before reading the ideal line estimates.",
        },
    );
    ui.horizontal_wrapped(|ui| {
        if let Some(previous) = match stage {
            Stage::Board => None,
            Stage::Net => Some(Stage::Board),
            Stage::Settings => Some(Stage::Net),
            Stage::Inspect => Some(Stage::Settings),
            Stage::Results => Some(Stage::Inspect),
        } {
            let r = ui.button(tr("Back"));
            scenario::publish_target(Handle::Choice("pcb_control", "back"), r.rect);
            if r.clicked() {
                go(out, previous);
            }
        }
        let next = match stage {
            Stage::Board if board.is_some() => Some(Stage::Net),
            Stage::Net if !selected.is_empty() => Some(Stage::Settings),
            Stage::Settings => Some(Stage::Inspect),
            _ => None,
        };
        if let Some(next) = next {
            let r = ui.button(trf("Next: {step}", args!(step = shown(next))));
            scenario::publish_target(Handle::Choice("pcb_control", "next"), r.rect);
            if r.clicked() {
                go(out, next);
            }
        }
    });
    let Some(board) = board else {
        board_sources(ui, out);
        ui_kit::caption(ui, "Import a PCB or open an example, then click a routed trace.");
        return;
    };
    match stage {
        Stage::Board => {
            board_sources(ui, out);
            if state.drawing.is_some() {
                pen(app, state, ui, board, out);
            }
        }
        Stage::Net => {
            choose_net(ui, board, selected, out);
            // A receiving trace only when there is another route to be one.
            if board.segments.iter().any(|g| g.net != selected) {
                super::pcb_coupling::select(app, ui, out);
            }
        }
        Stage::Settings => {
            for (label, field) in
                [("Reference net", &mut state.ground), ("Reference impedance (Ω)", &mut state.reference), ("Maximum frequency", &mut state.stop)]
            {
                ui.horizontal(|ui| {
                    ui.label(tr(label));
                    let r = ui.add(egui::TextEdit::singleline(field).desired_width(110.));
                    scenario::publish_target(Handle::Choice("pcb_control", label), r.rect);
                });
            }
            ui.small(app.session.display().frequency.symbol());
        }
        Stage::Inspect => {
            ui.label(format!(
                "{} → {}  |  {} Ω  |  {} {}",
                selected,
                state.ground,
                state.reference,
                state.stop,
                app.session.display().frequency.symbol()
            ));
            let r = ui.add_enabled(!app.busy(), egui::Button::new(tr("Inspect route")));
            scenario::publish_target(Handle::Choice("pcb_control", "inspect"), r.rect);
            if r.clicked() {
                out.push(Action::run("pcb.inspect", state.params(selected)));
            }
            fem(app, state, ui, selected, out);
        }
        Stage::Results => {
            match (result, &app.replies.pcb) {
                (Some(result), _) => {
                    let r = ui.button(tr("Keep for comparison"));
                    scenario::publish_target(Handle::Choice("pcb_control", "keep"), r.rect);
                    if r.clicked() {
                        state.baseline = Some(result.clone());
                    }
                }
                (None, Some(_)) => ui_kit::caption(ui, "Settings changed; inspect the route again."),
                (None, None) => {}
            }
            if app.replies.pcb.is_none() && state.attempted.as_ref() != Some(&(app.revision, selected.to_owned())) {
                state.attempted = Some((app.revision, selected.to_owned()));
                out.push(Action::run("pcb.inspect", state.params(selected)));
            }
        }
    }
    if let Some(failure) = app.failure.as_ref().filter(|f| f.command == "pcb.inspect" || f.command == "view.pcb") {
        ui.colored_label(Tokens::get().error, &failure.error);
    }
    ui.add_space(6.);
    match stage {
        Stage::Results => {
            if let Some(r) = result {
                super::pcb_coupling::show(app, state, ui, r, out);
            }
            draw_report(app, state, ui, result)
        }
        _ => stackup(app, ui, selected, &state.ground),
    }
}

/// Where a board comes from: a KiCad file or a template.
fn board_sources(ui: &mut egui::Ui, out: &mut Actions) {
    ui.horizontal_wrapped(|ui| {
        if ui.button(tr("Import PCB…")).clicked() {
            out.push(Action::File(crate::files::FileAction::PickPart));
        }
        ui.menu_button(tr("Templates"), |ui| {
            for template in emcraft_engine::templates::made_for(crate::steps::Solver::Pcb) {
                if ui.button(crate::lang::sample_label(template.name)).clicked() {
                    out.push(Action::Template(Some(template.name.into())));
                    ui.close();
                }
            }
        });
    });
}

/// The routed nets to choose the route from.
fn choose_net(ui: &mut egui::Ui, board: &emcraft_model::pcb::Board, selected: &str, out: &mut Actions) {
    let nets: BTreeSet<_> = board.segments.iter().map(|s| s.net.as_str()).collect();
    let r = egui::ComboBox::from_id_salt("pcb_net").selected_text(if selected.is_empty() { tr("Choose a routed net") } else { selected }).show_ui(
        ui,
        |ui| {
            for net in nets {
                let r = ui.selectable_label(net == selected, net);
                scenario::publish_target(Handle::Choice("pcb_net", net), r.rect);
                if r.clicked() {
                    out.push(Action::run("view.pcb", json!({"net": net, "stage": Stage::Net.id()})));
                }
            }
        },
    );
    scenario::publish_target(Handle::Choice("pcb_control", "net"), r.response.rect);
}

/// What the 3D view draws with (the pen): its net, layer and width, then that net's segments to
/// change or delete.
fn pen(app: &EmcraftApp, state: &mut crate::state::PcbUi, ui: &mut egui::Ui, board: &emcraft_model::pcb::Board, out: &mut Actions) {
    let Some(pen) = state.drawing.as_mut() else { return };
    let l = app.session.display().length;
    ui.add_space(6.);
    ui.heading(tr("Draw the route"));
    ui.horizontal_wrapped(|ui| {
        let nets: BTreeSet<&str> = board.segments.iter().map(|g| g.net.as_str()).chain(board.copper.iter().map(|c| c.net.as_str())).collect();
        egui::ComboBox::from_id_salt("pcb_pen_net").selected_text(pen.net.as_str()).show_ui(ui, |ui| {
            for n in nets {
                if ui.selectable_label(pen.net == n, n).clicked() {
                    pen.net = n.to_owned();
                    pen.from = None;
                }
            }
        });
        egui::ComboBox::from_id_salt("pcb_pen_layer").selected_text(pen.layer.as_str()).show_ui(ui, |ui| {
            for layer in &board.layers {
                if ui.selectable_label(pen.layer == layer.name, &layer.name).clicked() {
                    pen.layer = layer.name.clone();
                    pen.from = None;
                }
            }
        });
        let mut width = l.from_si(pen.width);
        if ui.add(egui::DragValue::new(&mut width).speed(0.01).range(0.001..=10.0).suffix(format!(" {}", l.symbol()))).changed() {
            pen.width = l.to_si(width);
        }
    });
    ui_kit::caption(ui, "Click on the layer in the 3D view: each click adds a segment from the last. Right click or Esc ends the line.");
    let net = pen.net.clone();
    segments(app, state, ui, board, &net, out);
}

/// The grid traces are drawn on (m), and how near (m) a click snaps to a route's end.
const DRAW_GRID: f64 = 0.05e-3;
const DRAW_SNAP: f64 = 0.4e-3;

/// Drawing copper in 3D: a toggle over the view's top right; while on, each click on the pen's
/// layer adds a segment from the last point (the first only sets it), snapped to the pen net's
/// ends there or a 0.05 mm grid; a right click or Escape ends the line, and with none, drawing.
fn draw(
    app: &EmcraftApp,
    state: &mut crate::state::PcbUi,
    v: &crate::view3d::View3d,
    ui: &mut egui::Ui,
    view: Rect,
    board: &emcraft_model::pcb::Board,
    out: &mut Actions,
) {
    let toggle = Rect::from_min_size(pos2(view.right() - 140., view.top() + 8.), vec2(130., 28.));
    let r = ui.new_child(egui::UiBuilder::new().max_rect(toggle)).selectable_label(state.drawing.is_some(), tr("Draw the route"));
    scenario::publish_target(Handle::Choice("pcb_mode", "draw"), r.rect);
    if r.clicked() {
        state.drawing = match state.drawing {
            Some(_) => None,
            None => Some(crate::state::Pen::for_board(board, app.session.views().pcb_net.as_deref())),
        };
    }
    let Some(pen) = state.drawing.as_mut() else { return };
    // Escape or a right click ends the line being drawn, and with none, drawing.
    if ui.input(|i| i.key_pressed(egui::Key::Escape) || i.pointer.secondary_clicked()) {
        match pen.from {
            Some(_) => pen.from = None,
            None => state.drawing = None,
        }
        return;
    }
    // The pen's layer as the view lays it, and where the pointer's ray meets it.
    let frame = crate::scene3d::board::frame(board);
    let Some(layer) = board.layers.iter().find(|l| l.name == pen.layer) else { return };
    let height = frame.point([0.0, 0.0], layer.z, board)[1];
    let on_layer = |pos: egui::Pos2| -> Option<[f64; 2]> {
        let (o, d) =
            v.camera.ray(f64::from(pos.x - view.left()), f64::from(pos.y - view.top()), f64::from(view.width()), f64::from(view.height()))?;
        let t = (height - o[1]) / d[1];
        (d[1].abs() > 1e-12 && t > 0.0).then(|| [o[0] + t * d[0], o[2] + t * d[2]])
    };
    let snapped = |p: [f64; 2]| {
        let ends = board.segments.iter().filter(|g| g.net == pen.net && g.layer == pen.layer).flat_map(|g| [g.start, g.end]);
        match ends.map(|q| (emcraft_model::pcb::distance(p, q), q)).min_by(|a, b| a.0.total_cmp(&b.0)) {
            Some((d, q)) if d <= DRAW_SNAP => q,
            _ => p.map(|c| (c / DRAW_GRID).round() * DRAW_GRID),
        }
    };
    // Not over the controls on the view: this toggle and the minimap.
    let free = |p: &egui::Pos2| view.contains(*p) && !toggle.contains(*p) && !minimap(view).contains(*p);
    let at = ui.input(|i| i.pointer.hover_pos()).filter(free).and_then(on_layer).map(snapped);
    let screen = |p: [f64; 2]| {
        let q = frame.point(p, layer.z, board);
        v.camera.project(q, f64::from(view.width()), f64::from(view.height())).map(|s| view.min + vec2(s[0] as f32, s[1] as f32))
    };
    if let (Some(a), Some(h)) = (pen.from, at)
        && let (Some(sa), Some(sh)) = (screen(a), screen(h))
    {
        let t = Tokens::get();
        let painter = ui.painter().with_clip_rect(view);
        painter.line_segment([sa, sh], Stroke::new(2., t.warning));
        painter.circle_filled(sa, 3., t.warning);
    }
    let (Some(p), true) = (at, ui.input(|i| i.pointer.primary_clicked()) && !app.busy()) else { return };
    if let Some(a) = pen.from.filter(|a| emcraft_model::pcb::distance(*a, p) > 0.0) {
        let l = app.session.display().length;
        let point = |q: [f64; 2]| [l.from_si(q[0]), l.from_si(q[1])];
        out.push(Action::run(
            "pcb.segment",
            json!({"net": pen.net, "layer": pen.layer, "start": point(a), "end": point(p), "width": l.from_si(pen.width)}),
        ));
    }
    pen.from = Some(p);
}

/// Where the minimap sits in the 3D view: its bottom right corner.
fn minimap(view: Rect) -> Rect {
    let size = vec2((view.width() * 0.3).clamp(160., 300.), (view.height() * 0.28).clamp(110., 190.));
    Rect::from_min_size(view.right_bottom() - size - vec2(12., 12.), size)
}

/// The selected route's segments, each with its layer, its width to change and a delete
/// (traces, vias and pours are added from the ribbon).
fn segments(app: &EmcraftApp, state: &mut crate::state::PcbUi, ui: &mut egui::Ui, board: &emcraft_model::pcb::Board, net: &str, out: &mut Actions) {
    let l = app.session.display().length;
    egui::CollapsingHeader::new(tr("Route segments")).id_salt("pcb_segments").default_open(true).show(ui, |ui| {
        for g in board.segments.iter().filter(|g| g.net == net).take(64) {
            ui.horizontal(|ui| {
                ui.small(format!("{} · {}", g.id, g.layer));
                let width = state.widths.entry(g.id.clone()).or_insert_with(|| l.from_si(g.width));
                let r = ui.add(egui::DragValue::new(width).speed(0.01).range(0.001..=10.0).suffix(format!(" {}", l.symbol())));
                scenario::publish_target(Handle::Choice("pcb_width", &g.id), r.rect);
                if (r.drag_stopped() || r.lost_focus()) && (*width - l.from_si(g.width)).abs() > 1e-12 && !app.busy() {
                    out.push(Action::run("pcb.segment", json!({"id": g.id, "width": *width})));
                }
                let d = ui.small_button("×").on_hover_text(tr("Delete"));
                scenario::publish_target(Handle::Choice("pcb_delete", &g.id), d.rect);
                if d.clicked() && !app.busy() {
                    state.widths.remove(&g.id);
                    out.push(Action::run("pcb.segment.delete", json!({"id": g.id})));
                }
            });
        }
    });
}

/// The route through the finite elements: its region, a 50 Ω port at each end, the mesh and a
/// sweep up to the maximum frequency, one after another; their results open in the S-parameter,
/// TDR and field views.
fn fem(app: &EmcraftApp, state: &crate::state::PcbUi, ui: &mut egui::Ui, net: &str, out: &mut Actions) {
    let Some(board) = app.session.model().pcb.as_ref() else { return };
    ui.add_space(8.);
    ui.heading(tr("3D field solution (FEM)"));
    ui_kit::caption(ui, "Solves the route in 3D with finite elements: a 50 Ω port at each end, its S-parameters and fields. It can take minutes.");
    let ends = emcraft_engine::pcb_fem::route_ends(board, net);
    let unit = app.session.display().frequency;
    let stop = state.stop.parse::<f64>().ok().filter(|f| *f > 0.0).map(|f| unit.to_si(f));
    let ready = ends.len() == 2 && stop.is_some();
    let r = ui.add_enabled(ready && !app.busy(), egui::Button::new(tr("Solve with FEM")));
    scenario::publish_target(Handle::Choice("pcb_control", "fem"), r.rect);
    if ends.len() != 2 {
        ui_kit::caption(ui, "The route needs exactly two free ends for its ports.");
    }
    if let (true, Some(stop)) = (r.clicked(), stop) {
        let length = app.session.display().length;
        let size = length.from_si(emcraft_engine::pcb_fem::route_size(board, net, stop));
        let f = unit.from_si(stop);
        let mut list = vec![("pcb.region".to_owned(), json!({"net": net}))];
        for (name, (id, end)) in ["1", "2"].into_iter().zip(&ends) {
            list.push((
                "pcb.port".into(),
                json!({"name": name, "segment": id, "end": end.id(), "impedance": state.reference.parse::<f64>().unwrap_or(50.)}),
            ));
        }
        list.push(("mesh.generate".into(), json!({"size": size})));
        list.push(("solve.frequency".into(), json!({"start": f / 21., "stop": f, "points": 21, "order": 1})));
        out.push(Action::RunAll(list));
    }
}

fn stackup(app: &EmcraftApp, ui: &mut egui::Ui, selected: &str, ground: &str) {
    let Some(board) = app.session.model().pcb.as_ref() else {
        return;
    };
    ui.heading(tr("Layer stackup (section)"));
    ui_kit::caption(ui, "The layout is a top view. Copper depth and dielectric thickness are included in the line estimate.");
    let t = Tokens::get();
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().min(540.), 180.), Sense::hover());
    scenario::publish_target(Handle::Choice("pcb_control", "stackup"), r);
    let painter = ui.painter_at(r);
    painter.rect_filled(r, 4., t.panel);
    let first = board.layers.first().map_or(0., |l| l.z);
    let last = board.layers.last().map_or(1., |l| l.z);
    let y = |z: f64| r.top() + 28. + ((z - first) / (last - first).max(1e-12)) as f32 * (r.height() - 62.);
    for pair in board.layers.windows(2) {
        let a = y(pair[0].z);
        let b = y(pair[1].z);
        painter.rect_filled(Rect::from_min_max(pos2(r.left() + 12., a), pos2(r.right() - 12., b)), 0., t.accent_soft);
        let h = app.session.display().length.from_si(pair[1].z - pair[0].z);
        painter.text(
            pos2(r.center().x, (a + b) / 2.),
            egui::Align2::CENTER_CENTER,
            format!("εr = {}   ·   {:.3} {}", pair[0].eps_below, h, app.session.display().length.symbol()),
            FontId::proportional(12.),
            t.text,
        );
    }
    for layer in &board.layers {
        let chosen = board.segments.iter().any(|s| s.net == selected && s.layer == layer.name);
        let reference = board.copper.iter().any(|c| c.net == ground && c.layer == layer.name);
        let colour = if chosen {
            t.warning
        } else if reference {
            t.success
        } else {
            t.text_dim
        };
        let at = y(layer.z);
        painter.line_segment([pos2(r.left() + 12., at), pos2(r.right() - 12., at)], Stroke::new(3., colour));
        painter.text(pos2(r.left() + 14., at - 7.), egui::Align2::LEFT_BOTTOM, &layer.name, FontId::proportional(12.), t.text);
    }
    ui_kit::caption(ui, "Depth spacing is schematic; labels show actual dimensions.");
    ui_kit::caption(
        ui,
        "The diagnosis is geometry screening and a quasi-static line estimate; Solve with FEM (step 4) solves the route's 3D fields.",
    );
}

/// The board's top view `height` tall: the reference copper filled, the routes as lines, the
/// gaps the report found; a click picks the nearest route.
#[allow(clippy::too_many_arguments)]
fn draw_board(
    app: &EmcraftApp,
    state: &mut crate::state::PcbUi,
    ui: &mut egui::Ui,
    out: &mut Actions,
    net: &str,
    ground: &str,
    report: Option<&Inspection>,
    height: f32,
) {
    let Some(board) = app.session.model().pcb.as_ref() else {
        return;
    };
    let t = Tokens::get();
    let Some((lo, hi)) = board.bounds() else {
        return;
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(100.0), height), Sense::click());
    scenario::publish_target(Handle::Choice("pcb_control", "board"), rect);
    let pad = rect.shrink(10.0);
    let scale = (pad.width() as f64 / (hi[0] - lo[0]).max(1e-9)).min(pad.height() as f64 / (hi[1] - lo[1]).max(1e-9));
    let to = |p: [f64; 2]| {
        pos2(pad.center().x + ((p[0] - (lo[0] + hi[0]) / 2.0) * scale) as f32, pad.center().y + ((p[1] - (lo[1] + hi[1]) / 2.0) * scale) as f32)
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, t.panel);
    // Even-odd filled copper polygons, including holes/bridges, are drawn as scan spans:
    // outlines alone would make a filled plane look like an unfilled zone boundary.
    if state.copper.as_ref().is_none_or(|c| c.revision != app.revision || c.rect != rect || c.ground != ground || c.theme != crate::theme::key()) {
        let mut shapes = Vec::new();
        let mut fill = egui::Mesh::default();
        for copper in board.copper.iter().filter(|c| c.net == ground) {
            let poly: Vec<_> = copper.polygon.iter().map(|p| to(*p)).collect();
            let coords: Vec<_> = poly.iter().map(|p| [p.x as f64, p.y as f64]).collect();
            for row in 0..150 {
                let step = pad.height() / 150.0;
                let y = pad.top() + (row as f32 + 0.5) * step;
                for [a, b] in emcraft_render::pcb::spans(&coords, y as f64) {
                    let ab = [a as f32, b as f32];
                    fill.add_colored_rect(
                        Rect::from_min_max(pos2(ab[0], y - step / 2.0), pos2(ab[1], y + step / 2.0)),
                        t.success.gamma_multiply(0.18),
                    );
                }
            }
            if poly.len() > 1 {
                shapes.push(egui::Shape::closed_line(poly, Stroke::new(1.0, t.success.gamma_multiply(0.6))));
            }
        }
        shapes.insert(0, egui::Shape::mesh(fill));
        state.copper =
            Some(crate::state::PcbCopperCache { revision: app.revision, rect, ground: ground.to_owned(), theme: crate::theme::key(), shapes });
    }
    if let Some(c) = &state.copper {
        painter.extend(c.shapes.iter().cloned());
    }
    let flagged: BTreeSet<&str> =
        report.into_iter().flat_map(|r| &r.findings).filter(|f| f.code.reference()).filter_map(|f| f.segment.as_deref()).collect();
    let mut nearest: Option<(f32, &str)> = None;
    for seg in &board.segments {
        let (a, b) = (to(seg.start), to(seg.end));
        let chosen = seg.net == net;
        let colour = match (chosen, flagged.contains(seg.id.as_str())) {
            (true, true) => t.warning,
            (true, false) => t.warning,
            _ if app.session.views().pcb_illustration.victim.as_deref() == Some(seg.net.as_str()) => t.accent,
            _ => t.text_dim,
        };
        painter.line_segment([a, b], Stroke::new((seg.width * scale) as f32 + if chosen { 2.0 } else { 0.5 }, colour));
        painter.circle_filled(a, 2.0, colour);
        painter.circle_filled(b, 2.0, colour);
        let target = Rect::from_two_pos(a, b).expand(3.0);
        scenario::publish_target(Handle::Choice("pcb_segment", &seg.id), target);
        if let Some(pos) = response.interact_pointer_pos() {
            let (d, _) = near([pos.x as f64, pos.y as f64], [a.x as f64, a.y as f64], [b.x as f64, b.y as f64]);
            if d < 12.0 && nearest.is_none_or(|(old, _)| d < (old as f64)) {
                nearest = Some((d as f32, &seg.net));
            }
        }
    }
    if let Some(report) = report {
        for section in &report.sections {
            if let Some(seg) = board.segments.iter().find(|s| s.net == net && s.id == section.id) {
                for &[a, b] in &section.uncovered {
                    let at = |t: f64| to([seg.start[0] + t * (seg.end[0] - seg.start[0]), seg.start[1] + t * (seg.end[1] - seg.start[1])]);
                    painter.line_segment([at(a), at(b)], Stroke::new(6.0, t.error));
                    painter.circle_stroke(at((a + b) / 2.0), 9.0, Stroke::new(2.0, t.error));
                }
            }
        }
    }
    if let Some(report) = report {
        let mut pairs = report.coupling_pairs.iter().filter(|p| app.session.views().pcb_illustration.victim.as_deref() == Some(p.target.as_str()));
        if let Some(pair) = pairs.nth(state.illustration.index) {
            let length = app.session.display().length;
            for (ends, colour) in [(pair.source_points, t.warning), (pair.target_points, t.accent)] {
                let at = |i: usize| to(ends[i].map(|x| length.to_si(x)));
                painter.line_segment([at(0), at(1)], Stroke::new(5., colour));
                painter.circle_stroke(at(0) + (at(1) - at(0)) * 0.5, 7., Stroke::new(2., colour));
            }
        }
    }
    for via in &board.vias {
        painter.circle_stroke(to(via.at), (via.diameter * scale / 2.0).max(3.0) as f32, Stroke::new(1.4, t.accent));
    }
    if response.clicked()
        && let Some((_, n)) = nearest
    {
        state.attempted = None;
        out.push(Action::run("view.pcb", json!({"net": n, "stage": Stage::Net.id()})));
    }
}

fn draw_report(app: &EmcraftApp, state: &crate::state::PcbUi, ui: &mut egui::Ui, report: Option<&Inspection>) {
    let t = Tokens::get();
    if let Some(crate::Failure { command, error }) = &app.failure
        && command == "pcb.inspect"
    {
        ui.colored_label(t.error, error);
    }
    let Some(report) = report else {
        return;
    };
    ui.label(trf(
        "Selected net: {net}; routed length {length} {unit}",
        args!(net = report.net, length = format!("{:.3}", report.length), unit = report.length_unit),
    ));
    ui.heading(tr("Return-path geometry"));
    let mut shown = BTreeSet::new();
    if report.findings.is_empty() {
        ui_kit::caption(ui, "No issue found by the implemented geometry checks; this is not a compliance pass.");
    }
    for f in &report.findings {
        if !shown.insert((f.code.id(), f.detail.as_str())) {
            continue;
        }
        ui.colored_label(if f.code == Code::NearbySignal { t.warning } else { t.error }, tr(finding_label(f.code)));
        ui.small(f.detail.as_str());
    }
    ui.heading(tr("Ideal transmission-line estimate"));
    ui_kit::caption(
        ui,
        "Lossless quasi-static outer microstrip only. Bends, vias, pads, copper thickness, solder mask, dispersion and coupling are excluded.",
    );
    for row in &report.sections {
        ui.small(format!(
            "{} → {}: {:.2} Ω, {:.1} ps",
            row.id,
            row.reference_layer,
            row.ideal_impedance,
            Time::Picosecond.from_si(row.ideal_delay_s)
        ));
    }
    if let Some(si) = report.si.as_ref().filter(|_| report.model_available) {
        ui.small(trf("Propagation delay: {value} ps", args!(value = format!("{:.1}", Time::Picosecond.from_si(si.delay_s)))));
        ui.small(tr("Magnitude |S| (0 to 1); frequency spans zero to the selected maximum"));
        let along = |f: fn(&emcraft_engine::pcb::LinePoint) -> f64| si.points.iter().map(|p| [p.frequency, f(p)]).collect::<Vec<_>>();
        let top = si.points.last().map_or(1.0, |p| p.frequency);
        let series = [("s11", t.error, along(|p| p.s11.mag)), ("s21", t.accent, along(|p| p.s21.mag))];
        chart(ui, top, &series, false, app.session.display().frequency.symbol());
        ui.small(tr("TDR reflection (ideal line model)"));
        let horizon = 4.0 * si.delay_s.max(si.resolution_s);
        let end = si.tdr.iter().position(|p| p.time_s > horizon).unwrap_or(si.tdr.len()).max(2).min(si.tdr.len());
        let shown = &si.tdr[..end];
        let top = shown.last().map_or(1.0, |p| p.time_s);
        chart(ui, top, &[("reflection", t.warning, shown.iter().map(|p| [p.time_s, p.reflection]).collect())], true, "ns");
        ui.small(trf(
            "TDR window {window} ns; resolution {resolution} ps",
            args!(
                window = format!("{:.2}", Time::Nanosecond.from_si(horizon)),
                resolution = format!("{:.1}", Time::Picosecond.from_si(si.resolution_s))
            ),
        ));
    } else {
        ui_kit::caption(
            ui,
            "No S-parameters or TDR: the route does not meet the model assumptions. Repair the reported geometry or use a detailed field model.",
        );
    }
    if let Some(old) = &state.baseline {
        ui.label(trf("Comparison — geometry findings: {before} → {after}", args!(before = old.findings.len(), after = report.findings.len())));
    }
    for note in &report.import_notes {
        ui_kit::caption(ui, Raw(note.as_str()));
    }
}

fn finding_label(code: Code) -> &'static str {
    match code {
        Code::ReturnGap => "Reference copper missing beneath the route",
        Code::ReferenceEdge => "Return corridor reaches a reference edge or slot",
        Code::NoReference => "No filled reference plane",
        Code::RouteTopology => "Route is branched, disconnected or closed",
        Code::ViaTransition => "Signal via needs a transition model",
        Code::MissingReturnVia => "Verify the return via at this layer transition",
        Code::NearbySignal => "Nearby parallel signal: check coupling",
        Code::UnsupportedCrossSection => "Cross-section outside the supported line model",
    }
}

/// A small chart of `series` (each its name, colour and points `[x, y]`, x from 0 to `xmax`):
/// magnitudes 0 to 1, or with `scalar` reflections −1 to 1 over times (s, shown in ns).
fn chart(ui: &mut egui::Ui, xmax: f64, series: &[(&str, Color32, Vec<[f64; 2]>)], scalar: bool, unit: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 112.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, Tokens::get().panel);
    let xmax = xmax.max(1e-30);
    let pad = Rect::from_min_max(rect.min + vec2(28.0, 8.0), rect.max - vec2(10.0, 20.0));
    let tokens = Tokens::get();
    let font = egui::FontId::proportional(10.0);
    for (fraction, value) in [(0.0, if scalar { "−1" } else { "0" }), (0.5, if scalar { "0" } else { "0.5" }), (1.0, "1")] {
        let y = pad.bottom() - fraction * pad.height();
        p.line_segment([pos2(pad.left(), y), pos2(pad.right(), y)], Stroke::new(0.5, tokens.border));
        p.text(pos2(pad.left() - 5.0, y), egui::Align2::RIGHT_CENTER, value, font.clone(), tokens.text_dim);
    }
    p.text(pos2(pad.left(), pad.bottom() + 5.0), egui::Align2::LEFT_TOP, "0", font.clone(), tokens.text_dim);
    let end = if scalar { Time::Nanosecond.from_si(xmax) } else { xmax };
    p.text(pos2(pad.right(), pad.bottom() + 5.0), egui::Align2::RIGHT_TOP, format!("{end:.2} {unit}"), font, tokens.text_dim);
    for (_, colour, xy) in series {
        let height = |y: f64| if scalar { ((y + 1.0) / 2.0).clamp(0.0, 1.0) } else { y.clamp(0.0, 1.0) } as f32 * pad.height();
        let points: Vec<_> = xy.iter().map(|&[x, y]| pos2(pad.left() + (x / xmax) as f32 * pad.width(), pad.bottom() - height(y))).collect();
        if points.len() > 1 {
            p.add(egui::Shape::line(points, Stroke::new(1.5, *colour)));
        }
    }
    ui.horizontal_wrapped(|ui| {
        for &(name, c, _) in series {
            ui.colored_label(c, name);
        }
    });
}

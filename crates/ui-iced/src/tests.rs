//! The window driven as a person would: its messages (typing, dragging, buttons) each changing
//! the chain as the egui window's do, and the view drawn headless and clicked.

use emcraft_model::system::RECEIVER;
use emcraft_ui_core::geom::Point;
use emcraft_ui_core::system::gesture::Command;
use serde_json::json;

use crate::app::{App, BAND, Field, Message, Pointer};

/// One test at a time on the GPU: the offscreen device's pictures and the simulator's wgpu
/// snapshots made side by side from several threads have crashed the driver.
fn gpu() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The window's settings as it runs: the pictures' face loaded for the charts' labels.
fn settings() -> iced::Settings {
    iced::Settings { fonts: vec![emcraft_render::text::face_bytes().into()], ..iced::Settings::default() }
}

fn field(node: &str, key: &'static str) -> Field {
    Field { node: node.to_owned(), key }
}

/// Types `text` into `f` and lets it go.
fn typed(app: &mut App, f: Field, text: &str) {
    app.update(Message::Typed(f.clone(), text.to_owned()));
    app.update(Message::Commit(f));
}

/// The ADS-B receiver's chain: a field typed reads the chain again; a card dragged moves there;
/// a stage deleted closes the chain over it, a link dragged to the receiver's input links it
/// again; the band typed is confirmed. Analytic evidence: the chain's noise figure is the LNA's
/// to within the later stages' share (Friis: F = F₁ + (F₂ − 1)/G₁ + …), so a 2 dB LNA reads
/// just over 2 dB.
#[test]
fn the_node_editor_edits_drags_and_links_the_chain() {
    let mut app = App::new(Some("adsb-receiver"));
    assert_eq!((app.error.as_deref(), app.unread.as_deref()), (None, None));
    typed(&mut app, field("LNA", "noise_figure"), "2");
    let r = emcraft_ui_core::system::reading(&app.session, "RX").and_then(|(_, nf)| nf).unwrap_or_default();
    assert!((2.0..2.2).contains(&r), "chain NF {r}");
    // A card dragged 50 to the right, from its header.
    let before = app.layout().node("LNA").map(|n| n.rect.left()).unwrap_or_default();
    app.update(Message::Grab("LNA".into()));
    for x in [300.0, 350.0] {
        app.update(Message::Pointer(Pointer::Move(Point::new(x, 100.0))));
    }
    app.update(Message::Pointer(Pointer::Release(Point::new(350.0, 100.0))));
    assert_eq!(app.layout().node("LNA").map(|n| n.rect.left()), Some(before + 50.0));
    // RX deleted; the filter linked to the receiver by a drag from its output.
    app.update(Message::Run(Command { id: "system.stage.delete", params: json!({"stage": "RX"}) }));
    assert_eq!(app.session.model().system.stages.len(), 2);
    app.update(Message::Run(Command { id: "system.unlink", params: json!({"from": "band-pass filter", "to": RECEIVER}) }));
    assert!(!app.session.model().system.order().1, "unlinked short of the receiver");
    let to = app.layout().node(RECEIVER).map(emcraft_ui_core::system::layout::NodeBox::input).unwrap_or_default();
    app.update(Message::StartLink("band-pass filter".into()));
    app.update(Message::Pointer(Pointer::Release(to)));
    assert!(app.session.model().system.order().1, "the filter linked to the receiver");
    // The band's start typed: confirmed, its stop kept.
    let stop = emcraft_ui_core::system::band::shown(&app.session).stop;
    typed(&mut app, field(BAND, "start"), "1");
    let band = emcraft_ui_core::system::band::shown(&app.session);
    assert_eq!((band.start, band.stop), (Some(1.0), stop));
    assert!(matches!(app.session.model().job.setup(), Some(emcraft_model::Setup::System { .. })), "confirmed");
    // Not a number: nothing runs, the model keeps its value.
    typed(&mut app, field("LNA", "gain"), "abc");
    assert_eq!(emcraft_ui_core::system::card::rows(&app.session, &app.layout().nodes[1])[0].value, {
        let v = emcraft_engine::system::stage_params(&app.session, &app.session.model().system.stages[0].part)["gain"].as_f64();
        emcraft_ui_core::system::card::Edit::Number { value: v, unit: "dB".into(), optional: false }
    });
}

/// The view drawn headless: an empty project shows the kinds of stage, and clicking one adds
/// it. With `EMCRAFT_ICED_SHOTS=DIR` the sample's view is written there as a PNG to look at.
#[test]
fn an_empty_chain_offers_its_stages_and_a_click_adds_one() {
    let _gpu = gpu();
    let mut app = App::new(None);
    app.update(Message::Page(emcraft_ui_core::view::View::System));
    let messages: Vec<Message> = {
        let mut ui = iced_test::simulator(app.view());
        assert!(ui.click("+ Amplifier").is_ok(), "the amplifier's button is shown");
        ui.into_messages().collect()
    };
    for m in messages {
        app.update(m);
    }
    assert_eq!(app.session.model().system.stages.len(), 1);
    if let Ok(dir) = std::env::var("EMCRAFT_ICED_SHOTS") {
        let themes = emcraft_ui_core::theme::Themes::load(None);
        for (name, theme, language) in [("system", "light", "en"), ("system-dark", "dark", "en"), ("system-ja-sepia", "sepia", "ja")] {
            emcraft_ui_core::lang::set(language);
            let sample = App::new(Some("adsb-receiver")).dressed(themes.get(theme));
            let mut ui = iced_test::Simulator::with_size(settings(), (1400.0, 1300.0), sample.view());
            let shot = ui.snapshot(&sample.theme()).unwrap_or_else(|e| panic!("{e}"));
            let path = std::path::Path::new(&dir).join(name);
            let _ = std::fs::remove_file(std::path::Path::new(&dir).join(format!("{name}-wgpu.png")));
            assert!(shot.matches_image(&path).unwrap_or(false));
        }
        emcraft_ui_core::lang::set("en");
    }
}

/// Waits for the running job to end (a cap of two minutes), looking at it as the frames would.
fn finish(app: &mut App) {
    let began = std::time::Instant::now();
    while app.job.is_some() && began.elapsed().as_secs() < 120 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        app.update(Message::Tick(std::time::Instant::now()));
    }
    assert!(app.job.is_none(), "the job ended");
}

/// The model page: a sample without a chain opens on it, its scene built and fitted; a drag
/// orbits, the wheel zooms, Fit frames it again, and a view resized keeps a moved camera but
/// fits an unmoved one again.
#[test]
fn the_model_page_shows_the_scene_and_navigates() {
    use crate::view3d::Nav;
    use emcraft_ui_core::view::View;
    let mut app = App::new(Some("highpass"));
    assert_eq!(app.page, View::Model);
    assert!(app.view3d.bounds().is_some(), "the scene has the model's box");
    let fitted = app.view3d.camera;
    app.update(Message::View3d(Nav::Size(300.0, 600.0)));
    assert_ne!(app.view3d.camera, fitted, "an unmoved camera fits the new shape");
    let fitted = app.view3d.camera;
    app.update(Message::View3d(Nav::Orbit(40.0, 0.0)));
    assert_ne!(app.view3d.camera.yaw, fitted.yaw);
    app.update(Message::View3d(Nav::Zoom(2.0)));
    assert!(app.view3d.camera.distance < fitted.distance);
    let moved = app.view3d.camera;
    app.update(Message::View3d(Nav::Size(900.0, 300.0)));
    assert_eq!(app.view3d.camera, moved, "a moved camera stays");
    app.update(Message::View3d(Nav::Size(300.0, 600.0)));
    app.update(Message::View3d(Nav::Fit));
    assert_eq!(app.view3d.camera.distance, fitted.distance, "fitted again as at first");
    // Another overlay builds the scene again; the theme's colours change it too.
    let packed = app.view3d.shown.as_ref().map(|s| s.packed.id);
    app.update(Message::View3d(Nav::Overlay(emcraft_ui_core::scene3d::Overlay::Materials)));
    assert_ne!(app.view3d.shown.as_ref().map(|s| s.packed.id), packed);
}

/// Run from the model page's bar: the confirmed job solves on the worker while the window keeps
/// its session, then its session comes back with the sweep and the S-parameters page shows it.
/// Cancelled, the project stays as it was. Analytic evidence: the high-pass filter's sweep is the
/// sample's own (its |S21| checked against the exact ladder in the engine's tests); here only
/// that the sweep arrived and how.
#[test]
fn run_solves_on_the_worker_and_shows_the_sweep() {
    use emcraft_ui_core::view::View;
    let mut app = App::new(Some("highpass"));
    app.update(Message::Start("solve.run".into()));
    assert!(app.job.is_some(), "the solve runs on the worker");
    assert!(app.progress().is_some_and(|p| !p.is_empty()));
    app.update(Message::Cancel);
    assert!(app.job.is_none() && app.session.results().sweep.is_none(), "cancelled: nothing changed");
    app.update(Message::Start("solve.run".into()));
    finish(&mut app);
    assert_eq!(app.error, None);
    assert!(app.session.results().sweep.is_some(), "the sweep came back");
    assert_eq!(app.page, View::SParameters);
    assert!(app.charts.sparameters.as_ref().is_some_and(|p| p.shown.is_ok()), "its chart drawn again with the sweep");
    assert!(app.note.as_deref().is_some_and(|n| n.contains(" s")), "its time noted: {:?}", app.note);
}

/// The 3D view's frame drawn offscreen on the process's own device, as the shader widget draws
/// it on the window's: the model's pixels differ from the ground. With `EMCRAFT_ICED_SHOTS=DIR`
/// the picture is written there. A machine with no adapter at all skips it.
#[test]
fn the_3d_frame_draws_the_model() {
    let _gpu = gpu();
    let app = App::new(Some("highpass"));
    let Some(shown) = app.view3d.shown.as_ref() else { panic!("no scene") };
    let frame = crate::view3d::shader::frame(&app.shot(shown), [640, 400], 1.0);
    let canvas = match emcraft_gpu3d::picture(0, &frame) {
        Ok(c) => c,
        Err(emcraft_gpu3d::Gpu3dError::NoAdapter(e)) => return eprintln!("skipped: {e}"),
        Err(e) => panic!("{e}"),
    };
    let corner = &canvas.rgba[..4];
    let differ = canvas.rgba.chunks(4).filter(|p| p != &corner).count();
    assert!(differ > canvas.rgba.len() / 4 / 20, "the model covers some of the view ({differ} pixels)");
    if let Ok(dir) = std::env::var("EMCRAFT_ICED_SHOTS") {
        let png = canvas.png().unwrap_or_default();
        assert!(std::fs::write(std::path::Path::new(&dir).join("model-3d.png"), png).is_ok());
        // The pages around it, solved (the simulator draws no shader: the 3D view is blank).
        let mut app = App::new(Some("highpass"));
        app.update(Message::Start("solve.run".into()));
        finish(&mut app);
        let ports = Message::View3d(crate::view3d::Nav::Overlay(emcraft_ui_core::scene3d::Overlay::Ports));
        let field = Message::View3d(crate::view3d::Nav::Overlay(emcraft_ui_core::scene3d::Overlay::Field));
        let h_arrows = Message::View3d(crate::view3d::Nav::Arrows(true, true));
        let cut = app.view3d.bounds().map(|b| emcraft_ui_core::scene3d::Cut::middle(0, b));
        let section = Message::View3d(crate::view3d::Nav::Section(cut));
        let pick = Message::Click(crate::view3d::Click { at: emcraft_ui_core::geom::Point::new(400.0, 300.0), add: false, double: false });
        for (name, page, then) in [
            ("sparameters", emcraft_ui_core::view::View::SParameters, None),
            ("model", emcraft_ui_core::view::View::Model, None),
            ("model-ports", emcraft_ui_core::view::View::Model, Some(ports)),
            ("model-field", emcraft_ui_core::view::View::Model, Some(field)),
            ("model-arrows", emcraft_ui_core::view::View::Model, Some(h_arrows)),
            ("model-selected", emcraft_ui_core::view::View::Model, Some(pick)),
            ("model-section", emcraft_ui_core::view::View::Model, Some(section)),
        ] {
            app.update(Message::Page(page));
            if let Some(m) = then {
                app.update(m);
            }
            let mut ui = iced_test::Simulator::with_size(settings(), (1400.0, 900.0), app.view());
            let shot = ui.snapshot(&app.theme()).unwrap_or_else(|e| panic!("{e}"));
            let _ = std::fs::remove_file(std::path::Path::new(&dir).join(format!("{name}-wgpu.png")));
            assert!(shot.matches_image(std::path::Path::new(&dir).join(name)).unwrap_or(false));
        }
    }
}

/// A click on the model picks the part under it (the shared selection's rules): its row names it
/// on the card and the scene is built again with it lit; Shift adds, a click on empty space
/// clears, a double click there fits the view again.
#[test]
fn a_click_picks_the_part_under_it() {
    use crate::view3d::{Click, Nav};
    use emcraft_ui_core::geom::Point;
    let mut app = App::new(Some("highpass"));
    app.update(Message::View3d(Nav::Size(800.0, 600.0)));
    let packed = app.view3d.shown.as_ref().map(|s| s.packed.id);
    let click = |at, add, double| Message::Click(Click { at, add, double });
    app.update(click(Point::new(400.0, 300.0), false, false));
    assert_eq!(app.selection.len(), 1, "the part in the middle picked");
    let rows: Vec<String> = app.picked().iter().map(|r| r.shown.clone()).collect();
    assert!(rows.first().is_some_and(|r| !r.is_empty()), "its row names it: {rows:?}");
    // The GPU rings it (the preferences' outline): the scene stays, the frame carries its tag.
    assert_eq!(app.view3d.shown.as_ref().map(|s| s.packed.id), packed, "ringed by the GPU: the scene stays");
    let shot = app.view3d.shown.as_ref().map_or_else(|| panic!("no scene"), |s| app.shot(s));
    assert_eq!(shot.selected, app.selection.iter().map(|p| p.tag()).collect::<Vec<_>>());
    // Without the outline the scene lights it.
    app.gpu_look.outline = false;
    app.update(Message::View3d(Nav::Fit));
    assert_ne!(app.view3d.shown.as_ref().map(|s| s.packed.id), packed, "lit: the scene built again");
    app.gpu_look.outline = true;
    app.update(click(Point::new(2.0, 2.0), true, false));
    assert_eq!(app.selection.len(), 1, "Shift on empty space keeps it");
    app.update(Message::View3d(Nav::Zoom(3.0)));
    let zoomed = app.view3d.camera.distance;
    app.update(click(Point::new(2.0, 2.0), false, true));
    assert!(app.selection.is_clear(), "a double click on empty space clears");
    assert!(app.view3d.camera.distance > zoomed, "and fits the view again");
}

/// The field overlay, once solved: the field view's plane drawn coloured with its colour bar; a
/// component changed plots the same samples again, a plane changed samples anew. Analytic
/// evidence: in the TE10 high-pass guide Ey is the field's only E component, so its colour bar
/// is signed (from −peak to +peak) and |E| unsigned.
#[test]
fn the_field_overlay_draws_the_plane_with_its_colour_bar() {
    use crate::view3d::Nav;
    use emcraft_engine::view::{Component, Plane};
    use emcraft_ui_core::scene3d::Overlay;
    let mut app = App::new(Some("highpass"));
    app.update(Message::Start("solve.run".into()));
    finish(&mut app);
    app.update(Message::View3d(Nav::Overlay(Overlay::Field)));
    assert_eq!(app.view3d.field_error, None);
    let bar = |app: &App| app.view3d.shown.as_ref().and_then(|s| s.built.colour_bar.clone());
    assert!(bar(&app).is_some_and(|b| b.scale.signed), "Ey is signed");
    let samples = |app: &App| app.view3d.sampled.as_ref().map(|s| s.samples().clone());
    let first = samples(&app);
    app.update(Message::View3d(Nav::Component(Component::E)));
    assert!(bar(&app).is_some_and(|b| !b.scale.signed), "|E| is not");
    assert!(first.zip(samples(&app)).is_some_and(|(a, b)| std::sync::Arc::ptr_eq(&a, &b)), "the same samples");
    app.update(Message::View3d(Nav::Plane(Plane::E)));
    let again = samples(&app);
    assert!(again.as_ref().is_some_and(|s| s.plane == Plane::E), "sampled anew on the E-plane");
}

/// The GPU colours the field's plane (the preferences' default): the scene keeps the plane as
/// one quad and its phasors go with the frame, so a phase changes the picture without building
/// the scene again; with the GPU's colouring off the scene holds the plane's coloured cells.
/// Analytic evidence: a standing TE10 wave's Ey at ωt and ωt + π/2 differ (Re(F e^{jωt}) of a
/// phasor with both parts non-zero), so the two pictures do.
#[test]
fn the_gpu_colours_the_field_and_a_phase_moves_only_the_frame() {
    let _gpu = gpu();
    use crate::view3d::Nav;
    use emcraft_ui_core::scene3d::Overlay;
    let mut app = App::new(Some("highpass"));
    app.update(Message::Start("solve.run".into()));
    finish(&mut app);
    app.update(Message::View3d(Nav::Overlay(Overlay::Field)));
    let shown = |app: &App| app.view3d.shown.clone().unwrap_or_else(|| panic!("no scene"));
    let gpu = shown(&app);
    assert!(gpu.field.is_some(), "the GPU colours the plane");
    // The arrows turn on the GPU. TE10's E lies across the H-plane (Ey only), so no E arrow lies
    // in it; its H (Hx, Hz) does.
    let turning = |s: &crate::view3d::Shown| s.built.arrows.iter().filter(|a| a.motion.is_some()).count();
    assert_eq!(turning(&gpu), 0, "E across the H-plane: no arrow in it");
    app.update(Message::View3d(Nav::Arrows(false, true)));
    let h = shown(&app);
    assert!(turning(&h) > 0 && !h.arrow_legend.is_empty(), "H's arrows, with their legend");
    let gpu = h;
    app.update(Message::View3d(Nav::Phase(std::f64::consts::FRAC_PI_2)));
    assert_eq!(shown(&app).packed.id, gpu.packed.id, "a phase builds nothing again");
    let picture = |app: &App| {
        let frame = crate::view3d::shader::frame(&app.shot(&shown(app)), [480, 320], 1.0);
        emcraft_gpu3d::picture(0, &frame)
    };
    let quarter = picture(&app);
    // Played, the phase moves on with the frames' clock (a period in field::PERIOD seconds).
    app.update(Message::View3d(Nav::Phase(0.0)));
    let t0 = std::time::Instant::now();
    app.update(Message::Play(true));
    app.update(Message::Tick(t0));
    app.update(Message::Tick(t0 + std::time::Duration::from_secs_f64(crate::view3d::field::PERIOD / 4.0)));
    assert!((app.view3d.field.phase - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "a quarter period: {}", app.view3d.field.phase);
    assert_eq!(shown(&app).packed.id, gpu.packed.id, "playing builds nothing again");
    app.update(Message::Play(false));
    app.update(Message::View3d(Nav::Phase(0.0)));
    match (picture(&app), quarter) {
        (Ok(a), Ok(b)) => assert_ne!(a.rgba, b.rgba, "the phase moves the picture"),
        (Err(emcraft_gpu3d::Gpu3dError::NoAdapter(e)), _) => eprintln!("skipped the pictures: {e}"),
        (a, b) => panic!("{:?} {:?}", a.err(), b.err()),
    }
    // Off: the CPU colours the cells, and the scene carries them.
    app.gpu_look.field = false;
    app.update(Message::View3d(Nav::Fit));
    let cpu = shown(&app);
    assert!(cpu.field.is_none());
    assert!(cpu.built.scene.tris.len() > gpu.built.scene.tris.len(), "the plane's cells in the scene");
}

/// The section view: the GPU clips the scene at the cut (nothing built again), the other side
/// kept at a click, and a click still picks a part on the side kept. Analytic evidence: the
/// cut through the guide's middle across z leaves the half of the box below it, so the picture
/// changes and the other half differs again.
#[test]
fn the_section_view_clips_on_the_gpu() {
    let _gpu = gpu();
    use crate::view3d::{Click, Nav};
    use emcraft_ui_core::geom::Point;
    use emcraft_ui_core::scene3d::Cut;
    let mut app = App::new(Some("highpass"));
    app.update(Message::View3d(Nav::Size(800.0, 600.0)));
    let packed = app.view3d.shown.as_ref().map(|s| s.packed.id);
    let picture = |app: &App| {
        let shown = app.view3d.shown.clone().unwrap_or_else(|| panic!("no scene"));
        emcraft_gpu3d::picture(0, &crate::view3d::shader::frame(&app.shot(&shown), [400, 300], 1.0))
    };
    let whole = picture(&app);
    let Some(bounds) = app.view3d.bounds() else { panic!("no box") };
    let cut = Cut::middle(2, bounds);
    app.update(Message::View3d(Nav::Section(Some(cut))));
    assert_eq!(app.view3d.shown.as_ref().map(|s| s.packed.id), packed, "clipped by the GPU: the scene stays");
    let low = picture(&app);
    app.update(Message::View3d(Nav::Section(Some(Cut { keep: cut.keep.flipped(), ..cut }))));
    let high = picture(&app);
    match (whole, low, high) {
        (Ok(w), Ok(l), Ok(h)) => assert!(w.rgba != l.rgba && l.rgba != h.rgba, "each side clipped"),
        (Err(emcraft_gpu3d::Gpu3dError::NoAdapter(e)), ..) => eprintln!("skipped the pictures: {e}"),
        (w, l, h) => panic!("{:?} {:?} {:?}", w.err(), l.err(), h.err()),
    }
    app.update(Message::Click(Click { at: Point::new(400.0, 300.0), add: false, double: false }));
    assert_eq!(app.selection.len(), 1, "a part on the side kept picked");
    app.update(Message::View3d(Nav::Section(None)));
    assert!(app.view3d.section.is_none());
}

/// The browser beside the 3D view: a port's row selects the port (lit in the view, named on the
/// card); once solved, the sweep's row shows the S-parameters page.
#[test]
fn a_browser_row_selects_its_part_and_shows_its_page() {
    use emcraft_ui_core::scene3d::Pick;
    use emcraft_ui_core::view::View;
    let mut app = App::new(Some("highpass"));
    let port = app.rows().find(|r| r.pick == Some(Pick::Port(0))).map(|r| r.id.clone());
    let Some(port) = port else { panic!("no row for port 1") };
    app.update(Message::Row(port.clone()));
    assert_eq!(&app.selection[..], [Pick::Port(0)]);
    assert!(app.picked().first().is_some_and(|r| r.id == port));
    app.update(Message::Start("solve.run".into()));
    finish(&mut app);
    app.update(Message::Page(View::Model));
    assert!(app.rows().any(|r| r.id == "result:sweep"), "the sweep's row once solved");
    app.update(Message::Row("result:sweep".into()));
    assert_eq!(app.page, View::SParameters);
}

/// A dialog over the window: a material's row edited opens on its values (εr 2), OK unchanged
/// runs nothing, a value the engine refuses comes back under its field with the dialog open, a
/// good one is set; a port selected and deleted is gone. Analytic evidence: the PTFE-like
/// sections' εr is the sample's own 2; the edit's value is read back from the model.
#[test]
fn a_dialog_edits_an_item_and_delete_takes_one_away() {
    use crate::dialog::Edit;
    let mut app = App::new(Some("highpass"));
    let eps = |app: &App| app.session.model().materials.iter().find(|m| m.name == "ptfe-like").map(|m| m.eps_r.get());
    assert_eq!(eps(&app), Some(2.0));
    app.update(Message::Row("material:ptfe-like".into()));
    app.update(Message::Edit);
    let field = |app: &App| app.dialog.as_ref().and_then(|d| d.fields.iter().position(|f| f.name == "eps_r"));
    let Some(k) = field(&app) else { panic!("the material's dialog with its εr") };
    assert_eq!(app.dialog.as_ref().map(|d| d.fields[k].text.clone()), Some("2".into()));
    let revision = app.session.revision();
    app.update(Message::Dialog(Edit::Ok));
    assert!(app.dialog.is_none() && app.session.revision() == revision, "unchanged: nothing ran");
    app.update(Message::Edit);
    app.update(Message::Dialog(Edit::Text(k, "-3".into())));
    app.update(Message::Dialog(Edit::Ok));
    let refused = app.dialog.as_ref().map(|d| d.fields[k].error.is_some() || d.error.is_some());
    assert_eq!(refused, Some(true), "refused, the dialog kept with why");
    app.update(Message::Dialog(Edit::Text(k, "2.5".into())));
    app.update(Message::Dialog(Edit::Ok));
    assert!(app.dialog.is_none());
    assert_eq!(eps(&app), Some(2.5));
    // A port deleted from the card.
    let ports = app.session.model().ports.len();
    let port = app.rows().find(|r| r.pick == Some(emcraft_ui_core::scene3d::Pick::Port(1))).map(|r| r.id.clone()).unwrap_or_default();
    app.update(Message::Row(port));
    app.update(Message::Delete);
    assert_eq!(app.session.model().ports.len(), ports - 1);
    assert!(app.selection.is_clear());
    if let Ok(dir) = std::env::var("EMCRAFT_ICED_SHOTS") {
        let _gpu = gpu();
        app.update(Message::Row("material:ptfe-like".into()));
        app.update(Message::Edit);
        let mut ui = iced_test::Simulator::with_size(settings(), (1400.0, 900.0), app.view());
        let shot = ui.snapshot(&app.theme()).unwrap_or_else(|e| panic!("{e}"));
        let _ = std::fs::remove_file(std::path::Path::new(&dir).join("dialog-wgpu.png"));
        assert!(shot.matches_image(std::path::Path::new(&dir).join("dialog")).unwrap_or(false));
    }
}

/// The ribbon: Materials' "new material" opens its dialog; filled in and confirmed, the material
/// is in the model. A system's ribbon opens on its own tab.
#[test]
fn the_ribbon_opens_a_commands_dialog_and_runs_it() {
    use crate::dialog::Edit;
    use emcraft_ui_core::ribbon::RibbonTab;
    let mut app = App::new(Some("highpass"));
    assert_eq!(app.tab, RibbonTab::Model);
    app.update(Message::Tab(RibbonTab::Materials));
    app.update(Message::Command("model.material.create".into()));
    let at = |app: &App, name: &str| app.dialog.as_ref().and_then(|d| d.fields.iter().position(|f| f.name == name));
    let (Some(name), Some(eps)) = (at(&app, "name"), at(&app, "eps_r")) else { panic!("the material's dialog") };
    app.update(Message::Dialog(Edit::Text(name, "alumina".into())));
    app.update(Message::Dialog(Edit::Text(eps, "9.8".into())));
    app.update(Message::Dialog(Edit::Ok));
    assert!(app.dialog.is_none(), "{:?}", app.dialog.as_ref().map(|d| d.error.clone()));
    assert!(app.session.model().materials.iter().any(|m| m.name == "alumina" && m.eps_r.get() == 9.8));
    assert!(app.rows().any(|r| r.id == "material:alumina"), "its row in the browser");
    assert_eq!(App::new(Some("adsb-receiver")).tab, RibbonTab::System);
}

/// Projects as files: a change marks the project unsaved; New asks first, Cancel keeps it,
/// Save As writes it where chosen and Open reads it back with the change, a project begun anew
/// in between. Analytic evidence: the εr written (3.3) is the one read back.
#[test]
fn a_project_is_saved_where_chosen_and_opened_again() {
    use emcraft_ui_core::files::{Answer, FileAction, Purpose, Then};
    let path = std::env::temp_dir().join(format!("emcraft-iced-{}.emcraft", std::process::id()));
    let path_text = path.to_string_lossy().into_owned();
    let mut app = App::new(Some("highpass"));
    app.update(Message::Row("material:ptfe-like".into()));
    app.update(Message::Edit);
    let k = app.dialog.as_ref().and_then(|d| d.fields.iter().position(|f| f.name == "eps_r")).unwrap_or_default();
    app.update(Message::Dialog(crate::dialog::Edit::Text(k, "3.3".into())));
    app.update(Message::Dialog(crate::dialog::Edit::Ok));
    assert!(app.session.is_dirty());
    app.update(Message::File(FileAction::Request(Then::New)));
    assert_eq!(app.asking, Some(Then::New), "asked before the change is lost");
    app.update(Message::File(FileAction::Confirm(Answer::Cancel)));
    assert!(app.asking.is_none() && app.session.is_dirty());
    app.update(Message::File(FileAction::Chosen(Purpose::SaveAs, Some(path_text.clone()))));
    assert_eq!(app.error, None);
    assert!(!app.session.is_dirty() && path.exists(), "saved");
    app.update(Message::File(FileAction::Request(Then::New)));
    assert!(app.session.model().materials.iter().all(|m| m.name != "ptfe-like"), "a new project");
    app.update(Message::File(FileAction::Chosen(Purpose::Open, Some(path_text))));
    let eps = app.session.model().materials.iter().find(|m| m.name == "ptfe-like").map(|m| m.eps_r.get());
    assert_eq!(eps, Some(3.3));
    let _ = std::fs::remove_file(&path);
}

/// Unsaved work is copied (here at once: no wait), the copy offered on the next start and
/// recovered with the change; a real save deletes the copy and puts the file first among the
/// recent projects.
#[test]
fn unsaved_work_is_copied_and_recovered() {
    use emcraft_ui_core::files::{FileAction, Purpose, Then};
    let dir = std::env::temp_dir().join(format!("emcraft-iced-autosave-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut app = App::new(Some("highpass")).with_autosave(Some(dir.clone()));
    app.autosave.every = 0.0;
    app.update(Message::Row("material:ptfe-like".into()));
    app.update(Message::Edit);
    let k = app.dialog.as_ref().and_then(|d| d.fields.iter().position(|f| f.name == "eps_r")).unwrap_or_default();
    app.update(Message::Dialog(crate::dialog::Edit::Text(k, "4.4".into())));
    app.update(Message::Dialog(crate::dialog::Edit::Ok));
    // The next message after the change finds the copy due.
    app.update(Message::Page(emcraft_ui_core::view::View::Model));
    let Some(copy) = app.autosave.file.clone() else { panic!("a copy written") };
    assert!(copy.exists());
    // The next start offers it; Recover opens it, unsaved, with the change.
    let mut next = App::new(None).with_autosave(Some(dir.clone()));
    assert_eq!(next.autosave.found.len(), 1);
    let then = Then::Recover { file: copy.display().to_string(), original: None };
    next.update(Message::File(FileAction::Request(then)));
    let eps = next.session.model().materials.iter().find(|m| m.name == "ptfe-like").map(|m| m.eps_r.get());
    assert_eq!(eps, Some(4.4));
    assert!(next.autosave.found.is_empty() && next.autosave.file.as_ref() == Some(&copy));
    // Saved where chosen: the copy goes, the file is the first recent project.
    let saved = std::env::temp_dir().join(format!("emcraft-iced-saved-{}.emcraft", std::process::id())).display().to_string();
    next.update(Message::File(FileAction::Chosen(Purpose::SaveAs, Some(saved.clone()))));
    assert!(!copy.exists(), "the copy deleted by the save");
    assert_eq!(next.prefs.recent.first(), Some(&saved));
    let _ = std::fs::remove_file(&saved);
    let _ = std::fs::remove_dir_all(&dir);
}

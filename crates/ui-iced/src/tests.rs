//! The window driven as a person would: its messages (typing, dragging, buttons) each changing
//! the chain do, and the view drawn headless and clicked.

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
    use emcraft_ui_core::navigation::Goal;
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
    app.update(Message::View3d(Nav::Goal(Goal::Fit)));
    assert!(app.view3d.animating() && app.view3d.camera == moved, "a fit is animated from the next frame");
    settle(&mut app);
    assert!(!app.view3d.animating());
    assert!((app.view3d.camera.distance - fitted.distance).abs() < 1e-12, "fitted again as at first");
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
    assert!(app.charts.page.as_ref().is_some_and(|p| p.shown.is_ok()), "its chart drawn again with the sweep");
    // The project's chart style (view.chart.style) is the window's chart's.
    app.update(Message::Run(Command { id: "view.chart.style", params: json!({"chart": "sparameters", "title": "Filter response"}) }));
    assert!(app.charts.page.as_ref().is_some_and(|p| p.shown.as_ref().is_ok_and(|c| c.title == "Filter response")));
    // An analysis read from the sweep: its dialog, its reply kept, its chart.
    app.update(Message::Page(View::Analyses));
    app.update(Message::Command("results.noise".into()));
    app.update(Message::Dialog(crate::dialog::Edit::Ok));
    finish(&mut app);
    assert!(app.replies.analysis.as_ref().is_some_and(|a| a.command == "results.noise"), "{:?}", app.error);
    assert!(app.charts.page.as_ref().is_some_and(|p| p.shown.is_ok()), "the noise figure's chart");
    // The sweep's other pages: TDR and the pulse, each its chart.
    for page in [View::Tdr, View::Pulse] {
        app.update(Message::Page(page));
        assert!(app.charts.page.as_ref().is_some_and(|p| p.shown.is_ok()), "{page:?}'s chart");
    }
    assert!(app.log.lines().iter().any(|n| n.text.contains(" s")), "its time noted: {:?}", app.log);
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
            ("tdr", emcraft_ui_core::view::View::Tdr, None),
            ("model", emcraft_ui_core::view::View::Model, None),
            ("model-ports", emcraft_ui_core::view::View::Model, Some(ports)),
            ("model-field", emcraft_ui_core::view::View::Model, Some(field)),
            ("model-arrows", emcraft_ui_core::view::View::Model, Some(h_arrows)),
            ("model-probe", emcraft_ui_core::view::View::Model, Some(Message::Hover(Some(emcraft_ui_core::geom::Point::new(400.0, 300.0))))),
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
    app.update(Message::View3d(Nav::Goal(emcraft_ui_core::navigation::Goal::Fit)));
    assert_ne!(app.view3d.shown.as_ref().map(|s| s.packed.id), packed, "lit: the scene built again");
    app.gpu_look.outline = true;
    app.update(click(Point::new(2.0, 2.0), true, false));
    assert_eq!(app.selection.len(), 1, "Shift on empty space keeps it");
    app.update(Message::View3d(Nav::Zoom(3.0)));
    let zoomed = app.view3d.camera.distance;
    app.update(click(Point::new(2.0, 2.0), false, true));
    settle(&mut app);
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
    app.update(Message::View3d(Nav::Goal(emcraft_ui_core::navigation::Goal::Fit)));
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

/// An item's form in the properties panel: a material's row selected opens it on its values (εr 2), OK unchanged
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
    assert_eq!(eps(&app), Some(2.5));
    // The item's form stays in the properties panel, on the value set.
    assert_eq!(app.dialog.as_ref().map(|d| d.fields[k].text.clone()), Some("2.5".into()));
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

/// The tree's ＋: Geometry's list offers a new material, whose dialog opens in the properties
/// panel; filled in and confirmed, the material is in the model and its row in the tree. A
/// resonator's Excitation offers nothing to add; a system's chain adds its source.
#[test]
fn a_steps_plus_opens_a_commands_dialog_and_runs_it() {
    use crate::dialog::Edit;
    let mut app = App::new(Some("highpass"));
    app.update(Message::Add(Some("geometry")));
    let handles: Vec<String> = crate::control::handles(&app).into_iter().map(|(h, _)| h).collect();
    assert!(handles.iter().any(|h| h == "command:model.material.create") && handles.iter().any(|h| h == "play:mesh"), "{handles:?}");
    app.update(Message::Command("model.material.create".into()));
    assert_eq!(app.adding, None, "the list closes");
    let at = |app: &App, name: &str| app.dialog.as_ref().and_then(|d| d.fields.iter().position(|f| f.name == name));
    let (Some(name), Some(eps)) = (at(&app, "name"), at(&app, "eps_r")) else { panic!("the material's dialog") };
    app.update(Message::Dialog(Edit::Text(name, "alumina".into())));
    app.update(Message::Dialog(Edit::Text(eps, "9.8".into())));
    app.update(Message::Dialog(Edit::Ok));
    assert!(app.dialog.is_none(), "{:?}", app.dialog.as_ref().map(|d| d.error.clone()));
    assert!(app.session.model().materials.iter().any(|m| m.name == "alumina" && m.eps_r.get() == 9.8));
    assert!(app.rows().any(|r| r.id == "material:alumina"), "its row in the browser");
    let cavity = App::new(Some("cavity"));
    let offers = |app: &App, h: &str| crate::control::handles(app).into_iter().any(|(name, _)| name == h);
    assert!(!offers(&cavity, "add:excitation") && offers(&cavity, "add:geometry"));
    assert!(offers(&App::new(Some("adsb-receiver")), "add:Source"));
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

/// The field's values under the pointer: over the plane on the field overlay a probe reads them
/// (the coloured component and each field's components); off the plane, or on another overlay,
/// nothing. Analytic evidence: TE10's E on the H-plane is Ey alone, so |Ex| and |Ez| read far
/// below |Ey|.
#[test]
fn the_pointer_over_the_plane_reads_the_field() {
    use crate::view3d::Nav;
    use emcraft_ui_core::geom::Point;
    use emcraft_ui_core::scene3d::Overlay;
    let mut app = App::new(Some("highpass"));
    app.update(Message::View3d(Nav::Size(800.0, 600.0)));
    app.update(Message::Start("solve.run".into()));
    finish(&mut app);
    app.update(Message::Page(emcraft_ui_core::view::View::Model));
    app.update(Message::View3d(Nav::Overlay(Overlay::Field)));
    app.update(Message::Hover(Some(Point::new(400.0, 300.0))));
    let Some(p) = app.probe() else { panic!("the plane under the view's middle") };
    let (ey, ex, ez) = (p.e.y.norm(), p.e.x.norm(), p.e.z.norm());
    assert!(ey > 0.0 && ex < 0.1 * ey && ez < 0.1 * ey, "Ey {ey}, Ex {ex}, Ez {ez}");
    assert!(emcraft_ui_core::scene3d::probe_lines(&p, app.view3d.field.component, 0.0)[0].contains("V/m"));
    app.update(Message::Hover(Some(Point::new(2.0, 2.0))));
    assert!(app.probe().is_none(), "off the plane");
    app.update(Message::Hover(Some(Point::new(400.0, 300.0))));
    app.update(Message::View3d(Nav::Overlay(Overlay::Plain)));
    assert!(app.probe().is_none(), "no field shown");
}

/// A cavity's eigenmodes: Run solves them and lands on their page; a mode's Field shows its
/// field on the 3D view's plane (the GPU colouring it), the mode chosen in the field's bar.
/// Analytic evidence: the first is TE101 of the 22.86 × 10.16 × 30 mm box, f = (c/2)·√(1/a² +
/// 1/d²) ≈ 8.245 GHz (to the mesh's half per cent), and the modes come in ascending order.
#[test]
fn a_cavitys_modes_are_listed_and_one_is_shown() {
    use emcraft_ui_core::view::View;
    let mut app = App::new(Some("cavity"));
    app.update(Message::Start("solve.run".into()));
    finish(&mut app);
    assert_eq!(app.error, None);
    assert_eq!(app.page, View::Eigenmodes);
    let freqs: Vec<f64> = app.session.results().eigen.iter().flat_map(|e| e.modes.iter().map(|m| m.frequency)).collect();
    assert!(!freqs.is_empty() && freqs.windows(2).all(|w| w[0] <= w[1]), "{freqs:?}");
    let c = 299_792_458.0;
    let te101 = c / 2.0 * (1.0 / 0.02286f64.powi(2) + 1.0 / 0.03f64.powi(2)).sqrt();
    assert!((freqs[0] / te101 - 1.0).abs() < 5e-3, "{} against {te101}", freqs[0]);
    app.update(Message::Mode(0));
    assert_eq!((app.page, app.view3d.field.mode), (View::Model, Some(0)));
    assert_eq!(app.view3d.field_error, None);
    assert!(app.view3d.shown.as_ref().is_some_and(|s| s.field.is_some()), "the mode's field on the plane");
    if let Ok(dir) = std::env::var("EMCRAFT_ICED_SHOTS") {
        let _gpu = gpu();
        for (name, page) in [("eigenmodes", View::Eigenmodes), ("cavity-mode", View::Model)] {
            app.update(Message::Page(page));
            let mut ui = iced_test::Simulator::with_size(settings(), (1400.0, 900.0), app.view());
            let shot = ui.snapshot(&app.theme()).unwrap_or_else(|e| panic!("{e}"));
            let _ = std::fs::remove_file(std::path::Path::new(&dir).join(format!("{name}-wgpu.png")));
            assert!(shot.matches_image(std::path::Path::new(&dir).join(name)).unwrap_or(false));
        }
    }
}

/// Undo and Redo, by their keys too: a material's εr changed is taken back and done again; the
/// keys go to the window only when no field takes them.
#[test]
fn undo_and_redo_by_keys() {
    use iced::keyboard::{Event, Key, Modifiers};
    let key = |c: &str, modifiers: Modifiers, status| {
        let event = iced::Event::Keyboard(Event::KeyPressed {
            key: Key::Character(c.into()),
            modified_key: Key::Character(c.into()),
            physical_key: iced::keyboard::key::Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
            location: iced::keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        });
        crate::files::shortcut(event, status, iced::window::Id::unique())
    };
    let mut app = App::new(Some("highpass"));
    let eps = |app: &App| app.session.model().materials.iter().find(|m| m.name == "ptfe-like").map(|m| m.eps_r.get());
    app.update(Message::Run(Command { id: "model.material.edit", params: json!({"name": "ptfe-like", "eps_r": 3.0}) }));
    assert_eq!(eps(&app), Some(3.0));
    let ignored = iced::event::Status::Ignored;
    let Some(undo) = key("z", Modifiers::CTRL, ignored) else { panic!("Ctrl+Z") };
    app.update(undo);
    assert_eq!(eps(&app), Some(2.0), "taken back");
    let Some(redo) = key("z", Modifiers::CTRL | Modifiers::SHIFT, ignored) else { panic!("Ctrl+Shift+Z") };
    app.update(redo);
    assert_eq!(eps(&app), Some(3.0), "done again");
    assert!(key("z", Modifiers::CTRL, iced::event::Status::Captured).is_none(), "a field's own");
    assert!(key("z", Modifiers::empty(), ignored).is_none());
}

/// The settings page: a theme chosen dresses the window at once, the units new projects start
/// with and the 3D view's drawing are kept, and an unknown mouse mapping is refused with why.
#[test]
fn settings_change_the_window_at_once() {
    use emcraft_ui_core::action::Pref;
    let mut app = App::new(Some("highpass"));
    app.update(Message::Settings(true));
    assert!(app.settings);
    app.update(Message::Pref(Pref::Theme("dark".into())));
    assert_eq!(app.look.theme.name, "dark");
    app.update(Message::Pref(Pref::Length(emcraft_model::units::Length::Metre)));
    assert_eq!(app.prefs.length, emcraft_model::units::Length::Metre);
    let off = emcraft_ui_core::prefs::GpuLook { outline: false, ..app.gpu_look };
    app.update(Message::Pref(Pref::GpuLook(off)));
    assert!(!app.gpu_look.outline, "the 3D view's drawing follows");
    if let Ok(dir) = std::env::var("EMCRAFT_ICED_SHOTS") {
        let _gpu = gpu();
        let mut ui = iced_test::Simulator::with_size(settings(), (1400.0, 900.0), app.view());
        let shot = ui.snapshot(&app.theme()).unwrap_or_else(|e| panic!("{e}"));
        let _ = std::fs::remove_file(std::path::Path::new(&dir).join("settings-wgpu.png"));
        assert!(shot.matches_image(std::path::Path::new(&dir).join("settings")).unwrap_or(false));
    }
    app.update(Message::Pref(Pref::Navigation("nowhere".into())));
    assert!(app.error.as_deref().is_some_and(|e| e.contains("nowhere")));
    app.update(Message::Page(emcraft_ui_core::view::View::Model));
    assert!(!app.settings, "a page leaves the settings");
}

/// The start page: a window opened on nothing shows it; a template's picture is its model drawn
/// from the corner by the GPU (offscreen), and its card opens the template, leaving the page.
#[test]
fn the_start_page_draws_templates_and_opens_one() {
    use emcraft_ui_core::files::{FileAction, Then};
    let _gpu = gpu();
    let mut app = App::new(None);
    assert!(app.home);
    // A system has no model to draw: its card goes without a picture.
    assert!(crate::start::drawn("adsb-receiver", (*app.look.theme).clone()).is_err());
    for name in ["highpass", "cavity"] {
        match crate::start::drawn(name, (*app.look.theme).clone()) {
            Ok(p) => app.update(Message::Thumb(name, Ok(p))),
            Err(e) if e.contains("adapter") => return eprintln!("skipped: {e}"),
            Err(e) => panic!("{name}: {e}"),
        }
    }
    assert_eq!(app.thumbs.1.len(), 2);
    if let Ok(dir) = std::env::var("EMCRAFT_ICED_SHOTS") {
        let mut ui = iced_test::Simulator::with_size(settings(), (1400.0, 900.0), app.view());
        let shot = ui.snapshot(&app.theme()).unwrap_or_else(|e| panic!("{e}"));
        let _ = std::fs::remove_file(std::path::Path::new(&dir).join("start-wgpu.png"));
        assert!(shot.matches_image(std::path::Path::new(&dir).join("start")).unwrap_or(false));
    }
    let open = Then::Sample { name: "cavity".into(), values: serde_json::Value::Null, purpose: None };
    app.update(Message::File(FileAction::Request(open)));
    assert!(!app.home && app.session.name() == "cavity");
}

/// The port modes page, on the replies kept: before the modes are solved it says what they
/// need; solve.port_modes' reply is kept and shown, each port's modes; opening another project
/// lets it go. Analytic evidence: WR-90's TE10 propagates at the band's frequency above its
/// 6.56 GHz cut-off, its β positive.
#[test]
fn port_modes_are_kept_and_shown() {
    use emcraft_ui_core::files::{FileAction, Then};
    use emcraft_ui_core::view::View;
    let mut app = App::new(Some("highpass"));
    app.update(Message::Page(View::PortModes));
    assert!(app.replies.port_modes.is_none());
    app.update(Message::Start("solve.port_modes".into()));
    finish(&mut app);
    let Some(modes) = app.replies.port_modes.clone() else { panic!("the reply kept: {:?}", app.error) };
    let first = &modes.ports[0].modes[0];
    assert!(first.propagating && first.beta.is_some_and(|b| b > 0.0), "{first:?}");
    let _ = app.view();
    app.update(Message::File(FileAction::Request(Then::Sample { name: "cavity".into(), values: serde_json::Value::Null, purpose: None })));
    assert!(app.replies.port_modes.is_none(), "another project lets it go");
}

/// A board's diagnosis: the split-ground template opens on the PCB page; a route chosen, the
/// conditions confirmed and the route inspected, its report is kept and shown. Analytic
/// evidence: the ground plane's slot under the route leaves its return current no copper there,
/// so the report finds the reference missing or a slot in the return corridor.
#[test]
fn a_boards_route_is_diagnosed() {
    use emcraft_engine::pcb::{Code, Stage};
    use emcraft_ui_core::view::View;
    let mut app = App::new(Some("pcb-split-ground"));
    assert_eq!(app.page, View::Pcb);
    let Some(net) = app.session.model().pcb().and_then(|b| b.segments.first()).map(|s| s.net.clone()) else { panic!("a routed board") };
    let run = |id: &'static str, params| Message::Run(Command { id, params });
    app.update(run("view.pcb", json!({"net": net, "stage": Stage::Settings.id()})));
    assert_eq!(emcraft_ui_core::pcb::route(&app.session), Some(net.as_str()));
    let _ = app.view();
    app.update(run("pcb.conditions", crate::pcb::conditions(&app)));
    assert!(app.session.views().pcb.conditions().is_some(), "{:?}", app.error);
    app.update(run("pcb.inspect", json!({"net": net})));
    app.update(run("view.pcb", json!({"stage": Stage::Results.id()})));
    let Some(report) = emcraft_ui_core::pcb::report(&app.session, &app.replies, &net) else { panic!("the report kept: {:?}", app.error) };
    assert!(report.findings.iter().any(|f| matches!(f.code, Code::ReturnGap | Code::ReferenceEdge)), "{:?}", report.findings);
    let _ = app.view();
    // A plain microstrip meets the line model: its ideal line's S-parameters and TDR, charted.
    let mut app = App::new(Some("pcb-microstrip"));
    let Some(net) = app.session.model().pcb().and_then(|b| b.segments.first()).map(|s| s.net.clone()) else { panic!("a routed board") };
    app.update(run("view.pcb", json!({"net": net, "stage": Stage::Settings.id()})));
    app.update(run("pcb.conditions", crate::pcb::conditions(&app)));
    app.update(run("pcb.inspect", json!({"net": net})));
    app.update(run("view.pcb", json!({"stage": Stage::Results.id()})));
    let line = emcraft_ui_core::pcb::report(&app.session, &app.replies, &net).and_then(|r| r.si.clone());
    assert!(line.is_some_and(|l| !l.points.is_empty() && !l.tdr.is_empty()), "{:?}", app.error);
    if let Ok(dir) = std::env::var("EMCRAFT_ICED_SHOTS") {
        let _gpu = gpu();
        let mut ui = iced_test::Simulator::with_size(settings(), (1400.0, 1000.0), app.view());
        let shot = ui.snapshot(&app.theme()).unwrap_or_else(|e| panic!("{e}"));
        let _ = std::fs::remove_file(std::path::Path::new(&dir).join("pcb-wgpu.png"));
        assert!(shot.matches_image(std::path::Path::new(&dir).join("pcb")).unwrap_or(false));
    }
}

/// Every scenario in `tests/scenarios/`, each on a window opened on nothing, driven through the
/// control channel as an agent drives it; the failures listed together.
#[test]
fn scenarios() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios");
    let mut files: Vec<_> = std::fs::read_dir(&dir).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    files.sort();
    assert!(!files.is_empty(), "no scenarios in {}", dir.display());
    let failed: Vec<String> = files
        .iter()
        .filter_map(|f| {
            // A scenario that does not read fails as one that fails.
            let steps: Vec<serde_json::Value> =
                match std::fs::read_to_string(f).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string())) {
                    Ok(steps) => steps,
                    Err(e) => return Some(format!("{}: does not read: {e}", f.display())),
                };
            let mut app = App::new(None);
            crate::scenario::run(&mut app, &steps).err().map(|e| format!("{}: {e}", f.display()))
        })
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// The frames a view change takes, one a second later than the last: what is under way ends.
fn settle(app: &mut App) {
    let now = std::time::Instant::now();
    for k in 0..3 {
        app.update(Message::View3d(crate::view3d::Nav::Tick(now + std::time::Duration::from_secs(k))));
    }
}

/// The 3D view's keys and box. Analytic evidence: an arrow tap turns the
/// level front view's yaw by exactly 15° (Shift 90°), Ctrl+5 looks down (pitch 90°), a held arrow turns at 90°/s
/// for the frames' time past the tap (capped at 0.1 s a frame), and a box dragged round the
/// whole view by window takes every part that is drawn.
#[test]
fn keys_turn_the_view_and_a_box_selects() {
    use crate::view3d::Nav;
    use emcraft_ui_core::geom::Point;
    use emcraft_ui_core::selection::BoxSel;
    let mut app = App::new(Some("highpass"));
    app.update(Message::View3d(Nav::Size(600.0, 400.0)));
    // Ctrl+1: the front view, level, where a turn right is a turn in yaw alone.
    let front = crate::view3d::shader::keys(ctrl_key("1"), iced::event::Status::Ignored, iced::window::Id::unique());
    app.update(front.unwrap_or(Message::Key3d(Nav::Released)));
    settle(&mut app);
    assert!(app.view3d.camera.pitch.abs() < 1e-9 && app.view3d.camera.yaw.abs() < 1e-9, "Ctrl+1 looks from the front");
    let yaw = app.view3d.camera.yaw;
    let arrow = |shift| Message::Key3d(Nav::Arrow { right: 1.0, up: 0.0, shift, ctrl: false });
    app.update(arrow(false));
    app.update(Message::Key3d(Nav::Released));
    settle(&mut app);
    let turned = (app.view3d.camera.yaw - yaw).to_degrees();
    assert!((turned - 15.0).abs() < 1e-9, "a tap turns 15°: {turned}");
    app.update(arrow(true));
    app.update(Message::Key3d(Nav::Released));
    settle(&mut app);
    assert!(((app.view3d.camera.yaw - yaw).to_degrees() - 105.0).abs() < 1e-9, "with Shift a quarter turn more");
    // Ctrl+5: the top view.
    let top = crate::view3d::shader::keys(ctrl_key("5"), iced::event::Status::Ignored, iced::window::Id::unique());
    assert!(matches!(top, Some(Message::Key3d(Nav::Goal(_)))));
    app.update(top.unwrap_or(Message::Key3d(Nav::Released)));
    settle(&mut app);
    assert!((app.view3d.camera.pitch.to_degrees() - 90.0).abs() < 1e-6, "Ctrl+5 looks down");
    // Held: after the tap the view turns steadily, 90°/s at most 0.1 s a frame.
    let at = app.view3d.camera;
    app.update(arrow(false));
    let t0 = std::time::Instant::now();
    app.update(Message::View3d(Nav::Tick(t0)));
    app.update(Message::View3d(Nav::Tick(t0 + std::time::Duration::from_millis(1000))));
    app.update(Message::View3d(Nav::Tick(t0 + std::time::Duration::from_millis(1050))));
    let turned = app.view3d.camera;
    app.update(Message::Key3d(Nav::Released));
    assert_ne!(turned, at, "a held arrow turns");
    assert!(!app.view3d.animating(), "let go, it stops");
    // A dialog over the view takes the keys.
    app.update(Message::Command("model.material.create".into()));
    assert!(app.dialog.is_some());
    let before = app.view3d.camera;
    app.update(arrow(false));
    settle(&mut app);
    assert_eq!(app.view3d.camera, before, "the dialog has the keys");
    app.update(Message::Dialog(crate::dialog::Edit::Cancel));
    // A box round the whole view takes what is drawn; one with Shift adds, one on nothing clears.
    app.update(Message::View3d(Nav::Box(Some(BoxSel::from_drag(Point::new(0.0, 0.0), Point::new(600.0, 400.0))))));
    app.update(Message::Boxed(false));
    assert!(app.selection.len() > 2, "a box round the view takes the parts: {:?}", app.selection.iter().collect::<Vec<_>>());
    assert!(app.view3d.boxsel.is_none(), "let go, the box goes");
    app.update(Message::View3d(Nav::Box(Some(BoxSel::from_drag(Point::new(1.0, 1.0), Point::new(3.0, 3.0))))));
    let many = app.selection.len();
    app.update(Message::Boxed(true));
    assert_eq!(app.selection.len(), many, "Shift on nothing keeps it");
    app.update(Message::View3d(Nav::Box(Some(BoxSel::from_drag(Point::new(1.0, 1.0), Point::new(3.0, 3.0))))));
    app.update(Message::Boxed(false));
    assert_eq!(app.selection.len(), 0, "a box on nothing clears it");
    // The wheel zooms toward the point under the pointer, within the model's limits.
    let d = app.view3d.camera.distance;
    app.update(Message::View3d(Nav::ZoomAt(1e6, Point::new(300.0, 200.0))));
    assert!(app.view3d.camera.distance < d && app.view3d.camera.distance > 0.0);
    app.update(Message::View3d(Nav::ZoomAt(f32::NAN, Point::new(300.0, 200.0))));
    assert!(app.view3d.camera.distance.is_finite());
}

/// A key pressed with Ctrl (Cmd on macOS).
fn ctrl_key(c: &str) -> iced::Event {
    use iced::keyboard::{Event, Key, Location, Modifiers, key};
    iced::Event::Keyboard(Event::KeyPressed {
        key: Key::Character(c.into()),
        modified_key: Key::Character(c.into()),
        physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
        location: Location::Standard,
        modifiers: Modifiers::COMMAND,
        text: None,
        repeat: false,
    })
}

/// The 3D view's tools. Analytic evidence: the measure tool snapped to two
/// corners of the face under the pointer measures exactly their coordinates' difference (the
/// engine's `geometry.measure`, compared with the corners themselves, in mm); the port tool on a
/// guide's end that has a port selects that port; its reference plane dragged off the face lands
/// on a whole thousandth of a millimetre (`model.port.edit`).
#[test]
fn the_tools_measure_select_ports_and_move_the_reference_plane() {
    use crate::view3d::{Click, GripMove, Nav};
    use emcraft_ui_core::geom::Point;
    use emcraft_ui_core::navigation::Goal;
    use emcraft_ui_core::tools::{SnapKind, Tool, screen};
    let mut app = App::new(Some("highpass"));
    app.update(Message::View3d(Nav::Size(800.0, 600.0)));
    app.update(Message::View3d(Nav::Goal(Goal::Home)));
    settle(&mut app);
    let size = emcraft_ui_core::geom::Vector::new(800.0, 600.0);
    let middle = Point::new(400.0, 300.0);
    // Measure: the face under the middle, then two of its corners.
    app.update(Message::Tool(Some(Tool::Measure)));
    app.update(Message::Hover(Some(middle)));
    let face = app.tools.hover.as_ref().map_or_else(|| panic!("a face under the middle"), |h| h.face.clone());
    let corners: Vec<[f64; 3]> = face.corners.clone();
    let (a, b) = (corners[0], corners.iter().copied().find(|c| c[0] != corners[0][0]).unwrap_or(corners[1]));
    let camera = app.view3d.camera;
    for c in [a, b] {
        let at = screen(&camera, size, c).unwrap_or_else(|| panic!("the corner in view"));
        app.update(Message::Hover(Some(at)));
        assert_eq!(app.tools.hover.as_ref().map(|h| h.snap.kind), Some(SnapKind::Vertex), "snapped to the corner");
        app.update(Message::Click(Click { at, add: false, double: false }));
    }
    let reply = app.tools.measured.as_ref().map_or_else(|| panic!("measured: {:?}", app.error), |m| m.reply.clone());
    let mm = (0..3).map(|k| ((b[k] - a[k]) * 1e3).powi(2)).sum::<f64>().sqrt();
    let got = reply["distance"].as_f64().unwrap_or(f64::NAN);
    assert!((got - mm).abs() < 1e-6 * mm.max(1.0), "{got} mm against the corners' {mm} mm");
    // Escape drops the measurement, again turns the tool off.
    app.update(Message::Escape(false));
    assert!(app.tools.measured.is_none() && app.tool == Some(Tool::Measure));
    app.update(Message::Escape(false));
    assert_eq!(app.tool, None);
    // The port tool on an end with a port selects it.
    app.update(Message::Tool(Some(Tool::Port)));
    let ends: Vec<[f64; 3]> = app
        .session
        .model()
        .ports
        .iter()
        .filter_map(|p| emcraft_ui_core::scene3d::port_quad(app.session.model(), p))
        .map(|q| emcraft_ui_core::scene3d::quad_centre(&q))
        .collect();
    let seen = ends.iter().filter_map(|c| screen(&camera, size, *c)).find_map(|at| {
        app.update(Message::Hover(Some(at)));
        app.tools.hover.as_ref().and_then(|h| h.port).filter(|p| matches!(p, emcraft_ui_core::tools::PortTarget::Existing(_))).map(|_| at)
    });
    let at = seen.unwrap_or_else(|| panic!("a port's face under the pointer"));
    app.update(Message::Click(Click { at, add: false, double: false }));
    assert!(matches!(app.selection.single(), Some(emcraft_ui_core::scene3d::Pick::Port(_))), "{:?}", app.selection.iter().collect::<Vec<_>>());
    // A click on empty space turns the tool off.
    app.update(Message::Hover(Some(Point::new(2.0, 2.0))));
    app.update(Message::Click(Click { at: Point::new(2.0, 2.0), add: false, double: false }));
    assert_eq!(app.tool, None);
    // The selected port's reference plane dragged by its grip.
    let Some(emcraft_ui_core::scene3d::Pick::Port(i)) = app.selection.single() else { panic!("a port") };
    let grip = app.grip_at().unwrap_or_else(|| panic!("its grip in view"));
    app.update(Message::Grip(GripMove::Start(grip)));
    app.update(Message::Grip(GripMove::By(40.0, 0.0)));
    app.update(Message::Grip(GripMove::By(0.0, 40.0)));
    assert!(app.tools.drag.is_some());
    app.update(Message::Grip(GripMove::End));
    let deembed = app.session.model().ports.get(i).map_or(0.0, |p| p.deembed * 1e3);
    assert!(deembed != 0.0, "moved off the port's face: {:?}", app.error);
    assert!(((deembed * 1e3).round() - deembed * 1e3).abs() < 1e-6, "a whole thousandth of a mm: {deembed}");
    // `ui.inspect` names the tools' state.
    let seen = crate::control::inspect(&app);
    assert!(seen["tools"]["drag"].is_null() && seen["tools"]["tool"].is_null());
}

/// A template's shape is set by its parameters, not drawn (docs/adr/0010-geometry-from-solidcraft.md):
/// the guide width's row opens its form, Apply builds the filter again at the new width; a body's
/// form is its material; the search offers no command that makes a shape. Analytic evidence: the
/// high-pass filter's guide is a = 22.86 mm wide (WR-90), set to 20 mm.
#[test]
fn a_templates_shape_is_its_parameters() {
    use crate::dialog::Edit;
    let mut app = App::new(Some("highpass"));
    let width = |app: &App| app.session.model().waveguide().map(|w| w.width.get() * 1e3);
    assert!(width(&app).is_some_and(|w| (w - 22.86).abs() < 1e-9));
    app.update(Message::Row("variable:a".into()));
    assert_eq!(app.dialog.as_ref().map(|d| d.command.id), Some("project.variable"));
    let value = app.dialog.as_ref().and_then(|d| d.fields.iter().position(|f| f.name == "value")).unwrap_or_else(|| panic!("its value"));
    app.update(Message::Dialog(Edit::Text(value, "20".into())));
    app.update(Message::Dialog(Edit::Ok));
    // The model is built again on the worker.
    finish(&mut app);
    assert!(app.error.is_none(), "{:?}", app.error);
    assert!(width(&app).is_some_and(|w| (w - 20.0).abs() < 1e-9), "{:?}", width(&app));
    let ids = |q: &str| emcraft_ui_core::search::matches(q).into_iter().map(|c| c.id).collect::<Vec<_>>();
    assert!(!ids("geometry").iter().any(|id| emcraft_ui_core::outline::SHAPES.contains(id)), "{:?}", ids("geometry"));
    assert!(ids("geometry").contains(&"geometry.import"));
    let mut posts = App::new(Some("inductive-post"));
    let body = posts.rows().find(|r| r.id.starts_with("body:")).map_or_else(|| panic!("a body"), |r| r.id.clone());
    posts.update(Message::Row(body));
    assert_eq!(posts.dialog.as_ref().map(|d| d.command.id), Some("geometry.body.material"));
}

/// The section view's dimensions, presets and the look at the cut. Analytic evidence: the iris
/// filter's H-plane cut (y = b/2) carries its overall extents and its irises' apertures, and its
/// two ports sized as the guide is wide; looking at the cut turns the camera orthographic to
/// look along y (pitch ±90°).
#[test]
fn the_section_shows_its_dimensions_and_looks_at_the_cut() {
    use crate::view3d::Nav;
    use emcraft_ui_core::scene3d::section::Preset;
    let mut app = App::new(Some("iris-bandpass"));
    app.update(Message::View3d(Nav::Size(800.0, 600.0)));
    let bounds = app.view3d.bounds().unwrap_or_else(|| panic!("a model"));
    app.update(Message::View3d(Nav::Section(Some(Preset::HPlane.cut(bounds)))));
    let (dims, ports) = app.section_notes().unwrap_or_else(|| panic!("notes on the cut"));
    assert!(dims.len() > 2, "{dims:?}");
    assert_eq!(ports.len(), 2);
    let width = app.session.model().waveguide().map_or(0.0, |w| w.width.get());
    assert!(ports.iter().all(|p| (p.b[0] - p.a[0] - width).abs() < 1e-12), "{ports:?}");
    app.update(Message::View3d(Nav::LookAtCut));
    settle(&mut app);
    assert_eq!(app.view3d.camera.projection, emcraft_render::view3d::Projection::Orthographic);
    assert!((app.view3d.camera.pitch.abs().to_degrees() - 90.0).abs() < 1e-6, "looking along y: {:?}", app.view3d.camera);
    app.update(Message::View3d(Nav::Dims(false)));
    assert!(app.section_notes().is_none(), "dimensions off");
}

/// The orbit's pivot, the field's plane dragged, the reference plane typed. Analytic evidence: an orbit about the origin
/// keeps the origin where it was on screen; the field's plane dragged by its whole extent on
/// screen moves by the model's whole extent along its normal (clamped there); facing the plane
/// looks along its normal; the reference plane typed as 2.5 mm is 2.5 mm, and 0 puts it back.
#[test]
fn the_pivot_the_plane_the_projection_and_the_reference_number() {
    use crate::view3d::Nav;
    use emcraft_render::view3d::Projection;
    use emcraft_ui_core::geom::Point;
    use emcraft_ui_core::navigation::OrbitAround;
    let mut app = App::new(Some("highpass"));
    app.update(Message::View3d(Nav::Size(800.0, 600.0)));
    settle(&mut app);
    let size = emcraft_ui_core::geom::Vector::new(800.0, 600.0);
    let origin = |app: &App| emcraft_ui_core::tools::screen(&app.view3d.camera, size, [0.0; 3]).unwrap_or_else(|| panic!("in view"));
    let before = origin(&app);
    app.update(Message::View3d(Nav::OrbitFrom(Point::new(400.0, 300.0), OrbitAround::Origin)));
    app.update(Message::View3d(Nav::Orbit(60.0, 25.0)));
    let after = origin(&app);
    assert!(before.distance(after) < 1e-2, "the pivot stays put: {before:?} → {after:?}");
    // About the view's centre instead, the origin moves.
    app.update(Message::View3d(Nav::OrbitFrom(Point::new(400.0, 300.0), OrbitAround::ViewCentre)));
    app.update(Message::View3d(Nav::Orbit(60.0, 25.0)));
    assert!(after.distance(origin(&app)) > 1.0);
    // The projection.
    app.update(Message::View3d(Nav::Projection(Projection::Orthographic)));
    assert_eq!(app.view3d.camera.projection, Projection::Orthographic);
    // The field's plane (H: across y) dragged by its whole extent on screen.
    let (lo, hi) = app.view3d.bounds().unwrap_or_else(|| panic!("a model"));
    let mid = [0, 1, 2].map(|i| 0.5 * (lo[i] + hi[i]));
    let (mut a, mut b) = (mid, mid);
    (a[1], b[1]) = (lo[1], hi[1]);
    let (pa, pb) = (emcraft_ui_core::tools::screen(&app.view3d.camera, size, a), emcraft_ui_core::tools::screen(&app.view3d.camera, size, b));
    let (Some(pa), Some(pb)) = (pa, pb) else { panic!("the plane's line in view") };
    app.update(Message::View3d(Nav::At(0.0)));
    app.update(Message::View3d(Nav::DragPlane(pb.x - pa.x, pb.y - pa.y)));
    assert!((app.view3d.field.at.unwrap_or(0.0) - (hi[1] - lo[1])).abs() < 1e-9, "{:?}", app.view3d.field.at);
    app.update(Message::View3d(Nav::FacePlane));
    settle(&mut app);
    assert!((app.view3d.camera.pitch.abs().to_degrees() - 90.0).abs() < 1e-6, "facing the H-plane: {:?}", app.view3d.camera);
    // The selected port's reference plane by its number.
    app.update(Message::Row("port:1".into()));
    app.update(Message::Page(emcraft_ui_core::view::View::Model));
    assert!(app.ref_text.as_ref().is_some_and(|(_, t, _)| t == "0"), "{:?}", app.ref_text);
    app.update(Message::RefText("2.5".into()));
    app.update(Message::RefSet);
    let deembed = |app: &App| app.session.model().ports.first().map_or(f64::NAN, |p| p.deembed * 1e3);
    assert!((deembed(&app) - 2.5).abs() < 1e-12, "{:?}", app.error);
    assert!(app.ref_text.as_ref().is_some_and(|(_, t, _)| t == "2.5"));
    app.update(Message::RefZero);
    assert_eq!(deembed(&app), 0.0);
}

/// Every step's panel is drawn for the high-pass filter (its port modes previewed in the
/// Excitation step: TE10 propagates in WR-90 at the band's top), and a kind of analysis chosen
/// on the bar is the project's. Evidence: the waveguide's TE10 cut-off (6.557 GHz) lies below
/// the band's top, so its mode propagates.
#[test]
fn every_steps_panel_and_the_kind() {
    use emcraft_engine::workflow::Step;
    let mut app = App::new(Some("highpass"));
    for step in Step::ALL {
        app.update(Message::Step(Some(step)));
        let _ = app.view();
        assert_eq!(app.workflow.step, (step != Step::Intent).then_some(step), "{step:?}");
        assert!(crate::step_panel::view(&app).is_some() == (step != Step::Intent));
    }
    app.update(Message::Step(Some(Step::Excitation)));
    app.update(Message::StepAct(crate::step_panel::Act::PreviewModes));
    finish(&mut app);
    let reply = app.replies.port_modes.as_ref().unwrap_or_else(|| panic!("port modes: {:?}", app.error));
    assert!(reply.ports.iter().all(|p| p.modes.first().is_some_and(|m| m.propagating)), "TE10 propagates");
    let _ = app.view();
    app.update(Message::Run(emcraft_ui_core::system::gesture::Command { id: "project.analysis", params: json!({"kind": "eigenmodes"}) }));
    assert_eq!(app.solver(), emcraft_ui_core::steps::Solver::Eigenmodes);
    app.update(Message::Step(Some(Step::Setup)));
    assert!(crate::step_panel::view(&app).is_some());
}

/// Hostile text in the Setup form never crashes: each is refused under its field (or read as
/// the number it is), Confirm runs nothing it cannot take, and the model stays as it was.
#[test]
fn hostile_setup_input_is_refused_in_place() {
    use crate::step_panel::Act;
    use emcraft_engine::workflow::Step;
    let mut app = App::new(Some("highpass"));
    app.update(Message::Step(Some(Step::Setup)));
    let before = app.session.model().clone();
    let long = "9".repeat(2 * emcraft_ui_core::dialogs::MAX_FIELD);
    for bad in ["", "NaN", "inf", "-1", "1e400", "0", "abc", "1e-400", "١٢", long.as_str(), "18446744073709551616"] {
        for name in emcraft_ui_core::steps::form::NAMES {
            app.update(Message::StepAct(Act::Typed(name, bad.to_owned())));
        }
        app.update(Message::StepAct(Act::Confirm));
        let _ = app.view();
        assert!(app.workflow.fields.text("start").chars().count() <= emcraft_ui_core::dialogs::MAX_FIELD, "the text is bounded");
    }
    assert_eq!(app.session.model().job, before.job, "nothing hostile was confirmed");
    assert!(!app.workflow.fields.errors.is_empty());
}

/// The Boundaries step finds the model's mirror planes. Analytic evidence: the high-pass
/// filter's rectangular guide, filled section by section across its whole width and height, is
/// mirror symmetric across x = a/2 and y = b/2 (its ports' TE10 too).
#[test]
fn the_boundaries_step_finds_the_mirror_planes() {
    let mut app = App::new(Some("highpass"));
    app.update(Message::Step(Some(emcraft_engine::workflow::Step::Boundaries)));
    let found = app.workflow.symmetry.as_ref().map(|(_, f, _)| f.iter().map(|f| f.axis.id()).collect::<Vec<_>>()).unwrap_or_default();
    assert!(found.contains(&"x") && found.contains(&"y"), "{found:?}");
    assert!(crate::step_panel::view(&app).is_some());
}

/// The S-parameters page. Analytic evidence: a band picked over 7–8 GHz refined with 5 points
/// adds the frequencies 7, 7.25, 7.5, 7.75 and 8 GHz to the sweep (merged, none twice); a click
/// at 7.3 GHz opens the field at the nearest solved frequency; the table has a row per solved
/// frequency; a reference plane set to 1 mm shifts arg S11 by 2β·1 mm (S follows at once).
#[test]
fn the_sparameters_page_refines_picks_and_compares() {
    use crate::sparameters::SAct;
    use emcraft_ui_core::view::View;
    let mut app = App::new(Some("highpass"));
    for points in [7, 4] {
        app.run_now("solve.frequency", &json!({"start": 6, "stop": 9, "points": points}));
        finish(&mut app);
    }
    app.update(Message::Page(View::SParameters));
    let solved = |app: &App| app.session.results().sweep.as_ref().map_or(Vec::new(), |s| s.results.iter().map(|r| r.frequency).collect::<Vec<_>>());
    assert_eq!(solved(&app).len(), 4);
    // Two solves kept: the older drawn over the chart, then taken off.
    let older = app.session.archive().first().map_or(0, |a| a.id);
    app.update(Message::S(SAct::Compare(older)));
    assert_eq!(app.sview.compare, vec![older]);
    assert!(matches!(app.charts.page.as_ref().map(|p| &p.shown), Some(Ok(_))));
    app.update(Message::S(SAct::Compare(older)));
    assert!(app.sview.compare.is_empty());
    // A band refined.
    app.update(Message::S(SAct::Band(Some((7e9, 8e9)))));
    app.update(Message::S(SAct::Points(5)));
    app.update(Message::S(SAct::Refine));
    finish(&mut app);
    let after = solved(&app);
    for f in [7.0e9, 7.25e9, 7.5e9, 7.75e9, 8.0e9] {
        assert!(after.iter().any(|x| (x - f).abs() < 1.0), "{f} solved: {after:?} {:?}", app.error);
    }
    assert!(app.sview.band.is_none());
    // The table: a row per solved frequency.
    app.update(Message::S(SAct::Table(true)));
    let table = emcraft_ui_core::results::sparameters::table(&app.session).unwrap_or_else(|| panic!("a table"));
    assert_eq!(table.rows.len(), after.len());
    let _ = app.view();
    // A click: the nearest solved frequency's field on the model page.
    app.update(Message::S(SAct::Pick(7.3e9)));
    assert_eq!(app.page, View::Model);
    assert!((app.view3d.field.frequency - 7.25e9).abs() < 1.0, "{}", app.view3d.field.frequency);
    // A reference plane typed and set: S11 turns by 2β·d, β = √(k²εr − (π/a)²) of TE10 in the
    // port's filling (εr 2, a 22.86 mm), |S11| unchanged.
    let s11 = |app: &App| {
        app.session.results().sweep.as_ref().and_then(|s| s.results.iter().find(|r| (r.frequency - 9e9).abs() < 1.0)).and_then(|r| r.s.get(0, 0))
    };
    let before = s11(&app).unwrap_or_else(|| panic!("S11 at 9 GHz"));
    app.update(Message::Page(View::SParameters));
    app.update(Message::S(SAct::Plane("1".into(), "1".into())));
    app.update(Message::S(SAct::SetPlane("1".into())));
    assert!((app.session.model().ports[0].deembed - 1e-3).abs() < 1e-12, "{:?}", app.error);
    assert_eq!(app.planes.get("1").map(|(s, _)| s.as_str()), Some("1"));
    let after = s11(&app).unwrap_or_else(|| panic!("S11 at 9 GHz"));
    let k = 2.0 * std::f64::consts::PI * 9e9 / emcraft_model::units::C0;
    let beta = (k * k * 2.0 - (std::f64::consts::PI / 22.86e-3).powi(2)).sqrt();
    let turned = (after.arg() - before.arg()).rem_euclid(std::f64::consts::TAU);
    let want = (2.0 * beta * 1e-3).rem_euclid(std::f64::consts::TAU);
    assert!((after.norm() - before.norm()).abs() < 1e-9, "|S11| kept");
    assert!((turned - want).abs() < 1e-6 || (turned - (std::f64::consts::TAU - want)).abs() < 1e-6, "turned {turned}, 2βd {want}");
}

/// The field view's side column. Analytic evidence: the high-pass filter is lossless, so the
/// power its S entries carry is all the incident power (|S11|² + |S21|² = 1): what is not
/// accounted for is within the solve's error. A probe added at the pinned point reads the field
/// there; its dialogs save the picture and the animation of the view as it stands.
#[test]
fn the_field_column_reads_power_probes_and_a_pinned_point() {
    use crate::view3d::Nav;
    use emcraft_ui_core::scene3d::Overlay;
    let mut app = App::new(Some("highpass"));
    app.run_now("solve.frequency", &json!({"start": 6, "stop": 9, "points": 4}));
    finish(&mut app);
    app.update(Message::View3d(Nav::Size(800.0, 600.0)));
    app.update(Message::View3d(Nav::Overlay(Overlay::Field)));
    app.update(Message::View3d(Nav::Frequency(8e9)));
    let report = emcraft_engine::power::at(&app.session, 8e9, 0).unwrap_or_else(|e| panic!("{e}"));
    let lost = report["unaccounted"].as_f64().unwrap_or(f64::NAN);
    assert!(lost.abs() < 1e-3, "lossless: {lost}");
    assert!(crate::field_panel::view(&app).is_some());
    // A click on the plane pins its point.
    let at = (0..60)
        .flat_map(|i| (0..40).map(move |j| emcraft_ui_core::geom::Point::new(200.0 + 7.0 * i as f32, 150.0 + 7.0 * j as f32)))
        .find(|p| app.probe_at(*p).is_some())
        .unwrap_or_else(|| panic!("the plane under the view"));
    app.update(Message::Click(crate::view3d::Click { at, add: false, double: false }));
    assert!(app.pinned.is_some());
    // A probe there, read; then taken away.
    let point = app.probe_point().unwrap_or_else(|| panic!("a point"));
    app.update(Message::Run(emcraft_ui_core::system::gesture::Command { id: "view.probe.add", params: json!({"at": point}) }));
    assert_eq!(app.session.views().probes.len(), 1, "{:?}", app.error);
    let name = app.session.views().probes[0].name.clone();
    let _ = app.view();
    app.update(Message::Run(emcraft_ui_core::system::gesture::Command { id: "view.probe.delete", params: json!({"name": name}) }));
    assert!(app.session.views().probes.is_empty());
    // The picture's dialog, filled with the view as it stands.
    let params = emcraft_ui_core::results::fields::save_params(&app.view3d.field, None, &app.session, "field.png", false);
    app.update(Message::Open("view.field", params));
    assert_eq!(app.dialog.as_ref().map(|d| d.command.id), Some("view.field"));
}

/// The TDR, pulse and far-field pages' choices make their charts. Evidence: the high-pass
/// filter's TDR in band pass is |h(t)| (no negative value); low pass, which needs a harmonic sweep
/// of a line without a cut-off, is refused with why;
/// a pulse sent through is charted as its power, then its instantaneous frequency; a θ cut starts
/// at θ = 90°.
#[test]
fn the_result_pages_choices_make_their_charts() {
    use crate::results::RAct;
    use emcraft_ui_core::view::View;
    let mut app = App::new(Some("highpass"));
    app.run_now("solve.frequency", &json!({"start": 6, "stop": 9, "points": 31}));
    finish(&mut app);
    let chart = |app: &App| app.charts.page.as_ref().and_then(|p| p.shown.as_ref().ok()).cloned();
    app.update(Message::Page(View::Tdr));
    app.update(Message::R(RAct::TdrMode(Some(emcraft_engine::tdr::Mode::BandPass))));
    let band = chart(&app).unwrap_or_else(|| panic!("a band-pass TDR: {:?}", app.charts.page.as_ref().map(|p| p.shown.clone().err())));
    assert!(band.series.iter().flat_map(|s| s.points.iter()).all(|p| p[1] >= -1e-12), "|h(t)| is not negative");
    app.update(Message::R(RAct::TdrMode(Some(emcraft_engine::tdr::Mode::LowPass))));
    // Low pass needs a harmonic sweep (f = kΔf) from a line without a cut-off: refused, why shown.
    let refused = app.charts.page.as_ref().and_then(|p| p.shown.as_ref().err().cloned());
    assert!(refused.is_some(), "a guide's sweep from 6 GHz has no low-pass TDR");
    // The pulse.
    app.update(Message::Page(View::Pulse));
    app.update(Message::R(RAct::PulseRun));
    finish(&mut app);
    let power = chart(&app).unwrap_or_else(|| panic!("the pulse: {:?}", app.error));
    app.update(Message::R(RAct::PulseShow(true)));
    let frequency = chart(&app).unwrap_or_else(|| panic!("its frequency"));
    assert_ne!(power.y.label, frequency.y.label, "power, then frequency");
    // The far field: a θ cut at 90°.
    app.update(Message::Page(View::FarField));
    app.update(Message::R(RAct::Cut(emcraft_engine::view::Cut::Theta)));
    assert_eq!(app.pattern.angle, 90.0);
    let _ = app.view();
}

/// The study page. Analytic evidence: a grid sweep of two variables over two values each has
/// 2 × 2 = 4 points, and its table a row per point; charted along one variable the other is held
/// at one of its two values, and the chart is made again when another is chosen.
#[test]
fn the_study_page_slices_a_grid() {
    use crate::results::RAct;
    use emcraft_ui_core::view::View;
    let mut app = App::new(Some("highpass"));
    let sweep = json!({"variables": [{"name": "er", "values": [2.0, 2.2]}, {"name": "l_gap", "values": [18, 22]}], "solve": {"command": "solve.frequency", "params": {"start": 6, "stop": 9, "points": 3, "order": 1}}});
    app.run_now("study.sweep", &sweep);
    finish(&mut app);
    let study = app.session.results().study.as_ref().unwrap_or_else(|| panic!("a study: {:?}", app.error));
    assert!(study.is_grid() && study.rows.len() == 4, "{} points", study.rows.len());
    app.update(Message::Page(View::Study));
    let table = emcraft_ui_core::results::study::points(&app.session, &app.study).unwrap_or_else(|| panic!("its table"));
    assert_eq!(table.rows.len(), 4);
    let chart = |app: &App| app.charts.page.as_ref().and_then(|p| p.shown.as_ref().ok()).map(|c| c.series.len());
    assert!(chart(&app).is_some(), "{:?}", app.charts.page.as_ref().map(|p| p.shown.clone().err()));
    app.update(Message::R(RAct::Along(1)));
    app.update(Message::R(RAct::Held(0, 1)));
    assert_eq!((app.study.slice.free, app.study.slice.at.first().copied()), (1, Some(1)));
    assert!(chart(&app).is_some());
    let _ = app.view();
}

/// A chart's style card. Evidence: each change is one view.chart.style the chart then shows — a
/// title typed and committed is the chart's, an axis end typed is the axis' end (a text that is no
/// number commits nothing), a curve hidden is left out of the chart, Reset brings it all back.
#[test]
fn the_style_card_edits_the_charts_style() {
    use crate::style_card::StyleAct;
    use emcraft_ui_core::results::style::Field;
    use emcraft_ui_core::view::View;
    let mut app = App::new(Some("highpass"));
    app.run_now("solve.frequency", &json!({"start": 6, "stop": 9, "points": 5}));
    finish(&mut app);
    app.update(Message::Page(View::SParameters));
    app.update(Message::Style(StyleAct::Toggle));
    assert!(crate::style_card::card(&app).is_some());
    let chart = |app: &App| app.charts.page.as_ref().and_then(|p| p.shown.as_ref().ok()).cloned().unwrap_or_else(|| panic!("a chart"));
    app.update(Message::Style(StyleAct::Typed(Field::Title, "Filter".into())));
    app.update(Message::Style(StyleAct::Commit(Field::Title)));
    assert_eq!(chart(&app).title, "Filter");
    app.update(Message::Style(StyleAct::Typed(Field::Min(1), "abc".into())));
    app.update(Message::Style(StyleAct::Commit(Field::Min(1))));
    assert_ne!(chart(&app).y.min, -30.0);
    app.update(Message::Style(StyleAct::Typed(Field::Min(1), "-30".into())));
    app.update(Message::Style(StyleAct::Commit(Field::Min(1))));
    assert_eq!(chart(&app).y.min, -30.0);
    let first = chart(&app).curves().first().map(|(k, _)| (*k).to_owned()).unwrap_or_default();
    app.update(Message::Style(StyleAct::Send(emcraft_ui_core::results::style::params(
        emcraft_engine::views::ChartId::SParameters,
        json!({"hidden": [first.clone()]}),
    ))));
    assert!(chart(&app).series.iter().all(|s| s.key != first), "{first} left out");
    app.update(Message::Style(StyleAct::Send(emcraft_ui_core::results::style::params(
        emcraft_engine::views::ChartId::SParameters,
        json!({"reset": true}),
    ))));
    assert_ne!(chart(&app).title, "Filter");
    let _ = app.view();
}

/// Command search: `/` focuses it; a query lists the commands that match, one picked opens as
/// its ribbon button would (its dialog); Enter on a command written with its parameters runs it;
/// JSON that does not read keeps the query and says why.
#[test]
fn the_command_search_finds_opens_and_runs() {
    let mut app = App::new(Some("highpass"));
    let slash = iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Character("/".into()),
        modified_key: iced::keyboard::Key::Character("/".into()),
        physical_key: iced::keyboard::key::Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::empty(),
        text: None,
        repeat: false,
    });
    let focus = crate::view3d::shader::keys(slash, iced::event::Status::Ignored, iced::window::Id::unique());
    assert!(matches!(focus, Some(Message::FocusSearch)));
    app.update(Message::Search("material.create".into()));
    assert!(emcraft_ui_core::search::matches(&app.search).iter().any(|c| c.id == "model.material.create"));
    let _ = app.view();
    app.update(Message::SearchPick(Some("model.material.create")));
    assert_eq!(app.dialog.as_ref().map(|d| d.command.id), Some("model.material.create"));
    assert!(app.search.is_empty());
    app.update(Message::Dialog(crate::dialog::Edit::Cancel));
    app.update(Message::Search(r#"units.set {"length": "m"}"#.into()));
    app.update(Message::SearchPick(None));
    assert_eq!(app.session.display().length.symbol(), "m", "{:?}", app.error);
    app.update(Message::Search("units.set {oops".into()));
    app.update(Message::SearchPick(None));
    assert!(app.error.as_deref().is_some_and(|e| e.starts_with("not JSON")) && !app.search.is_empty());
}

/// The browser: a section folds and unfolds; a row's eye hides what it names
/// (view.visibility) and shows it again; a material's row takes the workflow to the geometry's
/// step; Delete deletes the selected item (undoable).
#[test]
fn the_browser_folds_hides_and_deletes() {
    use emcraft_engine::visibility::DisplayObject;
    let mut app = App::new(Some("highpass"));
    app.update(Message::Fold("Bodies".into()));
    assert_eq!(app.folded, vec!["Bodies".to_owned()]);
    let _ = app.view();
    app.update(Message::Fold("Bodies".into()));
    assert!(app.folded.is_empty());
    let vacuum = DisplayObject::Material("vacuum".into());
    app.update(Message::Run(emcraft_ui_core::system::gesture::Command {
        id: "view.visibility",
        params: json!({"kind": vacuum.kind(), "name": vacuum.name(), "visible": false}),
    }));
    assert!(!app.session.views().visible(&vacuum));
    let materials = app.session.model().materials.len();
    app.update(Message::Row("material:ptfe-like".into()));
    assert_eq!(app.workflow.step, Some(emcraft_engine::workflow::Step::Geometry));
    // ptfe-like fills sections: deleting it is refused, the model as it was.
    app.update(Message::DeleteKey);
    assert_eq!(app.session.model().materials.len(), materials);
    assert!(app.error.as_deref().is_some_and(|e| e.contains("used by")), "{:?}", app.error);
    // A material of its own goes.
    app.update(Message::Run(emcraft_ui_core::system::gesture::Command { id: "model.material.create", params: json!({"name": "spare", "eps_r": 3}) }));
    assert_eq!(app.session.model().materials.len(), materials + 1);
    app.update(Message::Row("material:spare".into()));
    app.update(Message::DeleteKey);
    assert_eq!(app.session.model().materials.len(), materials, "{:?}", app.error);
}

/// A template's card: its parameters edited before it is built (one that is no number keeps
/// Open from building it), the project then built with them; the purpose chosen on the start
/// page is the project's analysis. Evidence: the high-pass filter built with a = 25 mm has a
/// guide 25 mm wide; the cavity opened for eigenmodes is analysed for eigenmodes.
#[test]
fn a_templates_card_builds_it_with_its_parameters() {
    use crate::template_dialog::TemplateAct;
    let mut app = App::new(None);
    app.update(Message::Template(TemplateAct::Open(Some("highpass"))));
    let card = app.template.card.clone().unwrap_or_else(|| panic!("the card"));
    let a = card.fields.iter().position(|f| f.name == "a").unwrap_or_else(|| panic!("a parameter a: {:?}", card.fields));
    app.update(Message::Template(TemplateAct::Field(a, "wide".into())));
    assert!(app.template.card.as_ref().is_some_and(|c| c.values().is_err()));
    app.update(Message::Template(TemplateAct::Build));
    assert!(app.template.card.is_some(), "not built with a word for a number");
    let _ = app.view();
    app.update(Message::Template(TemplateAct::Field(a, "25".into())));
    app.update(Message::Template(TemplateAct::Build));
    assert!(app.template.card.is_none());
    let width = app.session.model().waveguide().map_or(0.0, |w| w.width.get());
    assert!((width - 25e-3).abs() < 1e-12, "{width} {:?}", app.error);
    // A purpose chosen, a template made for it opened for it.
    app.update(Message::Purpose(Some(emcraft_ui_core::steps::Solver::Eigenmodes)));
    app.update(Message::Template(TemplateAct::Open(Some("cavity"))));
    app.update(Message::Template(TemplateAct::Build));
    assert_eq!(app.solver(), emcraft_ui_core::steps::Solver::Eigenmodes);
    assert_eq!(app.session.model().job.chosen(), Some(emcraft_ui_core::steps::Solver::Eigenmodes));
}

/// Escape closes the top layer open, one a press: the dialog over the settings, then the
/// settings, the tool, the selection; a field outside them that took the key closes nothing.
#[test]
fn escape_closes_the_top_layer_first() {
    use emcraft_ui_core::layer::Layer;
    use emcraft_ui_core::tools::Tool;
    let mut app = App::new(Some("highpass"));
    app.update(Message::Row("port:1".into()));
    app.update(Message::Tool(Some(Tool::Measure)));
    app.update(Message::Settings(true));
    app.update(Message::Command("model.material.create".into()));
    let top = |app: &App| crate::escape::stack(app).first().copied();
    assert_eq!(crate::escape::stack(&app), [Layer::Dialog, Layer::Settings, Layer::Tool, Layer::Selection]);
    // The dialog's own field took the key: the dialog closes all the same.
    app.update(Message::Escape(true));
    assert_eq!((app.dialog.is_none(), top(&app)), (true, Some(Layer::Settings)));
    app.update(Message::Escape(false));
    assert_eq!(top(&app), Some(Layer::Tool));
    // A field elsewhere let go of the key: the tool stays.
    app.update(Message::Escape(true));
    assert_eq!(app.tool, Some(Tool::Measure));
    app.update(Message::Escape(false));
    assert_eq!(top(&app), Some(Layer::Selection));
    app.update(Message::Escape(false));
    assert_eq!(crate::escape::stack(&app), []);
    // The search typed in clears; nothing open, Escape does nothing.
    app.update(Message::Search("mesh".into()));
    app.update(Message::Escape(true));
    assert_eq!(app.search, "");
    app.update(Message::Escape(false));
    assert_eq!(app.error, None);
}

/// Frames until `app` has played its recording and its jobs are done (bounded).
fn played(app: &mut App) {
    let began = std::time::Instant::now();
    while (app.job.is_some() || app.recorder.playing.is_some()) && began.elapsed().as_secs() < 120 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.update(Message::Tick(std::time::Instant::now()));
    }
    assert!(app.recorder.playing.is_none() && app.job.is_none(), "played to the end");
}

/// A session recorded — a sample opened (a job), a material made through its dialog, a mesh
/// generated, another page — and played back in another window gives the same project (its
/// history, command for command) on the same page; a recording that is none is refused.
#[test]
fn a_recorded_session_plays_back_to_the_same_project() {
    let mut app = App::new(None);
    app.start_recording();
    app.start("file.sample", json!({"name": "highpass"}));
    finish(&mut app);
    app.update(Message::Command("model.material.create".into()));
    if let Some(d) = app.dialog.as_mut() {
        let _ = d.set("name", &json!("ceramic"));
        let _ = d.set("eps_r", &json!(9.8));
    }
    app.update(Message::Dialog(crate::dialog::Edit::Ok));
    app.start("mesh.generate", json!({"size": 6}));
    finish(&mut app);
    app.update(Message::Page(emcraft_ui_core::view::View::Model));
    let r = app.stop_recording().unwrap_or_else(|| panic!("recorded"));
    assert_eq!(r.entries.len(), 4, "{:?}", r.entries.iter().map(|e| &e.action).collect::<Vec<_>>());
    let text = serde_json::to_string(&r).unwrap_or_default();
    let mut again = App::new(None);
    let started = emcraft_ui_core::record::Recording::parse(&text).and_then(|r| again.play(r));
    assert!(started.is_ok(), "{started:?}");
    played(&mut again);
    let history = |a: &App| serde_json::to_value(a.session.history()).unwrap_or_default();
    assert_eq!(history(&again), history(&app));
    assert_eq!(again.page, app.page);
    assert!(again.log.last().is_some_and(|l| l.text == "The recording is played."), "{:?}", again.log.last());
    for bad in ["", "[]", r#"{"format":"emcraft-recording","version":2,"history":[],"entries":[]}"#] {
        assert!(emcraft_ui_core::record::Recording::parse(bad).and_then(|r| again.play(r)).is_err(), "{bad}");
    }
}

/// Crash reports kept in a folder: the journal begun there notes what runs; a report asked for
/// shows on the card, Escape closes it, Delete takes it away; without a folder the buttons say
/// so.
#[test]
fn crash_reports_are_kept_shown_and_deleted() {
    use crate::record::ReportAct;
    let base = std::env::var_os("EMCRAFT_SCRATCH").map_or_else(std::env::temp_dir, std::path::PathBuf::from);
    let dir = base.join(format!("iced-crashes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut app = App::new(Some("highpass")).with_crash_reports(Some(dir.clone()), false);
    let journal = app.crashes.journal.as_ref().map(|j| j.path().to_path_buf()).unwrap_or_default();
    app.run_now("model.material.create", &json!({"name": "ceramic", "eps_r": 9.8}));
    let noted = emcraft_ui_core::crash::read_journal(&journal).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(noted.entries.len(), 1);
    app.update(Message::Reports(ReportAct::Bug));
    app.update(Message::Reports(ReportAct::Show(true)));
    assert_eq!(app.crashes.shown.as_ref().map(Vec::len), Some(1));
    // The report plays the session back: the sample's history, then the material.
    let report = app.crashes.shown.as_ref().and_then(|r| r.first()).map(|r| r.folder.clone()).unwrap_or_default();
    let recording = emcraft_ui_core::record::Recording::read(&report).unwrap_or_else(|e| panic!("{e}"));
    assert!(recording.history.as_array().is_some_and(|h| !h.is_empty()) && recording.entries.len() == 1);
    assert_eq!(crate::escape::stack(&app).first(), Some(&emcraft_ui_core::layer::Layer::Reports));
    let id = app.crashes.shown.as_ref().and_then(|r| r.first()).map(|r| r.id.clone()).unwrap_or_default();
    app.update(Message::Reports(ReportAct::Delete(id)));
    assert_eq!(app.crashes.shown.as_ref().map(Vec::len), Some(0));
    app.update(Message::Escape(false));
    assert!(app.crashes.shown.is_none());
    // A clean close leaves no journal behind.
    drop(app);
    assert!(!journal.exists());
    let _ = std::fs::remove_dir_all(&dir);
    let mut off = App::new(None);
    off.update(Message::Reports(ReportAct::Bug));
    assert!(off.error.as_deref().is_some_and(|e| e.contains("no folder")), "{:?}", off.error);
}

/// A language no font shows is said once (the window stays in English), with what to install;
/// said again only after the language or the font file changes.
#[test]
fn a_missing_font_is_said_once() {
    use emcraft_ui_core::fonts::State;
    let app = App::new(None).with_font(State::Missing(12)).with_font(State::Missing(12));
    let said: Vec<_> = app.log.lines().iter().filter(|l| l.text.contains("12 characters missing")).collect();
    assert_eq!((said.len(), said.first().map(|l| (l.error, l.count))), (1, Some((true, 1))));
    assert_eq!(app.font.text(), "missing (12 characters)");
    let fine = App::new(None).with_font(State::Found("Noto Sans CJK JP".into()));
    assert!(fine.log.lines().is_empty() && fine.font.text() == "Noto Sans CJK JP");
}

/// `ui.render` draws the window without one: on the CPU at the size asked, the 3D view drawn
/// offscreen into its place (more than the ground's two colours there); a size out of bounds is
/// refused.
#[test]
fn the_window_renders_without_a_window() {
    let _gpu = gpu();
    let mut app = App::new(Some("highpass"));
    let dir = std::env::var_os("EMCRAFT_SCRATCH").map_or_else(std::env::temp_dir, std::path::PathBuf::from);
    let path = dir.join(format!("iced-render-{}.png", std::process::id()));
    let reply = crate::scenario::call(&mut app, "ui.render", json!({"path": path.display().to_string(), "size": [1000, 700]}))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((reply["ok"].clone(), reply["result"]["width"].clone(), reply["result"]["height"].clone()), (json!(true), json!(1000), json!(700)));
    let (w, h, rgba) = crate::headless::render(&mut app, (1000.0, 700.0)).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((w, h, rgba.len()), (1000, 700, 1000 * 700 * 4));
    // The middle of the 3D view: the guide drawn, not only the ground.
    let colours: std::collections::BTreeSet<[u8; 4]> = (300..500)
        .flat_map(|y| (500..800).map(move |x| (y * 1000 + x) * 4))
        .filter_map(|i| rgba.get(i..i + 4).and_then(|p| <[u8; 4]>::try_from(p).ok()))
        .collect();
    assert!(colours.len() > 50, "the 3D view drawn: {} colours", colours.len());
    assert!(!app.headless);
    let bad = crate::scenario::call(&mut app, "ui.render", json!({"size": [0, 10]})).unwrap_or_default();
    assert_eq!(bad["ok"], json!(false));
    if std::env::var_os("KEEP_RENDER").is_none() {
        let _ = std::fs::remove_file(path);
    }
}

/// The round trip with SolidCraft (docs/adr/0010-geometry-from-solidcraft.md), SolidCraft itself
/// stood in for by writing the file: a design of the user's imported is a row that opens it in
/// SolidCraft (a program that is not there said so) and its parameters are rows of their own;
/// saved again, the model is read again; a parameter set here is set where the design is
/// imported; a new design is written empty and imported once something saves it. Analytic
/// evidence: the taper's z extent is its parameter `l` (mm).
#[test]
fn a_design_goes_to_solidcraft_and_comes_back() {
    use crate::dialog::Edit;
    use emcraft_ui_core::files::{FileAction, Purpose};
    let dir = std::env::var_os("EMCRAFT_SCRATCH")
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from)
        .join(format!("iced-designs-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let taper: &[u8] = include_bytes!("../../templates/designs/taper.solidcraft");
    let write = |path: &std::path::Path, l: &str| {
        let mut d: serde_json::Value = serde_json::from_slice(taper).unwrap_or_default();
        for p in d["params"].as_array_mut().into_iter().flatten() {
            if p["name"] == "l" {
                p["expr"] = json!(l);
            }
        }
        let _ = std::fs::write(path, serde_json::to_vec(&d).unwrap_or_default());
        // Written later than before, whatever the clock's grain.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
        let _ = std::fs::File::options().write(true).open(path).and_then(|f| f.set_modified(later));
    };
    let path = dir.join("taper.solidcraft");
    write(&path, "30");
    let p = path.display().to_string();
    let mut app = App::new(None);
    app.prefs.solidcraft = "/nonexistent/solidcraft".into();
    app.start("geometry.import", json!({"path": p, "materials": {"Body1": "interior"}}));
    finish(&mut app);
    let length = |app: &App| app.session.model().bounds().map(|b| (b.max.z - b.min.z) * 1e3);
    assert!(length(&app).is_some_and(|l| (l - 30.0).abs() < 1e-6), "{:?} {:?}", length(&app), app.error);
    let handles: Vec<String> = crate::control::handles(&app).into_iter().map(|(h, _)| h).collect();
    assert!(handles.contains(&format!("solidcraft:{p}")) && handles.contains(&format!("row:design:{p}:l")), "{handles:?}");
    app.update(Message::OpenDesign(p.clone()));
    assert!(app.error.as_deref().is_some_and(|e| e.contains("/nonexistent/solidcraft")), "{:?}", app.error);
    // Saved again in SolidCraft: read again.
    write(&path, "40");
    app.update(Message::Watch);
    finish(&mut app);
    assert!(length(&app).is_some_and(|l| (l - 40.0).abs() < 1e-6), "{:?}", length(&app));
    // Set here.
    app.update(Message::Row(format!("design:{p}:l")));
    assert_eq!(app.dialog.as_ref().map(|d| d.command.id), Some("geometry.design"));
    let value = app.dialog.as_ref().and_then(|d| d.fields.iter().position(|f| f.name == "value")).unwrap_or_else(|| panic!("its value"));
    app.update(Message::Dialog(Edit::Text(value, "35".into())));
    app.update(Message::Dialog(Edit::Ok));
    finish(&mut app);
    assert!(length(&app).is_some_and(|l| (l - 35.0).abs() < 1e-6), "{:?} {:?}", length(&app), app.error);
    // A new design: written empty, imported once saved.
    let mut blank = App::new(None);
    blank.prefs.solidcraft = "/nonexistent/solidcraft".into();
    let fresh = dir.join("new");
    blank.update(Message::File(FileAction::Chosen(Purpose::NewDesign, Some(fresh.display().to_string()))));
    let made = dir.join("new.solidcraft");
    assert_eq!(std::fs::read_to_string(&made).ok().as_deref(), Some(crate::designs::EMPTY));
    let inspected = blank.session.execute("geometry.import.inspect", &json!({"path": made.display().to_string()}));
    assert!(inspected.is_ok(), "the empty design reads: {inspected:?}");
    blank.update(Message::Watch);
    assert!(blank.job.is_none() && blank.session.model().bounds().is_none(), "nothing saved yet");
    write(&made, "20");
    blank.update(Message::Watch);
    finish(&mut blank);
    assert!(blank.session.model().bounds().is_some(), "imported: {:?}", blank.error);
    let _ = std::fs::remove_dir_all(&dir);
}

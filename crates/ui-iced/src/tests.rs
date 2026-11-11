//! The window driven as a person would: its messages (typing, dragging, buttons) each changing
//! the chain as the egui window's do, and the view drawn headless and clicked.

use emcraft_model::system::RECEIVER;
use emcraft_ui_core::geom::Point;
use emcraft_ui_core::system::gesture::Command;
use serde_json::json;

use crate::app::{App, BAND, Field, Message, Pointer};

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
    let mut app = App::new(None);
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
        let sample = App::new(Some("adsb-receiver"));
        let mut ui = iced_test::Simulator::with_size(iced::Settings::default(), (1400.0, 1300.0), sample.view());
        let shot = ui.snapshot(&iced::Theme::Light).unwrap_or_else(|e| panic!("{e}"));
        let path = std::path::Path::new(&dir).join("system");
        let _ = std::fs::remove_file(path.with_extension("png"));
        assert!(shot.matches_image(&path).unwrap_or(false));
    }
}

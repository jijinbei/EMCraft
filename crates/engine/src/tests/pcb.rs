use super::*;
use serde_json::json;
#[test]
fn board_import_is_embedded_saved_undoable_and_diagnostics_are_explicit() {
    let mut s = Session::new();
    let path = scratch("routing.kicad_pcb");
    std::fs::write(&path, include_str!("../../../io/tests/fixtures/route.kicad_pcb")).unwrap();
    s.execute("pcb.import", &json!({"path":path})).unwrap();
    assert_eq!(s.history().last().unwrap().command, "pcb.load");
    std::fs::remove_file(path).unwrap();
    let report = s.execute("pcb.inspect", &json!({"net":"DATA"})).unwrap();
    assert_eq!(report["model_available"], true);
    assert_eq!(report["si"]["method"], "analytical_transmission_line");
    assert_eq!(report["sections"].as_array().unwrap().len(), 2);
    let project = scratch("pcb.emcraft");
    s.execute("file.save", &json!({"path":project})).unwrap();
    let mut t = Session::new();
    t.execute("file.open", &json!({"path":project})).unwrap();
    assert_eq!(s.model().pcb, t.model().pcb);
    assert_eq!(t.views().pcb_net.as_deref(), Some("DATA"));
    t.execute("pcb.sample", &json!({"split":true})).unwrap();
    let bad = t.execute("pcb.inspect", &json!({"net":"DATA"})).unwrap();
    assert!(bad["si"].is_null());
    assert_eq!(bad["model_available"], false);
    t.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.model().pcb, t.model().pcb);
    let _ = std::fs::remove_file(project);
}
#[test]
fn board_commands_refuse_hostile_inputs_and_keep_the_model() {
    let mut s = Session::new();
    s.execute("pcb.sample", &json!({"split":false})).unwrap();
    let before = s.model().clone();
    for p in [
        json!({"net":"missing"}),
        json!({"net":"DATA","ground":"DATA"}),
        json!({"net":"DATA","reference":0}),
        json!({"net":"DATA","points":100000}),
        json!({"net":"DATA","stop":1e100}),
    ] {
        assert!(s.execute("pcb.inspect", &p).is_err());
        assert_eq!(s.model(), &before);
    }
    let mut board = s.model().pcb.clone().unwrap();
    board.segments[0].width = -1.0;
    assert!(s.execute("pcb.load", &json!({"data":board})).is_err());
    assert_eq!(s.model(), &before);
    s.execute("units.set", &json!({"length":"m","frequency":"MHz"})).unwrap();
    let r = s.execute("pcb.inspect", &json!({"net":"DATA","stop":1000})).unwrap();
    assert!((r["length"].as_f64().unwrap() - 0.04).abs() < 1e-12);
    assert_eq!(r["si"]["points"][63]["frequency"], 1000.0);
}

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

#[test]
fn serialize_load_round_trip() {
    let mut s = Session::new().unwrap();
    run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 50, "height": 50}),
    );
    run(
        &mut s,
        "variable.create",
        json!({"name": "c", "value": "#FF0000"}),
    );
    let data = run(&mut s, "document.serialize", Value::Null)["data"].clone();
    let before = s.snapshot();
    let mut other = Session::new().unwrap();
    run(&mut other, "document.load", json!({"data": data}));
    assert_eq!(*other.snapshot(), *before);
    assert!(other
        .dispatch("document.load", &json!({"data": "aGVsbG8="}))
        .is_err());
}

#[test]
fn import_and_export_through_commands() {
    let mut s = Session::new().unwrap();
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="20" height="20" fill="#123456"/></svg>"##;
    let id = run(&mut s, "import.svg", json!({"data": svg, "x": 5, "y": 5}))["id"]
        .as_u64()
        .unwrap();
    s.snapshot().validate().unwrap();
    let out = run(&mut s, "export.svg", json!({"id": id}));
    assert!(out["svg"].as_str().unwrap().contains("#123456"));
    let png = run(&mut s, "export.png", json!({"id": id, "scale": 2}));
    assert_eq!(png["width"], 40);
    let pdf = run(&mut s, "export.pdf", json!({"ids": [id]}));
    assert_eq!(pdf["pages"], 1);
    run(&mut s, "edit.undo", Value::Null);
    assert!(s
        .snapshot()
        .get(deesainer_engine::doc::NodeId(id))
        .is_none());
}

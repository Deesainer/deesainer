#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::doc::NodeId;
use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

fn size(s: &Session, id: u64) -> (f64, f64) {
    let r = s.doc().get(NodeId(id)).unwrap().rect;
    (r.width, r.height)
}

#[test]
fn instances_follow_their_main_component_until_resized() {
    let mut s = Session::new().unwrap();
    let bg = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 0, "y": 0, "width": 100, "height": 40}),
    ));
    let label = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 0, "y": 0, "width": 60, "height": 20}),
    ));
    let main = id(&run(
        &mut s,
        "component.create",
        json!({"ids": [bg, label]}),
    ));
    let a = id(&run(
        &mut s,
        "component.instantiate",
        json!({"component": main, "x": 0, "y": 200}),
    ));
    let b = id(&run(
        &mut s,
        "component.instantiate",
        json!({"component": main, "x": 0, "y": 300}),
    ));
    run(
        &mut s,
        "node.resize",
        json!({"id": b, "width": 300, "height": 40}),
    );
    run(
        &mut s,
        "node.resize",
        json!({"id": main, "width": 160, "height": 50}),
    );
    assert_eq!(size(&s, a), (160.0, 50.0));
    assert_eq!(size(&s, b), (300.0, 40.0));
    run(&mut s, "layout.addAutoLayout", json!({"ids": [main]}));
    let hugged = size(&s, main);
    assert_ne!(hugged, (160.0, 50.0));
    assert_eq!(size(&s, a), hugged);
    assert_eq!(size(&s, b), (300.0, 40.0));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(size(&s, main), (160.0, 50.0));
    assert_eq!(size(&s, a), (160.0, 50.0));
}

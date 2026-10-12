#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

#[test]
fn build_and_simulate_a_flow() {
    let mut s = Session::new().unwrap();
    let home = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 200, "height": 400, "name": "Home"}),
    ));
    let detail = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 300, "y": 0, "width": 200, "height": 400, "name": "Detail"}),
    ));
    let button = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"parent": home, "x": 20, "y": 300, "width": 160, "height": 48}),
    ));
    let back = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"parent": detail, "x": 10, "y": 10, "width": 30, "height": 30}),
    ));
    run(
        &mut s,
        "prototype.addInteraction",
        json!({"id": button, "trigger": "click", "action": "navigate", "to": detail, "transition": "push", "direction": "left"}),
    );
    run(
        &mut s,
        "prototype.addInteraction",
        json!({"id": back, "trigger": "click", "action": "back"}),
    );
    run(
        &mut s,
        "prototype.addInteraction",
        json!({"id": detail, "trigger": "delay", "delay": 2000, "action": "url", "url": "https://example.com"}),
    );
    run(&mut s, "prototype.setFlowStart", json!({"frame": home}));
    let map = run(&mut s, "prototype.list", Value::Null);
    assert_eq!(map["starts"], json!([home]));
    assert_eq!(map["interactions"].as_array().unwrap().len(), 3);
    let sim = run(
        &mut s,
        "prototype.simulate",
        json!({"events": [
            {"type": "click", "x": 100, "y": 320},
            {"type": "wait", "ms": 2500},
            {"type": "click", "x": 20, "y": 20}
        ], "preview": true}),
    );
    assert_eq!(sim["visited"], json!([home, detail, home]));
    assert_eq!(sim["urls"], json!(["https://example.com"]));
    assert_eq!(sim["current"], home);
    assert!(sim["preview"]["data"].as_str().unwrap().len() > 100);
    assert!(s
        .dispatch("prototype.addInteraction", &json!({"id": button, "trigger": "click", "action": "url", "url": "javascript:alert(1)"}))
        .is_err());
    run(&mut s, "prototype.removeInteraction", json!({"id": button}));
    assert!(s
        .doc()
        .get(deesainer_engine::doc::NodeId(button))
        .unwrap()
        .interactions
        .is_empty());
}

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

fn component(s: &mut Session, name: &str, x: f64, fill: &str) -> u64 {
    let r = id(&run(
        s,
        "node.create.rectangle",
        json!({"x": x, "y": 0, "width": 40, "height": 40, "fill": fill}),
    ));
    let c = id(&run(s, "component.create", json!({"ids": [r]})));
    run(s, "node.rename", json!({"id": c, "name": name}));
    c
}

#[test]
fn library_updates_are_reviewed_and_applied_selectively() {
    let mut lib = Session::new().unwrap();
    let button = component(&mut lib, "Button", 0.0, "#0D99FF");
    let _badge = component(&mut lib, "Badge", 100.0, "#F24E1E");
    run(
        &mut lib,
        "library.publish",
        json!({"note": "First release"}),
    );
    let v1 = run(&mut lib, "document.serialize", json!({}))["data"].clone();
    let mut app = Session::new().unwrap();
    run(
        &mut app,
        "library.import",
        json!({"data": v1, "library": "UI"}),
    );
    let local_button = app
        .snapshot()
        .nodes()
        .find(|n| n.name == "Button" && n.origin.is_some())
        .map(|n| n.id)
        .unwrap();
    run(
        &mut app,
        "component.instantiate",
        json!({"component": local_button.0, "x": 0, "y": 300}),
    );
    let rect = lib.snapshot().get(NodeId(button)).unwrap().children[0];
    run(
        &mut lib,
        "node.setProps",
        json!({"ids": [rect.0], "fill": "#14AE5C"}),
    );
    component(&mut lib, "Avatar", 200.0, "#9747FF");
    run(
        &mut lib,
        "library.publish",
        json!({"note": "Green buttons"}),
    );
    let v2 = run(&mut lib, "document.serialize", json!({}))["data"].clone();
    let review = run(
        &mut app,
        "library.changes",
        json!({"data": v2, "library": "UI"}),
    );
    let by_name = |n: &str| {
        review["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == json!(n))
            .cloned()
            .unwrap()
    };
    assert_eq!(by_name("Button")["status"], json!("changed"));
    assert_eq!(by_name("Button")["changed"], json!(1));
    assert_eq!(by_name("Button")["instances"], json!(1));
    assert_eq!(by_name("Badge")["status"], json!("unchanged"));
    assert_eq!(review["available"][0]["name"], json!("Avatar"));
    assert_eq!(
        review["published"]["label"],
        json!("Published: Green buttons")
    );
    let badge_source = by_name("Badge")["source"].clone();
    run(
        &mut app,
        "library.update",
        json!({"data": v2, "library": "UI", "components": [badge_source]}),
    );
    let again = run(
        &mut app,
        "library.changes",
        json!({"data": v2, "library": "UI"}),
    );
    let still = again["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == json!("Button"))
        .cloned()
        .unwrap();
    assert_eq!(still["status"], json!("changed"));
    run(
        &mut app,
        "library.update",
        json!({"data": v2, "library": "UI"}),
    );
    let done = run(
        &mut app,
        "library.changes",
        json!({"data": v2, "library": "UI"}),
    );
    assert!(done["components"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["status"] == json!("unchanged")));
}

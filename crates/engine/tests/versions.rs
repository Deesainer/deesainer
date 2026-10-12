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

#[test]
fn versions_are_saved_compared_restored_and_kept_in_the_file() {
    let mut s = Session::new().unwrap();
    let card = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 0, "y": 0, "width": 100, "height": 60, "fill": "#0D99FF"}),
    ));
    let v1 = id(&run(
        &mut s,
        "version.save",
        json!({"label": "First draft"}),
    ));
    run(
        &mut s,
        "node.setProps",
        json!({"ids": [card], "opacity": 0.5}),
    );
    let extra = id(&run(
        &mut s,
        "node.create.ellipse",
        json!({"x": 200, "y": 0, "width": 40, "height": 40}),
    ));
    let list = run(&mut s, "version.list", json!({}));
    assert_eq!(list[0]["label"], json!("First draft"));
    let d = run(&mut s, "version.diff", json!({ "id": v1 }));
    assert_eq!(d["removed"][0]["id"], json!(extra));
    let changed = d["changed"].as_array().unwrap();
    let card_change = changed.iter().find(|c| c["id"] == json!(card)).unwrap();
    assert!(card_change["props"]
        .as_array()
        .unwrap()
        .contains(&json!("opacity")));
    let png = run(&mut s, "version.preview", json!({ "id": v1, "scale": 0.5 }));
    assert_eq!(png["mimeType"], json!("image/png"));
    let r = run(&mut s, "version.restore", json!({ "id": v1 }));
    let doc = s.snapshot();
    assert!(doc.get(NodeId(extra)).is_none());
    assert_eq!(doc.get(NodeId(card)).unwrap().opacity, 1.0);
    let list = run(&mut s, "version.list", json!({}));
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert!(list[0]["label"]
        .as_str()
        .unwrap()
        .starts_with("Before restoring"));
    run(&mut s, "version.restore", json!({ "id": r["saved"] }));
    assert!(s.snapshot().get(NodeId(extra)).is_some());
    let fresh = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 0, "y": 200, "width": 10, "height": 10}),
    ));
    assert!(fresh > r["saved"].as_u64().unwrap());
    let data = run(&mut s, "document.serialize", json!({}))["data"].clone();
    let mut t = Session::new().unwrap();
    run(&mut t, "document.load", json!({ "data": data }));
    assert_eq!(
        run(&mut t, "version.list", json!({}))
            .as_array()
            .unwrap()
            .len(),
        3
    );
    run(&mut t, "version.delete", json!({ "id": v1 }));
    assert_eq!(
        run(&mut t, "version.list", json!({}))
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(t.dispatch("version.restore", &json!({"id": v1})).is_err());
}

#[test]
fn versions_are_compared_pixel_by_pixel() {
    let mut s = Session::new().unwrap();
    let r = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 0, "y": 0, "width": 100, "height": 100, "fill": "#0D99FF"}),
    ));
    let v = id(&run(&mut s, "version.save", json!({"label": "Before"})));
    let same = run(&mut s, "version.compare", json!({"id": v}));
    assert_eq!(same["changedPixels"], json!(0));
    run(&mut s, "node.move", json!({"ids": [r], "dx": 50, "dy": 0}));
    let moved = run(&mut s, "version.compare", json!({"id": v}));
    assert_eq!(
        (moved["width"].as_u64(), moved["height"].as_u64()),
        (Some(150), Some(100))
    );
    let n = moved["changedPixels"].as_u64().unwrap();
    assert!((9_000..=11_000).contains(&n), "{n}");
    assert!(moved["diff"].as_str().unwrap().len() > 100);
}

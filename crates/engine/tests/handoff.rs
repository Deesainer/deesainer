#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use base64::Engine as _;
use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

const PIXEL_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

#[test]
fn spec_code_and_measure_through_commands() {
    let mut s = Session::new().unwrap();
    let a = run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 10, "y": 20, "width": 120, "height": 40, "fill": "#1E90FF"}),
    )["id"]
        .as_u64()
        .unwrap();
    let b = run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 200, "y": 20, "width": 50, "height": 40}),
    )["id"]
        .as_u64()
        .unwrap();
    let spec = run(&mut s, "inspect.spec", json!({"id": a}));
    assert_eq!(spec["width"], json!(120.0));
    assert_eq!(spec["fills"][0]["color"], json!("#1E90FF"));
    for (language, needle) in [
        ("css", "background: #1E90FF;"),
        ("swiftui", ".frame(width: 120, height: 40)"),
        ("compose", ".size(width = 120.dp, height = 40.dp)"),
    ] {
        let out = run(
            &mut s,
            "inspect.code",
            json!({"id": a, "language": language}),
        );
        let code = out["code"].as_str().unwrap();
        assert!(
            code.contains(needle),
            "{language}: missing {needle:?} in\n{code}"
        );
    }
    let m = run(&mut s, "inspect.measure", json!({"from": a, "to": b}));
    assert_eq!(m["relation"], json!("apart"));
    assert_eq!(m["horizontalGap"], json!(70.0));
    assert!(s
        .dispatch("inspect.code", &json!({"id": a, "language": "cobol"}))
        .is_err());
}

#[test]
fn assets_come_back_as_they_were_placed() {
    let mut s = Session::new().unwrap();
    let placed = run(
        &mut s,
        "image.place",
        json!({"data": PIXEL_PNG, "x": 0, "y": 0, "width": 10, "height": 10}),
    );
    let spec = run(&mut s, "inspect.spec", json!({"id": placed["id"]}));
    let asset = spec["assets"][0]["asset"].as_str().unwrap().to_owned();
    assert_eq!(spec["assets"][0]["width"], json!(1));
    let out = run(&mut s, "inspect.asset", json!({"asset": asset}));
    assert_eq!(out["mimeType"], json!("image/png"));
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(out["data"].as_str().unwrap())
        .unwrap();
    let original = base64::engine::general_purpose::STANDARD
        .decode(PIXEL_PNG)
        .unwrap();
    assert_eq!(bytes, original);
    assert!(s
        .dispatch("inspect.asset", &json!({"asset": "nope"}))
        .is_err());
}

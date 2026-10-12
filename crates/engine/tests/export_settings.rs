#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

#[test]
fn export_settings_are_saved_and_export_every_row() {
    let mut s = Session::new().unwrap();
    let r = run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 0, "y": 0, "width": 40, "height": 20, "fill": "#FF0000"}),
    )["id"]
        .as_u64()
        .unwrap();
    run(
        &mut s,
        "node.rename",
        json!({"id": r, "name": "icons/close"}),
    );
    let settings = json!([
        {"format": "png", "scale": 1},
        {"format": "png", "scale": 2, "suffix": "@2x"},
        {"format": "png", "scale": 3, "suffix": "@2x"},
        {"format": "svg"},
    ]);
    run(
        &mut s,
        "node.setExportSettings",
        json!({"ids": [r], "settings": settings}),
    );
    let data = run(&mut s, "document.serialize", json!({}))["data"].clone();
    let mut t = Session::new().unwrap();
    run(&mut t, "document.load", json!({ "data": data }));
    let files = run(&mut t, "export.settings", json!({ "id": r }))["files"].clone();
    let names: Vec<&str> = files
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["fileName"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "icons_close.png",
            "icons_close@2x.png",
            "icons_close@2x 2.png",
            "icons_close.svg"
        ]
    );
    assert_eq!(files[1]["width"], json!(80));
    assert_eq!(files[2]["width"], json!(120));
    assert!(files[3]["svg"].as_str().unwrap().starts_with("<svg"));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.dispatch("export.settings", &json!({ "id": r })).is_err());
}

#[test]
fn export_settings_are_validated() {
    let mut s = Session::new().unwrap();
    for bad in [
        json!([{"format": "gif"}]),
        json!([{"format": "png", "scale": 0}]),
        json!([{"format": "png", "suffix": "a\nb"}]),
        json!(vec![json!({"format": "png"}); 33]),
    ] {
        assert!(
            s.dispatch("node.setExportSettings", &json!({ "settings": bad }))
                .is_err(),
            "{bad}"
        );
    }
}

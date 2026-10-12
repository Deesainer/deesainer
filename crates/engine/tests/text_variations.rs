#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::doc::{FontVariation, NodeId};
use deesainer_engine::Session;
use serde_json::json;

#[test]
fn variable_axes_are_set_checked_undone_and_exported() {
    let mut s = Session::new().unwrap();
    let id = NodeId(
        s.dispatch(
            "node.create.text",
            &json!({"x": 0, "y": 0, "content": "Axes", "fontSize": 40}),
        )
        .unwrap()["id"]
            .as_u64()
            .unwrap(),
    );
    s.dispatch(
        "text.setStyle",
        &json!({"ids": [id], "variations": [{"tag": "wdth", "value": 75}, {"tag": "opsz", "value": 32}]}),
    )
    .unwrap();
    let style = &s.doc().get(id).unwrap().text().unwrap().style;
    assert_eq!(
        style.variations,
        [
            FontVariation {
                tag: "wdth".into(),
                value: 75.0
            },
            FontVariation {
                tag: "opsz".into(),
                value: 32.0
            }
        ]
    );
    for bad in [
        json!([{"tag": "width", "value": 1}]),
        json!([{"tag": "wd t", "value": 1}]),
    ] {
        assert!(s
            .dispatch("text.setStyle", &json!({"ids": [id], "variations": bad}))
            .is_err());
    }
    let css = s
        .dispatch("inspect.code", &json!({"id": id, "language": "css"}))
        .unwrap();
    assert!(
        css.to_string()
            .contains("font-variation-settings: \\\"wdth\\\" 75, \\\"opsz\\\" 32"),
        "{css}"
    );
    s.dispatch("edit.undo", &json!({})).unwrap();
    assert!(s
        .doc()
        .get(id)
        .unwrap()
        .text()
        .unwrap()
        .style
        .variations
        .is_empty());
}

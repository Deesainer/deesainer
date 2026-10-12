#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

#[test]
fn annotations_are_added_edited_listed_and_shown_in_the_spec() {
    let mut s = Session::new().unwrap();
    let r = run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 0, "y": 0, "width": 120, "height": 40}),
    )["id"]
        .as_u64()
        .unwrap();
    let i = run(
        &mut s,
        "annotation.add",
        json!({"id": r, "text": "Primary button; 44 pt touch target", "props": ["height"]}),
    )["index"]
        .as_u64()
        .unwrap();
    run(
        &mut s,
        "annotation.add",
        json!({"id": r, "text": "Disabled at 40% opacity"}),
    );
    run(
        &mut s,
        "annotation.update",
        json!({"id": r, "index": i, "props": ["height", "fill"]}),
    );
    let list = run(&mut s, "annotation.list", json!({}));
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert_eq!(list[0]["props"], json!(["height", "fill"]));
    let spec = run(&mut s, "inspect.spec", json!({"id": r}));
    assert_eq!(
        spec["annotations"][1]["text"],
        json!("Disabled at 40% opacity")
    );
    run(&mut s, "annotation.remove", json!({"id": r, "index": 0}));
    assert_eq!(
        run(&mut s, "annotation.list", json!({}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(
        run(&mut s, "annotation.list", json!({}))
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(s
        .dispatch("annotation.add", &json!({"id": r, "text": "  "}))
        .is_err());
    let page = s.current_page().0;
    assert!(s
        .dispatch("annotation.add", &json!({"id": page, "text": "x"}))
        .is_err());
}

#[test]
fn opentype_features_are_set_validated_and_exported() {
    let mut s = Session::new().unwrap();
    let t = run(
        &mut s,
        "node.create.text",
        json!({"x": 0, "y": 0, "content": "office 1/2 0110"}),
    )["id"]
        .as_u64()
        .unwrap();
    run(
        &mut s,
        "text.setStyle",
        json!({"ids": [t], "features": [{"tag": "liga", "value": 0}, {"tag": "tnum", "value": 1}]}),
    );
    let doc = s.snapshot();
    let deesainer_doc::NodeKind::Text(d) = &doc.get(deesainer_doc::NodeId(t)).unwrap().kind else {
        panic!("not text")
    };
    assert_eq!(d.style.features.len(), 2);
    drop(doc);
    let css = run(&mut s, "inspect.code", json!({"id": t, "language": "css"}));
    let code = css["code"].as_str().unwrap();
    assert!(
        code.contains(r#"font-feature-settings: "liga" 0, "tnum" 1;"#),
        "{code}"
    );
    for bad in [
        json!([{"tag": "lig", "value": 1}]),
        json!([{"tag": "li-a", "value": 1}]),
        json!([{"tag": "liga", "value": 500}]),
    ] {
        assert!(s
            .dispatch("text.setStyle", &json!({"ids": [t], "features": bad}))
            .is_err());
    }
}

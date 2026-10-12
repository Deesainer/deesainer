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

fn fill_hex(s: &Session, node: u64) -> String {
    let doc = s.snapshot();
    let n = doc.get(NodeId(node)).unwrap();
    match &n.fills[0].kind {
        deesainer_doc::PaintKind::Solid { color } => {
            let [r, g, b, _] = color.to_rgba8();
            format!("#{r:02X}{g:02X}{b:02X}")
        }
        other => panic!("{other:?}"),
    }
}

fn setup(s: &mut Session) -> (u64, u64, u64, u64, u64) {
    let theme = id(&run(
        s,
        "collection.create",
        json!({"name": "Theme", "modes": ["Light", "Dark"]}),
    ));
    let bg = id(&run(
        s,
        "variable.create",
        json!({"name": "bg", "collection": theme, "values": ["#FFFFFF", "#111111"], "type": "color"}),
    ));
    let outer = id(&run(
        s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 200, "height": 200}),
    ));
    let inner = id(&run(
        s,
        "node.create.frame",
        json!({"parent": outer, "x": 10, "y": 10, "width": 100, "height": 100}),
    ));
    let r = id(&run(
        s,
        "node.create.rectangle",
        json!({"parent": inner, "x": 0, "y": 0, "width": 20, "height": 20, "fill": "#FF0000"}),
    ));
    run(
        s,
        "variable.bind",
        json!({"ids": [r], "target": "fillColor", "variable": bg}),
    );
    (theme, bg, outer, inner, r)
}

#[test]
fn frames_choose_the_mode_their_layers_resolve_in() {
    let mut s = Session::new().unwrap();
    let (theme, _, outer, inner, r) = setup(&mut s);
    assert_eq!(fill_hex(&s, r), "#FFFFFF");
    run(
        &mut s,
        "node.setVariableMode",
        json!({"ids": [outer], "collection": theme, "mode": 1}),
    );
    assert_eq!(fill_hex(&s, r), "#111111");
    run(
        &mut s,
        "node.setVariableMode",
        json!({"ids": [inner], "collection": theme, "mode": 0}),
    );
    assert_eq!(fill_hex(&s, r), "#FFFFFF");
    run(
        &mut s,
        "node.setVariableMode",
        json!({"ids": [inner], "collection": theme, "mode": null}),
    );
    assert_eq!(fill_hex(&s, r), "#111111");
    let list = run(&mut s, "variable.list", json!({}));
    assert_eq!(list["collections"][1]["active"], json!(0));
    assert_eq!(list["modes"]["names"], json!(["Default"]));
    assert!(s
        .dispatch(
            "node.setVariableMode",
            &json!({"ids": [inner], "collection": theme, "mode": 2})
        )
        .is_err());
    let data = run(&mut s, "document.serialize", json!({}))["data"].clone();
    let mut t = Session::new().unwrap();
    run(&mut t, "document.load", json!({ "data": data }));
    assert_eq!(fill_hex(&t, r), "#111111");
}

#[test]
fn collections_have_their_own_modes() {
    let mut s = Session::new().unwrap();
    let (theme, bg, ..) = setup(&mut s);
    let plain = id(&run(
        &mut s,
        "variable.create",
        json!({"name": "gap", "value": 8}),
    ));
    run(
        &mut s,
        "mode.add",
        json!({"collection": theme, "name": "High contrast"}),
    );
    let doc = s.snapshot();
    let v = |i: u64| {
        doc.variable(deesainer_doc::VariableId(i))
            .unwrap()
            .values
            .len()
    };
    assert_eq!((v(bg), v(plain)), (3, 1));
    assert!(s
        .dispatch(
            "variable.create",
            &json!({"name": "x", "collection": theme, "values": [1, 2]})
        )
        .is_err());
    run(
        &mut s,
        "collection.rename",
        json!({"collection": theme, "name": "Colours"}),
    );
    let list = run(&mut s, "variable.list", json!({}));
    assert_eq!(list["collections"][1]["name"], json!("Colours"));
}

#[test]
fn scopes_limit_what_a_variable_binds_to() {
    let mut s = Session::new().unwrap();
    let radius = id(&run(
        &mut s,
        "variable.create",
        json!({"name": "radius", "value": 8, "scopes": ["cornerRadius"]}),
    ));
    let r = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 0, "y": 0, "width": 20, "height": 20}),
    ));
    assert!(s
        .dispatch(
            "variable.bind",
            &json!({"ids": [r], "target": "opacity", "variable": radius})
        )
        .is_err());
    run(
        &mut s,
        "variable.bind",
        json!({"ids": [r], "target": "cornerRadius", "variable": radius}),
    );
    run(
        &mut s,
        "variable.setScopes",
        json!({"id": radius, "scopes": []}),
    );
    run(
        &mut s,
        "variable.bind",
        json!({"ids": [r], "target": "opacity", "variable": radius}),
    );
    assert!(s
        .dispatch(
            "variable.setScopes",
            &json!({"id": radius, "scopes": ["nope"]})
        )
        .is_err());
}

#[test]
fn deleting_a_collection_unbinds_and_undo_restores_it() {
    let mut s = Session::new().unwrap();
    let (theme, bg, outer, _, r) = setup(&mut s);
    run(
        &mut s,
        "node.setVariableMode",
        json!({"ids": [outer], "collection": theme, "mode": 1}),
    );
    run(&mut s, "collection.delete", json!({ "collection": theme }));
    let doc = s.snapshot();
    assert!(doc.variable(deesainer_doc::VariableId(bg)).is_none());
    assert!(doc.get(NodeId(r)).unwrap().bindings.is_empty());
    assert!(doc.get(NodeId(outer)).unwrap().variable_modes.is_empty());
    assert_eq!(fill_hex(&s, r), "#111111");
    run(&mut s, "edit.undo", json!({}));
    let doc = s.snapshot();
    assert!(doc.variable(deesainer_doc::VariableId(bg)).is_some());
    assert!(!doc.get(NodeId(r)).unwrap().bindings.is_empty());
    assert!(s
        .dispatch("collection.delete", &json!({"collection": 0}))
        .is_err());
}

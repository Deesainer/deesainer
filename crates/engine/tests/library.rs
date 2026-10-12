#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::doc::{NodeId, NodeKind};
use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

fn data(s: &mut Session) -> String {
    run(s, "document.serialize", json!({}))["data"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn library_file() -> (String, u64, u64) {
    let mut s = Session::new().unwrap();
    let f = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 100, "height": 40, "fill": "#1E90FF", "name": "Button"}),
    ));
    run(
        &mut s,
        "node.create.text",
        json!({"parent": f, "x": 10, "y": 10, "content": "OK", "name": "Label"}),
    );
    run(&mut s, "component.create", json!({"ids": [f]}));
    let card = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 200, "y": 0, "width": 200, "height": 100, "name": "Card"}),
    ));
    run(
        &mut s,
        "component.instantiate",
        json!({"component": f, "parent": card, "x": 10, "y": 10}),
    );
    run(&mut s, "component.create", json!({"ids": [card]}));
    (data(&mut s), f, card)
}

fn components(s: &Session) -> Vec<(u64, String, bool)> {
    s.snapshot()
        .nodes()
        .filter(|n| matches!(n.kind, NodeKind::Component { .. }))
        .map(|n| (n.id.0, n.name.clone(), n.origin.is_some()))
        .collect()
}

#[test]
fn import_copies_components_with_their_dependencies() {
    let (lib, button, card) = library_file();
    let mut s = Session::new().unwrap();
    let r = run(
        &mut s,
        "library.import",
        json!({"data": lib, "library": "Brand", "components": [card]}),
    );
    assert_eq!(r["imported"], 2);
    let comps = components(&s);
    assert_eq!(comps.len(), 2);
    assert!(comps.iter().all(|(_, _, linked)| *linked));
    let page = NodeId(r["page"].as_u64().unwrap());
    assert_eq!(s.snapshot().get(page).unwrap().name, "Library: Brand");
    let doc = s.snapshot();
    let local_button = doc
        .nodes()
        .find(|n| n.name == "Button" && matches!(n.kind, NodeKind::Component { .. }))
        .unwrap()
        .id;
    let nested = doc.nodes().find(
        |n| matches!(&n.kind, NodeKind::Instance { component, .. } if *component == local_button),
    );
    assert!(nested.is_some());
    let again = run(
        &mut s,
        "library.import",
        json!({"data": lib, "library": "Brand"}),
    );
    assert_eq!(again["imported"], 0);
    assert_eq!(again["skipped"], 2);
    let listed = run(&mut s, "library.list", json!({}));
    assert_eq!(listed[0]["library"], "Brand");
    assert_eq!(listed[0]["components"], 2);
    let _ = button;
    run(&mut s, "edit.undo", json!({}));
    assert!(components(&s).is_empty());
}

#[test]
fn update_replaces_content_and_keeps_instances() {
    let (lib, button, _) = library_file();
    let mut s = Session::new().unwrap();
    run(
        &mut s,
        "library.import",
        json!({"data": lib, "library": "Brand", "components": [button]}),
    );
    let main = components(&s)[0].0;
    let inst = id(&run(
        &mut s,
        "component.instantiate",
        json!({"component": main}),
    ));
    let mut l = Session::new().unwrap();
    run(&mut l, "document.load", json!({"data": lib}));
    run(
        &mut l,
        "node.setProps",
        json!({"ids": [button], "fill": "#FF0000"}),
    );
    run(
        &mut l,
        "node.resize",
        json!({"id": button, "width": 160, "height": 40}),
    );
    run(
        &mut l,
        "node.create.rectangle",
        json!({"parent": button, "x": 0, "y": 0, "width": 8, "height": 8, "name": "Badge"}),
    );
    let updated = data(&mut l);
    let r = run(
        &mut s,
        "library.update",
        json!({"data": updated, "library": "Brand"}),
    );
    assert_eq!(r["updated"], 1, "{r}");
    let doc = s.snapshot();
    let m = doc.get(NodeId(main)).unwrap();
    assert_eq!(m.rect.width, 160.0);
    assert_eq!(m.children.len(), 2);
    assert!(doc
        .subtree_pre_order(NodeId(main))
        .unwrap()
        .iter()
        .any(|i| doc.get(*i).unwrap().name == "Badge"));
    assert!(matches!(
        &doc.get(NodeId(inst)).unwrap().kind,
        NodeKind::Instance { component, .. } if component.0 == main
    ));
    assert!(s
        .dispatch(
            "library.update",
            &json!({"data": data(&mut l), "library": "Nothing"})
        )
        .is_err());
}

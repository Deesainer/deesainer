#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::doc::{NodeId, NodeKind, PaintKind};
use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

fn fill_hex(s: &Session, id: u64) -> String {
    match &s.snapshot().get(NodeId(id)).unwrap().fills[0].kind {
        PaintKind::Solid { color } => color.to_hex(),
        other => panic!("{other:?}"),
    }
}

fn button(s: &mut Session) -> (u64, u64) {
    let f = id(&run(
        s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 100, "height": 40, "fill": "#1E90FF", "name": "Button"}),
    ));
    let t = id(&run(
        s,
        "node.create.text",
        json!({"parent": f, "x": 10, "y": 10, "content": "OK", "name": "Label"}),
    ));
    let c = id(&run(s, "component.create", json!({"ids": [f]})));
    assert_eq!(c, f);
    (c, t)
}

#[test]
fn instances_follow_main_until_overridden() {
    let mut s = Session::new().unwrap();
    let (c, _) = button(&mut s);
    let i = id(&run(
        &mut s,
        "component.instantiate",
        json!({"component": c}),
    ));
    assert_eq!(s.snapshot().get(NodeId(i)).unwrap().rect.x, 140.0);
    run(
        &mut s,
        "node.setProps",
        json!({"ids": [c], "fill": "#FF0000"}),
    );
    assert_eq!(fill_hex(&s, i), "#FF0000");
    run(
        &mut s,
        "node.setProps",
        json!({"ids": [i], "fill": "#00FF00"}),
    );
    run(
        &mut s,
        "node.setProps",
        json!({"ids": [c], "fill": "#0000FF"}),
    );
    assert_eq!(fill_hex(&s, i), "#00FF00");
    run(&mut s, "component.resetOverrides", json!({"ids": [i]}));
    assert_eq!(fill_hex(&s, i), "#0000FF");
}

#[test]
fn text_override_and_detach() {
    let mut s = Session::new().unwrap();
    let (c, label) = button(&mut s);
    let i = id(&run(
        &mut s,
        "component.instantiate",
        json!({"component": c, "x": 0, "y": 100}),
    ));
    run(
        &mut s,
        "instance.setOverride",
        json!({"id": i, "target": label, "prop": "text", "value": "Cancel"}),
    );
    run(&mut s, "node.resize", json!({"id": i, "width": 200}));
    let out = run(&mut s, "component.detach", json!({"ids": [i]}));
    let frame = NodeId(out["ids"][0].as_u64().unwrap());
    let doc = s.snapshot();
    let f = doc.get(frame).unwrap();
    assert!(matches!(f.kind, NodeKind::Frame));
    assert_eq!(f.rect.width, 200.0);
    let copy = doc.get(f.children[0]).unwrap();
    assert_eq!(copy.text().unwrap().content, "Cancel");
    assert_ne!(copy.id, NodeId(label));
    assert!(doc.get(NodeId(i)).is_none());
    doc.validate().unwrap();
}

#[test]
fn variants_switch_and_keep_overrides_by_name() {
    let mut s = Session::new().unwrap();
    let (a, _) = button(&mut s);
    run(
        &mut s,
        "node.rename",
        json!({"id": a, "name": "State=Default"}),
    );
    let dup = run(
        &mut s,
        "edit.duplicate",
        json!({"ids": [a], "dx": 0, "dy": 60}),
    );
    let b = dup["ids"][0].as_u64().unwrap();
    assert!(matches!(
        s.snapshot().get(NodeId(b)).unwrap().kind,
        NodeKind::Component { .. }
    ));
    run(
        &mut s,
        "node.rename",
        json!({"id": b, "name": "State=Hover"}),
    );
    run(
        &mut s,
        "node.setProps",
        json!({"ids": [b], "fill": "#222222"}),
    );
    let set = id(&run(
        &mut s,
        "component.combineAsVariants",
        json!({"ids": [a, b]}),
    ));
    let i = id(&run(
        &mut s,
        "component.instantiate",
        json!({"component": a, "x": 0, "y": 300}),
    ));
    let label_a = s.snapshot().get(NodeId(a)).unwrap().children[0];
    run(
        &mut s,
        "instance.setOverride",
        json!({"id": i, "target": label_a.0, "prop": "text", "value": "Go"}),
    );
    run(
        &mut s,
        "instance.setVariant",
        json!({"ids": [i], "properties": {"State": "Hover"}}),
    );
    let doc = s.snapshot();
    let NodeKind::Instance {
        component,
        overrides,
    } = &doc.get(NodeId(i)).unwrap().kind
    else {
        panic!()
    };
    assert_eq!(*component, NodeId(b));
    let label_b = doc.get(NodeId(b)).unwrap().children[0];
    assert!(overrides.iter().any(|o| o.target == label_b));
    assert_eq!(fill_hex(&s, i), "#222222");
    let list = run(&mut s, "component.list", Value::Null);
    let set_entry = list
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == set)
        .unwrap();
    assert_eq!(set_entry["type"], "componentSet");
    assert!(s
        .dispatch(
            "instance.setVariant",
            &json!({"ids": [i], "properties": {"State": "Nope"}})
        )
        .is_err());
}

#[test]
fn go_to_main_and_render_instance() {
    let mut s = Session::new().unwrap();
    let (c, _) = button(&mut s);
    let i = id(&run(
        &mut s,
        "component.instantiate",
        json!({"component": c}),
    ));
    let r = run(&mut s, "component.goToMain", json!({"id": i}));
    assert_eq!(r["id"], c);
    assert_eq!(s.selection(), &[NodeId(c)]);
    let preview = run(&mut s, "view.renderPreview", json!({"id": i}));
    assert_eq!(preview["width"], 100);
}

#[test]
fn component_properties_drive_text_visibility_and_swaps() {
    let mut s = Session::new().unwrap();
    let (c, label) = button(&mut s);
    let icon = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"parent": c, "x": 80, "y": 10, "width": 10, "height": 10, "name": "Icon"}),
    ));
    run(
        &mut s,
        "component.addProperty",
        json!({"id": c, "name": "Label text", "kind": "text", "target": label}),
    );
    run(
        &mut s,
        "component.addProperty",
        json!({"id": c, "name": "Show icon", "kind": "boolean", "target": icon}),
    );
    assert!(s
        .dispatch(
            "component.addProperty",
            &json!({"id": c, "name": "Show icon", "kind": "boolean", "target": icon})
        )
        .is_err());
    assert!(s
        .dispatch(
            "component.addProperty",
            &json!({"id": c, "name": "Bad", "kind": "text", "target": icon})
        )
        .is_err());
    let i = id(&run(
        &mut s,
        "component.instantiate",
        json!({"component": c}),
    ));
    run(
        &mut s,
        "instance.setProperty",
        json!({"id": i, "name": "Label text", "value": "Save"}),
    );
    run(
        &mut s,
        "instance.setProperty",
        json!({"id": i, "name": "Show icon", "value": false}),
    );
    let doc = s.snapshot();
    let NodeKind::Instance { overrides, .. } = &doc.get(NodeId(i)).unwrap().kind else {
        panic!("not an instance");
    };
    assert_eq!(overrides.len(), 2);
    assert_eq!(
        doc.get(NodeId(label)).unwrap().text().unwrap().content,
        "OK"
    );
    assert!(doc.get(NodeId(icon)).unwrap().visible);
    run(
        &mut s,
        "instance.setProperty",
        json!({"id": i, "name": "Show icon", "value": true}),
    );
    let doc = s.snapshot();
    let NodeKind::Instance { overrides, .. } = &doc.get(NodeId(i)).unwrap().kind else {
        panic!("not an instance");
    };
    assert_eq!(overrides.len(), 1);
    s.dispatch("edit.undo", &Value::Null).unwrap();
    let doc = s.snapshot();
    let NodeKind::Instance { overrides, .. } = &doc.get(NodeId(i)).unwrap().kind else {
        panic!("not an instance");
    };
    assert_eq!(overrides.len(), 2);
    run(
        &mut s,
        "component.removeProperty",
        json!({"id": c, "name": "Show icon"}),
    );
    assert!(s
        .dispatch(
            "instance.setProperty",
            &json!({"id": i, "name": "Show icon", "value": true})
        )
        .is_err());
}

#[test]
fn instance_swap_points_a_nested_instance_at_another_component() {
    let mut s = Session::new().unwrap();
    let (a, _) = button(&mut s);
    let f2 = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 400, "y": 0, "width": 20, "height": 20, "fill": "#FF0000", "name": "Dot"}),
    ));
    let b = id(&run(&mut s, "component.create", json!({"ids": [f2]})));
    let card = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 200, "width": 200, "height": 100, "name": "Card"}),
    ));
    let nested = id(&run(
        &mut s,
        "component.instantiate",
        json!({"component": a, "parent": card}),
    ));
    let card_c = id(&run(&mut s, "component.create", json!({"ids": [card]})));
    run(
        &mut s,
        "component.addProperty",
        json!({"id": card_c, "name": "Icon", "kind": "instanceSwap", "target": nested}),
    );
    let i = id(&run(
        &mut s,
        "component.instantiate",
        json!({"component": card_c}),
    ));
    run(
        &mut s,
        "instance.setProperty",
        json!({"id": i, "name": "Icon", "value": b}),
    );
    let doc = s.snapshot();
    let NodeKind::Instance { overrides, .. } = &doc.get(NodeId(i)).unwrap().kind else {
        panic!("not an instance");
    };
    assert!(matches!(
        &overrides[0].value,
        deesainer_engine::doc::PropValue::Kind(NodeKind::Instance { component, .. })
            if component.0 == b
    ));
    assert!(s
        .dispatch(
            "instance.setProperty",
            &json!({"id": i, "name": "Icon", "value": card_c})
        )
        .is_err());
}

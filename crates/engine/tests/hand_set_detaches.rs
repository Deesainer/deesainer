#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::doc::{BindTarget, NodeId, Paint, PaintKind};
use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn solid(hex: &str) -> deesainer_color::Rgba {
    deesainer_color::Rgba::from_hex(hex).unwrap()
}

#[test]
fn a_colour_set_by_hand_replaces_the_style_and_variable() {
    let mut s = Session::new().unwrap();
    let r = run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 0, "y": 0, "width": 40, "height": 40}),
    )["id"]
        .as_u64()
        .unwrap();
    let style = run(
        &mut s,
        "style.create",
        json!({"name": "Brand", "kind": "paint", "value": [Paint::solid(solid("#8A2332"))]}),
    )["id"]
        .as_u64()
        .unwrap();
    run(
        &mut s,
        "style.apply",
        json!({"ids": [r], "style": style, "target": "fill"}),
    );
    let v = run(
        &mut s,
        "variable.create",
        json!({"name": "radius", "type": "number", "value": 8}),
    )["id"]
        .as_u64()
        .unwrap();
    run(
        &mut s,
        "variable.bind",
        json!({"ids": [r], "target": "cornerRadius", "variable": v}),
    );
    run(
        &mut s,
        "node.setProps",
        json!({"ids": [r], "fill": "#1E5AA8", "cornerRadius": 2}),
    );
    let n = s.doc().get(NodeId(r)).unwrap();
    assert_eq!(
        n.fills[0].kind,
        PaintKind::Solid {
            color: solid("#1E5AA8")
        }
    );
    assert_eq!(n.styles.fill, None);
    assert!(!n.bindings.contains_key(&BindTarget::CornerRadius));
    assert_eq!(n.corner_radius.top_left, 2.0);
    run(&mut s, "edit.undo", json!({}));
    let n = s.doc().get(NodeId(r)).unwrap();
    assert!(n.styles.fill.is_some());
    assert_eq!(
        n.fills[0].kind,
        PaintKind::Solid {
            color: solid("#8A2332")
        }
    );
    assert_eq!(n.corner_radius.top_left, 8.0);
}

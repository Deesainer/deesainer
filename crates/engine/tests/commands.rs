#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::doc::{Constraint, NodeId, NodeKind, PropValue};
use deesainer_engine::ops::Op;
use deesainer_engine::{EngineError, Session};
use deesainer_geom::Rect;
use serde_json::{json, Value};

fn session() -> Session {
    Session::new().unwrap()
}

fn id_of(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

fn rect(s: &mut Session, x: f64, y: f64, w: f64, h: f64) -> u64 {
    id_of(&run(
        s,
        "node.create.rectangle",
        json!({"x": x, "y": y, "width": w, "height": h}),
    ))
}

#[test]
fn create_move_undo_redo() {
    let mut s = session();
    let id = rect(&mut s, 10.0, 20.0, 100.0, 50.0);
    assert_eq!(s.selection(), &[NodeId(id)]);
    run(&mut s, "node.move", json!({"dx": 5, "dy": -5}));
    let r = s.snapshot().get(NodeId(id)).unwrap().rect;
    assert_eq!((r.x, r.y), (15.0, 15.0));
    assert_eq!(
        run(&mut s, "edit.undo", Value::Null),
        json!({"undone": "Move"})
    );
    assert_eq!(s.snapshot().get(NodeId(id)).unwrap().rect.x, 10.0);
    run(&mut s, "edit.redo", Value::Null);
    assert_eq!(s.snapshot().get(NodeId(id)).unwrap().rect.x, 15.0);
}

#[test]
fn each_command_is_one_history_step() {
    let mut s = session();
    let id = rect(&mut s, 0.0, 0.0, 10.0, 10.0);
    run(
        &mut s,
        "node.setProps",
        json!({"name": "Card", "opacity": 0.5, "visible": false, "cornerRadius": 8}),
    );
    run(&mut s, "edit.undo", Value::Null);
    let node = s.snapshot().get(NodeId(id)).cloned().unwrap();
    assert_eq!(node.name, "Rectangle");
    assert_eq!(node.opacity, 1.0);
    assert!(node.visible);
}

#[test]
fn unknown_params_are_rejected_with_the_accepted_list() {
    let mut s = session();
    let id = rect(&mut s, 0.0, 0.0, 10.0, 10.0);
    match s.dispatch("node.resize", &json!({"id": id, "w": 2})) {
        Err(EngineError::UnknownParam { key, accepted, .. }) => {
            assert_eq!(key, "w");
            assert!(accepted.contains("width"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn disabled_and_unknown_commands() {
    let mut s = session();
    assert!(matches!(
        s.dispatch("edit.undo", &Value::Null),
        Err(EngineError::Disabled(_))
    ));
    assert!(matches!(
        s.dispatch("nope", &Value::Null),
        Err(EngineError::UnknownCommand(_))
    ));
}

#[test]
fn delete_removes_subtree_and_undo_restores_it() {
    let mut s = session();
    let frame = id_of(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 400, "height": 300}),
    ));
    run(
        &mut s,
        "node.create.ellipse",
        json!({"parent": frame, "x": 10, "y": 10, "width": 20, "height": 20}),
    );
    let before = s.snapshot();
    run(&mut s, "edit.delete", json!({"ids": [frame]}));
    assert_eq!(s.snapshot().len(), 2);
    run(&mut s, "edit.undo", Value::Null);
    assert_eq!(*s.snapshot(), *before);
}

#[test]
fn pages_are_protected_and_managed() {
    let mut s = session();
    let first = s.current_page();
    assert!(s
        .dispatch("edit.delete", &json!({"ids": [first.0]}))
        .is_err());
    assert!(s.dispatch("page.delete", &Value::Null).is_err());
    let second = id_of(&run(&mut s, "page.create", json!({"name": "Mobile"})));
    assert_eq!(s.current_page(), NodeId(second));
    let r = rect(&mut s, 0.0, 0.0, 5.0, 5.0);
    assert_eq!(s.snapshot().page_of(NodeId(r)), Some(NodeId(second)));
    let dup = id_of(&run(&mut s, "page.duplicate", Value::Null));
    assert_eq!(s.snapshot().get(NodeId(dup)).unwrap().children.len(), 1);
    run(&mut s, "page.delete", json!({"id": second}));
    assert_eq!(s.snapshot().pages().len(), 2);
    s.snapshot().validate().unwrap();
}

#[test]
fn file_new_replaces_document_and_clears_history() {
    let mut s = session();
    rect(&mut s, 0.0, 0.0, 1.0, 1.0);
    run(&mut s, "file.new", json!({"width": 375, "height": 812}));
    assert!(!s.can_undo());
    assert_eq!(s.snapshot().len(), 3);
    assert!(s.selection().is_empty());
}

#[test]
fn group_fits_children_and_ungroup_restores_positions() {
    let mut s = session();
    let a = rect(&mut s, 10.0, 10.0, 10.0, 10.0);
    let b = rect(&mut s, 50.0, 30.0, 10.0, 10.0);
    let g = id_of(&run(&mut s, "object.group", json!({"ids": [a, b]})));
    let doc = s.snapshot();
    assert_eq!(
        doc.get(NodeId(g)).unwrap().rect,
        Rect::new(10.0, 10.0, 50.0, 30.0)
    );
    assert_eq!(doc.world_bounds(NodeId(b)).unwrap().x, 50.0);
    run(&mut s, "node.move", json!({"ids": [a], "dx": -10, "dy": 0}));
    let doc = s.snapshot();
    assert_eq!(doc.get(NodeId(g)).unwrap().rect.x, 0.0);
    assert_eq!(doc.world_bounds(NodeId(a)).unwrap().x, 0.0);
    assert_eq!(doc.world_bounds(NodeId(b)).unwrap().x, 50.0);
    run(&mut s, "object.ungroup", json!({"ids": [g]}));
    let doc = s.snapshot();
    assert!(doc.get(NodeId(g)).is_none());
    assert_eq!(doc.world_bounds(NodeId(b)).unwrap().x, 50.0);
    doc.validate().unwrap();
    run(&mut s, "edit.undo", Value::Null);
    assert!(s.snapshot().get(NodeId(g)).is_some());
}

#[test]
fn deleting_all_children_removes_the_group() {
    let mut s = session();
    let a = rect(&mut s, 0.0, 0.0, 10.0, 10.0);
    let g = id_of(&run(&mut s, "object.group", json!({"ids": [a]})));
    run(&mut s, "edit.delete", json!({"ids": [a]}));
    assert!(s.snapshot().get(NodeId(g)).is_none());
}

#[test]
fn reorder_and_align() {
    let mut s = session();
    let a = rect(&mut s, 0.0, 0.0, 10.0, 10.0);
    let b = rect(&mut s, 30.0, 5.0, 10.0, 20.0);
    let c = rect(&mut s, 90.0, 0.0, 10.0, 10.0);
    run(&mut s, "object.sendToBack", json!({"ids": [c]}));
    let page = s.current_page();
    assert_eq!(
        s.snapshot().get(page).unwrap().children,
        vec![NodeId(c), NodeId(a), NodeId(b)]
    );
    run(&mut s, "object.bringForward", json!({"ids": [c]}));
    assert_eq!(s.snapshot().get(page).unwrap().children[1], NodeId(c));
    run(&mut s, "align.top", json!({"ids": [a, b, c]}));
    assert_eq!(s.snapshot().get(NodeId(b)).unwrap().rect.y, 0.0);
    run(&mut s, "distribute.horizontal", json!({"ids": [a, b, c]}));
    assert_eq!(s.snapshot().get(NodeId(b)).unwrap().rect.x, 45.0);
}

#[test]
fn duplicate_copy_paste() {
    let mut s = session();
    let frame = id_of(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 100, "height": 100}),
    ));
    run(
        &mut s,
        "node.create.text",
        json!({"parent": frame, "x": 5, "y": 5, "content": "Hi"}),
    );
    let dup = run(&mut s, "edit.duplicate", json!({"ids": [frame], "dx": 120}));
    let copy = dup["ids"][0].as_u64().unwrap();
    let doc = s.snapshot();
    assert_eq!(doc.get(NodeId(copy)).unwrap().children.len(), 1);
    assert_eq!(doc.get(NodeId(copy)).unwrap().rect.x, 120.0);
    let clip = run(&mut s, "edit.copy", json!({"ids": [frame]}));
    let pasted = run(&mut s, "edit.paste", json!({"dy": 200}));
    assert_eq!(pasted["ids"].as_array().unwrap().len(), 1);
    let mut other = session();
    run(&mut other, "edit.paste", json!({"data": clip}));
    assert_eq!(other.snapshot().len(), 4);
    other.snapshot().validate().unwrap();
}

#[test]
fn reparent_keeps_page_position() {
    let mut s = session();
    let frame = id_of(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 100, "y": 100, "width": 200, "height": 200}),
    ));
    let r = rect(&mut s, 150.0, 150.0, 10.0, 10.0);
    run(&mut s, "node.reparent", json!({"id": r, "parent": frame}));
    let doc = s.snapshot();
    assert_eq!(doc.get(NodeId(r)).unwrap().rect.x, 50.0);
    assert_eq!(doc.world_bounds(NodeId(r)).unwrap().x, 150.0);
    assert!(s
        .dispatch("node.reparent", &json!({"id": frame, "parent": r}))
        .is_err());
}

#[test]
fn constraints_follow_frame_resize() {
    let mut s = session();
    let frame = id_of(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 100, "height": 100}),
    ));
    let r = id_of(&run(
        &mut s,
        "node.create.rectangle",
        json!({"parent": frame, "x": 80, "y": 10, "width": 10, "height": 10}),
    ));
    let mut c = s.snapshot().get(NodeId(r)).unwrap().constraints;
    c.horizontal = Constraint::Max;
    s.apply_remote(&[Op::set(NodeId(r), PropValue::Constraints(c))])
        .unwrap();
    run(&mut s, "node.resize", json!({"id": frame, "width": 200}));
    assert_eq!(s.snapshot().get(NodeId(r)).unwrap().rect.x, 180.0);
}

#[test]
fn batch_is_one_undo_step() {
    let mut s = session();
    let steps = vec![
        (
            "node.create.rectangle".to_owned(),
            json!({"x": 0, "y": 0, "width": 1, "height": 1}),
        ),
        ("node.move".to_owned(), json!({"dx": 1, "dy": 1})),
    ];
    s.dispatch_batch("Batch", &steps).unwrap();
    run(&mut s, "edit.undo", Value::Null);
    assert_eq!(s.snapshot().len(), 2);
}

#[test]
fn tree_and_find_for_agents() {
    let mut s = session();
    let id = rect(&mut s, 0.0, 0.0, 10.0, 10.0);
    run(&mut s, "node.rename", json!({"id": id, "name": "Hero"}));
    let tree = run(&mut s, "document.tree", Value::Null);
    assert_eq!(tree["tree"]["children"][0]["name"], "Hero");
    let found = run(&mut s, "document.find", json!({"name": "Hero"}));
    assert_eq!(found[0]["id"], id);
    let node = run(&mut s, "node.inspect", json!({"id": id}));
    assert_eq!(node["type"], "rectangle");
}

#[test]
fn every_command_is_documented() {
    let s = session();
    for spec in s.registry().iter() {
        assert!(!spec.doc.is_empty(), "{} has no doc", spec.id);
        assert!(spec.id.contains('.'), "{} is not a dotted id", spec.id);
        for p in spec.params {
            assert!(!p.doc.is_empty(), "{}.{} has no doc", spec.id, p.name);
        }
        assert_eq!(spec.input_schema()["type"], "object");
    }
}

#[test]
fn line_and_shapes() {
    let mut s = session();
    let l = id_of(&run(
        &mut s,
        "node.create.line",
        json!({"x": 0, "y": 0, "x2": 0, "y2": 100}),
    ));
    let node = s.snapshot().get(NodeId(l)).cloned().unwrap();
    assert!(matches!(node.kind, NodeKind::Line));
    assert!((node.rotation - 90.0).abs() < 1e-9);
    assert_eq!(node.rect.width, 100.0);
    assert!(s
        .dispatch(
            "node.create.polygon",
            &json!({"x": 0, "y": 0, "width": 1, "height": 1, "count": 2})
        )
        .is_err());
}

#[test]
fn panic_hunt() {
    let hostile = [
        Value::Null,
        json!({}),
        json!([]),
        json!("x"),
        json!({"x": "a", "y": null, "width": -1, "height": 1e308}),
        json!({"x": 1e308, "y": 1e308, "width": 1e308, "height": 1e308, "x2": -1e308, "y2": 1e308}),
        json!({"id": 999_999, "ids": [999_999], "dx": 1e308, "dy": -1e308, "parent": 999_999}),
        json!({"id": 1, "ids": [1], "dx": 0, "dy": 0, "width": 0, "height": 0, "parent": 1}),
        json!({"id": 2, "ids": [2], "parent": 2, "index": 99}),
        json!({"id": 3, "ids": [3], "parent": 3, "index": 0, "degrees": 1e300}),
        json!({"ids": "nope", "id": -1, "parent": 2, "fill": "#zzz"}),
        json!({"content": "", "x": 0, "y": 0, "fontSize": 0}),
        json!({"content": "ü".repeat(10_000), "x": 0, "y": 0, "fontSize": 1e300}),
        json!({"opacity": 2, "id": 3, "blend": "nope", "count": 1_000_000}),
        json!({"fills": [{"type": "solid", "color": {"r": 1, "g": 0, "b": 0, "a": 1}, "opacity": 5}]}),
        json!({"effects": [{"type": "layerBlur", "radius": -1}], "strokes": "x", "style": {"weight": -1}}),
        json!({"data": {"roots": [5], "nodes": []}}),
        json!({"data": {"roots": [1], "nodes": [{"id": 1}]}}),
        json!({"name": "x", "type": "frame", "page": 1}),
    ];
    let mut s = session();
    let frame = id_of(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 100, "height": 100}),
    ));
    run(
        &mut s,
        "node.create.rectangle",
        json!({"parent": frame, "x": 0, "y": 0, "width": 10, "height": 10}),
    );
    let ids: Vec<&str> = s.registry().iter().map(|c| c.id).collect();
    for id in ids {
        for params in &hostile {
            if let Err(EngineError::Internal(msg)) = s.dispatch(id, params) {
                panic!("{id} panicked on {params}: {msg}");
            }
            s.snapshot()
                .validate()
                .unwrap_or_else(|e| panic!("{id} broke the document on {params}: {e}"));
        }
    }
}

#[test]
fn corner_radii_per_corner() {
    let mut s = Session::new().unwrap();
    let r = s
        .dispatch(
            "node.create.rectangle",
            &json!({"x": 0, "y": 0, "width": 100, "height": 60}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    s.dispatch(
        "node.setProps",
        &json!({"ids": [r], "cornerRadii": [1, 2, 3, 4]}),
    )
    .unwrap();
    let c = s
        .snapshot()
        .get(deesainer_engine::doc::NodeId(r))
        .unwrap()
        .corner_radius;
    assert_eq!(
        (c.top_left, c.top_right, c.bottom_right, c.bottom_left),
        (1.0, 2.0, 3.0, 4.0)
    );
    for bad in [
        json!([1, 2, 3]),
        json!([1, 2, 3, -4]),
        json!("round"),
        json!([1, 2, 3, "x"]),
    ] {
        assert!(
            s.dispatch("node.setProps", &json!({"ids": [r], "cornerRadii": bad}))
                .is_err(),
            "{bad}"
        );
    }
}

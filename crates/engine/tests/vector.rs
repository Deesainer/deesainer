#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::doc::{NodeId, NodeKind, PathCmd};
use deesainer_engine::Session;
use deesainer_geom::Rect;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

fn id(v: &Value) -> NodeId {
    NodeId(v["id"].as_u64().unwrap())
}

fn rect(s: &mut Session, x: f64, y: f64, w: f64, h: f64) -> u64 {
    run(
        s,
        "node.create.rectangle",
        json!({"x": x, "y": y, "width": w, "height": h}),
    )["id"]
        .as_u64()
        .unwrap()
}

#[test]
fn vector_from_page_points_is_fitted() {
    let mut s = Session::new().unwrap();
    let v = id(&run(
        &mut s,
        "node.create.vector",
        json!({"cmds": [
            {"cmd": "moveTo", "to": {"x": 10, "y": 20}},
            {"cmd": "lineTo", "to": {"x": 110, "y": 20}},
            {"cmd": "lineTo", "to": {"x": 60, "y": 120}},
            {"cmd": "close"}
        ], "space": "page"}),
    ));
    let doc = s.snapshot();
    let node = doc.get(v).unwrap();
    assert_eq!(node.rect, Rect::new(10.0, 20.0, 100.0, 100.0));
    assert!(!node.fills.is_empty(), "closed paths are filled");
    let NodeKind::Vector { path } = &node.kind else {
        panic!()
    };
    assert!(matches!(path.cmds[0], PathCmd::MoveTo { to } if to.x == 0.0 && to.y == 0.0));
    run(
        &mut s,
        "vector.setPath",
        json!({"id": v.0, "cmds": [
            {"cmd": "moveTo", "to": {"x": 0, "y": 0}},
            {"cmd": "lineTo", "to": {"x": 50, "y": 0}}
        ]}),
    );
    assert_eq!(s.snapshot().get(v).unwrap().rect.x, 10.0);
    assert!(s
        .dispatch(
            "node.create.vector",
            &json!({"cmds": [{"cmd": "lineTo", "to": {"x": 1, "y": 1}}]})
        )
        .is_err());
}

#[test]
fn boolean_groups_children_and_renders() {
    let mut s = Session::new().unwrap();
    let a = rect(&mut s, 0.0, 0.0, 40.0, 40.0);
    let b = rect(&mut s, 10.0, 10.0, 20.0, 20.0);
    let g = id(&run(
        &mut s,
        "object.booleanSubtract",
        json!({"ids": [a, b]}),
    ));
    let doc = s.snapshot();
    let node = doc.get(g).unwrap();
    assert!(matches!(node.kind, NodeKind::BooleanOp { .. }));
    assert_eq!(node.children, vec![NodeId(a), NodeId(b)]);
    let preview = run(&mut s, "view.renderPreview", json!({"id": g.0}));
    assert_eq!(preview["width"], 40);
    run(&mut s, "edit.undo", Value::Null);
    assert!(s.snapshot().get(g).is_none());
    assert_eq!(s.snapshot().get(NodeId(a)).unwrap().rect.x, 0.0);
}

#[test]
fn flatten_merges_layers_into_one_vector() {
    let mut s = Session::new().unwrap();
    let a = rect(&mut s, 0.0, 0.0, 10.0, 10.0);
    let b = rect(&mut s, 20.0, 0.0, 10.0, 10.0);
    let v = id(&run(&mut s, "object.flatten", json!({"ids": [a, b]})));
    let doc = s.snapshot();
    assert!(doc.get(NodeId(a)).is_none() && doc.get(NodeId(b)).is_none());
    let node = doc.get(v).unwrap();
    assert!(matches!(node.kind, NodeKind::Vector { .. }));
    assert_eq!(node.rect, Rect::new(0.0, 0.0, 30.0, 10.0));
    doc.validate().unwrap();
}

#[test]
fn outline_stroke_makes_a_filled_vector() {
    let mut s = Session::new().unwrap();
    let r = rect(&mut s, 0.0, 0.0, 50.0, 50.0);
    run(
        &mut s,
        "node.setProps",
        json!({"ids": [r], "stroke": "#FF0000", "strokeWeight": 4, "strokeAlign": "center"}),
    );
    let out = run(&mut s, "object.outlineStroke", json!({"ids": [r]}));
    let v = NodeId(out["ids"][0].as_u64().unwrap());
    let doc = s.snapshot();
    let node = doc.get(v).unwrap();
    assert_eq!(node.fills.len(), 1);
    assert!(doc.get(NodeId(r)).unwrap().strokes.is_empty());
    assert_eq!(node.rect, Rect::new(-2.0, -2.0, 54.0, 54.0));
}

#[test]
fn flip_mirrors_children_and_paths() {
    let mut s = Session::new().unwrap();
    let f = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 100, "height": 100}),
    ));
    let r = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"parent": f.0, "x": 10, "y": 0, "width": 20, "height": 20}),
    ));
    run(&mut s, "object.flipHorizontal", json!({"ids": [f.0]}));
    assert_eq!(s.snapshot().get(r).unwrap().rect.x, 70.0);
    let p = id(&run(
        &mut s,
        "node.create.polygon",
        json!({"x": 0, "y": 0, "width": 10, "height": 10}),
    ));
    run(&mut s, "object.flipVertical", json!({"ids": [p.0]}));
    assert!(matches!(
        s.snapshot().get(p).unwrap().kind,
        NodeKind::Vector { .. }
    ));
}

#[test]
fn shape_options() {
    let mut s = Session::new().unwrap();
    let star = id(&run(
        &mut s,
        "node.create.star",
        json!({"x": 0, "y": 0, "width": 10, "height": 10}),
    ));
    run(
        &mut s,
        "node.setShape",
        json!({"id": star.0, "count": 8, "innerRatio": 0.5}),
    );
    assert!(matches!(
        s.snapshot().get(star).unwrap().kind,
        NodeKind::Star { points: 8, .. }
    ));
    assert!(s
        .dispatch("node.setShape", &json!({"id": star.0, "count": 1}))
        .is_err());
}

#[test]
fn flipping_an_angular_gradient_mirrors_its_sweep() {
    let mut s = Session::new().unwrap();
    let r = rect(&mut s, 0.0, 0.0, 100.0, 100.0);
    let black = json!({"r": 0, "g": 0, "b": 0, "a": 1});
    let white = json!({"r": 1, "g": 1, "b": 1, "a": 1});
    run(
        &mut s,
        "node.setFills",
        json!({"ids": [r], "fills": [{"type": "angularGradient", "center": {"x": 0.25, "y": 0.5},
            "angle": 30, "stops": [{"position": 0, "color": black}, {"position": 0.25, "color": white}]}]}),
    );
    run(&mut s, "object.flipHorizontal", json!({"ids": [r]}));
    let doc = s.snapshot();
    let deesainer_doc::PaintKind::AngularGradient {
        center,
        angle,
        stops,
    } = &doc.get(NodeId(r)).unwrap().fills[0].kind
    else {
        panic!("not angular")
    };
    assert!((center.x - 0.75).abs() < 1e-9 && (center.y - 0.5).abs() < 1e-9);
    assert!((angle - 150.0).abs() < 1e-9, "{angle}");
    assert!((stops[0].position - 0.75).abs() < 1e-6, "{stops:?}");
    assert_eq!(stops[1].position, 1.0);
}

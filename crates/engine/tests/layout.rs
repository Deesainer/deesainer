#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::doc::NodeId;
use deesainer_engine::Session;
use deesainer_geom::Rect;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

fn rect_of(s: &Session, id: u64) -> Rect {
    s.snapshot().get(NodeId(id)).unwrap().rect
}

fn add_rect(s: &mut Session, parent: Option<u64>, x: f64, w: f64, h: f64) -> u64 {
    let mut p = json!({"x": x, "y": 0, "width": w, "height": h});
    if let Some(parent) = parent {
        p["parent"] = json!(parent);
    }
    id(&run(s, "node.create.rectangle", p))
}

#[test]
fn add_auto_layout_wraps_and_hugs_siblings() {
    let mut s = Session::new().unwrap();
    let a = add_rect(&mut s, None, 0.0, 40.0, 20.0);
    let b = add_rect(&mut s, None, 50.0, 40.0, 30.0);
    let c = add_rect(&mut s, None, 100.0, 40.0, 20.0);
    let f = id(&run(
        &mut s,
        "layout.addAutoLayout",
        json!({"ids": [c, a, b]}),
    ));
    let frame = s.snapshot().get(NodeId(f)).cloned().unwrap();
    assert_eq!(frame.children, vec![NodeId(a), NodeId(b), NodeId(c)]);
    let l = frame.layout.unwrap();
    assert_eq!(l.gap, 10.0);
    assert_eq!((frame.rect.width, frame.rect.height), (140.0, 30.0));
    assert_eq!(rect_of(&s, b).x, 50.0);
    run(&mut s, "node.move", json!({"ids": [b], "dx": 33, "dy": 0}));
    assert_eq!(rect_of(&s, b).x, 50.0);
    run(&mut s, "edit.undo", Value::Null);
    run(&mut s, "edit.undo", Value::Null);
    assert!(s.snapshot().get(NodeId(f)).is_none());
    assert_eq!(rect_of(&s, c).x, 100.0);
}

#[test]
fn vertical_stack_padding_fill_and_hug() {
    let mut s = Session::new().unwrap();
    let f = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 200, "height": 300}),
    ));
    let a = add_rect(&mut s, Some(f), 0.0, 50.0, 20.0);
    let b = add_rect(&mut s, Some(f), 0.0, 60.0, 30.0);
    run(
        &mut s,
        "layout.setAutoLayout",
        json!({"id": f, "direction": "vertical", "gap": 8, "padding": 16, "primarySizing": "hug"}),
    );
    assert_eq!(rect_of(&s, a), Rect::new(16.0, 16.0, 50.0, 20.0));
    assert_eq!(rect_of(&s, b), Rect::new(16.0, 44.0, 60.0, 30.0));
    assert_eq!(rect_of(&s, f).height, 16.0 + 20.0 + 8.0 + 30.0 + 16.0);
    run(
        &mut s,
        "layout.setChild",
        json!({"ids": [a], "horizontal": "fill"}),
    );
    assert_eq!(rect_of(&s, a).width, 200.0 - 32.0);
    run(
        &mut s,
        "layout.setAutoLayout",
        json!({"id": f, "counterAlign": "center"}),
    );
    assert_eq!(rect_of(&s, b).x, (200.0 - 60.0) / 2.0);
    run(
        &mut s,
        "layout.setChild",
        json!({"ids": [a], "absolute": true}),
    );
    assert_eq!(rect_of(&s, b).y, 16.0);
    assert!(s
        .dispatch("layout.setChild", &json!({"ids": [b], "horizontal": "hug"}))
        .is_err());
}

#[test]
fn text_fills_width_and_reflows() {
    let mut s = Session::new().unwrap();
    let f = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 120, "height": 100}),
    ));
    let t = id(&run(
        &mut s,
        "node.create.text",
        json!({"parent": f, "x": 0, "y": 0, "content": "a fairly long sentence that must wrap in a narrow column", "fontSize": 16}),
    ));
    let single_line = rect_of(&s, t).height;
    run(
        &mut s,
        "layout.setAutoLayout",
        json!({"id": f, "direction": "vertical", "padding": 10, "primarySizing": "hug"}),
    );
    run(
        &mut s,
        "layout.setChild",
        json!({"ids": [t], "horizontal": "fill"}),
    );
    let r = rect_of(&s, t);
    assert_eq!(r.width, 100.0);
    assert!(r.height > single_line, "{} vs {single_line}", r.height);
    assert_eq!(rect_of(&s, f).height, r.height + 20.0);
}

#[test]
fn nested_hug_frames_resolve_bottom_up() {
    let mut s = Session::new().unwrap();
    let outer = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 10, "height": 10}),
    ));
    let inner = id(&run(
        &mut s,
        "node.create.frame",
        json!({"parent": outer, "x": 0, "y": 0, "width": 10, "height": 10}),
    ));
    add_rect(&mut s, Some(inner), 0.0, 30.0, 30.0);
    add_rect(&mut s, Some(inner), 0.0, 30.0, 30.0);
    run(
        &mut s,
        "layout.setAutoLayout",
        json!({"id": inner, "direction": "horizontal", "gap": 10, "primarySizing": "hug", "counterSizing": "hug"}),
    );
    run(
        &mut s,
        "layout.setAutoLayout",
        json!({"id": outer, "direction": "vertical", "padding": 5, "primarySizing": "hug", "counterSizing": "hug"}),
    );
    assert_eq!(rect_of(&s, inner).width, 70.0);
    assert_eq!(rect_of(&s, outer), Rect::new(0.0, 0.0, 80.0, 40.0));
    s.snapshot().validate().unwrap();
}

#[test]
fn constraints_command() {
    let mut s = Session::new().unwrap();
    let f = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 100, "height": 100}),
    ));
    let r = add_rect(&mut s, Some(f), 10.0, 80.0, 10.0);
    run(
        &mut s,
        "node.setConstraints",
        json!({"ids": [r], "horizontal": "leftRight"}),
    );
    run(&mut s, "node.resize", json!({"id": f, "width": 200}));
    assert_eq!(rect_of(&s, r).width, 180.0);
}

#[test]
fn constraints_apply_in_frames_the_layout_resizes() {
    let mut s = Session::new().unwrap();
    let row = s
        .dispatch(
            "node.create.frame",
            &json!({"x": 0, "y": 0, "width": 300, "height": 100}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    s.dispatch(
        "layout.setAutoLayout",
        &json!({"id": row, "direction": "horizontal", "gap": 0, "padding": 0}),
    )
    .unwrap();
    let cell = s
        .dispatch(
            "node.create.frame",
            &json!({"parent": row, "x": 0, "y": 0, "width": 100, "height": 100}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let badge = s
        .dispatch(
            "node.create.rectangle",
            &json!({"parent": cell, "x": 80, "y": 10, "width": 10, "height": 10}),
        )
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    s.dispatch(
        "node.setConstraints",
        &json!({"ids": [badge], "horizontal": "right"}),
    )
    .unwrap();
    s.dispatch(
        "layout.setChild",
        &json!({"ids": [cell], "horizontal": "fill"}),
    )
    .unwrap();
    let doc = s.snapshot();
    let rect = |id: u64| doc.get(deesainer_engine::doc::NodeId(id)).unwrap().rect;
    assert_eq!(rect(cell).width, 300.0, "the cell fills the row");
    assert_eq!(rect(badge).x, 280.0);
}

#[test]
fn min_and_max_sizes_hold_in_auto_layout() {
    let mut s = Session::new().unwrap();
    let f = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 300, "height": 50}),
    ));
    let a = add_rect(&mut s, Some(f), 0.0, 10.0, 20.0);
    let b = add_rect(&mut s, Some(f), 0.0, 10.0, 20.0);
    run(
        &mut s,
        "layout.setAutoLayout",
        json!({"id": f, "direction": "horizontal", "gap": 0, "padding": 0}),
    );
    run(
        &mut s,
        "layout.setChild",
        json!({"ids": [a, b], "horizontal": "fill"}),
    );
    assert_eq!(rect_of(&s, a).width, 150.0);
    run(
        &mut s,
        "node.setProps",
        json!({"ids": [a], "maxWidth": 100}),
    );
    assert_eq!((rect_of(&s, a).width, rect_of(&s, b).width), (100.0, 200.0));
    run(
        &mut s,
        "node.setProps",
        json!({"ids": [a], "maxWidth": null}),
    );
    assert_eq!(rect_of(&s, a).width, 150.0);
    assert!(s
        .dispatch(
            "node.setProps",
            &json!({"ids": [a], "minWidth": 50, "maxWidth": 40})
        )
        .is_err());
    run(
        &mut s,
        "layout.setChild",
        json!({"ids": [a, b], "horizontal": "fixed"}),
    );
    run(
        &mut s,
        "layout.setAutoLayout",
        json!({"id": f, "primarySizing": "hug"}),
    );
    run(
        &mut s,
        "node.setProps",
        json!({"ids": [f], "maxWidth": 250}),
    );
    let total = rect_of(&s, a).width + rect_of(&s, b).width;
    assert!(total > 250.0, "{total}");
    assert_eq!(rect_of(&s, f).width, 250.0);
    let doc = s.snapshot();
    assert_eq!(
        doc.get(NodeId(f)).unwrap().size_limits.max_width,
        Some(250.0)
    );
}

#[test]
fn grid_auto_layout_places_children_in_rows_of_columns() {
    let mut s = Session::new().unwrap();
    let f = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 340, "height": 100}),
    ));
    let kids: Vec<u64> = (0..5)
        .map(|i| add_rect(&mut s, Some(f), 0.0, 40.0, 20.0 + i as f64 * 10.0))
        .collect();
    run(
        &mut s,
        "layout.setAutoLayout",
        json!({"id": f, "direction": "grid", "gridColumns": 3, "gap": 20, "counterGap": 10,
            "padding": 10, "counterSizing": "hug"}),
    );
    run(
        &mut s,
        "layout.setChild",
        json!({"ids": [kids[0]], "horizontal": "fill"}),
    );
    let col = (340.0 - 20.0 - 40.0) / 3.0;
    let a = rect_of(&s, kids[0]);
    assert!(
        (a.width - col).abs() < 1e-6 && a.x == 10.0 && a.y == 10.0,
        "{a:?}"
    );
    let b = rect_of(&s, kids[1]);
    assert!((b.x - (10.0 + col + 20.0)).abs() < 1e-6, "{b:?}");
    let d = rect_of(&s, kids[3]);
    assert_eq!((d.x, d.y), (10.0, 10.0 + 40.0 + 10.0));
    assert_eq!(rect_of(&s, f).height, 130.0);
    assert!(s
        .dispatch("layout.setAutoLayout", &json!({"id": f, "gridColumns": 0}))
        .is_err());
}

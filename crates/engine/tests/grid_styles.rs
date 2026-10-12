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

#[test]
fn grid_styles_are_shared_by_frames_and_follow_updates() {
    let mut s = Session::new().unwrap();
    let a = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 400, "height": 300}),
    ));
    let b = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 500, "y": 0, "width": 400, "height": 300}),
    ));
    run(
        &mut s,
        "node.setProps",
        json!({"ids": [a], "layoutGrids": [{"pattern": "columns", "count": 12, "gutter": 16, "offset": 24}]}),
    );
    let style = id(&run(
        &mut s,
        "style.create",
        json!({"name": "Desktop/12 columns", "kind": "grid", "from": a}),
    ));
    run(
        &mut s,
        "style.apply",
        json!({"ids": [a, b], "style": style}),
    );
    let doc = s.snapshot();
    assert_eq!(doc.get(NodeId(b)).unwrap().layout_grids[0].count, 12);
    drop(doc);
    run(
        &mut s,
        "style.update",
        json!({"id": style, "value": [{"pattern": "columns", "count": 8, "gutter": 16, "offset": 24}]}),
    );
    let doc = s.snapshot();
    assert_eq!(doc.get(NodeId(a)).unwrap().layout_grids[0].count, 8);
    assert_eq!(doc.get(NodeId(b)).unwrap().layout_grids[0].count, 8);
    drop(doc);
    run(
        &mut s,
        "style.detach",
        json!({"ids": [b], "target": "grid"}),
    );
    run(
        &mut s,
        "style.update",
        json!({"id": style, "value": [{"pattern": "rows", "count": 4}]}),
    );
    let doc = s.snapshot();
    assert_eq!(doc.get(NodeId(b)).unwrap().layout_grids[0].count, 8);
    assert_eq!(doc.get(NodeId(a)).unwrap().layout_grids[0].count, 4);
    assert!(s
        .dispatch(
            "style.apply",
            &json!({"ids": [a], "style": style, "target": "fill"})
        )
        .is_err());
}

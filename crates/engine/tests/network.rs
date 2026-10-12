#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::doc::{NodeId, NodeKind};
use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

fn square(s: &mut Session) -> u64 {
    run(
        s,
        "node.create.vector",
        json!({"cmds": [
            {"cmd": "moveTo", "to": {"x": 100, "y": 50}},
            {"cmd": "lineTo", "to": {"x": 200, "y": 50}},
            {"cmd": "lineTo", "to": {"x": 200, "y": 150}},
            {"cmd": "lineTo", "to": {"x": 100, "y": 150}},
            {"cmd": "close"},
        ]}),
    )["id"]
        .as_u64()
        .unwrap()
}

fn closes(s: &Session, id: u64) -> usize {
    match &s.snapshot().get(NodeId(id)).unwrap().kind {
        NodeKind::Vector { path } => path
            .cmds
            .iter()
            .filter(|c| matches!(c, deesainer_engine::doc::PathCmd::Close))
            .count(),
        _ => panic!("not a vector"),
    }
}

#[test]
fn connecting_vertices_splits_a_shape_into_regions() {
    let mut s = Session::new().unwrap();
    let id = square(&mut s);
    let net = run(&mut s, "vector.toNetwork", json!({"id": id}));
    assert_eq!(net["vertices"].as_array().unwrap().len(), 4);
    assert_eq!(net["regions"], 1);
    assert_eq!(closes(&s, id), 1);
    let c = run(&mut s, "vector.connect", json!({"id": id, "a": 0, "b": 2}));
    assert_eq!(c["segment"], 4);
    let net = run(&mut s, "vector.getNetwork", json!({"id": id}));
    assert_eq!(net["regions"], 2);
    assert_eq!(closes(&s, id), 2);
    let n = s.snapshot().get(NodeId(id)).unwrap().clone();
    assert_eq!(
        (n.rect.x, n.rect.y, n.rect.width, n.rect.height),
        (100.0, 50.0, 100.0, 100.0)
    );
    assert!(n.network.is_some());
    run(
        &mut s,
        "vector.moveVertex",
        json!({"id": id, "vertex": 2, "x": 150, "y": 100}),
    );
    let n = s.snapshot().get(NodeId(id)).unwrap().clone();
    assert_eq!((n.rect.width, n.rect.height), (150.0, 100.0));
    run(&mut s, "edit.undo", json!({}));
    let net = run(&mut s, "vector.getNetwork", json!({"id": id}));
    assert_eq!(net["vertices"][2]["x"], 100.0);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(closes(&s, id), 1);
    assert!(s
        .dispatch("vector.connect", &json!({"id": id, "a": 0, "b": 9}))
        .is_err());
    assert!(s
        .dispatch("vector.connect", &json!({"id": id, "a": 1, "b": 1}))
        .is_err());
}

#[test]
fn a_network_can_branch_and_survives_path_edits() {
    let mut s = Session::new().unwrap();
    let id = square(&mut s);
    run(&mut s, "vector.toNetwork", json!({"id": id}));
    let v = run(
        &mut s,
        "vector.addVertex",
        json!({"id": id, "x": 50, "y": 50}),
    );
    assert_eq!(v["vertex"], 4);
    run(&mut s, "vector.connect", json!({"id": id, "a": 1, "b": 4}));
    let net = run(&mut s, "vector.getNetwork", json!({"id": id}));
    assert_eq!(net["segments"].as_array().unwrap().len(), 5);
    assert_eq!(net["regions"], 1);
    run(
        &mut s,
        "vector.setTangents",
        json!({"id": id, "segment": 0, "ta": {"x": 0, "y": -30}, "tb": {"x": 0, "y": -30}}),
    );
    run(
        &mut s,
        "vector.setPath",
        json!({"id": id, "cmds": [
            {"cmd": "moveTo", "to": {"x": 0, "y": 0}},
            {"cmd": "lineTo", "to": {"x": 40, "y": 0}},
            {"cmd": "lineTo", "to": {"x": 40, "y": 40}},
            {"cmd": "close"},
        ]}),
    );
    assert!(s.snapshot().get(NodeId(id)).unwrap().network.is_none());
    let net = run(&mut s, "vector.getNetwork", json!({"id": id}));
    assert_eq!(net["vertices"].as_array().unwrap().len(), 3);
    run(
        &mut s,
        "vector.removeVertex",
        json!({"id": id, "vertex": 0}),
    );
    let net = run(&mut s, "vector.getNetwork", json!({"id": id}));
    assert_eq!(net["vertices"].as_array().unwrap().len(), 2);
    assert_eq!(net["regions"], 0);
}

#[test]
fn a_network_can_be_set_from_vertices_and_segments() {
    let mut s = Session::new().unwrap();
    let id = square(&mut s);
    let r = run(
        &mut s,
        "vector.setNetwork",
        json!({"id": id,
            "vertices": [{"x":0,"y":0},{"x":60,"y":0},{"x":30,"y":50},{"x":120,"y":50}],
            "segments": [{"a":0,"b":1},{"a":1,"b":2},{"a":2,"b":0},{"a":1,"b":3},{"a":3,"b":2}]}),
    );
    assert_eq!(r["regions"], 2);
    assert_eq!(closes(&s, id), 2);
    assert!(s
        .dispatch(
            "vector.setNetwork",
            &json!({"id": id, "vertices": [{"x":0,"y":0}], "segments": [{"a":0,"b":3}]})
        )
        .is_err());
    let doc_json = run(&mut s, "document.serialize", json!({}));
    let mut t = Session::new().unwrap();
    run(&mut t, "document.load", json!({"data": doc_json["data"]}));
    assert!(t.snapshot().get(NodeId(id)).unwrap().network.is_some());
}

#[test]
fn a_region_can_have_its_own_fill() {
    let mut s = Session::new().unwrap();
    let id = square(&mut s);
    run(&mut s, "vector.toNetwork", json!({"id": id}));
    run(&mut s, "vector.connect", json!({"id": id, "a": 0, "b": 2}));
    run(
        &mut s,
        "node.setProps",
        json!({"ids": [id], "fill": "#8AB4F8"}),
    );
    let before = run(&mut s, "vector.getNetwork", json!({"id": id}));
    assert_eq!(before["regionList"].as_array().unwrap().len(), 2);
    assert_eq!(before["regionList"][0]["ownFill"], false);
    let red = json!([{"type": "solid", "color": {"r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0}}]);
    let after = run(
        &mut s,
        "vector.setRegionFill",
        json!({"id": id, "region": 0, "fills": red}),
    );
    assert_eq!(after["regionList"][0]["ownFill"], true);
    assert_eq!(after["regionList"][1]["ownFill"], false);
    let svg = run(&mut s, "export.svg", json!({"id": id}));
    let svg = svg["svg"].as_str().unwrap().to_lowercase();
    assert!(svg.contains("#ff0000"), "{svg}");
    assert!(svg.contains("#8ab4f8"), "{svg}");
    run(
        &mut s,
        "vector.moveVertex",
        json!({"id": id, "vertex": 1, "x": 120, "y": 0}),
    );
    let moved = run(&mut s, "vector.getNetwork", json!({"id": id}));
    assert_eq!(moved["regionList"][0]["ownFill"], true);
    let reset = run(
        &mut s,
        "vector.setRegionFill",
        json!({"id": id, "region": 0, "fills": null}),
    );
    assert_eq!(reset["regionList"][0]["ownFill"], false);
    let svg = run(&mut s, "export.svg", json!({"id": id}));
    assert!(!svg["svg"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("#ff0000"));
    assert!(s
        .dispatch(
            "vector.setRegionFill",
            &json!({"id": id, "region": 9, "fills": red})
        )
        .is_err());
    run(
        &mut s,
        "vector.setRegionFill",
        json!({"id": id, "region": 1, "fills": red}),
    );
    let saved = run(&mut s, "document.serialize", json!({}));
    let mut t = Session::new().unwrap();
    run(&mut t, "document.load", json!({"data": saved["data"]}));
    let net = t
        .snapshot()
        .get(NodeId(id))
        .unwrap()
        .network
        .clone()
        .unwrap();
    assert_eq!(net.region_paints.len(), 1);
}

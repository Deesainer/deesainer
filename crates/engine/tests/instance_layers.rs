#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::doc::NodeId;
use deesainer_engine::Session;
use deesainer_geom::Point;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

#[test]
fn the_layer_under_a_point_of_an_instance_is_found_and_overridden() {
    let mut s = Session::new().unwrap();
    let card = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 200, "height": 100}),
    ));
    let icon = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"parent": card, "x": 10, "y": 10, "width": 30, "height": 30, "fill": "#FF0000"}),
    ));
    let label = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"parent": card, "x": 60, "y": 10, "width": 100, "height": 20, "fill": "#00FF00"}),
    ));
    let main = id(&run(&mut s, "component.create", json!({"ids": [card]})));
    let inst = id(&run(
        &mut s,
        "component.instantiate",
        json!({"component": main, "x": 500, "y": 300}),
    ));
    let doc = s.snapshot();
    let instance = doc.get(NodeId(inst)).unwrap();
    let at = |x: f64, y: f64| deesainer_engine::slots::layer_at(&doc, instance, Point::new(x, y));
    assert_eq!(at(520.0, 320.0), Some(NodeId(icon)));
    assert_eq!(at(600.0, 315.0), Some(NodeId(label)));
    assert_eq!(at(600.0, 380.0), None);
    let outline = deesainer_engine::slots::layer_outline(&doc, instance, NodeId(label)).unwrap();
    assert_eq!(outline[0], Point::new(560.0, 310.0));
    assert_eq!(outline[2], Point::new(660.0, 330.0));
    drop(doc);
    run(
        &mut s,
        "instance.setOverride",
        json!({"id": inst, "target": label, "prop": "fill", "value": "#0000FF"}),
    );
    let doc = s.snapshot();
    let (_, _, resolved) =
        deesainer_engine::slots::slot_box(&doc, doc.get(NodeId(inst)).unwrap(), NodeId(label))
            .unwrap();
    assert_eq!(
        resolved.fills[0].kind,
        deesainer_doc::PaintKind::Solid {
            color: deesainer_color::Rgba::from_hex("#0000FF").unwrap()
        }
    );
    assert_ne!(doc.get(NodeId(label)).unwrap().fills, resolved.fills);
}

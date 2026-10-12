#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::doc::{Document, NodeId, NodeKind};
use deesainer_engine::Session;
use deesainer_geom::{Affine, Rect};
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

fn rgb(doc: &Document, page: NodeId, x: u32, y: u32) -> [u8; 3] {
    let vp = deesainer_render::Viewport {
        transform: Affine::IDENTITY,
        width: 800,
        height: 400,
    };
    let opts = deesainer_render::Options {
        background: true,
        text: None,
        hidden: &[],
    };
    let (pix, _) = deesainer_render::render_page(doc, page, &vp, opts).unwrap();
    let [r, g, b, _] = deesainer_render::pixel_rgba(&pix, x, y).unwrap();
    [r, g, b]
}

fn card(s: &mut Session) -> (u64, u64, u64, u64) {
    let card = id(&run(
        s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 200, "height": 120, "name": "Card", "fill": "#FFFFFF"}),
    ));
    run(s, "component.create", json!({"ids": [card]}));
    let slot = run(
        s,
        "component.addSlot",
        json!({"id": card, "name": "Body", "x": 20, "y": 40, "width": 160, "height": 60}),
    );
    assert_eq!(slot["property"], "Body");
    let frame = slot["frame"].as_u64().unwrap();
    run(
        s,
        "node.create.rectangle",
        json!({"parent": frame, "x": 0, "y": 0, "width": 160, "height": 60, "fill": "#888888"}),
    );
    let a = id(&run(
        s,
        "component.instantiate",
        json!({"component": card, "x": 300, "y": 0}),
    ));
    let b = id(&run(
        s,
        "component.instantiate",
        json!({"component": card, "x": 550, "y": 0}),
    ));
    (card, frame, a, b)
}

#[test]
fn an_instance_holds_its_own_layers_in_a_slot() {
    let mut s = Session::new().unwrap();
    let (_, _, a, b) = card(&mut s);
    let page = s.current_page();
    let badge = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 40, "y": 300, "width": 50, "height": 30, "fill": "#E53935"}),
    ));
    let out = run(
        &mut s,
        "instance.fillSlot",
        json!({"id": a, "name": "Body", "ids": [badge]}),
    );
    let content = out["content"].as_u64().unwrap();
    let doc = s.snapshot();
    let holder = doc.get(NodeId(content)).unwrap();
    assert!(matches!(&holder.kind, NodeKind::SlotContent { slot } if slot == "Body"));
    assert_eq!(holder.parent, Some(NodeId(a)));
    assert_eq!(holder.rect, Rect::new(20.0, 40.0, 160.0, 60.0));
    let moved = doc.get(NodeId(badge)).unwrap();
    assert_eq!(moved.parent, Some(NodeId(content)));
    assert_eq!((moved.rect.x, moved.rect.y), (0.0, 0.0));
    assert_eq!(
        doc.nodes()
            .filter(|n| matches!(n.kind, NodeKind::Component { .. }))
            .count(),
        1,
        "filling a slot makes no component"
    );
    assert_eq!(rgb(&doc, page, 300 + 20 + 10, 40 + 10), [0xE5, 0x39, 0x35]);
    assert_eq!(
        rgb(&doc, page, 300 + 20 + 100, 40 + 10),
        [0xFF, 0xFF, 0xFF],
        "the default content is gone"
    );
    assert_eq!(rgb(&doc, page, 550 + 20 + 10, 40 + 10), [0x88, 0x88, 0x88]);
    run(&mut s, "node.setPosition", json!({"id": badge, "x": 100}));
    assert_eq!(
        rgb(&s.snapshot(), page, 300 + 20 + 110, 40 + 10),
        [0xE5, 0x39, 0x35]
    );
    assert!(s
        .dispatch(
            "instance.fillSlot",
            &json!({"id": a, "name": "Nope", "ids": [badge]})
        )
        .is_err());
    assert!(s
        .dispatch(
            "instance.fillSlot",
            &json!({"id": a, "name": "Body", "ids": [a]})
        )
        .is_err());
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    let doc = s.snapshot();
    assert_eq!(doc.get(NodeId(badge)).unwrap().parent, Some(page));
    assert!(doc.get(NodeId(content)).is_none());
    run(&mut s, "edit.redo", json!({}));
    run(
        &mut s,
        "instance.clearSlot",
        json!({"id": a, "name": "Body"}),
    );
    let doc = s.snapshot();
    assert!(doc.get(NodeId(content)).is_none());
    assert_eq!(rgb(&doc, page, 300 + 20 + 10, 40 + 10), [0x88, 0x88, 0x88]);
    let _ = b;
}

#[test]
fn the_slot_frame_sets_the_layout_and_follows_resizing() {
    let mut s = Session::new().unwrap();
    let (_, frame, a, _) = card(&mut s);
    run(
        &mut s,
        "node.setConstraints",
        json!({"ids": [frame], "horizontal": "leftRight"}),
    );
    run(
        &mut s,
        "layout.setAutoLayout",
        json!({"id": frame, "direction": "horizontal", "gap": 10, "padding": 5}),
    );
    let x = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 0, "y": 300, "width": 30, "height": 20}),
    ));
    let y = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 100, "y": 300, "width": 30, "height": 20}),
    ));
    let out = run(
        &mut s,
        "instance.fillSlot",
        json!({"id": a, "name": "Body", "ids": [x, y], "copy": true}),
    );
    let content = out["content"].as_u64().unwrap();
    let layers: Vec<u64> = out["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    let doc = s.snapshot();
    assert!(doc.get(NodeId(x)).unwrap().parent == Some(s.current_page()));
    let rect = |doc: &Document, id: u64| doc.get(NodeId(id)).unwrap().rect;
    assert_eq!(
        (rect(&doc, layers[0]).x, rect(&doc, layers[1]).x),
        (5.0, 45.0),
        "laid out by the slot's auto layout"
    );
    run(&mut s, "node.resize", json!({"id": a, "width": 300}));
    let doc = s.snapshot();
    assert_eq!(rect(&doc, content).width, 260.0);
}

#[test]
fn components_go_into_slots_and_detach_keeps_the_content() {
    let mut s = Session::new().unwrap();
    let (card_id, _, a, _) = card(&mut s);
    let icon = id(&run(
        &mut s,
        "node.create.ellipse",
        json!({"x": 0, "y": 300, "width": 24, "height": 24, "name": "Icon", "fill": "#1E88E5"}),
    ));
    let icon = id(&run(&mut s, "component.create", json!({"ids": [icon]})));
    run(
        &mut s,
        "component.setSlotPreferred",
        json!({"id": card_id, "name": "Body", "components": [icon]}),
    );
    assert!(s
        .dispatch(
            "component.setSlotPreferred",
            &json!({"id": card_id, "name": "Body", "components": [card_id]})
        )
        .is_err());
    let out = run(
        &mut s,
        "instance.fillSlot",
        json!({"id": a, "name": "Body", "component": icon}),
    );
    let inserted = out["layers"][0].as_u64().unwrap();
    let doc = s.snapshot();
    assert!(
        matches!(doc.get(NodeId(inserted)).unwrap().kind, NodeKind::Instance { component, .. } if component == NodeId(icon))
    );
    assert!(s
        .dispatch(
            "instance.fillSlot",
            &json!({"id": a, "name": "Body", "component": card_id})
        )
        .is_err());
    let detached = run(&mut s, "component.detach", json!({"ids": [a]}));
    let root = detached["ids"][0].as_u64().unwrap();
    let doc = s.snapshot();
    let frame = doc.get(NodeId(root)).unwrap();
    let body = frame
        .children
        .iter()
        .filter_map(|c| doc.get(*c))
        .find(|c| c.name == "Body")
        .unwrap();
    assert_eq!(
        body.children,
        vec![NodeId(inserted)],
        "the default content is replaced"
    );
    assert!(doc
        .nodes()
        .all(|n| !matches!(n.kind, NodeKind::SlotContent { .. })));
}

#[test]
fn content_follows_variants_with_the_same_slot() {
    let mut s = Session::new().unwrap();
    let make = |s: &mut Session, name: &str, x: f64, fill: &str| {
        let c = id(&run(
            s,
            "node.create.frame",
            json!({"x": x, "y": 0, "width": 200, "height": 120, "name": name, "fill": fill}),
        ));
        run(s, "component.create", json!({"ids": [c]}));
        run(
            s,
            "component.addSlot",
            json!({"id": c, "name": "Body", "x": 20, "y": 40, "width": 160, "height": 60}),
        );
        c
    };
    let small = make(&mut s, "Size=Small", 0.0, "#FFFFFF");
    let large = make(&mut s, "Size=Large", 250.0, "#EEEEEE");
    run(
        &mut s,
        "component.combineAsVariants",
        json!({"ids": [small, large]}),
    );
    let inst = id(&run(
        &mut s,
        "component.instantiate",
        json!({"component": small, "x": 0, "y": 200}),
    ));
    let dot = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 0, "y": 500, "width": 10, "height": 10}),
    ));
    run(
        &mut s,
        "instance.fillSlot",
        json!({"id": inst, "name": "Body", "ids": [dot]}),
    );
    run(
        &mut s,
        "instance.setVariant",
        json!({"ids": [inst], "properties": {"Size": "Large"}}),
    );
    let doc = s.snapshot();
    let NodeKind::Instance { component, .. } = doc.get(NodeId(inst)).unwrap().kind else {
        panic!()
    };
    assert_eq!(component, NodeId(large));
    assert_eq!(doc.slot_fills(NodeId(inst)).len(), 1);
    assert!(doc.is_ancestor(NodeId(inst), NodeId(dot)).unwrap());
}

#[test]
fn slots_from_before_hold_layers_too() {
    let mut s = Session::new().unwrap();
    let card = id(&run(
        &mut s,
        "node.create.frame",
        json!({"x": 0, "y": 0, "width": 200, "height": 120, "fill": "#FFFFFF"}),
    ));
    run(&mut s, "component.create", json!({"ids": [card]}));
    let holder = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 400, "y": 0, "width": 50, "height": 50, "fill": "#888888"}),
    ));
    let holder = id(&run(&mut s, "component.create", json!({"ids": [holder]})));
    let nested = id(&run(
        &mut s,
        "component.instantiate",
        json!({"component": holder, "x": 10, "y": 10, "parent": card}),
    ));
    run(
        &mut s,
        "component.addProperty",
        json!({"id": card, "name": "Icon", "kind": "instanceSwap", "target": nested}),
    );
    let a = id(&run(
        &mut s,
        "component.instantiate",
        json!({"component": card, "x": 0, "y": 200}),
    ));
    let red = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 0, "y": 500, "width": 20, "height": 20, "fill": "#E53935"}),
    ));
    run(
        &mut s,
        "instance.fillSlot",
        json!({"id": a, "name": "Icon", "ids": [red]}),
    );
    let doc = s.snapshot();
    let page = s.current_page();
    assert_eq!(rgb(&doc, page, 15, 215), [0xE5, 0x39, 0x35]);
    assert_eq!(
        rgb(&doc, page, 45, 245),
        [0xFF, 0xFF, 0xFF],
        "the placeholder is replaced"
    );
}

#[test]
fn deep_clicks_reach_slot_content() {
    let mut s = Session::new().unwrap();
    let (_, _, a, _) = card(&mut s);
    let page = s.current_page();
    let badge = id(&run(
        &mut s,
        "node.create.rectangle",
        json!({"x": 40, "y": 300, "width": 50, "height": 30, "fill": "#E53935"}),
    ));
    run(
        &mut s,
        "instance.fillSlot",
        json!({"id": a, "name": "Body", "ids": [badge]}),
    );
    let doc = s.snapshot();
    let p = deesainer_geom::Point::new(300.0 + 20.0 + 10.0, 40.0 + 10.0);
    assert_eq!(
        deesainer_vector::hit_test(&doc, page, p, 0.0, false),
        Some(NodeId(a))
    );
    assert_eq!(
        deesainer_vector::hit_test(&doc, page, p, 0.0, true),
        Some(NodeId(badge))
    );
    let empty = deesainer_geom::Point::new(300.0 + 20.0 + 120.0, 40.0 + 40.0);
    assert_eq!(
        deesainer_vector::hit_test(&doc, page, empty, 0.0, true),
        Some(NodeId(a))
    );
}

#[test]
fn double_clicking_into_an_instance_reaches_the_default_slot_content() {
    let mut s = Session::new().unwrap();
    let (_, frame, _, b) = card(&mut s);
    let doc = s.snapshot();
    let grey = doc.get(NodeId(frame)).unwrap().children[0];
    let inst = doc.get(NodeId(b)).unwrap();
    let p = deesainer_geom::Point::new(550.0 + 20.0 + 10.0, 40.0 + 10.0);
    assert_eq!(deesainer_engine::slots::layer_at(&doc, inst, p), Some(grey));
    let outline = deesainer_engine::slots::layer_outline(&doc, inst, grey).unwrap();
    assert_eq!((outline[0].x, outline[0].y), (570.0, 40.0));
}
